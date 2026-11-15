//! The reader's two tabs (Overview and Changes) and the cite peek: a floating card with the
//! cited lines that opens on a hover dwell or a press, and pops out into its own window.

use std::ops::Range;
use std::time::{Duration, Instant};

use rain_core::{LineKind, Side};
use rune::Tx;

use super::{App, ComposeMode};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReaderTab {
    Overview,
    #[default]
    Changes,
}

/// A hover opens the peek after this long on a chip.
pub const PEEK_DWELL: Duration = Duration::from_millis(300);
/// Leaving the chip and the card closes it after this long.
pub const PEEK_GRACE: Duration = Duration::from_millis(250);
/// Lines shown around the cited range.
const PEEK_CONTEXT: usize = 6;
/// Rows a peek holds at most; a longer cite scrolls inside the card.
const PEEK_MAX_ROWS: usize = 120;
/// How far the page may scroll under an open peek before it closes.
const PEEK_SCROLL_SLACK: f32 = 24.0;

/// What a peek shows and where it hangs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeekTarget {
    pub key: String,
    pub lines: Option<(u32, u32)>,
    pub side: Side,
    /// The element the card hangs under.
    pub anchor: String,
    /// The disambiguated chip label.
    pub label: String,
}

/// One row of a peek: owned, so it can travel to the pop-out window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeekRow {
    pub old: u32,
    pub new: u32,
    pub kind: LineKind,
    pub text: String,
    /// Byte ranges of `text` that changed against its paired line.
    pub words: Vec<Range<usize>>,
    /// Inside the cited lines.
    pub cited: bool,
    /// Lines the diff does not show sit between this row and the one before.
    pub gap_before: bool,
}

/// A peek's lines and title, independent of any window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeekContent {
    pub title: String,
    pub key: String,
    pub lang: Option<String>,
    pub rows: Vec<PeekRow>,
}

#[derive(Default)]
pub struct PeekState {
    pub open: Option<PeekTarget>,
    /// Opened by a press: leaving it does not close it.
    pub pinned: bool,
    hover: Option<(PeekTarget, Instant)>,
    leave: Option<Instant>,
    /// The Overview page's viewport top and content top, and how far it was scrolled when the
    /// peek opened.
    page_top: Option<f32>,
    body_top: Option<f32>,
    scroll_at_open: Option<f32>,
}

impl PeekState {
    fn scroll(&self) -> Option<f32> {
        Some(self.page_top? - self.body_top?)
    }

    fn opened(&mut self, target: PeekTarget, pinned: bool) {
        self.open = Some(target);
        self.pinned = pinned;
        self.hover = None;
        self.leave = None;
        self.scroll_at_open = self.scroll();
    }
}

impl App {
    pub fn reader_tab(&self) -> ReaderTab {
        self.brief.tab.unwrap_or_default()
    }

    /// Whether the Overview has a story to tell: a brief or story notes.
    pub fn has_overview(&self) -> bool {
        self.brief_doc().is_some() || !self.story_notes().is_empty()
    }

    /// The first load picks the tab: the Overview when it has a story, else Changes. Reloads
    /// keep whatever is showing.
    pub(crate) fn settle_reader_tab(&mut self) {
        if self.brief.tab.is_none() {
            self.brief.tab = Some(if self.has_overview() {
                ReaderTab::Overview
            } else {
                ReaderTab::Changes
            });
        }
    }

    pub fn set_reader_tab(&mut self, tab: ReaderTab) {
        if self.reader_tab() != tab {
            self.close_peek();
        }
        self.brief.tab = Some(tab);
    }

    /// `b`: Overview ↔ Changes.
    pub fn toggle_reader_tab(&mut self) {
        let next = match self.reader_tab() {
            ReaderTab::Overview => ReaderTab::Changes,
            ReaderTab::Changes => ReaderTab::Overview,
        };
        self.set_reader_tab(next);
        self.status = match next {
            ReaderTab::Overview => "Overview · b back to the changes".into(),
            ReaderTab::Changes => "Changes · b back to the overview".into(),
        };
    }

    /// The Overview tab, from the list, the picker or a story note.
    pub fn show_overview(&mut self) {
        self.set_reader_tab(ReaderTab::Overview);
    }

    // ----- selection -----

    /// Whether text selected in an Overview section shows its toolbar: nothing covers it.
    pub fn overview_toolbar_shown(&self) -> bool {
        self.brief.selection.is_some()
            && self.brief.menu.is_none()
            && self.reader_tab() == ReaderTab::Overview
            && self.phase == super::Phase::Ready
            && self.brief.composer.is_none()
            && self.composer.is_none()
            && self.layout.overlays.is_empty()
    }

    /// Comment or Ask from the toolbar or the menu: the composer on the section, quoting the
    /// selected text when there is one in it.
    pub fn compose_on_section_selection(&mut self, id: &str, mode: ComposeMode) {
        self.brief.menu = None;
        let quote = self.brief.selection.take_if(|(at, _)| at.as_str() == id).map(|(_, t)| t);
        self.comment_on_section(id, quote, mode);
    }

    /// Copy: the selected text, else the section's text.
    pub fn copy_section_selection(&mut self, id: &str, tx: &mut Tx) {
        self.brief.menu = None;
        let text = match &self.brief.selection {
            Some((at, t)) if at == id => Some(t.clone()),
            _ => self.brief_doc().and_then(|d| d.section(id)).map(|s| s.text.to_owned()),
        };
        match text.filter(|t| !t.trim().is_empty()) {
            Some(t) => {
                tx.copy(t);
                self.status = "Copied".into();
            }
            None => self.status = "Nothing to copy in this section".into(),
        }
    }

    /// A right-click on Overview section `id` at window point `at`.
    pub fn open_section_menu(&mut self, id: &str, at: (f32, f32)) {
        self.focus_section(id);
        self.brief.menu = Some((id.to_owned(), at));
    }

    pub fn close_section_menu(&mut self) {
        self.brief.menu = None;
    }

    /// Esc: the menu, else the toolbar, goes; the text stays selected.
    pub(crate) fn hide_overview_toolbar(&mut self) -> bool {
        if self.brief.menu.take().is_some() {
            return true;
        }
        let shown = self.overview_toolbar_shown();
        if shown {
            self.brief.selection = None;
        }
        shown
    }

    // ----- peek -----

    /// A chip's hover: in starts the dwell, out starts the grace.
    pub fn peek_hover(&mut self, target: PeekTarget, inside: bool, tx: &mut Tx) {
        self.peek_hover_at(target, inside, Instant::now());
        tx.wake_after(if inside { PEEK_DWELL } else { PEEK_GRACE } + Duration::from_millis(10));
    }

    pub(crate) fn peek_hover_at(&mut self, target: PeekTarget, inside: bool, now: Instant) {
        let p = &mut self.brief.peek;
        if inside {
            p.leave = None;
            if p.open.as_ref() != Some(&target) {
                p.hover = Some((target, now));
            }
        } else {
            if p.hover.as_ref().is_some_and(|(t, _)| *t == target) {
                p.hover = None;
            }
            if p.open.is_some() && !p.pinned {
                p.leave = Some(now);
            }
        }
    }

    /// The card's own hover keeps it open.
    pub fn peek_card_hover(&mut self, inside: bool, tx: &mut Tx) {
        self.peek_card_hover_at(inside, Instant::now());
        if !inside {
            tx.wake_after(PEEK_GRACE + Duration::from_millis(10));
        }
    }

    pub(crate) fn peek_card_hover_at(&mut self, inside: bool, now: Instant) {
        let p = &mut self.brief.peek;
        if inside {
            p.leave = None;
        } else if p.open.is_some() && !p.pinned {
            p.leave = Some(now);
        }
    }

    /// Every poll: a dwell that ran its time opens, a grace that ran out closes.
    pub(crate) fn peek_tick(&mut self, now: Instant) -> bool {
        let p = &mut self.brief.peek;
        if let Some((t, since)) = &p.hover
            && now.saturating_duration_since(*since) >= PEEK_DWELL
        {
            let t = t.clone();
            p.opened(t, false);
            return true;
        }
        if let Some(at) = p.leave
            && now.saturating_duration_since(at) >= PEEK_GRACE
        {
            p.leave = None;
            p.open = None;
            return true;
        }
        false
    }

    /// Opens the peek on `target` now; `pinned` keeps it when the pointer leaves.
    pub fn open_peek(&mut self, target: PeekTarget, pinned: bool) {
        let p = &mut self.brief.peek;
        if pinned && p.pinned && p.open.as_ref() == Some(&target) {
            self.close_peek();
            return;
        }
        p.opened(target, pinned);
    }

    pub fn close_peek(&mut self) -> bool {
        let p = &mut self.brief.peek;
        p.hover = None;
        p.leave = None;
        p.open.take().is_some()
    }

    /// The Overview's viewport was laid out with its top at `y`.
    pub fn peek_page_top(&mut self, y: f32) {
        self.brief.peek.page_top = Some(y);
        self.peek_scrolled();
    }

    /// The Overview's content was laid out with its top at `y` (it moves as the page scrolls).
    pub fn peek_page_moved(&mut self, y: f32) {
        self.brief.peek.body_top = Some(y);
        self.peek_scrolled();
    }

    /// A scroll that takes the anchor away closes the peek.
    fn peek_scrolled(&mut self) {
        let p = &mut self.brief.peek;
        let (Some(now), true) = (p.scroll(), p.open.is_some()) else {
            return;
        };
        match p.scroll_at_open {
            None => p.scroll_at_open = Some(now),
            Some(at) if (now - at).abs() > PEEK_SCROLL_SLACK => {
                self.close_peek();
            }
            Some(_) => {}
        }
    }

    /// "Open in reader": the cite in the Changes tab.
    pub fn peek_jump(&mut self, tx: &mut Tx) {
        let Some(t) = self.brief.peek.open.clone() else { return };
        self.close_peek();
        self.goto_cite(&t.key, t.lines, t.side, tx);
    }

    /// The cited lines of `t` with their neighbours, as the peek draws them.
    pub fn peek_content(&self, t: &PeekTarget) -> PeekContent {
        let mut out = PeekContent {
            title: t.label.clone(),
            key: t.key.clone(),
            lang: rain_diffview::language(&t.key).map(str::to_owned),
            rows: Vec::new(),
        };
        let Some(g) = self.index_of(&t.key) else { return out };
        let Some(f) = self.diff.files().get(g) else { return out };
        if f.lines.is_empty() {
            return out;
        }
        let num = |i: usize| {
            let l = &f.lines[i];
            if t.side == Side::Old { l.old } else { l.new }
        };
        let (from, to, cited) = match t.lines {
            Some((a, b)) => {
                let first = (0..f.lines.len()).find(|&i| num(i) >= a && num(i) != 0);
                let last = (0..f.lines.len()).rev().find(|&i| num(i) != 0 && num(i) <= b);
                match (first, last) {
                    (Some(x), Some(y)) if x <= y => (x, y, Some((a, b))),
                    (Some(x), _) => (x, x, Some((a, b))),
                    _ => (0, 0, None),
                }
            }
            None => (0, 0, None),
        };
        let start = from.saturating_sub(PEEK_CONTEXT);
        let end = (to + PEEK_CONTEXT).min(f.lines.len() - 1).min(start + PEEK_MAX_ROWS - 1);
        let (mut last_old, mut last_new) = (0, 0);
        for i in start..=end {
            let l = &f.lines[i];
            let jumped = |at: u32, last: u32| at != 0 && last != 0 && at > last + 1;
            let gap_before = jumped(l.old, last_old) || jumped(l.new, last_new);
            if l.old != 0 {
                last_old = l.old;
            }
            if l.new != 0 {
                last_new = l.new;
            }
            let n = num(i);
            out.rows.push(PeekRow {
                old: l.old,
                new: l.new,
                kind: l.kind,
                text: f.line_text(i as u32).trim_end_matches(['\r', '\n']).to_owned(),
                words: self.diff.word_changes(g, i as u32),
                cited: cited.is_some_and(|(a, b)| n != 0 && n >= a && n <= b),
                gap_before,
            });
        }
        out
    }

    /// Scenes: the peek on the first How section's second cite (`VoucherService.cs:25-33` in the
    /// story brief), hung under its chip.
    pub(crate) fn sample_peek_target(&self) -> Option<PeekTarget> {
        let how = self.brief_doc()?.how.first()?;
        let (i, c) = how
            .cites
            .iter()
            .enumerate()
            .rev()
            .find(|(_, c)| c.lines.is_some() && rain_overview::brief::resolve(&c.path, &self.keys).is_some())?;
        let key = rain_overview::brief::resolve(&c.path, &self.keys)?.to_owned();
        let spec = format!("{}:{}-{}", c.path, c.lines?.0, c.lines?.1);
        let paths: Vec<String> = how
            .cites
            .iter()
            .map(|c| match c.lines {
                Some((a, b)) => format!("{}:{a}-{b}", c.path),
                None => c.path.clone(),
            })
            .collect();
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        let label = rain_overview::brief::cite_labels(&refs, &self.keys).get(i).cloned().unwrap_or(spec);
        Some(PeekTarget {
            key,
            lines: c.lines,
            side: c.side.unwrap_or(Side::New),
            anchor: format!("cite-{}-{i}", how.id),
            label,
        })
    }

    /// Scenes over a diff the story brief doesn't cite: the first added lines of the first
    /// must-review file (else the first file with additions).
    pub(crate) fn first_change_peek_target(&self) -> Option<PeekTarget> {
        let must = |g: &usize| self.tags.get(&self.keys[*g]) == Some(&rain_core::FileTag::MustReview);
        let with_adds = |g: &usize| self.diff.files()[*g].additions > 0;
        let g = (0..self.keys.len())
            .find(|g| must(g) && with_adds(g))
            .or_else(|| (0..self.keys.len()).find(with_adds))?;
        let first = self.diff.files()[g].lines.iter().find(|l| l.kind == LineKind::Added)?.new;
        let key = self.keys[g].clone();
        let spec = format!("{key}:{first}-{}", first + 4);
        let label = rain_overview::brief::cite_labels(&[spec.as_str()], &self.keys).remove(0);
        Some(PeekTarget {
            key,
            lines: Some((first, first + 4)),
            side: Side::New,
            anchor: String::new(),
            label,
        })
    }

    /// "Pop out": the open peek in its own window, in this window's theme.
    pub fn pop_out_peek(&mut self) {
        let Some(t) = self.brief.peek.open.clone() else { return };
        let content = self.peek_content(&t);
        let scheme = self.seen_scheme.get();
        self.close_peek();
        self.status = if crate::peek_window::show(content, scheme) {
            format!("{} popped out · Esc closes it", t.label)
        } else {
            "No window to pop out into here".into()
        };
    }
}
