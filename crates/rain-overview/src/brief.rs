//! The Brief against the loaded diff: where its cites land, what it gets wrong (`check`), code
//! spans in its prose that name diff files, and the tour it implies.

use std::collections::BTreeSet;
use std::ops::Range;

use rain_core::Side;
use rain_core::brief::{BriefDoc, Cite, TourStep};
use serde::Serialize;

use crate::coverage::glob_set;

/// The key a cite or code span names: the key itself, else the one key ending in `/<path>`, else
/// `path` without its repo segment (a one-repo session keys bare paths), else the one key whose
/// file name is `path`. `None` when nothing or several match.
pub fn resolve<'k>(path: &str, keys: &'k [String]) -> Option<&'k str> {
    let path = path.trim().trim_start_matches("./");
    if path.is_empty() {
        return None;
    }
    if let Some(k) = keys.iter().find(|k| k.as_str() == path) {
        return Some(k);
    }
    let unique = |mut it: Box<dyn Iterator<Item = &'k String> + '_>| match (it.next(), it.next()) {
        (Some(k), None) => Some(k.as_str()),
        _ => None,
    };
    let suffix = format!("/{path}");
    let bare = path.split_once('/').map(|(_, rest)| rest);
    unique(Box::new(keys.iter().filter(move |k| k.ends_with(&suffix))))
        .or_else(|| bare.and_then(|b| keys.iter().find(|k| k.as_str() == b)).map(String::as_str))
        .or_else(|| {
            (!path.contains('/'))
                .then(|| unique(Box::new(keys.iter().filter(move |k| k.rsplit('/').next() == Some(path)))))
                .flatten()
        })
}

/// A cite that points at lines no hunk of its file shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutsideCite {
    pub section: String,
    pub path: String,
    pub lines: (u32, u32),
}

/// A repo whose loaded target is not the sha the brief was written against.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StaleSha {
    pub repo: String,
    pub written: String,
    pub loaded: String,
}

/// What `GET /api/brief` reports as `check`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BriefCheck {
    /// Cited paths that name no file of the diff.
    pub phantom_paths: Vec<String>,
    pub cites_outside_diff: Vec<OutsideCite>,
    /// Real changes no cite names and `notCovered` does not exclude.
    pub files_not_cited: Vec<String>,
    pub stale: Vec<StaleSha>,
}

impl BriefCheck {
    pub fn is_clean(&self) -> bool {
        self == &BriefCheck::default()
    }
}

/// Whether file `key` shows any of `lines` on `side`.
pub type ShownFn<'a> = &'a dyn Fn(&str, Side, (u32, u32)) -> bool;

/// What [`check`] reads from the session and the loaded diff.
pub struct CheckInput<'a> {
    /// Every file key of the diff, in reading order.
    pub keys: &'a [String],
    /// Keys a reviewer accepts without reading (generated, formatting-only, lockfiles, moves):
    /// never reported as uncited.
    pub noise: &'a BTreeSet<String>,
    /// `(repo id, loaded target sha)` per repo.
    pub targets: &'a [(String, String)],
    /// `None` skips the line check.
    pub shown: Option<ShownFn<'a>>,
}

/// Phantom paths, cites outside the diff, files the brief never cites, and stale shas.
pub fn check(doc: &BriefDoc, input: &CheckInput) -> BriefCheck {
    let mut out = BriefCheck::default();
    let mut cited: BTreeSet<&str> = BTreeSet::new();
    for (section, c) in doc.cites() {
        match resolve(&c.path, input.keys) {
            None => {
                if !out.phantom_paths.contains(&c.path) {
                    out.phantom_paths.push(c.path.clone());
                }
            }
            Some(k) => {
                cited.insert(k);
                if let (Some(lines), Some(shown)) = (c.lines, input.shown)
                    && !shown(k, c.side.unwrap_or(Side::New), lines)
                {
                    out.cites_outside_diff.push(OutsideCite {
                        section: section.to_owned(),
                        path: k.to_owned(),
                        lines,
                    });
                }
            }
        }
    }
    let skipped = glob_set(doc.not_covered.iter().flat_map(|n| n.paths.iter().map(String::as_str)));
    out.files_not_cited = input
        .keys
        .iter()
        .filter(|k| !cited.contains(k.as_str()) && !input.noise.contains(*k) && !skipped.is_match(k.as_str()))
        .cloned()
        .collect();
    out.stale = stale(doc, input.targets);
    out
}

/// Repos the brief names at a sha other than the loaded one. Either sha may be short; a brief
/// with one unnamed repo matches a one-repo session.
pub fn stale(doc: &BriefDoc, targets: &[(String, String)]) -> Vec<StaleSha> {
    doc.written_against
        .iter()
        .filter_map(|w| {
            let (_, loaded) = targets
                .iter()
                .find(|(id, _)| *id == w.repo || (targets.len() == 1 && (w.repo.is_empty() || id.is_empty())))?;
            let same = loaded.starts_with(&w.sha) || w.sha.starts_with(loaded.as_str());
            (!loaded.is_empty() && !w.sha.is_empty() && !same).then(|| StaleSha {
                repo: w.repo.clone(),
                written: w.sha.clone(),
                loaded: loaded.clone(),
            })
        })
        .collect()
}

/// A backticked span in prose that names a diff file, optionally with lines: `VoucherService.cs:27-31`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeRef {
    /// Byte range of the span's text inside the prose, backticks excluded.
    pub range: Range<usize>,
    pub key: String,
    pub lines: Option<(u32, u32)>,
}

/// `path`, `path:12`, `path:12-30` or `path#L12-L30` → the path and its lines.
pub fn split_lines(s: &str) -> (&str, Option<(u32, u32)>) {
    let parse = |t: &str| -> Option<(u32, u32)> {
        let t = t.trim_start_matches('L');
        let (a, b) = t.split_once('-').unwrap_or((t, t));
        let a: u32 = a.parse().ok()?;
        let b: u32 = b.trim_start_matches('L').parse().ok()?;
        (a > 0).then_some((a, b.max(a)))
    };
    for sep in [':', '#'] {
        if let Some((p, l)) = s.rsplit_once(sep)
            && let Some(lines) = parse(l)
        {
            return (p, Some(lines));
        }
    }
    (s, None)
}

/// Every backticked span of `text` that resolves to one of `keys`, in order.
pub fn code_refs(text: &str, keys: &[String]) -> Vec<CodeRef> {
    let mut out = Vec::new();
    let mut rest = 0;
    while let Some(open) = text[rest..].find('`').map(|i| rest + i) {
        let Some(close) = text[open + 1..].find('`').map(|i| open + 1 + i) else {
            break;
        };
        let inner = &text[open + 1..close];
        let (path, lines) = split_lines(inner);
        let looks_like_file = path.contains('.') && !path.contains(' ');
        if looks_like_file && let Some(k) = resolve(path, keys) {
            out.push(CodeRef {
                range: open + 1..close,
                key: k.to_owned(),
                lines,
            });
        }
        rest = close + 1;
    }
    out
}

/// Chip labels for `paths` (each optionally ending in `:a-b`): the shortest path suffix no other
/// cite or diff key with the same file name shares, `first/…/name` when a shared middle would
/// make it long, the whole path when that is short; lines appended. A path that resolves to a
/// key is labelled from the key.
pub fn cite_labels(paths: &[&str], keys: &[String]) -> Vec<String> {
    let full: Vec<(String, Option<(u32, u32)>)> = paths
        .iter()
        .map(|p| {
            let (path, lines) = split_lines(p);
            let path = resolve(path, keys).unwrap_or(path.trim().trim_start_matches("./"));
            (path.to_owned(), lines)
        })
        .collect();
    let mut all: Vec<&str> = keys.iter().map(String::as_str).collect();
    all.extend(full.iter().map(|(p, _)| p.as_str()));
    all.sort_unstable();
    all.dedup();
    full.iter()
        .map(|(path, lines)| {
            let label = short_label(path, &all);
            match lines {
                Some((a, b)) if a == b => format!("{label}:{a}"),
                Some((a, b)) => format!("{label}:{a}-{b}"),
                None => label,
            }
        })
        .collect()
}

fn short_label(path: &str, all: &[&str]) -> String {
    let segs: Vec<&str> = path.split('/').collect();
    let name = segs[segs.len() - 1];
    let others: Vec<Vec<&str>> = all
        .iter()
        .filter(|o| **o != path && o.rsplit('/').next() == Some(name))
        .map(|o| o.split('/').collect())
        .collect();
    let shares = |o: &Vec<&str>, k: usize| o.len() >= k && o[o.len() - k..] == segs[segs.len() - k..];
    let k = (1..=segs.len())
        .find(|&k| !others.iter().any(|o| shares(o, k)))
        .unwrap_or(segs.len());
    let suffix = segs[segs.len() - k..].join("/");
    if k < 3 || (k == segs.len() && path.len() <= 32) {
        return suffix;
    }
    let first = segs[segs.len() - k];
    let elided_unique = !others.iter().any(|o| o.len() >= k && o[o.len() - k] == first);
    if elided_unique { format!("{first}/…/{name}") } else { suffix }
}

/// A tour step whose cite resolved against the diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub step: TourStep,
    /// `None` when the cite names no file of the diff.
    pub key: Option<String>,
}

/// The brief's tour (else one step per `how[]` cite), each cite resolved to a key.
pub fn tour(doc: &BriefDoc, keys: &[String]) -> Vec<Step> {
    doc.tour_steps()
        .into_iter()
        .map(|step| Step {
            key: resolve(&step.cite.path, keys).map(str::to_owned),
            step,
        })
        .collect()
}

/// The keys a section's cites resolve to, deduplicated in cite order.
pub fn section_keys(cites: &[Cite], keys: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in cites {
        if let Some(k) = resolve(&c.path, keys)
            && !out.iter().any(|x| x == k)
        {
            out.push(k.to_owned());
        }
    }
    out
}

/// `Brief › How 2`, `Brief › Intent`, `Brief › Invariant 1`: the title an artifact composer and
/// readback use for section `id`.
pub fn section_title(doc: &BriefDoc, id: &str) -> String {
    if let Some(i) = doc.how.iter().position(|h| h.id == id) {
        return format!("Brief › How {}", i + 1);
    }
    if let Some(i) = doc.invariants.iter().position(|v| v.id == id) {
        return format!("Brief › Invariant {}", i + 1);
    }
    match doc.section(id) {
        Some(s) => format!("Brief › {}", s.title),
        None => format!("Brief › {id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../../fixtures/brief/sample.json");
    const M_KEYS: &str = include_str!("../../../fixtures/brief/m-keys.txt");
    const EXPECTED: &str = include_str!("../../../fixtures/brief/sample-check-m.json");

    fn sample() -> BriefDoc {
        let mut d: BriefDoc = serde_json::from_str(SAMPLE).unwrap();
        d.assign_ids();
        d
    }

    fn m_keys() -> Vec<String> {
        M_KEYS.lines().filter(|l| !l.is_empty()).map(str::to_owned).collect()
    }

    #[test]
    fn resolve_takes_keys_suffixes_and_unique_file_names() {
        let keys: Vec<String> = ["server/src/A.cs", "server/src/B.cs", "app/src/B.cs"].map(String::from).to_vec();
        assert_eq!(resolve("server/src/A.cs", &keys), Some("server/src/A.cs"));
        assert_eq!(resolve("src/A.cs", &keys), Some("server/src/A.cs"));
        assert_eq!(resolve("A.cs", &keys), Some("server/src/A.cs"));
        assert_eq!(resolve("B.cs", &keys), None, "two files are called B.cs");
        assert_eq!(resolve("server/src/B.cs", &keys), Some("server/src/B.cs"));
        assert_eq!(resolve("Nope.cs", &keys), None);
        let one_repo = vec!["src/A.cs".to_owned()];
        assert_eq!(resolve("server/src/A.cs", &one_repo), Some("src/A.cs"));
    }

    #[test]
    fn check_on_the_sample_against_m_matches_the_fixture() {
        let doc = sample();
        let keys = m_keys();
        assert_eq!(keys.len(), 40);
        let noise: BTreeSet<String> = keys.iter().filter(|k| k.contains("/i18n/")).cloned().collect();
        let targets = vec![
            ("server".to_owned(), "a1b2c3d".to_owned()),
            ("app".to_owned(), "ffff000".to_owned()),
        ];
        let shown = |k: &str, _: Side, l: (u32, u32)| !(k.ends_with("Voucher2Controller.cs") && l.0 > 100);
        let c = check(
            &doc,
            &CheckInput {
                keys: &keys,
                noise: &noise,
                targets: &targets,
                shown: Some(&shown),
            },
        );
        let expected: serde_json::Value = serde_json::from_str(EXPECTED).unwrap();
        assert_eq!(serde_json::to_value(&c).unwrap(), expected);
        assert_eq!(c.phantom_paths, ["server/src/Services/VoucherService.cs"]);
        assert_eq!(c.stale.len(), 1, "app was loaded at another sha");
        assert!(
            !c.files_not_cited.iter().any(|k| k.ends_with(".scss")),
            "notCovered globs are skipped"
        );
    }

    #[test]
    fn lines_outside_the_diff_are_reported_per_section() {
        let mut doc = sample();
        doc.how[0].cites[0].lines = Some((400, 410));
        let keys = m_keys();
        let shown = |_: &str, _: Side, l: (u32, u32)| l.0 < 100;
        let c = check(
            &doc,
            &CheckInput {
                keys: &keys,
                noise: &BTreeSet::new(),
                targets: &[],
                shown: Some(&shown),
            },
        );
        assert_eq!(
            c.cites_outside_diff,
            [OutsideCite {
                section: "how.1".into(),
                path: "server/src/Acme.Api/Controllers/Voucher2Controller.cs".into(),
                lines: (400, 410)
            }]
        );
        assert!(c.stale.is_empty());
    }

    #[test]
    fn code_spans_link_to_diff_files_with_lines() {
        let keys: Vec<String> = ["server/src/Services/VoucherService.cs", "server/src/Api/VouchersController.cs"]
            .map(String::from)
            .to_vec();
        let text = "Calls `VoucherService.cs:27-31` from `VouchersController.cs`, not `POST /voucher` or `Other.cs`.";
        let refs = code_refs(text, &keys);
        assert_eq!(refs.len(), 2);
        assert_eq!(&text[refs[0].range.clone()], "VoucherService.cs:27-31");
        assert_eq!(refs[0].key, "server/src/Services/VoucherService.cs");
        assert_eq!(refs[0].lines, Some((27, 31)));
        assert_eq!(refs[1].lines, None);
        assert_eq!(split_lines("a.cs#L3-L9"), ("a.cs", Some((3, 9))));
        assert_eq!(split_lines("a.cs:12"), ("a.cs", Some((12, 12))));
        assert_eq!(split_lines("http://x"), ("http://x", None));
    }

    #[test]
    fn cite_labels_disambiguate_two_files_with_one_name() {
        let keys: Vec<String> = [
            "rain/crates/rain-core/src/tags.rs",
            "rain/crates/rain-overview/src/tags.rs",
            "rain/crates/rain-app/src/lib.rs",
        ]
        .map(String::from)
        .to_vec();
        let labels = cite_labels(
            &["rain-core/src/tags.rs:253-353", "crates/rain-overview/src/tags.rs", "lib.rs:7"],
            &keys,
        );
        assert_eq!(labels, ["rain-core/…/tags.rs:253-353", "rain-overview/…/tags.rs", "lib.rs:7"]);
    }

    #[test]
    fn cite_labels_keep_short_paths_whole_and_avoid_ambiguous_ellipses() {
        let keys: Vec<String> = ["src/a/n.rs", "lib/a/n.rs", "x/a/m.rs", "y/a/m.rs", "x/b/m.rs", "one/two.rs"]
            .map(String::from)
            .to_vec();
        assert_eq!(cite_labels(&["src/a/n.rs"], &keys), ["src/a/n.rs"], "the whole path, short");
        assert_eq!(cite_labels(&["x/b/m.rs:4"], &keys), ["b/m.rs:4"], "two segments are enough");
        assert_eq!(
            cite_labels(&["x/a/m.rs"], &keys),
            ["x/a/m.rs"],
            "x/…/m.rs would also match x/b/m.rs"
        );
        assert_eq!(cite_labels(&["two.rs:3-3"], &keys), ["two.rs:3"]);
        assert_eq!(cite_labels(&["Nowhere.cs"], &keys), ["Nowhere.cs"], "unresolved keeps its name");
        let long: Vec<String> = ["repo/crates/alpha/src/deep/mod.rs", "repo/crates/beta/src/deep/mod.rs"]
            .map(String::from)
            .to_vec();
        assert_eq!(cite_labels(&["repo/crates/alpha/src/deep/mod.rs"], &long), ["alpha/…/mod.rs"]);
    }

    #[test]
    fn tour_resolves_each_step_and_titles_name_sections() {
        let doc = sample();
        let keys = m_keys();
        let steps = tour(&doc, &keys);
        assert_eq!(steps.len(), 2);
        assert_eq!(
            steps[0].key.as_deref(),
            Some("server/src/Acme.Api/Controllers/Voucher2Controller.cs")
        );
        let mut no_tour = doc.clone();
        no_tour.tour.clear();
        let derived = tour(&no_tour, &keys);
        assert_eq!(derived.len(), 5);
        assert_eq!(derived[2].key, None, "the phantom VoucherService.cs");
        assert_eq!(section_title(&doc, "how.2"), "Brief › How 2");
        assert_eq!(section_title(&doc, "inv.1"), "Brief › Invariant 1");
        assert_eq!(section_title(&doc, "verify"), "Brief › Verify by hand");
        assert_eq!(
            section_keys(&doc.how[1].cites, &keys),
            ["server/src/Acme.Data/Migrations/20260101000000_AddVoucherColumns.cs"]
        );
    }

    #[test]
    fn how_sections_group_the_files_they_cite() {
        let ch = crate::chapters_of_brief(&sample(), &m_keys());
        let titles: Vec<&str> = ch.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(
            titles,
            [
                "The API records a voucher",
                "One Voucher row in the same unit of work",
                "The form shows the voucher"
            ]
        );
        assert_eq!(ch[0].1.len(), 2);
        assert_eq!(ch[2].1, ["app/src/app/voucher0/voucher0-form.component.html"]);
    }
}
