use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum GitError {
    /// The `git` executable could not be started.
    GitNotFound(std::io::Error),
    NotARepo {
        dir: PathBuf,
        stderr: String,
    },
    BadRef {
        rev: String,
        stderr: String,
    },
    Exited {
        args: String,
        code: Option<i32>,
        stderr: String,
    },
    Io(std::io::Error),
    /// A path that would escape the repository.
    BadPath(String),
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitError::GitNotFound(e) => write!(f, "git was not found on PATH ({e})"),
            GitError::NotARepo { dir, stderr } => write!(f, "{} is not a git repository: {}", dir.display(), stderr.trim()),
            GitError::BadRef { rev, stderr } => write!(f, "unknown revision '{rev}': {}", stderr.trim()),
            GitError::Exited { args, code, stderr } => {
                let code = code.map_or_else(|| "a signal".to_owned(), |c| c.to_string());
                write!(f, "git {args} exited with {code}: {}", stderr.trim())
            }
            GitError::Io(e) => write!(f, "git i/o: {e}"),
            GitError::BadPath(p) => write!(f, "path outside the repository: {p}"),
        }
    }
}

impl std::error::Error for GitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            GitError::GitNotFound(e) | GitError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for GitError {
    fn from(e: std::io::Error) -> Self {
        GitError::Io(e)
    }
}
