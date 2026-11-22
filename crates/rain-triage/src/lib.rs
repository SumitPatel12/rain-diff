//! Deterministic triage: every changed file gets a group and the reason for it, a risk where one
//! can be told without an agent, and a place in one stable order; plus the change's facts and
//! shape. The same input gives the same bytes.

pub mod rules;
mod ws;

use std::collections::{BTreeMap, BTreeSet};

use rain_core::overview::RiskLevel;
use rain_core::{FileDiff, FileStatus, FileTag, LineKind};
use rain_overview::Rated;
use serde::{Deserialize, Serialize};

pub use ws::{Ws, analyse};

/// Files over this many diff lines are their own group (and are not read for whitespace).
pub const HUGE_LINES: usize = 20_000;
/// A rename with at most this many changed lines reads as a move with edits.
pub const MOVE_EDITS: u32 = 5;
/// Changed lines past which a real change is worth a closer look.
pub const CHURN_LINES: u32 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Group {
    Must,
    Review,
    Skim,
    Tests,
    Moves,
    FormattingOnly,
    Generated,
    Lockfile,
    Huge,
    Binary,
    Ignored,
}

impl Group {
    /// In list order.
    pub const ALL: [Group; 11] = [
        Group::Must,
        Group::Review,
        Group::Skim,
        Group::Tests,
        Group::Moves,
        Group::FormattingOnly,
        Group::Generated,
        Group::Lockfile,
        Group::Huge,
        Group::Binary,
        Group::Ignored,
    ];

    /// Must, Review and Skim are read file by file; the rest are groups.
    pub fn is_change(self) -> bool {
        matches!(self, Group::Must | Group::Review | Group::Skim)
    }

    pub fn label(self) -> &'static str {
        match self {
            Group::Must => "Must",
            Group::Review => "Review",
            Group::Skim => "Skim",
            Group::Tests => "Tests",
            Group::Moves => "Moves",
            Group::FormattingOnly => "Formatting-only",
            Group::Generated => "Generated",
            Group::Lockfile => "Lockfile",
            Group::Huge => "Huge",
            Group::Binary => "Binary",
            Group::Ignored => "Ignored",
        }
    }

    /// The round-2 tag the file tree still filters by.
    pub fn tag(self) -> FileTag {
        match self {
            Group::Must => FileTag::MustReview,
            Group::Review | Group::Tests | Group::Huge => FileTag::Review,
            Group::Skim | Group::Moves | Group::Binary => FileTag::Skim,
            Group::FormattingOnly => FileTag::FormattingOnly,
            Group::Generated | Group::Lockfile => FileTag::Generated,
            Group::Ignored => FileTag::Ignored,
        }
    }

    fn from_risk(r: Option<RiskLevel>) -> Group {
        match r {
            Some(RiskLevel::Critical | RiskLevel::High) => Group::Must,
            Some(RiskLevel::Medium) | None => Group::Review,
            Some(RiskLevel::Low) => Group::Skim,
        }
    }
}

/// Why a file is in its group. `trusted` reasons are facts about the diff (path rules, `-w`,
/// renames); the rest are heuristics or the agent's word.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reason {
    pub group: Group,
    pub trusted: bool,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// `<repoId>/<path>`.
    pub key: String,
    pub group: Group,
    pub reason: Reason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<RiskLevel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// The one real line of an otherwise whitespace-only change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub real_line: Option<u32>,
    /// A test's subject, or a subject's test, by key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pair: Option<String>,
    /// The old key of a renamed file and its changed line count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_from: Option<(String, u32)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Shape {
    /// Under 10 files and 200 changed lines.
    Tiny,
    #[default]
    Dense,
    /// 100+ files, most of them in groups.
    SparseHuge,
    /// Generated, lockfile and huge files carry most of the lines.
    GeneratedHuge,
    /// A second or later round.
    Iterative,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupCount {
    pub group: Group,
    pub count: usize,
    /// Added plus removed lines.
    pub lines: u64,
    /// Every file's reason is trusted.
    pub trusted: bool,
    /// The first file's reason.
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestOff {
    pub key: String,
    pub line: u32,
    pub marker: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub files: usize,
    pub lines: u64,
    /// Files read one by one (Must, Review, Skim).
    pub real: usize,
    pub tests: usize,
    pub tests_added: usize,
    pub tests_removed: usize,
    /// Added lines in tests that skip, focus or ignore a test.
    pub tests_off: Vec<TestOff>,
    pub migrations: usize,
    pub renames: usize,
    pub binaries: usize,
    /// Real changes the overview's reading order never mentions (`None` without one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uncited: Option<usize>,
}

/// What `GET /api/triage` returns.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Triage {
    pub shape: Shape,
    pub groups: Vec<GroupCount>,
    /// In list order.
    pub items: Vec<Item>,
    pub facts: Facts,
}

impl Triage {
    pub fn tags(&self) -> BTreeMap<String, FileTag> {
        self.items.iter().map(|i| (i.key.clone(), i.group.tag())).collect()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input<'a> {
    pub files: &'a [FileDiff],
    /// `<repoId>/<path>` per file.
    pub keys: &'a [String],
    /// Rename similarity per file (empty, or parallel to `files`).
    pub similarity: &'a [Option<u8>],
    /// Keys whose every change vanishes under `git diff -w`.
    pub formatting_only: Option<&'a BTreeSet<String>>,
    /// Keys whose every changed line is part of a block moved unchanged (see
    /// [`rain_core::moves`]).
    pub moved_only: Option<&'a BTreeSet<String>>,
    /// The overview's reading order by key; its presence also makes `Facts::uncited` known.
    pub rated: Option<&'a BTreeMap<String, Rated>>,
    /// Keys the overview's ignore globs cover.
    pub ignored: Option<&'a BTreeSet<String>>,
    pub round: u32,
}

struct Decided {
    group: Group,
    reason: Reason,
    risk: Option<RiskLevel>,
    notes: Vec<String>,
    real_line: Option<u32>,
    moved_from: Option<(String, u32)>,
}

/// A file whose changes are whitespace and re-wrapped code: a reading of the lines, not a fact
/// git gives, so the reason is not trusted.
const REWRAPPED: &str = "formatting-only (re-wrapped)";

/// A file whose changes are all blocks found again elsewhere: matched text, so not trusted.
const MOVED_BLOCKS: &str = "moved blocks (same lines elsewhere)";

fn reason(group: Group, trusted: bool, text: impl Into<String>) -> Reason {
    Reason {
        group,
        trusted,
        text: text.into(),
    }
}

/// `key` with its path replaced by `path` (same repo prefix).
fn sibling_key(key: &str, f: &FileDiff, path: &str) -> String {
    format!("{}{path}", &key[..key.len().saturating_sub(f.path.len())])
}

fn generated_reason(f: &FileDiff) -> Option<&'static str> {
    if rain_git::generated::generated_path(&f.path) {
        return Some("generated (path rule)");
    }
    let header = rain_git::header_from_diff(f).is_some_and(|h| rain_git::generated::generated_header(&f.path, &h));
    match (header, f.generated) {
        (true, _) => Some("generated (header)"),
        (false, true) => Some("generated (linguist-generated or header)"),
        (false, false) => None,
    }
}

fn decide(f: &FileDiff, key: &str, similarity: Option<u8>, input: &Input, is_test: bool) -> Decided {
    let changes = f.additions + f.deletions;
    let grouped = |group, text: &str| Decided {
        group,
        reason: reason(group, true, text),
        risk: None,
        notes: Vec::new(),
        real_line: None,
        moved_from: None,
    };
    if f.binary {
        return grouped(Group::Binary, "binary");
    }
    if rules::lockfile(&f.path) {
        return grouped(Group::Lockfile, "lockfile");
    }
    let migration = rules::migration(&f.path);
    let mut found: Vec<(RiskLevel, String)> = Vec::new();
    if migration {
        found.push((RiskLevel::High, "migration".into()));
    }
    let added = f
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.kind == LineKind::Added)
        .map(|(i, _)| f.line_text(i as u32));
    if let Some(what) = rules::public_api(&f.path, added) {
        found.push((RiskLevel::High, format!("public API ({what})")));
    }
    if let Some(word) = rules::money_word(&f.path) {
        found.push((RiskLevel::High, format!("money path ({word})")));
    }
    // A risky file is not hidden on a header, a whitespace check or the overview's ignore list.
    let risky = !found.is_empty();
    let mut overridden: Option<String> = None;
    match generated_reason(f) {
        Some(why) if risky && why != "generated (path rule)" => overridden = Some(why.into()),
        Some(why) => return grouped(Group::Generated, why),
        None => {}
    }
    if f.lines.len() > HUGE_LINES {
        return grouped(Group::Huge, &format!("huge ({} diff lines)", f.lines.len()));
    }
    let renamed = f.status == FileStatus::Renamed && f.old_path.is_some();
    if renamed && (similarity == Some(100) || changes == 0) {
        return grouped(
            Group::Moves,
            if similarity == Some(100) {
                "moved (R100)"
            } else {
                "moved (no changes)"
            },
        );
    }
    let w = analyse(f);
    let formatting = if input.formatting_only.is_some_and(|s| s.contains(key)) && !ws::indent_sensitive(&f.path) {
        Some("formatting-only (-w empty)")
    } else if w.changed > 0 && w.real == 0 && w.rewrapped > 0 {
        Some(REWRAPPED)
    } else if w.changed > 0 && w.real == 0 {
        Some("formatting-only (whitespace)")
    } else {
        None
    };
    for (skip, group) in [
        (formatting, Group::FormattingOnly),
        (
            input.moved_only.is_some_and(|s| s.contains(key)).then_some(MOVED_BLOCKS),
            Group::Moves,
        ),
        (
            input.ignored.is_some_and(|s| s.contains(key)).then_some("ignored (overview)"),
            Group::Ignored,
        ),
    ] {
        match skip {
            Some(why) if risky => overridden = overridden.or(Some(why.into())),
            Some(why) => {
                let mut d = grouped(group, why);
                d.reason.trusted = why != REWRAPPED && why != MOVED_BLOCKS;
                return d;
            }
            None => {}
        }
    }
    if is_test && !migration {
        return grouped(Group::Tests, "test");
    }

    if changes > CHURN_LINES {
        found.push((RiskLevel::Medium, format!("heuristic: churn ({changes} lines)")));
    }
    let real_line = (w.real == 1 && w.matched + w.rewrapped > 0).then_some(w.real_line).flatten();
    let moved_from = renamed.then(|| (sibling_key(key, f, f.old_path.as_deref().unwrap_or_default()), changes));
    let mut notes: Vec<String> = found.iter().skip(1).map(|(_, t)| t.clone()).collect();
    notes.extend(overridden);
    if let Some(l) = real_line {
        notes.push(format!("formatting + real line (L{l})"));
    }
    if let Some((old, n)) = &moved_from {
        notes.push(format!("moved from {old}, {n} edits"));
    }
    let risk = found.iter().map(|(r, _)| *r).max();
    let group = if risk == Some(RiskLevel::High) {
        Group::Must
    } else {
        Group::Review
    };
    let reason = match found.first() {
        Some((_, text)) => reason(group, false, text.clone()),
        None => match (real_line, &moved_from) {
            (Some(l), _) => reason(group, true, format!("formatting + real line (L{l})")),
            (None, Some((_, n))) if *n <= MOVE_EDITS => reason(group, true, format!("moved with {n} edits (rename)")),
            _ => reason(group, true, "change"),
        },
    };
    Decided {
        group,
        reason,
        risk,
        notes,
        real_line,
        moved_from,
    }
}

/// Classifies every file of `input` and orders them: Must, Review, Skim (the overview's narrative
/// order first, then by key), then tests beside their subjects, then moves, formatting-only,
/// generated, lockfiles, huge, binary and ignored files (by key).
pub fn classify(input: &Input) -> Triage {
    let files = input.files;
    let keys = input.keys;
    let n = files.len().min(keys.len());
    let test_stems: Vec<Option<String>> = files.iter().take(n).map(|f| rules::test_stem(&f.path)).collect();

    let mut subjects: BTreeMap<(u16, &str), &str> = BTreeMap::new();
    let mut by_key: Vec<usize> = (0..n).collect();
    by_key.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
    for &i in &by_key {
        if test_stems[i].is_none() && !files[i].binary {
            subjects
                .entry((files[i].repo, rules::subject_stem(&files[i].path)))
                .or_insert(keys[i].as_str());
        }
    }

    let tested: Vec<Option<&str>> = (0..n)
        .map(|i| {
            test_stems[i]
                .as_deref()
                .and_then(|s| subjects.get(&(files[i].repo, s)).copied())
                .filter(|p| *p != keys[i])
        })
        .collect();
    let mut subject_tests: BTreeMap<&str, &str> = BTreeMap::new();
    for &i in &by_key {
        if let Some(p) = tested[i] {
            subject_tests.entry(p).or_insert(keys[i].as_str());
        }
    }

    let mut items: Vec<Item> = Vec::with_capacity(n);
    for i in 0..n {
        let (f, key) = (&files[i], keys[i].as_str());
        let pair = tested[i].or_else(|| subject_tests.get(key).copied());
        let mut d = decide(f, key, input.similarity.get(i).copied().flatten(), input, test_stems[i].is_some());
        if d.group == Group::Tests
            && let Some(p) = tested[i]
        {
            d.reason.text = format!("test (pairs {})", rules::file_name(p));
        }
        let rated = input.rated.and_then(|r| r.get(key));
        let mut why = None;
        if let Some(r) = rated {
            // The agent can raise a rule's risk, not lower it.
            let floor = d.risk.filter(|_| d.group.is_change());
            let risk = floor.max(r.risk);
            let agent = Group::from_risk(risk);
            if d.group.is_change() || agent == Group::Must {
                d.group = agent;
                if floor.is_none() || r.risk > floor {
                    d.risk = risk;
                    let text = match r.risk {
                        Some(risk) => format!("rated {} (overview)", format!("{risk:?}").to_ascii_lowercase()),
                        None => "cited (overview)".to_owned(),
                    };
                    d.reason = reason(agent, false, text);
                }
            }
            why = Some(r.why.clone()).filter(|w| !w.is_empty());
        }
        if d.group == Group::Must && pair.is_none() && test_stems[i].is_none() {
            d.notes.push("no test touched".into());
        }
        if why.is_none() && !d.notes.is_empty() {
            why = Some(d.notes.join("; "));
        }
        d.reason.group = d.group;
        items.push(Item {
            key: key.to_owned(),
            group: d.group,
            reason: d.reason,
            risk: d.risk,
            why,
            real_line: d.real_line,
            pair: pair.map(str::to_owned),
            moved_from: d.moved_from,
        });
    }
    let index: BTreeMap<&str, usize> = (0..n).map(|i| (keys[i].as_str(), i)).collect();
    let line_count = |key: &str| index.get(key).map_or(0, |&i| u64::from(files[i].additions + files[i].deletions));
    let rank = |g: Group| Group::ALL.iter().position(|x| *x == g).unwrap_or(usize::MAX);
    let cited = |key: &str| input.rated.and_then(|r| r.get(key)).map_or(usize::MAX, |r| r.order);
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|&a, &b| {
        let (x, y) = (&items[a], &items[b]);
        let head = |it: &Item| (rank(it.group), if it.group.is_change() { cited(&it.key) } else { 0 });
        head(x).cmp(&head(y)).then_with(|| x.key.cmp(&y.key))
    });
    let place: BTreeMap<String, usize> = order.iter().enumerate().map(|(p, &i)| (items[i].key.clone(), p)).collect();
    order.sort_by(|&a, &b| {
        let (x, y) = (&items[a], &items[b]);
        let at = |it: &Item| {
            let beside = match it.group {
                Group::Tests => it.pair.as_deref().and_then(|p| place.get(p)).copied().unwrap_or(usize::MAX),
                _ => place.get(&it.key).copied().unwrap_or(usize::MAX),
            };
            (rank(it.group), beside)
        };
        at(x).cmp(&at(y)).then_with(|| x.key.cmp(&y.key))
    });
    let mut slots: Vec<Option<Item>> = items.into_iter().map(Some).collect();
    let items: Vec<Item> = order.into_iter().filter_map(|i| slots[i].take()).collect();

    let mut groups: Vec<GroupCount> = Vec::new();
    for g in Group::ALL {
        let mine: Vec<&Item> = items.iter().filter(|i| i.group == g).collect();
        if let Some(first) = mine.first() {
            groups.push(GroupCount {
                group: g,
                count: mine.len(),
                lines: mine.iter().map(|i| line_count(&i.key)).sum(),
                trusted: mine.iter().all(|i| i.reason.trusted),
                reason: first.reason.text.clone(),
            });
        }
    }

    let mut facts = Facts {
        files: n,
        lines: (0..n).map(|i| u64::from(files[i].additions + files[i].deletions)).sum(),
        real: items.iter().filter(|i| i.group.is_change()).count(),
        uncited: input
            .rated
            .map(|r| items.iter().filter(|i| i.group.is_change() && !r.contains_key(&i.key)).count()),
        ..Facts::default()
    };
    for i in 0..n {
        let f = &files[i];
        let is_test = test_stems[i].is_some();
        if is_test {
            facts.tests += 1;
            facts.tests_added += usize::from(f.status == FileStatus::Added);
            facts.tests_removed += usize::from(f.status == FileStatus::Deleted);
        }
        facts.migrations += usize::from(rules::migration(&f.path));
        facts.renames += usize::from(f.status == FileStatus::Renamed);
        facts.binaries += usize::from(f.binary);
        if (is_test || f.path.ends_with(".rs")) && f.lines.len() <= HUGE_LINES {
            for (j, l) in f.lines.iter().enumerate() {
                if l.kind == LineKind::Added
                    && let Some(m) = rules::test_off_in(&f.path, f.line_text(j as u32))
                {
                    facts.tests_off.push(TestOff {
                        key: keys[i].clone(),
                        line: l.new,
                        marker: m.to_owned(),
                    });
                }
            }
        }
    }
    facts.tests_off.sort_by(|a, b| (&a.key, a.line).cmp(&(&b.key, b.line)));

    let grouped_lines: u64 = groups
        .iter()
        .filter(|g| matches!(g.group, Group::Generated | Group::Lockfile | Group::Huge))
        .map(|g| g.lines)
        .sum();
    let shape = if facts.files < 10 && facts.lines < 200 {
        Shape::Tiny
    } else if input.round >= 2 {
        Shape::Iterative
    } else if grouped_lines * 2 > facts.lines {
        Shape::GeneratedHuge
    } else if facts.files >= 100 && facts.files - facts.real > facts.real {
        Shape::SparseHuge
    } else {
        Shape::Dense
    };

    Triage {
        shape,
        groups,
        items,
        facts,
    }
}
