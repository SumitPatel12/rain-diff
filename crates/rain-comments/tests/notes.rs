use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rain_comments::notes::{from_json, from_overview};
use rain_comments::{
    SessionFile, add_notes, apply, autosave, comments_json_string, comments_output, from_difit_json, merge_threads, notes_for_file,
    notes_json_string, ops, post_threads, remove_note,
};
use rain_core::overview::GraphRevision;
use rain_core::{Anchor, Note, NoteKind, OverviewDocument, Session, SessionRepo, Shared, Side};

fn fixture(path: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(path);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn multi_repo() -> Session {
    Session {
        repos: ["server", "app"]
            .iter()
            .map(|id| SessionRepo {
                id: (*id).into(),
                ..SessionRepo::default()
            })
            .collect(),
        ..Session::default()
    }
}

fn golden_session() -> Session {
    let mut s = Session::default();
    apply(&mut s, from_difit_json(&fixture("difit/import.json")).unwrap(), "");
    s
}

fn note(kind: NoteKind, path: &str, body: &str, author: &str) -> Note {
    Note {
        id: String::new(),
        kind,
        repo: String::new(),
        file_path: path.into(),
        anchor: None,
        title: None,
        body: body.into(),
        author: Some(author.into()),
        ..Note::default()
    }
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
fn fixture_batch_lands_split_by_repo_and_repeats_as_no_op() {
    let mut s = multi_repo();
    let notes = from_json(&fixture("notes/notes.json")).unwrap();
    assert_eq!(notes.len(), 5);
    let r = add_notes(&mut s, notes.clone(), "", false);
    assert_eq!((r.count, r.ids.len(), r.changed), (5, 5, true));
    assert_eq!((s.version, s.notes_version, s.comments_version), (1, 1, 0));
    assert!(
        s.notes
            .iter()
            .all(|n| n.id.len() == 16 && n.created_at.len() == 24 && n.updated_at == n.created_at)
    );
    let stories: Vec<&Note> = s.notes.iter().filter(|n| n.kind == NoteKind::Story).collect();
    assert_eq!(stories.len(), 1);
    assert_eq!(stories[0].repo, "");
    let vouchers = notes_for_file(&s, "server", "src/Services/VoucherService.cs");
    assert_eq!(vouchers.len(), 1);
    assert_eq!(
        (vouchers[0].repo.as_str(), vouchers[0].file_path.as_str()),
        ("server", "src/Services/VoucherService.cs")
    );
    let inv = notes_for_file(&s, "server", "src/Services/AccountService.cs");
    assert_eq!(
        inv[0].anchor,
        Some(Anchor::Range {
            side: Side::Old,
            start: 44,
            end: 52
        })
    );
    assert_eq!(inv[0].title.as_deref(), Some("Old total"));

    let again = add_notes(&mut s, notes, "", false);
    assert!(!again.changed);
    assert_eq!(again.ids, r.ids, "a duplicate reports the existing id");
    assert_eq!(s.notes_version, 1);
}

#[test]
fn explicit_repo_wins_over_a_path_prefix_and_single_repo_never_splits() {
    let mut s = multi_repo();
    add_notes(&mut s, vec![note(NoteKind::File, "server/x.cs", "b", "claude")], "app", false);
    assert_eq!((s.notes[0].repo.as_str(), s.notes[0].file_path.as_str()), ("app", "server/x.cs"));
    let mut one = Session::default();
    add_notes(&mut one, vec![note(NoteKind::File, "server/x.cs", "b", "claude")], "", false);
    assert_eq!((one.notes[0].repo.as_str(), one.notes[0].file_path.as_str()), ("", "server/x.cs"));
}

#[test]
fn same_id_updates_and_same_content_dedupes() {
    let mut s = Session::default();
    let id = add_notes(&mut s, vec![note(NoteKind::File, "a.ts", "v1", "claude")], "", false).ids[0].clone();
    let mut edit = note(NoteKind::File, "a.ts", "v2", "claude");
    edit.id = id.clone();
    let r = add_notes(&mut s, vec![edit.clone()], "", false);
    assert!(r.changed);
    assert_eq!((s.notes.len(), s.notes[0].body.as_str()), (1, "v2"));
    assert!(!add_notes(&mut s, vec![edit], "", false).changed);
    let by_content = add_notes(&mut s, vec![note(NoteKind::File, "a.ts", "  v2 ", "someone")], "", false);
    assert!(
        !by_content.changed,
        "same kind, path, anchor and body is a duplicate whoever wrote it"
    );
    assert_eq!(by_content.ids, [id]);
}

#[test]
fn replace_swaps_only_the_same_authors_notes_of_that_kind_and_file() {
    let mut s = Session::default();
    add_notes(
        &mut s,
        vec![
            note(NoteKind::File, "a.ts", "old claude", "claude"),
            note(NoteKind::File, "a.ts", "mine", "sumit"),
            note(NoteKind::File, "b.ts", "other file", "claude"),
            note(NoteKind::Story, "", "story", "claude"),
        ],
        "",
        false,
    );
    let r = add_notes(&mut s, vec![note(NoteKind::File, "a.ts", "new claude", "claude")], "", true);
    assert!(r.changed);
    let bodies: Vec<&str> = s.notes.iter().map(|n| n.body.as_str()).collect();
    assert_eq!(bodies, ["mine", "other file", "story", "new claude"]);
    assert_eq!(s.notes_version, 2);
    assert_eq!(s.comments_version, 0);

    let id = s.notes[3].id.clone();
    let same = add_notes(&mut s, vec![note(NoteKind::File, "a.ts", "new claude", "claude")], "", true);
    assert!(!same.changed, "replacing a note with itself is no change");
    assert_eq!((same.ids.as_slice(), s.notes.len(), s.notes_version), ([id].as_slice(), 4, 2));
}

#[test]
fn remove_note_spares_overview_ids() {
    let mut s = Session::default();
    let id = add_notes(&mut s, vec![note(NoteKind::Story, "", "s", "claude")], "", false).ids[0].clone();
    assert!(!remove_note(&mut s, "overview:x"));
    assert!(!remove_note(&mut s, "missing"));
    assert!(remove_note(&mut s, &id));
    assert!(s.notes.is_empty());
    assert_eq!((s.notes_version, s.comments_version), (2, 0));
}

#[test]
fn incoming_overview_ids_are_not_kept() {
    let mut s = Session::default();
    let mut n = note(NoteKind::Story, "", "s", "claude");
    n.id = "overview:x".into();
    let r = add_notes(&mut s, vec![n], "", false);
    assert!(!r.ids[0].starts_with("overview:"));
}

#[test]
fn note_validation_errors() {
    let cases = [
        ("nope", "Invalid note JSON"),
        ("[1]", "Note must be an object"),
        (r#"{"kind":"chapter","body":"b"}"#, "Invalid note field: kind"),
        (r#"{"body":"b"}"#, "Invalid note field: kind"),
        (r#"{"kind":"story","filePath":"a.ts","body":"b"}"#, "Invalid note field: filePath"),
        (r#"{"kind":"file","body":"b"}"#, "Invalid note field: filePath"),
        (
            r#"{"kind":"file","filePath":"a","position":{"side":"new","line":1},"body":"b"}"#,
            "Invalid note field: position",
        ),
        (r#"{"kind":"line","filePath":"a","body":"b"}"#, "Invalid note field: position"),
        (
            r#"{"kind":"line","filePath":"a","position":{"side":"mid","line":1},"body":"b"}"#,
            "Invalid note field: position.side",
        ),
        (
            r#"{"kind":"line","filePath":"a","position":{"side":"new","line":0},"body":"b"}"#,
            "Invalid note field: position.line",
        ),
        (r#"{"kind":"story","body":"  "}"#, "Invalid note field: body"),
        (r#"{"kind":"story","body":"b","author":3}"#, "Invalid note field: author"),
    ];
    for (json, want) in cases {
        assert_eq!(from_json(json).unwrap_err(), want, "{json}");
    }
    let n = from_json(r#"{"kind":"story","body":"  trimmed  "}"#).unwrap();
    assert_eq!(n[0].body, "trimmed");
}

#[test]
fn overview_notes_map_onto_line_notes() {
    let mut doc: OverviewDocument = serde_json::from_str(&fixture("overview/sample-overview-schema.json")).unwrap();
    let notes = from_overview(&doc);
    assert_eq!(notes.len(), doc.notes.len());
    assert_eq!(notes.len(), 3);
    assert!(
        notes
            .iter()
            .all(|n| n.id.starts_with("overview:") && n.is_overview() && n.kind == NoteKind::Line)
    );
    assert_eq!(notes[0].id, "overview:record-before-refresh");
    assert_eq!(notes[0].author.as_deref(), Some("overview"));
    assert_eq!(
        (notes[0].repo.as_str(), notes[0].key().as_str()),
        ("", "server/src/Api/VouchersController.cs")
    );
    assert_eq!(notes[0].anchor, Some(Anchor::Line { side: Side::New, line: 22 }));
    assert_eq!(
        notes[2].anchor,
        Some(Anchor::Range {
            side: Side::New,
            start: 44,
            end: 52
        })
    );
    doc.notes[0].revision = Some(GraphRevision::Base);
    assert_eq!(from_overview(&doc)[0].anchor, Some(Anchor::Line { side: Side::Old, line: 22 }));

    let mut s = multi_repo();
    s.overview = Some(doc);
    add_notes(
        &mut s,
        vec![note(NoteKind::File, "server/src/Api/VouchersController.cs", "f", "claude")],
        "",
        false,
    );
    let v: serde_json::Value = serde_json::from_str(&notes_json_string(&s, None, None)).unwrap();
    assert_eq!(v["notesVersion"], 1);
    assert_eq!(v["notes"].as_array().unwrap().len(), 4, "stored notes then the overview's");
    assert_eq!(v["notes"][0]["repo"], "server");
    assert_eq!(v["notes"][1]["id"], "overview:record-before-refresh");
    let one: serde_json::Value = serde_json::from_str(&notes_json_string(&s, None, Some("server/src/Api/VouchersController.cs"))).unwrap();
    assert_eq!(
        one["notes"].as_array().unwrap().len(),
        2,
        "a stored file note and an overview note share the key"
    );
    let client: serde_json::Value = serde_json::from_str(&notes_json_string(&s, Some("app"), None)).unwrap();
    assert!(client["notes"].as_array().unwrap().is_empty());
}

fn three_notes(s: &mut Session) {
    let mut line = note(NoteKind::Line, "src/a.ts", "line", "claude");
    line.anchor = Some(Anchor::Line { side: Side::New, line: 2 });
    add_notes(
        s,
        vec![
            note(NoteKind::Story, "", "story", "claude"),
            note(NoteKind::File, "src/a.ts", "file", "claude"),
            line,
        ],
        "",
        false,
    );
}

#[test]
fn notes_never_reach_comment_readback() {
    let plain = golden_session();
    let mut with = plain.clone();
    three_notes(&mut with);
    assert_eq!(with.threads.len(), plain.threads.len());
    assert!(with.threads.len() >= 2);
    assert_eq!(comments_json_string(&with, None), comments_json_string(&plain, None));
    assert_eq!(comments_output(&with, None), comments_output(&plain, None));
    assert_eq!(
        comments_json_string(&with, None),
        fixture("difit/comments-json.golden.json").trim_end()
    );

    let notes = with.notes.clone();
    let threads = with.threads.clone();
    let base = with.comments_version;
    assert!(!post_threads(&mut with, threads[..1].to_vec(), Some(base)));
    assert_eq!(with.notes, notes);
    assert!(post_threads(&mut with, threads.clone(), Some(0)));
    assert_eq!(with.notes, notes);
    assert_eq!(merge_threads(&with.threads, &threads), with.threads);
    let stale = apply(&mut with, from_difit_json(&fixture("difit/import.json")).unwrap(), "");
    assert!(!stale.changed);
    assert_eq!(with.notes, notes);
}

#[test]
fn notes_persist_through_the_store_and_autosave() {
    let dir = scratch("notes-store");
    let path = SessionFile::path_in(&dir, "notes");
    let mut s = golden_session();
    three_notes(&mut s);
    SessionFile::save(&path, &s).unwrap();
    assert_eq!(SessionFile::load(&path).unwrap(), s);

    let auto = SessionFile::path_in(&dir, "auto");
    let shared = Arc::new(Shared::new(Session::default()));
    let saver = autosave(shared.clone(), auto.clone(), Duration::from_millis(20));
    three_notes(&mut shared.lock());
    saver.touch();
    saver.flush();
    let back = SessionFile::load(&auto).unwrap();
    assert_eq!(back.notes.len(), 3);
    assert_eq!(back.notes, shared.lock().notes);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn reply_by_thread_id_appends_or_warns() {
    let mut s = Session::default();
    let id = ops::add_thread(
        &mut s,
        "",
        "a.cs",
        Anchor::Range {
            side: Side::New,
            start: 3,
            end: 5,
        },
        "why?",
        Some("sumit"),
        None,
    );
    let reply = format!(r#"{{"type":"reply","threadId":"{id}","body":" because ","author":"claude"}}"#);
    let r = apply(&mut s, from_difit_json(&reply).unwrap(), "");
    assert!(r.changed && r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(s.threads[0].messages.len(), 2);
    assert_eq!(s.threads[0].messages[1].body, "because");
    assert!(
        !apply(&mut s, from_difit_json(&reply).unwrap(), "").changed,
        "the same reply twice lands once"
    );
    let ignored_anchor =
        format!(r#"{{"type":"reply","threadId":"{id}","filePath":"zzz","position":{{"side":"old","line":1}},"body":"more"}}"#);
    assert!(apply(&mut s, from_difit_json(&ignored_anchor).unwrap(), "").changed);
    assert_eq!(s.threads[0].messages.len(), 3);
    let r = apply(
        &mut s,
        from_difit_json(r#"{"type":"reply","threadId":"nope","body":"x"}"#).unwrap(),
        "",
    );
    assert_eq!(r.warnings, ["No thread with id nope."]);
    assert!(!r.changed);
    assert_eq!(
        from_difit_json(r#"{"type":"reply","threadId":" ","body":"x"}"#).unwrap_err(),
        "Invalid comment import field: threadId"
    );
    assert_eq!(
        from_difit_json(r#"{"type":"thread","threadId":"t","body":"x"}"#).unwrap_err(),
        "Invalid comment import field: filePath",
        "a thread import still needs its anchor"
    );
}

#[test]
fn repo_prefixed_paths_split_in_multi_repo_sessions() {
    let mut s = multi_repo();
    let t = r#"{"type":"thread","filePath":"server/src/x.cs","position":{"side":"new","line":4},"body":"hm"}"#;
    apply(&mut s, from_difit_json(t).unwrap(), "");
    assert_eq!(
        (s.threads[0].repo.as_str(), s.threads[0].file_path.as_str()),
        ("server", "src/x.cs")
    );
    let r = r#"{"type":"reply","filePath":"server/src/x.cs","position":{"side":"new","line":4},"body":"ok"}"#;
    let res = apply(&mut s, from_difit_json(r).unwrap(), "");
    assert!(res.warnings.is_empty(), "{:?}", res.warnings);
    assert_eq!(s.threads[0].messages.len(), 2);
    let unknown = r#"{"type":"thread","filePath":"docs/x.md","position":{"side":"new","line":1},"body":"d"}"#;
    apply(&mut s, from_difit_json(unknown).unwrap(), "");
    assert_eq!((s.threads[1].repo.as_str(), s.threads[1].file_path.as_str()), ("", "docs/x.md"));
    let named = r#"{"type":"thread","filePath":"server/y.cs","position":{"side":"new","line":1},"body":"n","repo":"app"}"#;
    apply(&mut s, from_difit_json(named).unwrap(), "");
    assert_eq!(
        (s.threads[2].repo.as_str(), s.threads[2].file_path.as_str()),
        ("app", "server/y.cs")
    );
}
