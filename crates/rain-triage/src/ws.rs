//! What a diff changes once whitespace is ignored, from the hunks alone (the same comparison as
//! `git diff -w`, line by line within each hunk, by a hash of each line's non-whitespace text).

use std::collections::HashMap;

use rain_core::{FileDiff, LineKind};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ws {
    /// Added plus removed lines.
    pub changed: u32,
    /// Changed lines left once whitespace-only pairs are matched (a rewritten line counts once).
    pub real: u32,
    /// Removed/added pairs that differ only in whitespace.
    pub matched: u32,
    /// Where the first real change is: its new line, else its old line.
    pub real_line: Option<u32>,
}

/// A hash of the line's non-whitespace characters.
fn squash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for c in s.chars().filter(|c| !c.is_whitespace()) {
        c.hash(&mut h);
    }
    h.finish()
}

pub fn analyse(f: &FileDiff) -> Ws {
    let mut out = Ws::default();
    for h in &f.hunks {
        let mut removed: HashMap<u64, Vec<u32>> = HashMap::new();
        for i in h.lines.clone() {
            let rec = &f.lines[i as usize];
            if rec.kind == LineKind::Removed {
                removed.entry(squash(f.line_text(i))).or_default().push(rec.old);
            }
        }
        let mut lone_added: Vec<u32> = Vec::new();
        for i in h.lines.clone() {
            let rec = &f.lines[i as usize];
            match rec.kind {
                LineKind::Added => {
                    out.changed += 1;
                    match removed.get_mut(&squash(f.line_text(i))).filter(|v| !v.is_empty()) {
                        Some(v) => {
                            v.pop();
                            out.matched += 1;
                        }
                        None => lone_added.push(rec.new),
                    }
                }
                LineKind::Removed => out.changed += 1,
                LineKind::Context => {}
            }
        }
        let mut lone_removed: Vec<u32> = removed.into_values().flatten().collect();
        lone_removed.sort_unstable();
        out.real += lone_added.len().max(lone_removed.len()) as u32;
        if out.real_line.is_none() {
            out.real_line = lone_added.first().or(lone_removed.first()).copied();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use rain_core::{FileDiffBuilder, FileStatus};

    use super::*;

    #[test]
    fn whitespace_pairs_cancel_and_a_rewrite_counts_once() {
        let mut b = FileDiffBuilder::new(0, "a.cs", FileStatus::Modified);
        b.push_hunk(1, 4, 1, 4, "");
        b.push_line(LineKind::Removed, 1, 0, "    int x = 1;");
        b.push_line(LineKind::Removed, 2, 0, "  int y;");
        b.push_line(LineKind::Removed, 3, 0, "int z = 3;");
        b.push_line(LineKind::Added, 0, 1, "\tint x = 1;");
        b.push_line(LineKind::Added, 0, 2, "\tint  y;");
        b.push_line(LineKind::Added, 0, 3, "int z = 4;");
        b.push_line(LineKind::Context, 4, 4, "}");
        let w = analyse(&b.finish());
        assert_eq!(
            w,
            Ws {
                changed: 6,
                real: 1,
                matched: 2,
                real_line: Some(3)
            }
        );

        let mut b = FileDiffBuilder::new(0, "b.cs", FileStatus::Modified);
        b.push_hunk(1, 1, 1, 1, "");
        b.push_line(LineKind::Removed, 1, 0, "a  b");
        b.push_line(LineKind::Added, 0, 1, "a b");
        assert_eq!(analyse(&b.finish()).real, 0);
    }
}
