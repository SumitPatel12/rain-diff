//! Every entry of `docs/decisions.md` is pinned by a test that exists or a line that still says
//! what the entry quotes, so the page cannot outlive the code it describes.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn sources(dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(std::fs::read_to_string(&path).unwrap());
        }
    }
}

/// Whether a `#[test]` named `name` is in `text`.
fn has_test(text: &str, name: &str) -> bool {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let decl = format!("fn {name}()");
    lines
        .iter()
        .enumerate()
        .any(|(i, l)| l.starts_with(&decl) && i > 0 && lines[i - 1] == "#[test]")
}

/// The text between backticks in `line`.
fn quoted(line: &str) -> Vec<&str> {
    line.split('`').skip(1).step_by(2).collect()
}

/// What is wrong with one pin: `` `crate::test_name` `` or `` `path:line` has `text` ``.
fn check(pin: &str) -> Option<String> {
    let q = quoted(pin);
    let first = *q.first()?;
    if let Some((krate, test)) = first.split_once("::") {
        let dir = root().join("crates").join(krate);
        if !dir.is_dir() {
            return Some(format!("no crate {krate}"));
        }
        let mut texts = Vec::new();
        sources(&dir, &mut texts);
        return (!texts.iter().any(|t| has_test(t, test))).then(|| format!("{krate} has no test {test}"));
    }
    let Some((path, line)) = first.rsplit_once(':') else {
        return Some(format!("`{first}` is neither `crate::test` nor `path:line`"));
    };
    let Ok(text) = std::fs::read_to_string(root().join(path)) else {
        return Some(format!("{path} is missing"));
    };
    let (Some(needle), Ok(n)) = (q.get(1), line.parse::<usize>()) else {
        return Some(format!("`{first}` needs a line number and the text that line has"));
    };
    match text.lines().nth(n.wrapping_sub(1)) {
        Some(l) if l.contains(needle) => None,
        Some(_) => Some(format!("{first} no longer has `{needle}`")),
        None => Some(format!("{path} has no line {n}")),
    }
}

/// Each entry's title with the bullets under its "Pinned by:" line.
fn entries(doc: &str) -> Vec<(String, Vec<String>)> {
    let mut entries: Vec<(String, Vec<String>)> = Vec::new();
    let mut pinning = false;
    for l in doc.lines() {
        if let Some(title) = l.strip_prefix("## ") {
            entries.push((title.to_owned(), Vec::new()));
            pinning = false;
        } else if l.starts_with("**Pinned by:**") {
            pinning = true;
        } else if pinning && l.starts_with("- ") {
            if let Some((_, pins)) = entries.last_mut() {
                pins.push(l.to_owned());
            }
        } else if !l.trim().is_empty() {
            pinning = false;
        }
    }
    entries
}

#[test]
fn every_decision_is_pinned_by_a_test_that_exists_or_a_line_that_still_says_it() {
    let doc = std::fs::read_to_string(root().join("docs/decisions.md")).unwrap();
    let entries = entries(&doc);
    assert!(entries.len() >= 10, "docs/decisions.md did not parse: {} entries", entries.len());
    let mut wrong = Vec::new();
    for (title, pins) in &entries {
        if pins.is_empty() {
            wrong.push(format!("{title}: no pin"));
        }
        for pin in pins {
            match check(pin) {
                None if !quoted(pin).is_empty() => {}
                None => wrong.push(format!("{title}: `{pin}` names nothing")),
                Some(why) => wrong.push(format!("{title}: {why}")),
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "docs/decisions.md has drifted from the code:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn a_pin_to_a_missing_test_file_or_line_is_caught() {
    assert_eq!(check("- `rain-cli::a_pin_to_a_missing_test_file_or_line_is_caught`"), None);
    assert_eq!(check("- `Cargo.toml:1` has `[workspace]`"), None);
    for gone in [
        "- `rain-cli::no_such_test_anywhere`",
        "- `rain-nowhere::a_pin_to_a_missing_test_file_or_line_is_caught`",
        "- `rain-cli::check`",
        "- `docs/gone.md:1` has `x`",
        "- `Cargo.toml:1` has `not on that line`",
        "- `Cargo.toml:999999` has `[workspace]`",
        "- `Cargo.toml:1`",
        "- `Cargo.toml`",
    ] {
        assert!(check(gone).is_some(), "{gone}");
    }
}
