//! What the CLI hands the app: which repos and revisions to diff and where state lives.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoLaunch {
    pub dir: PathBuf,
    pub base: String,
    pub target: String,
    pub merge_base: bool,
    pub include_untracked: bool,
}

impl Default for RepoLaunch {
    fn default() -> Self {
        RepoLaunch {
            dir: PathBuf::from("."),
            base: "HEAD".into(),
            target: "WORKTREE".into(),
            merge_base: false,
            include_untracked: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    /// Session name; the session file is `<sessions dir>/<session>.json`.
    pub session: String,
    pub repos: Vec<RepoLaunch>,
    pub ignore_whitespace: bool,
    pub port: Option<u16>,
    /// Comment-import JSON files to load at start.
    pub comments: Vec<PathBuf>,
    pub overview: Option<PathBuf>,
    /// `<repo>/<path>:<line>` to open at.
    pub goto: Option<String>,
    /// Start with an empty session instead of the saved one.
    pub clean: bool,
    /// A `.patch` file to show instead of running git.
    pub fixture: Option<PathBuf>,
    /// Context lines around each change; `None` is git's default of 3.
    pub context: Option<u32>,
    /// Another window holds the session: show it, never save it.
    pub read_only: bool,
}
