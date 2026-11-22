//! Which removed line sits beside which added line. A block's lines pair in order by default;
//! where a line was inserted or deleted mid-block, the lines that kept most of their text pair
//! with each other instead, so the lines after it stay beside their own edits. The idea is
//! delta's (pairing by likeness); the method here is Rain's own.
//!
//! Pairs never cross, so both sides still read top to bottom. This only lays rows out: line
//! numbers, anchors and saved sessions never see it.

use rain_core::diff::FileDiff;
use rain_core::formatting::formatting_only;

use crate::model::{self, Block};

/// A block is paired by likeness only up to this many removed × added lines (128 × 128, the
/// most pairs word changes are worked out for). The comparison is quadratic and runs where the
/// file is indexed, so a bigger block keeps its lines in order at no cost.
pub(crate) const LIKENESS_CELLS: usize = 128 * 128;

/// Lines longer than this are never compared (the cap word changes have).
const LINE_MAX_BYTES: usize = 1000;

/// Two lines are alike when at least half their text, by weight, is the same words and marks.
const ALIKE: u32 = 500;

thread_local! {
    static CELLS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Line pairs compared for likeness on this thread so far.
#[cfg(test)]
pub(crate) fn likeness_cells() -> u64 {
    CELLS.with(|n| n.get())
}

/// A file's change blocks, and the pairs of those not paired in order.
#[derive(Debug, Default)]
pub(crate) struct Pairing {
    pub(crate) blocks: Vec<Block>,
    /// `(removed, added)` line indexes of every block paired by text, ascending on both sides.
    matched: Vec<(u32, u32)>,
}

impl Pairing {
    pub(crate) fn new(f: &FileDiff) -> Pairing {
        let mut blocks = model::blocks(f);
        let mut matched = Vec::new();
        let mut scratch = Scratch::default();
        for b in &mut blocks {
            b.quiet = formatting_only(f, b.d0..b.a0, b.a0..b.e);
            if b.quiet {
                continue;
            }
            let from = matched.len();
            scratch.pair(f, b, &mut matched);
            b.by_text = matched.len() > from;
        }
        Pairing { blocks, matched }
    }

    pub(crate) fn block_of(&self, i: u32) -> Option<&Block> {
        model::block_of(&self.blocks, i)
    }

    /// The line paired with changed line `i`, if any.
    pub(crate) fn partner(&self, i: u32) -> Option<u32> {
        let b = self.block_of(i)?;
        if !b.by_text {
            return b.partner(i);
        }
        if i < b.a0 {
            let k = self.matched.partition_point(|p| p.0 < i);
            self.matched.get(k).filter(|p| p.0 == i).map(|p| p.1)
        } else {
            let k = self.matched.partition_point(|p| p.1 < i);
            self.matched.get(k).filter(|p| p.1 == i).map(|p| p.0)
        }
    }

    /// The pairs of `b`, a block paired by text, top to bottom.
    pub(crate) fn pairs_of(&self, b: &Block) -> &[(u32, u32)] {
        let from = self.matched.partition_point(|p| p.0 < b.d0);
        let to = self.matched.partition_point(|p| p.0 < b.a0);
        &self.matched[from..to]
    }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.blocks.capacity() * size_of::<Block>() + self.matched.capacity() * size_of::<(u32, u32)>()
    }
}

/// A line as a sorted bag of its words and marks, each hashed, with its weight.
#[derive(Default)]
struct Bag {
    tokens: Vec<(u64, u32)>,
    weight: u32,
}

impl Bag {
    fn fill(&mut self, line: &str) {
        self.tokens.clear();
        self.weight = 0;
        if line.len() > LINE_MAX_BYTES {
            return;
        }
        let hash = |s: &str| {
            s.bytes()
                .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
        };
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut rest = line;
        while let Some(c) = rest.chars().next() {
            let len = if word(c) {
                rest.find(|c: char| !word(c)).unwrap_or(rest.len())
            } else {
                c.len_utf8()
            };
            // A word weighs its length and a mark one, so shared brackets alone pair nothing.
            if !c.is_whitespace() {
                let w = if word(c) { len as u32 } else { 1 };
                self.tokens.push((hash(&rest[..len]), w));
                self.weight += w;
            }
            rest = &rest[len..];
        }
        self.tokens.sort_unstable();
    }

    /// How alike two lines are, in thousandths: twice the weight they share over their total.
    fn likeness(&self, other: &Bag) -> u32 {
        let total = self.weight + other.weight;
        if total == 0 {
            return 0;
        }
        let (a, b) = (&self.tokens, &other.tokens);
        let (mut i, mut j, mut shared) = (0, 0, 0u32);
        while i < a.len() && j < b.len() {
            match a[i].0.cmp(&b[j].0) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
                std::cmp::Ordering::Equal => {
                    shared += a[i].1.min(b[j].1);
                    (i, j) = (i + 1, j + 1);
                }
            }
        }
        2000 * shared / total
    }
}

#[derive(Default)]
struct Scratch {
    removed: Vec<Bag>,
    added: Vec<Bag>,
    like: Vec<u16>,
    best: Vec<u32>,
}

impl Scratch {
    /// Appends `b`'s pairs to `out` when some of its lines pair better by text than in order;
    /// nothing when in order is right, or the block is too big to compare.
    fn pair(&mut self, f: &FileDiff, b: &Block, out: &mut Vec<(u32, u32)>) {
        let (nd, na) = ((b.a0 - b.d0) as usize, (b.e - b.a0) as usize);
        if nd == 0 || na == 0 || nd * na == 1 || nd * na > LIKENESS_CELLS {
            return;
        }
        CELLS.with(|n| n.set(n.get() + (nd * na) as u64));
        let fill = |bags: &mut Vec<Bag>, from: u32, n: usize| {
            bags.resize_with(bags.len().max(n), Bag::default);
            for (k, bag) in bags[..n].iter_mut().enumerate() {
                bag.fill(f.line_text(from + k as u32));
            }
        };
        fill(&mut self.removed, b.d0, nd);
        fill(&mut self.added, b.a0, na);
        self.like.clear();
        for r in &self.removed[..nd] {
            self.like.extend(self.added[..na].iter().map(|a| r.likeness(a) as u16));
        }
        // The most total likeness over pairs that are alike and never cross.
        let w = na + 1;
        self.best.clear();
        self.best.resize((nd + 1) * w, 0);
        let alike = |like: &[u16], i: usize, j: usize| Some(u32::from(like[i * na + j])).filter(|&l| l >= ALIKE);
        for i in (0..nd).rev() {
            for j in (0..na).rev() {
                let skip = self.best[(i + 1) * w + j].max(self.best[i * w + j + 1]);
                let take = alike(&self.like, i, j).map_or(0, |l| l + self.best[(i + 1) * w + j + 1]);
                self.best[i * w + j] = skip.max(take);
            }
        }
        if self.best[0] == 0 {
            return;
        }
        let from = out.len();
        // Lines between two alike pairs sit side by side in order, as a whole block does.
        let in_order = |out: &mut Vec<(u32, u32)>, d: (usize, usize), a: (usize, usize)| {
            out.extend((0..(d.1 - d.0).min(a.1 - a.0)).map(|k| (b.d0 + (d.0 + k) as u32, b.a0 + (a.0 + k) as u32)));
        };
        let (mut i, mut j, mut d, mut a) = (0, 0, 0, 0);
        while i < nd && j < na {
            let here = self.best[i * w + j];
            if alike(&self.like, i, j).is_some_and(|l| l + self.best[(i + 1) * w + j + 1] == here) {
                in_order(out, (d, i), (a, j));
                out.push((b.d0 + i as u32, b.a0 + j as u32));
                (i, j) = (i + 1, j + 1);
                (d, a) = (i, j);
            } else if self.best[(i + 1) * w + j] >= self.best[i * w + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
        in_order(out, (d, nd), (a, na));
        let same = out.len() - from == nd.min(na) && out[from..].iter().all(|p| p.0 - b.d0 == p.1 - b.a0);
        if same {
            out.truncate(from);
        }
    }
}

#[cfg(test)]
mod tests {
    use rain_core::diff::{FileDiffBuilder, FileStatus, LineKind};

    use super::*;

    fn file(removed: &[&str], added: &[&str]) -> FileDiff {
        let mut b = FileDiffBuilder::new(0, "src/cart.rs", FileStatus::Modified);
        b.push_hunk(1, removed.len() as u32, 1, added.len() as u32, "");
        for (k, t) in removed.iter().enumerate() {
            b.push_line(LineKind::Removed, k as u32 + 1, 0, t);
        }
        for (k, t) in added.iter().enumerate() {
            b.push_line(LineKind::Added, 0, k as u32 + 1, t);
        }
        b.finish()
    }

    /// Each removed line's partner as an index into `added`, and the same from the added side.
    fn partners(removed: &[&str], added: &[&str]) -> (Vec<Option<u32>>, Vec<Option<u32>>) {
        let p = Pairing::new(&file(removed, added));
        let (nd, na) = (removed.len() as u32, added.len() as u32);
        let old: Vec<Option<u32>> = (0..nd).map(|i| p.partner(i).map(|a| a - nd)).collect();
        let new: Vec<Option<u32>> = (nd..nd + na).map(|i| p.partner(i)).collect();
        for (i, a) in old.iter().enumerate() {
            if let Some(a) = a {
                assert_eq!(new[*a as usize], Some(i as u32), "partners agree from both sides");
            }
        }
        let paired: Vec<u32> = old.iter().flatten().copied().collect();
        assert!(paired.windows(2).all(|w| w[0] < w[1]), "pairs never cross: {old:?}");
        (old, new)
    }

    const OLD: [&str; 3] = [
        "let subtotal = cart.items.iter().map(Item::price).sum();",
        "let tax = subtotal * region.tax_rate;",
        "let total = subtotal + tax;",
    ];

    #[test]
    fn a_line_inserted_mid_block_leaves_the_lines_after_it_beside_their_own() {
        let new = [
            "let subtotal = cart.items.iter().map(Item::net_price).sum();",
            "let discount = coupons.best_for(&cart);",
            "let tax = (subtotal - discount) * region.tax_rate;",
            "let total = subtotal - discount + tax;",
        ];
        let (old, added) = partners(&OLD, &new);
        assert_eq!(old, [Some(0), Some(2), Some(3)]);
        assert_eq!(added[1], None, "the inserted line is a pure add");
    }

    #[test]
    fn a_line_deleted_mid_block_leaves_the_lines_after_it_beside_their_own() {
        let new = [
            "let subtotal = cart.items.iter().map(Item::net_price).sum();",
            "let total = subtotal + shipping;",
        ];
        let (old, _) = partners(&OLD, &new);
        assert_eq!(old, [Some(0), None, Some(1)], "the deleted line is a pure remove");
    }

    #[test]
    fn a_block_of_unrelated_lines_stays_paired_in_order() {
        let new = ["if cart.is_empty() {", "    return Err(Empty);", "}", "audit.record(&cart);"];
        let f = file(&OLD, &new);
        let p = Pairing::new(&f);
        assert!(!p.blocks[0].by_text && p.matched.is_empty());
        assert_eq!((0..3).map(|i| p.partner(i)).collect::<Vec<_>>(), [Some(3), Some(4), Some(5)]);
        assert_eq!(p.partner(6), None);
    }

    #[test]
    fn a_rewritten_line_between_two_kept_ones_stays_beside_what_replaced_it() {
        let new = [
            "let subtotal = cart.items.iter().map(Item::net_price).sum();",
            "audit.record(&cart);",
            "metrics.count(\"checkout\");",
            "let total = subtotal + tax + shipping;",
        ];
        let (old, added) = partners(&OLD, &new);
        assert_eq!(old, [Some(0), Some(1), Some(3)], "the middle lines pair in order");
        assert_eq!(added[2], None);
    }

    #[test]
    fn reindented_lines_pair_with_themselves() {
        let old = ["let a = load(id);", "let b = parse(a);", "save(b);"];
        let new = ["if ready {", "    let a = load(id);", "    let b = parse(a);", "    save(b);", "}"];
        let (old, added) = partners(&old, &new);
        assert_eq!(old, [Some(1), Some(2), Some(3)]);
        assert_eq!((added[0], added[4]), (None, None));
    }

    #[test]
    fn pairing_is_the_same_every_time() {
        let old = ["let total = subtotal + tax;"];
        let new = ["let total = subtotal + tax + fee;", "let total = subtotal + tax + fee;"];
        let first = partners(&old, &new);
        assert!((0..20).all(|_| partners(&old, &new) == first));
        assert_eq!(first, (vec![Some(0)], vec![Some(0), None]), "of equal choices, the earliest");
    }

    #[test]
    fn a_block_past_the_cap_pairs_in_order_and_compares_nothing() {
        let lines = |n: usize, tag: &str| (0..n).map(|i| format!("let {tag}{i} = compute({i});")).collect::<Vec<_>>();
        let block = |nd: usize, na: usize| {
            let (r, a) = (lines(nd, "x"), lines(na, "x"));
            let (r, a): (Vec<&str>, Vec<&str>) = (r.iter().map(String::as_str).collect(), a.iter().map(String::as_str).collect());
            let f = file(&r, &a[1..]);
            let before = likeness_cells();
            let p = Pairing::new(&f);
            (p, likeness_cells() - before)
        };
        let (p, cells) = block(500, 500);
        assert_eq!(cells, 0, "500 × 500 is past the cap: no line is compared");
        assert!(!p.blocks[0].by_text && p.matched.is_empty());
        assert_eq!(p.partner(0), Some(500), "in order, as before");

        let (p, cells) = block(128, 129);
        assert_eq!(cells, 128 * 128, "the cap itself is compared, each cell once");
        assert_eq!(p.partner(1), Some(128), "line 1 finds itself after the missing first line");
        assert_eq!(p.partner(0), None);
    }

    #[test]
    fn a_rewritten_file_is_read_to_its_first_difference_and_no_further() {
        let lines = |tag: &str| (0..3000).map(|i| format!("    let {tag}{i} = compute({i});")).collect::<Vec<_>>();
        let (old, new) = (lines("price"), lines("cost"));
        let (old, new): (Vec<&str>, Vec<&str>) = (old.iter().map(String::as_str).collect(), new.iter().map(String::as_str).collect());
        let f = file(&old, &new);
        let (cells, read) = (likeness_cells(), rain_core::formatting::work());
        let p = Pairing::new(&f);
        let now = rain_core::formatting::work();
        assert!(!p.blocks[0].quiet);
        assert_eq!(likeness_cells(), cells);
        assert_eq!(
            (now.ink - read.ink, now.lexed - read.lexed),
            (3, 0),
            "`let` matched, then `p` against `c`"
        );
    }

    #[test]
    fn single_pairs_and_one_sided_blocks_compare_nothing() {
        let before = likeness_cells();
        Pairing::new(&file(&["let a = 1;"], &["let a = 2;"]));
        Pairing::new(&file(&[], &["let a = 2;", "let b = 3;"]));
        Pairing::new(&file(&["let a = 2;", "let b = 3;"], &[]));
        assert_eq!(likeness_cells(), before);
    }
}
