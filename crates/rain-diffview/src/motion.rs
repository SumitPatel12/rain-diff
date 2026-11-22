//! Vim's normal-mode motions over lines of text, by byte offset: word classes, `w` `b` `e`
//! across lines, `f` `t` finds, grapheme steps and display columns. Each scan stays inside the
//! lines it walks and allocates nothing; ASCII takes a byte-at-a-time path.

use unicode_segmentation::GraphemeCursor;

pub(crate) trait Lines {
    type Id: Copy;
    fn text(&self, l: Self::Id) -> &str;
    fn next(&self, l: Self::Id) -> Option<Self::Id>;
    fn prev(&self, l: Self::Id) -> Option<Self::Id>;
}

/// A place in a line: `byte == text.len()` is past its end, where Vim's motions pass through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pos<L> {
    pub line: L,
    pub byte: usize,
}

/// The grapheme boundary after `at` (`at` itself at the end).
pub(crate) fn next_grapheme(s: &str, at: usize) -> usize {
    let b = s.as_bytes();
    if at >= b.len() {
        return b.len();
    }
    if b[at] < 0x80 && b.get(at + 1).is_none_or(|&c| c < 0x80) {
        return at + 1;
    }
    GraphemeCursor::new(at, s.len(), true)
        .next_boundary(s, 0)
        .ok()
        .flatten()
        .unwrap_or(s.len())
}

/// The grapheme boundary before `at` (0 at the start).
pub(crate) fn prev_grapheme(s: &str, at: usize) -> usize {
    let b = s.as_bytes();
    let at = at.min(b.len());
    if at == 0 {
        return 0;
    }
    if b[at - 1] < 0x80 && b.get(at).is_none_or(|&c| c < 0x80) {
        return at - 1;
    }
    GraphemeCursor::new(at, s.len(), true)
        .prev_boundary(s, 0)
        .ok()
        .flatten()
        .unwrap_or(0)
}

/// The start of the grapheme holding byte `at`; past the end, the last grapheme's start.
pub(crate) fn snap(s: &str, at: usize) -> usize {
    if at >= s.len() {
        return prev_grapheme(s, s.len());
    }
    if s.as_bytes()[at] < 0x80 && (at == 0 || s.as_bytes()[at - 1] < 0x80) {
        return at;
    }
    let mut at = at;
    while !s.is_char_boundary(at) {
        at -= 1;
    }
    let mut c = GraphemeCursor::new(at, s.len(), true);
    match c.is_boundary(s, 0) {
        Ok(false) => c.prev_boundary(s, 0).ok().flatten().unwrap_or(0),
        _ => at,
    }
}

fn char_at(s: &str, at: usize) -> Option<char> {
    s.get(at..)?.chars().next()
}

/// Vim's character class: 0 blank, 1 punctuation, 2 keyword, and one class per script Vim
/// splits words at (kana, CJK ideographs, Hangul, emoji). `big` makes every non-blank one class.
pub(crate) fn class(c: char, big: bool) -> u32 {
    if c == ' ' || c == '\t' || (!c.is_ascii() && c.is_whitespace()) {
        return 0;
    }
    if big {
        return 1;
    }
    if c.is_ascii() {
        return if c.is_ascii_alphanumeric() || c == '_' { 2 } else { 1 };
    }
    match c as u32 {
        0x3040..=0x309f => 0x3040,
        0x30a0..=0x30ff => 0x30a0,
        0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x2fa1f => 0x4e00,
        0xac00..=0xd7a3 => 0xac00,
        0x2600..=0x27bf | 0x1f000..=0x1faff => 3,
        _ if c.is_alphanumeric() => 2,
        _ => 1,
    }
}

/// [`class`] of each ASCII byte, for the scans' byte-at-a-time path (`[small, big]`).
static ASCII: [[u8; 128]; 2] = {
    let mut t = [[0u8; 128]; 2];
    let mut i = 0;
    while i < 128 {
        let c = i as u8;
        let blank = c == b' ' || c == b'\t';
        t[0][i] = if blank {
            0
        } else if c.is_ascii_alphanumeric() || c == b'_' {
            2
        } else {
            1
        };
        t[1][i] = if blank { 0 } else { 1 };
        i += 1;
    }
    t
};

/// The class at `at`; past the end of the line, blank.
pub(crate) fn class_at(s: &str, at: usize, big: bool) -> u32 {
    let b = s.as_bytes();
    match b.get(at) {
        None => 0,
        Some(&c) if c < 0x80 => u32::from(ASCII[usize::from(big)][c as usize]),
        Some(_) => char_at(s, at).map_or(0, |c| class(c, big)),
    }
}

/// The first byte from `at` whose class isn't `cls` (the end of the line at most). A combining
/// mark stays with the character it marks.
fn run_end(s: &str, mut at: usize, cls: u32, big: bool) -> usize {
    let b = s.as_bytes();
    let table = &ASCII[usize::from(big)];
    while at < b.len() {
        let c = b[at];
        if c < 0x80 {
            if u32::from(table[c as usize]) != cls {
                return at;
            }
            at += 1;
            continue;
        }
        let ch = char_at(s, at).unwrap_or(' ');
        if class(ch, big) != cls && GraphemeCursor::new(at, s.len(), true).is_boundary(s, 0) != Ok(false) {
            return at;
        }
        at += ch.len_utf8();
    }
    at
}

/// The first byte of the run of class `cls` that holds the character at `at`.
fn run_start(s: &str, at: usize, cls: u32, big: bool) -> usize {
    let mut start = at;
    let b = s.as_bytes();
    let table = &ASCII[usize::from(big)];
    while start > 0 {
        if b[start - 1] < 0x80 {
            if u32::from(table[b[start - 1] as usize]) != cls {
                break;
            }
            start -= 1;
            continue;
        }
        let mut p = start - 1;
        while !s.is_char_boundary(p) {
            p -= 1;
        }
        let ch = char_at(s, p).unwrap_or(' ');
        if class(ch, big) != cls && GraphemeCursor::new(start, s.len(), true).is_boundary(s, 0) != Ok(false) {
            break;
        }
        start = p;
    }
    snap(s, start)
}

/// Vim's `inc()`: one grapheme on, onto the end of the line, then to the next line's start.
/// 0 within the line, 2 onto its end, 1 onto the next line, -1 at the end of the last line.
fn inc<B: Lines>(b: &B, p: &mut Pos<B::Id>) -> i8 {
    let t = b.text(p.line);
    if p.byte < t.len() {
        p.byte = next_grapheme(t, p.byte);
        return if p.byte < t.len() { 0 } else { 2 };
    }
    match b.next(p.line) {
        Some(n) => {
            *p = Pos { line: n, byte: 0 };
            1
        }
        None => -1,
    }
}

/// Vim's `dec()`: one grapheme back, from a line's start onto the end of the line before.
fn dec<B: Lines>(b: &B, p: &mut Pos<B::Id>) -> i8 {
    if p.byte > 0 {
        p.byte = prev_grapheme(b.text(p.line), p.byte);
        return 0;
    }
    match b.prev(p.line) {
        Some(l) => {
            *p = Pos {
                line: l,
                byte: b.text(l).len(),
            };
            1
        }
        None => -1,
    }
}

fn cls<B: Lines>(b: &B, p: Pos<B::Id>, big: bool) -> u32 {
    class_at(b.text(p.line), p.byte, big)
}

/// `w` / `W`, Vim's `fwd_word`: the start of the next word, stopping on empty lines. False when
/// it ran out of lines.
pub(crate) fn word_forward<B: Lines>(b: &B, p: &mut Pos<B::Id>, count: u32, big: bool) -> bool {
    for _ in 0..count.max(1) {
        let sclass = cls(b, *p, big);
        let last_line = b.next(p.line).is_none();
        let i = inc(b, p);
        if i == -1 || (i >= 1 && last_line) {
            return false;
        }
        if sclass != 0 {
            p.byte = run_end(b.text(p.line), p.byte, sclass, big);
        }
        loop {
            let t = b.text(p.line);
            if t.is_empty() && p.byte == 0 {
                break;
            }
            p.byte = run_end(t, p.byte, 0, big);
            if p.byte < t.len() {
                break;
            }
            match b.next(p.line) {
                Some(n) => *p = Pos { line: n, byte: 0 },
                None => return true,
            }
        }
    }
    true
}

/// `e`, Vim's `end_word`: the last character of this or the next word, across empty lines.
pub(crate) fn word_end<B: Lines>(b: &B, p: &mut Pos<B::Id>, count: u32, big: bool) -> bool {
    for _ in 0..count.max(1) {
        let sclass = cls(b, *p, big);
        if inc(b, p) == -1 {
            return false;
        }
        let here = cls(b, *p, big);
        if here == sclass && sclass != 0 {
            p.byte = run_end(b.text(p.line), p.byte, sclass, big);
        } else {
            loop {
                let t = b.text(p.line);
                p.byte = run_end(t, p.byte, 0, big);
                if p.byte < t.len() {
                    break;
                }
                match b.next(p.line) {
                    Some(n) => *p = Pos { line: n, byte: 0 },
                    None => return false,
                }
            }
            let c = cls(b, *p, big);
            p.byte = run_end(b.text(p.line), p.byte, c, big);
        }
        dec(b, p);
    }
    true
}

/// `b` / `B`, Vim's `bck_word`: the start of this or the previous word, stopping on empty lines.
pub(crate) fn word_back<B: Lines>(b: &B, p: &mut Pos<B::Id>, count: u32, big: bool) -> bool {
    for _ in 0..count.max(1) {
        if dec(b, p) == -1 {
            return false;
        }
        let mut empty = false;
        loop {
            let t = b.text(p.line);
            if class_at(t, p.byte, big) != 0 {
                break;
            }
            if t.is_empty() {
                empty = true;
                break;
            }
            let mut j = p.byte.min(t.len());
            while j > 0 && class_at(t, prev_grapheme(t, j), big) == 0 {
                j = prev_grapheme(t, j);
            }
            if j > 0 {
                p.byte = prev_grapheme(t, j);
                break;
            }
            match b.prev(p.line) {
                Some(l) => {
                    *p = Pos {
                        line: l,
                        byte: b.text(l).len(),
                    }
                }
                None => {
                    p.byte = 0;
                    return true;
                }
            }
        }
        if !empty {
            let t = b.text(p.line);
            p.byte = run_start(t, p.byte, class_at(t, p.byte, big), big);
        }
    }
    true
}

/// Where `f` / `t` (forward) or `F` / `T` (back) for `c` lands in `s` from `at`, the `count`th
/// match; `again` is a `;` / `,` repeat, which steps over a match `t` already stands before.
pub(crate) fn find(s: &str, at: usize, c: char, forward: bool, till: bool, count: u32, again: bool) -> Option<usize> {
    let mut from = at;
    let mut hit = at;
    for k in 0..count.max(1) {
        if forward {
            let mut start = next_grapheme(s, from);
            if till && again && k == 0 {
                start = next_grapheme(s, start);
            }
            hit = start + s.get(start..)?.find(c)?;
            from = hit;
        } else {
            let mut end = from;
            if till && again && k == 0 {
                end = prev_grapheme(s, end);
            }
            hit = s.get(..end)?.rfind(c)?;
            from = hit;
        }
    }
    let hit = snap(s, hit);
    Some(match (till, forward) {
        (false, _) => hit,
        (true, true) => prev_grapheme(s, hit),
        (true, false) => next_grapheme(s, hit),
    })
}

/// The first non-blank of `s` (`^`); a blank line's last character.
pub(crate) fn first_non_blank(s: &str) -> usize {
    let at = s.bytes().position(|c| c != b' ' && c != b'\t').unwrap_or(s.len());
    snap(s, at)
}

/// The display column of byte `at`: one per grapheme, tabs to the next stop of four.
pub(crate) fn column(s: &str, at: usize) -> u32 {
    let b = s.as_bytes();
    let at = at.min(b.len());
    let (mut i, mut col) = (0, 0u32);
    while i < at {
        let c = b[i];
        if c < 0x80 && (i + 1 >= b.len() || b[i + 1] < 0x80) {
            col = if c == b'\t' { col / 4 * 4 + 4 } else { col + 1 };
            i += 1;
        } else {
            col += 1;
            i = next_grapheme(s, i);
        }
    }
    col
}

/// The grapheme of `s` covering display column `col`; past the end, the last grapheme.
pub(crate) fn at_column(s: &str, col: u32) -> usize {
    let b = s.as_bytes();
    let (mut i, mut c) = (0, 0u32);
    while i < b.len() {
        let next = if b[i] == b'\t' { c / 4 * 4 + 4 } else { c + 1 };
        if next > col {
            return i;
        }
        c = next;
        let n = next_grapheme(s, i);
        if n >= b.len() {
            return i;
        }
        i = n;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Buf<'a>(&'a [&'a str]);

    impl Lines for Buf<'_> {
        type Id = usize;
        fn text(&self, l: usize) -> &str {
            self.0[l]
        }
        fn next(&self, l: usize) -> Option<usize> {
            (l + 1 < self.0.len()).then_some(l + 1)
        }
        fn prev(&self, l: usize) -> Option<usize> {
            l.checked_sub(1)
        }
    }

    fn walk(lines: &[&str], from: (usize, usize), f: impl Fn(&Buf, &mut Pos<usize>) -> bool) -> (usize, usize) {
        let b = Buf(lines);
        let mut p = Pos {
            line: from.0,
            byte: from.1,
        };
        f(&b, &mut p);
        let t = lines[p.line];
        (p.line, if t.is_empty() { 0 } else { snap(t, p.byte) })
    }

    #[test]
    fn w_stops_at_class_changes_and_empty_lines() {
        let lines = ["foo.bar(baz)  qux", "", "   x", "last"];
        let w = |from, n| walk(&lines, from, |b, p| word_forward(b, p, n, false));
        assert_eq!(w((0, 0), 1), (0, 3));
        assert_eq!(w((0, 3), 1), (0, 4));
        assert_eq!(w((0, 0), 4), (0, 8));
        assert_eq!(w((0, 11), 1), (0, 14));
        assert_eq!(w((0, 14), 1), (1, 0), "an empty line is a word");
        assert_eq!(w((1, 0), 1), (2, 3));
        assert_eq!(w((0, 0), 7), (1, 0));
        assert_eq!(w((0, 0), 8), (2, 3));
        assert_eq!(w((3, 0), 1), (3, 3), "the last word goes to the last character");
        let big = |from| walk(&lines, from, |b, p| word_forward(b, p, 1, true));
        assert_eq!(big((0, 0)), (0, 14));
    }

    #[test]
    fn e_and_b_mirror_vim() {
        let lines = ["foo.bar(baz)  qux", "", "   x", "last"];
        let e = |from, n| walk(&lines, from, |b, p| word_end(b, p, n, false));
        assert_eq!(e((0, 0), 1), (0, 2));
        assert_eq!(e((0, 2), 1), (0, 3));
        assert_eq!(e((0, 4), 1), (0, 6));
        assert_eq!(e((0, 14), 1), (0, 16));
        assert_eq!(e((0, 16), 1), (2, 3), "e skips empty lines");
        let b = |from, n| walk(&lines, from, |b, p| word_back(b, p, n, false));
        assert_eq!(b((0, 16), 1), (0, 14));
        assert_eq!(b((0, 14), 1), (0, 11));
        assert_eq!(b((0, 6), 1), (0, 4));
        assert_eq!(b((2, 3), 1), (1, 0), "b stops on an empty line");
        assert_eq!(b((1, 0), 1), (0, 14));
        assert_eq!(b((0, 2), 1), (0, 0));
        assert_eq!(b((0, 0), 1), (0, 0));
        assert_eq!(b((3, 0), 2), (1, 0));
        assert_eq!(b((3, 0), 3), (0, 14));
    }

    #[test]
    fn words_in_unicode_and_tabs() {
        let lines = ["\tcafé\u{301} = naïve_x;", "日本語abc 👍👍 x"];
        let w = |from, n| walk(&lines, from, |b, p| word_forward(b, p, n, false));
        assert_eq!(w((0, 0), 1), (0, 1));
        assert_eq!(w((0, 1), 1), (0, 9), "the combining accent stays in the word");
        assert_eq!(w((1, 0), 1), (1, 9), "ideographs and Latin are different words");
        assert_eq!(w((1, 9), 1), (1, 13));
        assert_eq!(w((1, 13), 1), (1, 22));
        let e = walk(&lines, (0, 1), |b, p| word_end(b, p, 1, false));
        assert_eq!(e, (0, 4), "e lands on the accented letter's start");
        assert_eq!(next_grapheme(lines[0], 4), 8);
        assert_eq!(prev_grapheme(lines[0], 8), 4);
        assert_eq!(snap(lines[0], 7), 4);
        assert_eq!(column(lines[0], 1), 4);
        assert_eq!(column(lines[0], 8), 8);
        assert_eq!(at_column(lines[0], 2), 0, "inside the tab");
        assert_eq!(at_column(lines[0], 8), 8);
        assert_eq!(at_column(lines[0], 99), snap(lines[0], 99));
    }

    #[test]
    fn find_and_till() {
        let s = "a,b,c,d";
        assert_eq!(find(s, 0, ',', true, false, 1, false), Some(1));
        assert_eq!(find(s, 0, ',', true, false, 2, false), Some(3));
        assert_eq!(find(s, 0, ',', true, true, 1, false), Some(0));
        assert_eq!(find(s, 0, ',', true, true, 1, true), Some(2), "; after t steps past the match");
        assert_eq!(find(s, 6, ',', false, false, 1, false), Some(5));
        assert_eq!(find(s, 6, ',', false, true, 1, false), Some(6));
        assert_eq!(find(s, 6, ',', false, true, 1, true), Some(4));
        assert_eq!(find(s, 0, 'z', true, false, 1, false), None);
        assert_eq!(find(s, 0, ',', true, false, 9, false), None);
        assert_eq!(first_non_blank("  \tx y"), 3);
        assert_eq!(first_non_blank("   "), 2);
    }
}
