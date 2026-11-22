//! Notes beside the diff: agent story, file and line notes from the session, plus the overview's
//! intent notes mapped onto the same model at render time (never stored).

use rain_core::{Anchor, Note, NoteKind};

use super::App;

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

    pub fn note_popped(&self, id: &str) -> bool {
        self.popped_note.as_deref() == Some(id)
    }

    /// What the Intent window shows for note `id`.
    pub fn note_content(&self, id: &str) -> Option<crate::note_window::NoteContent> {
        let n = self.note(id)?;
        let place = self.file_of_note(&n).map(|g| match n.anchor {
            Some(a) => {
                let (lo, hi) = a.lines();
                if lo == hi {
                    format!("{}:{lo}", self.keys[g])
                } else {
                    format!("{}:{lo}-{hi}", self.keys[g])
                }
            }
            None => self.keys[g].clone(),
        });
        let mut meta = Vec::new();
        if let Some(a) = n.author.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
            meta.push(a.to_owned());
        }
        if let Some(a) = n.anchor {
            let (lo, hi) = a.lines();
            meta.push(if lo == hi { format!("L{lo}") } else { format!("L{lo}–{hi}") });
        }
        Some(crate::note_window::NoteContent {
            id: n.id.clone(),
            label: if n.is_overview() { "Intent".into() } else { "Note".into() },
            meta: meta.iter().map(|m| format!("· {m}")).collect::<Vec<_>>().join(" "),
            title: n.title.clone().filter(|t| !t.trim().is_empty()),
            body: n.body.clone(),
            place,
        })
    }

    /// Pop out: the note opens in the Intent window (replacing any other) and collapses inline.
    pub fn pop_out_note(&mut self, id: &str) {
        let Some(c) = self.note_content(id) else { return };
        if crate::note_window::show(c, self.seen_scheme.get()) {
            self.status = "Note opened in its own window · Bring back returns it".into();
        }
        self.popped_note = Some(id.to_owned());
    }

    /// Bring back: the note shows inline again and the window closes.
    pub fn bring_back_note(&mut self, id: &str) {
        if self.note_popped(id) {
            self.popped_note = None;
            crate::note_window::hide();
        }
    }

    /// The Intent window's Show in diff: the Changes, scrolled to the note's line.
    pub fn goto_note(&mut self, id: &str, tx: &mut rune::Tx) {
        let Some(n) = self.note(id) else { return };
        let Some(g) = self.file_of_note(&n) else { return };
        self.set_reader_tab(super::ReaderTab::Changes);
        self.folded.remove(&format!("note:{id}"));
        if let (Some(l), Some(a)) = (self.local(g), n.anchor) {
            let at = rain_diffview::LineRef::new(a.side(), a.end_line());
            self.active_mut().scroll_to_line(l, at, tx);
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
