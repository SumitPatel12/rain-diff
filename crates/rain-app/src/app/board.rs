//! The list: triage's one order cut into sections (the real changes under Must review / Review /
//! Skim, then the groups, folded ones pinned under the rows, Waiting on you last), folding remembered in
//! the settings, accept-group with undo, the reader's stream ↔ file-at-a-time flip and jump
//! history.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rain_comments::ops;
use rain_core::{AcceptedGroup, ReaderMode};
use rain_triage::{Group, Item, Triage};
use rune::Tx;

use super::{App, Subset};

/// Section ids are settings bits, so they never change; [`SECTION_ORDER`] places them.
pub const CHANGES: u8 = 0;
/// The section the file filter and the tree belong to.
pub const FILES: u8 = CHANGES;
pub const WAITING: u8 = 1;
pub const READ_ME: u8 = 2;
pub const TESTS: u8 = 3;
pub const MOVES: u8 = 4;
pub const FORMATTING: u8 = 5;
pub const GENERATED: u8 = 6;
pub const LOCKFILE: u8 = 7;
pub const HUGE: u8 = 8;
pub const BINARY: u8 = 9;
pub const IGNORED: u8 = 10;
pub const DONE: u8 = 11;

/// The list top to bottom; Waiting on you is last.
pub const SECTION_ORDER: [u8; 12] = [
    READ_ME, CHANGES, TESTS, MOVES, FORMATTING, GENERATED, LOCKFILE, HUGE, BINARY, IGNORED, DONE, WAITING,
];

/// Sections that start folded; a settings bit flips a section from its default.
const FOLDED_BY_DEFAULT: [u8; 10] = [TESTS, MOVES, FORMATTING, GENERATED, LOCKFILE, HUGE, BINARY, IGNORED, DONE, WAITING];

/// The triage groups Changes holds, each under its own sub-heading.
pub const CHANGE_GROUPS: [Group; 3] = [Group::Must, Group::Review, Group::Skim];

/// The settings bit of a change group's own fold, after the sections' bits.
fn sub_bit(g: Group) -> u8 {
    12 + CHANGE_GROUPS.iter().position(|&x| x == g).unwrap_or(0) as u8
}

/// A change group's name, in the filter chips' words.
pub fn change_title(g: Group) -> &'static str {
    match g {
        Group::Must => "Must review",
        Group::Skim => "Skim",
        _ => "Review",
    }
}

/// How long the accept snackbar offers Undo.
pub const UNDO_FOR: Duration = Duration::from_secs(10);

/// Jump history entries kept each way.
pub const HISTORY: usize = 50;

/// The group sections, in list order, with the group each holds.
pub const GROUP_SECTIONS: [(u8, Group); 8] = [
    (TESTS, Group::Tests),
    (MOVES, Group::Moves),
    (FORMATTING, Group::FormattingOnly),
    (GENERATED, Group::Generated),
    (LOCKFILE, Group::Lockfile),
    (HUGE, Group::Huge),
    (BINARY, Group::Binary),
    (IGNORED, Group::Ignored),
];

pub fn section_of(g: Group) -> u8 {
    GROUP_SECTIONS.iter().find(|(_, x)| *x == g).map_or(CHANGES, |(s, _)| *s)
}

pub fn section_group(section: u8) -> Option<Group> {
    GROUP_SECTIONS.iter().find(|(s, _)| *s == section).map(|(_, g)| *g)
}

pub fn section_title(section: u8) -> &'static str {
    match section {
        READ_ME => "READ ME FIRST",
        CHANGES => "REAL CHANGES",
        TESTS => "TESTS",
        MOVES => "MOVES",
        FORMATTING => "FORMATTING-ONLY",
        GENERATED => "GENERATED",
        LOCKFILE => "LOCKFILE",
        HUGE => "HUGE",
        BINARY => "BINARY",
        IGNORED => "IGNORED",
        DONE => "DONE",
        _ => "WAITING ON YOU",
    }
}

/// The session's copy of a triage group (same variants, same wire names).
pub fn core_group(g: Group) -> rain_core::Group {
    match g {
        Group::Must => rain_core::Group::Must,
        Group::Review => rain_core::Group::Review,
        Group::Skim => rain_core::Group::Skim,
        Group::Tests => rain_core::Group::Tests,
        Group::Moves => rain_core::Group::Moves,
        Group::FormattingOnly => rain_core::Group::FormattingOnly,
        Group::Generated => rain_core::Group::Generated,
        Group::Lockfile => rain_core::Group::Lockfile,
        Group::Huge => rain_core::Group::Huge,
        Group::Binary => rain_core::Group::Binary,
        Group::Ignored => rain_core::Group::Ignored,
    }
}

/// One row of the list's scrolling part (Waiting on you is drawn below it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    Header(u8),
    /// A change group's sub-heading inside Changes.
    Sub(Group),
    /// A file, by full-list index.
    File(usize),
    /// An accepted group in Done, by index into the session's `accepted`.
    Accepted(usize),
}

/// The list as drawn: its rows, and per section the files the filter shows and those an
/// Accept would mark.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Listing {
    pub rows: Vec<Row>,
    pub shown: [usize; 12],
    pub acceptable: [usize; 12],
    /// Whether the section has any file at all, filter or not.
    pub present: [bool; 12],
    /// Per change group (as [`CHANGE_GROUPS`]), the files the filter shows.
    pub changes: [usize; 3],
    /// Folded group sections, drawn under the scrolling rows so they and their Accept always show.
    pub footer: Vec<u8>,
    /// [`App::list_order`] as of this listing.
    pub order: Vec<usize>,
    pub version: u64,
}

/// What file-at-a-time shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Pick {
    /// Counts and the Accept row, before anything is chosen.
    #[default]
    Landing,
    File(usize),
    Group(Group),
    Brief,
}

#[derive(Default)]
pub struct BoardState {
    /// Triage's items in list order.
    pub items: Vec<Item>,
    /// Per full-list file, its item.
    item_of: Vec<Option<usize>>,
    /// Changes drawn as the folder tree instead of by group.
    pub folders: bool,
    /// File-at-a-time's one-file diff (empty for a card).
    pub single: Option<Subset>,
    pub pick: Pick,
    /// The picked file's key, which a reload re-finds.
    pick_key: Option<String>,
    back: Vec<String>,
    forward: Vec<String>,
    /// The accept snackbar's text and when it appeared.
    pub snack: Option<(String, Instant)>,
    cache: RefCell<Option<(u64, Arc<Listing>)>>,
}

impl BoardState {
    pub fn history(&self) -> (&[String], &[String]) {
        (&self.back, &self.forward)
    }
}

impl App {
    /// Takes the loader's triage; its items are in the same order as the files.
    pub(crate) fn install_triage(&mut self, t: Triage) {
        self.board.items = t.items;
        let mut item_of = vec![None; self.keys.len()];
        for (i, it) in self.board.items.iter().enumerate() {
            if let Some(g) = self.index_of(&it.key) {
                item_of[g] = Some(i);
            }
        }
        self.board.item_of = item_of;
        self.board.cache.replace(None);
        self.board.single = None;
        if self.single_mode() {
            let pick = match self.board.pick {
                Pick::File(_) => self
                    .board
                    .pick_key
                    .clone()
                    .and_then(|k| self.index_of(&k))
                    .map_or(Pick::Landing, Pick::File),
                p => p,
            };
            self.show_pick(pick);
        }
    }

    pub fn item(&self, g: usize) -> Option<&Item> {
        self.board.item_of.get(g).copied().flatten().map(|i| &self.board.items[i])
    }

    /// The triage group of full-list file `g` (Review when triage didn't see it).
    pub fn group_of(&self, g: usize) -> Group {
        self.item(g).map_or(Group::Review, |i| i.group)
    }

    pub fn section_folded(&self, section: u8) -> bool {
        self.settings.section_collapsed(section) != FOLDED_BY_DEFAULT.contains(&section)
    }

    /// Whether a change group (Must review, Review, Skim) is folded under its sub-heading.
    pub fn sub_folded(&self, g: Group) -> bool {
        self.settings.section_collapsed(sub_bit(g))
    }

    /// Folds or unfolds a change group and remembers it.
    pub fn toggle_sub(&mut self, g: Group) {
        self.settings.toggle_section(sub_bit(g));
        self.save_settings();
    }

    /// Folds or unfolds a list section and remembers it.
    pub fn toggle_list_section(&mut self, section: u8) {
        self.settings.toggle_section(section);
        self.save_settings();
    }

    /// Unfolds `section` when it is folded.
    pub fn unfold_section(&mut self, section: u8) {
        if self.section_folded(section) {
            self.toggle_list_section(section);
        }
    }

    /// Grouped ↔ Folders for the Changes section.
    pub fn toggle_changes_mode(&mut self) {
        self.board.folders = !self.board.folders;
        self.status = if self.board.folders {
            "Changes by folder".into()
        } else {
            "Changes by group".into()
        };
    }

    /// Files of accepted groups that are not undone.
    pub fn accepted_files(&self) -> BTreeSet<usize> {
        self.mirror
            .accepted
            .iter()
            .filter(|a| !a.undone)
            .flat_map(|a| a.files.iter())
            .filter_map(|k| self.index_of(k))
            .collect()
    }

    fn listing_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let f = &self.tree.filter;
        (
            self.settings.sidebar_sections,
            self.board.folders,
            self.mirror.version,
            self.board.items.len(),
            self.keys.len(),
            self.has_map(),
            self.reloader.installs(),
            self.tree.filter_revision(),
        )
            .hash(&mut h);
        (&f.tags, &f.repos, f.unviewed_only, f.commented_only, &f.text).hash(&mut h);
        h.finish()
    }

    /// The list as drawn, rebuilt only when what it shows changed.
    pub fn listing(&self) -> Arc<Listing> {
        let key = self.listing_key();
        if let Some((k, l)) = self.board.cache.borrow().as_ref()
            && *k == key
        {
            return l.clone();
        }
        let mut l = self.build_listing();
        l.version = key;
        let l = Arc::new(l);
        self.board.cache.replace(Some((key, l.clone())));
        l
    }

    fn build_listing(&self) -> Listing {
        let accepted = self.accepted_files();
        let mut by_section: [Vec<usize>; 12] = Default::default();
        let mut l = Listing::default();
        for g in 0..self.keys.len() {
            let item = self.item(g);
            let sec = section_of(self.group_of(g));
            if accepted.contains(&g) {
                l.present[DONE as usize] = true;
                continue;
            }
            l.present[sec as usize] = true;
            if !self.tree.is_shown(&self.keys[g]) {
                continue;
            }
            by_section[sec as usize].push(g);
            if sec != CHANGES && item.is_none_or(|i| i.real_line.is_none()) && !self.is_viewed(g) {
                l.acceptable[sec as usize] += 1;
            }
        }
        for (n, files) in l.shown.iter_mut().zip(&by_section) {
            *n = files.len();
        }
        let rank = |g: Group| CHANGE_GROUPS.iter().position(|&x| x == g).unwrap_or(1);
        by_section[CHANGES as usize].sort_by_key(|&g| rank(self.group_of(g)));
        for &g in &by_section[CHANGES as usize] {
            l.changes[rank(self.group_of(g))] += 1;
        }
        l.shown[DONE as usize] = self.mirror.accepted.iter().filter(|a| !a.undone).count();
        l.present[DONE as usize] |= l.shown[DONE as usize] > 0;
        let filtering = !self.tree.filter.is_empty();
        for sec in SECTION_ORDER {
            match sec {
                WAITING | READ_ME => {}
                DONE => {
                    if !l.present[DONE as usize] {
                        continue;
                    }
                    l.rows.push(Row::Header(DONE));
                    if !self.section_folded(DONE) {
                        l.rows.extend(
                            self.mirror
                                .accepted
                                .iter()
                                .enumerate()
                                .filter(|(_, a)| !a.undone)
                                .map(|(i, _)| Row::Accepted(i)),
                        );
                    }
                }
                _ => {
                    let empty = !l.present[sec as usize] || (filtering && l.shown[sec as usize] == 0);
                    if sec != CHANGES && empty {
                        continue;
                    }
                    if sec != CHANGES && self.section_folded(sec) {
                        l.footer.push(sec);
                        continue;
                    }
                    l.rows.push(Row::Header(sec));
                    let tree_instead = sec == CHANGES && self.board.folders;
                    if self.section_folded(sec) || tree_instead {
                        continue;
                    }
                    let mut last = None;
                    for &g in &by_section[sec as usize] {
                        let group = self.group_of(g);
                        if sec == CHANGES && last != Some(group) {
                            l.rows.push(Row::Sub(group));
                            last = Some(group);
                        }
                        if sec == CHANGES && self.sub_folded(group) {
                            continue;
                        }
                        l.rows.push(Row::File(g));
                    }
                }
            }
        }
        l.order = self.list_order();
        l
    }

    /// The one list order: Changes, then the groups not accepted, as the filter narrows them.
    pub fn list_order(&self) -> Vec<usize> {
        let accepted = self.accepted_files();
        let mut out: Vec<(u8, usize)> = (0..self.keys.len())
            .filter(|g| !accepted.contains(g) && self.tree.is_shown(&self.keys[*g]))
            .map(|g| (section_of(self.group_of(g)), g))
            .collect();
        let rank = |s: u8| SECTION_ORDER.iter().position(|x| *x == s).unwrap_or(usize::MAX);
        let sub = |g: usize| CHANGE_GROUPS.iter().position(|&x| x == self.group_of(g)).unwrap_or(0);
        out.sort_by_key(|&(s, g)| (rank(s), sub(g), g));
        out.into_iter().map(|(_, g)| g).collect()
    }

    /// `Ctrl+L`: focuses the list's filter box.
    pub fn focus_list_filter(&mut self, tx: &mut Tx) {
        self.filter_files(tx);
    }

    // ----- accept -----

    /// `Shift+A`: accepts the current item's group.
    pub fn accept_group(&mut self, tx: &mut Tx) {
        let group = match self.board.pick {
            Pick::Group(g) if self.single_mode() => Some(g),
            _ => self.current().map(|g| self.group_of(g)),
        };
        match group {
            Some(g) if !g.is_change() => self.accept(g, tx),
            Some(_) => self.status = "Real changes are read one by one: Space marks this file viewed".into(),
            None => self.status = "No file to accept the group of".into(),
        }
    }

    /// Marks every unviewed file of `group` the filter shows viewed without reading, logs it
    /// with the reason, and offers Undo. A formatting change with a real line is never in a group.
    pub fn accept(&mut self, group: Group, tx: &mut Tx) {
        let accepted = self.accepted_files();
        let files: Vec<usize> = (0..self.keys.len())
            .filter(|&g| {
                self.group_of(g) == group
                    && self.item(g).is_some_and(|i| i.real_line.is_none())
                    && !accepted.contains(&g)
                    && !self.is_viewed(g)
                    && self.tree.is_shown(&self.keys[g])
            })
            .collect();
        if files.is_empty() {
            self.status = format!("Nothing left to accept in {}", group.label());
            return;
        }
        let keys: Vec<String> = files.iter().map(|&g| self.keys[g].clone()).collect();
        let why = files
            .first()
            .and_then(|&g| self.item(g))
            .and_then(|i| i.reason.text.split_once('(').map(|(_, r)| r.trim_end_matches(')').to_owned()));
        let reason = format!(
            "{} {} accepted without reading{}",
            keys.len(),
            group.label().to_lowercase(),
            why.map(|w| format!(" ({w})")).unwrap_or_default()
        );
        self.tree.set_viewed_many(keys.iter().map(|k| (k.as_str(), true)));
        self.persist_viewed_keys(&keys);
        let record = AcceptedGroup {
            group: core_group(group),
            files: keys,
            reason: reason.clone(),
            at: rain_comments::clock::now_iso(),
            undone: false,
        };
        self.mutate(|s| {
            s.accepted.push(record);
            s.version += 1;
        });
        self.board.snack = Some((reason.clone(), Instant::now()));
        self.status = reason;
        if self.single_mode() && self.board.pick == Pick::Group(group) {
            self.step_item(true, tx);
        }
    }

    /// `u`: undoes the last accept that is not undone: its files read unviewed again.
    pub fn undo_accept(&mut self, _tx: &mut Tx) {
        let Some(i) = self.mirror.accepted.iter().rposition(|a| !a.undone) else {
            self.status = "Nothing to undo".into();
            return;
        };
        let a = self.mirror.accepted[i].clone();
        let mut items: Vec<(String, String)> = Vec::new();
        let mut unmarked: Vec<&str> = Vec::new();
        for k in &a.files {
            let Some(g) = self.index_of(k) else { continue };
            let (repo, path) = (self.repo_tag(g).to_owned(), self.diff.files()[g].path.clone());
            // A file viewed by hand after the accept stays viewed.
            let read_since = self
                .mirror
                .viewed
                .iter()
                .any(|v| v.repo == repo && v.file_path == path && v.viewed_at > a.at);
            if !read_since {
                unmarked.push(k);
                items.push((repo, path));
            }
        }
        self.tree.set_viewed_many(unmarked.into_iter().map(|k| (k, false)));
        self.mutate(|s| {
            for (repo, path) in &items {
                ops::clear_viewed(s, repo, path);
            }
            if let Some(x) = s.accepted.get_mut(i) {
                x.undone = true;
            }
            s.version += 1;
        });
        self.board.snack = None;
        self.status = format!("Undone: {}", a.reason);
    }

    /// The snackbar's text while it still offers Undo.
    pub fn accept_snack(&self) -> Option<&str> {
        self.board
            .snack
            .as_ref()
            .filter(|(_, at)| at.elapsed() < UNDO_FOR)
            .map(|(m, _)| m.as_str())
    }

    // ----- reader mode -----

    pub fn single_mode(&self) -> bool {
        self.mirror.ui.reader == ReaderMode::Single
    }

    /// `Shift+S`: stream ↔ file-at-a-time, remembered in the session.
    pub fn toggle_reader_mode(&mut self, tx: &mut Tx) {
        let cur = self.current();
        let single = !self.single_mode();
        self.mutate(|s| {
            s.ui.reader = if single { ReaderMode::Single } else { ReaderMode::Stream };
            s.version += 1;
        });
        if single {
            self.show_pick(cur.map_or(Pick::Landing, Pick::File));
            self.status = "File at a time · Space done+next · ] [ items · Shift+S stream".into();
        } else {
            self.board.single = None;
            self.rebuild_annotations();
            if let Some(g) = cur {
                self.goto_file(g, tx);
            }
            self.status = "Stream · every file one under another".into();
        }
    }

    /// File-at-a-time shows `pick`.
    pub fn show_pick(&mut self, pick: Pick) {
        let files = match pick {
            Pick::File(g) if g < self.keys.len() => vec![g],
            _ => Vec::new(),
        };
        let mut sub = Subset::new(self, files, None);
        if let Some(&g) = sub.files.first() {
            sub.diff.set_collapsed(
                0,
                self.diff.collapsed(g) && self.diff.files()[g].lines.len() > crate::load::LARGE_FILE_LINES,
            );
        }
        self.board.single = Some(sub);
        self.board.pick = pick;
        self.board.pick_key = match pick {
            Pick::File(g) => self.keys.get(g).cloned(),
            _ => None,
        };
        self.rebuild_annotations();
    }

    /// File-at-a-time's items in order: Changes files, then one card per group left to accept.
    pub fn pick_order(&self) -> Vec<Pick> {
        let order = self.list_order();
        let mut out: Vec<Pick> = Vec::new();
        for g in order {
            let group = self.group_of(g);
            if group.is_change() {
                out.push(Pick::File(g));
            } else if !out.contains(&Pick::Group(group)) {
                out.push(Pick::Group(group));
            }
        }
        out
    }

    /// Where `pick` sits in [`App::pick_order`]: a group's file counts as its card.
    fn pick_index(&self, order: &[Pick], pick: Pick) -> Option<usize> {
        let at = match pick {
            Pick::File(g) if !self.group_of(g).is_change() => Pick::Group(self.group_of(g)),
            p => p,
        };
        order.iter().position(|p| *p == at)
    }

    /// `]`/`[`: the next or previous file, or item in file-at-a-time.
    pub fn step_item(&mut self, forward: bool, tx: &mut Tx) {
        if !self.single_mode() {
            self.active_mut().next_file(forward, tx);
            if let Some(k) = self.current_key() {
                self.tree.reveal(&k, tx);
            }
            return;
        }
        let order = self.pick_order();
        if order.is_empty() {
            self.show_pick(Pick::Landing);
            return;
        }
        let next = match self.pick_index(&order, self.board.pick) {
            Some(i) if forward => order.get(i + 1).copied(),
            Some(i) => i.checked_sub(1).map(|i| order[i]),
            None if forward => order.first().copied(),
            None => order.last().copied(),
        };
        match next {
            Some(p) => self.open_pick(p, tx),
            None => self.status = if forward { "Last item" } else { "First item" }.into(),
        }
    }

    /// Shows `pick` in file-at-a-time, the list following.
    pub fn open_pick(&mut self, pick: Pick, tx: &mut Tx) {
        match pick {
            Pick::File(g) => self.goto_file(g, tx),
            p => {
                self.record_jump();
                self.show_pick(p);
            }
        }
    }

    /// `Space` in file-at-a-time: done (viewed, or the group accepted) and the next item not done.
    pub(crate) fn done_and_next(&mut self, tx: &mut Tx) {
        match self.board.pick {
            Pick::File(g) => self.set_viewed(g, true),
            Pick::Group(group) => {
                self.accept(group, tx);
                return;
            }
            Pick::Landing | Pick::Brief => {}
        }
        let order = self.pick_order();
        let at = self.pick_index(&order, self.board.pick);
        let after = at.map_or(0, |i| i + 1);
        let pending = |s: &App, p: &Pick| match *p {
            Pick::File(g) => !s.is_viewed(g),
            Pick::Group(group) => (0..s.keys.len()).any(|g| s.group_of(g) == group && !s.is_viewed(g)),
            _ => false,
        };
        let next = order[after.min(order.len())..]
            .iter()
            .chain(order[..after.min(order.len())].iter())
            .copied()
            .find(|p| pending(self, p));
        match next {
            Some(p) => {
                self.open_pick(p, tx);
                self.status = match p {
                    Pick::File(g) => format!("Done. Next: {}", self.keys[g]),
                    Pick::Group(group) => format!("Done. Next: {} group", group.label()),
                    _ => "Done".into(),
                };
            }
            None => {
                self.show_pick(Pick::Landing);
                self.status = self.all_viewed_status();
            }
        }
    }

    /// `J`/`K`: the first file of the next or previous list section.
    pub fn step_section(&mut self, forward: bool, tx: &mut Tx) {
        let order = self.list_order();
        let cur = self.current().map(|g| section_of(self.group_of(g)));
        let rank = |s: u8| SECTION_ORDER.iter().position(|x| *x == s).unwrap_or(0);
        let firsts: Vec<(u8, usize)> = order.iter().fold(Vec::new(), |mut v, &g| {
            let s = section_of(self.group_of(g));
            if v.last().is_none_or(|(x, _)| *x != s) {
                v.push((s, g));
            }
            v
        });
        let target = match cur {
            Some(c) if forward => firsts.iter().find(|(s, _)| rank(*s) > rank(c)),
            Some(c) => firsts.iter().rev().find(|(s, _)| rank(*s) < rank(c)),
            None => firsts.first(),
        };
        match target {
            Some(&(s, g)) => {
                self.unfold_section(s);
                self.goto_file(g, tx);
                self.status = section_title(s).into();
            }
            None => self.status = if forward { "Last section" } else { "First section" }.into(),
        }
    }

    // ----- jump history -----

    /// Remembers the reader's file before a jump to another place.
    pub(crate) fn record_jump(&mut self) {
        let Some(k) = self.current_key() else { return };
        if self.board.back.last() == Some(&k) {
            return;
        }
        self.board.back.push(k);
        if self.board.back.len() > HISTORY {
            self.board.back.remove(0);
        }
        self.board.forward.clear();
    }

    /// `Alt+←`/`Alt+→`: back or forward through the files jumped between.
    pub fn jump_history(&mut self, back: bool, tx: &mut Tx) {
        let popped = if back { self.board.back.pop() } else { self.board.forward.pop() };
        let Some(key) = popped else {
            self.status = if back { "No earlier place" } else { "No later place" }.into();
            return;
        };
        if let Some(k) = self.current_key() {
            let to = if back { &mut self.board.forward } else { &mut self.board.back };
            to.push(k);
            if to.len() > HISTORY {
                to.remove(0);
            }
        }
        let Some(g) = self.index_of(&key) else { return };
        self.show_file(g, tx);
        self.status = format!("{}: {key}", if back { "Back" } else { "Forward" });
    }
}
