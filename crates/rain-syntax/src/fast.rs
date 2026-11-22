//! Linear scanners for languages rune-kit lacks, built on its nearest one. ~100x faster than
//! syntect's grammars, for 1M-line files such as EF model snapshots.
//!
//! C#: the TypeScript scanner (C-family comments, strings, numbers, calls, capitalised types)
//! plus a pass over the text it left plain for C#-only keywords and types.
//! SCSS: the CSS scanner over the code with `//` comments blanked, plus those comments.

use rune_kit::syntax::{Highlight, Paint, Syntax, builtin};

const KEYWORDS: &[&str] = &[
    "abstract",
    "base",
    "checked",
    "const",
    "default",
    "delegate",
    "event",
    "explicit",
    "extern",
    "fixed",
    "foreach",
    "get",
    "goto",
    "implicit",
    "init",
    "internal",
    "is",
    "lock",
    "nameof",
    "operator",
    "out",
    "override",
    "params",
    "partial",
    "ref",
    "required",
    "sealed",
    "set",
    "sizeof",
    "stackalloc",
    "struct",
    "unchecked",
    "unsafe",
    "using",
    "virtual",
    "volatile",
    "when",
    "where",
    "yield",
    "record",
    "var",
    "dynamic",
];

const TYPES: &[&str] = &[
    "bool", "byte", "char", "decimal", "double", "float", "int", "long", "nint", "nuint", "object", "sbyte", "short", "uint", "ulong",
    "ushort",
];

fn word_kind(w: &str) -> Option<Syntax> {
    if KEYWORDS.contains(&w) {
        Some(Syntax::Keyword)
    } else if TYPES.contains(&w) {
        Some(Syntax::Type)
    } else {
        None
    }
}

fn plain_words(code: &str, from: usize, to: usize, out: &mut Vec<Highlight>) {
    let b = code.as_bytes();
    let mut i = from;
    while i < to {
        let c = b[i];
        if c.is_ascii_alphabetic() || c == b'_' {
            let s = i;
            while i < to && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            let preceded = s > 0 && (b[s - 1] == b'.' || b[s - 1].is_ascii_alphanumeric());
            if !preceded && let Some(k) = word_kind(&code[s..i]) {
                out.push((s..i, Paint::Kind(k)));
            }
        } else if c == b'#' && b[..i].iter().rev().take_while(|&&c| c != b'\n').all(|c| c.is_ascii_whitespace()) {
            let s = i;
            i += 1;
            while i < to && b[i].is_ascii_alphabetic() {
                i += 1;
            }
            out.push((s..i, Paint::Kind(Syntax::Keyword)));
        } else {
            i += 1;
        }
    }
}

/// Byte ranges of `//` line comments outside strings and not part of a URL (`://`).
fn line_comments(code: &str) -> Vec<std::ops::Range<usize>> {
    let b = code.as_bytes();
    let mut out = Vec::new();
    let mut quote = None;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match quote {
            Some(q) if c == q || c == b'\n' => quote = None,
            Some(_) if c == b'\\' => i += 1,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => quote = Some(c),
            None if c == b'/' && b.get(i + 1) == Some(&b'*') => {
                i = code[i + 2..].find("*/").map_or(b.len(), |e| i + 2 + e + 1);
            }
            None if c == b'/' && b.get(i + 1) == Some(&b'/') && (i == 0 || b[i - 1] != b':') => {
                let end = code[i..].find('\n').map_or(b.len(), |e| i + e);
                out.push(i..code[i..end].trim_end().len() + i);
                i = end;
                continue;
            }
            None => {}
        }
        i += 1;
    }
    out
}

pub fn scss(code: &str) -> Vec<Highlight> {
    let comments = line_comments(code);
    if comments.is_empty() {
        return builtin(Some("css"), code);
    }
    let mut blank = code.as_bytes().to_vec();
    for r in &comments {
        blank[r.clone()].fill(b' ');
    }
    let blank = String::from_utf8(blank).expect("whole characters were replaced by spaces");
    let mut out = builtin(Some("css"), &blank);
    out.extend(comments.into_iter().map(|r| (r, Paint::Kind(Syntax::Comment))));
    out.sort_by_key(|(r, _)| r.start);
    out
}

/// Languages whose only strings over a line end are backtick templates, in pieces around `${}`.
pub(crate) fn has_templates(key: &str) -> bool {
    matches!(
        key.to_ascii_lowercase().as_str(),
        "js" | "javascript" | "jsx" | "mjs" | "cjs" | "ts" | "typescript" | "mts" | "cts" | "tsx"
    )
}

/// The delimiter of the comment or string `code` (whole lines) ends inside, going by the
/// scanner's own last range: one that runs over the final line end never closed. Scanning the
/// lines after it behind that delimiter puts the scanner back inside.
pub(crate) fn left_open<'a>(code: &'a str, hl: &[Highlight], templates: bool) -> Option<&'a str> {
    let (r, p) = hl.last()?;
    if r.end != code.len() || !code.ends_with('\n') {
        return None;
    }
    let t = &code[r.start..];
    match p {
        Paint::Kind(Syntax::Comment) => ["/*", "<!--"].into_iter().find(|o| t.starts_with(o)),
        Paint::Kind(Syntax::String) if templates => Some("`"),
        Paint::Kind(Syntax::String) => {
            let b = t.as_bytes();
            let lead = b
                .iter()
                .take(8)
                .take_while(|c| c.is_ascii_alphabetic() || matches!(c, b'#' | b'$' | b'@'))
                .count();
            match b.get(lead) {
                Some(&q @ (b'"' | b'\'' | b'`')) => {
                    let triple = b.get(lead + 1) == Some(&q) && b.get(lead + 2) == Some(&q);
                    Some(&t[..lead + if triple { 3 } else { 1 }])
                }
                _ => None,
            }
        }
        _ => None,
    }
}

pub fn csharp(code: &str) -> Vec<Highlight> {
    let base = builtin(Some("ts"), code);
    let b = code.as_bytes();
    let mut out = Vec::with_capacity(base.len() + base.len() / 4);
    let mut last = 0;
    for (r, p) in base {
        let mut r = r;
        if p == Paint::Kind(Syntax::String) {
            while r.start > last && matches!(b[r.start - 1], b'$' | b'@') {
                r.start -= 1;
            }
        }
        plain_words(code, last, r.start, &mut out);
        last = r.end;
        out.push((r, p));
    }
    plain_words(code, last, code.len(), &mut out);
    out
}
