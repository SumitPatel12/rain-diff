//! The Brief written as one Markdown document, converted to the same `rain-brief/v1` [`BriefDoc`].
//! `# Title`, then `##` sections named as in the reader; `{#id}` on a heading or at a list item's
//! end keeps an id stable across rewrites; code spans that read as paths become cites.

use std::fmt;
use std::ops::Range;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::brief::{BRIEF_FORMAT, BriefDoc, Cite, How, Invariant, NotCovered, RepoSha, Section, TourStep};

/// Why a Markdown Brief was refused, at a 1-based line of the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BriefMdError {
    pub line: usize,
    pub msg: String,
}

impl fmt::Display for BriefMdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.msg)
    }
}

impl std::error::Error for BriefMdError {}

const SECTION_NAMES: &str = "Intent, How it works, Invariants, Not changed, Not covered, Verify by hand, Tour";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Intent,
    How,
    Invariants,
    NotChanged,
    NotCovered,
    Verify,
    Tour,
}

fn kind_of(name: &str) -> Option<Kind> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "intent" => Kind::Intent,
        "how" | "how it works" => Kind::How,
        "invariants" => Kind::Invariants,
        "not changed" => Kind::NotChanged,
        "not covered" => Kind::NotCovered,
        "verify" | "verify by hand" => Kind::Verify,
        "tour" => Kind::Tour,
        _ => return None,
    })
}

struct Heading {
    level: u8,
    title: String,
    id: Option<String>,
    /// From the `#` to the start of the line after the heading.
    span: Range<usize>,
}

/// A slice of the source with where it starts, so errors inside it name the document's line.
#[derive(Clone, Copy)]
struct Part<'a> {
    text: &'a str,
    at: usize,
}

struct Doc<'a> {
    src: &'a str,
}

impl Doc<'_> {
    fn line(&self, offset: usize) -> usize {
        self.src[..offset.min(self.src.len())].bytes().filter(|&b| b == b'\n').count() + 1
    }

    fn err(&self, offset: usize, msg: impl Into<String>) -> BriefMdError {
        BriefMdError {
            line: self.line(offset),
            msg: msg.into(),
        }
    }
}

fn options() -> Options {
    Options::ENABLE_HEADING_ATTRIBUTES
}

fn level(l: HeadingLevel) -> u8 {
    match l {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn line_end(src: &str, at: usize) -> usize {
    if at > 0 && src.as_bytes()[at - 1] == b'\n' {
        return at;
    }
    src[at..].find('\n').map_or(src.len(), |i| at + i + 1)
}

/// The document's own headings (not ones inside lists or quotes).
fn headings(src: &str) -> Vec<Heading> {
    let mut out = Vec::new();
    let mut nested = 0usize;
    let mut open: Option<Heading> = None;
    for (e, r) in Parser::new_ext(src, options()).into_offset_iter() {
        match e {
            Event::Start(Tag::Item | Tag::BlockQuote(_) | Tag::FootnoteDefinition(_)) => nested += 1,
            Event::End(TagEnd::Item | TagEnd::BlockQuote(_) | TagEnd::FootnoteDefinition) => nested -= 1,
            Event::Start(Tag::Heading { level: l, id, .. }) if nested == 0 => {
                open = Some(Heading {
                    level: level(l),
                    title: String::new(),
                    id: id.map(|i| i.to_string()),
                    span: r.start..line_end(src, r.end),
                });
            }
            Event::End(TagEnd::Heading(_)) => out.extend(open.take()),
            Event::Text(t) => {
                if let Some(h) = &mut open {
                    h.title.push_str(&t);
                }
            }
            Event::Code(c) => {
                if let Some(h) = &mut open {
                    h.title.push('`');
                    h.title.push_str(&c);
                    h.title.push('`');
                }
            }
            _ => {}
        }
    }
    for h in &mut out {
        h.title = h.title.trim().to_owned();
    }
    out
}

/// A code span as a cite: a path with a `.` and no space, then optional `:a` or `:a-b`.
pub fn cite_of(span: &str) -> Option<Cite> {
    let s = span.trim();
    let (path, lines) = match s.rsplit_once(':') {
        Some((p, l)) if !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit() || b == b'-') => {
            let (a, b) = l.split_once('-').unwrap_or((l, l));
            let (a, b) = (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?);
            (p, Some((a.min(b), a.max(b))))
        }
        _ => (s, None),
    };
    let pathlike =
        path.contains('.') && !path.starts_with('.') && !path.chars().any(|c| c.is_whitespace() || "()[]{}<>\"'`,;:=!?|&$".contains(c));
    pathlike.then(|| Cite {
        path: path.trim_start_matches("./").to_owned(),
        lines,
        ..Cite::default()
    })
}

fn push_cite(out: &mut Vec<Cite>, c: Cite) {
    if !out.contains(&c) {
        out.push(c);
    }
}

/// Inline code spans of `md` with their byte ranges in `md`.
fn code_spans(md: &str) -> Vec<(String, Range<usize>)> {
    Parser::new_ext(md, options())
        .into_offset_iter()
        .filter_map(|(e, r)| match e {
            Event::Code(c) => Some((c.to_string(), r)),
            _ => None,
        })
        .collect()
}

fn cites_in(md: &str) -> Vec<Cite> {
    let mut out = Vec::new();
    for (c, _) in code_spans(md) {
        if let Some(c) = cite_of(&c) {
            push_cite(&mut out, c);
        }
    }
    out
}

/// The top-level list items of `p`, each as its content with the marker and indent removed; any
/// other text is an error naming `what`.
fn items(d: &Doc, p: Part, what: &str) -> Result<Vec<(String, usize)>, BriefMdError> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut covered: Vec<Range<usize>> = Vec::new();
    for (e, r) in Parser::new_ext(p.text, options()).into_offset_iter() {
        match e {
            Event::Start(Tag::List(_)) => {
                if depth == 0 {
                    covered.push(r.clone());
                }
                depth += 1;
            }
            Event::End(TagEnd::List(_)) => depth -= 1,
            Event::Start(Tag::Item) if depth == 1 => out.push((item_content(&p.text[r.clone()]), p.at + r.start)),
            _ => {}
        }
    }
    let mut at = 0;
    for r in covered.iter().chain(std::iter::once(&(p.text.len()..p.text.len()))) {
        if let Some(i) = p.text[at..r.start].find(|c: char| !c.is_whitespace()) {
            return Err(d.err(p.at + at + i, format!("{what} are list items (`- ...`)")));
        }
        at = r.end;
    }
    Ok(out)
}

/// A list item's Markdown without its `- ` / `1. ` marker, continuation lines dedented.
fn item_content(raw: &str) -> String {
    let first = raw.lines().next().unwrap_or("");
    let lead = first.len() - first.trim_start().len();
    let rest = &first[lead..];
    let marker = rest
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '-' | '*' | '+' | '.' | ')')))
        .unwrap_or(rest.len());
    let body = &rest[marker..];
    let col = lead + marker + (body.len() - body.trim_start().len());
    let mut out = body.trim_start().to_owned();
    for l in raw.lines().skip(1) {
        out.push('\n');
        let indent = l.len() - l.trim_start().len();
        out.push_str(&l[indent.min(col)..]);
    }
    out.trim().to_owned()
}

/// Removes a `{#id}` that ends a line of `text`; the id and the rest.
fn take_id(text: &str) -> (Option<String>, String) {
    let mut id = None;
    let lines: Vec<String> = text
        .lines()
        .map(|l| {
            let t = l.trim_end();
            if id.is_none()
                && t.ends_with('}')
                && let Some(at) = t.rfind("{#")
            {
                let v = &t[at + 2..t.len() - 1];
                if !v.is_empty() && !v.contains(char::is_whitespace) {
                    id = Some(v.to_owned());
                    return t[..at].trim_end().to_owned();
                }
            }
            l.to_owned()
        })
        .collect();
    (id, lines.join("\n").trim().to_owned())
}

fn section(text: &str, id: Option<String>) -> Option<Section> {
    let text = text.trim();
    (!text.is_empty()).then(|| Section {
        id: id.unwrap_or_default(),
        text: text.to_owned(),
        cites: cites_in(text),
    })
}

fn invariant(d: &Doc, content: &str, at: usize) -> Result<Invariant, BriefMdError> {
    let (id, content) = take_id(content);
    let marks: Vec<(usize, bool)> = ["Enforced:", "Tested:"]
        .iter()
        .enumerate()
        .flat_map(|(i, m)| content.match_indices(m).map(move |(p, _)| (p, i == 0)))
        .collect();
    let mut marks = marks;
    marks.sort();
    let text_end = marks.first().map_or(content.len(), |m| m.0);
    let mut v = Invariant {
        id: id.unwrap_or_default(),
        text: content[..text_end].trim().to_owned(),
        ..Invariant::default()
    };
    if v.text.is_empty() {
        return Err(d.err(at, "an invariant needs its statement before Enforced:/Tested:"));
    }
    for (i, &(start, enforced)) in marks.iter().enumerate() {
        let end = marks.get(i + 1).map_or(content.len(), |m| m.0);
        let label = if enforced { "Enforced:" } else { "Tested:" };
        for (span, _) in code_spans(&content[start + label.len()..end]) {
            let c = cite_of(&span).ok_or_else(|| d.err(at, format!("`{span}` after {label} is not a path")))?;
            push_cite(if enforced { &mut v.enforced } else { &mut v.tested }, c);
        }
    }
    Ok(v)
}

fn tour_step(d: &Doc, content: &str, at: usize) -> Result<TourStep, BriefMdError> {
    let (id, content) = take_id(content);
    let mut title: Option<(String, Range<usize>)> = None;
    let mut cite: Option<(String, Range<usize>)> = None;
    let mut strong: Option<(String, usize)> = None;
    for (e, r) in Parser::new_ext(&content, options()).into_offset_iter() {
        match e {
            Event::Start(Tag::Strong) if title.is_none() && strong.is_none() => strong = Some((String::new(), r.start)),
            Event::End(TagEnd::Strong) => {
                if let Some((t, s)) = strong.take() {
                    title = Some((t, s..r.end));
                }
            }
            Event::Text(t) => {
                if let Some((s, _)) = &mut strong {
                    s.push_str(&t);
                }
            }
            Event::Code(c) if strong.is_none() && cite.is_none() => cite = Some((c.to_string(), r)),
            _ => {}
        }
    }
    let (title, title_at) = title.ok_or_else(|| d.err(at, "a tour step starts with a **title**"))?;
    let (span, cite_at) = cite.ok_or_else(|| d.err(at, "a tour step needs a `path:lines` code span"))?;
    let cite = cite_of(&span).ok_or_else(|| d.err(at, format!("`{span}` in a tour step is not a path")))?;
    let mut cuts = [title_at, cite_at];
    cuts.sort_by_key(|r| r.start);
    let mut rest = String::new();
    let mut from = 0;
    for r in &cuts {
        rest.push_str(&content[from..r.start]);
        rest.push(' ');
        from = r.end;
    }
    rest.push_str(&content[from..]);
    let text = rest
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(TourStep {
        id: id.unwrap_or_default(),
        title: title.trim().to_owned(),
        text: text.trim().to_owned(),
        cite,
    })
}

fn not_covered(p: Part, id: Option<String>) -> Option<NotCovered> {
    let mut paths = Vec::new();
    let mut fences: Vec<Range<usize>> = Vec::new();
    let mut in_paths = false;
    for (e, r) in Parser::new_ext(p.text, options()).into_offset_iter() {
        match e {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) if info.trim() == "paths" => {
                in_paths = true;
                fences.push(r);
            }
            Event::End(TagEnd::CodeBlock) => in_paths = false,
            Event::Text(t) if in_paths => {
                paths.extend(t.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned));
            }
            _ => {}
        }
    }
    let mut text = String::new();
    let mut from = 0;
    for r in &fences {
        text.push_str(&p.text[from..r.start]);
        from = r.end;
    }
    text.push_str(&p.text[from..]);
    let text = text.trim().to_owned();
    (!text.is_empty() || !paths.is_empty()).then(|| NotCovered {
        id: id.unwrap_or_default(),
        text,
        paths,
    })
}

impl BriefDoc {
    /// Converts a Markdown Brief; `shas` become `writtenAgainst`. Ids not given are assigned as
    /// for JSON ([`BriefDoc::assign_ids`]).
    pub fn from_markdown(md: &str, shas: &[RepoSha]) -> Result<BriefDoc, BriefMdError> {
        let d = Doc { src: md };
        let hs = headings(md);
        let mut doc = BriefDoc {
            format: BRIEF_FORMAT.into(),
            written_against: shas.to_vec(),
            ..BriefDoc::default()
        };
        let first = hs.first().map_or(md.len(), |h| h.span.start);
        if let Some(i) = md[..first].find(|c: char| !c.is_whitespace()) {
            return Err(d.err(i, "the Brief starts with a `# Title`"));
        }
        let Some(h1) = hs.first().filter(|h| h.level == 1) else {
            return Err(d.err(first, "the Brief starts with a `# Title`"));
        };
        doc.title = h1.title.clone();
        let tops: Vec<usize> = (1..hs.len()).filter(|&i| hs[i].level <= 2).collect();
        let body_end = |i: usize| hs.iter().skip(i + 1).find(|h| h.level <= 2).map_or(md.len(), |h| h.span.start);
        let preamble_end = tops.first().map_or(md.len(), |&i| hs[i].span.start);
        if let Some(at) = md[h1.span.end..preamble_end].find(|c: char| !c.is_whitespace()) {
            return Err(d.err(h1.span.end + at, "text before the first `## section`; put it under `## Intent`"));
        }
        let mut seen: Vec<Kind> = Vec::new();
        for &i in &tops {
            let h = &hs[i];
            if h.level == 1 {
                return Err(d.err(h.span.start, "only one `# Title`"));
            }
            let kind = kind_of(&h.title).ok_or_else(|| {
                d.err(
                    h.span.start,
                    format!("unknown section `## {}`; use one of: {SECTION_NAMES}", h.title),
                )
            })?;
            if seen.contains(&kind) {
                return Err(d.err(h.span.start, format!("`## {}` appears twice", h.title)));
            }
            seen.push(kind);
            let end = body_end(i);
            let p = Part {
                text: &md[h.span.end..end],
                at: h.span.end,
            };
            let id = h.id.clone();
            match kind {
                Kind::Intent => doc.intent = section(p.text, id),
                Kind::NotChanged => doc.not_changed = section(p.text, id),
                Kind::Verify => doc.verify = section(p.text, id),
                Kind::NotCovered => doc.not_covered = not_covered(p, id),
                Kind::How => doc.how = how(&d, &hs, i, end)?,
                Kind::Invariants => {
                    for (c, at) in items(&d, p, "Invariants")? {
                        doc.invariants.push(invariant(&d, &c, at)?);
                    }
                }
                Kind::Tour => {
                    for (c, at) in items(&d, p, "Tour steps")? {
                        doc.tour.push(tour_step(&d, &c, at)?);
                    }
                }
            }
        }
        doc.assign_ids();
        Ok(doc)
    }
}

/// `### Title {#id}` blocks under the How heading `hs[at]`, up to byte `end`.
fn how(d: &Doc, hs: &[Heading], at: usize, end: usize) -> Result<Vec<How>, BriefMdError> {
    let src = d.src;
    let subs: Vec<&Heading> = hs[at + 1..]
        .iter()
        .take_while(|h| h.span.start < end)
        .filter(|h| h.level == 3)
        .collect();
    let first = subs.first().map_or(end, |h| h.span.start);
    if let Some(i) = src[hs[at].span.end..first].find(|c: char| !c.is_whitespace()) {
        return Err(d.err(hs[at].span.end + i, "How it works is `### Step` headings, each with its text"));
    }
    Ok(subs
        .iter()
        .enumerate()
        .map(|(n, h)| {
            let stop = subs.get(n + 1).map_or(end, |x| x.span.start);
            let text = src[h.span.end..stop].trim();
            How {
                id: h.id.clone().unwrap_or_default(),
                title: h.title.clone(),
                text: text.to_owned(),
                cites: cites_in(text),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = "# Vouchers at checkout
## Intent
Adds an optional voucher to account payments. `server/src/Api/Voucher2Controller.cs:20-27` takes it.
## How it works
### The API records a voucher {#how.api}
`POST /voucher` validates the amount. Cites: `server/src/Api/Voucher2Controller.cs:20-27`
## Invariants
- A voucher is never more than the account total. {#inv.cap}
  Enforced: `server/src/Api/Voucher2Controller.cs:23` Tested: `app/src/app/voucher1/voucher1.service.spec.ts`
## Not changed
## Not covered
Generated i18n files.
```paths
app/src/assets/i18n/*.json
```
## Verify by hand
## Tour
1. **The endpoint** `server/src/Api/Voucher2Controller.cs:20-27` Start where a voucher enters the system.
";

    fn sha(repo: &str, sha: &str) -> RepoSha {
        RepoSha {
            repo: repo.into(),
            sha: sha.into(),
        }
    }

    fn cite(path: &str, lines: Option<(u32, u32)>) -> Cite {
        Cite {
            path: path.into(),
            lines,
            ..Cite::default()
        }
    }

    fn err(md: &str) -> BriefMdError {
        BriefDoc::from_markdown(md, &[]).unwrap_err()
    }

    #[test]
    fn the_design_example_becomes_the_brief() {
        let ctl = "server/src/Api/Voucher2Controller.cs";
        let got = BriefDoc::from_markdown(EXAMPLE, &[sha("server", "a1b2c3d")]).unwrap();
        let want = BriefDoc {
            format: BRIEF_FORMAT.into(),
            title: "Vouchers at checkout".into(),
            written_against: vec![sha("server", "a1b2c3d")],
            intent: Some(Section {
                id: "intent".into(),
                text: format!("Adds an optional voucher to account payments. `{ctl}:20-27` takes it."),
                cites: vec![cite(ctl, Some((20, 27)))],
            }),
            how: vec![How {
                id: "how.api".into(),
                title: "The API records a voucher".into(),
                text: format!("`POST /voucher` validates the amount. Cites: `{ctl}:20-27`"),
                cites: vec![cite(ctl, Some((20, 27)))],
            }],
            invariants: vec![Invariant {
                id: "inv.cap".into(),
                text: "A voucher is never more than the account total.".into(),
                enforced: vec![cite(ctl, Some((23, 23)))],
                tested: vec![cite("app/src/app/voucher1/voucher1.service.spec.ts", None)],
            }],
            not_changed: None,
            not_covered: Some(NotCovered {
                id: "not-covered".into(),
                text: "Generated i18n files.".into(),
                paths: vec!["app/src/assets/i18n/*.json".into()],
            }),
            verify: None,
            tour: vec![TourStep {
                id: "tour.1".into(),
                title: "The endpoint".into(),
                text: "Start where a voucher enters the system.".into(),
                cite: cite(ctl, Some((20, 27))),
            }],
        };
        assert_eq!(got, want);
    }

    #[test]
    fn the_sample_written_as_markdown_keeps_its_ids() {
        let json: BriefDoc = serde_json::from_str(include_str!("../../../fixtures/brief/sample.json")).unwrap();
        let md = BriefDoc::from_markdown(include_str!("../../../fixtures/brief/sample.md"), &json.written_against).unwrap();
        let ids = |d: &BriefDoc| d.sections().iter().map(|s| s.id.to_owned()).collect::<Vec<_>>();
        assert_eq!(ids(&md), ids(&json));
        assert_eq!(
            md.tour.iter().map(|t| &t.id).collect::<Vec<_>>(),
            json.tour.iter().map(|t| &t.id).collect::<Vec<_>>()
        );
        assert_eq!(md.title, json.title);
        assert_eq!(md.written_against, json.written_against);
        assert_eq!(md.not_covered, json.not_covered);
        assert_eq!(md.invariants, json.invariants);
        for (m, j) in md.how.iter().zip(&json.how) {
            assert_eq!((&m.title, &m.text), (&j.title, &j.text));
        }
    }

    #[test]
    fn given_ids_win_and_missing_ones_follow_the_json_defaults() {
        let doc = BriefDoc::from_markdown(
            "# T\n## Intent {#why}\nBecause.\n## How\n### One\na\n### Two {#api}\nb\n### Three\nc\n",
            &[],
        )
        .unwrap();
        let ids: Vec<&str> = doc.sections().iter().map(|s| s.id).collect();
        assert_eq!(ids, ["why", "how.1", "api", "how.3"]);
    }

    #[test]
    fn section_names_ignore_case_and_take_the_short_forms() {
        let doc = BriefDoc::from_markdown("# T\n## INTENT\ni\n## how\n### a\nx\n## verify\nv\n", &[]).unwrap();
        assert_eq!(doc.intent.unwrap().text, "i");
        assert_eq!(doc.how[0].title, "a");
        assert_eq!(doc.verify.unwrap().text, "v");
    }

    #[test]
    fn path_code_spans_become_deduped_cites_and_stay_in_the_text() {
        let doc = BriefDoc::from_markdown(
            "# T\n## Not changed\nNot `a/b.rs:3`, `a/b.rs:3`, `a/b.rs`, `Foo.bar()`, `x = y.z`, `http://h.io/x`, `cargo test` or `v1`.\n",
            &[],
        )
        .unwrap();
        let s = doc.not_changed.unwrap();
        assert_eq!(s.cites, [cite("a/b.rs", Some((3, 3))), cite("a/b.rs", None)]);
        assert!(s.text.contains("`a/b.rs:3`, `a/b.rs:3`"));
    }

    #[test]
    fn cites_parse_ranges_and_refuse_what_is_not_a_path() {
        assert_eq!(cite_of("src/a.rs:20-27"), Some(cite("src/a.rs", Some((20, 27)))));
        assert_eq!(cite_of("src/a.rs:9"), Some(cite("src/a.rs", Some((9, 9)))));
        assert_eq!(cite_of("app/**/*.scss"), Some(cite("app/**/*.scss", None)));
        assert_eq!(cite_of("Cargo.toml"), Some(cite("Cargo.toml", None)));
        for not in ["POST /voucher", "a.b()", "Makefile", ".skip(", "x.y:z", "https://x.io", "a.rs:1-"] {
            assert_eq!(cite_of(not), None, "{not}");
        }
    }

    #[test]
    fn body_markdown_is_kept_as_written() {
        let doc = BriefDoc::from_markdown(
            "# T\n## Intent\nOne **bold** line.\n\n- a list\n- `src/a.rs`\n\n```rust\nlet x = `src/no.rs`;\n```\n",
            &[],
        )
        .unwrap();
        let s = doc.intent.unwrap();
        assert!(s.text.starts_with("One **bold** line.\n\n- a list") && s.text.ends_with("```"));
        assert_eq!(s.cites, [cite("src/a.rs", None)], "code blocks are not cites");
    }

    #[test]
    fn headings_inside_how_bodies_below_h3_stay_text() {
        let doc = BriefDoc::from_markdown("# T\n## How\n### A\nx\n#### Detail\ny\n### B\nz\n", &[]).unwrap();
        assert_eq!(doc.how.len(), 2);
        assert_eq!(doc.how[0].text, "x\n#### Detail\ny");
    }

    #[test]
    fn errors_name_the_line() {
        assert_eq!(
            err("# T\n## Intent\ni\n\n## Risks\nr\n"),
            BriefMdError {
                line: 5,
                msg: format!("unknown section `## Risks`; use one of: {SECTION_NAMES}"),
            }
        );
        assert_eq!(err("Hello\n# T\n").line, 1);
        assert_eq!(err("## Intent\nx\n").line, 1);
        assert_eq!(err("# T\nloose\n## Intent\nx\n").line, 2);
        assert_eq!(err("# T\n## Intent\na\n## Intent\nb\n").line, 4);
        assert_eq!(err("# T\n## Intent\na\n# Again\n").line, 4);
        assert_eq!(err("# T\n## How\nno heading\n### A\nx\n").line, 3);
        assert_eq!(err("# T\n## Invariants\n- ok\n\nprose\n").line, 5);
        assert_eq!(err("# T\n## Tour\n1. **A** no cite\n").line, 3);
        assert_eq!(err("# T\n\n## Tour\n1. `a/b.rs` no title\n").line, 4);
        assert_eq!(err("# T\n## Tour\n1. **A** `not a path`\n").line, 3);
        assert_eq!(err("# T\n## Invariants\n- x Enforced: `foo()`\n").line, 3);
        assert_eq!(err("# T\n## Invariants\n- x\n- Tested: `a.rs`\n").line, 4);
    }

    #[test]
    fn invariants_and_tour_items_take_ids_and_markers_anywhere_in_the_item() {
        let doc = BriefDoc::from_markdown(
            "# T\n## Invariants\n* Totals add up. Enforced: `a.rs:1` `b.rs:2` {#inv.sum}\n* Second\n  spans lines.\n  Tested: `t.rs`\n## Tour\n- **Start** here `a.rs:1`\n  then read on. {#tour.start}\n",
            &[],
        )
        .unwrap();
        assert_eq!(doc.invariants[0].id, "inv.sum");
        assert_eq!(doc.invariants[0].enforced, [cite("a.rs", Some((1, 1))), cite("b.rs", Some((2, 2)))]);
        assert_eq!(doc.invariants[1].id, "inv.2");
        assert_eq!(doc.invariants[1].text, "Second\nspans lines.");
        assert_eq!(doc.invariants[1].tested, [cite("t.rs", None)]);
        assert_eq!(doc.tour[0].id, "tour.start");
        assert_eq!(
            (doc.tour[0].title.as_str(), doc.tour[0].text.as_str()),
            ("Start", "here\nthen read on.")
        );
    }
}
