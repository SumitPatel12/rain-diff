use std::cell::Cell;
use std::path::PathBuf;

use rain_comments::ops::{self, ThreadExtra};
use rain_comments::{SessionFile, apply_with_excerpts, from_difit_json, snapshot_notes};
use rain_core::brief::BriefDoc;
use rain_core::comments::{BRIEF_PATH, MAP_PATH, note_path};
use rain_core::{
    Anchor, AnchorHow, AnchorState, AnswerStatus, CodeSnapshot, DiffSet, DismissReason, FileDiff, FileDiffBuilder, FileStatus, LineKind,
    Note, NoteKind, OrphanReason, OverviewDocument, RepoInfo, Session, Side, Target, TargetKind, ThreadKind, ThreadState,
};

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn line(n: u32) -> Anchor {
    Anchor::Line { side: Side::New, line: n }
}

/// A file whose every line is added, as `path`.
fn added(path: &str, lines: &[&str]) -> FileDiff {
    let mut b = FileDiffBuilder::new(0, path, FileStatus::Modified);
    b.push_hunk(1, 0, 1, lines.len() as u32, "");
    for (i, t) in lines.iter().enumerate() {
        b.push_line(LineKind::Added, 0, i as u32 + 1, t);
    }
    b.finish()
}

fn set(files: Vec<FileDiff>) -> DiffSet {
    DiffSet {
        repos: vec![RepoInfo::default()],
        files,
        ignore_whitespace: false,
    }
}

fn no_rev(_: usize, _: Side) -> Option<String> {
    None
}

/// Posts a thread on `anchor` of `file` the way the UI does: snapshot taken at post time.
fn post(s: &mut Session, file: &FileDiff, anchor: Anchor, body: &str) -> String {
    let snap = ops::snapshot_at(file, &anchor, &file.path);
    assert!(snap.is_some(), "the anchor is in the diff");
    ops::add_thread(s, "", &file.path, anchor, body, None, snap)
}

const BLOCKS: [&str; 9] = ["fn a() {", "  x();", "}", "fn b() {", "  y();", "}", "fn c() {", "  z();", "}"];

#[test]
fn insertion_above_moves_to_the_brace_whose_neighbours_match() {
    let v1 = added("src/a.rs", &BLOCKS);
    let mut s = Session::default();
    let id = post(&mut s, &v1, line(6), "close of b");
    let snap = s.threads[0].code_snapshot.clone().unwrap();
    assert_eq!(
        (snap.before.as_slice(), snap.after.as_slice()),
        (
            &["fn b() {".to_owned(), "  y();".to_owned()][..],
            &["fn c() {".to_owned(), "  z();".to_owned()][..]
        )
    );

    let mut lines = vec!["// one", "// two"];
    lines.extend(BLOCKS);
    assert_eq!(
        ops::reanchor(&mut s, &set(vec![added("src/a.rs", &lines)]), no_rev, |_, _, _| None),
        1
    );
    let t = ops::thread(&s, &id).unwrap();
    assert_eq!(t.anchor, line(8), "three closing braces; the one closing b wins on neighbours");
    assert_eq!(
        (t.anchor_state, t.anchor_how, t.original),
        (AnchorState::Moved, Some(AnchorHow::Exact), Some(line(6)))
    );
    assert_eq!(t.code_snapshot.as_ref().unwrap(), &snap, "the post-time snapshot is never replaced");
    assert!(!t.outdated);
}

#[test]
fn a_reply_during_the_ladder_keeps_the_moved_anchor() {
    let v1 = added("src/a.rs", &BLOCKS);
    let mut live = Session::default();
    let id = post(&mut live, &v1, line(6), "close of b");
    let other = post(&mut live, &v1, line(2), "x");
    let mut work = live.clone();
    let base = work.threads.clone();
    let mut lines = vec!["// one", "// two"];
    lines.extend(BLOCKS);
    ops::reanchor(&mut work, &set(vec![added("src/a.rs", &lines)]), no_rev, |_, _, _| None);
    ops::reply(&mut live, &id, "still?", Some("claude"));
    let attached = Anchor::Line { side: Side::New, line: 9 };
    live.threads.iter_mut().find(|t| t.id == other).unwrap().anchor = attached;

    assert_eq!(ops::merge_anchors(&mut live, &base, &work.threads), 1);
    let t = ops::thread(&live, &id).unwrap();
    assert_eq!((t.anchor, t.anchor_state), (line(8), AnchorState::Moved));
    assert_eq!(t.messages.len(), 2, "the reply made meanwhile stays");
    assert_eq!(
        ops::thread(&live, &other).unwrap().anchor,
        attached,
        "an anchor changed meanwhile wins"
    );
}

#[test]
fn equal_candidates_are_ambiguous_orphans_not_guesses() {
    let v1 = added("src/a.rs", &["x", "{", "}", "y"]);
    let mut s = Session::default();
    let id = post(&mut s, &v1, line(3), "brace");
    let v2 = added("src/a.rs", &["p", "p", "{", "}", "q", "p", "{", "}", "q"]);
    ops::mark_outdated(&mut s, &set(vec![v2]));
    let t = ops::thread(&s, &id).unwrap();
    assert_eq!(t.anchor_state, AnchorState::Orphaned);
    assert_eq!(t.orphan.as_ref().unwrap().reason, OrphanReason::Ambiguous);
}

#[test]
fn rename_follows_the_old_path() {
    let v1 = added("src/old.rs", &["a", "b", "c"]);
    let mut s = Session::default();
    let id = post(&mut s, &v1, line(2), "on b");
    let mut v2 = added("src/new.rs", &["a", "b", "c"]);
    v2.old_path = Some("src/old.rs".into());
    v2.status = FileStatus::Renamed;
    assert_eq!(ops::mark_outdated(&mut s, &set(vec![v2.clone()])), 1);
    let t = ops::thread(&s, &id).unwrap();
    assert_eq!(t.file_path, "src/new.rs");
    assert_eq!((t.anchor_state, t.anchor_how), (AnchorState::Moved, Some(AnchorHow::Renamed)));

    let mut s = Session::default();
    let id = post(&mut s, &v1, line(2), "on b");
    let mut rewritten = added("src/new.rs", &["q", "r"]);
    rewritten.old_path = Some("src/old.rs".into());
    ops::mark_outdated(&mut s, &set(vec![rewritten]));
    let t = ops::thread(&s, &id).unwrap();
    assert_eq!(t.orphan.as_ref().unwrap().reason, OrphanReason::FileRenamed);
}

#[test]
fn rewrite_is_outdated_then_addressed_after_an_agent_reply() {
    let v1 = added("src/a.rs", &["a", "b", "c", "d", "e"]);
    let mut s = Session::default();
    let id = ops::add_thread(&mut s, "", "src/a.rs", line(3), "F1 issue: c is wrong", Some("claude"), None);
    ops::reply(&mut s, &id, "Agreed", None);
    assert_eq!(ops::mark_outdated(&mut s, &set(vec![v1])), 1, "the first sight takes the snapshot");
    assert_eq!(s.threads[0].code_snapshot.as_ref().unwrap().before, ["a", "b"]);

    ops::mark_outdated(&mut s, &set(vec![added("src/a.rs", &["a", "b", "c2", "d", "e"])]));
    let t = ops::thread(&s, &id).unwrap();
    assert_eq!((t.placement(), t.outdated), (AnchorState::Outdated, true));
    assert!(!ops::addressed_suggested(t) && !ops::waiting_on_user(t, "sumit"));

    ops::reply(&mut s, &id, "Fixed in the next commit.", Some("claude"));
    let t = ops::thread(&s, &id).unwrap();
    assert!(ops::addressed_suggested(t) && ops::waiting_on_user(t, "sumit"));
    assert_eq!(t.state, ThreadState::Open, "a suggestion, never applied by itself");

    assert!(ops::confirm_addressed(&mut s, &id));
    let t = ops::thread(&s, &id).unwrap();
    assert_eq!(t.state, ThreadState::Addressed);
    assert!(!ops::waiting_on_user(t, "sumit"));
}

#[test]
fn deleted_line_is_orphaned_with_its_post_time_snapshot() {
    let v1 = added("src/a.rs", &["a", "b", "c", "d", "e"]);
    let mut s = Session::default();
    let id = post(&mut s, &v1, line(3), "on c");
    let mut b = FileDiffBuilder::new(0, "src/a.rs", FileStatus::Modified);
    b.push_hunk(1, 5, 1, 4, "");
    b.push_line(LineKind::Context, 1, 1, "a");
    b.push_line(LineKind::Context, 2, 2, "b");
    b.push_line(LineKind::Removed, 3, 0, "c");
    b.push_line(LineKind::Context, 4, 3, "d");
    b.push_line(LineKind::Context, 5, 4, "e");
    ops::reanchor(&mut s, &set(vec![b.finish()]), |_, _| Some("c2".into()), |_, _, _| None);
    let t = ops::thread(&s, &id).unwrap();
    assert!(t.outdated);
    let o = t.orphan.as_ref().unwrap();
    assert_eq!(o.reason, OrphanReason::LineDeleted);
    assert_eq!(o.snapshot.content, "c");
    assert_eq!(o.snapshot.lines, Some((3, 3)));
    assert_eq!(t.code_snapshot.as_ref().unwrap().content, "c");
}

#[test]
fn line_map_hunk_that_removes_the_line_reads_as_deleted() {
    let v1 = added("src/a.rs", &["a", "b", "c", "d", "e"]);
    let mut s = Session::default();
    let id = post(&mut s, &v1, line(3), "on c");
    s.threads[0].commit = Some("c1".into());
    let v2 = added("src/a.rs", &["a", "b", "x", "y"]);
    ops::reanchor(&mut s, &set(vec![v2]), |_, _| Some("c2".into()), |_, _, _| Some(vec![[3, 1, 2, 0]]));
    assert_eq!(
        ops::thread(&s, &id).unwrap().orphan.as_ref().unwrap().reason,
        OrphanReason::LineDeleted
    );
}

#[test]
fn a_file_deleted_in_the_diff_reads_deleted_and_one_that_left_it_does_not() {
    let mut s = Session::default();
    let gone = post(&mut s, &added("src/gone.rs", &["x"]), line(1), "on x");
    let left = post(&mut s, &added("src/left.rs", &["y"]), line(1), "on y");
    let mut b = FileDiffBuilder::new(0, "src/gone.rs", FileStatus::Deleted);
    b.push_hunk(1, 1, 0, 0, "");
    b.push_line(LineKind::Removed, 1, 0, "x");
    ops::mark_outdated(&mut s, &set(vec![b.finish()]));
    let reason = |id: &str| ops::thread(&s, id).unwrap().orphan.as_ref().unwrap().reason;
    assert_eq!(reason(&gone), OrphanReason::FileDeleted);
    assert_eq!(reason(&left), OrphanReason::LeftDiff, "a reverted file is not a deleted one");
}

#[test]
fn rewritten_block_is_orphaned_and_a_gone_file_reads_left_the_diff() {
    let v1 = added("src/a.rs", &["a", "b", "c", "d", "e"]);
    let mut s = Session::default();
    let id = post(&mut s, &v1, line(3), "on c");
    let gone = post(&mut s, &added("src/gone.rs", &["x"]), line(1), "on x");
    ops::mark_outdated(&mut s, &set(vec![added("src/a.rs", &["p", "q", "r", "s", "t"])]));
    assert_eq!(
        ops::thread(&s, &id).unwrap().orphan.as_ref().unwrap().reason,
        OrphanReason::ContentRewritten
    );
    let g = ops::thread(&s, &gone).unwrap();
    assert_eq!(g.orphan.as_ref().unwrap().reason, OrphanReason::LeftDiff);
    assert_eq!(g.orphan.as_ref().unwrap().snapshot.content, "x");
    let v = s.comments_version;
    assert_eq!(
        ops::mark_outdated(&mut s, &set(vec![added("src/a.rs", &["p", "q", "r", "s", "t"])])),
        0,
        "orphans stay put"
    );
    assert_eq!(s.comments_version, v);
}

#[test]
fn batched_line_maps_ask_once_per_revision_pair() {
    let v1a = added("src/a.rs", &["a", "b"]);
    let v1b = added("src/b.rs", &["c", "d"]);
    let mut s = Session::default();
    post(&mut s, &v1a, line(1), "a");
    post(&mut s, &v1a, line(2), "b");
    post(&mut s, &v1b, line(2), "d");
    for t in &mut s.threads {
        t.commit = Some("c1".into());
    }
    let calls = Cell::new(0);
    let v2 = set(vec![added("src/a.rs", &["n", "a", "b"]), added("src/b.rs", &["n", "c", "d"])]);
    let n = ops::reanchor_batched(
        &mut s,
        &v2,
        |_, _| Some("c2".into()),
        |repo, side, from, to, paths: &[&str]| {
            calls.set(calls.get() + 1);
            assert_eq!((repo, side, from, to), (0, Side::New, "c1", "c2"));
            assert_eq!(paths, ["src/a.rs", "src/b.rs"]);
            vec![Some(vec![[0, 0, 1, 1]]); paths.len()]
        },
    );
    assert_eq!((n, calls.get()), (3, 1));
    let lines: Vec<u32> = s.threads.iter().map(|t| t.anchor.lines().0).collect();
    assert_eq!(lines, [2, 3, 3]);
    assert!(s.threads.iter().all(|t| t.anchor_how == Some(AnchorHow::Exact)));
}

#[test]
fn line_notes_take_a_snapshot_and_follow_their_code() {
    let mut s = Session::default();
    s.notes.push(Note {
        id: "n1".into(),
        kind: NoteKind::Line,
        file_path: "src/a.rs".into(),
        anchor: Some(line(2)),
        body: "why b".into(),
        ..Note::default()
    });
    let v1 = added("src/a.rs", &["a", "b", "c"]);
    let files = set(vec![v1.clone()]);
    let excerpt = |_: &str, p: &str, a: &Anchor| files.files.iter().find(|f| f.path == p).and_then(|f| ops::snapshot_at(f, a, p));
    assert_eq!(snapshot_notes(&mut s, &excerpt), 1);
    assert_eq!(s.notes[0].code_snapshot.as_ref().unwrap().content, "b");
    assert_eq!(
        ops::reanchor_notes(&mut s, &set(vec![added("src/a.rs", &["n", "a", "b", "c"])]), no_rev),
        1
    );
    assert_eq!((s.notes[0].anchor, s.notes[0].anchor_state), (Some(line(3)), AnchorState::Moved));
    ops::reanchor_notes(&mut s, &set(vec![]), no_rev);
    assert_eq!(s.notes[0].orphan.as_ref().unwrap().reason, OrphanReason::LeftDiff);
    assert_eq!(s.comments_version, 0, "notes never touch the comments version");
}

fn brief() -> BriefDoc {
    serde_json::from_str(&fixture("brief/sample.json")).unwrap()
}

fn brief_thread(s: &mut Session, section: &str, quote: &str) -> String {
    let target = Target {
        kind: TargetKind::Brief,
        section: Some(section.into()),
        quote: Some(quote.into()),
        ..Target::default()
    };
    let snap = ops::artifact_snapshot(s, &target);
    ops::add_thread_with(
        s,
        "",
        BRIEF_PATH,
        line(1),
        "Why the same unit of work?",
        None,
        snap,
        ThreadExtra {
            kind: Some(ThreadKind::Question),
            target: Some(target),
            ..ThreadExtra::default()
        },
    )
}

#[test]
fn brief_threads_survive_rewrites_by_id_then_quote_else_orphan() {
    let mut s = Session {
        brief: Some(brief()),
        ..Session::default()
    };
    let id = brief_thread(&mut s, "how.2", "same unit of work");
    let t = ops::thread(&s, &id).unwrap();
    assert!(
        t.code_snapshot
            .as_ref()
            .unwrap()
            .content
            .starts_with("One Voucher row in the same unit of work\nThe voucher row")
    );
    assert_eq!(t.answer.as_ref().unwrap().status, AnswerStatus::Queued, "a question starts queued");
    assert_eq!(ops::reanchor_targets(&mut s), 0, "unchanged brief, nothing moves");

    let mut b = brief();
    b.how[1].id = "how.persist".into();
    s.brief = Some(b);
    assert_eq!(ops::reanchor_targets(&mut s), 1);
    let t = ops::thread(&s, &id).unwrap();
    assert_eq!((t.anchor_state, t.anchor_how), (AnchorState::Moved, Some(AnchorHow::Fuzzy)));
    assert_eq!(t.target.as_ref().unwrap().section.as_deref(), Some("how.persist"));

    let mut b = brief();
    b.how[1].id = "how.persist".into();
    b.how[1].title = "Persisting vouchers".into();
    b.how[1].text = "Saved together with the payment.".into();
    s.brief = Some(b);
    ops::reanchor_targets(&mut s);
    assert_eq!(
        ops::thread(&s, &id).unwrap().placement(),
        AnchorState::Outdated,
        "rewritten under the quote"
    );

    let mut b = brief();
    b.how.remove(1);
    s.brief = Some(b);
    ops::reanchor_targets(&mut s);
    let t = ops::thread(&s, &id).unwrap();
    let o = t.orphan.as_ref().unwrap();
    assert_eq!(o.reason, OrphanReason::SectionRemoved);
    assert!(o.snapshot.content.contains("so a failed payment never leaves a voucher behind"));
}

#[test]
fn a_section_inserted_above_moves_a_positional_id_to_where_the_quote_went() {
    let mut s = Session {
        brief: Some(brief()),
        ..Session::default()
    };
    let id = brief_thread(&mut s, "how.2", "same unit of work");
    let mut b = brief();
    let mut first = b.how[0].clone();
    first.title = "A new first step".into();
    first.text = "Validates the request.".into();
    b.how.insert(0, first);
    for (i, h) in b.how.iter_mut().enumerate() {
        h.id = format!("how.{}", i + 1);
    }
    s.brief = Some(b);
    assert_eq!(ops::reanchor_targets(&mut s), 1);
    let t = ops::thread(&s, &id).unwrap();
    assert_eq!((t.anchor_state, t.anchor_how), (AnchorState::Moved, Some(AnchorHow::Fuzzy)));
    assert_eq!(t.target.as_ref().unwrap().section.as_deref(), Some("how.3"));
}

#[test]
fn node_threads_follow_the_label_and_note_threads_a_replacement() {
    let doc: OverviewDocument = serde_json::from_str(&fixture("overview/sample-overview.json")).unwrap();
    let mut s = Session {
        overview: Some(doc.clone()),
        ..Session::default()
    };
    let target = Target {
        kind: TargetKind::Node,
        node: Some("voucher-service".into()),
        view: Some("architecture".into()),
        ..Target::default()
    };
    let snap = ops::artifact_snapshot(&s, &target);
    assert!(snap.as_ref().unwrap().content.starts_with("VoucherService\n"));
    let id = ops::add_thread_with(
        &mut s,
        "",
        MAP_PATH,
        line(1),
        "Is this the only writer?",
        None,
        snap,
        ThreadExtra {
            target: Some(target),
            ..ThreadExtra::default()
        },
    );
    let mut renamed = doc.clone();
    renamed.graph.nodes.iter_mut().find(|n| n.id == "voucher-service").unwrap().id = "vouchers".into();
    s.overview = Some(renamed);
    assert_eq!(ops::reanchor_targets(&mut s), 1);
    let t = ops::thread(&s, &id).unwrap();
    assert_eq!((t.anchor_state, t.anchor_how), (AnchorState::Moved, Some(AnchorHow::Label)));
    assert_eq!(t.target.as_ref().unwrap().node.as_deref(), Some("vouchers"));
    let mut gone = doc;
    gone.graph.nodes.retain(|n| n.id != "voucher-service");
    s.overview = Some(gone);
    ops::reanchor_targets(&mut s);
    assert_eq!(
        ops::thread(&s, &id).unwrap().orphan.as_ref().unwrap().reason,
        OrphanReason::NodeRemoved
    );

    let mut s = Session::default();
    let note = Note {
        id: "k7p2qa".into(),
        kind: NoteKind::Line,
        file_path: "src/a.rs".into(),
        anchor: Some(line(22)),
        body: "Order matters here.".into(),
        author: Some("claude".into()),
        ..Note::default()
    };
    s.notes.push(note.clone());
    let target = Target {
        kind: TargetKind::Note,
        note: Some("k7p2qa".into()),
        ..Target::default()
    };
    let snap = ops::artifact_snapshot(&s, &target);
    let id = ops::add_thread_with(
        &mut s,
        "",
        &note_path("k7p2qa"),
        line(1),
        "Why?",
        None,
        snap,
        ThreadExtra::default(),
    );
    assert_eq!(
        ops::thread(&s, &id).unwrap().target_kind(),
        TargetKind::Note,
        "the path implies the target"
    );
    s.notes[0].id = "z9".into();
    ops::reanchor_targets(&mut s);
    let t = ops::thread(&s, &id).unwrap();
    assert_eq!(
        (t.anchor_state, t.target.as_ref().unwrap().note.as_deref()),
        (AnchorState::Moved, Some("z9"))
    );
    assert_eq!(t.file_path, "@note:z9");
    s.notes.clear();
    ops::reanchor_targets(&mut s);
    let t = ops::thread(&s, &id).unwrap();
    assert_eq!(t.orphan.as_ref().unwrap().reason, OrphanReason::NoteRemoved);
    assert_eq!(t.orphan.as_ref().unwrap().snapshot.content, "Order matters here.");
}

#[test]
fn stale_brief_cites_are_reported() {
    let doc = brief();
    let mut f = added("src/Services/VoucherService.cs", &["x"; 40]);
    f.repo = 0;
    let diff = DiffSet {
        repos: vec![RepoInfo {
            id: "server".into(),
            ..RepoInfo::default()
        }],
        files: vec![f],
        ignore_whitespace: false,
    };
    let stale = ops::reanchor_brief(&doc, &diff);
    assert!(
        !stale.iter().any(|c| c.cite.path == "server/src/Services/VoucherService.cs"),
        "{stale:?}"
    );
    assert!(stale.iter().any(|c| c.section == "how.1" && c.reason == "file not in the diff"));
}

#[test]
fn a_huge_cite_range_is_checked_without_walking_every_line() {
    let diff = |lines: &str| {
        let text = fixture("brief/sample.json").replace(r#""lines": [27, 31]"#, &format!(r#""lines": {lines}"#));
        let doc: BriefDoc = serde_json::from_str(&text).unwrap();
        let mut f = added("src/Services/VoucherService.cs", &["x"; 40]);
        f.repo = 0;
        let set = DiffSet {
            repos: vec![RepoInfo {
                id: "server".into(),
                ..RepoInfo::default()
            }],
            files: vec![f],
            ignore_whitespace: false,
        };
        ops::reanchor_brief(&doc, &set)
            .into_iter()
            .filter(|c| c.cite.path == "server/src/Services/VoucherService.cs")
            .map(|c| c.reason)
            .collect::<Vec<_>>()
    };
    let started = std::time::Instant::now();
    assert_eq!(diff("[41, 4294967295]"), ["lines outside the diff"]);
    assert!(diff("[40, 4294967295]").is_empty(), "line 40 is shown");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn imports_snapshot_against_the_loaded_diff_or_become_file_level() {
    let files = set(vec![added("src/a.rs", &["a", "b", "c"])]);
    let excerpt = |_: &str, p: &str, a: &Anchor| files.files.iter().find(|f| f.path == p).and_then(|f| ops::snapshot_at(f, a, p));
    let mut s = Session::default();
    let json = r#"[{"type":"thread","filePath":"src/a.rs","position":{"side":"new","line":2},"body":"F1 issue (blocking): b","author":"claude"},
                   {"type":"thread","filePath":"src/a.rs","position":{"side":"new","line":40},"body":"far away","author":"claude"},
                   {"type":"thread","filePath":"@brief","position":{"side":"new","line":1},"body":"why?","kind":"question","target":{"kind":"brief","section":"intent"}}]"#;
    let r = apply_with_excerpts(&mut s, from_difit_json(json).unwrap(), "", &excerpt);
    assert_eq!(
        r.warnings,
        ["src/a.rs:new:40 is outside the loaded diff; stored as a file-level thread."]
    );
    let snap = s.threads[0].code_snapshot.as_ref().unwrap();
    assert_eq!((snap.content.as_str(), snap.before.as_slice()), ("b", &["a".to_owned()][..]));
    assert_eq!(s.threads[0].kind(), ThreadKind::Issue);
    assert_eq!(s.threads[1].target_kind(), TargetKind::File);
    assert!(s.threads[1].code_snapshot.is_none());
    assert_eq!(s.threads[2].kind, Some(ThreadKind::Question));
    assert_eq!(s.threads[2].answer.as_ref().unwrap().status, AnswerStatus::Queued);

    let reply = r#"{"type":"reply","threadId":"THREAD","body":"Because the intent says so.","author":"claude"}"#
        .replace("THREAD", &s.threads[2].id);
    rain_comments::apply(&mut s, from_difit_json(&reply).unwrap(), "");
    let a = s.threads[2].answer.as_ref().unwrap();
    assert_eq!(a.status, AnswerStatus::Answered);
    assert_eq!(a.message_id.as_deref(), Some(s.threads[2].messages[1].id.as_str()));
    assert!(
        ops::waiting_on_user(&s.threads[2], "sumit"),
        "an answered question waits on the user"
    );
}

#[test]
fn states_keep_history_and_leave_default_readback() {
    let mut s = Session::default();
    let id = ops::add_thread(&mut s, "", "a.rs", line(1), "x", Some("claude"), None);
    let other = ops::add_thread(&mut s, "", "a.rs", line(2), "y", Some("claude"), None);
    assert!(ops::resolve(&mut s, &id));
    assert!(!ops::resolve(&mut s, &id));
    assert_eq!(s.threads.len(), 2, "resolve never deletes");
    assert!(!ops::waiting_on_user(&s.threads[0], "sumit"));
    assert!(!rain_comments::comments_json_string(&s, None).contains(&id));
    assert!(rain_comments::comments_json_string_with(&s, None, true).contains(&id));
    assert!(!rain_comments::comments_output(&s, None).contains("L1"));
    assert!(rain_comments::comments_output_with(&s, None, true).contains("state: resolved"));

    assert!(ops::dismiss(&mut s, &other, DismissReason::Style));
    let t = ops::thread(&s, &other).unwrap();
    assert_eq!((t.state, t.dismiss_reason), (ThreadState::Dismissed, Some(DismissReason::Style)));
    assert!(t.state_at.is_some());
    assert!(ops::reopen(&mut s, &other));
    let t = ops::thread(&s, &other).unwrap();
    assert_eq!((t.state, t.dismiss_reason), (ThreadState::Open, None));
    assert!(ops::set_state(&mut s, &other, ThreadState::Addressed, Some("abc123"), None));
    let t = ops::thread(&s, &other).unwrap();
    assert_eq!(t.state_sha.as_deref(), Some("abc123"));
    assert!(
        ops::waiting_on_user(t, "sumit"),
        "the agent's addressed waits for the user's confirm"
    );
    assert!(ops::confirm_addressed(&mut s, &other));
    let t = ops::thread(&s, &other).unwrap();
    assert_eq!(t.state_sha.as_deref(), Some("abc123"));
    assert!(!ops::waiting_on_user(t, "sumit"));
}

#[test]
fn round_two_session_loads_open_with_kinds_derived() {
    let dir = std::env::temp_dir()
        .join("rain-comments-tests")
        .join(format!("round2-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("round2.json");
    std::fs::write(&path, fixture("sessions/round2.json")).unwrap();
    let s = SessionFile::load(&path).unwrap();
    assert!(!s.threads.is_empty());
    assert!(s.threads.iter().all(|t| t.state == ThreadState::Open && t.kind.is_none()));
    assert_eq!(s.threads[0].kind(), ThreadKind::Issue);
    assert!(s.threads.iter().all(|t| t.target.is_none() && t.answer.is_none()));
}

#[test]
fn snapshots_carry_place_and_neighbours() {
    let f = added("src/a.rs", &["a", "b", "c", "d"]);
    let snap = ops::snapshot_at(
        &f,
        &Anchor::Range {
            side: Side::New,
            start: 1,
            end: 2,
        },
        "src/a.rs",
    )
    .unwrap();
    assert_eq!(
        snap,
        CodeSnapshot {
            content: "a\nb".into(),
            path: Some("src/a.rs".into()),
            lines: Some((1, 2)),
            side: Some(Side::New),
            after: vec!["c".into(), "d".into()],
            ..CodeSnapshot::default()
        }
    );
    assert!(ops::snapshot_at(&f, &line(9), "src/a.rs").is_none());
}
