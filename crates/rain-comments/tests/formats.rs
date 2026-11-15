use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rain_comments::{
    SessionFile, anchor_label, apply, autosave, comments_json, comments_json_string, comments_output, from_difit_json, import, ops,
    post_threads,
};
use rain_core::{Anchor, CodeSnapshot, DiffSet, Disposition, FileDiffBuilder, FileStatus, LineKind, RepoInfo, Session, Shared, Side};

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
    let id = ops::add_thread(&mut s, "server", "src/a.ts", span, "Rename this.", Some("sumit"), Some(snap));
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
    assert!(ops::waiting_on_user(&s.threads[0], "sumit"));
    let m = ops::reply(&mut s, &id, "y", None).unwrap();
    assert!(!ops::waiting_on_user(&s.threads[0], "sumit"));
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
fn stale_post_merges_and_fresh_post_replaces() {
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
    let v = s.version;
    assert!(!post_threads(&mut s, vec![], Some(v)));
    assert!(s.threads.is_empty());
    assert_eq!(
        import::thread_key("a", &Anchor::Line { side: Side::Old, line: 3 }, "b"),
        "a|old|3|b"
    );
}
