//! Vim's normal and visual modes on the line cursor: the cursor's column, motions with counts,
//! the keys that wait for another (`gg`, `yy`, `f{c}`), `Ctrl+D` / `Ctrl+U` and the visual
//! selection. A key works on the lines it moves over and nothing else; scrolling keeps three
//! lines between the cursor and the viewport's edges and moves only as far as that needs.

use std::ops::Range;

use rain_core::diff::Side;
use rune::Tx;

use super::{DiffState, Draw, LineRange, LineRef, SpanSelection};
use crate::motion::{self, Lines, Pos};

/// Lines kept between the cursor and the viewport's top and bottom (Vim's `scrolloff`).
pub(crate) const SCROLLOFF: f32 = 3.0;
/// Columns kept beside the cursor when the code scrolls sideways.
const SIDE_MARGIN: f32 = 4.0;
const MAX_COUNT: u32 = 99_999;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VimKey {
    /// A typed character; letters typed with Shift arrive upper case.
    Char(char),
    HalfDown,
    HalfUp,
    Escape,
}

impl VimKey {
    /// Whether normal mode acts on this key by itself, with nothing waiting.
    pub fn acts_alone(self) -> bool {
        match self {
            VimKey::HalfDown | VimKey::HalfUp => true,
            VimKey::Escape => false,
            VimKey::Char(c) => matches!(
                c,
                '0'..='9'
                    | 'g'
                    | 'G'
                    | 'f'
                    | 't'
                    | 'y'
                    | 'v'
                    | 'V'
                    | 'j'
                    | 'k'
                    | 'h'
                    | 'l'
                    | 'w'
                    | 'b'
                    | 'e'
                    | 'W'
                    | 'B'
                    | '^'
                    | '$'
                    | ';'
                    | ','
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Vim {
    /// Not a Vim key here: the app's keymap takes it.
    Pass,
    Done,
    /// The key only waits for the next: nothing on screen changed, so no view need run.
    Wait,
    /// Copy this: `y` in visual mode, `yy`.
    Yank(String),
    /// `c` or `?` in visual mode: the selection is now the diff's span or line selection.
    Comment,
    Ask,
}

/// Visual mode's fixed end; the cursor is the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Visual {
    /// `V`: whole lines.
    pub line: bool,
    pub file: usize,
    pub at: LineRef,
    pub byte: usize,
}

/// The column `j` and `k` aim for: the cursor's own until a vertical move asks (`Here`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Want {
    #[default]
    Here,
    Col(u32),
    End,
}

impl Want {
    pub(crate) fn byte_in(self, t: &str) -> usize {
        match self {
            Want::Here => 0,
            Want::Col(c) => motion::at_column(t, c),
            Want::End => motion::snap(t, t.len()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Op {
    #[default]
    None,
    G,
    Y,
    Find {
        forward: bool,
        till: bool,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Pending {
    count: u32,
    op: Op,
}

/// Visual mode's extent in row order: first and last rows, the first's start byte and the
/// last's end byte (exclusive), and the side its text is read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct VisualSpan {
    pub line: bool,
    pub side: Side,
    pub rows: (u32, u32),
    pub start: usize,
    pub end: usize,
}

impl VisualSpan {
    /// The bytes of row `r` (its text `len` long) a charwise span covers.
    pub(crate) fn bytes(&self, r: u32, len: usize) -> Option<Range<usize>> {
        if self.line || r < self.rows.0 || r > self.rows.1 {
            return None;
        }
        let a = if r == self.rows.0 { self.start } else { 0 };
        let b = if r == self.rows.1 { self.end } else { len };
        Some(a.min(len)..b.min(len))
    }
}

/// One file's lines in row order, each on the side the cursor takes there.
pub(super) struct FileLines<'a> {
    pub(super) st: &'a DiffState,
    pub(super) f: usize,
    pub(super) prefer: Side,
}

impl FileLines<'_> {
    pub(super) fn step(&self, r: u32, forward: bool) -> Option<(u32, Side)> {
        let mut i = i64::from(r);
        loop {
            i += if forward { 1 } else { -1 };
            let row = *self.st.rows.get(usize::try_from(i).ok()?)?;
            if row.file() != self.f {
                return None;
            }
            if row.has_line() {
                let side = self.st.side_for(row, self.prefer);
                if self.st.row_line(row, side).is_some() {
                    return Some((i as u32, side));
                }
            }
        }
    }
}

impl Lines for FileLines<'_> {
    type Id = (u32, Side);

    fn text(&self, (r, side): (u32, Side)) -> &str {
        self.st.row_text(r, side).unwrap_or("")
    }

    fn next(&self, (r, _): (u32, Side)) -> Option<(u32, Side)> {
        self.step(r, true)
    }

    fn prev(&self, (r, _): (u32, Side)) -> Option<(u32, Side)> {
        self.step(r, false)
    }
}

pub(super) fn norm(t: &str, col: usize) -> usize {
    if t.is_empty() { 0 } else { motion::snap(t, col) }
}

impl DiffState {
    pub fn visual(&self) -> Option<Visual> {
        self.visual
    }

    /// The cursor's byte in its line, on a grapheme's start.
    pub fn cursor_byte(&self) -> Option<usize> {
        let (_, _, t) = self.cursor_line()?;
        Some(norm(t, self.col))
    }

    pub(super) fn cursor_line(&self) -> Option<(u32, Side, &str)> {
        let (_, at) = self.cursor?;
        let r = self.cursor_row()?;
        Some((r, at.side, self.row_text(r, at.side)?))
    }

    /// The cursor's block: its side and the bytes of its line under it (empty on an empty line).
    pub(crate) fn block(&self) -> Option<(Side, Range<usize>)> {
        let (_, side, t) = self.cursor_line()?;
        let a = norm(t, self.col);
        Some((side, a..motion::next_grapheme(t, a)))
    }

    /// Turns a pending "this column" into the column `j` and `k` keep aiming for.
    pub(crate) fn aim(&mut self) {
        if self.want == Want::Here
            && let Some(c) = self.cursor_line().map(|(_, _, t)| motion::column(t, norm(t, self.col)))
        {
            self.want = Want::Col(c);
        }
    }

    /// The cursor was put on a line by a jump (a hunk, a click), from `prev`: a new line's first
    /// non-blank; the same line keeps its column.
    pub(crate) fn jumped(&mut self, prev: Option<(usize, LineRef)>) {
        if prev != self.cursor {
            self.col = self.cursor_line().map_or(0, |(_, _, t)| motion::first_non_blank(t));
            self.want = Want::Here;
        }
        self.sync_visual();
    }

    /// Puts the cursor on row `r`'s line on `side` at byte `col`, keeping it three lines off the
    /// viewport's edges (`reveal`) and its column on screen while lines don't wrap.
    pub(super) fn land(&mut self, r: u32, side: Side, col: usize, reveal: bool, tx: &mut Tx) {
        let before = self.cursor_row();
        let row = self.rows[r as usize];
        let Some(n) = self.row_line(row, side) else { return };
        self.cursor = Some((row.file(), LineRef::new(side, n)));
        self.col = col;
        self.visible = row.file();
        if reveal && (before != Some(r) || !self.in_view(r as usize)) {
            self.reveal_rows(r, tx);
        }
        self.reveal_column(r, side, tx);
        self.sync_visual();
    }

    /// A click on row `r`'s code on `side`, on the grapheme holding `byte`: the cursor goes there
    /// and, as in any editor, a selection (visual, Shift or lines) is dropped.
    pub(crate) fn click_code(&mut self, r: u32, side: Side, byte: usize, tx: &mut Tx) {
        self.end_visual();
        self.caret_visual = false;
        self.selection = None;
        self.span = None;
        let byte = self.row_text(r, side).map_or(0, |t| motion::snap(t, byte));
        self.land(r, side, byte, false, tx);
        self.want = Want::Here;
    }

    pub(super) fn land_at_want(&mut self, r: u32, side: Side, tx: &mut Tx) {
        let col = self.row_text(r, side).map_or(0, |t| self.want.byte_in(t));
        self.land(r, side, col, true, tx);
    }

    /// Scrolls the cursor row's margin box (see the view) just into view, unless the row
    /// already sits that far inside the viewport.
    fn reveal_rows(&self, r: u32, tx: &mut Tx) {
        let top = self.row_below_header().map_or(self.visible_rows.start, |t| t as usize);
        let m = SCROLLOFF as usize;
        let r = r as usize;
        if !self.visible_rows.is_empty() && top + m < r && r + m + 1 < self.visible_rows.end {
            return;
        }
        tx.scroll_into_view(format!("{}/vim-reveal", self.key_of(self.rows[r])));
    }

    /// Scrolls sideways by the least that shows the cursor's column, plus a margin; to the
    /// start when the column is near it.
    fn reveal_column(&self, r: u32, side: Side, tx: &mut Tx) {
        let w = self.view_w.get();
        if self.wraps() || w <= 0.0 {
            return;
        }
        let Some(t) = self.row_text(r, side) else { return };
        let digits = self.digits(self.rows[r as usize].file());
        let (adv, room) = (crate::view::ADV, w - crate::view::code_left(digits));
        let x = self.scroll.get()[2];
        let c = motion::column(t, self.col) as f32 * adv;
        let to = if c < x {
            (c - SIDE_MARGIN * adv).max(0.0)
        } else if c + adv > x + room {
            c + adv + SIDE_MARGIN * adv - room
        } else {
            return;
        };
        tx.scroll_to_x(self.scroll_key.borrow().as_str(), to);
    }

    /// The view keyed `view` tells the state its scroll viewport's key as it builds.
    pub(crate) fn set_scroll_key(&self, view: &str) {
        if self.scroll_key.borrow().strip_suffix("/scroll") != Some(view) {
            *self.scroll_key.borrow_mut() = format!("{view}/scroll");
        }
    }

    /// Drops a count or a key waiting for another, as a key Vim doesn't know does.
    pub fn vim_cancel(&mut self) {
        self.pending = Pending::default();
    }

    /// Whether a key waits for its next one (`g`, `y`, `f`, `t`), which is then Vim's.
    pub fn vim_waiting(&self) -> bool {
        self.pending.op != Op::None
    }

    /// Visual mode leaves; a linewise one takes its line selection with it.
    pub fn end_visual(&mut self) {
        if let Some(v) = self.visual.take()
            && v.line
        {
            self.selection = None;
        }
    }

    /// Visual mode follows the cursor: a linewise one as the line selection; it ends when the
    /// cursor leaves its file or the diff.
    pub(crate) fn sync_visual(&mut self) {
        let Some(v) = self.visual else { return };
        match self.cursor {
            Some((f, at)) if f == v.file && self.cursor_row().is_some() => {
                if v.line {
                    self.selection = Some(LineRange {
                        file: f,
                        start: v.at,
                        end: at,
                    });
                }
            }
            _ => self.end_visual(),
        }
    }

    pub(super) fn toggle_visual(&mut self, line: bool) {
        match self.visual {
            Some(v) if v.line == line => self.end_visual(),
            Some(v) => {
                if v.line {
                    self.selection = None;
                }
                self.visual = Some(Visual { line, ..v });
                self.sync_visual();
            }
            None => {
                let (Some((file, at)), Some(byte)) = (self.cursor, self.cursor_byte()) else {
                    return;
                };
                self.span = None;
                self.selection = None;
                self.caret_visual = false;
                self.visual = Some(Visual { line, file, at, byte });
                self.sync_visual();
            }
        }
    }

    /// Visual mode's extent, while its ends have rows.
    pub(crate) fn visual_span(&self) -> Option<VisualSpan> {
        let v = self.visual?;
        let ra = self.row_of(v.file, v.at.side, v.at.line)?;
        let ta = self.row_text(ra, v.at.side)?;
        let (rc, _, tc) = self.cursor_line()?;
        let a = (ra, norm(ta, v.byte), ta);
        let c = (rc, norm(tc, self.col), tc);
        let (first, last) = if (a.0, a.1) <= (c.0, c.1) { (a, c) } else { (c, a) };
        Some(VisualSpan {
            line: v.line,
            side: v.at.side,
            rows: (first.0, last.0),
            start: first.1,
            end: motion::next_grapheme(last.2, last.1),
        })
    }

    pub(crate) fn visual_side(&self, s: &VisualSpan, r: u32) -> Side {
        let row = self.rows[r as usize];
        if self.draw(row.file()) == Draw::Split {
            s.side
        } else {
            self.side_for(row, s.side)
        }
    }

    /// The text visual mode covers: whole lines each ending in a newline, or from character to
    /// character.
    pub(super) fn visual_text(&self) -> Option<String> {
        let s = self.visual_span()?;
        let mut out = String::new();
        let mut first = true;
        for r in s.rows.0..=s.rows.1 {
            if !self.rows[r as usize].has_line() {
                continue;
            }
            let Some(t) = self.row_full(r, self.visual_side(&s, r)) else {
                continue;
            };
            if s.line {
                out.push_str(t);
                out.push('\n');
                continue;
            }
            if !first {
                out.push('\n');
            }
            first = false;
            out.push_str(t.get(s.bytes(r, t.len()).unwrap_or(0..0)).unwrap_or(""));
        }
        Some(out)
    }

    /// Visual mode becomes the diff's own selection, for the composer: a span on one side, or
    /// the lines it covers.
    pub(super) fn visual_to_selection(&mut self) {
        let Some(v) = self.visual else { return };
        if v.line {
            self.visual = None;
            return;
        }
        let (Some(s), Some(text), Some((f, cur))) = (self.visual_span(), self.visual_text(), self.cursor) else {
            self.end_visual();
            return;
        };
        let line = |r: u32| self.row_line(self.rows[r as usize], s.side);
        let col = |r: u32, b: usize| self.row_text(r, s.side).map_or(0, |t| t[..b.min(t.len())].chars().count() as u32);
        self.span = match (cur.side == v.at.side, line(s.rows.0), line(s.rows.1)) {
            (true, Some(a), Some(b)) => Some(SpanSelection {
                file: f,
                side: s.side,
                start_line: a,
                start_col: col(s.rows.0, s.start),
                end_line: b,
                end_col: col(s.rows.1, s.end),
                text,
            }),
            _ => {
                self.selection = Some(LineRange {
                    file: f,
                    start: v.at,
                    end: cur,
                });
                None
            }
        };
        self.visual = None;
    }

    /// `n` lines from the cursor's, each ending in a newline (`yy`).
    fn yank_lines(&self, n: u32) -> Option<String> {
        let (r, side, _) = self.cursor_line()?;
        let lines = FileLines {
            st: self,
            f: self.rows[r as usize].file(),
            prefer: side,
        };
        let (mut at, mut out) = ((r, side), String::new());
        for k in 0..n.max(1) {
            out.push_str(self.row_full(at.0, at.1).unwrap_or(""));
            out.push('\n');
            match lines.next(at) {
                Some(next) if k + 1 < n => at = next,
                _ => break,
            }
        }
        Some(out)
    }

    /// Puts the cursor on the first line at the reading position when it has none on screen.
    pub(super) fn ensure_cursor(&mut self, tx: &mut Tx) -> bool {
        if self.cursor_row().is_some() {
            return true;
        }
        self.cursor = None;
        self.want = Want::Here;
        self.cursor_steps(true, 1, tx);
        false
    }

    pub(super) fn move_in_line(&mut self, to: impl FnOnce(&str, usize) -> Option<usize>, tx: &mut Tx) {
        let Some((r, side, t)) = self.cursor_line() else { return };
        let Some(b) = to(t, norm(t, self.col)) else { return };
        self.want = Want::Here;
        self.land(r, side, b, true, tx);
    }

    pub(super) fn word(&mut self, kind: u8, big: bool, n: u32, tx: &mut Tx) {
        let Some((r, side, t)) = self.cursor_line() else { return };
        let lines = FileLines {
            st: self,
            f: self.rows[r as usize].file(),
            prefer: side,
        };
        let mut p = Pos {
            line: (r, side),
            byte: norm(t, self.col),
        };
        match kind {
            b'w' => motion::word_forward(&lines, &mut p, n, big),
            b'e' => motion::word_end(&lines, &mut p, n, big),
            _ => motion::word_back(&lines, &mut p, n, big),
        };
        let byte = norm(lines.text(p.line), p.byte);
        self.want = Want::Here;
        self.land(p.line.0, p.line.1, byte, true, tx);
    }

    fn find_char(&mut self, c: char, forward: bool, till: bool, n: u32, again: bool, tx: &mut Tx) {
        if !again {
            self.last_find = Some((c, forward, till));
        }
        self.move_in_line(|t, at| motion::find(t, at, c, forward, till, n, again), tx);
    }

    /// `gg` / `G`: the file's first or last line, or with a count its line `count` on the
    /// cursor's side, at its first non-blank.
    pub(super) fn file_edge(&mut self, last: bool, count: u32, tx: &mut Tx) {
        let prefer = self.cursor.map_or(Side::New, |(_, at)| at.side);
        let f = self.cursor.map_or(self.visible, |(f, _)| f);
        if f >= self.files.len() {
            return;
        }
        if count > 0 {
            self.open_line(f, LineRef::new(prefer, count));
        }
        let lines = FileLines { st: self, f, prefer };
        let header = self.file_rows[f];
        let end = self.file_rows.get(f + 1).copied().unwrap_or(self.rows.len() as u32);
        let at = (count > 0)
            .then(|| {
                self.row_of(f, prefer, count)
                    .map(|r| (r, self.side_for(self.rows[r as usize], prefer)))
            })
            .flatten()
            .or_else(|| {
                if last || count > 0 {
                    lines.step(end, false)
                } else {
                    lines.step(header, true)
                }
            });
        let Some((r, side)) = at else { return };
        let col = self.row_text(r, side).map_or(0, motion::first_non_blank);
        self.want = Want::Here;
        self.land(r, side, col, true, tx);
    }

    /// Ctrl+D / Ctrl+U (`part` 0.5), Page Down / Page Up: that part of a viewport down or up,
    /// the cursor moving as many lines and keeping its place on screen.
    pub(super) fn half_page(&mut self, down: bool, part: f32, tx: &mut Tx) {
        let [y, h, _] = self.scroll.get();
        let h = if h > 0.0 { h } else { 30.0 * self.m.line };
        let n = (h * part / self.m.line).round().max(1.0) as u32;
        let from = match self.cursor_row() {
            Some(r) if self.visible_rows.is_empty() || self.visible_rows.contains(&(r as usize)) => r,
            _ => self.row_below_header().unwrap_or(0),
        };
        let Some(r) = self.step_rows(i64::from(from), down, n) else {
            return;
        };
        let dy = self.row_y(r as usize) - self.row_y(from as usize);
        tx.scroll_to_y(self.scroll_key.borrow().as_str(), (y + dy).max(0.0));
        let prefer = self.cursor.map_or(Side::New, |(_, at)| at.side);
        self.aim();
        let side = self.side_for(self.rows[r as usize], prefer);
        let col = self.row_text(r, side).map_or(0, |t| self.want.byte_in(t));
        self.land(r, side, col, false, tx);
    }

    /// A key that only waits for the next (or drops what waited).
    fn wait(&mut self, p: Pending) -> Vim {
        self.pending = p;
        Vim::Wait
    }

    /// One normal- or visual-mode key, with whatever count or first key came before it.
    pub fn vim(&mut self, key: VimKey, tx: &mut Tx) -> Vim {
        let p = std::mem::take(&mut self.pending);
        let n = p.count.max(1);
        if let Op::Find { forward, till } = p.op {
            return match key {
                VimKey::Char(c) => {
                    self.find_char(c, forward, till, n, false, tx);
                    Vim::Done
                }
                _ => self.wait(Pending::default()),
            };
        }
        let c = match key {
            VimKey::Escape if p != Pending::default() => return self.wait(Pending::default()),
            VimKey::Escape if self.visual.is_some() => {
                self.end_visual();
                return Vim::Done;
            }
            VimKey::Escape => return Vim::Pass,
            VimKey::HalfDown | VimKey::HalfUp => {
                self.half_page(key == VimKey::HalfDown, 0.5, tx);
                return Vim::Done;
            }
            VimKey::Char(c) => c,
        };
        match (p.op, c) {
            (Op::G, 'g') => {
                self.file_edge(false, p.count, tx);
                return Vim::Done;
            }
            (Op::Y, 'y') => {
                return match self.yank_lines(n) {
                    Some(t) => Vim::Yank(t),
                    None => Vim::Done,
                };
            }
            (Op::G | Op::Y, _) => return self.wait(Pending::default()),
            _ => {}
        }
        if let Some(d) = c.to_digit(10)
            && (d > 0 || p.count > 0)
        {
            let count = (p.count * 10 + d).min(MAX_COUNT);
            return self.wait(Pending { count, op: Op::None });
        }
        let in_visual = self.visual.is_some();
        let count = p.count;
        match c {
            'g' => return self.wait(Pending { count, op: Op::G }),
            'G' => self.file_edge(true, p.count, tx),
            'f' | 't' => {
                let op = Op::Find {
                    forward: true,
                    till: c == 't',
                };
                return self.wait(Pending { count, op });
            }
            'y' if in_visual => {
                let text = self.visual_text();
                self.end_visual();
                return text.map_or(Vim::Done, Vim::Yank);
            }
            'y' => return self.wait(Pending { count, op: Op::Y }),
            'c' | '?' if in_visual => {
                self.visual_to_selection();
                return if c == 'c' { Vim::Comment } else { Vim::Ask };
            }
            'v' | 'V' => {
                self.ensure_cursor(tx);
                self.toggle_visual(c == 'V');
            }
            'j' | 'k' => {
                self.cursor_steps(c == 'j', n, tx);
            }
            'h' | 'l' | 'w' | 'b' | 'e' | 'W' | 'B' | '0' | '^' | '$' | ';' | ',' if !self.ensure_cursor(tx) => {}
            'h' | 'l' => self.step_chars(c == 'l', n, tx),
            'w' | 'e' | 'b' => self.word(c as u8, false, n, tx),
            'W' | 'B' => self.word(c.to_ascii_lowercase() as u8, true, n, tx),
            '0' => self.move_in_line(|_, _| Some(0), tx),
            '^' => self.move_in_line(|t, _| Some(motion::first_non_blank(t)), tx),
            '$' => {
                if n > 1 {
                    self.cursor_steps(true, n - 1, tx);
                }
                self.want = Want::End;
                if let Some((r, side, _)) = self.cursor_line() {
                    self.land_at_want(r, side, tx);
                }
            }
            ';' | ',' => {
                if let Some((ch, forward, till)) = self.last_find {
                    self.find_char(ch, forward == (c == ';'), till, n, true, tx);
                }
            }
            _ => return Vim::Pass,
        }
        Vim::Done
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::state::DiffMode;
    use crate::testing::from_texts;

    const TEXT: &str = "fn alpha(beta, gamma) {\n\tlet x = 1;\n\n    a,b,c,d;\n}\n";

    fn added(text: &str) -> DiffState {
        let mut st = DiffState::new(vec![from_texts("v.rs", "", text)]);
        st.cursor_step(true, &mut Tx::default());
        st
    }

    fn keys(st: &mut DiffState, keys: &str) -> Vim {
        let mut last = Vim::Pass;
        for c in keys.chars() {
            let k = if c == '\u{1b}' { VimKey::Escape } else { VimKey::Char(c) };
            last = st.vim(k, &mut Tx::default());
        }
        last
    }

    fn at(st: &DiffState) -> (u32, usize) {
        (st.cursor().unwrap().1.line, st.cursor_byte().unwrap())
    }

    #[test]
    fn motions_walk_a_file_as_vim_does() {
        let mut st = added(TEXT);
        assert_eq!(at(&st), (1, 0));
        let mut go = |k: &str| {
            keys(&mut st, k);
            at(&st)
        };
        assert_eq!(go("w"), (1, 3));
        assert_eq!(go("w"), (1, 8));
        assert_eq!(go("2w"), (1, 13));
        assert_eq!(go("0"), (1, 0));
        assert_eq!(go("3w"), (1, 9), "a count repeats the motion");
        assert_eq!(go("e"), (1, 12));
        assert_eq!(go("b"), (1, 9));
        assert_eq!(go("$"), (1, 22));
        assert_eq!(go("j"), (2, 10), "$ then j stays at the end");
        assert_eq!(go("j"), (3, 0), "an empty line");
        assert_eq!(go("j"), (4, 11));
        assert_eq!(go("^"), (4, 4));
        assert_eq!(go("k"), (3, 0));
        assert_eq!(go("k"), (2, 1), "column 4 of a tab-led line is past the tab");
        assert_eq!(go("w"), (2, 5));
        assert_eq!(go("$"), (2, 10));
        assert_eq!(go("w"), (3, 0), "w stops on an empty line");
        assert_eq!(go("w"), (4, 4));
        assert_eq!(go("f,"), (4, 5));
        assert_eq!(go(";"), (4, 7));
        assert_eq!(go(","), (4, 5));
        assert_eq!(go("2f,"), (4, 9));
        assert_eq!(go("t;"), (4, 10));
        assert_eq!(go("fz"), (4, 10), "no match: no move");
        assert_eq!(go("h"), (4, 9));
        assert_eq!(go("2l"), (4, 11));
        assert_eq!(go("l"), (5, 0), "l at the last character goes on to the next line");
        assert_eq!(go("h"), (4, 11), "h at the first goes back to the line before's end");
        assert_eq!(go("3h"), (4, 8));
        assert_eq!(go("4l"), (5, 0), "a count runs across the line's end");
        assert_eq!(go("l"), (5, 0), "and stops at the file's end");
        assert_eq!(go("k"), (4, 0));
        assert_eq!(go("2h"), (2, 10), "an empty line counts as one stop");
        assert_eq!(go("gg"), (1, 0));
        assert_eq!(go("G"), (5, 0));
        assert_eq!(go("4G"), (4, 4), "a count picks the line, at its first non-blank");
        assert_eq!(go("2gg"), (2, 1));
        assert_eq!(go("3\u{1b}w"), (2, 5), "Escape drops the count");
        assert_eq!(go("g\u{1b}w"), (2, 7), "and a waiting g");
        assert_eq!(go("W"), (2, 9));
        assert_eq!(go("B"), (2, 7));
    }

    #[test]
    fn acts_alone_names_exactly_the_keys_normal_mode_takes() {
        for c in (0x21u8..0x7f).map(char::from) {
            let mut st = added(TEXT);
            let took = st.vim(VimKey::Char(c), &mut Tx::default()) != Vim::Pass;
            assert_eq!(VimKey::Char(c).acts_alone(), took, "{c}");
            assert_eq!(st.vim_waiting(), matches!(c, 'g' | 'y' | 'f' | 't'), "{c}");
        }
        assert!(VimKey::HalfDown.acts_alone() && VimKey::HalfUp.acts_alone() && !VimKey::Escape.acts_alone());
    }

    #[test]
    fn a_click_drops_any_selection_and_moves_the_cursor() {
        let mut st = added(TEXT);
        for k in ["vll", "Vj"] {
            keys(&mut st, k);
            assert!(st.visual().is_some());
            let r = st.row_of(0, Side::New, 4).unwrap();
            st.click_code(r, Side::New, 2, &mut Tx::default());
            assert!(
                st.visual().is_none() && st.selection().is_none() && st.span_selection().is_none(),
                "{k}"
            );
            assert_eq!(at(&st), (4, 2));
        }
    }

    #[test]
    fn visual_modes_extend_with_motions_and_yank() {
        let mut st = added(TEXT);
        keys(&mut st, "w");
        assert_eq!(keys(&mut st, "vey"), Vim::Yank("alpha".into()));
        assert!(st.visual().is_none(), "y leaves visual mode");
        keys(&mut st, "v");
        assert!(st.visual().is_some_and(|v| !v.line));
        keys(&mut st, "j\u{1b}");
        assert!(st.visual().is_none());
        assert_eq!(keys(&mut st, "\u{1b}"), Vim::Pass, "the app's Escape takes it next");
        keys(&mut st, "ggVj");
        let sel = st.selection().expect("V selects lines");
        assert_eq!((sel.start.line, sel.end.line), (1, 2));
        assert_eq!(keys(&mut st, "y"), Vim::Yank("fn alpha(beta, gamma) {\n\tlet x = 1;\n".into()));
        assert!(st.selection().is_none() && st.visual().is_none());
        assert_eq!(keys(&mut st, "yy"), Vim::Yank("\tlet x = 1;\n".into()));
        assert_eq!(keys(&mut st, "gg2yy"), Vim::Yank("fn alpha(beta, gamma) {\n\tlet x = 1;\n".into()));
        keys(&mut st, "$vjj");
        assert_eq!(
            keys(&mut st, "y"),
            Vim::Yank("{\n\tlet x = 1;\n".into()),
            "from the brace to the empty line"
        );
        keys(&mut st, "ggwve");
        assert_eq!(keys(&mut st, "c"), Vim::Comment);
        let sp = st.span_selection().expect("c leaves the span for the composer");
        assert_eq!(
            (sp.start_line, sp.start_col, sp.end_line, sp.end_col, sp.text.as_str()),
            (1, 3, 1, 8, "alpha")
        );
        assert!(st.visual().is_none());
        keys(&mut st, "Vj");
        assert_eq!(keys(&mut st, "?"), Vim::Ask);
        assert!(st.selection().is_some_and(|s| s.anchor()
            == rain_core::Anchor::Range {
                side: Side::New,
                start: 1,
                end: 2
            }));
    }

    #[test]
    fn split_keeps_the_cursor_on_its_side() {
        let old = "let old_name = 1;\nkeep\n";
        let new = "let new_name = 22;\nkeep\n";
        let mut st = DiffState::new(vec![from_texts("s.rs", old, new)]);
        st.set_mode(DiffMode::Split);
        let r = st.row_of(0, Side::Old, 1).unwrap();
        st.set_cursor(Some((0, LineRef::new(Side::Old, 1))));
        assert_eq!(st.cursor_row(), Some(r));
        keys(&mut st, "$");
        assert_eq!(st.cursor_byte(), Some(16), "the old side's end");
        keys(&mut st, "0w");
        assert_eq!((st.cursor().unwrap().1.side, st.cursor_byte()), (Side::Old, Some(4)));
        assert_eq!(st.block(), Some((Side::Old, 4..5)));
    }

    fn timed(st: &mut DiffState, k: &str) -> f64 {
        let t = Instant::now();
        keys(st, k);
        t.elapsed().as_secs_f64() * 1e3
    }

    #[test]
    fn motions_on_a_minified_line_stay_well_under_a_frame() {
        let line: String = "var a=b.c(d,e)+f[g];".repeat(10_000);
        let mut st = added(&format!("{line}\n{}\n", "x".repeat(200_000)));
        st.set_viewport([0.0, 0.0, 800.0, 600.0]);
        let mut worst = Vec::new();
        for k in ["w", "e", "b", "$", "b", "0", "f]", ";", "50%", "2w"] {
            worst.push((k, timed(&mut st, k)));
        }
        let (r, side, _) = st.cursor_line().unwrap();
        st.land(r, side, 100_000, true, &mut Tx::default());
        for k in ["w", "b", "e", "f;", ",", "$"] {
            worst.push((k, timed(&mut st, k)));
        }
        keys(&mut st, "j0");
        for k in ["w", "e", "$", "b", "0"] {
            worst.push((k, timed(&mut st, k)));
        }
        println!("200k-character lines, ms per key: {worst:?}");
        assert!(worst.iter().all(|&(_, ms)| ms < 2.0), "{worst:?}");
    }

    #[test]
    fn the_cursor_stays_in_the_drawn_part_of_a_very_long_line_and_copy_takes_all_of_it() {
        use crate::state::LINE_CAP;
        let long = "var a=b.c(d,e)+f[g];".repeat(100_000);
        let mut st = added(&format!("short\n{long}\nlast one\n"));
        let last = LINE_CAP - 1;
        let mut go = |k: &str| {
            keys(&mut st, k);
            at(&st)
        };
        assert_eq!(go("j$"), (2, last), "$ is the last drawn character");
        assert_eq!(go("l"), (3, 0), "l goes on to the next line from there");
        assert_eq!(go("h"), (2, last));
        assert_eq!(go("w"), (3, 0), "w leaves the line at the cut");
        assert_eq!(go("b"), (2, last - 1), "b comes back to the last drawn word");
        assert_eq!(go("0"), (2, 0));
        assert_eq!(go("w"), (2, 4));
        assert_eq!(go("j$k"), (2, last), "k from a line's end lands on the cut, not past it");
        assert_eq!(go("0"), (2, 0));
        assert_eq!(go("99999l"), (3, 7), "a count walks the drawn part only");
        assert_eq!(go("k0"), (2, 0));

        assert_eq!(st.copy_text().map(|t| t.len()), Some(long.len()), "Ctrl+C takes the whole line");
        assert_eq!(keys(&mut st, "yy"), Vim::Yank(format!("{long}\n")));
        assert_eq!(keys(&mut st, "Vy"), Vim::Yank(format!("{long}\n")));
        keys(&mut st, "ggvjj");
        assert_eq!(
            keys(&mut st, "y"),
            Vim::Yank(format!("short\n{long}\nl")),
            "a selection running through the line takes all of it"
        );
        keys(&mut st, "ggjv$");
        assert_eq!(keys(&mut st, "y"), Vim::Yank(long[..LINE_CAP].to_owned()), "v$ takes what is drawn");
        st.set_selection(Some(LineRange::line(0, LineRef::new(Side::New, 2))));
        assert_eq!(st.selected_text().map(|t| t.len()), Some(long.len()));
    }

    #[test]
    fn three_w_over_many_blank_lines_is_fast() {
        let text = format!("start\n{}end\n", "   \n\t\n".repeat(25_000));
        let mut st = added(&text);
        let ms = timed(&mut st, "3w");
        println!("3w over 50k blank lines: {ms:.2} ms");
        assert_eq!(at(&st), (50_002, 2), "past every blank line to the last character");
        assert!(ms < 50.0, "{ms:.2} ms");
    }
}
