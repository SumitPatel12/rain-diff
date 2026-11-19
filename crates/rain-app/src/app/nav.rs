//! Moving around: files, lines, threads, `--goto` specs, the `Space` walk and viewed marks.

use rain_comments::ops;
use rain_core::diff::Side;
use rain_diffview::LineRef;
use rune::Tx;

use super::App;
use super::board::Pick;
use super::overview::ReaderTab;
use crate::load::LARGE_FILE_LINES;

impl App {
    /// Shows full-list file `g` at the top, leaving a filter that hides it; a jump to another
    /// file is remembered for `Alt+←`.
    pub fn goto_file(&mut self, g: usize, tx: &mut Tx) {
        if self.current() != Some(g) {
            self.record_jump();
        }
        self.show_file(g, tx);
    }

    /// [`App::goto_file`] without the history (walking, going back).
    pub(crate) fn show_file(&mut self, g: usize, tx: &mut Tx) {
        self.set_reader_tab(ReaderTab::Changes);
        self.leave_filter_for(g);
        self.single_on(g);
        if let Some(l) = self.local(g) {
            let big = self.active().files()[l].lines.len() > LARGE_FILE_LINES;
            if self.active().collapsed(l) && !big {
                self.active_mut().set_collapsed(l, false);
            }
            self.active_mut().scroll_to_file(l, tx);
            if self.active().cursor().is_some_and(|(f, _)| f != l) {
                self.active_mut().set_cursor(None);
            }
        }
        let key = self.keys[g].clone();
        self.tree.reveal(&key, tx);
    }

    pub fn goto_line(&mut self, g: usize, at: LineRef, tx: &mut Tx) {
        self.set_reader_tab(ReaderTab::Changes);
        if self.current() != Some(g) {
            self.record_jump();
        }
        self.leave_filter_for(g);
        self.single_on(g);
        if let Some(l) = self.local(g) {
            self.active_mut().scroll_to_line(l, at, tx);
            if self.active().cursor() != Some((l, at)) {
                self.active_mut().scroll_to_file(l, tx);
                self.status = format!("Line {} is outside the diff; showing the file", at.line);
            }
        }
        let key = self.keys[g].clone();
        self.tree.reveal(&key, tx);
    }

    pub fn goto_key(&mut self, key: &str, tx: &mut Tx) {
        if let Some(i) = self.index_of(key) {
            self.goto_file(i, tx);
        }
    }

    /// `<repo>/<path>:<line>` (line optional).
    pub fn goto_spec(&mut self, spec: &str, tx: &mut Tx) {
        let (path, line) = match spec.rsplit_once(':') {
            Some((p, l)) if l.chars().all(|c| c.is_ascii_digit()) && !l.is_empty() => (p, l.parse::<u32>().ok()),
            _ => (spec, None),
        };
        let path = rain_core::paths::normalize(path);
        let found = self.index_of(&path).or_else(|| {
            self.diff
                .files()
                .iter()
                .position(|f| f.path == path || path.ends_with(&format!("/{}", f.path)))
        });
        match (found, line) {
            (Some(g), Some(n)) => self.goto_line(g, LineRef::new(Side::New, n), tx),
            (Some(g), None) => self.goto_file(g, tx),
            (None, _) => self.status = format!("--goto: no file {path} in this diff"),
        }
    }

    /// Jumps to `<repo>/<path>:<line>` on the next frame (needs a `Tx`).
    pub fn goto_later(&mut self, spec: String) {
        self.pending_goto = Some(spec);
    }

    pub fn goto_thread_later(&mut self, id: &str) {
        self.pending_goto = Some(format!("thread:{id}"));
    }

    pub fn goto_thread(&mut self, id: &str, tx: &mut Tx) {
        let Some(t) = ops::thread(&self.mirror, id).cloned() else { return };
        let Some(g) = self.file_of_thread(&t) else {
            self.status = format!("{} is not in this diff", t.file_path);
            return;
        };
        self.focused_thread = Some(t.id.clone());
        self.expanded.insert(t.id.clone());
        self.goto_line(g, LineRef::new(t.anchor.side(), t.anchor.lines().0), tx);
    }

    /// File-at-a-time follows a jump to `g`.
    fn single_on(&mut self, g: usize) {
        if self.single_mode() && self.board.pick != Pick::File(g) {
            self.show_pick(Pick::File(g));
        }
    }

    /// The order `Space` walks: the list's order, narrowed by the filter, accepted groups out.
    pub fn walk_order(&self) -> Vec<usize> {
        self.list_order()
    }

    /// Marks the current file viewed and moves to the next unviewed one in [`App::walk_order`];
    /// in file-at-a-time, done and the next item.
    pub fn viewed_and_next(&mut self, tx: &mut Tx) {
        if self.single_mode() {
            self.done_and_next(tx);
            return;
        }
        let Some(cur) = self.current() else { return };
        self.set_viewed(cur, true);
        let order = self.walk_order();
        let at = order.iter().position(|&g| g == cur).unwrap_or(0);
        let next = order[at + 1..]
            .iter()
            .chain(order[..at].iter())
            .copied()
            .find(|&g| !self.is_viewed(g));
        match next {
            Some(g) => {
                self.show_file(g, tx);
                self.active_mut().set_cursor(None);
                self.status = format!("Viewed. Next: {}", self.keys[g]);
            }
            None => self.status = self.all_viewed_status(),
        }
    }

    pub fn toggle_viewed_current(&mut self, tx: &mut Tx) {
        if let Some(cur) = self.current() {
            let on = !self.is_viewed(cur);
            self.mark_viewed(cur, on, tx);
        }
    }

    /// Marks file `g` viewed or not; marking it brings the next open file after it to the top.
    pub fn mark_viewed(&mut self, g: usize, on: bool, tx: &mut Tx) {
        self.set_viewed(g, on);
        if on {
            self.top_next_open(g, tx);
        }
    }

    /// Puts the first file after `g` in the reader that isn't folded at the top.
    pub(crate) fn top_next_open(&mut self, g: usize, tx: &mut Tx) {
        if self.single_mode() {
            return;
        }
        let Some(l) = self.local(g) else { return };
        let d = self.active();
        let Some(next) = (l + 1..d.files().len()).find(|&n| !d.collapsed(n)) else {
            return;
        };
        self.active_mut().scroll_to_file(next, tx);
        let key = self.keys[self.global(next)].clone();
        self.tree.reveal(&key, tx);
    }

    /// Marks every file the filter shows viewed, in one change.
    pub fn mark_all_viewed(&mut self) {
        let keys: Vec<String> = (0..self.keys.len())
            .filter(|&g| !self.is_viewed(g) && self.tree.is_shown(&self.keys[g]))
            .map(|g| self.keys[g].clone())
            .collect();
        if keys.is_empty() {
            self.status = "Every file is already viewed".into();
            return;
        }
        self.tree.set_viewed_many(keys.iter().map(|k| (k.as_str(), true)));
        self.persist_viewed_keys(&keys);
        for k in &keys {
            if let Some(g) = self.index_of(k) {
                self.fold_viewed(g, true);
            }
        }
        self.status = format!("Marked {} file{} viewed", keys.len(), if keys.len() == 1 { "" } else { "s" });
    }
}
