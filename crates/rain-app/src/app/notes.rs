//! Notes beside the diff: agent story, file and line notes from the session, plus the overview's
//! intent notes mapped onto the same model at render time (never stored).

use rain_core::{Anchor, Note, NoteKind, Session};

use super::App;

/// Whether `fresh` holds different notes from `old`; [`App::sync_session`] calls
/// [`App::on_notes_changed`] when it does.
pub(crate) fn notes_changed(old: &Session, fresh: &Session) -> bool {
    old.notes_version != fresh.notes_version || old.notes != fresh.notes
}

/// The `expanded` entry of a note's "Show all" state; prefixed so it never meets a thread id.
pub(crate) fn expanded_key(id: &str) -> String {
    format!("note:{id}")
}

impl App {
    /// Stored notes then the overview's, in that order.
    pub fn all_notes(&self) -> Vec<Note> {
        let mut out = self.mirror.notes.clone();
        if let Some(l) = &self.overview {
            out.extend(rain_comments::notes::from_overview(&l.document));
        }
        out
    }

    pub fn story_notes(&self) -> Vec<&Note> {
        self.mirror.notes.iter().filter(|n| n.kind == NoteKind::Story).collect()
    }

    /// Full-list index of a file or line note's file.
    pub fn file_of_note(&self, n: &Note) -> Option<usize> {
        if n.kind == NoteKind::Story {
            return None;
        }
        if let Some(i) = self.index_of(&n.key()) {
            return Some(i);
        }
        if !n.repo.is_empty() {
            return None;
        }
        if self.repos.len() == 1 {
            return self.index_of(&rain_core::paths::join(&self.repos[0].id, &n.file_path));
        }
        self.diff.files().iter().position(|f| f.path == n.file_path)
    }

    pub fn file_notes_for(&self, key: &str) -> Vec<&Note> {
        let g = self.index_of(key);
        self.mirror
            .notes
            .iter()
            .filter(|n| n.kind == NoteKind::File && g.is_some() && self.file_of_note(n) == g)
            .collect()
    }

    /// A note by id, stored or overview-derived (`overview:<id>`).
    pub fn note(&self, id: &str) -> Option<Note> {
        if let Some(n) = self.mirror.notes.iter().find(|n| n.id == id) {
            return Some(n.clone());
        }
        let l = self.overview.as_ref()?;
        rain_comments::notes::overview_note_by_id(&l.document, id)
    }

    pub fn note_expanded(&self, id: &str) -> bool {
        self.expanded.contains(&expanded_key(id))
    }

    pub fn toggle_note_expanded(&mut self, id: &str) {
        let k = expanded_key(id);
        if !self.expanded.remove(&k) {
            self.expanded.insert(k);
        }
    }

    /// Runs after the session's notes changed (from `before` notes), before the annotations are
    /// rebuilt.
    pub(crate) fn on_notes_changed(&mut self, before: usize) {
        let live: std::collections::BTreeSet<String> = self.mirror.notes.iter().map(|n| expanded_key(&n.id)).collect();
        self.expanded
            .retain(|k| !k.starts_with("note:") || k.starts_with("note:overview:") || live.contains(k));
        self.status = match self.mirror.notes.len().cmp(&before) {
            std::cmp::Ordering::Greater => "Note added".into(),
            std::cmp::Ordering::Less => "Note removed".into(),
            std::cmp::Ordering::Equal => "Note updated".into(),
        };
    }

    /// `(file, anchor, annotation id)` for every line note placed on a line; ids are `note:<id>`.
    pub(crate) fn note_annotations(&self) -> Vec<(usize, Anchor, String)> {
        self.all_notes()
            .into_iter()
            .filter(|n| n.kind == NoteKind::Line)
            .filter_map(|n| {
                let a = n.anchor?;
                let f = self.file_of_note(&n)?;
                Some((f, a, format!("note:{}", n.id)))
            })
            .collect()
    }
}
