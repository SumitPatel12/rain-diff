//! Agent notes: parsing, add/dedupe/replace, removal, lookups, the overview's notes on the same
//! model, and the `GET /api/notes` body. Nothing here touches threads or `comments_version`.

use rain_core::notes::OVERVIEW_NOTE_PREFIX;
use rain_core::overview::{GraphRevision, OverviewNote};
use rain_core::{Anchor, Note, NoteKind, OverviewDocument, Session, Side};
use serde::Serialize;
use serde_json::Value;

use crate::clock::{new_id, now_iso};
use crate::import::{check_position, check_span};
use crate::ops::split_known_repo;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotesResult {
    /// Notes read, whether or not they were new.
    pub count: usize,
    /// The id each input ended up under, in input order (an existing note's id for a duplicate).
    pub ids: Vec<String>,
    pub changed: bool,
}

fn field(name: &str) -> String {
    format!("Invalid note field: {name}")
}

/// Parses one note object or an array of them, checked like comment imports. Bodies come back
/// trimmed; `id`, `createdAt` and `updatedAt` may be left out.
pub fn from_json(json: &str) -> Result<Vec<Note>, String> {
    let v: Value = serde_json::from_str(json).map_err(|_| "Invalid note JSON".to_owned())?;
    from_value(v)
}

pub fn from_value(v: Value) -> Result<Vec<Note>, String> {
    match v {
        Value::Array(items) => items.into_iter().map(entry).collect(),
        other => Ok(vec![entry(other)?]),
    }
}

fn entry(v: Value) -> Result<Note, String> {
    let Value::Object(o) = &v else {
        return Err("Note must be an object".into());
    };
    let kind = match o.get("kind").and_then(Value::as_str) {
        Some("story") => NoteKind::Story,
        Some("file") => NoteKind::File,
        Some("line") => NoteKind::Line,
        _ => return Err(field("kind")),
    };
    let has_path = o.get("filePath").and_then(Value::as_str).is_some_and(|p| !p.trim().is_empty());
    match kind {
        NoteKind::Story if has_path => return Err(field("filePath")),
        NoteKind::File | NoteKind::Line if !has_path => return Err(field("filePath")),
        _ => {}
    }
    if kind == NoteKind::Line {
        check_position(o.get("position"), field)?;
        check_span(o.get("span"), field)?;
    } else if o.contains_key("position") || o.contains_key("span") {
        return Err(field("position"));
    }
    if o.get("body").and_then(Value::as_str).is_none_or(|b| b.trim().is_empty()) {
        return Err(field("body"));
    }
    for name in ["id", "repo", "title", "author", "createdAt", "updatedAt"] {
        if o.get(name).is_some_and(|t| !t.is_string()) {
            return Err(field(name));
        }
    }
    let mut n: Note = serde_json::from_value(v).map_err(|e| format!("Invalid note: {e}"))?;
    n.body = n.body.trim().to_owned();
    Ok(n)
}

fn same_content(a: &Note, b: &Note) -> bool {
    a.kind == b.kind && a.repo == b.repo && a.file_path == b.file_path && a.anchor == b.anchor && a.body == b.body
}

fn replace_key(n: &Note) -> (NoteKind, &str, &str, Option<&str>) {
    (n.kind, &n.repo, &n.file_path, n.author.as_deref())
}

/// Adds notes in order. A note whose `id` exists updates that note; one with the same kind,
/// repo, path, anchor and body as an existing note is skipped. With `replace`, existing notes
/// with the kind, repo, path and author of any incoming note go first. Notes land in
/// `default_repo` unless they name one; with no repo at all a known `<repo>/` path prefix is
/// split off. Bumps the notes version once when anything changed.
pub fn add_notes(s: &mut Session, notes: Vec<Note>, default_repo: &str, replace: bool) -> NotesResult {
    let count = notes.len();
    let notes: Vec<Note> = notes.into_iter().map(|n| normalize(s, n, default_repo)).collect();
    let mut changed = false;
    if replace {
        let before = s.notes.len();
        s.notes.retain(|e| {
            notes.iter().all(|n| replace_key(n) != replace_key(e)) || notes.iter().any(|n| same_content(e, n) && e.title == n.title)
        });
        changed = s.notes.len() != before;
    }
    let mut ids = Vec::with_capacity(count);
    for mut n in notes {
        let now = now_iso();
        if !n.id.is_empty()
            && let Some(e) = s.notes.iter_mut().find(|e| e.id == n.id)
        {
            if !same_content(e, &n) || e.title != n.title || e.author != n.author {
                if (e.kind, &e.repo, &e.file_path, e.anchor) != (n.kind, &n.repo, &n.file_path, n.anchor) {
                    e.code_snapshot = n.code_snapshot.take();
                    e.commit = None;
                    e.anchor_state = Default::default();
                    e.anchor_how = None;
                    e.orphan = None;
                }
                e.kind = n.kind;
                e.repo = n.repo;
                e.file_path = n.file_path;
                e.anchor = n.anchor;
                e.title = n.title;
                e.body = n.body;
                e.author = n.author;
                e.updated_at = if n.updated_at.is_empty() { now } else { n.updated_at };
                changed = true;
            }
            ids.push(n.id);
            continue;
        }
        if let Some(e) = s.notes.iter().find(|e| same_content(e, &n)) {
            ids.push(e.id.clone());
            continue;
        }
        if n.id.is_empty() {
            n.id = new_id();
        }
        if n.created_at.is_empty() {
            n.created_at = now;
        }
        if n.updated_at.is_empty() {
            n.updated_at = n.created_at.clone();
        }
        ids.push(n.id.clone());
        s.notes.push(n);
        changed = true;
    }
    if changed {
        s.bump_notes();
    }
    NotesResult { count, ids, changed }
}

fn normalize(s: &Session, mut n: Note, default_repo: &str) -> Note {
    if n.id.starts_with(OVERVIEW_NOTE_PREFIX) {
        n.id.clear();
    }
    n.body = n.body.trim().to_owned();
    n.file_path = rain_core::paths::normalize(n.file_path.trim());
    match n.kind {
        NoteKind::Story => {
            n.file_path.clear();
            n.anchor = None;
        }
        NoteKind::File => n.anchor = None,
        NoteKind::Line => {}
    }
    if n.repo.is_empty() {
        if !default_repo.is_empty() {
            n.repo = default_repo.to_owned();
        } else if !n.file_path.is_empty() {
            (n.repo, n.file_path) = split_known_repo(s, &n.file_path);
        }
    }
    n
}

/// Line notes without a snapshot take one from the loaded diff, so they re-anchor like threads
/// ([`crate::ops::reanchor_notes`]). Returns how many did; bumps the notes version when any did.
pub fn snapshot_notes(s: &mut Session, excerpt: crate::import::Excerpt<'_>) -> usize {
    let mut n = 0;
    for note in s.notes.iter_mut().filter(|x| x.kind == NoteKind::Line && x.code_snapshot.is_none()) {
        if let Some(a) = note.anchor
            && let Some(snap) = excerpt(&note.repo, &note.file_path, &a)
        {
            note.code_snapshot = Some(snap);
            n += 1;
        }
    }
    if n > 0 {
        s.bump_notes();
    }
    n
}

/// Removes a stored note. Overview notes (`overview:` ids) belong to the overview and stay.
pub fn remove_note(s: &mut Session, id: &str) -> bool {
    if id.starts_with(OVERVIEW_NOTE_PREFIX) {
        return false;
    }
    let before = s.notes.len();
    s.notes.retain(|n| n.id != id);
    let changed = s.notes.len() != before;
    if changed {
        s.bump_notes();
    }
    changed
}

/// File and line notes of `repo`/`path`, compared as `<repo>/<path>` keys so a note stored with
/// an empty repo and a joined path matches too.
pub fn notes_for_file<'a>(s: &'a Session, repo: &str, path: &str) -> Vec<&'a Note> {
    let key = rain_core::paths::join(repo, path);
    s.notes.iter().filter(|n| n.kind != NoteKind::Story && n.key() == key).collect()
}

/// The overview's `notes[]` as line notes: id `overview:<id>`, author `overview`, the overview's
/// path (a `<repo>/<path>` key) with an empty repo, the old side for `revision: "base"`.
pub fn from_overview(doc: &OverviewDocument) -> Vec<Note> {
    doc.notes.iter().map(overview_note).collect()
}

/// The overview note `overview:<id>` on the note model, without converting the others.
pub fn overview_note_by_id(doc: &OverviewDocument, id: &str) -> Option<Note> {
    let id = id.strip_prefix(OVERVIEW_NOTE_PREFIX)?;
    doc.notes.iter().find(|o| o.id == id).map(overview_note)
}

fn overview_note(o: &OverviewNote) -> Note {
    let side = if o.revision == Some(GraphRevision::Base) {
        Side::Old
    } else {
        Side::New
    };
    let end = o.end_line.unwrap_or(o.start_line).max(o.start_line);
    let anchor = if end == o.start_line {
        Anchor::Line { side, line: o.start_line }
    } else {
        Anchor::Range {
            side,
            start: o.start_line,
            end,
        }
    };
    Note {
        id: format!("{OVERVIEW_NOTE_PREFIX}{}", o.id),
        kind: NoteKind::Line,
        repo: String::new(),
        file_path: o.path.clone(),
        anchor: Some(anchor),
        title: None,
        body: o.text.clone(),
        author: Some("overview".into()),
        ..Note::default()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NotesJson<'a> {
    notes_version: u64,
    notes: Vec<&'a Note>,
}

/// `GET /api/notes`: `{"notesVersion": notes_version, "notes": [...]}`, stored notes then the
/// overview's, narrowed to a repo (story notes of every repo kept) and to a `<repo>/<path>` key
/// (file and line notes only) when given.
pub fn notes_json_string(s: &Session, repo: Option<&str>, file_key: Option<&str>) -> String {
    let overview = s.overview.as_ref().map(from_overview).unwrap_or_default();
    let notes = s
        .notes
        .iter()
        .chain(overview.iter())
        .filter(|n| {
            repo.is_none_or(|r| {
                n.repo == r || (n.repo.is_empty() && (n.kind == NoteKind::Story || n.file_path.starts_with(&format!("{r}/"))))
            })
        })
        .filter(|n| file_key.is_none_or(|k| n.kind != NoteKind::Story && n.key() == k))
        .collect();
    serde_json::to_string(&NotesJson {
        notes_version: s.notes_version,
        notes,
    })
    .unwrap_or_default()
}
