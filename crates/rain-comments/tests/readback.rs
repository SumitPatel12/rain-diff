//! Goldens for round-3 readback: a question with its answer, an orphan, and threads on the
//! Brief, the Map, a note, a file and the whole change. `RAIN_UPDATE_GOLDEN=1` rewrites them.

use std::path::PathBuf;

use rain_comments::{apply, comments_json_string_with, comments_output_with, from_difit_json, ops};
use rain_core::{FileDiffBuilder, FileStatus, LineKind, Note, NoteKind, OverviewDocument, RepoInfo, Session, Side};

fn fixture(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn golden(name: &str, got: &str) {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden").join(name);
    if std::env::var_os("RAIN_UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, got).unwrap();
    }
    let want = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    assert_eq!(got, want.replace("\r\n", "\n"), "{name}");
}

const IMPORTS: &str = r#"[
  {"type":"thread","id":"q1","filePath":"src/a.rs","position":{"side":"new","line":2},"body":"Why is b computed twice?","kind":"question","createdAt":"2026-11-14T10:00:00.000Z",
   "codeSnapshot":{"content":"b","path":"src/a.rs","lines":[2,2],"side":"new","before":["a"],"after":["c","d"]}},
  {"type":"reply","threadId":"q1","id":"q1r","body":"It is not; the second call is cached.","author":"claude","createdAt":"2026-11-14T10:01:00.000Z"},
  {"type":"thread","id":"f1","filePath":"src/a.rs","position":{"side":"new","line":3},"body":"F1 issue (blocking): c overflows.","author":"claude","createdAt":"2026-11-14T10:02:00.000Z",
   "codeSnapshot":{"content":"c","path":"src/a.rs","lines":[3,3],"side":"new","before":["a","b"],"after":["d"]}},
  {"type":"thread","id":"b1","filePath":"@brief","position":{"side":"new","line":1},"body":"Is the unit of work shared with refunds?","createdAt":"2026-11-14T10:03:00.000Z",
   "target":{"kind":"brief","section":"how.2","quote":"same unit of work"}},
  {"type":"thread","id":"m1","filePath":"@map","position":{"side":"new","line":1},"body":"Only writer?","createdAt":"2026-11-14T10:04:00.000Z",
   "target":{"kind":"node","node":"voucher-service","view":"architecture"}},
  {"type":"thread","id":"n1","filePath":"@note:k7p2qa","position":{"side":"new","line":1},"body":"Which order?","createdAt":"2026-11-14T10:05:00.000Z"},
  {"type":"thread","id":"fl","filePath":"src/a.rs","position":{"side":"new","line":1},"body":"Split this file.","createdAt":"2026-11-14T10:06:00.000Z","target":{"kind":"file"}},
  {"type":"thread","id":"pr","filePath":"@pr","position":{"side":"new","line":1},"body":"Ship it after F1.","createdAt":"2026-11-14T10:07:00.000Z"}
]"#;

fn session() -> Session {
    let mut s = Session {
        brief: Some(serde_json::from_str(&fixture("brief/sample.json")).unwrap()),
        overview: Some(serde_json::from_str::<OverviewDocument>(&fixture("overview/sample-overview.json")).unwrap()),
        ..Session::default()
    };
    s.notes.push(Note {
        id: "k7p2qa".into(),
        kind: NoteKind::Line,
        file_path: "src/a.rs".into(),
        anchor: Some(rain_core::Anchor::Line { side: Side::New, line: 2 }),
        body: "Order matters: b before c. The cache is warm.".into(),
        author: Some("claude".into()),
        ..Note::default()
    });
    let r = apply(&mut s, from_difit_json(IMPORTS).unwrap(), "");
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    for t in &mut s.threads {
        t.commit = Some("a1b2c3d".into());
    }
    let mut b = FileDiffBuilder::new(0, "src/a.rs", FileStatus::Modified);
    b.push_hunk(1, 4, 1, 3, "");
    b.push_line(LineKind::Context, 1, 1, "a");
    b.push_line(LineKind::Context, 2, 2, "b");
    b.push_line(LineKind::Removed, 3, 0, "c");
    b.push_line(LineKind::Context, 4, 3, "d");
    let diff = rain_core::DiffSet {
        repos: vec![RepoInfo::default()],
        files: vec![b.finish()],
        ignore_whitespace: false,
    };
    ops::reanchor(&mut s, &diff, |_, _| Some("d4e5f6a".into()), |_, _, _| None);
    for t in &mut s.threads {
        if let Some(o) = &mut t.orphan {
            o.at = "2026-11-14T11:00:00.000Z".into();
        }
        t.state_at = None;
    }
    s
}

#[test]
fn round_three_threads_read_back_with_ids_states_and_artifact_locations() {
    let s = session();
    assert_eq!(s.threads[1].orphan.as_ref().unwrap().reason, rain_core::OrphanReason::LineDeleted);
    golden("round3-output.txt", &comments_output_with(&s, None, false));
    golden("round3-comments.json", &comments_json_string_with(&s, None, false));
}

#[test]
fn settled_threads_read_back_only_with_all() {
    let mut s = session();
    ops::dismiss(&mut s, "m1", rain_core::DismissReason::OutOfScope);
    for t in &mut s.threads {
        t.state_at = None;
        t.updated_at = t.created_at.clone();
    }
    assert!(!comments_output_with(&s, None, false).contains("Only writer?"));
    let all = comments_output_with(&s, None, true);
    assert!(
        all.contains("[id m1] kind: comment · state: dismissed (out of scope) · anchor: placed"),
        "{all}"
    );
}
