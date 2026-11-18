//! The cursor keys every profile shares, as an editor has them: arrows by character and line,
//! Ctrl+arrows by word, Home and End, Ctrl+Home and Ctrl+End, Page Up and Page Down. With Shift
//! each extends a selection, which is visual mode started for it, so copy, comment and ask take
//! it as they take Vim's. Ctrl+A selects the file's lines; Ctrl+C copies.

use rain_core::diff::Side;
use rune::Tx;

use super::vim::{FileLines, norm};
use super::{DiffState, LineRange, LineRef};
use crate::motion::{self, Lines};

/// A cursor key for [`DiffState::caret`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caret {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    /// The first non-blank, then column 0.
    Home,
    End,
    FileStart,
    FileEnd,
    PageUp,
    PageDown,
}

/// A page is most of the viewport, as a scroll view's Page Down is.
const PAGE: f32 = 0.9;

impl DiffState {
    /// One cursor key; `extend` (Shift) grows the selection from where the cursor was. A plain
    /// key drops a selection Shift made, and moves on in a visual mode `v` started. With no
    /// cursor on screen the key only puts it there.
    pub fn caret(&mut self, key: Caret, extend: bool, tx: &mut Tx) -> bool {
        self.pending = super::Pending::default();
        if !self.ensure_cursor(tx) {
            if extend {
                self.start_caret_visual();
            }
            return self.cursor.is_some();
        }
        if extend {
            self.start_caret_visual();
        } else if self.caret_visual && self.visual.is_some() {
            self.end_visual();
        }
        match key {
            Caret::Left => self.step_chars(false, 1, tx),
            Caret::Right => self.step_chars(true, 1, tx),
            Caret::Up | Caret::Down => {
                self.cursor_steps(key == Caret::Down, 1, tx);
            }
            Caret::WordLeft => self.word(b'b', false, 1, tx),
            Caret::WordRight => self.word(b'w', false, 1, tx),
            Caret::Home => self.move_in_line(
                |t, at| {
                    let first = motion::first_non_blank(t);
                    Some(if at == first { 0 } else { first })
                },
                tx,
            ),
            Caret::End => {
                self.want = super::Want::End;
                if let Some((r, side, _)) = self.cursor_line() {
                    self.land_at_want(r, side, tx);
                }
            }
            Caret::FileStart | Caret::FileEnd => self.file_edge(key == Caret::FileEnd, 0, tx),
            Caret::PageUp | Caret::PageDown => self.half_page(key == Caret::PageDown, PAGE, tx),
        }
        true
    }

    fn start_caret_visual(&mut self) {
        if self.visual.is_none() && self.cursor.is_some() {
            self.toggle_visual(false);
            self.caret_visual = true;
        }
    }

    /// Left or Right by `n` characters (arrows, `h`, `l`), onto the line before or after at
    /// either end; it stops at the file's ends.
    pub(super) fn step_chars(&mut self, forward: bool, n: u32, tx: &mut Tx) {
        let Some((r, side, t)) = self.cursor_line() else { return };
        let lines = FileLines {
            st: self,
            f: self.rows[r as usize].file(),
            prefer: side,
        };
        let (mut line, mut t, mut at) = ((r, side), t, norm(t, self.col));
        for _ in 0..n.max(1) {
            let next = motion::next_grapheme(t, at);
            if forward && next < t.len() {
                at = next;
            } else if !forward && at > 0 {
                at = motion::prev_grapheme(t, at);
            } else {
                let Some(to) = (if forward { lines.next(line) } else { lines.prev(line) }) else {
                    break;
                };
                line = to;
                t = lines.text(to);
                at = if forward { 0 } else { norm(t, usize::MAX) };
            }
        }
        self.want = super::Want::Here;
        self.land(line.0, line.1, at, true, tx);
    }

    /// Ctrl+A: every line of the cursor's file (else the one on screen), on the cursor's side.
    pub fn select_file(&mut self) -> bool {
        let prefer = self.cursor.map_or(Side::New, |(_, at)| at.side);
        let f = self.cursor.map_or(self.visible, |(f, _)| f);
        let Some(&header) = self.file_rows.get(f) else { return false };
        let end = self.file_rows.get(f + 1).copied().unwrap_or(self.rows.len() as u32);
        let lines = FileLines { st: self, f, prefer };
        let (Some(a), Some(b)) = (lines.step(header, true), lines.step(end, false)) else {
            return false;
        };
        let line = |(r, side): (u32, Side)| self.row_line(self.rows[r as usize], side).map(|n| LineRef::new(side, n));
        let (Some(start), Some(end)) = (line(a), line(b)) else {
            return false;
        };
        self.end_visual();
        self.span = None;
        self.selection = Some(LineRange { file: f, start, end });
        true
    }

    /// What Ctrl+C copies from the diff: the visual selection, the line selection, else the
    /// cursor's line. `None` while a text selection is up, which copies itself.
    pub fn copy_text(&self) -> Option<String> {
        if self.span.is_some() {
            return None;
        }
        if self.visual.is_some_and(|v| !v.line) {
            return self.visual_text();
        }
        if self.selection.is_some() {
            return self.selected_text();
        }
        self.cursor_line().map(|(_, _, t)| t.to_owned())
    }

    /// Visual mode becomes the line or span selection the composer reads, as `c` does in it.
    pub fn take_visual_selection(&mut self) {
        if self.visual.is_some() {
            self.visual_to_selection();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::DiffMode;
    use crate::testing::from_texts;

    const TEXT: &str = "fn alpha(beta, gamma) {\n    let x = 1;\n\n    a,b;\n}\n";

    fn added(text: &str) -> DiffState {
        let mut st = DiffState::new(vec![from_texts("v.rs", "", text)]);
        st.cursor_step(true, &mut Tx::default());
        st
    }

    fn go(st: &mut DiffState, keys: &[Caret], extend: bool) -> (u32, usize) {
        for k in keys {
            st.caret(*k, extend, &mut Tx::default());
        }
        (st.cursor().unwrap().1.line, st.cursor_byte().unwrap())
    }

    #[test]
    fn arrows_home_end_and_words_move_as_an_editor_does() {
        use Caret::*;
        let mut st = added(TEXT);
        assert_eq!(go(&mut st, &[Right, Right], false), (1, 2));
        assert_eq!(go(&mut st, &[Down], false), (2, 2), "Down keeps the column");
        assert_eq!(go(&mut st, &[End], false), (2, 13));
        assert_eq!(go(&mut st, &[Right], false), (3, 0), "Right at the end wraps to the next line");
        assert_eq!(go(&mut st, &[Left], false), (2, 13), "Left at the start wraps back");
        assert_eq!(go(&mut st, &[Home], false), (2, 4), "Home: the first non-blank");
        assert_eq!(go(&mut st, &[Home], false), (2, 0), "then column 0");
        assert_eq!(go(&mut st, &[Home], false), (2, 4));
        assert_eq!(go(&mut st, &[WordRight], false), (2, 8));
        assert_eq!(go(&mut st, &[WordLeft], false), (2, 4));
        assert_eq!(go(&mut st, &[FileEnd], false), (5, 0));
        assert_eq!(go(&mut st, &[FileStart], false), (1, 0));
        assert_eq!(go(&mut st, &[Up], false), (1, 0), "nothing above the first line");
        assert_eq!(go(&mut st, &[Left], false), (1, 0));
        assert_eq!(go(&mut st, &[PageDown], false).0, 5, "a page past a short file stops at its end");
        assert_eq!(go(&mut st, &[PageUp], false).0, 1);
    }

    #[test]
    fn shift_extends_a_selection_that_copies_and_a_plain_key_drops() {
        use Caret::*;
        let mut st = added(TEXT);
        go(&mut st, &[WordRight], false);
        go(&mut st, &[Right, Right, Right, Right], true);
        assert!(st.visual().is_some_and(|v| !v.line));
        assert_eq!(st.copy_text().as_deref(), Some("alpha"));
        go(&mut st, &[Down], true);
        assert_eq!(st.copy_text().as_deref(), Some("alpha(beta, gamma) {\n    let "));
        go(&mut st, &[Left], false);
        assert!(st.visual().is_none(), "a plain arrow ends a Shift selection");
        assert_eq!(st.copy_text().as_deref(), Some("    let x = 1;"), "nothing selected: the line");
        st.vim(crate::VimKey::Char('v'), &mut Tx::default());
        go(&mut st, &[Right], false);
        assert!(st.visual().is_some(), "an arrow moves on in Vim's own visual mode");
        st.end_visual();
        go(&mut st, &[End], true);
        st.take_visual_selection();
        assert!(st.visual().is_none());
        let sp = st.span_selection().expect("the composer's span");
        assert_eq!((sp.start_line, sp.end_line, sp.text.as_str()), (2, 2, " x = 1;"));
    }

    #[test]
    fn ctrl_a_selects_the_file_and_escape_clears_it() {
        let mut st = added(TEXT);
        assert!(st.select_file());
        let sel = st.selection().unwrap();
        assert_eq!((sel.start.line, sel.end.line), (1, 5));
        assert_eq!(st.copy_text().unwrap().lines().count(), 5);
        let esc = rune::KeyEvent {
            key: rune::Key::Escape,
            mods: rune::Mods::default(),
        };
        assert!(st.key(&esc, &mut Tx::default()));
        assert!(st.selection().is_none());
    }

    #[test]
    fn split_keeps_each_key_on_the_cursor_side() {
        let mut st = DiffState::new(vec![from_texts("s.rs", "let old_name = 1;\nkeep\n", "let new_name = 22;\nkeep\n")]);
        st.set_mode(DiffMode::Split);
        st.set_cursor(Some((0, LineRef::new(Side::Old, 1))));
        st.caret(Caret::End, false, &mut Tx::default());
        assert_eq!((st.cursor().unwrap().1.side, st.cursor_byte()), (Side::Old, Some(16)));
        st.caret(Caret::Right, false, &mut Tx::default());
        assert_eq!(st.cursor().unwrap().1, LineRef::new(Side::Old, 2));
        st.set_cursor(Some((0, LineRef::new(Side::New, 1))));
        st.caret(Caret::End, true, &mut Tx::default());
        assert_eq!(st.copy_text().as_deref(), Some("let new_name = 22;"));
    }
}
