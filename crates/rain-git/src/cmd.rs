use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::thread::JoinHandle;

use crate::GitError;

/// Git's own defaults, except `histogram`, for every setting that changes what a diff says, so
/// one review has the same hunks on every machine.
const PINNED_CONFIG: &[&str] = &[
    "core.quotePath=true",
    "core.bigFileThreshold=512m",
    "color.ui=false",
    "diff.algorithm=histogram",
    "diff.indentHeuristic=true",
    "diff.context=3",
    "diff.interHunkContext=0",
    "diff.renames=true",
    "diff.renameLimit=1000",
    "diff.relative=false",
    "diff.noprefix=false",
    "diff.mnemonicPrefix=false",
    "diff.srcPrefix=a/",
    "diff.dstPrefix=b/",
    "diff.submodule=short",
    "diff.suppressBlankEmpty=false",
    "diff.autoRefreshIndex=true",
];

/// What every `git diff` starts with: the settings a command-line flag pins harder than
/// [`PINNED_CONFIG`] can (a `diff=<driver>` attribute outranks config, not a flag).
pub(crate) const DIFF: &[&str] = &[
    "diff",
    "--no-ext-diff",
    "--no-textconv",
    "--color=never",
    "--no-relative",
    "--submodule=short",
    "--src-prefix=a/",
    "--dst-prefix=b/",
    "--diff-algorithm=histogram",
    "--indent-heuristic",
    "--inter-hunk-context=0",
    "-O/dev/null",
];

/// `git -C <repo>` with output settings pinned so user config cannot change what we parse.
pub(crate) fn git(repo: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C").arg(repo);
    for setting in PINNED_CONFIG {
        c.args(["-c", setting]);
    }
    // It outranks `-U`.
    c.env_remove("GIT_DIFF_OPTS")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

pub(crate) fn spawn_error(e: std::io::Error) -> GitError {
    if e.kind() == std::io::ErrorKind::NotFound {
        GitError::GitNotFound(e)
    } else {
        GitError::Io(e)
    }
}

fn exited(args: &[&str], code: Option<i32>, stderr: &[u8]) -> GitError {
    GitError::Exited {
        args: args.join(" "),
        code,
        stderr: String::from_utf8_lossy(stderr).into_owned(),
    }
}

/// Runs git to completion; stdout on success.
pub(crate) fn run(repo: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
    let out = git(repo).args(args).output().map_err(spawn_error)?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(exited(args, out.status.code(), &out.stderr))
    }
}

pub(crate) fn run_text(repo: &Path, args: &[&str]) -> Result<String, GitError> {
    Ok(String::from_utf8_lossy(&run(repo, args)?).trim().to_owned())
}

/// A running git whose stdout is read by the caller while stdin and stderr are served on
/// threads, so neither pipe can fill up and stall it.
pub(crate) struct Streaming {
    child: Child,
    args: String,
    stderr: Option<JoinHandle<Vec<u8>>>,
    stdin: Option<JoinHandle<()>>,
}

impl Streaming {
    pub(crate) fn spawn(repo: &Path, args: &[&str], input: Option<Vec<u8>>) -> Result<(Streaming, ChildStdout), GitError> {
        let mut cmd = git(repo);
        cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
        if input.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn().map_err(spawn_error)?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| GitError::Io(std::io::Error::other("no stdout pipe")))?;
        let stderr = child.stderr.take().map(|mut e| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = e.read_to_end(&mut buf);
                buf
            })
        });
        let stdin = match (child.stdin.take(), input) {
            (Some(mut pipe), Some(bytes)) => Some(std::thread::spawn(move || {
                let _ = pipe.write_all(&bytes);
            })),
            _ => None,
        };
        let s = Streaming {
            child,
            args: args.join(" "),
            stderr,
            stdin,
        };
        Ok((s, stdout))
    }

    /// Waits for exit; an error carries git's stderr.
    pub(crate) fn finish(mut self) -> Result<(), GitError> {
        let status = self.child.wait()?;
        if let Some(t) = self.stdin.take() {
            let _ = t.join();
        }
        let stderr = self.stderr.take().and_then(|t| t.join().ok()).unwrap_or_default();
        if status.success() {
            Ok(())
        } else {
            Err(GitError::Exited {
                args: std::mem::take(&mut self.args),
                code: status.code(),
                stderr: String::from_utf8_lossy(&stderr).into_owned(),
            })
        }
    }
}

impl Drop for Streaming {
    fn drop(&mut self) {
        if self.stderr.is_some() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_git_call_pins_the_diff_settings_and_drops_the_context_override() {
        let c = git(Path::new("."));
        let args: Vec<String> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        for setting in PINNED_CONFIG {
            let at = args
                .iter()
                .position(|a| a == setting)
                .unwrap_or_else(|| panic!("{setting} is missing"));
            assert_eq!(args[at - 1], "-c", "{setting}");
        }
        assert!(args.iter().any(|a| a == "diff.algorithm=histogram"));
        assert!(
            c.get_envs().any(|(k, v)| k == "GIT_DIFF_OPTS" && v.is_none()),
            "GIT_DIFF_OPTS is removed"
        );
        assert!(DIFF.contains(&"--diff-algorithm=histogram") && DIFF.contains(&"-O/dev/null"));
    }

    #[test]
    fn every_git_call_takes_no_optional_lock_and_never_prompts() {
        let c = git(Path::new("."));
        let env = |name: &str| {
            c.get_envs()
                .find(|(k, _)| *k == name)
                .and_then(|(_, v)| v)
                .map(|v| v.to_string_lossy().into_owned())
        };
        assert_eq!(env("GIT_OPTIONAL_LOCKS").as_deref(), Some("0"), "a read never writes the index");
        assert_eq!(env("GIT_TERMINAL_PROMPT").as_deref(), Some("0"));
        assert_eq!(env("LC_ALL").as_deref(), Some("C"), "messages are parsed in one language");
    }
}
