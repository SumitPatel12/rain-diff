//! What a diff changes once whitespace is ignored, from the hunks alone (the same comparison as
//! `git diff -w`): within each run of removed/added lines, removed and added lines are paired in
//! order by a hash of their non-whitespace text, so a reordered line is a real change.

use std::collections::HashSet;

use rain_core::{FileDiff, LineKind};

/// Past this many removed × added lines (of those found on both sides) in one run, the run is not
/// paired: every line in it is real.
pub const PAIR_CELLS: usize = 1 << 20;

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

/// Languages where leading whitespace is syntax (Python blocks, YAML nesting, Makefile tabs).
pub fn indent_sensitive(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rfind('.').map_or("", |at| &name[at + 1..]).to_ascii_lowercase();
    matches!(name, "Makefile" | "makefile" | "GNUmakefile" | "Snakefile")
        || matches!(
            ext.as_str(),
            "py" | "pyi"
                | "pyw"
                | "yaml"
                | "yml"
                | "mk"
                | "sass"
                | "pug"
                | "jade"
                | "haml"
                | "slim"
                | "coffee"
                | "nim"
                | "fs"
                | "fsi"
                | "fsx"
                | "elm"
                | "hs"
        )
}

/// A hash of the line's non-whitespace characters, and of its indentation when that is syntax.
fn squash(s: &str, keep_indent: bool) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    if keep_indent {
        s[..s.len() - s.trim_start().len()].hash(&mut h);
    }
    for c in s.chars().filter(|c| !c.is_whitespace()) {
        c.hash(&mut h);
    }
    h.finish()
}

/// The `(a, b)` index pairs of a longest common subsequence (in order), or `None` when the run is
/// too big to pair.
fn pair(a: &[u64], b: &[u64]) -> Option<Vec<(usize, usize)>> {
    let head = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let mut out: Vec<(usize, usize)> = (0..head).map(|i| (i, i)).collect();
    out.extend((0..tail).map(|i| (a.len() - 1 - i, b.len() - 1 - i)));
    let (a_mid, b_mid) = (&a[head..a.len() - tail], &b[head..b.len() - tail]);
    if a_mid.is_empty() || b_mid.is_empty() {
        return Some(out);
    }
    // Only lines found on both sides can pair; a plain rewrite then needs no table at all.
    let (in_a, in_b): (HashSet<u64>, HashSet<u64>) = (a_mid.iter().copied().collect(), b_mid.iter().copied().collect());
    let xi: Vec<usize> = (0..a_mid.len()).filter(|&i| in_b.contains(&a_mid[i])).collect();
    let yi: Vec<usize> = (0..b_mid.len()).filter(|&j| in_a.contains(&b_mid[j])).collect();
    if xi.is_empty() {
        return Some(out);
    }
    if xi.len().saturating_mul(yi.len()) > PAIR_CELLS {
        return None;
    }
    let x: Vec<u64> = xi.iter().map(|&i| a_mid[i]).collect();
    let y: Vec<u64> = yi.iter().map(|&j| b_mid[j]).collect();
    let w = y.len() + 1;
    let mut t = vec![0u32; (x.len() + 1) * w];
    for i in (0..x.len()).rev() {
        for j in (0..y.len()).rev() {
            t[i * w + j] = if x[i] == y[j] {
                t[(i + 1) * w + j + 1] + 1
            } else {
                t[(i + 1) * w + j].max(t[i * w + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < x.len() && j < y.len() {
        if x[i] == y[j] {
            out.push((head + xi[i], head + yi[j]));
            (i, j) = (i + 1, j + 1);
        } else if t[(i + 1) * w + j] >= t[i * w + j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    Some(out)
}

/// A removed or added line: its squashed hash, its line number, its index in the file's lines.
type Run = Vec<(u64, u32, u32)>;

pub fn analyse(f: &FileDiff) -> Ws {
    let keep_indent = indent_sensitive(&f.path);
    let mut out = Ws::default();
    for h in &f.hunks {
        let (mut lone_added, mut lone_removed) = (Vec::new(), Vec::new());
        let (mut removed, mut added): (Run, Run) = (Vec::new(), Vec::new());
        let mut flush = |removed: &mut Run, added: &mut Run, out: &mut Ws| {
            let ra: Vec<u64> = removed.iter().map(|r| r.0).collect();
            let aa: Vec<u64> = added.iter().map(|a| a.0).collect();
            let (mut mr, mut ma) = (vec![false; ra.len()], vec![false; aa.len()]);
            for (r, a) in pair(&ra, &aa).unwrap_or_default() {
                (mr[r], ma[a]) = (true, true);
                // An identical pair is a line moved within the run, not a whitespace edit.
                out.matched += u32::from(f.line_text(removed[r].2) != f.line_text(added[a].2));
            }
            let la: Vec<u32> = added.iter().zip(&ma).filter(|(_, m)| !**m).map(|(a, _)| a.1).collect();
            let lr: Vec<u32> = removed.iter().zip(&mr).filter(|(_, m)| !**m).map(|(r, _)| r.1).collect();
            out.real += la.len().max(lr.len()) as u32;
            lone_added.extend(la);
            lone_removed.extend(lr);
            removed.clear();
            added.clear();
        };
        for i in h.lines.clone() {
            let rec = &f.lines[i as usize];
            match rec.kind {
                LineKind::Added => {
                    out.changed += 1;
                    added.push((squash(f.line_text(i), keep_indent), rec.new, i));
                }
                LineKind::Removed => {
                    out.changed += 1;
                    removed.push((squash(f.line_text(i), keep_indent), rec.old, i));
                }
                LineKind::Context => flush(&mut removed, &mut added, &mut out),
            }
        }
        flush(&mut removed, &mut added, &mut out);
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

    #[test]
    fn a_reorder_is_a_real_change() {
        let mut b = FileDiffBuilder::new(0, "Pay.cs", FileStatus::Modified);
        b.push_hunk(1, 2, 1, 2, "");
        b.push_line(LineKind::Removed, 1, 0, "checkAuth();");
        b.push_line(LineKind::Removed, 2, 0, "doWork();");
        b.push_line(LineKind::Added, 0, 1, "doWork();");
        b.push_line(LineKind::Added, 0, 2, "checkAuth();");
        let w = analyse(&b.finish());
        assert_eq!((w.matched, w.real), (0, 1));

        let mut b = FileDiffBuilder::new(0, "Pay.cs", FileStatus::Modified);
        b.push_hunk(1, 2, 1, 2, "");
        b.push_line(LineKind::Removed, 1, 0, "checkAuth();");
        b.push_line(LineKind::Context, 2, 1, "doWork();");
        b.push_line(LineKind::Added, 0, 2, "  checkAuth();");
        assert_eq!(analyse(&b.finish()).real, 2, "a line moved past context");
    }

    #[test]
    fn indentation_is_real_where_it_is_syntax() {
        let hunk = |path: &str| {
            let mut b = FileDiffBuilder::new(0, path, FileStatus::Modified);
            b.push_hunk(1, 2, 1, 2, "");
            b.push_line(LineKind::Context, 1, 1, "if x:");
            b.push_line(LineKind::Removed, 2, 0, "    return x");
            b.push_line(LineKind::Added, 0, 2, "return x");
            analyse(&b.finish()).real
        };
        assert_eq!(hunk("app/a.py"), 1);
        assert_eq!(hunk("ci/build.yml"), 1);
        assert_eq!(hunk("Makefile"), 1);
        assert_eq!(hunk("src/a.ts"), 0);
    }

    #[test]
    fn a_run_too_big_to_pair_is_real() {
        let n = 1100u32;
        let mut b = FileDiffBuilder::new(0, "big.cs", FileStatus::Modified);
        b.push_hunk(1, n, 1, n, "");
        for i in 0..n {
            b.push_line(LineKind::Removed, i + 1, 0, &format!("x{i};"));
        }
        for i in 0..n {
            b.push_line(LineKind::Added, 0, i + 1, &format!("x{};", (i + 1) % n));
        }
        let t0 = std::time::Instant::now();
        let w = analyse(&b.finish());
        assert!(t0.elapsed().as_millis() < 500);
        assert_eq!((w.matched, w.real), (0, n));
    }
}
