//! Watches a repository's `HEAD`, `refs/`, `packed-refs` and `index` (never the worktree) and
//! calls back once a burst of changes settles, so an open window can offer a reload.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::GitError;
use crate::repo::git_dirs;

/// A burst of changes is over once this long passes without another.
pub const QUIET: Duration = Duration::from_millis(750);
/// How often a change held by a pause checks for `resume`.
pub const TICK: Duration = Duration::from_millis(250);
/// The callback runs at least this often, so a missed or starved event still surfaces.
pub const CAP: Duration = Duration::from_secs(60);

const NAMES: [&str; 4] = ["HEAD", "index", "packed-refs", "ORIG_HEAD"];

/// A running watch; dropping it stops the watcher and its thread.
pub struct GitWatch {
    watcher: Option<RecommendedWatcher>,
    paused: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl GitWatch {
    /// While paused, changes are noted but the callback waits until [`GitWatch::resume`].
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }
}

impl Drop for GitWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Dropping the watcher drops its sender, which wakes the thread to see `stop`; nothing
        // waits for it.
        drop(self.watcher.take());
        drop(self.thread.take());
    }
}

fn relevant(path: &Path, refs: &Path) -> bool {
    if path.starts_with(refs) {
        return true;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let name = name.strip_suffix(".lock").unwrap_or(name);
    NAMES.contains(&name)
}

/// Calls `on_change` on a background thread after `HEAD`, a ref or the index changed (once
/// [`QUIET`] passes without another change), and at least every [`CAP`]. Idle, nothing wakes
/// between changes but the cap.
pub fn watch_git(repo: &Path, on_change: impl Fn() + Send + 'static) -> Result<GitWatch, GitError> {
    let (git_dir, common) = git_dirs(repo)?;
    let refs = common.join("refs");
    let (tx, rx) = channel::<()>();
    let filter_refs = refs.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok_and(|e| e.paths.iter().any(|p| relevant(p, &filter_refs))) {
            let _ = tx.send(());
        }
    })
    .map_err(|e| watch_error(repo, e))?;
    let mut roots: Vec<(PathBuf, RecursiveMode)> = vec![(git_dir.clone(), RecursiveMode::NonRecursive)];
    if refs.is_dir() {
        roots.push((refs, RecursiveMode::Recursive));
    }
    if common != git_dir {
        roots.push((common, RecursiveMode::NonRecursive));
    }
    for (p, mode) in roots {
        watcher.watch(&p, mode).map_err(|e| watch_error(repo, e))?;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let (stop2, paused2) = (stop.clone(), paused.clone());
    let thread = std::thread::Builder::new().name("rain-git-watch".into()).spawn(move || {
        // `burst`: a change arrived and the quiet after it hasn't passed yet.
        let (mut burst, mut pending) = (false, false);
        let mut last = Instant::now();
        loop {
            // Idle, it sleeps until the cap; a change held by a pause is retried every tick so
            // `resume` takes effect promptly.
            let wait = match (burst, pending) {
                (true, _) => QUIET,
                (false, true) => TICK,
                (false, false) => CAP.saturating_sub(last.elapsed()),
            };
            match rx.recv_timeout(wait) {
                Ok(()) => burst = true,
                Err(RecvTimeoutError::Timeout) => pending |= std::mem::take(&mut burst),
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if stop2.load(Ordering::SeqCst) {
                return;
            }
            pending |= last.elapsed() >= CAP;
            if pending && !paused2.load(Ordering::SeqCst) {
                pending = false;
                last = Instant::now();
                on_change();
            }
        }
    })?;
    Ok(GitWatch {
        watcher: Some(watcher),
        paused,
        stop,
        thread: Some(thread),
    })
}

fn watch_error(repo: &Path, e: impl std::fmt::Display) -> GitError {
    GitError::Io(std::io::Error::other(format!("watching {}: {e}", repo.display())))
}
