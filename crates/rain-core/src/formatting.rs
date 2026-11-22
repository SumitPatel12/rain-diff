//! Whether a run of removed lines and the added lines replacing it hold the same code laid out
//! differently: re-indented, or re-wrapped as a formatter splits or joins an argument list.
//! The idea is difftastic's (formatting is not a change); this is a far lighter check, on the
//! lines' words and marks, with no parse.
//!
//! It answers no whenever it can't be sure: in languages where line breaks or indentation are
//! syntax, around line comments and preprocessor lines, and where a text literal runs past its
//! line. Text inside quotes is compared as written, spaces included.

use std::cell::Cell;
use std::ops::Range;

use crate::diff::FileDiff;

/// A run holding a line longer than this is never formatting only. It is the most of a line the
/// diff draws, so a longer line could not be shown to be the same code.
pub const LINE_MAX_BYTES: usize = 10_000;

/// What the check read on this thread so far; tests hold it to the length of the lines.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    /// Characters compared between the two sides, spaces and commas aside.
    pub ink: u64,
    /// Bytes the tokeniser's searches were given.
    pub lexed: u64,
}

thread_local! {
    static WORK: Cell<Work> = const { Cell::new(Work { ink: 0, lexed: 0 }) };
}

pub fn work() -> Work {
    WORK.get()
}

/// Languages with braces and statement ends, where spacing between words and marks is free.
fn free_form(path: &str) -> bool {
    const EXTENSIONS: [&str; 25] = [
        "rs", "c", "h", "cc", "cpp", "cxx", "hh", "hpp", "hxx", "cs", "java", "js", "jsx", "mjs", "cjs", "ts", "tsx", "mts", "cts", "css",
        "scss", "less", "json", "jsonc", "dart",
    ];
    let name = path.rsplit('/').next().unwrap_or(path);
    let Some((stem, ext)) = name.rsplit_once('.') else { return false };
    !stem.is_empty() && EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e))
}

/// A word, mark or quoted text, and the line (counted within its side) it sits on.
type Token<'a> = (&'a str, u32);

/// What stops a side's line breaks from moving freely.
#[derive(Default)]
struct Side<'a> {
    tokens: Vec<Token<'a>>,
    /// A `//` comment, a `#` line, or a line that ends where a statement may silently end.
    bound_to_lines: bool,
}

const SINGLE: &str = "()[]{},;";

/// Appends `line`'s tokens; `false` when a quoted text does not close on the line.
fn lex<'a>(line: &'a str, n: u32, out: &mut Side<'a>) -> bool {
    let mut lexed = 0;
    let closed = lex_into(line, n, out, &mut lexed);
    let w = WORK.get();
    WORK.set(Work {
        lexed: w.lexed + lexed,
        ..w
    });
    closed
}

fn lex_into<'a>(line: &'a str, n: u32, out: &mut Side<'a>, lexed: &mut u64) -> bool {
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let first = out.tokens.len();
    let mut rest = line.trim_start();
    out.bound_to_lines |= rest.starts_with('#');
    while let Some(c) = rest.chars().next() {
        let len = if c.is_whitespace() {
            rest = rest.trim_start();
            continue;
        } else if word(c) {
            rest.find(|c: char| !word(c)).unwrap_or(rest.len())
        } else if c == '`' {
            return false;
        } else if c == '"' || c == '\'' {
            let mut escaped = false;
            let close = rest[1..].find(|x: char| {
                let found = x == c && !escaped;
                escaped = x == '\\' && !escaped;
                found
            });
            match close {
                Some(at) => at + 2,
                None => return false,
            }
        } else if rest.starts_with("//") {
            out.bound_to_lines = true;
            rest.len()
        } else if SINGLE.contains(c) {
            1
        } else {
            let mark = |x: char| !(x.is_whitespace() || word(x) || SINGLE.contains(x) || "\"'`".contains(x));
            let run = &rest[..rest.find(|x: char| !mark(x)).unwrap_or(rest.len())];
            *lexed += run.len() as u64;
            run.find("//").unwrap_or(run.len())
        };
        *lexed += len as u64;
        out.tokens.push((&rest[..len], n));
        rest = &rest[len..];
    }
    let ends = out.tokens[first..].last().map(|t| t.0);
    let starts = out.tokens.get(first).map(|t| t.0);
    out.bound_to_lines |= matches!(ends, Some("return" | "break" | "continue" | "throw" | "yield" | "async" | "await"))
        || matches!(starts, Some(t) if t.starts_with("++") || t.starts_with("--"));
    true
}

fn side<'a>(f: &'a FileDiff, lines: Range<u32>) -> Option<Side<'a>> {
    let mut out = Side::default();
    let start = lines.start;
    (start..lines.end).all(|i| lex(f.line_text(i), i - start, &mut out)).then_some(out)
}

/// The tokens' texts without the comma a formatter adds after the last item of a list it
/// wrapped: one that ends its line, follows an item, and has the list's closing bracket next,
/// on a later line. A lone item in round brackets keeps its comma (a one-item tuple needs it).
fn without_trailing_commas<'a>(tokens: &[Token<'a>]) -> Vec<&'a str> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut commas: Vec<u32> = vec![0];
    for (k, &(t, line)) in tokens.iter().enumerate() {
        match t {
            "(" | "[" | "{" => commas.push(0),
            ")" | "]" | "}" if commas.len() > 1 => {
                commas.pop();
            }
            "," => {
                let after_item = k > 0 && !matches!(tokens[k - 1].0, "," | "(" | "[" | "{");
                let seen = commas.last_mut().expect("the outermost list is never popped");
                let closes = tokens.get(k + 1).is_some_and(|&(next, at)| match next {
                    ")" => at > line && *seen > 0,
                    "]" | "}" => at > line,
                    _ => false,
                });
                if after_item && closes {
                    continue;
                }
                *seen += 1;
            }
            _ => {}
        }
        out.push(t);
    }
    out
}

/// Whether both runs hold the same characters in the same order once spaces and commas are
/// dropped, as two layouts of the same code do. It stops at the first character that differs,
/// and at a line over [`LINE_MAX_BYTES`], which it never reads.
fn same_ink(f: &FileDiff, removed: Range<u32>, added: Range<u32>) -> bool {
    let long = &Cell::new(false);
    let ink = |lines: Range<u32>| {
        lines
            .map_while(move |i| {
                let text = f.line_text(i);
                long.set(long.get() || text.len() > LINE_MAX_BYTES);
                (!long.get()).then_some(text)
            })
            .flat_map(str::chars)
            .filter(|c| !c.is_whitespace() && *c != ',')
    };
    let (mut old, mut new) = (ink(removed), ink(added));
    let mut read = 0;
    let same = loop {
        match (old.next(), new.next()) {
            (None, None) => break true,
            (a, b) if a != b => break false,
            _ => read += 1,
        }
    };
    let w = WORK.get();
    WORK.set(Work { ink: w.ink + read, ..w });
    same && !long.get()
}

/// Whether lines `removed` and `added` of `f` (one run of each, the second replacing the
/// first) are the same code: the same words, marks and quoted texts in the same order, with
/// only the spacing, the line breaks or a wrapped list's trailing comma changed.
pub fn formatting_only(f: &FileDiff, removed: Range<u32>, added: Range<u32>) -> bool {
    if removed.is_empty() || added.is_empty() || !free_form(&f.path) {
        return false;
    }
    if !same_ink(f, removed.clone(), added.clone()) {
        return false;
    }
    let (Some(old), Some(new)) = (side(f, removed), side(f, added)) else {
        return false;
    };
    if old.tokens == new.tokens {
        return true;
    }
    !old.bound_to_lines && !new.bound_to_lines && without_trailing_commas(&old.tokens) == without_trailing_commas(&new.tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{FileDiffBuilder, FileStatus, LineKind};

    fn same(path: &str, removed: &[&str], added: &[&str]) -> bool {
        let mut b = FileDiffBuilder::new(0, path, FileStatus::Modified);
        b.push_hunk(1, removed.len() as u32, 1, added.len() as u32, "");
        for (k, t) in removed.iter().enumerate() {
            b.push_line(LineKind::Removed, k as u32 + 1, 0, t);
        }
        for (k, t) in added.iter().enumerate() {
            b.push_line(LineKind::Added, 0, k as u32 + 1, t);
        }
        let (nd, n) = (removed.len() as u32, (removed.len() + added.len()) as u32);
        formatting_only(&b.finish(), 0..nd, nd..n)
    }

    const CALL: [&str; 1] = ["    let order = Order::new(customer, items, shipping);"];
    const WRAPPED: [&str; 5] = [
        "    let order = Order::new(",
        "        customer,",
        "        items,",
        "        shipping,",
        "    );",
    ];

    #[test]
    fn a_call_split_across_lines_is_formatting_both_ways() {
        assert!(same("src/shop.rs", &CALL, &WRAPPED));
        assert!(same("src/shop.rs", &WRAPPED, &CALL));
        assert!(same("web/shop.ts", &CALL, &WRAPPED[..]));
        assert!(same(
            "src/shop.rs",
            &CALL,
            &["    let order =", "        Order::new(customer, items, shipping);"]
        ));
    }

    #[test]
    fn reindented_lines_are_formatting() {
        assert!(same(
            "Shop.cs",
            &["if (ok) {", "  Ship(order);", "}"],
            &["if (ok)", "{", "\tShip(order);", "}"]
        ));
        assert!(same(
            "shop.js",
            &["  ship(order); // then bill"],
            &["    ship(order); // then bill"]
        ));
        assert!(same("shop.css", &[".cart{color:red}"], &[".cart {", "  color: red", "}"]));
    }

    #[test]
    fn a_changed_word_mark_or_order_is_a_change() {
        assert!(!same(
            "src/shop.rs",
            &CALL,
            &[
                "    let order = Order::new(",
                "        customer,",
                "        item,",
                "        shipping,",
                "    );"
            ]
        ));
        assert!(!same("src/shop.rs", &["int x = 1;"], &["intx = 1;"]), "words don't join");
        assert!(!same("src/shop.rs", &["a + +b"], &["a ++ b"]), "marks don't join");
        assert!(!same("src/shop.rs", &["f(a, b)"], &["f(b, a)"]));
        assert!(!same("src/shop.rs", &["f(a, b)"], &["f(a b)"]), "a comma between items stays");
    }

    #[test]
    fn spaces_inside_quotes_are_a_change() {
        assert!(!same("src/shop.rs", &["log(\"paid  in full\");"], &["log(\"paid in full\");"]));
        assert!(!same("shop.js", &["log('a b', c);"], &["log('ab',", "  c);"]));
        assert!(same(
            "src/shop.rs",
            &["log(\"paid \\\" in full\",x);"],
            &["log(\"paid \\\" in full\", x);"]
        ));
        assert!(
            !same("shop.js", &["const q = `select a`;"], &["const q =", "  `select a`;"]),
            "template text may run on"
        );
        assert!(
            !same("src/shop.rs", &["let s = \"one", "two\";"], &["let s = \"one", "  two\";"]),
            "a text left open"
        );
    }

    #[test]
    fn languages_where_layout_is_syntax_are_never_formatting() {
        let (old, new) = (["total = price(a, b)"], ["total = price(", "    a, b)"]);
        for path in [
            "app/shop.py",
            "ci/build.yml",
            "Makefile",
            "README.md",
            "notes.txt",
            "LICENSE",
            "shop.go",
            "shop.kt",
            "x.unknown",
        ] {
            assert!(!same(path, &old, &new), "{path}");
            assert!(!same(path, &["  a = 1;"], &["a = 1;"]), "{path}: indentation");
        }
        assert!(same("src/shop.java", &old, &new));
    }

    #[test]
    fn line_breaks_stay_put_around_comments_directives_and_bare_returns() {
        assert!(!same("shop.js", &["ship(); // later", "bill();"], &["ship(); // later bill();"]));
        assert!(!same("shop.c", &["#define SHIP(x) \\", "  ship(x)"], &["#define SHIP(x) ship(x)"]));
        assert!(!same("shop.js", &["return", "  total;"], &["return total;"]));
        assert!(!same("shop.js", &["a = b", "++c"], &["a = b ++c"]));
    }

    /// What one check of `removed` against `added` read.
    fn read(path: &str, removed: &[&str], added: &[&str]) -> (bool, Work) {
        let before = work();
        let verdict = same(path, removed, added);
        let now = work();
        (
            verdict,
            Work {
                ink: now.ink - before.ink,
                lexed: now.lexed - before.lexed,
            },
        )
    }

    #[test]
    fn a_line_of_marks_is_searched_once_not_once_per_mark() {
        let marks = "a = b + c - d * e % f < g > h ! i & j | k ^ l ? m : n; ".repeat(16);
        assert!((850..=1000).contains(&marks.len()));
        let spaced = marks.replace(' ', "  ");
        let (verdict, w) = read("shop.js", &[&marks], &[&spaced]);
        assert!(verdict, "the same marks, spaced out");
        let bytes = (marks.len() + spaced.len()) as u64;
        assert!(w.lexed <= 2 * bytes, "{} bytes searched in {bytes} bytes of lines", w.lexed);

        let slashes = "a / b ".repeat(150);
        let (verdict, w) = read("shop.js", &[&slashes], &[&slashes.replace(" / ", "/")]);
        assert!(verdict);
        assert!(w.lexed <= 4 * slashes.len() as u64, "{} bytes searched", w.lexed);
    }

    #[test]
    fn a_comment_ends_a_run_of_marks_wherever_it_starts() {
        assert!(same("shop.js", &["a = b / c; // half"], &["a = b/c; // half"]));
        assert!(!same("shop.js", &["a = b / c; // half"], &["a = b/c; / / half"]));
        assert!(same("shop.js", &["x =// odd", "1;"], &["x  =// odd", "  1;"]));
        assert!(
            !same("shop.js", &["x =// odd", "1;"], &["x =// odd 1;"]),
            "the comment would take the 1"
        );
    }

    #[test]
    fn a_differing_run_is_read_only_up_to_its_first_difference() {
        let body: Vec<String> = (0..500).map(|i| format!("    total += price({i}) * rate;")).collect();
        let mut old: Vec<&str> = vec!["fn bill() {"];
        old.extend(body.iter().map(String::as_str));
        let mut new = old.clone();
        new[0] = "fn  pay() {";
        let (verdict, w) = read("src/shop.rs", &old, &new);
        assert!(!verdict);
        assert_eq!((w.ink, w.lexed), (2, 0), "`fn` matched, then `b` against `p`");
    }

    #[test]
    fn a_run_with_a_line_past_the_cap_is_a_change_and_the_line_is_never_read() {
        let unit = "var a=b.c(d,e)+f[g];";
        let line = |bytes: usize| unit.repeat(bytes / unit.len());
        let whole = line(LINE_MAX_BYTES);
        assert_eq!(whole.len(), LINE_MAX_BYTES);
        let halves = whole.split_at(LINE_MAX_BYTES / 2);
        let (verdict, w) = read("app.min.js", &[&whole], &[halves.0, halves.1]);
        assert!(verdict, "a line of {LINE_MAX_BYTES} bytes split in two is still formatting");
        assert!(w.ink > 0 && w.lexed > 0);
        assert!(w.lexed <= 4 * LINE_MAX_BYTES as u64, "{} bytes searched", w.lexed);

        let long = line(200_000);
        let respaced = long.replacen(';', "; ", 1);
        for (removed, added) in [
            (vec![long.as_str()], vec![respaced.as_str()]),
            (vec![long.as_str()], vec![&long[..100_000], &long[100_000..]]),
            (vec!["f(a);", long.as_str()], vec!["f( a );", long.as_str()]),
            (vec![long.as_str(), "f(a);"], vec![long.as_str(), "f( a );"]),
        ] {
            let (verdict, w) = read("app.min.js", &removed, &added);
            assert!(!verdict, "one line is past the cap");
            assert!(w.ink <= "f(a);".len() as u64 && w.lexed == 0, "read {w:?}");
        }
    }

    #[test]
    fn only_a_wrapped_lists_last_comma_may_come_or_go() {
        assert!(!same("src/shop.rs", &["let t = (a,);"], &["let t = (a);"]));
        assert!(
            !same("src/shop.rs", &["let t = (", "    a,", ");"], &["let t = (a);"]),
            "a one-item tuple"
        );
        assert!(!same("shop.js", &["[a,", ",", "]"], &["[a,]"]), "a hole in an array");
        assert!(!same("shop.js", &["f(a, b,)"], &["f(a, b)"]), "not at the end of its line");
        assert!(same("shop.js", &["const o = {a, b};"], &["const o = {", "  a,", "  b,", "};"]));
    }
}
