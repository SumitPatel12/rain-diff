//! Port of difit `server/overview/coverage.ts`: how one repo's diff splits into review, ignored,
//! generated, uncovered and formatting-only files, plus the self-repo match difit launch scripts use.

use std::collections::BTreeSet;

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use rain_core::overview::{OverviewCoverage, OverviewDocument};

pub fn empty_coverage(repo: Option<&str>) -> OverviewCoverage {
    OverviewCoverage {
        repo: repo.map(str::to_owned),
        ..OverviewCoverage::default()
    }
}

/// `**` crosses directories, `*` and `?` do not; case-insensitive. A glob globset cannot compile
/// is matched literally, as PR Lens treats every other character.
pub fn glob_set<'a>(globs: impl IntoIterator<Item = &'a str>) -> GlobSet {
    let mut set = GlobSetBuilder::new();
    for g in globs {
        let build = |p: &str| {
            GlobBuilder::new(p)
                .literal_separator(true)
                .case_insensitive(true)
                .backslash_escape(false)
                .build()
        };
        if let Ok(glob) = build(g).or_else(|_| build(&globset::escape(g))) {
            set.add(glob);
        }
    }
    set.build().unwrap_or_else(|_| GlobSet::empty())
}

pub fn matches_glob(glob: &str, path: &str) -> bool {
    glob_set([glob]).is_match(path)
}

/// `files`, `generated` and `formatting_only` are repo-relative diff paths of `repo`.
pub fn coverage(
    doc: &OverviewDocument,
    repo: Option<&str>,
    files: &[&str],
    generated: &[&str],
    formatting_only: &[&str],
) -> OverviewCoverage {
    let Some(repo) = repo else { return empty_coverage(None) };
    let order = doc.reading_order.iter().find(|o| o.repo == repo);
    let review_paths: Vec<&str> = order.iter().flat_map(|o| o.review.iter().map(|e| e.path.as_str())).collect();
    let globs = glob_set(order.iter().flat_map(|o| o.ignore.iter().map(|e| e.glob.as_str())));
    let in_diff: BTreeSet<&str> = files.iter().copied().collect();
    let generated: BTreeSet<&str> = generated.iter().copied().collect();

    let review: Vec<String> = review_paths
        .iter()
        .filter(|p| in_diff.contains(*p))
        .map(|p| (*p).to_owned())
        .collect();
    let missing_review = review_paths
        .iter()
        .filter(|p| !in_diff.contains(*p))
        .map(|p| (*p).to_owned())
        .collect();
    let review_set: BTreeSet<&str> = review.iter().map(String::as_str).collect();
    let mut cov = OverviewCoverage {
        repo: Some(repo.to_owned()),
        review: review.clone(),
        missing_review,
        ..Default::default()
    };
    for &f in files {
        let normalised = f.replace('\\', "/");
        let ignored = globs.is_match(&normalised);
        let is_generated = generated.contains(f);
        if ignored {
            cov.ignored.push(f.to_owned());
        }
        if is_generated {
            cov.generated.push(f.to_owned());
        }
        if !ignored && !is_generated && !review_set.contains(f) {
            cov.uncovered.push(f.to_owned());
        }
    }
    cov.formatting_only = formatting_only.iter().map(|f| (*f).to_owned()).collect();
    cov
}

/// Diff files with changed lines that have no non-zero row in `git diff --numstat -w -z` output.
/// `files` are `(path, additions + deletions)`.
pub fn parse_formatting_only(numstat: &str, files: &[(&str, u32)]) -> Vec<String> {
    let mut changed = BTreeSet::new();
    let tokens: Vec<&str> = numstat.split('\0').collect();
    let mut i = 0;
    while i < tokens.len() {
        let mut parts = tokens[i].splitn(3, '\t');
        if let (Some(a), Some(d), Some(path)) = (parts.next(), parts.next(), parts.next()) {
            let num = |s: &str| s == "-" || (!s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()));
            if num(a) && num(d) {
                let rows: Vec<&str> = if path.is_empty() {
                    vec![tokens.get(i + 1).copied().unwrap_or(""), tokens.get(i + 2).copied().unwrap_or("")]
                } else {
                    vec![path]
                };
                if path.is_empty() {
                    i += 2;
                }
                if !(a == "0" && d == "0") {
                    changed.extend(rows);
                }
            }
        }
        i += 1;
    }
    files
        .iter()
        .filter(|(p, n)| *n > 0 && !changed.contains(p))
        .map(|(p, _)| (*p).to_owned())
        .collect()
}

/// The overview repo a checkout folder is: its `dir` equal to the folder name (case-insensitive),
/// else the longest `dir` the folder name contains (a worktree like `acme-server-wt-123`).
pub fn self_repo<'a>(doc: &'a OverviewDocument, folder: &str) -> Option<&'a str> {
    let folder = folder.to_lowercase();
    doc.repos
        .iter()
        .find(|r| r.dir.to_lowercase() == folder)
        .or_else(|| {
            doc.repos
                .iter()
                .filter(|r| folder.contains(&r.dir.to_lowercase()))
                .max_by_key(|r| r.dir.len())
        })
        .map(|r| r.id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::sample;

    #[test]
    fn splits_the_diff_into_review_ignored_generated_and_uncovered() {
        let doc = sample();
        let cov = coverage(
            &doc,
            Some("server"),
            &[
                "src/Data/Voucher.cs",
                "src/Api/VouchersController.cs",
                "src/Services/VoucherService.cs",
                "src/Services/AccountService.cs",
                "src/Migrations/X.Designer.cs",
                "src/Services/Unlisted.cs",
            ],
            &["src/Migrations/X.Designer.cs"],
            &[],
        );
        assert_eq!(
            cov,
            OverviewCoverage {
                repo: Some("server".into()),
                review: vec![
                    "src/Api/VouchersController.cs".into(),
                    "src/Services/VoucherService.cs".into(),
                    "src/Services/AccountService.cs".into(),
                    "src/Data/Voucher.cs".into()
                ],
                missing_review: vec![],
                ignored: vec!["src/Migrations/X.Designer.cs".into()],
                generated: vec!["src/Migrations/X.Designer.cs".into()],
                uncovered: vec!["src/Services/Unlisted.cs".into()],
                formatting_only: vec![],
            }
        );
    }

    #[test]
    fn lists_review_entries_not_in_the_diff() {
        let cov = coverage(&sample(), Some("server"), &["src/Api/VouchersController.cs"], &[], &[]);
        assert_eq!(cov.review, ["src/Api/VouchersController.cs"]);
        assert_eq!(
            cov.missing_review,
            [
                "src/Services/VoucherService.cs",
                "src/Services/AccountService.cs",
                "src/Data/Voucher.cs"
            ]
        );
    }

    #[test]
    fn double_star_crosses_directories_single_star_does_not() {
        let cov = coverage(
            &sample(),
            Some("app"),
            &["src/app/generated/models/voucher.ts", "src/app/vouchers/voucher.service.ts"],
            &[],
            &[],
        );
        assert_eq!(cov.ignored, ["src/app/generated/models/voucher.ts"]);
        assert!(cov.uncovered.is_empty());
        assert!(matches_glob("src/Migrations/*.Designer.cs", "src/Migrations/1_Add.Designer.cs"));
        assert!(!matches_glob("src/Migrations/*.Designer.cs", "src/Migrations/a/1.Designer.cs"));
        assert!(matches_glob("**/*.snap", "a/b/c.snap") && matches_glob("**/*.snap", "c.snap"));
        assert!(matches_glob("src/MIGRATIONS/*.cs", "src/Migrations/A.cs"));
        assert!(matches_glob("src/[weird", "src/[weird"));
    }

    #[test]
    fn generated_files_are_covered_without_a_glob() {
        let cov = coverage(&sample(), Some("app"), &["package-lock.json"], &["package-lock.json"], &[]);
        assert_eq!(cov.generated, ["package-lock.json"]);
        assert!(cov.uncovered.is_empty());
    }

    #[test]
    fn formatting_only_passes_through_and_no_repo_is_empty() {
        let doc = sample();
        let cov = coverage(
            &doc,
            Some("server"),
            &["src/Api/VouchersController.cs", "src/Services/Spacing.cs"],
            &[],
            &["src/Services/Spacing.cs"],
        );
        assert_eq!(cov.formatting_only, ["src/Services/Spacing.cs"]);
        assert_eq!(
            coverage(&doc, None, &["src/Api/VouchersController.cs"], &[], &[]),
            empty_coverage(None)
        );
    }

    #[test]
    fn parses_formatting_only_from_numstat() {
        let files = [
            ("src/A.cs", 1),
            ("src/Spacing.cs", 1),
            ("src/Renamed.cs", 1),
            ("src/Empty.cs", 0),
            ("src/Zero.cs", 1),
        ];
        let numstat = ["3\t1\tsrc/A.cs", "2\t0\t", "src/Old.cs", "src/Renamed.cs", "0\t0\tsrc/Zero.cs", ""].join("\0");
        assert_eq!(parse_formatting_only(&numstat, &files), ["src/Spacing.cs", "src/Zero.cs"]);
    }

    #[test]
    fn self_repo_matches_folder_then_containment() {
        let doc = sample();
        assert_eq!(self_repo(&doc, "acme-server"), Some("server"));
        assert_eq!(self_repo(&doc, "Acme-Server"), Some("server"));
        assert_eq!(self_repo(&doc, "acme-server-wt-ACME-1234"), Some("server"));
        assert_eq!(self_repo(&doc, "acme-app"), Some("app"));
        assert_eq!(self_repo(&doc, "acme-mobile"), None);
        let mut nested = sample();
        nested.repos[0].dir = "acme-app-admin".into();
        nested.repos[0].id = "admin".into();
        nested.repos.reverse();
        assert_eq!(self_repo(&nested, "acme-app-admin-wt-1"), Some("admin"));
    }
}
