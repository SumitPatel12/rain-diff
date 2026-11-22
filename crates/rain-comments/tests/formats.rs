use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rain_comments::{
    Autosave, SessionFile, anchor_label, apply, autosave, comments_json, comments_json_string, comments_output, from_difit_json, import,
    ops, post_threads,
};
use rain_core::{
    Anchor, CodeSnapshot, DiffSet, Disposition, FileDiffBuilder, FileStatus, LineKind, RepoInfo, Session, SessionRepo, Shared, Side,
};

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/difit").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn golden_session() -> Session {
    let mut s = Session::default();
    let imports = from_difit_json(&fixture("import.json")).unwrap();
    let r = apply(&mut s, imports, "");
    assert_eq!(r.count, 5);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    s
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir()
        .join("rain-comments-tests")
        .join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn comments_output_is_byte_equal_to_difit() {
    let s = golden_session();
    assert_eq!(comments_output(&s, None), fixture("comments-output.golden.txt"));
    assert_eq!(comments_output(&Session::default(), None), "");
}

#[test]
fn comments_json_is_byte_equal_to_difit() {
    let s = golden_session();
    let golden = fixture("comments-json.golden.json");
    assert_eq!(comments_json_string(&s, None), golden.trim_end());
    let v: serde_json::Value = serde_json::from_str(&golden).unwrap();
    assert_eq!(comments_json(&s, None), v);
}

#[test]
fn repeated_import_changes_nothing() {
    let mut s = golden_session();
    let before = s.clone();
    let r = apply(&mut s, from_difit_json(&fixture("import.json")).unwrap(), "");
    assert!(!r.changed);
    assert_eq!(s, before);
}

#[test]
fn import_dedupes_by_file_side_line_body() {
    let mut s = Session::default();
    let one = r#"{"type":"thread","filePath":"a.cs","position":{"side":"new","line":3},"body":"  Fix this.  ","author":"claude"}"#;
    apply(&mut s, from_difit_json(one).unwrap(), "");
    let other_author = r#"{"type":"thread","filePath":"a.cs","position":{"side":"new","line":3},"body":"Fix this.","author":"someone"}"#;
    let r = apply(&mut s, from_difit_json(other_author).unwrap(), "");
    assert!(!r.changed, "same file|side|line|body is a duplicate whoever wrote it");
    let old_side = r#"{"type":"thread","filePath":"a.cs","position":{"side":"old","line":3},"body":"Fix this."}"#;
    assert!(apply(&mut s, from_difit_json(old_side).unwrap(), "").changed);
    let range = r#"{"type":"thread","filePath":"a.cs","position":{"side":"new","line":{"start":3,"end":4}},"body":"Fix this."}"#;
    assert!(apply(&mut s, from_difit_json(range).unwrap(), "").changed);
    assert!(
        apply(&mut s, from_difit_json(one).unwrap(), "server").changed,
        "another repo's thread is not a duplicate"
    );
    assert_eq!(s.threads.len(), 4);
    assert_eq!(s.threads[0].messages[0].body, "Fix this.");
    assert_eq!(s.threads[3].repo, "server");
}

fn with_repos(ids: &[&str]) -> Session {
    Session {
        repos: ids
            .iter()
            .map(|id| SessionRepo {
                id: (*id).into(),
                ..SessionRepo::default()
            })
            .collect(),
        ..Session::default()
    }
}

fn thread_at(path: &str, body: &str) -> String {
    format!(r#"{{"type":"thread","filePath":"{path}","position":{{"side":"new","line":10}},"body":"{body}"}}"#)
}

fn reply_at(path: &str, body: &str) -> String {
    format!(r#"{{"type":"reply","filePath":"{path}","position":{{"side":"new","line":10}},"body":"{body}","author":"claude"}}"#)
}

#[test]
fn a_reply_by_file_reaches_a_single_repo_thread_in_every_form() {
    for stored in ["shop", ""] {
        let mut s = with_repos(&["shop"]);
        apply(&mut s, from_difit_json(&thread_at("api/Tips.cs", "Why?")).unwrap(), stored);
        assert_eq!(
            (s.threads[0].repo.as_str(), s.threads[0].file_path.as_str()),
            (stored, "api/Tips.cs")
        );
        for (n, (path, repo)) in [("shop/api/Tips.cs", ""), ("api/Tips.cs", "shop"), ("api/Tips.cs", "")]
            .into_iter()
            .enumerate()
        {
            let r = apply(&mut s, from_difit_json(&reply_at(path, &format!("r{n}"))).unwrap(), repo);
            assert!(r.warnings.is_empty(), "{path} in {repo:?}, thread in {stored:?}: {:?}", r.warnings);
            assert!(r.changed);
            assert_eq!(s.threads[0].messages.len(), n + 2);
        }
    }
}

#[test]
fn a_reply_by_file_reaches_a_multi_repo_thread_by_prefix_or_repo() {
    let mut s = with_repos(&["server", "app"]);
    apply(&mut s, from_difit_json(&thread_at("api/Tips.cs", "Why?")).unwrap(), "server");
    for (n, (path, repo)) in [
        ("server/api/Tips.cs", ""),
        ("api/Tips.cs", "server"),
        ("server/api/Tips.cs", "server"),
    ]
    .into_iter()
    .enumerate()
    {
        let r = apply(&mut s, from_difit_json(&reply_at(path, &format!("r{n}"))).unwrap(), repo);
        assert!(r.warnings.is_empty(), "{path} in {repo:?}: {:?}", r.warnings);
        assert_eq!(s.threads[0].messages.len(), n + 2);
    }
    let other = apply(&mut s, from_difit_json(&reply_at("api/Tips.cs", "x")).unwrap(), "app");
    assert_eq!(
        other.warnings,
        ["Skipped reply import for api/Tips.cs:new:10 because no matching thread was found."]
    );
    let bare = apply(&mut s, from_difit_json(&reply_at("api/Tips.cs", "x")).unwrap(), "");
    assert_eq!(bare.warnings.len(), 1, "a bare path names no repo among several");
    let wrong = apply(&mut s, from_difit_json(&reply_at("app/api/Tips.cs", "x")).unwrap(), "");
    assert_eq!(
        wrong.warnings,
        ["Skipped reply import for api/Tips.cs:new:10 because no matching thread was found."]
    );
}

#[test]
fn a_repo_prefixed_thread_lands_in_that_repo_and_dedupes_against_either_form() {
    let mut s = with_repos(&["shop"]);
    apply(&mut s, from_difit_json(&thread_at("shop/api/Tips.cs", "Why?")).unwrap(), "");
    assert_eq!(
        (s.threads[0].repo.as_str(), s.threads[0].file_path.as_str()),
        ("shop", "api/Tips.cs")
    );
    for (path, repo) in [("shop/api/Tips.cs", ""), ("api/Tips.cs", "shop"), ("api/Tips.cs", "")] {
        let r = apply(&mut s, from_difit_json(&thread_at(path, "Why?")).unwrap(), repo);
        assert!(!r.changed, "{path} in {repo:?} is the same thread");
    }
    assert_eq!(s.threads.len(), 1);

    let mut bare = with_repos(&["shop"]);
    apply(&mut bare, from_difit_json(&thread_at("api/Tips.cs", "Why?")).unwrap(), "");
    assert!(!apply(&mut bare, from_difit_json(&thread_at("shop/api/Tips.cs", "Why?")).unwrap(), "").changed);
    assert!(!apply(&mut bare, from_difit_json(&thread_at("api/Tips.cs", "Why?")).unwrap(), "shop").changed);

    let mut many = with_repos(&["server", "app"]);
    apply(&mut many, from_difit_json(&thread_at("server/a.cs", "Why?")).unwrap(), "");
    assert!(!apply(&mut many, from_difit_json(&thread_at("a.cs", "Why?")).unwrap(), "server").changed);
    assert!(apply(&mut many, from_difit_json(&thread_at("a.cs", "Why?")).unwrap(), "app").changed);
    assert!(
        apply(&mut many, from_difit_json(&thread_at("server/a.cs", "Why?")).unwrap(), "app").changed,
        "a named repo keeps the path as written"
    );
    assert_eq!(
        (many.threads[2].repo.as_str(), many.threads[2].file_path.as_str()),
        ("app", "server/a.cs")
    );
}

#[test]
fn a_folder_named_like_the_repo_is_found_as_written_first() {
    let mut s = with_repos(&["api"]);
    ops::add_thread(
        &mut s,
        "",
        "api/Tips.cs",
        Anchor::Line { side: Side::New, line: 10 },
        "In the api folder.",
        None,
        None,
    );
    ops::add_thread(
        &mut s,
        "",
        "Tips.cs",
        Anchor::Line { side: Side::New, line: 10 },
        "At the root.",
        None,
        None,
    );
    let r = apply(&mut s, from_difit_json(&reply_at("api/Tips.cs", "ok")).unwrap(), "");
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!((s.threads[0].messages.len(), s.threads[1].messages.len()), (2, 1));
    let r = apply(&mut s, from_difit_json(&reply_at("api/api/Tips.cs", "again")).unwrap(), "");
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(s.threads[0].messages.len(), 3);
}

#[test]
fn reply_attaches_to_the_newest_matching_thread_or_warns() {
    let mut s = Session::default();
    let t = r#"[{"type":"thread","filePath":"a.cs","position":{"side":"new","line":3},"body":"first","createdAt":"2026-01-01T00:00:00.000Z"},
                {"type":"thread","filePath":"a.cs","position":{"side":"new","line":3},"body":"second","createdAt":"2026-01-02T00:00:00.000Z"}]"#;
    apply(&mut s, from_difit_json(t).unwrap(), "");
    let reply = r#"{"type":"reply","filePath":"a.cs","position":{"side":"new","line":3},"body":"ok","author":"claude","createdAt":"2026-01-03T00:00:00.000Z"}"#;
    let r = apply(&mut s, from_difit_json(reply).unwrap(), "");
    assert!(r.changed);
    assert_eq!(s.threads[1].messages.len(), 2);
    assert_eq!(s.threads[1].updated_at, "2026-01-03T00:00:00.000Z");
    let r = apply(&mut s, from_difit_json(reply).unwrap(), "");
    assert!(!r.changed, "the same reply twice lands once");
    let orphan = r#"{"type":"reply","filePath":"b.cs","position":{"side":"old","line":{"start":1,"end":2}},"body":"?"}"#;
    let r = apply(&mut s, from_difit_json(orphan).unwrap(), "");
    assert_eq!(
        r.warnings,
        ["Skipped reply import for b.cs:old:1-2 because no matching thread was found."]
    );
}

#[test]
fn import_validation_matches_difit_errors() {
    let cases = [
        ("not json", "Invalid --comment JSON"),
        ("[1]", "Comment import must be an object"),
        (r#"{"type":"x"}"#, "Invalid comment import field: type"),
        (r#"{"type":"thread","filePath":" "}"#, "Invalid comment import field: filePath"),
        (
            r#"{"type":"thread","filePath":"a","position":{"side":"mid","line":1}}"#,
            "Invalid comment import field: position.side",
        ),
        (
            r#"{"type":"thread","filePath":"a","position":{"side":"new","line":0}}"#,
            "Invalid comment import field: position.line",
        ),
        (
            r#"{"type":"thread","filePath":"a","position":{"side":"new","line":{"start":5,"end":2}}}"#,
            "Invalid comment import field: position.line",
        ),
        (
            r#"{"type":"thread","filePath":"a","position":{"side":"new","line":1},"body":"  "}"#,
            "Invalid comment import field: body",
        ),
    ];
    for (json, want) in cases {
        assert_eq!(from_difit_json(json).unwrap_err(), want, "{json}");
    }
    let span = r#"{"type":"thread","filePath":"a","position":{"side":"new","line":{"start":2,"end":3}},"span":{"startLine":2,"startCol":4,"endLine":3,"endCol":1},"body":"x"}"#;
    let imp = from_difit_json(span).unwrap();
    assert!(matches!(imp[0].body().anchor, Some(Anchor::Span { start_col: 4, .. })));
    for bad in [
        r#"{"startLine":0,"startCol":0,"endLine":1,"endCol":0}"#,
        r#"{"startLine":3,"startCol":0,"endLine":2,"endCol":0}"#,
        r#"{"startLine":2,"startCol":5,"endLine":2,"endCol":1}"#,
        r#"{"startLine":2,"startCol":4294967296,"endLine":3,"endCol":1}"#,
        r#"{"startLine":2}"#,
    ] {
        let json = format!(r#"{{"type":"thread","filePath":"a","position":{{"side":"new","line":2}},"span":{bad},"body":"x"}}"#);
        assert_eq!(from_difit_json(&json).unwrap_err(), "Invalid comment import field: span", "{bad}");
    }
}

#[test]
fn span_labels_never_overflow() {
    let span = Anchor::Span {
        side: Side::New,
        start_line: 1,
        start_col: u32::MAX,
        end_line: 1,
        end_col: u32::MAX,
    };
    assert_eq!(anchor_label("a", &span), "a:L1C4294967296-L1C4294967296");
}

#[test]
fn span_and_disposition_lines_are_additive() {
    let mut s = Session::default();
    let span = Anchor::Span {
        side: Side::New,
        start_line: 4,
        start_col: 2,
        end_line: 4,
        end_col: 9,
    };
    let snap = CodeSnapshot {
        content: "total =".into(),
        ..CodeSnapshot::default()
    };
    let id = ops::add_thread(&mut s, "server", "src/a.ts", span, "Rename this.", Some("reviewer"), Some(snap));
    ops::set_disposition(&mut s, &id, Some(Disposition::Ask));
    let out = comments_output(&s, Some("server"));
    assert!(
        out.contains("src/a.ts:L4-L4\nDisposition: ask\nSelection: total =\nRename this.\n"),
        "{out}"
    );
    assert!(comments_output(&s, None).contains("server/src/a.ts:L4-L4"));
    assert_eq!(comments_output(&s, Some("app")), "");
    let v = comments_json(&s, Some("server"));
    assert_eq!(v["threads"][0]["position"]["line"]["start"], 4);
    assert_eq!(v["threads"][0]["span"]["endCol"], 9);
    assert_eq!(v["threads"][0]["disposition"], "ask");
}

#[test]
fn a_repo_filter_reads_back_every_thread_of_a_one_repo_session() {
    let mut s = with_repos(&["shop"]);
    apply(&mut s, from_difit_json(&thread_at("a.cs", "made in the window")).unwrap(), "");
    apply(&mut s, from_difit_json(&thread_at("b.cs", "made with a repo")).unwrap(), "shop");
    assert_eq!(comments_json(&s, Some("shop"))["threads"].as_array().unwrap().len(), 2);
    assert!(comments_output(&s, Some("shop")).contains("Total comments: 2"));
    assert_eq!(comments_output(&s, Some("other")), "");

    let mut many = with_repos(&["server", "app"]);
    apply(&mut many, from_difit_json(&thread_at("a.cs", "no repo")).unwrap(), "");
    apply(&mut many, from_difit_json(&thread_at("b.cs", "in server")).unwrap(), "server");
    assert_eq!(comments_json(&many, Some("server"))["threads"].as_array().unwrap().len(), 1);
}

#[test]
fn store_round_trips_every_kind_of_state() {
    let dir = scratch("store");
    let path = SessionFile::path_in(&dir, "ACME-1234 / round 2");
    assert!(path.ends_with("ACME-1234___round_2.json"), "{}", path.display());
    assert_eq!(SessionFile::load(&path).unwrap(), Session::default());

    let mut s = golden_session();
    s.id = "ACME-1234".into();
    let line = ops::add_thread(&mut s, "", "a.cs", Anchor::Line { side: Side::New, line: 1 }, "line", None, None);
    let range = ops::add_thread(
        &mut s,
        "",
        "a.cs",
        Anchor::Range {
            side: Side::Old,
            start: 2,
            end: 5,
        },
        "range",
        None,
        None,
    );
    let span = Anchor::Span {
        side: Side::New,
        start_line: 3,
        start_col: 1,
        end_line: 4,
        end_col: 6,
    };
    ops::add_thread(&mut s, "app", "b.ts", span, "span", Some("claude"), None);
    ops::reply(&mut s, &line, "reply", Some("claude")).unwrap();
    ops::set_disposition(&mut s, &range, Some(Disposition::Defer));
    ops::set_viewed(&mut s, "", "a.cs", "h1");
    ops::set_draft(&mut s, "", "a.cs", Anchor::Line { side: Side::New, line: 9 }, "half a thought");
    SessionFile::save(&path, &s).unwrap();
    assert_eq!(SessionFile::load(&path).unwrap(), s);
    let leftovers = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|x| x == "tmp"))
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn an_older_snapshot_never_lands_after_a_newer_one() {
    let dir = scratch("snapshots");
    let path = dir.join("s.json");
    let mut s = Session::default();
    let older = SessionFile::snapshot(&path, &s).unwrap();
    s.version = 7;
    let newer = SessionFile::snapshot(&path, &s).unwrap();
    newer.write().unwrap();
    older.write().unwrap();
    assert_eq!(SessionFile::load(&path).unwrap().version, 7, "the older snapshot is dropped");
    s.version = 8;
    SessionFile::snapshot(&path, &s).unwrap().write().unwrap();
    assert_eq!(SessionFile::load(&path).unwrap().version, 8, "a later one still writes");
}

#[test]
fn autosave_writes_after_the_debounce_and_on_drop() {
    let dir = scratch("autosave");
    let path = dir.join("s.json");
    let shared = Arc::new(Shared::new(Session::default()));
    let saver = autosave(shared.clone(), path.clone(), Duration::from_millis(30));
    ops::add_thread(
        &mut shared.lock(),
        "",
        "a",
        Anchor::Line { side: Side::New, line: 1 },
        "x",
        None,
        None,
    );
    saver.touch();
    saver.flush();
    assert_eq!(SessionFile::load(&path).unwrap().threads.len(), 1);
    let id = shared.lock().threads[0].id.clone();
    ops::reply(&mut shared.lock(), &id, "y", None);
    saver.touch();
    drop(saver);
    assert_eq!(SessionFile::load(&path).unwrap().threads[0].messages.len(), 2);
}

#[test]
fn autosave_saves_during_a_steady_stream_of_changes() {
    let dir = scratch("autosave-steady");
    let path = dir.join("s.json");
    let shared = Arc::new(Shared::new(Session::default()));
    let saver = autosave(shared.clone(), path.clone(), Duration::from_millis(60));
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_millis(900) && !path.exists() {
        ops::set_viewed(&mut shared.lock(), "", "a", &format!("{:?}", start.elapsed()));
        saver.touch();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(path.exists(), "touches every 20 ms must not postpone the save forever");
    drop(saver);
}

#[test]
fn a_place_touch_waits_to_ride_along_but_still_saves_and_flushes() {
    let dir = scratch("autosave-later");
    let path = dir.join("s.json");
    let shared = Arc::new(Shared::new(Session::default()));
    let saver = autosave(shared.clone(), path.clone(), Duration::from_millis(30));
    ops::set_viewed(&mut shared.lock(), "", "a", "h1");
    saver.touch_later();
    std::thread::sleep(Duration::from_millis(300));
    assert!(!path.exists(), "a place-only change waits longer than the debounce");
    let start = std::time::Instant::now();
    while start.elapsed() < Autosave::LATER * 2 && !path.exists() {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(path.exists(), "it saves within Autosave::LATER");
    std::fs::remove_file(&path).unwrap();
    ops::set_viewed(&mut shared.lock(), "", "b", "h2");
    saver.touch_later();
    drop(saver);
    assert_eq!(SessionFile::load(&path).unwrap().viewed.len(), 2, "closing saves it at once");
}

#[test]
fn the_session_file_is_compact_json() {
    let dir = scratch("compact");
    let path = dir.join("s.json");
    let mut s = Session::default();
    ops::set_viewed(&mut s, "", "a", "h1");
    SessionFile::save(&path, &s).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains('\n'), "{text}");
    assert_eq!(SessionFile::load(&path).unwrap().viewed.len(), 1);
}

#[test]
fn a_thread_waits_on_whoever_did_not_speak_last() {
    use ops::Owed;
    let mut s = Session::default();
    let at = Anchor::Line { side: Side::New, line: 1 };
    let id = ops::add_thread(&mut s, "", "a", at, "F1 issue: x", Some("claude"), None);
    assert_eq!(ops::waiting_on_agent(&s.threads[0], ""), None, "the agent's own draft");
    ops::reply(&mut s, &id, "not like this", None).unwrap();
    assert_eq!(ops::waiting_on_agent(&s.threads[0], ""), Some(Owed::Reply));
    assert!(!ops::waiting_on_user(&s.threads[0], ""));
    ops::reply(&mut s, &id, "Reworked.", Some("claude")).unwrap();
    assert_eq!(ops::waiting_on_agent(&s.threads[0], ""), None);
    assert!(ops::waiting_on_user(&s.threads[0], ""));

    ops::reply(&mut s, &id, "@claude why a lock?", None).unwrap();
    s.threads[0].answer = Some(rain_core::Answer::default());
    assert_eq!(ops::waiting_on_agent(&s.threads[0], ""), Some(Owed::Question));
    assert_eq!(ops::claim(&mut s, &id, false), Some(true));
    assert_eq!(ops::waiting_on_agent(&s.threads[0], ""), Some(Owed::Claimed));
    ops::reply(&mut s, &id, "To keep the total right.", Some("claude")).unwrap();
    assert_eq!(ops::waiting_on_agent(&s.threads[0], ""), None, "answered");

    ops::reply(&mut s, &id, "still wrong", None).unwrap();
    assert!(ops::resolve(&mut s, &id));
    assert_eq!(ops::waiting_on_agent(&s.threads[0], ""), None, "a settled thread waits on nobody");
}

#[test]
fn ops_bump_the_version_only_on_change() {
    let mut s = Session::default();
    let id = ops::add_thread(
        &mut s,
        "",
        "a",
        Anchor::Line { side: Side::New, line: 1 },
        "x",
        Some("claude"),
        None,
    );
    assert_eq!(s.version, 1);
    assert!(ops::waiting_on_user(&s.threads[0], "reviewer"));
    let m = ops::reply(&mut s, &id, "y", None).unwrap();
    assert!(!ops::waiting_on_user(&s.threads[0], "reviewer"));
    assert_eq!(s.version, 2);
    assert!(!ops::edit_message(&mut s, &id, &m, "y"));
    assert!(ops::edit_message(&mut s, &id, &m, "z"));
    assert!(ops::toggle_disposition(&mut s, &id, Disposition::Agree));
    assert_eq!(s.threads[0].disposition, Some(Disposition::Agree));
    assert!(ops::toggle_disposition(&mut s, &id, Disposition::Agree));
    assert_eq!(s.threads[0].disposition, None);
    let v = s.version;
    assert!(!ops::set_disposition(&mut s, &id, None));
    assert!(ops::reply(&mut s, "nope", "y", None).is_none());
    assert_eq!(s.version, v);
    assert!(ops::set_viewed(&mut s, "", "a", "h"));
    assert!(!ops::set_viewed(&mut s, "", "a", "h"));
    assert!(ops::is_viewed(&s, "", "a", "h"));
    assert!(!ops::is_viewed(&s, "", "a", "h2"), "a changed diff reads as unviewed");
    assert!(ops::clear_viewed(&mut s, "", "a"));
    let anchor = Anchor::Line { side: Side::New, line: 2 };
    assert!(ops::set_draft(&mut s, "", "a", anchor, "draft"));
    assert_eq!(ops::take_draft(&mut s, "", "a", anchor).as_deref(), Some("draft"));
    assert!(ops::take_draft(&mut s, "", "a", anchor).is_none());
    assert!(!ops::delete_message(&mut s, &id, &id), "the first message goes with its thread");
    assert!(ops::delete_message(&mut s, &id, &m));
    assert!(ops::delete_thread(&mut s, &id));
    assert!(s.threads.is_empty());
}

fn diff_with(lines: &[(LineKind, u32, u32, &str)]) -> DiffSet {
    let mut b = FileDiffBuilder::new(0, "src/a.ts", FileStatus::Modified);
    b.push_hunk(1, 3, 1, 3, "");
    for &(k, o, n, t) in lines {
        b.push_line(k, o, n, t);
    }
    DiffSet {
        repos: vec![RepoInfo {
            id: "app".into(),
            ..RepoInfo::default()
        }],
        files: vec![b.finish()],
        ignore_whitespace: false,
    }
}

#[test]
fn mark_outdated_flips_when_the_snapshot_differs() {
    let mut s = Session::default();
    let snap = |c: &str| {
        Some(CodeSnapshot {
            content: c.into(),
            ..CodeSnapshot::default()
        })
    };
    ops::add_thread(
        &mut s,
        "app",
        "src/a.ts",
        Anchor::Range {
            side: Side::New,
            start: 1,
            end: 2,
        },
        "r",
        None,
        snap("let a = 1;\nlet b = 2;"),
    );
    let span = Anchor::Span {
        side: Side::New,
        start_line: 2,
        start_col: 4,
        end_line: 2,
        end_col: 5,
    };
    ops::add_thread(&mut s, "app", "src/a.ts", span, "s", None, snap("b"));
    ops::add_thread(
        &mut s,
        "app",
        "src/gone.ts",
        Anchor::Line { side: Side::New, line: 1 },
        "g",
        None,
        snap("x"),
    );
    ops::add_thread(
        &mut s,
        "app",
        "src/a.ts",
        Anchor::Line { side: Side::New, line: 99 },
        "far",
        None,
        snap("x"),
    );

    let same = diff_with(&[(LineKind::Context, 1, 1, "let a = 1;"), (LineKind::Added, 0, 2, "let b = 2;")]);
    assert_eq!(ops::mark_outdated(&mut s, &same), 1);
    let flags: Vec<bool> = s.threads.iter().map(|t| t.outdated).collect();
    assert_eq!(flags, [false, false, true, false]);

    let changed = diff_with(&[(LineKind::Context, 1, 1, "let a = 1;"), (LineKind::Added, 0, 2, "let c = 3;")]);
    assert_eq!(ops::mark_outdated(&mut s, &changed), 2);
    let flags: Vec<bool> = s.threads.iter().map(|t| t.outdated).collect();
    assert_eq!(flags, [true, true, true, false]);
    assert_eq!(ops::mark_outdated(&mut s, &same), 2, "matching again clears the flag");
}

fn file_diff(lines: &[&str]) -> DiffSet {
    let mut b = FileDiffBuilder::new(0, "src/a.ts", FileStatus::Modified);
    b.push_hunk(1, 0, 1, lines.len() as u32, "");
    for (i, t) in lines.iter().enumerate() {
        b.push_line(LineKind::Added, 0, i as u32 + 1, t);
    }
    DiffSet {
        repos: vec![RepoInfo::default()],
        files: vec![b.finish()],
        ignore_whitespace: false,
    }
}

#[test]
fn threads_take_a_snapshot_then_follow_their_code() {
    let mut s = Session::default();
    let line = |n| Anchor::Line { side: Side::New, line: n };
    ops::add_thread(&mut s, "", "src/a.ts", line(2), "on b", Some("claude"), None);
    let first = file_diff(&["a", "b", "c"]);
    let rev = |c: &'static str| move |_: usize, _: Side| Some(c.to_owned());
    assert_eq!(ops::reanchor(&mut s, &first, rev("c1"), |_, _, _| None), 1);
    let t = &s.threads[0];
    assert_eq!(t.code_snapshot.as_ref().map(|c| c.content.as_str()), Some("b"));
    assert_eq!(t.commit.as_deref(), Some("c1"));

    let pushed = file_diff(&["new 1", "new 2", "a", "b", "c"]);
    let maps = std::cell::Cell::new(0);
    let map = |_: &rain_core::Thread, from: &str, to: &str| {
        maps.set(maps.get() + 1);
        assert_eq!((from, to), ("c1", "c2"));
        Some(vec![[0, 0, 1, 2]])
    };
    assert_eq!(ops::reanchor(&mut s, &pushed, rev("c2"), map), 1);
    assert_eq!(maps.get(), 1);
    let t = &s.threads[0];
    assert_eq!((t.anchor, t.original, t.outdated), (line(4), Some(line(2)), false));
    assert_eq!(t.commit.as_deref(), Some("c2"));

    let searched = file_diff(&["x", "b", "y", "z", "w"]);
    assert_eq!(ops::reanchor(&mut s, &searched, rev("c3"), |_, _, _| None), 1);
    assert_eq!((s.threads[0].anchor, s.threads[0].original), (line(2), Some(line(2))));

    let gone = file_diff(&["x", "q", "y"]);
    ops::reanchor(&mut s, &gone, rev("c4"), |_, _, _| None);
    assert!(s.threads[0].outdated, "no line holds the snapshot");
}

#[test]
fn line_maps_follow_hunk_headers() {
    let hunks = [[3, 0, 4, 2], [10, 2, 12, 1]];
    assert_eq!(ops::map_line(&hunks, 3), Some(3));
    assert_eq!(ops::map_line(&hunks, 4), Some(6));
    assert_eq!(ops::map_line(&hunks, 10), None);
    assert_eq!(ops::map_line(&hunks, 12), Some(13));
}

#[test]
fn repo_less_threads_on_a_path_in_two_repos_are_left_alone() {
    let mut s = Session::default();
    ops::add_thread(&mut s, "", "src/a.ts", Anchor::Line { side: Side::New, line: 1 }, "x", None, None);
    let mut d = file_diff(&["a"]);
    let mut twin = d.files[0].clone();
    twin.repo = 1;
    d.files.push(twin);
    d.repos = vec![
        RepoInfo {
            id: "server".into(),
            ..RepoInfo::default()
        },
        RepoInfo {
            id: "app".into(),
            ..RepoInfo::default()
        },
    ];
    assert_eq!(ops::mark_outdated(&mut s, &d), 0);
    let t = &s.threads[0];
    assert!(t.code_snapshot.is_none() && !t.outdated, "a path in two repos is not guessed at");
}

#[test]
fn deleting_the_answer_reopens_the_question_for_the_next_reply() {
    use rain_core::AnswerStatus;
    let mut s = Session::default();
    let q = r#"{"type":"thread","filePath":"a.cs","position":{"side":"new","line":1},"body":"why?","kind":"question"}"#;
    apply(&mut s, from_difit_json(q).unwrap(), "");
    let id = s.threads[0].id.clone();
    let first = ops::reply(&mut s, &id, "because", Some("claude")).unwrap();
    assert_eq!(s.threads[0].answer.as_ref().unwrap().message_id.as_deref(), Some(first.as_str()));
    assert!(ops::delete_message(&mut s, &id, &first));
    let a = s.threads[0].answer.as_ref().unwrap();
    assert_eq!(
        (a.status, a.message_id.as_deref()),
        (AnswerStatus::Cancelled, None),
        "no dangling answer"
    );
    let second = ops::reply(&mut s, &id, "really because", Some("claude")).unwrap();
    let a = s.threads[0].answer.as_ref().unwrap();
    assert_eq!((a.status, a.message_id.as_deref()), (AnswerStatus::Answered, Some(second.as_str())));
}

#[test]
fn an_agent_reply_answers_an_at_claude_question_on_a_comment_thread() {
    use rain_core::{Answer, AnswerStatus};
    let mut s = Session::default();
    let c = r#"{"type":"thread","filePath":"a.cs","position":{"side":"new","line":1},"body":"DROPPED: moot.","author":"Skippy"}"#;
    apply(&mut s, from_difit_json(c).unwrap(), "");
    let id = s.threads[0].id.clone();
    ops::reply(&mut s, &id, "Skippy again", Some("Skippy"));
    assert!(s.threads[0].answer.is_none(), "an agent reply nobody asked for is no answer");
    ops::reply(&mut s, &id, "@Claude I'm not getting this one.", None);
    s.threads[0].answer = Some(Answer {
        status: AnswerStatus::Answering,
        by: Some("launcher".into()),
        ..Answer::default()
    });
    let reply = r#"{"type":"reply","threadId":"THREAD","body":"Fair, badly worded.","author":"Skippy"}"#.replace("THREAD", &id);
    apply(&mut s, from_difit_json(&reply).unwrap(), "");
    let t = &s.threads[0];
    let a = t.answer.as_ref().unwrap();
    assert_eq!(t.kind(), rain_core::ThreadKind::Comment, "the thread stays a comment");
    assert_eq!(a.status, AnswerStatus::Answered);
    assert_eq!(a.by.as_deref(), Some("attached"), "the agent answered, not the launcher");
    assert_eq!(a.message_id.as_deref(), t.messages.last().map(|m| m.id.as_str()));
}

#[test]
fn a_moved_thread_still_dedupes_and_takes_replies_at_its_original_line() {
    let mut s = Session::default();
    let add = r#"{"type":"thread","filePath":"a.cs","position":{"side":"new","line":12},"body":"check this","author":"claude"}"#;
    apply(&mut s, from_difit_json(add).unwrap(), "");
    let t = &mut s.threads[0];
    t.original = Some(t.anchor);
    t.anchor = Anchor::Line { side: Side::New, line: 15 };
    assert!(!apply(&mut s, from_difit_json(add).unwrap(), "").changed, "no duplicate at :12");
    assert_eq!(s.threads.len(), 1);
    let reply = r#"{"type":"reply","filePath":"a.cs","position":{"side":"new","line":12},"body":"more","author":"claude"}"#;
    let r = apply(&mut s, from_difit_json(reply).unwrap(), "");
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(s.threads[0].messages.len(), 2);
}

#[test]
fn a_current_post_of_the_open_readback_keeps_resolved_and_other_threads() {
    let mut s = golden_session();
    let gone = s.threads[0].id.clone();
    assert!(ops::resolve(&mut s, &gone));
    let read = comments_json(&s, None);
    let mut mine: Vec<rain_core::Thread> = serde_json::from_value(read["threads"].clone()).unwrap();
    assert_eq!(mine.len(), s.threads.len() - 1, "readback leaves the resolved thread out");
    mine[0].kind = Some(rain_core::ThreadKind::Nit);
    let edited = mine[0].id.clone();
    let base = s.comments_version;
    let r = import::post(&mut s, &mine, Some(base));
    assert!(!r.merged && r.conflicts.is_empty());
    assert_eq!(s.threads.len(), 3, "a thread the client didn't send is kept");
    assert_eq!(ops::thread(&s, &gone).unwrap().state, rain_core::ThreadState::Resolved);
    assert_eq!(ops::thread(&s, &edited).unwrap().kind, Some(rain_core::ThreadKind::Nit));
}

#[test]
fn a_stale_post_applies_newer_state_and_reports_what_it_kept() {
    let mut s = golden_session();
    let base = s.comments_version;
    let mut mine = s.threads.clone();
    ops::reply(&mut s, &mine[1].id, "someone else", Some("claude"));
    mine[0].state = rain_core::ThreadState::Addressed;
    mine[0].blocking = Some(true);
    mine[0].updated_at = "2999-01-01T00:00:00.000Z".into();
    mine[1].kind = Some(rain_core::ThreadKind::Praise);
    let r = import::post(&mut s, &mine, Some(base));
    assert!(r.merged);
    let t0 = ops::thread(&s, &mine[0].id).unwrap();
    assert_eq!(
        (t0.state, t0.blocking),
        (rain_core::ThreadState::Addressed, Some(true)),
        "a newer copy wins"
    );
    let t1 = ops::thread(&s, &mine[1].id).unwrap();
    assert_eq!(t1.kind, None, "the stored copy is newer");
    assert_eq!(t1.messages.len(), 2);
    assert_eq!(
        r.conflicts,
        vec![mine[1].id.clone()],
        "the kept value is reported, not dropped silently"
    );
}

#[test]
fn stale_post_merges_and_fresh_post_never_deletes() {
    let mut s = golden_session();
    let mut mine = s.threads.clone();
    mine[2].messages[0].body = "edited".into();
    mine[2].messages[0].updated_at = "2026-11-07T00:00:00.000Z".into();
    mine.truncate(3);
    let base = s.version;
    let reply = r#"{"type":"reply","filePath":"src/legacy.cs","position":{"side":"old","line":7},"body":"agent reply","author":"claude","createdAt":"2026-11-06T11:00:00.000Z"}"#;
    apply(&mut s, from_difit_json(reply).unwrap(), "");
    assert!(post_threads(&mut s, mine.clone(), Some(base)), "stale base merges");
    assert_eq!(s.threads[2].messages.len(), 2, "the agent's reply survives the stale push");
    assert_eq!(s.threads[2].messages[0].body, "edited");
    let v = s.comments_version;
    let kept = s.threads.clone();
    assert!(!post_threads(&mut s, vec![], Some(v)));
    assert_eq!(s.threads, kept, "an empty post deletes nothing");
    assert_eq!(
        import::thread_key("a", &Anchor::Line { side: Side::Old, line: 3 }, "b"),
        "a|old|3|b"
    );
}
