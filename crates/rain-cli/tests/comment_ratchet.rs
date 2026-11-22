//! Comment lines per crate may only go down: `docs/comment-baseline.txt` holds the most each crate
//! may have. Rewrite it after deleting comments with `RAIN_COMMENT_BASELINE=write`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const BASELINE: &str = "docs/comment-baseline.txt";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Lines whose first non-blank text is `//` (`//`, `///`, `//!`), in every `.rs` file under `dir`.
fn comment_lines(dir: &Path) -> usize {
    let mut n = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|f| f != "target") {
                n += comment_lines(&path);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).unwrap();
            n += text.lines().filter(|l| l.trim_start().starts_with("//")).count();
        }
    }
    n
}

fn counts() -> BTreeMap<String, usize> {
    let root = root();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(root.join("crates"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("Cargo.toml").is_file())
        .collect();
    dirs.push(root.join("xtask"));
    dirs.iter()
        .map(|d| (d.file_name().unwrap().to_string_lossy().into_owned(), comment_lines(d)))
        .collect()
}

fn parse(text: &str) -> BTreeMap<String, usize> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (name, n) = l.split_once(char::is_whitespace).unwrap_or((l, ""));
            let n = n
                .trim()
                .parse()
                .unwrap_or_else(|_| panic!("{BASELINE}: `{l}` is not `<crate> <count>`"));
            (name.to_owned(), n)
        })
        .collect()
}

fn render(counts: &BTreeMap<String, usize>) -> String {
    let mut out = String::from(
        "# Most comment lines (`//`, `///`, `//!` starting a line) each crate may have.\n\
         # Checked by crates/rain-cli/tests/comment_ratchet.rs; lower it, never raise it without a reason.\n",
    );
    for (name, n) in counts {
        out.push_str(&format!("{name} {n}\n"));
    }
    out
}

/// Each crate over its baseline, or missing from one side.
fn over(baseline: &BTreeMap<String, usize>, now: &BTreeMap<String, usize>) -> Vec<String> {
    let mut wrong = Vec::new();
    for (name, &n) in now {
        match baseline.get(name) {
            None => wrong.push(format!("{name} is not in {BASELINE}")),
            Some(&max) if n > max => wrong.push(format!("{name} has {n} comment lines, {} over its {max}", n - max)),
            Some(_) => {}
        }
    }
    for name in baseline.keys().filter(|n| !now.contains_key(*n)) {
        wrong.push(format!("{BASELINE} names {name}, which is not a crate"));
    }
    wrong
}

#[test]
fn no_crate_has_more_comment_lines_than_its_baseline() {
    let now = counts();
    let path = root().join(BASELINE);
    if std::env::var("RAIN_COMMENT_BASELINE").is_ok_and(|v| v == "write") {
        std::fs::write(&path, render(&now)).unwrap();
        return;
    }
    let baseline = parse(&std::fs::read_to_string(&path).unwrap());
    let wrong = over(&baseline, &now);
    assert!(
        wrong.is_empty(),
        "comments only go down (see {BASELINE}):\n{}\n\
         Delete the new comments, or keep one only where code would be misread without it and \
         raise the count with a reason in the commit. After deleting comments, lower the baseline: \
         RAIN_COMMENT_BASELINE=write cargo test -p rain-cli --test comment_ratchet",
        wrong.join("\n")
    );
}

#[test]
fn a_crate_over_its_baseline_or_missing_from_it_is_caught() {
    let base = parse("# note\na 3\nb 2\n");
    let now = |a, b| BTreeMap::from([("a".to_owned(), a), ("b".to_owned(), b)]);
    assert!(over(&base, &now(3, 1)).is_empty());
    assert_eq!(over(&base, &now(4, 2)).len(), 1);
    let mut extra = now(1, 1);
    extra.insert("c".to_owned(), 0);
    assert_eq!(over(&base, &extra), vec![format!("c is not in {BASELINE}")]);
    assert_eq!(over(&parse("a 3\nb 2\ngone 1\n"), &now(1, 1)).len(), 1);
    assert_eq!(parse(&render(&now(5, 0))), now(5, 0));
}
