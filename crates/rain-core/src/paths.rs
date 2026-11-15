//! `<repoId>/<repo-relative path>` paths, as the overview and multi-repo sessions write them.

/// `"server/src/x.cs"` → `("server", "src/x.cs")`; a path with no `/` has an empty repo.
pub fn split_repo_path(p: &str) -> (&str, &str) {
    match p.split_once('/') {
        Some((repo, rest)) => (repo, rest),
        None => ("", p),
    }
}

pub fn join(repo: &str, path: &str) -> String {
    if repo.is_empty() {
        path.to_owned()
    } else {
        format!("{repo}/{path}")
    }
}

/// Forward slashes, no leading `./`.
pub fn normalize(p: &str) -> String {
    let p = p.replace('\\', "/");
    p.strip_prefix("./").unwrap_or(&p).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_and_join_round_trip() {
        assert_eq!(split_repo_path("server/src/x.cs"), ("server", "src/x.cs"));
        assert_eq!(split_repo_path("x.cs"), ("", "x.cs"));
        assert_eq!(join("server", "src/x.cs"), "server/src/x.cs");
        assert_eq!(join("", "x.cs"), "x.cs");
        assert_eq!(normalize(".\\src\\x.cs"), "src/x.cs");
    }
}
