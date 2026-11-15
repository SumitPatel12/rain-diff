//! The file filter: the list's chips and text (the tree's filter) narrow every section and the
//! diff to a subset of the files.

use rain_core::diff::Side;
use rain_tree::TreeFilter;
use rune::Tx;

use super::{App, Subset};

#[derive(Default)]
pub struct FilterState {
    /// The filtered files' diff while a filter is active.
    pub subset: Option<Subset>,
}

impl App {
    /// The tree's filter changed: the diff follows it, keeping the reader's file when it still
    /// matches.
    pub fn on_filter_change(&mut self, _f: &TreeFilter, tx: &mut Tx) {
        let keep = self.reading_place();
        if !self.tree.filter_edit.focused {
            // A pressed chip keeps the focus, and Space would press it again instead of walking.
            tx.blur();
        }
        if !self.follow_tree_filter() {
            return;
        }
        match keep.and_then(|(g, line)| self.local(g).map(|l| (l, line))) {
            Some((l, Some(at))) => self.active_mut().scroll_to_line(l, at, tx),
            Some((l, None)) => self.active_mut().scroll_to_file(l, tx),
            None if !self.active().files().is_empty() => self.active_mut().scroll_to_file(0, tx),
            None => {}
        }
    }

    /// Sets the file filter from outside the tree (palette, scenes, a stale subset after a
    /// session change); the diff moves on the next frame.
    pub fn apply_filter(&mut self, f: &TreeFilter) {
        let keep = self.reading_place();
        self.tree.apply(f);
        if !self.follow_tree_filter() {
            return;
        }
        let g = keep.map(|(g, _)| g).filter(|&g| self.local(g).is_some());
        if let Some(g) = g.or_else(|| (!self.active().files().is_empty()).then(|| self.global(0))) {
            self.goto_later(self.keys[g].clone());
        }
    }

    /// Rebuilds the filter subset from the tree's shown files; true when it changed.
    fn follow_tree_filter(&mut self) -> bool {
        if self.tree.filter.is_empty() {
            return self.filter.subset.take().is_some();
        }
        let files: Vec<usize> = (0..self.keys.len()).filter(|&g| self.tree.is_shown(&self.keys[g])).collect();
        if self.filter.subset.as_ref().is_some_and(|s| s.files == files) {
            return false;
        }
        self.filter.subset = Some(self.filter_subset(files));
        self.rebuild_annotations();
        true
    }

    /// The subset for `files`, carrying over the full diff's open and collapsed files.
    fn filter_subset(&self, files: Vec<usize>) -> Subset {
        let mut sub = Subset::new(self, files, self.filter.subset.as_ref());
        let differ: Vec<(usize, bool)> = sub
            .files
            .iter()
            .enumerate()
            .map(|(l, &g)| (l, self.diff.collapsed(g)))
            .filter(|&(l, c)| sub.diff.collapsed(l) != c)
            .collect();
        for (l, c) in differ {
            sub.diff.set_collapsed(l, c);
        }
        sub
    }

    /// The reader's file and cursor line, when the cursor is on it.
    fn reading_place(&self) -> Option<(usize, Option<rain_diffview::LineRef>)> {
        let g = self.current()?;
        let line = self.active().cursor().filter(|&(l, _)| self.global(l) == g).map(|(_, at)| at);
        Some((g, line))
    }

    /// Whether a file filter is set, shown or paused.
    pub fn filter_active(&self) -> bool {
        self.filter.subset.is_some() || !self.tree.filter.is_empty()
    }

    /// Drops the filter; true when there was one to drop.
    pub fn clear_filter(&mut self) -> bool {
        if !self.filter_active() {
            return false;
        }
        let keep = self.reading_place();
        self.tree.apply(&TreeFilter::default());
        self.filter.subset = None;
        if let Some((g, line)) = keep {
            let key = self.keys[g].clone();
            self.goto_later(match line {
                Some(at) if at.side == Side::New => format!("{key}:{}", at.line),
                _ => key,
            });
        }
        self.status = "Filter cleared".into();
        true
    }

    /// Clears the filter before a jump to full-list file `g` the filter hides.
    pub(crate) fn leave_filter_for(&mut self, g: usize) {
        if self.filter.subset.as_ref().is_none_or(|s| s.files.contains(&g)) {
            return;
        }
        self.tree.apply(&TreeFilter::default());
        self.filter.subset = None;
        self.status = format!("Filter cleared to show {}", self.keys[g]);
    }

    /// What `Space` says when nothing is left to walk.
    pub(crate) fn all_viewed_status(&self) -> String {
        match self.filter.subset {
            Some(_) => "Every filtered file is viewed (Esc clears the filter)".into(),
            _ => "Everything is viewed".into(),
        }
    }

    /// `Ctrl+L`: start filtering files, with the list open (an overlay while zoomed).
    pub fn filter_files(&mut self, _tx: &mut Tx) {
        if self.layout.is_zoomed() {
            self.open_overlay(super::layout::Overlay::Files);
        } else if !self.sidebar.is_open() {
            self.toggle_list();
        }
        self.unfold_section(super::board::FILES);
        self.tree.filter_edit.focused = true;
    }
}
