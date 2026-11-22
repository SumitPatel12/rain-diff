//! `Ctrl+P`: the picker. Fuzzy over the one ordered list (files, groups) plus the Brief's
//! sections, the Map's nodes, the tour's steps and the threads waiting on you.

use std::collections::HashSet;
use std::ops::Range;

use rain_comments::ops;
use rain_core::fuzzy::{Fit, Matcher, Query};
use rain_triage::Group;
use rune::{Key, KeyEvent, TextEdit, Tx};

use super::board::Pick;
use super::layout::Overlay;
use super::{App, ME};
use crate::settings::PICKER_MIN;
use crate::theme::space;

/// Rows the picker lists at most.
pub const PICKER_ROWS: usize = 60;
/// The panel's distance below the toolbar.
pub const PICKER_TOP: f32 = 34.0;
/// Room the panel leaves at the window's sides and bottom.
const PICKER_MARGIN: f32 = 20.0;

#[derive(Default)]
pub struct PickerState {
    pub edit: TextEdit,
    pub sel: usize,
    /// The panel's size when a resize drag began.
    pub drag_from: Option<(f32, f32)>,
    /// Normal mode's first `g` of `gg`.
    pub pending_g: bool,
    pub cache: std::cell::RefCell<Cache>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    File(usize),
    Group(Group),
    Brief(String),
    Node(String),
    Step(String),
    Thread(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// `File`, `Group`, `Brief`, `Map`, `Tour`, `Waiting`.
    pub kind: &'static str,
    pub label: String,
    /// The folder, a count or the section's first words.
    pub detail: String,
    pub target: Target,
}

/// How well `query` fits `hay`, all of it a name, higher is better; `None` when it doesn't.
#[cfg(test)]
pub fn fuzzy(query: &str, hay: &str) -> Option<i32> {
    Matcher::default().fit(&Query::new(query), hay, 0).score()
}

/// The file the reader is on gives up one part in this many of its score. fff takes a quarter;
/// an eighth still lets a clearly better fit lead when it is the file you are on.
const CURRENT_FILE: i32 = 8;

/// What orders rows of one score, least first: a file not yet viewed, then one with an open
/// thread, then the one with more changed lines. The text alone decides between scores.
type Tie = (bool, bool, std::cmp::Reverse<u32>);

/// What the entries and their order read, so they are rebuilt only when it changes.
type Source = (u64, u32, u64, rain_tree::TreeFilter, Option<usize>);

/// The entries with the text each is matched on, kept while what they show stands; and the
/// last query's rows, so a redraw or a move of the selection matches nothing again.
#[derive(Default)]
pub struct Cache {
    source: Option<Source>,
    entries: Vec<Entry>,
    /// The rows of an empty query, and whether the first is the file the last jump left.
    blank: (Vec<u32>, bool),
    /// `detail/label` per entry, and where the label starts: made at the first query, so
    /// opening the picker costs no more than listing its entries.
    hay: Vec<(String, usize)>,
    /// Per entry, what orders equal scores: made at the first query, as `hay` is.
    ties: Vec<Tie>,
    query: Option<String>,
    /// The entries holding all the query's letters: all that a query typed on from it can
    /// match. `None` is every entry.
    held: Option<Vec<u32>>,
    rows: Vec<u32>,
    /// Per row, the byte ranges of its label the query matched.
    marks: Vec<Vec<Range<usize>>>,
    /// Per file row, the byte ranges of its folder the query matched.
    folder_marks: Vec<Vec<Range<usize>>>,
    matcher: Matcher,
    /// Passes over the entries so far.
    pub scans: u32,
    /// Rows whose matched letters were looked up so far.
    pub marked: u32,
}

impl Cache {
    fn refresh(&mut self, s: &App) {
        let source = (
            s.seen_version,
            s.reloader.installs(),
            s.seen_overview_version,
            s.tree.filter.clone(),
            s.current(),
        );
        if self.source.as_ref() == Some(&source) {
            return;
        }
        self.entries = s.picker_entries();
        self.blank = s.picker_blank(&self.entries);
        self.hay.clear();
        self.ties.clear();
        self.source = Some(source);
        self.query = None;
    }

    fn fill_hay(&mut self) {
        if self.hay.len() == self.entries.len() {
            return;
        }
        self.hay = self
            .entries
            .iter()
            .map(|e| {
                let mut text = String::with_capacity(e.detail.len() + 1 + e.label.len());
                text.push_str(&e.detail);
                text.push('/');
                text.push_str(&e.label);
                (text, e.detail.len() + 1)
            })
            .collect();
    }

    /// One pass of `query` over the entries `from` (all of them for `None`): those holding all
    /// its letters, and the score of each that fits.
    fn scan(&mut self, query: &Query, from: Option<Vec<u32>>) -> (Vec<u32>, Vec<(i32, u32)>) {
        self.scans += 1;
        self.fill_hay();
        let from = from.unwrap_or_else(|| (0..self.entries.len() as u32).collect());
        let here = self.source.as_ref().and_then(|s| s.4);
        let (mut held, mut scored) = (Vec::new(), Vec::new());
        for i in from {
            let (hay, name_at) = &self.hay[i as usize];
            let fit = self.matcher.fit(query, hay, *name_at);
            if fit != Fit::Miss {
                held.push(i);
            }
            if let Fit::Hit(sc) = fit {
                let current = here.is_some_and(|g| self.entries[i as usize].target == Target::File(g));
                scored.push((if current { sc - sc / CURRENT_FILE } else { sc }, i));
            }
        }
        (held, scored)
    }

    /// Puts the entries matching `q` in `rows`, best first (by [`Tie`], then list order, on
    /// ties), `PICKER_ROWS` at most.
    fn find(&mut self, s: &App, q: &str) {
        if self.query.as_deref() == Some(q) {
            return;
        }
        let from = match &self.query {
            Some(last) if q.starts_with(last.as_str()) => self.held.take(),
            _ => None,
        };
        self.query = Some(q.to_owned());
        self.marks.clear();
        self.folder_marks.clear();
        if q.is_empty() {
            self.rows.clone_from(&self.blank.0);
            self.held = None;
            return;
        }
        let mut query = Query::new(q);
        let (held, mut scored) = self.scan(&query, from);
        // Nothing fits as typed: once more over everything, forgiving a typo.
        if scored.is_empty()
            && let Some(loose) = query.loosened()
        {
            scored = self.scan(&loose, None).1;
            query = loose;
        }
        if self.ties.len() != self.entries.len() {
            self.ties = s.picker_ties(&self.entries);
        }
        let ties = &self.ties;
        let best_first = |a: &(i32, u32), b: &(i32, u32)| {
            let tie = || ties[a.1 as usize].cmp(&ties[b.1 as usize]);
            b.0.cmp(&a.0).then_with(tie).then(a.1.cmp(&b.1))
        };
        if scored.len() > PICKER_ROWS {
            scored.select_nth_unstable_by(PICKER_ROWS, best_first);
            scored.truncate(PICKER_ROWS);
        }
        scored.sort_unstable_by(best_first);
        self.held = Some(held);
        self.rows = scored.into_iter().map(|(_, i)| i).collect();
        let mut found = Vec::new();
        for &i in &self.rows {
            let (hay, name_at) = &self.hay[i as usize];
            self.matcher.marks(&query, hay, *name_at, &mut found);
            self.marked += 1;
            let in_label = found.iter().filter(|r| r.end > *name_at);
            self.marks
                .push(in_label.map(|r| r.start.max(*name_at) - name_at..r.end - name_at).collect());
            let folder = name_at - 1;
            let drawn = matches!(self.entries[i as usize].target, Target::File(_));
            let in_folder = found.iter().filter(|r| drawn && r.start < folder);
            self.folder_marks.push(in_folder.map(|r| r.start..r.end.min(folder)).collect());
        }
    }
}

impl App {
    /// `Ctrl+P`: opens the picker with the keyboard in its box.
    pub fn open_picker(&mut self, _tx: &mut Tx) {
        self.show_picker("");
    }

    pub fn show_picker(&mut self, query: &str) {
        self.picker.edit = TextEdit::default();
        self.picker.edit.text = query.to_owned();
        self.picker.edit.set_caret(query.len());
        self.picker.edit.focused = true;
        self.picker.sel = 0;
        self.picker.pending_g = false;
        // Rebuilt on opening, for whatever changed while it was closed.
        *self.picker.cache.borrow_mut() = Cache::default();
        self.open_overlay(Overlay::Picker);
    }

    pub fn close_picker(&mut self) {
        self.picker.edit.focused = false;
        self.close_overlay(Overlay::Picker);
    }

    /// Everything the picker can go to, in list order.
    fn picker_entries(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        for g in self.list_order() {
            let key = &self.keys[g];
            let (dir, name) = key.rsplit_once('/').unwrap_or(("", key.as_str()));
            out.push(Entry {
                kind: "File",
                label: name.to_owned(),
                detail: dir.to_owned(),
                target: Target::File(g),
            });
        }
        for g in Group::ALL.into_iter().filter(|g| !g.is_change()) {
            let n = (0..self.keys.len()).filter(|&f| self.group_of(f) == g).count();
            if n > 0 {
                out.push(Entry {
                    kind: "Group",
                    label: g.label().to_owned(),
                    detail: format!("{n} files"),
                    target: Target::Group(g),
                });
            }
        }
        if let Some(b) = &self.mirror.brief {
            for s in b.sections() {
                out.push(Entry {
                    kind: "Brief",
                    label: s.title.to_owned(),
                    detail: rain_comments::label::first_sentence(s.text, 60),
                    target: Target::Brief(s.id.to_owned()),
                });
            }
            for (i, st) in b.tour_steps().into_iter().enumerate() {
                out.push(Entry {
                    kind: "Tour",
                    label: format!("{}. {}", i + 1, st.title),
                    detail: st.cite.path.clone(),
                    target: Target::Step(match st.cite.lines {
                        Some((a, _)) => format!("{}:{a}", st.cite.path),
                        None => st.cite.path.clone(),
                    }),
                });
            }
        }
        if let Some(l) = &self.overview {
            for n in &l.document.graph.nodes {
                out.push(Entry {
                    kind: "Map",
                    label: n.label.clone(),
                    detail: n.kind.clone(),
                    target: Target::Node(n.id.clone()),
                });
            }
        }
        for t in self.mirror.threads.iter().filter(|t| ops::waiting_on_user(t, ME)) {
            let body = t.messages.last().map_or("", |m| m.body.as_str());
            out.push(Entry {
                kind: "Waiting",
                label: rain_comments::label::first_sentence(body, 70),
                detail: t.file_path.clone(),
                target: Target::Thread(t.id.clone()),
            });
        }
        out
    }

    /// The rows of an empty query, each group in list order: the file the last jump left, the
    /// files not yet viewed, those viewed with an open thread, the other files, then the rest;
    /// and whether the first row is that left file.
    fn picker_blank(&self, entries: &[Entry]) -> (Vec<u32>, bool) {
        let mut threads = vec![false; self.keys.len()];
        for t in self.mirror.threads.iter().filter(|t| t.is_open()) {
            if let Some(g) = self.file_of_thread(t) {
                threads[g] = true;
            }
        }
        let left = self.previous_file();
        let rank = |i: &u32| match entries[*i as usize].target {
            Target::File(g) if Some(g) == left => 0,
            Target::File(g) if !self.is_viewed(g) => 1,
            Target::File(g) if threads[g] => 2,
            Target::File(_) => 3,
            _ => 4,
        };
        let mut rows: Vec<u32> = (0..entries.len() as u32).collect();
        rows.sort_by_key(rank);
        rows.truncate(PICKER_ROWS);
        let leads = rows.first().is_some_and(|i| rank(i) == 0);
        (rows, leads)
    }

    /// Whether the first row is the file the last jump left: only with no query.
    pub fn picker_leads_with_previous(&self) -> bool {
        let cache = self.picker_cache();
        cache.blank.1 && cache.query.as_deref() == Some("")
    }

    /// Each entry's [`Tie`]; an entry that is no file comes after the files it ties with.
    fn picker_ties(&self, entries: &[Entry]) -> Vec<Tie> {
        let open = self.mirror.threads.iter().filter(|t| t.is_open());
        let threads: HashSet<usize> = open.filter_map(|t| self.file_of_thread(t)).collect();
        entries
            .iter()
            .map(|e| match e.target {
                Target::File(g) => {
                    let f = &self.diff.files()[g];
                    let changed = f.additions.saturating_add(f.deletions);
                    (self.is_viewed(g), !threads.contains(&g), std::cmp::Reverse(changed))
                }
                _ => (true, true, std::cmp::Reverse(0)),
            })
            .collect()
    }

    /// The entries matching the picker's text, best first ([`Tie`], then list order, on ties).
    pub fn picker_rows(&self) -> Vec<Entry> {
        let cache = self.picker_cache();
        cache.rows.iter().map(|&i| cache.entries[i as usize].clone()).collect()
    }

    /// Per row of [`App::picker_rows`], the byte ranges of its label the query matched; none
    /// without a query.
    pub fn picker_marks(&self) -> Vec<Vec<Range<usize>>> {
        self.picker_cache().marks.clone()
    }

    /// Per row of [`App::picker_rows`], the byte ranges of its folder (a file's `detail`) the
    /// query matched; none without a query.
    pub fn picker_folder_marks(&self) -> Vec<Vec<Range<usize>>> {
        self.picker_cache().folder_marks.clone()
    }

    pub fn picker_len(&self) -> usize {
        self.picker_cache().rows.len()
    }

    /// The cache, holding the rows for the picker's text as things stand.
    fn picker_cache(&self) -> std::cell::RefMut<'_, Cache> {
        let mut cache = self.picker.cache.borrow_mut();
        cache.refresh(self);
        cache.find(self, self.picker.edit.text.trim());
        cache
    }

    pub fn picker_move(&mut self, down: bool, tx: &mut Tx) {
        let i = if down {
            self.picker.sel + 1
        } else {
            self.picker.sel.saturating_sub(1)
        };
        self.picker_select(i, tx);
    }

    /// Selects row `i` (clamped to the last) and scrolls the list just enough to show it.
    pub fn picker_select(&mut self, i: usize, tx: &mut Tx) {
        self.picker.sel = i.min(self.picker_len().saturating_sub(1));
        tx.scroll_into_view_in("picker-list", format!("pick-{}", self.picker.sel));
    }

    /// Vim's normal mode in the picker: `Esc` took the keys from the query box.
    pub fn picker_normal(&self) -> bool {
        self.layout.is_open(Overlay::Picker) && !self.picker.edit.focused && self.keymap_profile().vim_motions()
    }

    /// A key in normal mode: `j k ↑ ↓` move, `gg G` jump to the ends, `Enter` goes, `i` or
    /// `/` type again, `Esc` closes. The rest do nothing.
    pub fn picker_normal_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        let g = std::mem::take(&mut self.picker.pending_g);
        match &ev.key {
            Key::Down => self.picker_move(true, tx),
            Key::Up => self.picker_move(false, tx),
            Key::Enter => self.picker_go(ev.mods.ctrl || ev.mods.cmd, tx),
            Key::Escape => self.close_picker(),
            Key::Text(t) if !(ev.mods.ctrl || ev.mods.cmd || ev.mods.alt) => match t.as_str() {
                "j" => self.picker_move(true, tx),
                "k" => self.picker_move(false, tx),
                "i" | "/" => self.picker.edit.focused = true,
                "G" => self.picker_select(usize::MAX, tx),
                "g" if g => self.picker_select(0, tx),
                "g" => self.picker.pending_g = true,
                _ => {}
            },
            _ => {}
        }
        true
    }

    /// The panel's size: the saved one, inside the window.
    pub fn picker_size(&self) -> (f32, f32) {
        self.fit_picker(self.settings.picker_size)
    }

    fn fit_picker(&self, (w, h): (f32, f32)) -> (f32, f32) {
        let room_w = if self.window_width > 0.0 {
            self.window_width - 2.0 * PICKER_MARGIN
        } else {
            f32::MAX
        };
        let room_h = if self.window_height > 0.0 {
            self.window_height - space::TOP_BAR - space::STATUS - PICKER_TOP - PICKER_MARGIN
        } else {
            f32::MAX
        };
        (w.min(room_w).max(PICKER_MIN.0), h.min(room_h).max(PICKER_MIN.1))
    }

    /// Resizes the panel from a drag on its right edge (`dx`; it stays centred, so the left edge
    /// mirrors it) or its bottom (`dy`); saved once the drag ends.
    pub fn drag_picker(&mut self, phase: rune::Phase, dx: f32, dy: f32, cancelled: bool) {
        let at = self.picker_size();
        match phase {
            rune::Phase::Down => self.picker.drag_from = Some(at),
            rune::Phase::Move | rune::Phase::Up => {
                let from = self.picker.drag_from.unwrap_or(at);
                self.settings.picker_size = if cancelled {
                    from
                } else {
                    self.fit_picker((from.0 + 2.0 * dx, from.1 + dy))
                };
                if phase == rune::Phase::Up {
                    self.picker.drag_from = None;
                    self.save_settings();
                }
            }
        }
    }

    /// `Enter` goes to the selected entry; zoom is restored first unless `keep_zoom` (Ctrl+Enter).
    pub fn picker_go(&mut self, keep_zoom: bool, tx: &mut Tx) {
        let rows = self.picker_rows();
        let Some(e) = rows.get(self.picker.sel.min(rows.len().saturating_sub(1))).cloned() else {
            return;
        };
        self.close_picker();
        if self.layout.is_zoomed() && !keep_zoom {
            self.toggle_zoom();
        }
        match e.target {
            Target::File(g) => self.goto_file(g, tx),
            Target::Group(g) => {
                if self.single_mode() {
                    self.open_pick(Pick::Group(g), tx);
                } else {
                    self.unfold_section(super::board::section_of(g));
                    if let Some(f) = self.list_order().into_iter().find(|&f| self.group_of(f) == g) {
                        self.goto_file(f, tx);
                    }
                }
            }
            Target::Brief(_) => {
                self.show_overview();
                self.status = format!("Brief › {}", e.label);
            }
            Target::Node(id) => self.open_node(id, tx),
            Target::Step(spec) => {
                if !self.tour_from_picker(&e.label, tx) {
                    self.goto_spec(&spec, tx);
                    self.status = format!("Tour {}", e.label);
                }
            }
            Target::Thread(id) => self.goto_thread(&id, tx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fuzzy;

    #[test]
    fn word_starts_and_runs_win() {
        let vsc = |h| fuzzy("vsc", h).unwrap_or(i32::MIN);
        assert!(vsc("VouchersController.cs") > vsc("VoucherService.cs"));
        assert!(vsc("VouchersController.cs") > vsc("voucher.service.ts"));
        assert!(fuzzy("zz", "VoucherService.cs").is_none());
        assert_eq!(fuzzy("", "x"), Some(0));
    }
}
