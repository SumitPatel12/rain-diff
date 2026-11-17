//! Reload keeping the reading place, what a reload changed (anchors, hunks since viewed), and
//! the HEAD watch with its banner.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rain_comments::ops;
use rain_core::{AnchorState, HunkViewed, Side, ViewedRecord, short_sha};
use rain_diffview::LineRef;
use rain_git::{GitWatch, TargetKind};
use rune::Tx;

use super::place::Captured;
use super::{App, Overlay, Phase};
use crate::load;

/// How long the "Reloaded" message stays.
pub const TOAST_FOR: Duration = Duration::from_secs(8);

/// New commits under the open window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stale {
    pub repo: String,
    pub branch: String,
    pub commits: Option<u32>,
    pub sha: String,
}

impl Stale {
    pub fn message(&self) -> String {
        let short = short_sha(&self.sha);
        match self.commits {
            Some(1) => format!("1 new commit on {} ({short})", self.branch),
            Some(n) => format!("{n} new commits on {} ({short})", self.branch),
            None => format!("{} moved to {short} (history rewritten)", self.branch),
        }
    }
}

/// One watched repo: what the watch compares the target with.
#[derive(Clone, Debug)]
struct Watched {
    id: String,
    dir: PathBuf,
    target: String,
    loaded: String,
    head: String,
}

/// What a reload changed, for its message.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReloadCounts {
    pub moved: usize,
    pub addressed: usize,
    pub orphaned: usize,
    pub changed_files: usize,
}

impl ReloadCounts {
    pub fn message(&self) -> String {
        let mut parts = Vec::new();
        if self.moved > 0 {
            parts.push(format!("{} moved", self.moved));
        }
        if self.addressed > 0 {
            parts.push(format!("{} addressed?", self.addressed));
        }
        if self.orphaned > 0 {
            parts.push(format!("{} orphaned", self.orphaned));
        }
        if self.changed_files > 0 {
            parts.push(format!(
                "{} file{} changed since you viewed",
                self.changed_files,
                if self.changed_files == 1 { "" } else { "s" }
            ));
        }
        match parts.is_empty() {
            true => "Reloaded".into(),
            false => format!("Reloaded · {}", parts.join(" · ")),
        }
    }
}

#[derive(Default)]
pub struct ReloadState {
    watches: Arc<Mutex<Vec<GitWatch>>>,
    watch_started: bool,
    watched: Arc<Mutex<Vec<Watched>>>,
    /// A finished check: `Some(None)` = up to date again.
    found: Arc<Mutex<Option<Option<Stale>>>>,
    /// Each repo's last check since the last install, by folder: a watch checks its own repo.
    checked: Arc<Mutex<HashMap<PathBuf, Option<Stale>>>>,
    pub banner: Option<Stale>,
    pub toast: Option<(String, Instant)>,
    pub counts: Option<ReloadCounts>,
    pub(crate) pending_place: Option<Captured>,
    pub(crate) saved_place: Option<(String, Option<u32>, Side, i64)>,
    /// The file, line and side the place last scheduled a save for.
    pub(crate) touched_place: Option<(String, Option<u32>, Side)>,
    /// The place moved by a scroll alone since then, unsaved.
    pub(crate) place_unsaved: bool,
    installs: u32,
    /// Each thread's anchor state and `addressed?` as of the last install.
    anchors: BTreeMap<String, (AnchorState, bool)>,
    /// Per file, the hunks whose text changed since the file was viewed.
    pub changed: BTreeMap<usize, Vec<usize>>,
}

impl ReloadState {
    /// Installs so far: a new one means new keys even when their count is the same.
    pub(crate) fn installs(&self) -> u32 {
        self.installs
    }
}

impl App {
    /// `Shift+R` and the HEAD-watch banner's action.
    pub fn reload_now(&mut self, _tx: &mut Tx) {
        self.status = "Reloading".into();
        self.reload();
    }

    /// Reloads on the loader thread while the reader stays up; [`App::install`] keeps the place.
    pub(crate) fn reload_in_place(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.texts_tx = Some(tx.clone());
        load::spawn(self.launch.clone(), self.shared.clone(), tx, wake);
    }

    /// Install's first half: where the reader is, unless it was never shown.
    pub(crate) fn before_install(&self) -> Option<Captured> {
        if self.phase != Phase::Ready || self.keys.is_empty() {
            return None;
        }
        self.reloader.pending_place.clone().or_else(|| self.capture_place())
    }

    /// A session with no saved place opens at the list's first file.
    fn first_place(&self) -> Option<Captured> {
        let g = *self.list_order().first()?;
        Some(Captured {
            place: rain_core::ReadingPlace {
                file: self.keys[g].clone(),
                ..Default::default()
            },
            text: None,
            dy: None,
        })
    }

    /// Install's second half: the place to put back (the old one on a reload, the saved one on
    /// open unless a `--goto` says otherwise), artifact survival, what changed, the watch.
    pub(crate) fn after_install(&mut self, before: Option<Captured>, keep: Option<String>) {
        let reload = self.reloader.installs > 0;
        self.reloader.installs += 1;
        let place = match before {
            Some(c) => Some(c),
            None if !reload => self.mirror.place.clone().map(|place| Captured {
                place,
                text: None,
                dy: None,
            }),
            None => None,
        };
        let place = place.or_else(|| (!reload).then(|| self.first_place()).flatten());
        if let Some(c) = place
            && (self.pending_goto.is_none() || self.pending_goto == keep)
            && self.index_of(&c.place.file).is_some()
        {
            self.pending_goto = None;
            self.reloader.pending_place = Some(c);
        }
        if ops::reanchor_targets(&mut self.shared.lock()) > 0 {
            self.shared.lock().bump_comments();
            self.shared.notify_waiters();
            self.sync_session();
        }
        self.reloader.changed = self.changed_since_viewed();
        let anchors = anchor_states(&self.mirror);
        if reload {
            let counts = reload_counts(&self.reloader.anchors, &anchors, self.reloader.changed.len());
            self.reloader.counts = Some(counts);
            self.reloader.toast = Some((counts.message(), Instant::now()));
            self.status = counts.message();
            self.reloader.banner = None;
            self.shared.set_stale(false);
            self.reapply_lens();
        }
        self.reloader.anchors = anchors;
        self.update_watched();
        if let Some(w) = self.wake.clone() {
            self.start_head_watch(w);
        }
    }

    /// Per file, the hunks not among those recorded when it was viewed (files viewed whole
    /// before hunks were recorded, or unchanged, have none).
    fn changed_since_viewed(&self) -> BTreeMap<usize, Vec<usize>> {
        let mut records: HashMap<(&str, &str), &ViewedRecord> = HashMap::new();
        for v in &self.mirror.viewed {
            records.entry((v.repo.as_str(), v.file_path.as_str())).or_insert(v);
        }
        let mut out = BTreeMap::new();
        for g in 0..self.keys.len() {
            let Some(&rec) = records.get(&(self.repo_tag(g), self.diff.files()[g].path.as_str())) else {
                continue;
            };
            if rec.diff_content_hash == self.hashes[g] || rec.hunks.is_empty() {
                continue;
            }
            let seen: HashSet<&str> = rec.hunks.iter().map(|h| h.hash.as_str()).collect();
            let changed: Vec<usize> = (0..self.diff.files()[g].hunks.len())
                .filter(|&h| self.diff.hunk_hash(g, h).is_none_or(|x| !seen.contains(x.as_str())))
                .collect();
            if !changed.is_empty() {
                out.insert(g, changed);
            }
        }
        out
    }

    /// Every hunk of full-list file `g` by id and text hash, for a viewed mark.
    pub(crate) fn hunk_marks(&self, g: usize) -> Vec<HunkViewed> {
        (0..self.diff.files().get(g).map_or(0, |f| f.hunks.len()))
            .filter_map(|h| {
                Some(HunkViewed {
                    id: self.diff.hunk_id(g, h)?,
                    hash: self.diff.hunk_hash(g, h)?,
                })
            })
            .collect()
    }

    /// The "changed since you viewed" chip's jump: the first changed hunk of `g`.
    pub fn goto_changed_hunk(&mut self, g: usize, tx: &mut Tx) {
        let Some(&h) = self.reloader.changed.get(&g).and_then(|v| v.first()) else {
            return;
        };
        let Some(hunk) = self.diff.files().get(g).and_then(|f| f.hunks.get(h)) else {
            return;
        };
        let at = if hunk.new_count == 0 {
            LineRef::new(Side::Old, hunk.old_start)
        } else {
            LineRef::new(Side::New, hunk.new_start)
        };
        self.goto_line(g, at, tx);
    }

    /// Watches each repo's `.git` for new commits, starting on a thread (the watcher scans
    /// `refs/`); the banner appears on the frame after a check finds one.
    pub fn start_head_watch(&mut self, wake: Arc<dyn Fn() + Send + Sync>) {
        if self.reloader.watch_started {
            return;
        }
        self.reloader.watch_started = true;
        let watched = self.reloader.watched.clone();
        let found = self.reloader.found.clone();
        let checked = self.reloader.checked.clone();
        let watches = self.reloader.watches.clone();
        let dirs: Vec<PathBuf> = watched
            .lock()
            .map(|w| w.iter().map(|r| r.dir.clone()).collect())
            .unwrap_or_default();
        let _ = std::thread::Builder::new().name("rain-watch-start".into()).spawn(move || {
            for dir in dirs {
                let check = Check {
                    watched: watched.clone(),
                    found: found.clone(),
                    checked: checked.clone(),
                };
                if let Some(w) = watch_repo(&dir, check, wake.clone())
                    && let Ok(mut all) = watches.lock()
                {
                    all.push(w);
                }
            }
        });
    }

    fn update_watched(&mut self) {
        let list: Vec<Watched> = self
            .repos
            .iter()
            .zip(&self.resolved)
            .filter_map(|(r, res)| {
                let res = res.as_ref()?;
                let head = match res.kind {
                    TargetKind::Commit => String::new(),
                    _ => rain_git::head_sha(&r.dir).unwrap_or_default(),
                };
                Some(Watched {
                    id: r.id.clone(),
                    dir: r.dir.clone(),
                    target: r.target.clone(),
                    loaded: res.target.clone(),
                    head,
                })
            })
            .collect();
        if let Ok(mut w) = self.reloader.watched.lock() {
            *w = list;
        }
        if let Ok(mut c) = self.reloader.checked.lock() {
            c.clear();
        }
    }

    pub fn head_watch_count(&self) -> usize {
        self.reloader.watches.lock().map_or(0, |w| w.len())
    }

    /// Frame work for this module: the place to restore, the watch's result, the message's end,
    /// a lens a filter replaced, and saving the place.
    pub(crate) fn rounds_poll(&mut self, tx: &mut Tx) -> bool {
        if self.phase != Phase::Ready {
            return false;
        }
        let mut changed = false;
        if let Some(c) = self.reloader.pending_place.take() {
            self.restore_place(&c, tx);
            changed = true;
        }
        let found = self.reloader.found.lock().ok().and_then(|mut f| f.take());
        if let Some(stale) = found
            && stale != self.reloader.banner
        {
            self.shared.set_stale(stale.is_some());
            self.reloader.banner = stale;
            changed = true;
        }
        if self.reloader.toast.as_ref().is_some_and(|(_, at)| at.elapsed() >= TOAST_FOR) {
            self.reloader.toast = None;
            changed = true;
        }
        changed |= self.check_lens();
        self.save_place();
        changed
    }

    /// The reload message's "Show": the Conversation, where moved and orphaned threads are.
    pub fn show_reload_changes(&mut self, _tx: &mut Tx) {
        self.reloader.toast = None;
        self.open_overlay(Overlay::Conversation);
    }

    pub fn dismiss_reload_toast(&mut self, _tx: &mut Tx) {
        self.reloader.toast = None;
    }
}

/// What a repo's watch reads and writes.
#[derive(Clone)]
struct Check {
    watched: Arc<Mutex<Vec<Watched>>>,
    found: Arc<Mutex<Option<Option<Stale>>>>,
    checked: Arc<Mutex<HashMap<PathBuf, Option<Stale>>>>,
}

impl Check {
    /// Compares `dir`'s targets with what was loaded, then reports the first stale repo of every
    /// repo's last check, in the watched order.
    fn run(&self, dir: &std::path::Path) {
        let list = self.watched.lock().map(|w| w.clone()).unwrap_or_default();
        let mine = list.iter().filter(|r| r.dir == dir).find_map(|r| {
            let m = rain_git::target_moved(&r.dir, &r.target, &r.loaded, &r.head).ok()??;
            Some(Stale {
                repo: r.id.clone(),
                branch: m.branch,
                commits: m.commits,
                sha: m.to,
            })
        });
        let Ok(mut checked) = self.checked.lock() else { return };
        checked.insert(dir.to_owned(), mine);
        let stale = list.iter().find_map(|r| checked.get(&r.dir).cloned().flatten());
        if let Ok(mut f) = self.found.lock() {
            *f = Some(stale);
        }
    }
}

/// One repo's watch; its check looks at that repo only.
fn watch_repo(dir: &std::path::Path, check: Check, wake: Arc<dyn Fn() + Send + Sync>) -> Option<GitWatch> {
    let own = dir.to_owned();
    let check = move || {
        check.run(&own);
        wake();
    };
    match rain_git::watch_git(dir, check) {
        Ok(w) => Some(w),
        Err(e) => {
            eprintln!("Rain: not watching {} for new commits ({e}); Shift+R reloads", dir.display());
            None
        }
    }
}

fn anchor_states(s: &rain_core::Session) -> BTreeMap<String, (AnchorState, bool)> {
    s.threads
        .iter()
        .map(|t| (t.id.clone(), (t.placement(), ops::addressed_suggested(t))))
        .collect()
}

fn reload_counts(
    before: &BTreeMap<String, (AnchorState, bool)>,
    after: &BTreeMap<String, (AnchorState, bool)>,
    changed_files: usize,
) -> ReloadCounts {
    let mut c = ReloadCounts {
        changed_files,
        ..ReloadCounts::default()
    };
    for (id, &(state, addressed)) in after {
        let (was, was_addressed) = before.get(id).copied().unwrap_or_default();
        if state == AnchorState::Moved && was != AnchorState::Moved {
            c.moved += 1;
        }
        if state == AnchorState::Orphaned && was != AnchorState::Orphaned {
            c.orphaned += 1;
        }
        if addressed && !was_addressed {
            c.addressed += 1;
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn watched(id: &str, dir: &str) -> Watched {
        Watched {
            id: id.into(),
            dir: dir.into(),
            target: "HEAD".into(),
            loaded: "0".repeat(40),
            head: String::new(),
        }
    }

    #[test]
    fn a_repos_check_keeps_the_others_last_results() {
        let missing = std::env::temp_dir().join("rain-no-such-repo");
        let a = missing.join("a").to_string_lossy().into_owned();
        let b = missing.join("b").to_string_lossy().into_owned();
        let check = Check {
            watched: Arc::new(Mutex::new(vec![watched("a", &a), watched("b", &b)])),
            found: Arc::default(),
            checked: Arc::default(),
        };
        let stale_b = Stale {
            repo: "b".into(),
            branch: "main".into(),
            commits: Some(2),
            sha: "f".repeat(40),
        };
        check.checked.lock().unwrap().insert(PathBuf::from(&b), Some(stale_b.clone()));
        check.run(Path::new(&a));
        assert_eq!(*check.found.lock().unwrap(), Some(Some(stale_b)), "a's check doesn't re-run b's");
        assert_eq!(check.checked.lock().unwrap().get(&PathBuf::from(&a)), Some(&None));
        check.checked.lock().unwrap().clear();
        check.run(Path::new(&a));
        assert_eq!(*check.found.lock().unwrap(), Some(None));
    }
}
