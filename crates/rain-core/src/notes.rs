//! Agent-authored notes: context for the reviewer, kept apart from comment threads so they never
//! reach comment readback or export.

use serde::{Deserialize, Serialize};

use crate::comments::{Anchor, AnchorHow, AnchorState, CodeSnapshot, Orphan};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteKind {
    /// The whole change.
    #[default]
    Story,
    /// One file.
    File,
    /// Lines of one file.
    Line,
}

impl NoteKind {
    pub fn label(&self) -> &'static str {
        match self {
            NoteKind::Story => "story",
            NoteKind::File => "file",
            NoteKind::Line => "line",
        }
    }
}

/// `{"id","kind","repo"?,"filePath"?,"position"?,"span"?,"title"?,"body","author"?,"createdAt","updatedAt"}`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    #[serde(default)]
    pub id: String,
    pub kind: NoteKind,
    /// Repo id; empty in single-repo sessions.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo: String,
    /// Empty for story notes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file_path: String,
    /// Set for line notes only.
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Markdown.
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    /// The anchored lines when a line note was posted, so it re-anchors like a thread.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_snapshot: Option<CodeSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(default, skip_serializing_if = "is_placed")]
    pub anchor_state: AnchorState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor_how: Option<AnchorHow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orphan: Option<Orphan>,
}

fn is_placed(s: &AnchorState) -> bool {
    *s == AnchorState::Placed
}

impl Note {
    /// `<repo>/<path>` as the tree and the overview key files; empty for story notes.
    pub fn key(&self) -> String {
        crate::paths::join(&self.repo, &self.file_path)
    }

    /// Derived from the overview document rather than stored in the session.
    pub fn is_overview(&self) -> bool {
        self.id.starts_with(OVERVIEW_NOTE_PREFIX)
    }
}

pub const OVERVIEW_NOTE_PREFIX: &str = "overview:";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::Side;

    fn note(kind: NoteKind, path: &str, anchor: Option<Anchor>) -> Note {
        Note {
            id: "n1".into(),
            kind,
            repo: String::new(),
            file_path: path.into(),
            anchor,
            title: None,
            body: "**why**".into(),
            author: Some("claude".into()),
            created_at: "2026-11-07T00:00:00.000Z".into(),
            updated_at: "2026-11-07T00:00:00.000Z".into(),
            ..Note::default()
        }
    }

    fn round_trip(n: &Note) -> String {
        let s = serde_json::to_string(n).unwrap();
        assert_eq!(&serde_json::from_str::<Note>(&s).unwrap(), n);
        s
    }

    #[test]
    fn story_note_round_trips_without_path_or_anchor() {
        let mut n = note(NoteKind::Story, "", None);
        n.title = Some("Summary".into());
        assert_eq!(
            round_trip(&n),
            r#"{"id":"n1","kind":"story","title":"Summary","body":"**why**","author":"claude","createdAt":"2026-11-07T00:00:00.000Z","updatedAt":"2026-11-07T00:00:00.000Z"}"#
        );
    }

    #[test]
    fn file_note_round_trips_with_repo() {
        let mut n = note(NoteKind::File, "src/x.cs", None);
        n.repo = "server".into();
        let s = round_trip(&n);
        assert!(s.contains(r#""repo":"server","filePath":"src/x.cs""#), "{s}");
        assert!(!s.contains("position"));
        assert_eq!(n.key(), "server/src/x.cs");
    }

    #[test]
    fn line_note_round_trips_line_range_and_span() {
        for a in [
            Anchor::Line { side: Side::New, line: 12 },
            Anchor::Range {
                side: Side::Old,
                start: 3,
                end: 7,
            },
            Anchor::Span {
                side: Side::New,
                start_line: 4,
                start_col: 1,
                end_line: 5,
                end_col: 2,
            },
        ] {
            let s = round_trip(&note(NoteKind::Line, "a.ts", Some(a)));
            assert!(s.contains(r#""position":{"side":"#), "{s}");
        }
        let s = round_trip(&note(NoteKind::Line, "a.ts", Some(Anchor::Line { side: Side::New, line: 12 })));
        assert!(s.contains(r#""position":{"side":"new","line":12}"#), "{s}");
    }

    #[test]
    fn unknown_fields_and_missing_ids_are_tolerated() {
        let n: Note = serde_json::from_str(r#"{"kind":"file","filePath":"a.ts","body":"b","extra":1}"#).unwrap();
        assert_eq!(n.kind, NoteKind::File);
        assert!(n.id.is_empty() && n.created_at.is_empty() && n.anchor.is_none());
        let n: Note =
            serde_json::from_str(r#"{"kind":"line","filePath":"a.ts","position":{"side":"old","line":{"start":2,"end":4}},"body":"b"}"#)
                .unwrap();
        assert_eq!(
            n.anchor,
            Some(Anchor::Range {
                side: Side::Old,
                start: 2,
                end: 4
            })
        );
    }
}
