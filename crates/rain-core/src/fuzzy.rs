//! The fuzzy matcher the picker, the palette and fuzzy find share.
//!
//! Alignment, scores and matched positions come from `neo_frizbee`, the Smith-Waterman
//! matcher the fff file finder uses (MIT, Copyright (c) 2024 Liam Dyer; see `LICENSE-frizbee`
//! beside this crate's manifest). The layer over it follows fff's (`fff-core/src/score.rs`
//! and `grep/fuzzy_grep.rs`, MIT, Copyright (c) 2025 Dmitriy Kovalenko; see `LICENSE-fff`
//! beside this crate's manifest): each whitespace-separated term must match, in any order; a
//! match in the file name earns a bonus over one in its folders; a line of code fits only when
//! a term's letters lie close together.

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};

use neo_frizbee::{CaseMatching, Config, Scoring};

/// Bytes of a text and characters of a term that are looked at; the rest is ignored.
pub const MAX_TEXT: usize = 1024;
const MAX_TERM: usize = 128;

/// What a matched letter scores before bonuses and gaps.
const LETTER: i32 = 12;
/// A term counts when it earns this share of what its letters in a row would; under it the
/// letters are scattered. A term forgiven a typo has to fit closer.
const FLOOR_PERCENT: i32 = 66;
const FLOOR_PERCENT_TYPOS: i32 = 75;
/// fff's file name bonus: this share of the path score, capped, scaled by how well the name
/// alone matches; and its share for a term that is the whole name.
const NAME_SHARE: i32 = 6;
const NAME_CAP: i32 = 30;
const WHOLE_NAME_PERCENT: i32 = 40;
/// A capital after a lowercase letter starts a word. fff gives this only to queries with a
/// capital; frizbee's own default keeps camelCase initials findable for the others.
const CAMEL: u16 = 4;
/// fff's bonuses for a query with a capital: capitals, and letters in the case typed.
const CAMEL_CASED: u16 = 8;
const SAME_CASE: u16 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// Some term is not in the text at all.
    Miss,
    /// Every term is there, but one only as scattered letters.
    Weak,
    /// Higher is better.
    Hit(i32),
}

impl Fit {
    pub fn score(self) -> Option<i32> {
        match self {
            Fit::Hit(s) => Some(s),
            _ => None,
        }
    }
}

struct Term {
    text: String,
    chars: usize,
    /// Typed with a capital, which then prefers capitals and the case typed.
    cased: bool,
    /// Typed with a `/`: a path, which the file name alone can't match.
    path: bool,
    /// Letters of the term a text may lack.
    typos: u16,
}

pub struct Query {
    terms: Vec<Term>,
    /// Tells a matcher its compiled terms are another query's.
    stamp: u64,
}

fn stamp() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl Query {
    /// The query as typed: every letter of every term must be in a text, in order.
    pub fn new(text: &str) -> Query {
        let terms = text
            .split_whitespace()
            .map(|t| {
                let text: String = t.chars().take(MAX_TERM).collect();
                Term {
                    chars: text.chars().count(),
                    cased: text.chars().any(char::is_uppercase),
                    path: text.contains(['/', '\\']),
                    typos: 0,
                    text,
                }
            })
            .collect();
        Query { terms, stamp: stamp() }
    }

    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// This query forgiving a mistyped letter in a term of four letters or more, two from
    /// eight: for when the query as typed finds nothing. `None` when every term is shorter.
    /// (fff always forgives at least two, which floods a list of a few hundred.)
    pub fn loosened(&self) -> Option<Query> {
        let terms: Vec<Term> = self
            .terms
            .iter()
            .map(|t| Term {
                text: t.text.clone(),
                typos: (t.chars / 4).min(2) as u16,
                ..*t
            })
            .collect();
        terms.iter().any(|t| t.typos > 0).then(|| Query { terms, stamp: stamp() })
    }
}

/// Scores texts against a [`Query`]. The query's terms are compiled on first use and kept, so
/// a pass over many texts with one query allocates nothing more.
#[derive(Default)]
pub struct Matcher {
    lines: bool,
    /// The query the compiled terms are for.
    compiled_for: u64,
    compiled: Vec<Compiled>,
}

/// A term compiled for frizbee, twice for files: frizbee's bonus for a text's first letter
/// suits a name or a label, but the first letter of a path is only a folder's.
struct Compiled {
    name: neo_frizbee::Matcher,
    path: Option<neo_frizbee::Matcher>,
}

impl Compiled {
    /// The matcher for the whole of a text whose name starts at `name_at`.
    fn whole(&mut self, name_at: usize) -> &mut neo_frizbee::Matcher {
        match &mut self.path {
            Some(path) if name_at > 0 => path,
            _ => &mut self.name,
        }
    }
}

impl Matcher {
    /// A matcher for lines of code or prose, by fff's fuzzy-grep rules: a term's matched
    /// letters must lie close together (within three times its length, mostly adjacent, in
    /// few pieces), so a line doesn't match by letters picked across it. A term of six
    /// letters or more may have one mistyped, of twelve or more two.
    pub fn in_words() -> Matcher {
        Matcher {
            lines: true,
            ..Matcher::default()
        }
    }

    /// How well `text` fits `query`; the name is `text[name_at..]`.
    pub fn fit(&mut self, query: &Query, text: &str, name_at: usize) -> Fit {
        if query.terms.is_empty() {
            return Fit::Hit(0);
        }
        self.compile(query);
        let text = cut(text);
        let mut total = 0;
        let mut weak = false;
        for (t, m) in query.terms.iter().zip(&mut self.compiled) {
            let scored = if self.lines {
                line_term(&mut m.name, t, text).map(|(score, ok, _)| (score, ok))
            } else {
                file_term(m, t, text, name_at)
            };
            let Some((score, ok)) = scored else {
                return Fit::Miss;
            };
            total += score;
            weak |= !ok;
        }
        if weak { Fit::Weak } else { Fit::Hit(total) }
    }

    /// [`Matcher::fit`], and on a hit the byte ranges of `text` the query's letters matched, in
    /// order and merged, into `out`. Costlier: for the rows that are drawn.
    pub fn marks(&mut self, query: &Query, text: &str, name_at: usize, out: &mut Vec<Range<usize>>) -> Fit {
        out.clear();
        let fit = self.fit(query, text, name_at);
        if fit.score().is_none() || query.terms.is_empty() {
            return fit;
        }
        let text = cut(text);
        let mut at: Vec<u32> = Vec::new();
        for (t, m) in query.terms.iter().zip(&mut self.compiled) {
            if self.lines {
                at.extend(line_term(&mut m.name, t, text).map(|(.., at)| at).unwrap_or_default());
            } else {
                at.extend(file_term_marks(m, t, text, name_at));
            }
        }
        ranges(text, at, out);
        fit
    }

    fn compile(&mut self, query: &Query) {
        if self.compiled_for == query.stamp {
            return;
        }
        let lines = self.lines;
        self.compiled = query
            .terms
            .iter()
            .map(|t| {
                let name = config(t, lines);
                let mut path = name;
                path.scoring.prefix_bonus = path.scoring.delimiter_bonus;
                Compiled {
                    name: neo_frizbee::Matcher::new(t.text.as_str(), &name),
                    path: (!lines).then(|| neo_frizbee::Matcher::new(t.text.as_str(), &path)),
                }
            })
            .collect();
        self.compiled_for = query.stamp;
    }
}

/// The frizbee settings for a term. Case never excludes a text: a capital typed only earns.
fn config(t: &Term, lines: bool) -> Config {
    let scoring = if lines {
        Scoring {
            exact_match_bonus: 100,
            prefix_bonus: 0,
            capitalization_bonus: if t.cased { CAMEL } else { 0 },
            ..Scoring::default()
        }
    } else {
        Scoring {
            capitalization_bonus: if t.cased { CAMEL_CASED } else { CAMEL },
            matching_case_bonus: if t.cased { SAME_CASE } else { 0 },
            ..Scoring::default()
        }
    };
    Config {
        max_typos: Some(if lines { line_typos(t) } else { t.typos }),
        casing: CaseMatching::Ignore,
        scoring,
        ..Config::default()
    }
}

fn line_typos(t: &Term) -> u16 {
    (t.chars / 6).min(2) as u16
}

/// `text` up to `MAX_TEXT` bytes, ending on a character.
fn cut(text: &str) -> &str {
    let mut end = text.len().min(MAX_TEXT);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The name in `text`, when it has folders before it and the term isn't a path itself.
fn name_of<'a>(t: &Term, text: &'a str, name_at: usize) -> Option<&'a str> {
    (name_at > 0 && !t.path)
        .then(|| text.get(name_at..))
        .flatten()
        .filter(|n| !n.is_empty())
}

/// Whether the term is the whole name, or the name without its extension.
fn is_whole(t: &Term, name: &str) -> bool {
    let stem = name.rfind('.').map_or(name, |dot| &name[..dot]);
    name.eq_ignore_ascii_case(&t.text) || stem.eq_ignore_ascii_case(&t.text)
}

fn clears_floor(t: &Term, score: i32) -> bool {
    let letters = t.chars.saturating_sub(t.typos as usize).max(1) as i32;
    let floor = if t.typos > 0 { FLOOR_PERCENT_TYPOS } else { FLOOR_PERCENT };
    score * 100 >= letters * LETTER * floor
}

/// A term against a path or label: its score with the name bonus, and whether it clears the
/// floor. `None` when the term isn't in the text.
fn file_term(m: &mut Compiled, t: &Term, text: &str, name_at: usize) -> Option<(i32, bool)> {
    let path = m.whole(name_at).match_one(text, 0)?;
    let base = i32::from(path.score);
    let Some(name) = name_of(t, text, name_at) else {
        let whole = name_at == 0 && is_whole(t, text);
        let bonus = if whole { base * WHOLE_NAME_PERCENT / 100 } else { 0 };
        return Some((base + bonus, clears_floor(t, base)));
    };
    let starts = (usize::from(path.end_col) + 1).saturating_sub(t.text.len());
    let alone = if starts >= name_at { None } else { m.name.match_one(name, 0) };
    let most = (base / NAME_SHARE).min(NAME_CAP);
    let bonus = if is_whole(t, name) {
        base * WHOLE_NAME_PERCENT / 100
    } else if starts >= name_at {
        most
    } else if let Some(alone) = alone {
        let perfect = t.text.len() as i32 * 16;
        most * i32::from(alone.score).min(perfect) / perfect
    } else {
        0
    };
    let best = base.max(alone.map_or(0, |a| i32::from(a.score)));
    Some((base + bonus, clears_floor(t, best)))
}

/// The bytes of `text` a term matched: in the name when the name alone fits it, else wherever
/// the path's best alignment falls.
fn file_term_marks(m: &mut Compiled, t: &Term, text: &str, name_at: usize) -> Vec<u32> {
    if let Some(name) = name_of(t, text, name_at)
        && let Some(found) = m.name.match_one_indices(name, 0)
        && clears_floor(t, i32::from(found.score))
    {
        return found.indices.iter().map(|i| i + name_at as u32).collect();
    }
    let whole = m.whole(name_at).match_one_indices(text, 0);
    whole.map(|found| found.indices).unwrap_or_default()
}

/// A term against a line: its score, whether fff's fuzzy-grep checks pass (score, how many
/// letters matched, their span, density and pieces), and the bytes matched. `None` when the
/// term isn't in the line.
fn line_term(m: &mut neo_frizbee::Matcher, t: &Term, text: &str) -> Option<(i32, bool, Vec<u32>)> {
    let mut found = m.match_one_indices(text, 0)?;
    found.indices.sort_unstable();
    let (at, n) = (&found.indices, t.text.len());
    let typos = usize::from(line_typos(t));
    let dense = match (at.first(), at.last()) {
        (Some(&first), Some(&last)) => {
            let span = (last - first + 1) as usize;
            let pieces = at.windows(2).filter(|w| w[1] != w[0] + 1).count();
            let density = if at.len() >= n { 45 } else { 65 };
            span <= n * 3 && at.len() * 100 / span >= density && pieces <= (n / 3).max(2)
        }
        _ => false,
    };
    let ok = usize::from(found.score) * 2 >= n * 16 && at.len() >= n.saturating_sub(typos).max(1) && dense;
    Some((i32::from(found.score), ok, found.indices))
}

/// Byte indices into `text` as merged ranges that start and end on characters.
fn ranges(text: &str, mut at: Vec<u32>, out: &mut Vec<Range<usize>>) {
    at.sort_unstable();
    at.dedup();
    for i in at {
        let (mut start, mut end) = (i as usize, i as usize + 1);
        if end > text.len() {
            break;
        }
        while !text.is_char_boundary(start) {
            start -= 1;
        }
        while !text.is_char_boundary(end) {
            end += 1;
        }
        match out.last_mut() {
            Some(r) if r.end >= start => r.end = r.end.max(end),
            _ => out.push(start..end),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILES: [&str; 17] = [
        "server/src/Acme.Shop.Api/Controllers/VouchersController.cs",
        "server/src/Acme.Shop/Services/VoucherService.cs",
        "server/src/Acme.Shop/Services/InvoiceService.cs",
        "server/src/Acme.Shop/Pricing/DiscountCalculator.cs",
        "server/src/Acme.Shop/Domain/Account.cs",
        "server/src/Acme.Shop/Domain/Voucher.cs",
        "server/tests/Acme.Shop.Tests/VoucherServiceTests.cs",
        "server/tests/Acme.Shop.Tests/InvoiceServiceTests.cs",
        "web/src/app/vouchers/voucher.service.ts",
        "web/src/app/vouchers/voucher-list.component.ts",
        "web/src/app/vouchers/voucher-list.component.html",
        "web/src/app/icons/index.ts",
        "web/src/app/invoices/index.ts",
        "web/src/app/invoices/IndexedInvoiceTable.tsx",
        "web/src/app/orders/latest-orders.component.ts",
        "docs/contributing.md",
        "README.md",
    ];

    fn name_at(path: &str) -> usize {
        path.rfind('/').map_or(0, |i| i + 1)
    }

    fn fit(query: &str, path: &str) -> Fit {
        Matcher::default().fit(&Query::new(query), path, name_at(path))
    }

    fn score(query: &str, path: &str) -> i32 {
        fit(query, path).score().unwrap_or_else(|| panic!("{query:?} fits {path}"))
    }

    fn ranked_by(q: &Query) -> Vec<(&'static str, i32)> {
        let mut m = Matcher::default();
        let mut hits: Vec<(i32, usize)> = FILES
            .iter()
            .enumerate()
            .filter_map(|(i, p)| m.fit(q, p, name_at(p)).score().map(|s| (s, i)))
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        hits.into_iter().map(|(s, i)| (&FILES[i][name_at(FILES[i])..], s)).collect()
    }

    /// The file names `query` lists, best first, list order on ties: as the picker sorts.
    fn ranked(query: &str) -> Vec<&'static str> {
        ranked_by(&Query::new(query)).into_iter().map(|(n, _)| n).collect()
    }

    fn place(rows: &[&str], name: &str) -> usize {
        rows.iter().position(|r| *r == name).unwrap_or_else(|| panic!("{name} in {rows:?}"))
    }

    fn marked(query: &str, path: &str) -> String {
        let mut out = Vec::new();
        Matcher::default().marks(&Query::new(query), path, name_at(path), &mut out);
        out.iter().map(|r| &path[r.clone()]).collect::<Vec<_>>().join("|")
    }

    #[test]
    fn the_best_alignment_wins_not_the_first() {
        assert_eq!(ranked("vsc")[0], "VouchersController.cs");
        let rows = ranked("con");
        assert_eq!(&rows[..2], ["contributing.md", "VouchersController.cs"], "{rows:?}");
        for late in ["Account.cs", "DiscountCalculator.cs"] {
            assert!(place(&rows, late) > 1, "{rows:?}");
        }
        assert_eq!(marked("con", FILES[0]), "Con", "the run in Controller, not the c of Vouchers");
        assert_eq!(marked("cs", FILES[4]), "cs", "the extension, not the cc of Account");
        let rows = ranked("component");
        assert_eq!(
            &rows[..3],
            [
                "voucher-list.component.ts",
                "voucher-list.component.html",
                "latest-orders.component.ts"
            ],
            "equal runs keep list order"
        );
    }

    #[test]
    fn the_file_name_outweighs_the_folder() {
        let rows = ranked("serv");
        let last_service = [
            "VoucherService.cs",
            "InvoiceService.cs",
            "VoucherServiceTests.cs",
            "InvoiceServiceTests.cs",
        ]
        .iter()
        .map(|n| place(&rows, n))
        .max()
        .unwrap();
        for folder_only in ["Account.cs", "Voucher.cs"] {
            assert!(place(&rows, folder_only) > last_service, "{rows:?}");
        }
        assert_eq!(marked("serv", FILES[1]), "Serv", "the name's letters are marked, not the folder's");
        assert_eq!(marked("serv", FILES[4]), "serv", "the folder's when the name has none");
        let rows = ranked("cs");
        let md = rows.iter().position(|r| *r == "contributing.md").unwrap_or(usize::MAX);
        for cs in FILES.iter().filter(|f| f.ends_with(".cs")) {
            assert!(place(&rows, &cs[name_at(cs)..]) < md, "{rows:?}");
        }
        assert_eq!(ranked("api/vou")[0], "VouchersController.cs", "a path query still finds its file");
        assert_eq!(ranked("vouchers/ser")[0], "voucher.service.ts");
    }

    #[test]
    fn terms_match_in_any_order_and_all_must() {
        for q in ["html list", "list html"] {
            assert_eq!(ranked(q)[0], "voucher-list.component.html", "{q}");
        }
        assert_eq!(ranked("controller api"), ["VouchersController.cs"]);
        assert_eq!(ranked("index web")[..2], ["index.ts", "index.ts"]);
        assert!(ranked("voucher zz").is_empty());
        assert_eq!(fit("", "x"), Fit::Hit(0));
        assert_eq!(fit("  ", "x"), Fit::Hit(0));
    }

    #[test]
    fn scattered_letters_are_not_listed() {
        assert_eq!(ranked("readme"), ["README.md"]);
        assert_eq!(fit("readme", FILES[14]), Fit::Weak, "every letter is there, none together");
        assert_eq!(fit("readme", FILES[5]), Fit::Weak, "nor by folder initials and a stray letter");
        assert_eq!(fit("zz", FILES[1]), Fit::Miss);
        let rows = ranked("acc");
        assert_eq!(rows[0], "Account.cs");
        assert!(!rows.contains(&"VoucherService.cs"), "{rows:?}");
        assert!(
            fit("vc", "InternationalizationVouchersController.cs").score().is_some(),
            "initials far apart still fit"
        );
        let initials = "web/VeryLongWordsSeparatedCompletelyEverywhere.ts";
        assert!(fit("lwsce", initials).score().is_some(), "initials fit however long the words");
        assert!(fit("v", "web/server.ts").score().is_some(), "one letter fits wherever it is");
        let stray = "server/src/Services/AccountService.cs";
        assert!(score("vou", FILES[5]) > score("vou", stray), "a run and a stray letter rank last");
    }

    #[test]
    fn a_whole_name_comes_first() {
        let rows = ranked("voucher");
        assert_eq!(rows[0], "Voucher.cs");
        assert!(score("voucher", FILES[5]) > score("voucher", FILES[1]));
        let rows = ranked("index");
        assert_eq!(rows, ["index.ts", "index.ts", "IndexedInvoiceTable.tsx"]);
    }

    #[test]
    fn capitals_in_the_query_prefer_capitals_and_never_filter() {
        let rows = ranked("VS");
        assert!(place(&rows, "VoucherService.cs") < place(&rows, "voucher.service.ts"), "{rows:?}");
        assert!(score("README", "README.md") > score("README", "readme.md"));
        assert_eq!(score("readme", "README.md"), score("readme", "readme.md"));
        let cased = |name| score("Readme", name);
        assert!(cased("Readme.md") > cased("readme.md") && cased("readme.md") > cased("README.md"));
    }

    #[test]
    fn marks_are_the_matched_letters_as_byte_ranges() {
        assert_eq!(marked("vsc", "VouchersController.cs"), "V|sC");
        assert_eq!(marked("list html", FILES[10]), "list|html");
        assert_eq!(marked("api controller", FILES[0]), "Api|Controller");
        assert_eq!(marked("né", "données/née.rs"), "né");
        assert_eq!(marked("zz", FILES[0]), "");
        assert_eq!(marked("readme", FILES[14]), "", "a weak fit marks nothing");
        let mut out = vec![0..1, 2..3];
        assert_eq!(Matcher::default().marks(&Query::new(""), "x", 0, &mut out), Fit::Hit(0));
        assert!(out.is_empty());
    }

    #[test]
    fn a_typo_is_forgiven_only_when_asked_and_only_in_longer_terms() {
        assert!(ranked("redame").is_empty(), "as typed, nothing");
        let loose = Query::new("redame").loosened().expect("six letters may have one wrong");
        let rows: Vec<&str> = ranked_by(&loose).into_iter().map(|(n, _)| n).collect();
        assert_eq!(rows, ["README.md"], "no scattered rows come with it");
        let loose = Query::new("contorller").loosened().unwrap();
        assert_eq!(ranked_by(&loose)[0].0, "VouchersController.cs");
        assert!(Query::new("vsc").loosened().is_none(), "three letters must all be there");
        assert!(Query::new("ab cd efg").loosened().is_none());
        assert!(Query::new("ab redame").loosened().is_some());
        assert!(Matcher::default().fit(&loose, "x/Controller.cs", 2).score().is_some());
    }

    fn line_marks(q: &str, line: &str) -> (Fit, String) {
        let mut out = Vec::new();
        let fit = Matcher::in_words().marks(&Query::new(q), line, 0, &mut out);
        (fit, out.iter().map(|r| &line[r.clone()]).collect::<Vec<_>>().join("|"))
    }

    #[test]
    fn in_a_line_a_terms_letters_must_lie_close_together() {
        let hit = |q, line| {
            let (fit, marks) = line_marks(q, line);
            assert!(fit.score().is_some(), "{q} in {line}: {fit:?}");
            marks
        };
        let spread = "    var service = context.Vouchers;";
        assert!(Matcher::default().fit(&Query::new("vsc"), spread, 0).score().is_some());
        assert_eq!(line_marks("vsc", spread).0, Fit::Weak, "letters from three words");
        let line = "return new VouchersController(service, clock);";
        assert_eq!(line_marks("vsc", line).0, Fit::Weak, "initials alone are too thin for a line");
        assert_eq!(line_marks("eun", line).0, Fit::Weak, "scattered inside a word");
        assert_eq!(line_marks("qzj", line).0, Fit::Miss);
        assert_eq!(hit("clock vouch", line), "Vouch|clock", "words apart, any order");
        assert_eq!(hit("vouchcontr", line), "Vouch|Contr");
        assert_eq!(hit("serviceclock", line), "service|clock", "neighbouring words may be one term");
        assert_eq!(hit("new", line), "new");
        assert_eq!(hit("SC", line), "sC", "a capital typed still finds either case");
        assert_eq!(hit("contorller", line), "Contr|ller", "ten letters, one mistyped");
        assert!(line_marks("cnotr", line).0.score().is_none(), "a short term gets no typo");
    }

    #[test]
    fn long_texts_and_terms_are_cut_not_scanned() {
        let long = format!("{}needle", "x".repeat(MAX_TEXT));
        assert_eq!(fit("needle", &long), Fit::Miss, "past what is looked at");
        let near = format!("{}needle", "x".repeat(MAX_TEXT - 6));
        assert!(fit("needle", &near).score().is_some());
        let wide = format!("{}é", "x".repeat(MAX_TEXT - 1));
        assert_eq!(fit("é", &wide), Fit::Miss, "a character cut in half is left out");
        let term = "a".repeat(MAX_TERM + 50);
        assert!(
            fit(&term, &"a".repeat(MAX_TERM)).score().is_some(),
            "a term is cut to its first letters"
        );
    }
}
