//! Paths whose text never goes into a prompt (`ask.redact` globs).

use globset::{GlobBuilder, GlobSetBuilder};

/// Whether `path` (repo-relative or `<repo>/<path>`, either slash) matches one of `globs`,
/// ignoring case. A glob that doesn't parse matches nothing.
pub fn matches(path: &str, globs: &[String]) -> bool {
    let mut b = GlobSetBuilder::new();
    for g in globs {
        if let Ok(g) = GlobBuilder::new(g).case_insensitive(true).literal_separator(false).build() {
            b.add(g);
        }
    }
    let Ok(set) = b.build() else { return false };
    let p = path.replace('\\', "/");
    set.is_match(&p)
}

/// For a `claude` command: `--disallowedTools` with a `Read(//**/…)` deny rule per glob, so the
/// answerer can't open on its own the files the prompt leaves out. Other commands are unchanged.
pub fn deny_reads(mut argv: Vec<String>, globs: &[String]) -> Vec<String> {
    let is_claude = argv.first().is_some_and(|p| {
        std::path::Path::new(p)
            .file_stem()
            .is_some_and(|s| s.eq_ignore_ascii_case("claude"))
    });
    if !is_claude || globs.is_empty() {
        return argv;
    }
    argv.push("--disallowedTools".into());
    for g in globs {
        let g = g.replace('\\', "/");
        let rest = g.trim_start_matches('/').trim_start_matches("**/");
        argv.push(format!("Read(//**/{rest})"));
    }
    argv
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_commands_get_a_read_deny_rule_per_glob() {
        let argv = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
        let got = deny_reads(argv("claude -p --add-dir C:/r"), &defaults()[..2]);
        assert_eq!(
            got[4..],
            ["--disallowedTools", "Read(//**/*.env)", "Read(//**/appsettings*.json)"].map(String::from)
        );
        assert_eq!(deny_reads(argv("my-agent --ask"), &defaults()), argv("my-agent --ask"));
        assert_eq!(deny_reads(argv("claude -p"), &[]), argv("claude -p"));
    }

    fn defaults() -> Vec<String> {
        ["**/*.env", "**/appsettings*.json", "**/secrets*", "**/*.pem", "**/*.key"]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn secrets_match_anywhere_and_code_does_not() {
        let g = defaults();
        for p in [
            ".env",
            "prod.env",
            "server/src/Api/appsettings.Development.json",
            r"server\src\AppSettings.json",
            "ops/secrets.yaml",
            "certs/server.PEM",
        ] {
            assert!(matches(p, &g), "{p}");
        }
        for p in ["server/src/Api/VouchersController.cs", "@brief", "docs/env.md"] {
            assert!(!matches(p, &g), "{p}");
        }
        assert!(!matches("a.env", &[]));
        assert!(!matches("a.env", &["[".into()]));
    }
}
