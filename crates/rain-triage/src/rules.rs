//! Path and content rules: lockfiles, tests and their subjects, migrations, public API surface,
//! money/auth paths, tests turned off.

const LOCKFILES: [&str; 14] = [
    "package-lock.json",
    "npm-shrinkwrap.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "Cargo.lock",
    "packages.lock.json",
    "go.sum",
    "bun.lockb",
    "gradle.lockfile",
    "Package.resolved",
    ".terraform.lock.hcl",
    "Gemfile.lock",
    "Pipfile.lock",
    "composer.lock",
];

pub fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn ext(name: &str) -> &str {
    name.rfind('.').map_or("", |at| &name[at + 1..])
}

fn segments(path: &str) -> impl Iterator<Item = &str> {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    parts.into_iter()
}

pub fn lockfile(path: &str) -> bool {
    let name = file_name(path);
    LOCKFILES.contains(&name) || name.ends_with(".lock")
}

/// The subject's stem a test file covers (`voucher.service.spec.ts` → `voucher.service`,
/// `VoucherServiceTests.cs` → `VoucherService`), or `None` when `path` is not a test.
pub fn test_stem(path: &str) -> Option<String> {
    let name = file_name(path);
    let e = ext(name);
    let stem = name.strip_suffix(&format!(".{e}")).unwrap_or(name);
    for marker in [".spec", ".test", "_spec", "_test"] {
        if let Some(s) = stem.strip_suffix(marker)
            && !s.is_empty()
        {
            return Some(s.to_owned());
        }
    }
    if e == "py"
        && let Some(s) = stem.strip_prefix("test_")
    {
        return Some(s.to_owned());
    }
    if matches!(e, "cs" | "java" | "kt" | "scala" | "swift" | "php") {
        for marker in ["Tests", "Test", "Spec"] {
            if let Some(s) = stem.strip_suffix(marker)
                && !s.is_empty()
            {
                return Some(s.to_owned());
            }
        }
    }
    let in_test_dir = segments(path)
        .any(|s| matches!(s, "test" | "tests" | "__tests__" | "spec" | "specs" | "e2e") || s.ends_with(".Tests") || s.ends_with(".Test"));
    in_test_dir.then(|| stem.to_owned())
}

/// The stem a subject file is paired by: its name without the last extension.
pub fn subject_stem(path: &str) -> &str {
    let name = file_name(path);
    name.rfind('.').filter(|&at| at > 0).map_or(name, |at| &name[..at])
}

pub fn migration(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    segments(&lower).any(|s| s == "migrations" || s == "migrate" || s == "migration")
        || lower.ends_with(".sql")
        || file_name(&lower) == "schema.prisma"
}

/// Why the path is public API surface, if it is.
pub fn public_api<'a>(path: &str, mut added: impl Iterator<Item = &'a str>) -> Option<&'static str> {
    let name = file_name(path);
    let stem = subject_stem(path);
    if stem.ends_with("Controller") || stem.ends_with(".controller") {
        return Some("controller");
    }
    if ext(name) == "proto" {
        return Some("proto");
    }
    if matches!(name, "index.ts" | "index.js" | "index.mjs" | "public-api.ts") {
        return Some("exports");
    }
    if name == "lib.rs" && added.any(|l| l.trim_start().starts_with("pub ")) {
        return Some("pub items");
    }
    None
}

const MONEY_WORDS: [&str; 17] = [
    "payment",
    "payments",
    "invoice",
    "invoices",
    "tip",
    "tips",
    "voucher",
    "vouchers",
    "auth",
    "authentication",
    "authorization",
    "token",
    "tokens",
    "secret",
    "secrets",
    "billing",
    "refund",
];

/// The first money/auth word in the path, split at case, digit and punctuation boundaries
/// (`Voucher0Controller.cs` has `Voucher`; `Tooltip.ts` has none).
pub fn money_word(path: &str) -> Option<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut prev: Option<char> = None;
    for c in path.chars() {
        let boundary = match prev {
            _ if !c.is_alphabetic() => true,
            Some(p) => c.is_uppercase() && p.is_lowercase() || !p.is_alphabetic(),
            None => false,
        };
        if boundary && !cur.is_empty() {
            words.push(std::mem::take(&mut cur));
        }
        if c.is_alphabetic() {
            cur.push(c);
        }
        prev = Some(c);
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words.into_iter().find(|w| MONEY_WORDS.contains(&w.to_ascii_lowercase().as_str()))
}

/// Attributes that turn a test off; they count only at the start of a line.
const ATTRIBUTES_OFF: [&str; 4] = ["#[ignore", "@Disabled", "@Ignore", "[Ignore"];

/// Calls that turn a test off; they count only in code, not in a string or a comment.
const CALLS_OFF: [&str; 6] = [".skip(", ".only(", "xit(", "xdescribe(", "t.Skip(", "(Skip ="];

const DOC_COMMENTS: [&str; 4] = ["///", "//!", "/**", "*"];

/// Whether byte `at` of `line` is inside a string literal or after a `//` comment start.
fn in_string_or_comment(line: &str, at: usize) -> bool {
    let mut open: Option<char> = None;
    let mut chars = line[..at].chars().peekable();
    while let Some(c) = chars.next() {
        match open {
            Some(_) if c == '\\' => {
                chars.next();
            }
            Some(q) if c == q => open = None,
            Some(_) => {}
            None if matches!(c, '"' | '\'' | '`') => open = Some(c),
            None if c == '/' && chars.peek() == Some(&'/') => return true,
            None => {}
        }
    }
    open.is_some()
}

/// The marker on an added line that turns a test off, if any.
pub fn test_off(line: &str) -> Option<&'static str> {
    let t = line.trim_start();
    if DOC_COMMENTS.iter().any(|d| t.starts_with(d)) {
        return None;
    }
    if let Some(m) = ATTRIBUTES_OFF.iter().copied().find(|m| t.starts_with(m)) {
        return Some(m);
    }
    CALLS_OFF.iter().copied().find(|m| {
        line.match_indices(m).any(|(i, _)| {
            let word_start =
                !m.starts_with(|c: char| c.is_ascii_alphabetic()) || !line[..i].ends_with(|c: char| c.is_alphanumeric() || c == '_');
            word_start && !in_string_or_comment(line, i)
        })
    })
}

fn fixture(path: &str) -> bool {
    path.starts_with("fixtures/") || path.contains("/fixtures/")
}

/// [`test_off`] for a file: Rust only turns tests off with `#[ignore`; fixtures never do.
pub fn test_off_in(path: &str, line: &str) -> Option<&'static str> {
    if fixture(path) {
        return None;
    }
    test_off(line).filter(|m| !path.ends_with(".rs") || *m == "#[ignore")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tests_and_their_subjects() {
        let cases = [
            ("app/src/app/voucher1/services/voucher1.service.spec.ts", Some("voucher1.service")),
            ("src/a.test.js", Some("a")),
            ("pkg/x_test.go", Some("x")),
            ("app/test_y.py", Some("y")),
            ("server/tests/VoucherServiceTests.cs", Some("VoucherService")),
            ("src/Contest.cs", None),
            ("crates/x/tests/repo.rs", Some("repo")),
            ("src/Acme.Tests/Helpers.cs", Some("Helpers")),
            ("src/Services/VoucherService.cs", None),
        ];
        for (path, want) in cases {
            assert_eq!(test_stem(path).as_deref(), want, "{path}");
        }
        assert_eq!(subject_stem("src/voucher1.service.ts"), "voucher1.service");
        assert_eq!(subject_stem(".gitignore"), ".gitignore");
    }

    #[test]
    fn heuristics() {
        assert!(lockfile("app/package-lock.json") && lockfile("Cargo.lock") && lockfile("x/poetry.lock"));
        assert!(!lockfile("src/lock.rs"));
        assert!(migration("server/src/Data/Migrations/2026_AddVoucher.cs") && migration("db/001.sql"));
        assert!(!migration("src/MigrationsHelper.cs"));
        assert_eq!(money_word("server/src/Api/PaymentsController.cs").as_deref(), Some("Payments"));
        assert_eq!(
            money_word("app/app/voucher0/voucher0-form.component.html").as_deref(),
            Some("voucher")
        );
        assert_eq!(money_word("src/Tooltip.ts"), None);
        assert_eq!(money_word("src/TokenizerUtil.cs"), None);
        assert_eq!(public_api("Api/Hold0Controller.cs", std::iter::empty()), Some("controller"));
        assert_eq!(
            public_api("crates/a/src/lib.rs", ["  pub fn x() {}"].into_iter()),
            Some("pub items")
        );
        assert_eq!(public_api("crates/a/src/lib.rs", ["  fn x() {}"].into_iter()), None);
        assert_eq!(test_off("    #[ignore = \"slow\"]"), Some("#[ignore"));
        assert_eq!(test_off("  it.skip('x', () => {})"), Some(".skip("));
        assert_eq!(test_off("  xit('x', () => {})"), Some("xit("));
        assert_eq!(test_off("    std::process::exit(2)"), None);
        assert_eq!(test_off_in("src/a.rs", "  for l in it.skip(1) {"), None);
        assert_eq!(test_off_in("src/a.spec.ts", "  it.skip('x')"), Some(".skip("));
    }

    #[test]
    fn tests_off_markers_in_code() {
        assert_eq!(test_off("@Disabled(\"flaky\")"), Some("@Disabled"));
        assert_eq!(test_off("    @Ignore"), Some("@Ignore"));
        assert_eq!(test_off("    [Ignore(\"later\")]"), Some("[Ignore"));
        assert_eq!(test_off("    [Fact(Skip = \"flaky\")]"), Some("(Skip ="));
        assert_eq!(test_off("  describe.only('x', () => {})"), Some(".only("));
        assert_eq!(test_off("  xdescribe('x', () => {})"), Some("xdescribe("));
        assert_eq!(test_off("\tt.Skip(\"needs network\")"), Some("t.Skip("));
        assert_eq!(test_off("  const s = 'it'; it.skip('x')"), Some(".skip("));
        assert_eq!(test_off("  const s = \"a\\\"b\"; it.skip('x')"), Some(".skip("));
    }

    #[test]
    fn tests_off_markers_outside_code() {
        assert_eq!(test_off("    let src = \"#[ignore]\\nfn a() {}\";"), None);
        assert_eq!(test_off("    assert_eq!(test_off(\"#[ignore = 1]\"), None);"), None);
        assert_eq!(test_off("    /// Turns a test off with `#[ignore`."), None);
        assert_eq!(test_off("    //! `#[ignore` and `.skip(` markers"), None);
        assert_eq!(test_off("  /** it.skip('x') is counted */"), None);
        assert_eq!(test_off("   * @Ignore on a class"), None);
        assert_eq!(test_off("  const m = 'it.skip(';"), None);
        assert_eq!(test_off("  const m = `xit(${a})`;"), None);
        assert_eq!(test_off("  run(); // it.skip('x') later"), None);
        assert_eq!(test_off("  log(\"use @Disabled\")"), None);
        assert_eq!(test_off_in("crates/x/tests/fixtures/a.rs", "#[ignore]"), None);
        assert_eq!(test_off_in("fixtures/web/a.spec.ts", "it.skip('x')"), None);
        assert_eq!(test_off_in("crates/x/tests/a.rs", "#[ignore]"), Some("#[ignore"));
    }
}
