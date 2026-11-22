//! Blocks of removed lines that reappear as added lines elsewhere in the same repo's diff: a
//! function moved down its file, or out into another one. The idea is VS Code's moved-code
//! detection and git's `--color-moved`; the method here is Rain's own.
//!
//! Lines match on their text without the whitespace around it, so a block moved into a deeper
//! indent still matches. A block needs [`MIN_LINES`] lines holding a word and [`MIN_ALNUM`]
//! letters and digits; blank lines and lone brackets ride along but never start a match. A few
//! lines may change inside a block on the way. Longer blocks are claimed first, and each line
//! belongs to one move at most.
//!
//! This only tells the reader what moved: line numbers, anchors and saved sessions never see it.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;

use crate::diff::{FileDiff, LineKind};

/// Lines holding a word that a moved block has at least.
pub const MIN_LINES: u32 = 3;
/// Letters and digits a moved block has at least.
pub const MIN_ALNUM: u32 = 20;
/// No moves are looked for in a diff with more removed plus added lines than this.
pub const MAX_CHANGED_LINES: usize = 200_000;
/// A file with more diff lines than this is left out, as generated and binary files are.
const MAX_FILE_LINES: usize = 20_000;
/// A longer line matches nothing.
const MAX_LINE_BYTES: usize = 1000;
/// A line removed in more places than this starts no match.
const MAX_CANDIDATES: usize = 8;
/// The most lines in a row, on either side, that may change inside a moved block.
const MAX_EDITED: u32 = 2;
/// Lines holding a word that must match again after changed lines to keep them in the block.
const REJOIN_LINES: u32 = 2;
/// Line comparisons allowed per changed line before the search gives up.
const STEPS_PER_LINE: u64 = 32;

const NONE: u32 = u32::MAX;

/// A run of lines of one file: `start` indexes [`FileDiff::lines`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub file: u32,
    pub start: u32,
    pub len: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Move {
    pub from: Span,
    pub to: Span,
    /// The old number of the first removed line, and the new number of the first added one.
    pub from_line: u32,
    pub to_line: u32,
    pub from_path: Arc<str>,
    pub to_path: Arc<str>,
    /// Some lines inside changed on the way.
    pub edited: bool,
}

impl Move {
    /// What the first line of a side says about the other: `moved to src/tax.rs:120`.
    pub fn label(&self, removed: bool) -> String {
        let (word, path, line) = if removed {
            ("to", &self.to_path, self.to_line)
        } else {
            ("from", &self.from_path, self.from_line)
        };
        let place = if self.from.file == self.to.file {
            format!("line {line}")
        } else {
            format!("{path}:{line}")
        };
        let edited = if self.edited { ", edited" } else { "" };
        format!("moved {word} {place}{edited}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MovedLine {
    /// Its index in its file's lines.
    pub line: u32,
    /// Its move, an index into [`Moves::moves`].
    pub mv: u32,
    /// The line it is at the other end, in that file's lines; none for a line added or dropped
    /// on the way.
    pub partner: Option<u32>,
    /// The same text at both ends, but for the whitespace around it.
    pub exact: bool,
}

/// How much the search did: a test reads it to check the work stays in step with the diff.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// Lines hashed.
    pub lines: u64,
    /// Lines compared or walked after that.
    pub steps: u64,
    /// The diff was past a cap, so no moves were looked for.
    pub gave_up: bool,
}

/// Every move of a diff, and each file's moved lines.
#[derive(Clone, Debug, Default)]
pub struct Moves {
    pub moves: Vec<Move>,
    /// Per file, its moved lines in line order.
    lines: Vec<Vec<MovedLine>>,
    pub work: Work,
}

impl Moves {
    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
    }

    pub fn line(&self, file: usize, i: u32) -> Option<&MovedLine> {
        let lines = self.lines.get(file)?;
        lines.binary_search_by_key(&i, |l| l.line).ok().map(|k| &lines[k])
    }

    pub fn of(&self, l: &MovedLine) -> &Move {
        &self.moves[l.mv as usize]
    }

    /// Whether every changed line of `f`, file `file` of the diff, moved unchanged.
    pub fn only_moves(&self, file: usize, f: &FileDiff) -> bool {
        let changed = (f.additions + f.deletions) as usize;
        changed > 0
            && self
                .lines
                .get(file)
                .is_some_and(|l| l.len() == changed && l.iter().all(|l| l.exact))
    }

    /// The same moves for the files in another order: file `i` is now `at[i]`.
    pub fn reordered(mut self, at: &[u32]) -> Moves {
        let mut lines = vec![Vec::new(); self.lines.len()];
        for (i, l) in std::mem::take(&mut self.lines).into_iter().enumerate() {
            lines[at[i] as usize] = l;
        }
        self.lines = lines;
        for m in &mut self.moves {
            m.from.file = at[m.from.file as usize];
            m.to.file = at[m.to.file as usize];
        }
        self
    }

    pub fn heap_bytes(&self) -> usize {
        self.moves.capacity() * size_of::<Move>()
            + self.lines.capacity() * size_of::<Vec<MovedLine>>()
            + self.lines.iter().map(|l| l.capacity() * size_of::<MovedLine>()).sum::<usize>()
    }
}

/// The removed (or the added) lines of every file, numbered in file and line order.
#[derive(Default)]
struct Lines {
    /// Each line's hash; 0 for a line that matches nothing.
    hash: Vec<u64>,
    /// Letters and digits in each line, to 255.
    alnum: Vec<u8>,
    run: Vec<u32>,
    runs: Vec<Run>,
    taken: Vec<bool>,
    last: Option<(u32, u32, u32)>,
}

/// Lines that follow each other in a file: the first one's index there, and its number here.
struct Run {
    file: u32,
    first: u32,
    at: u32,
}

impl Lines {
    fn push(&mut self, file: u32, i: u32, number: u32, f: &FileDiff) {
        if i == 0 || number == 0 || self.last != Some((file, i - 1, number - 1)) {
            self.runs.push(Run {
                file,
                first: i,
                at: self.hash.len() as u32,
            });
        }
        self.last = Some((file, i, number));
        let text = f.line_text(i).trim();
        let hash = if text.len() > MAX_LINE_BYTES {
            0
        } else {
            let seed = 0xcbf2_9ce4_8422_2325u64 ^ u64::from(f.repo);
            text.bytes()
                .fold(seed, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
                .max(1)
        };
        self.hash.push(hash);
        self.alnum
            .push(text.chars().filter(|c| c.is_alphanumeric()).take(255).count() as u8);
        self.run.push(self.runs.len() as u32 - 1);
        self.taken.push(false);
    }

    fn len(&self) -> u32 {
        self.hash.len() as u32
    }

    /// The file of line `p` and its index there.
    fn place(&self, p: u32) -> (u32, u32) {
        let r = &self.runs[self.run[p as usize] as usize];
        (r.file, r.first + p - r.at)
    }
}

/// Whether `at` is a line of `run` (and, with `free`, in no move yet).
fn inside(l: &Lines, at: i64, run: u32, free: bool) -> bool {
    (0..i64::from(l.len())).contains(&at) && l.run[at as usize] == run && !(free && l.taken[at as usize])
}

/// Removed lines `p..p + len` beside added lines `q..q + len`.
#[derive(Clone, Copy)]
struct Stretch {
    p: u32,
    q: u32,
    len: u32,
}

struct Search<'a> {
    files: &'a [FileDiff],
    old: Lines,
    new: Lines,
    steps: u64,
    budget: u64,
}

impl Search<'_> {
    fn spent(&self) -> bool {
        self.steps > self.budget
    }

    fn same(&mut self, p: u32, q: u32) -> bool {
        self.steps += 1;
        let h = self.old.hash[p as usize];
        if h == 0 || h != self.new.hash[q as usize] {
            return false;
        }
        let ((fa, a), (fb, b)) = (self.old.place(p), self.new.place(q));
        let (fa, fb) = (&self.files[fa as usize], &self.files[fb as usize]);
        fa.repo == fb.repo && fa.line_text(a).trim() == fb.line_text(b).trim()
    }

    /// How many line pairs from `p` and `q` on (or back, by `step`) are the same within `runs`;
    /// with `free`, only lines no move has taken.
    fn stretch(&mut self, p: i64, q: i64, step: i64, runs: (u32, u32), free: bool) -> u32 {
        let (mut p, mut q, mut n) = (p, q, 0);
        while self.open(p, q, runs, free) && self.same(p as u32, q as u32) {
            (p, q, n) = (p + step, q + step, n + 1);
        }
        n
    }

    fn open(&self, p: i64, q: i64, runs: (u32, u32), free: bool) -> bool {
        inside(&self.old, p, runs.0, free) && inside(&self.new, q, runs.1, free)
    }

    /// Lines holding a word in removed lines `p..p + len`, and their letters and digits.
    fn weight(&mut self, p: u32, len: u32) -> (u32, u32) {
        self.steps += u64::from(len);
        let lines = &self.old.alnum[p as usize..(p + len) as usize];
        (
            lines.iter().filter(|&&n| n > 0).count() as u32,
            lines.iter().map(|&n| u32::from(n)).sum(),
        )
    }

    fn big_enough(&mut self, s: Stretch) -> bool {
        let (lines, alnum) = self.weight(s.p, s.len);
        lines >= MIN_LINES && alnum >= MIN_ALNUM
    }

    /// Every longest stretch of removed lines that is the same as a stretch of added lines and
    /// big enough to be a move; `None` when the budget ran out.
    fn stretches(&mut self) -> Option<Vec<Stretch>> {
        let mut first: HashMap<u64, u32> = HashMap::new();
        let mut next = vec![NONE; self.old.hash.len()];
        for p in (0..self.old.len()).rev() {
            let h = self.old.hash[p as usize];
            if h != 0 && self.old.alnum[p as usize] > 0 {
                next[p as usize] = first.insert(h, p).unwrap_or(NONE);
            }
        }
        // Per diagonal, where the last stretch found on it ends: its later lines find it again.
        let mut ends: HashMap<u64, u32> = HashMap::new();
        let mut out = Vec::new();
        for q in 0..self.new.len() {
            if self.new.alnum[q as usize] == 0 {
                continue;
            }
            let Some(&head) = first.get(&self.new.hash[q as usize]) else {
                continue;
            };
            let mut from = [NONE; MAX_CANDIDATES];
            let (mut n, mut p) = (0, head);
            while p != NONE && n <= MAX_CANDIDATES {
                if n < MAX_CANDIDATES {
                    from[n] = p;
                }
                (n, p) = (n + 1, next[p as usize]);
            }
            self.steps += n as u64;
            if n > MAX_CANDIDATES {
                continue;
            }
            for &p in &from[..n] {
                let diagonal = u64::from(p) + u64::from(self.new.len()) - u64::from(q);
                if ends.get(&diagonal).is_some_and(|&end| q < end) || !self.same(p, q) {
                    continue;
                }
                let runs = (self.old.run[p as usize], self.new.run[q as usize]);
                let before = self.stretch(i64::from(p) - 1, i64::from(q) - 1, -1, runs, false);
                let after = self.stretch(i64::from(p) + 1, i64::from(q) + 1, 1, runs, false);
                let s = Stretch {
                    p: p - before,
                    q: q - before,
                    len: before + 1 + after,
                };
                ends.insert(diagonal, s.q + s.len);
                if self.big_enough(s) {
                    out.push(s);
                }
                if self.spent() {
                    return None;
                }
            }
        }
        Some(out)
    }

    /// Changed lines right after (or, with `back`, right before) a move that are followed by
    /// more of the same block: how many on each side, and how many lines then match again.
    fn rejoin(&mut self, p: i64, q: i64, back: bool, runs: (u32, u32)) -> Option<(u32, u32, u32)> {
        let step = if back { -1 } else { 1 };
        for total in 1..=2 * MAX_EDITED {
            for gp in 0..=total.min(MAX_EDITED) {
                let gq = total - gp;
                if gq > MAX_EDITED {
                    continue;
                }
                let gap_free = (0..gp).all(|k| inside(&self.old, p + step * i64::from(k), runs.0, true))
                    && (0..gq).all(|k| inside(&self.new, q + step * i64::from(k), runs.1, true));
                if !gap_free {
                    continue;
                }
                let (sp, sq) = (p + step * i64::from(gp), q + step * i64::from(gq));
                let n = self.stretch(sp, sq, step, runs, true);
                if n == 0 {
                    continue;
                }
                let first = if back { sp - i64::from(n) + 1 } else { sp };
                if self.weight(first as u32, n).0 >= REJOIN_LINES {
                    return Some((gp, gq, n));
                }
            }
        }
        None
    }

    /// Takes stretch `s` as a move, with the changed lines around it that more of the block
    /// follows; its parts as `(p, q, removed lines, added lines, same)`, top to bottom.
    fn claim(&mut self, s: Stretch) -> Vec<(u32, u32, u32, u32, bool)> {
        let runs = (self.old.run[s.p as usize], self.new.run[s.q as usize]);
        let mut parts = vec![(s.p, s.q, s.len, s.len, true)];
        self.take(s.p, s.len, s.q, s.len);
        let (mut p, mut q) = (s.p + s.len, s.q + s.len);
        while let Some((gp, gq, n)) = self.rejoin(i64::from(p), i64::from(q), false, runs) {
            parts.push((p, q, gp, gq, false));
            parts.push((p + gp, q + gq, n, n, true));
            self.take(p, gp + n, q, gq + n);
            (p, q) = (p + gp + n, q + gq + n);
        }
        let (mut p, mut q) = (s.p, s.q);
        while let Some((gp, gq, n)) = self.rejoin(i64::from(p) - 1, i64::from(q) - 1, true, runs) {
            parts.insert(0, (p - gp, q - gq, gp, gq, false));
            parts.insert(0, (p - gp - n, q - gq - n, n, n, true));
            self.take(p - gp - n, gp + n, q - gq - n, gq + n);
            (p, q) = (p - gp - n, q - gq - n);
        }
        parts
    }

    fn take(&mut self, p: u32, removed: u32, q: u32, added: u32) {
        self.old.taken[p as usize..(p + removed) as usize].fill(true);
        self.new.taken[q as usize..(q + added) as usize].fill(true);
    }
}

fn searched(f: &FileDiff) -> bool {
    !f.binary && !f.generated && f.lines.len() <= MAX_FILE_LINES
}

/// The moves of `files`, one diff's files. The same files give the same moves.
pub fn detect(files: &[FileDiff]) -> Moves {
    let mut out = Moves {
        lines: vec![Vec::new(); files.len()],
        ..Moves::default()
    };
    let count = |of: fn(&FileDiff) -> u32| -> usize { files.iter().filter(|f| searched(f)).map(|f| of(f) as usize).sum() };
    let (removed, added) = (count(|f| f.deletions), count(|f| f.additions));
    if removed == 0 || added == 0 {
        return out;
    }
    if removed + added > MAX_CHANGED_LINES {
        out.work.gave_up = true;
        return out;
    }
    let mut s = Search {
        files,
        old: Lines::default(),
        new: Lines::default(),
        steps: 0,
        budget: STEPS_PER_LINE * (removed + added) as u64 + 1024,
    };
    for (fi, f) in files.iter().enumerate().filter(|(_, f)| searched(f)) {
        for (i, l) in f.lines.iter().enumerate() {
            match l.kind {
                LineKind::Removed => s.old.push(fi as u32, i as u32, l.old, f),
                LineKind::Added => s.new.push(fi as u32, i as u32, l.new, f),
                LineKind::Context => {}
            }
        }
    }
    out.work.lines = u64::from(s.old.len() + s.new.len());
    let found = s.stretches();
    out.work.steps = s.steps;
    let Some(found) = found else {
        out.work.gave_up = true;
        return out;
    };
    // Longest first; of equals, one that stays in its file, then the earliest.
    let key = |s: &Search, x: Stretch| (x.len, s.old.place(x.p).0 == s.new.place(x.q).0, Reverse(x.p), Reverse(x.q));
    let mut heap: BinaryHeap<_> = found.into_iter().map(|x| key(&s, x)).collect();
    let mut paths: Vec<Option<Arc<str>>> = vec![None; files.len()];
    let mut path = |file: u32| -> Arc<str> {
        paths[file as usize]
            .get_or_insert_with(|| Arc::from(files[file as usize].path.as_str()))
            .clone()
    };
    while let Some((len, _, Reverse(p), Reverse(q))) = heap.pop() {
        s.steps += u64::from(len);
        if s.spent() {
            out.moves.clear();
            out.lines.iter_mut().for_each(Vec::clear);
            out.work.gave_up = true;
            break;
        }
        // What other moves left of it, when they took some of its lines first.
        let free = |s: &Search, k: u32| !s.old.taken[(p + k) as usize] && !s.new.taken[(q + k) as usize];
        if !(0..len).all(|k| free(&s, k)) {
            let mut k = 0;
            while k < len {
                let from = k;
                while k < len && free(&s, k) {
                    k += 1;
                }
                let rest = Stretch {
                    p: p + from,
                    q: q + from,
                    len: k - from,
                };
                if rest.len > 0 && s.big_enough(rest) {
                    heap.push(key(&s, rest));
                }
                k += 1;
            }
            continue;
        }
        let parts = s.claim(Stretch { p, q, len });
        let (first, last) = (parts[0], parts[parts.len() - 1]);
        let ((from_file, from), (to_file, to)) = (s.old.place(first.0), s.new.place(first.1));
        let mv = out.moves.len() as u32;
        out.moves.push(Move {
            from: Span {
                file: from_file,
                start: from,
                len: last.0 + last.2 - first.0,
            },
            to: Span {
                file: to_file,
                start: to,
                len: last.1 + last.3 - first.1,
            },
            from_line: files[from_file as usize].lines[from as usize].old,
            to_line: files[to_file as usize].lines[to as usize].new,
            from_path: path(from_file),
            to_path: path(to_file),
            edited: parts.len() > 1,
        });
        for (p, q, removed, added, exact) in parts {
            let (a, b) = (s.old.place(p).1, s.new.place(q).1);
            let both = removed.min(added);
            for k in 0..removed {
                out.lines[from_file as usize].push(MovedLine {
                    line: a + k,
                    mv,
                    partner: (k < both).then_some(b + k),
                    exact,
                });
            }
            for k in 0..added {
                out.lines[to_file as usize].push(MovedLine {
                    line: b + k,
                    mv,
                    partner: (k < both).then_some(a + k),
                    exact,
                });
            }
        }
    }
    out.work.steps = s.steps;
    for l in &mut out.lines {
        l.sort_unstable_by_key(|l| l.line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{FileDiffBuilder, FileStatus};

    /// A file of one hunk from `-`, `+` and unchanged (` `) lines.
    fn file(repo: u16, path: &str, lines: &[(char, &str)]) -> FileDiff {
        let mut b = FileDiffBuilder::new(repo, path, FileStatus::Modified);
        b.push_hunk(1, 0, 1, 0, "");
        let (mut old, mut new) = (0, 0);
        for &(sign, text) in lines {
            match sign {
                '-' => {
                    old += 1;
                    b.push_line(LineKind::Removed, old, 0, text);
                }
                '+' => {
                    new += 1;
                    b.push_line(LineKind::Added, 0, new, text);
                }
                _ => {
                    (old, new) = (old + 1, new + 1);
                    b.push_line(LineKind::Context, old, new, text);
                }
            }
        }
        b.finish()
    }

    fn signed<'a>(sign: char, lines: &[&'a str]) -> Vec<(char, &'a str)> {
        lines.iter().map(|l| (sign, *l)).collect()
    }

    const TAX: [&str; 5] = [
        "fn tax_for(order: &Order, region: &Region) -> Money {",
        "    let rate = region.tax_rate(order.placed_on);",
        "",
        "    order.subtotal() * rate",
        "}",
    ];

    type At = (u32, u32, u32);

    fn spans(m: &Moves) -> Vec<(At, At)> {
        let at = |s: Span| (s.file, s.start, s.len);
        m.moves.iter().map(|m| (at(m.from), at(m.to))).collect()
    }

    #[test]
    fn a_block_moved_down_its_file_is_one_move() {
        let mut lines = signed('-', &TAX);
        lines.extend([(' ', "fn total(order: &Order) -> Money {"), (' ', "}")]);
        lines.extend(signed('+', &TAX));
        let f = file(0, "src/orders/total.rs", &lines);
        let m = detect(std::slice::from_ref(&f));
        assert_eq!(spans(&m), [((0, 0, 5), (0, 7, 5))]);
        let mv = &m.moves[0];
        assert_eq!((mv.from_line, mv.to_line, mv.edited), (1, 3, false));
        assert_eq!(mv.label(true), "moved to line 3");
        assert_eq!(mv.label(false), "moved from line 1");
        assert_eq!(
            m.line(0, 2).map(|l| (l.partner, l.exact)),
            Some((Some(9), true)),
            "the blank line rides along"
        );
        assert_eq!(m.line(0, 9).map(|l| l.partner), Some(Some(2)));
        assert!(m.line(0, 5).is_none(), "an unchanged line is in no move");
        assert!(m.only_moves(0, &f));
    }

    #[test]
    fn a_block_moved_to_another_file_and_a_deeper_indent_still_matches() {
        let indented: Vec<String> = TAX.iter().map(|l| format!("\t    {l}  ")).collect();
        let mut added = vec![('+', "impl Billing {")];
        added.extend(indented.iter().map(|l| ('+', l.as_str())));
        added.push(('+', "}"));
        let files = [
            file(0, "src/orders/total.rs", &signed('-', &TAX)),
            file(0, "src/billing/tax.rs", &added),
            file(1, "src/billing/tax.rs", &signed('+', &TAX)),
        ];
        let m = detect(&files);
        assert_eq!(spans(&m), [((0, 0, 5), (1, 1, 5))], "never into another repo");
        assert_eq!(m.moves[0].label(true), "moved to src/billing/tax.rs:2");
        assert_eq!(m.moves[0].label(false), "moved from src/orders/total.rs:1");
        assert!(m.only_moves(0, &files[0]));
        assert!(!m.only_moves(1, &files[1]), "the lines around the block are new");
        assert!(!m.only_moves(2, &files[2]));
    }

    #[test]
    fn boilerplate_and_blocks_under_the_minimum_are_not_moves() {
        let none = |removed: &[&str], added: &[&str]| {
            let files = [file(0, "a.rs", &signed('-', removed)), file(0, "b.rs", &signed('+', added))];
            detect(&files).is_empty()
        };
        let ends = ["        }", "    }", "", "}", "});", "", "{"];
        assert!(none(&ends, &ends), "blank lines and brackets hold no word");
        let two = [
            "let subtotal = cart.items.iter().map(Item::price).sum();",
            "let tax = subtotal * region.tax_rate;",
        ];
        assert!(none(&two, &two), "two lines are under three");
        let short = ["a = 1;", "}", "b = 2;", "c = 3;"];
        assert!(none(&short, &short), "three lines of 6 letters and digits are under 20");
        let guard = ["if order.is_empty() {", "    return Err(Empty);", "}"];
        assert!(none(&guard, &guard), "two lines with a word and a bracket");
        let three = ["if order.is_empty() {", "    return Err(Empty);", "} else if order.is_held() {"];
        assert!(!none(&three, &three), "three lines with words and 20 letters are a move");
    }

    #[test]
    fn a_line_removed_in_many_places_starts_no_match() {
        let block = ["use std::collections::HashMap;", "use std::sync::Arc;", "use std::path::PathBuf;"];
        let mut files: Vec<FileDiff> = (0..=MAX_CANDIDATES)
            .map(|i| file(0, &format!("src/m{i}.rs"), &signed('-', &block)))
            .collect();
        files.push(file(0, "src/prelude.rs", &signed('+', &block)));
        let m = detect(&files);
        assert!(m.is_empty() && !m.work.gave_up);
        files.remove(0);
        assert_eq!(detect(&files).moves.len(), 1, "removed in {MAX_CANDIDATES} places, it can");
    }

    #[test]
    fn of_two_removed_copies_the_one_in_the_added_blocks_file_is_the_move() {
        let mut here = signed('-', &TAX);
        here.push((' ', "// billing"));
        here.extend(signed('+', &TAX));
        let files = [
            file(0, "src/a.rs", &signed('-', &TAX)),
            file(0, "src/b.rs", &here),
            file(0, "src/c.rs", &signed('-', &TAX)),
        ];
        let m = detect(&files);
        assert_eq!(spans(&m), [((1, 0, 5), (1, 6, 5))]);
        assert!(
            m.line(0, 0).is_none() && m.line(2, 0).is_none(),
            "the other copies are plain removals"
        );
        assert!((0..10).all(|_| spans(&detect(&files)) == spans(&m)), "the same every time");

        let files = [
            file(0, "src/a.rs", &signed('-', &TAX)),
            file(0, "src/b.rs", &signed('+', &TAX)),
            file(0, "src/c.rs", &signed('-', &TAX)),
        ];
        assert_eq!(spans(&detect(&files)), [((0, 0, 5), (1, 0, 5))], "else the earliest copy");
    }

    const STEPS: [&str; 8] = [
        "let cart = carts.load(customer.id)?;",
        "let subtotal = cart.items.iter().map(Item::price).sum();",
        "let discount = coupons.best_for(&cart);",
        "let tax = (subtotal - discount) * region.tax_rate;",
        "let shipping = carriers.quote(&cart, &address)?;",
        "let total = subtotal - discount + tax + shipping;",
        "ledger.record(customer.id, total)?;",
        "receipts.send(customer.email, total);",
    ];

    #[test]
    fn a_line_changed_on_the_way_stays_inside_its_move() {
        let mut added = STEPS;
        added[3] = "let tax = (subtotal - discount) * region.tax_rate_on(today);";
        let files = [file(0, "src/a.rs", &signed('-', &STEPS)), file(0, "src/b.rs", &signed('+', &added))];
        let m = detect(&files);
        assert_eq!(spans(&m), [((0, 0, 8), (1, 0, 8))]);
        assert!(m.moves[0].edited);
        assert_eq!(m.moves[0].label(true), "moved to src/b.rs:1, edited");
        let line = |file: usize, i: u32| m.line(file, i).map(|l| (l.partner, l.exact));
        assert_eq!(line(0, 3), Some((Some(3), false)), "the changed line knows its other end");
        assert_eq!(line(1, 3), Some((Some(3), false)));
        assert_eq!(line(0, 2), Some((Some(2), true)));
        assert_eq!(line(1, 7), Some((Some(7), true)));
        assert!(!m.only_moves(0, &files[0]), "a file with an edited move is not only moves");
    }

    #[test]
    fn a_line_added_or_dropped_on_the_way_stays_inside_its_move() {
        let mut added = STEPS.to_vec();
        added.insert(4, "audit.note(&cart);");
        let files = [file(0, "src/a.rs", &signed('-', &STEPS)), file(0, "src/b.rs", &signed('+', &added))];
        let m = detect(&files);
        assert_eq!(spans(&m), [((0, 0, 8), (1, 0, 9))]);
        assert_eq!(m.line(1, 4).map(|l| (l.partner, l.exact)), Some((None, false)));
        assert_eq!(m.line(1, 5).map(|l| (l.partner, l.exact)), Some((Some(4), true)));

        let files = [file(0, "src/a.rs", &signed('-', &added)), file(0, "src/b.rs", &signed('+', &STEPS))];
        let m = detect(&files);
        assert_eq!(spans(&m), [((0, 0, 9), (1, 0, 8))]);
        assert_eq!(m.line(0, 4).map(|l| (l.partner, l.exact)), Some((None, false)));
    }

    #[test]
    fn three_changed_lines_in_a_row_end_the_move() {
        let mut added = STEPS;
        for l in &mut added[3..6] {
            *l = "metrics.count(\"checkout\");";
        }
        let files = [file(0, "src/a.rs", &signed('-', &STEPS)), file(0, "src/b.rs", &signed('+', &added))];
        let m = detect(&files);
        assert_eq!(
            spans(&m),
            [((0, 0, 3), (1, 0, 3))],
            "the two lines after them are under the minimum"
        );
        assert!(!m.moves[0].edited);
    }

    #[test]
    fn moves_follow_their_files_into_a_new_order() {
        let files = [
            file(0, "src/a.rs", &signed('-', &TAX)),
            file(0, "src/b.rs", &signed('+', &TAX)),
            file(0, "src/c.rs", &signed('+', &["// nothing"])),
        ];
        let m = detect(&files).reordered(&[2, 0, 1]);
        assert_eq!(spans(&m), [((2, 0, 5), (0, 0, 5))]);
        assert!(m.line(2, 0).is_some() && m.line(0, 4).is_some() && m.line(1, 0).is_none());
    }

    /// `n` lines no two of which are alike.
    fn unique(n: usize, tag: &str) -> Vec<String> {
        (0..n).map(|i| format!("    let {tag}_{i} = compute({i}, &ledger);")).collect()
    }

    fn one_sided(path: &str, sign: char, lines: &[String]) -> FileDiff {
        let lines: Vec<(char, &str)> = lines.iter().map(|l| (sign, l.as_str())).collect();
        file(0, path, &lines)
    }

    /// `n` removed lines in files of 10,000, and the same lines added in blocks of 50 in the
    /// opposite order.
    fn shuffled(n: usize) -> Vec<FileDiff> {
        let lines = unique(n, "moved");
        let mut back: Vec<String> = Vec::with_capacity(n);
        for block in lines.chunks(50).rev() {
            back.extend(block.iter().cloned());
        }
        let mut files = Vec::new();
        for (i, (old, new)) in lines.chunks(10_000).zip(back.chunks(10_000)).enumerate() {
            files.push(one_sided(&format!("src/old{i}.rs"), '-', old));
            files.push(one_sided(&format!("src/new{i}.rs"), '+', new));
        }
        files
    }

    #[test]
    fn the_work_grows_with_the_diff_and_a_diff_without_moves_does_almost_none() {
        let n = MAX_CHANGED_LINES;
        let (a, b) = (detect(&shuffled(n / 4)), detect(&shuffled(n / 2)));
        assert_eq!((a.moves.len(), b.moves.len()), (n / 4 / 50, n / 2 / 50));
        assert_eq!((a.work.lines, b.work.lines), (n as u64 / 2, n as u64));
        assert!(b.work.steps <= 4 * n as u64, "{} steps for {n} lines", b.work.steps);
        assert!(
            b.work.steps <= a.work.steps * 21 / 10,
            "twice the lines, about twice the work: {:?} {:?}",
            a.work,
            b.work
        );
        assert!(!b.work.gave_up);

        let apart = [
            one_sided("src/old.rs", '-', &unique(10_000, "old")),
            one_sided("src/new.rs", '+', &unique(10_000, "new")),
        ];
        let none = detect(&apart);
        assert_eq!(
            (none.work.lines, none.work.steps, none.moves.len()),
            (20_000, 0, 0),
            "no line in common: hashed, then nothing"
        );

        let added = [one_sided("src/new.rs", '+', &unique(10_000, "new"))];
        assert_eq!(detect(&added).work, Work::default(), "nothing removed: not even hashed");
    }

    #[test]
    fn a_diff_past_the_cap_is_not_searched() {
        let mut files = shuffled(MAX_CHANGED_LINES / 2);
        files.push(one_sided("src/one_more.rs", '+', &unique(1, "extra")));
        let m = detect(&files);
        assert_eq!((m.moves.len(), m.work.lines, m.work.steps, m.work.gave_up), (0, 0, 0, true));
        assert!(m.line(0, 0).is_none());

        files.pop();
        files[0].generated = true;
        let m = detect(&files);
        assert!(!m.work.gave_up && m.line(0, 0).is_none(), "a generated file is left out");
        assert_eq!(m.moves.len(), (MAX_CHANGED_LINES / 2 - 10_000) / 50);
    }
}
