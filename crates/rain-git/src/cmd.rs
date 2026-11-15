use std::io::{Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::thread::JoinHandle;

use crate::GitError;

/// `git -C <repo>` with output settings pinned so user config cannot change what we parse.
pub(crate) fn git(repo: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("-C")
        .arg(repo)
        .args([
            "-c",
            "core.quotePath=true",
            "-c",
            "diff.noprefix=false",
            "-c",
            "diff.mnemonicPrefix=false",
        ])
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
