//! What the view needs from a core [`FileDiff`] beyond its fields: hunk bounds per side, the
//! file's language, whole-text line indexes for expanding context, and change blocks for pairing
//! removed lines with the added lines that replace them.

use rain_core::diff::{FileDiff, Hunk, LineKind, Side};

/// The first line number `h` covers on `side` (its start, or the line after it when it covers
/// none there).
pub(crate) fn first(h: &Hunk, side: Side) -> u32 {
    let (start, count) = match side {
        Side::Old => (h.old_start, h.old_count),
        Side::New => (h.new_start, h.new_count),
    };
    if count == 0 { start.saturating_add(1) } else { start.max(1) }
}

/// One past the last line number `h` covers on `side`.
pub(crate) fn end(h: &Hunk, side: Side) -> u32 {
    first(h, side).saturating_add(if side == Side::Old { h.old_count } else { h.new_count })
}

/// The header line as a patch writes it, `@@ -1,4 +1,5 @@ section`.
pub fn hunk_header(f: &FileDiff, h: &Hunk) -> String {
    let range = |start: u32, count: u32| {
        if count == 1 {
            format!("{start}")
        } else {
            format!("{start},{count}")
        }
    };
    let mut out = format!("@@ -{} +{} @@", range(h.old_start, h.old_count), range(h.new_start, h.new_count));
    let section = f.hunk_section(h);
    if !section.is_empty() {
        out.push(' ');
        out.push_str(section);
    }
    out
}

pub(crate) fn number(f: &FileDiff, i: u32, side: Side) -> Option<u32> {
    let l = &f.lines[i as usize];
    let n = if side == Side::Old { l.old } else { l.new };
    (n != 0).then_some(n)
}

/// The highlighter language for the path's extension (or name, for a `Makefile`), as written:
/// `"rs"`, `"tsx"`.
pub fn language(path: &str) -> Option<&str> {
    let name = path.rsplit('/').next()?;
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => Some(ext),
        _ => Some(name).filter(|n| matches!(*n, "Makefile" | "Dockerfile")),
    }
}

/// Each line's byte range in a whole text, without its line break.
#[derive(Debug, Default)]
pub(crate) struct LineIndex {
    lines: Vec<(u32, u32)>,
}

impl LineIndex {
    pub(crate) fn new(text: &str) -> LineIndex {
        let mut lines = Vec::new();
        let mut start = 0;
        for (i, _) in text.match_indices('\n') {
            let end = if text[..i].ends_with('\r') { i - 1 } else { i };
            lines.push((start as u32, end as u32));
            start = i + 1;
        }
        if start < text.len() {
            let end = if text.ends_with('\r') { text.len() - 1 } else { text.len() };
            lines.push((start as u32, end as u32));
        }
        LineIndex { lines }
    }

    /// Line `n` (1-based) of `text`, the text this index was built from.
    pub(crate) fn line<'a>(&self, text: &'a str, n: u32) -> Option<&'a str> {
        let &(a, b) = self.lines.get((n as usize).checked_sub(1)?)?;
        text.get(a as usize..b as usize)
    }

    pub(crate) fn len(&self) -> u32 {
        self.lines.len() as u32
    }
}

/// A run of removed lines `d0..a0` followed by the added lines `a0..e` (indexes into
/// [`FileDiff::lines`]); either run may be empty.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub(crate) d0: u32,
    pub(crate) a0: u32,
    pub(crate) e: u32,
    /// Its lines pair by likeness (see [`crate::pairing`]), not in order.
    pub(crate) by_text: bool,
    /// Both runs are the same code laid out differently (see [`rain_core::formatting`]).
    pub(crate) quiet: bool,
}

impl Block {
    pub(crate) fn pairs(&self) -> u32 {
        (self.a0 - self.d0).min(self.e - self.a0)
    }

    /// The line paired with `i` when this block pairs in order, if any.
    pub(crate) fn partner(&self, i: u32) -> Option<u32> {
        if i < self.a0 {
            let k = i - self.d0;
            (k < self.e - self.a0).then_some(self.a0 + k)
        } else {
            let k = i - self.a0;
            (k < self.a0 - self.d0).then_some(self.d0 + k)
        }
    }
}

/// The change blocks of `f`, in line order. One linear pass, one allocation.
pub(crate) fn blocks(f: &FileDiff) -> Vec<Block> {
    let mut out = Vec::new();
    for h in &f.hunks {
        let mut i = h.lines.start;
        while i < h.lines.end {
            if f.lines[i as usize].kind == LineKind::Context {
                i += 1;
                continue;
            }
            let d0 = i;
            while i < h.lines.end && f.lines[i as usize].kind == LineKind::Removed {
                i += 1;
            }
            let a0 = i;
            while i < h.lines.end && f.lines[i as usize].kind == LineKind::Added {
                i += 1;
            }
            out.push(Block {
                d0,
                a0,
                e: i,
                by_text: false,
                quiet: false,
            });
        }
    }
    out
}

pub(crate) fn block_of(blocks: &[Block], i: u32) -> Option<&Block> {
    let b = blocks.get(blocks.partition_point(|b| b.e <= i))?;
    (b.d0 <= i).then_some(b)
}

/// Columns `line` takes with tabs stopping every four.
pub(crate) fn columns(line: &str) -> u32 {
    rain_core::diff::display_columns(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rain_core::diff::{FileDiffBuilder, FileStatus};

    #[test]
    fn blocks_pair_removed_with_added_lines_in_order() {
        let mut b = FileDiffBuilder::new(0, "a", FileStatus::Modified);
        b.push_hunk(1, 5, 1, 4, "");
        b.push_line(LineKind::Context, 1, 1, "a");
        b.push_line(LineKind::Removed, 2, 0, "b");
        b.push_line(LineKind::Removed, 3, 0, "c");
        b.push_line(LineKind::Removed, 4, 0, "d");
        b.push_line(LineKind::Added, 0, 2, "B");
        b.push_line(LineKind::Context, 5, 3, "e");
        b.push_line(LineKind::Added, 0, 4, "f");
        let f = b.finish();
        let bl = blocks(&f);
        assert_eq!(bl.iter().map(|b| (b.d0, b.a0, b.e)).collect::<Vec<_>>(), [(1, 4, 5), (6, 6, 7)]);
        assert_eq!(block_of(&bl, 1).and_then(|b| b.partner(1)), Some(4));
        assert_eq!(block_of(&bl, 4).and_then(|b| b.partner(4)), Some(1));
        assert_eq!(block_of(&bl, 2).and_then(|b| b.partner(2)), None);
        assert!(block_of(&bl, 5).is_none());
        assert_eq!(block_of(&bl, 6).and_then(|b| b.partner(6)), None);
    }

    #[test]
    fn languages_come_from_the_extension() {
        assert_eq!(language("src/lib.rs"), Some("rs"));
        assert_eq!(language("web/App.tsx"), Some("tsx"));
        assert_eq!(language(".gitignore"), None);
        assert_eq!(language("Makefile"), Some("Makefile"));
    }
}
