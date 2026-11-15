//! The diff around an anchor, both sides, as the prompt quotes it.

use rain_core::{FileDiff, LineKind, Side};

/// Lines kept at most, so one huge hunk can't crowd out the question.
pub const MAX_LINES: usize = 60;

/// The diff lines from `ctx` above `lines` to `ctx` below them on `side`, with removed and added
/// lines in between kept so a modified range shows both sides: `old new ±text` per line. `None`
/// when no line of the file is in range.
pub fn around(f: &FileDiff, side: Side, lines: (u32, u32), ctx: u32) -> Option<String> {
    let (a, b) = (lines.0.min(lines.1), lines.0.max(lines.1));
    let (lo, hi) = (a.saturating_sub(ctx).max(1), b.saturating_add(ctx));
    let num = |i: usize| match side {
        Side::New => f.lines[i].new,
        Side::Old => f.lines[i].old,
    };
    let hit = |i: &usize| (lo..=hi).contains(&num(*i));
    let first = (0..f.lines.len()).find(hit)?;
    let last = (0..f.lines.len()).rev().find(hit)?;
    let mut out = String::new();
    for i in (first..=last).take(MAX_LINES) {
        let r = &f.lines[i];
        let mark = match r.kind {
            LineKind::Added => '+',
            LineKind::Removed => '-',
            LineKind::Context => ' ',
        };
        let n = |v: u32| if v == 0 { String::new() } else { v.to_string() };
        out.push_str(&format!("{:>5} {:>5} {mark}{}\n", n(r.old), n(r.new), f.line_text(i as u32)));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rain_core::LineRec;

    fn file(rows: &[(LineKind, u32, u32, &str)]) -> FileDiff {
        let mut f = FileDiff::default();
        for (kind, old, new, text) in rows {
            let start = f.text.len() as u32;
            f.text.push_str(text);
            f.lines.push(LineRec {
                kind: *kind,
                old: *old,
                new: *new,
                start,
                end: f.text.len() as u32,
                no_newline: false,
            });
            f.text.push('\n');
        }
        f
    }

    #[test]
    fn a_modified_range_shows_both_sides_and_context() {
        let f = file(&[
            (LineKind::Context, 9, 9, "a"),
            (LineKind::Context, 10, 10, "b"),
            (LineKind::Removed, 11, 0, "return total;"),
            (LineKind::Added, 0, 11, "return total + vouchers;"),
            (LineKind::Context, 12, 12, "}"),
            (LineKind::Context, 30, 30, "far"),
        ]);
        let got = around(&f, Side::New, (11, 11), 1).unwrap();
        assert_eq!(
            got,
            "   10    10  b\n   11       -return total;\n         11 +return total + vouchers;\n   12    12  }\n"
        );
        assert!(!got.contains("far"));
        assert!(around(&f, Side::New, (100, 101), 2).is_none());
    }
}
