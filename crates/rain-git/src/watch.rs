//! Watches a repository's `HEAD`, `refs/`, `packed-refs` and `index` (never the worktree) and
//! calls back once a burst of changes settles, so an open window can offer a reload.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify_debouncer_full::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::GitError;
use crate::repo::git_dirs;

/// The debouncer's tick.
pub const TICK: Duration = Duration::from_millis(250);
/// Quiet ticks before a burst is flushed.
pub const QUIET_TICKS: u32 = 3;
/// The callback runs at least this often, so a missed or starved event still surfaces.
pub const CAP: Duration = Duration::from_secs(60);

const NAMES: [&str; 4] = ["HEAD", "index", "packed-refs", "ORIG_HEAD"];

/// A running watch; dropping it stops the watcher and its thread.
pub struct GitWatch {
    debouncer: Option<Debouncer<RecommendedWatcher, RecommendedCache>>,
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
        if let Some(d) = self.debouncer.take() {
            d.stop_nonblocking();
        }
        // The thread sees `stop` within a tick; nothing waits for it.
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

/// Calls `on_change` on a background thread after `HEAD`, a ref or the index changed (debounced:
/// [`QUIET_TICKS`] quiet [`TICK`]s), and at least every [`CAP`].
pub fn watch_git(repo: &Path, on_change: impl Fn() + Send + 'static) -> Result<GitWatch, GitError> {
    let (git_dir, common) = git_dirs(repo)?;
    let refs = common.join("refs");
    let (tx, rx) = channel::<()>();
    let filter_refs = refs.clone();
    let mut debouncer = new_debouncer(TICK * QUIET_TICKS, Some(TICK), move |res: DebounceEventResult| {
        if let Ok(events) = res
            && events.iter().any(|e| e.paths.iter().any(|p| relevant(p, &filter_refs)))
        {
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
        debouncer.watch(&p, mode).map_err(|e| watch_error(repo, e))?;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let (stop2, paused2) = (stop.clone(), paused.clone());
    let thread = std::thread::Builder::new().name("rain-git-watch".into()).spawn(move || {
        let mut pending = false;
        let mut last = Instant::now();
        loop {
            let hit = match rx.recv_timeout(TICK) {
                Ok(()) => true,
                Err(RecvTimeoutError::Timeout) => false,
                Err(RecvTimeoutError::Disconnected) => return,
            };
            if stop2.load(Ordering::SeqCst) {
                return;
            }
            pending |= hit || last.elapsed() >= CAP;
            if pending && !paused2.load(Ordering::SeqCst) {
                pending = false;
                last = Instant::now();
                on_change();
            }
        }
    })?;
    Ok(GitWatch {
        debouncer: Some(debouncer),
        paused,
        stop,
        thread: Some(thread),
    })
}

fn watch_error(repo: &Path, e: impl std::fmt::Display) -> GitError {
    GitError::Io(std::io::Error::other(format!("watching {}: {e}", repo.display())))
}
