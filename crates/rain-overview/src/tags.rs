//! Review tags per file, the reading order they imply, notes per file and a session budget.

use std::collections::{BTreeMap, BTreeSet};

use rain_core::FileTag;
use rain_core::overview::{OverviewCoverage, OverviewDocument, OverviewNote, RiskLevel};
use rain_core::paths::join;

/// Tags that collapse into one group row instead of being read file by file.
pub const GROUP_TAGS: [FileTag; 3] = [FileTag::FormattingOnly, FileTag::Generated, FileTag::Ignored];

pub fn is_group(tag: FileTag) -> bool {
    GROUP_TAGS.contains(&tag)
}

/// Lower is more important; an unrated file outranks a skim because nobody has looked at it.
pub fn importance(tag: FileTag) -> u8 {
    match tag {
        FileTag::MustReview => 0,
        FileTag::Review => 1,
        FileTag::Uncovered => 2,
        FileTag::Skim => 3,
        FileTag::FormattingOnly => 4,
        FileTag::Generated => 5,
        FileTag::Ignored => 6,
    }
}

/// Keys are `<repoId>/<path>`. A reading-order entry wins (by its risk), except that a
/// whitespace-only diff demotes anything below must-review to formatting-only; then ignore
/// globs, generated, formatting-only, and the rest is uncovered. Coverages without a repo are skipped.
pub fn tags(doc: &OverviewDocument, coverages: &[OverviewCoverage]) -> BTreeMap<String, FileTag> {
    let mut out = BTreeMap::new();
    for cov in coverages {
        let Some(repo) = cov.repo.as_deref() else { continue };
        let risks: BTreeMap<&str, _> = doc
            .reading_order
            .iter()
            .filter(|o| o.repo == repo)
            .flat_map(|o| o.review.iter().map(|e| (e.path.as_str(), e.risk)))
            .collect();
        fn set(v: &[String]) -> BTreeSet<&str> {
            v.iter().map(String::as_str).collect()
        }
        let (review, ignored, generated, formatting) =
            (set(&cov.review), set(&cov.ignored), set(&cov.generated), set(&cov.formatting_only));
        let all: BTreeSet<&str> = [&cov.review, &cov.uncovered, &cov.ignored, &cov.generated, &cov.formatting_only]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect();
        for path in all {
            let tag = if review.contains(path) {
                match FileTag::from_risk(risks.get(path).copied().flatten()) {
                    FileTag::MustReview => FileTag::MustReview,
                    _ if formatting.contains(path) => FileTag::FormattingOnly,
                    t => t,
                }
            } else if ignored.contains(path) {
                FileTag::Ignored
            } else if generated.contains(path) {
                FileTag::Generated
            } else if formatting.contains(path) {
                FileTag::FormattingOnly
            } else {
                FileTag::Uncovered
            };
            out.insert(join(repo, path), tag);
        }
    }
    out
}

/// What the reading order says about one file: its place in the narrative, risk and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rated {
    pub order: usize,
    pub risk: Option<RiskLevel>,
    pub why: String,
}

/// Every reading-order entry by `<repoId>/<path>`, numbered in authored order; the first mention wins.
pub fn rated(doc: &OverviewDocument) -> BTreeMap<String, Rated> {
    let mut out = BTreeMap::new();
    let entries = doc
        .reading_order
        .iter()
        .flat_map(|o| o.review.iter().map(move |e| (join(&o.repo, &e.path), e)));
    for (order, (key, e)) in entries.enumerate() {
        out.entry(key).or_insert_with(|| Rated {
            order,
            risk: e.risk,
            why: e.why.clone(),
        });
    }
    out
}

pub fn tag_of(tags: &BTreeMap<String, FileTag>, repo_path: &str) -> FileTag {
    tags.get(repo_path).copied().unwrap_or(FileTag::Uncovered)
}

/// Review entries in authored order (repos in reading-order order), then uncovered files in `files`
/// order, then the groups: formatting-only, generated, ignored. Only paths in `files` appear.
pub fn reading_order(doc: &OverviewDocument, files: &[String], tags: &BTreeMap<String, FileTag>) -> Vec<String> {
    let in_diff: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let mut seen = BTreeSet::new();
    let mut out = Vec::with_capacity(files.len());
    for o in &doc.reading_order {
        for e in &o.review {
            let key = join(&o.repo, &e.path);
            if in_diff.contains(key.as_str()) && !is_group(tag_of(tags, &key)) && seen.insert(key.clone()) {
                out.push(key);
            }
        }
    }
    for f in files {
        if !is_group(tag_of(tags, f)) && seen.insert(f.clone()) {
            out.push(f.clone());
        }
    }
    for g in GROUP_TAGS {
        for f in files {
            if tag_of(tags, f) == g && seen.insert(f.clone()) {
                out.push(f.clone());
            }
        }
    }
    out
}

/// Notes anchored in `repo_path` (`<repoId>/<path>`).
pub fn notes_for<'a>(doc: &'a OverviewDocument, repo_path: &str) -> Vec<&'a OverviewNote> {
    doc.notes.iter().filter(|n| n.path == repo_path).collect()
}

/// Changed lines per tier and the sessions they take at [`Budget::LINES_PER_SESSION`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    pub must: u64,
    pub review: u64,
    pub uncovered: u64,
    pub skim: u64,
    pub grouped: u64,
    pub sessions: u64,
}

impl Budget {
    /// SmartBear/Cisco: defect finding falls off past ~400 changed lines in one sitting.
    pub const LINES_PER_SESSION: u64 = 400;
}

/// `files` are `(repo_path, additions + deletions)`; skim and grouped lines are not budgeted.
pub fn budget(files: &[(&str, u32)], tags: &BTreeMap<String, FileTag>) -> Budget {
    let mut b = Budget::default();
    for &(path, lines) in files {
        let lines = u64::from(lines);
        match tag_of(tags, path) {
            FileTag::MustReview => b.must += lines,
            FileTag::Review => b.review += lines,
            FileTag::Uncovered => b.uncovered += lines,
            FileTag::Skim => b.skim += lines,
            _ => b.grouped += lines,
        }
    }
    b.sessions = (b.must + b.review + b.uncovered).div_ceil(Budget::LINES_PER_SESSION);
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coverage::coverage;
    use crate::test_support::sample;

    fn sample_tags() -> BTreeMap<String, FileTag> {
        let doc = sample();
        let server = coverage(
            &doc,
            Some("server"),
            &[
                "src/Api/VouchersController.cs",
                "src/Services/VoucherService.cs",
                "src/Data/Voucher.cs",
                "src/Migrations/X.Designer.cs",
                "src/Services/Unlisted.cs",
                "src/Services/Spacing.cs",
                "src/Gen.cs",
            ],
            &["src/Gen.cs"],
            &["src/Services/Spacing.cs", "src/Data/Voucher.cs"],
        );
        let app = coverage(
            &doc,
            Some("app"),
            &["src/app/vouchers/voucher.service.ts", "src/app/generated/x.ts"],
            &[],
            &[],
        );
        tags(&doc, &[server, app, coverage(&doc, None, &["x"], &[], &[])])
    }

    #[test]
    fn tag_table() {
        let t = sample_tags();
        let want = [
            ("server/src/Api/VouchersController.cs", FileTag::MustReview),
            ("server/src/Services/VoucherService.cs", FileTag::MustReview),
            ("server/src/Data/Voucher.cs", FileTag::FormattingOnly),
            ("server/src/Migrations/X.Designer.cs", FileTag::Ignored),
            ("server/src/Services/Unlisted.cs", FileTag::Uncovered),
            ("server/src/Services/Spacing.cs", FileTag::FormattingOnly),
            ("server/src/Gen.cs", FileTag::Generated),
            ("app/src/app/vouchers/voucher.service.ts", FileTag::Review),
            ("app/src/app/generated/x.ts", FileTag::Ignored),
        ];
        for (path, tag) in want {
            assert_eq!(t.get(path), Some(&tag), "{path}");
        }
        assert_eq!(t.len(), want.len());
        assert_eq!(tag_of(&t, "mobile/unknown.ts"), FileTag::Uncovered);
    }

    #[test]
    fn low_risk_is_skim_and_unrated_is_review() {
        let mut doc = sample();
        doc.reading_order[1].review[0].risk = Some(rain_core::overview::RiskLevel::Low);
        doc.reading_order[1].review[1].risk = None;
        let cov = coverage(
            &doc,
            Some("app"),
            &["src/app/vouchers/voucher-form.component.ts", "src/app/vouchers/voucher.service.ts"],
            &[],
            &[],
        );
        let t = tags(&doc, &[cov]);
        assert_eq!(t["app/src/app/vouchers/voucher-form.component.ts"], FileTag::Skim);
        assert_eq!(t["app/src/app/vouchers/voucher.service.ts"], FileTag::Review);
    }

    #[test]
    fn reading_order_puts_review_first_then_uncovered_then_groups() {
        let t = sample_tags();
        let files: Vec<String> = t.keys().rev().cloned().collect();
        assert_eq!(
            reading_order(&sample(), &files, &t),
            [
                "server/src/Api/VouchersController.cs",
                "server/src/Services/VoucherService.cs",
                "app/src/app/vouchers/voucher.service.ts",
                "server/src/Services/Unlisted.cs",
                "server/src/Services/Spacing.cs",
                "server/src/Data/Voucher.cs",
                "server/src/Gen.cs",
                "server/src/Migrations/X.Designer.cs",
                "app/src/app/generated/x.ts",
            ]
        );
    }

    #[test]
    fn notes_and_budget() {
        let doc = sample();
        assert_eq!(notes_for(&doc, "server/src/Api/VouchersController.cs").len(), 1);
        assert!(notes_for(&doc, "src/Api/VouchersController.cs").is_empty());
        let t = sample_tags();
        let b = budget(
            &[
                ("server/src/Api/VouchersController.cs", 500),
                ("app/src/app/vouchers/voucher.service.ts", 120),
                ("server/src/Gen.cs", 9000),
                ("x", 1),
            ],
            &t,
        );
        assert_eq!((b.must, b.review, b.uncovered, b.grouped, b.sessions), (500, 120, 1, 9000, 2));
    }

    #[test]
    fn rated_numbers_entries_in_authored_order() {
        let doc = sample();
        let r = rated(&doc);
        let first = &r["server/src/Api/VouchersController.cs"];
        assert_eq!(first.order, 0);
        assert!(!first.why.is_empty());
        let n = doc.reading_order.iter().map(|o| o.review.len()).sum::<usize>();
        assert!(r.values().all(|e| e.order < n));
        assert_eq!(r.len(), n);
    }

    #[test]
    fn importance_orders_groups_last() {
        let mut all = FileTag::ALL;
        all.sort_by_key(|t| importance(*t));
        assert_eq!(&all[..3], &[FileTag::MustReview, FileTag::Review, FileTag::Uncovered]);
        assert!(all[4..].iter().all(|t| is_group(*t)));
    }
}
