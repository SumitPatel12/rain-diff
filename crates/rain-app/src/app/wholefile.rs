//! Whole-file diffs: every unchanged line of a file, or of every file, opened around its changes.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rain_core::{FileDiff, FileStatus};
use rain_git::Resolved;
use rune::Tx;

use super::App;
use crate::load::{self, LARGE_FILE_LINES, Msg};

/// Why a file can't be opened whole, as the status line says it.
fn not_wholeable(f: &FileDiff) -> Option<&'static str> {
    match f.status {
        FileStatus::Added | FileStatus::Deleted => Some("Already the whole file"),
        _ if f.binary => Some("A binary file has no lines to open"),
        _ if f.hunks.is_empty() => Some("No changed lines to open around"),
        _ => None,
    }
}

/// Git reads expand-all runs at once: each is a `git` process, so a few overlap well.
const READERS: usize = 4;

/// One file's whole texts to read: its repo folder and revisions, the file, its key.
type Job = (PathBuf, Resolved, Arc<FileDiff>, String);

impl App {
    /// `z`: shows the current file whole, or folds it back.
    pub fn toggle_whole_file(&mut self, tx: &mut Tx) {
        if let Some(g) = self.current() {
            self.toggle_whole(g, tx);
        }
    }

    /// Whether full-list file `g` shows whole in the diff on screen, or will once its texts arrive.
    pub fn is_whole(&self, g: usize) -> bool {
        self.local(g).is_some_and(|l| self.active().whole(l))
    }

    /// Shows full-list file `g` whole, or folds it back and drops its texts.
    pub fn toggle_whole(&mut self, g: usize, tx: &mut Tx) {
        let Some(local) = self.local(g) else { return };
        let Some(f) = self.diff.files().get(g).cloned() else { return };
        if self.is_whole(g) {
            self.set_whole_everywhere(g, false);
            self.set_texts_everywhere(g, None, None);
            self.status = format!("{}: changes only", f.path);
            self.keep_cursor(local, tx);
            return;
        }
        if let Some(why) = not_wholeable(&f) {
            self.status = why.into();
            return;
        }
        if !self.has_repo(g) {
            self.status = "Whole file needs a git repo (fixture)".into();
            return;
        }
        if self.active().collapsed(local) {
            self.active_mut().set_collapsed(local, false);
        }
        self.set_whole_everywhere(g, true);
        if self.active().expandable(local) {
            self.status = format!("{}: whole file", f.path);
            self.keep_cursor(local, tx);
        } else {
            self.fetch_texts(local);
        }
        if f.lines.len() > LARGE_FILE_LINES {
            self.status = format!("{} lines: opening takes about a second", group(f.lines.len()));
        }
    }

    /// `E`: opens every file of the active diff whole, reading their texts one after another;
    /// files over [`LARGE_FILE_LINES`] lines are skipped.
    pub fn expand_all_context(&mut self, _tx: &mut Tx) {
        let a = self.active();
        let mut picked: Vec<usize> = Vec::new();
        let mut huge = 0;
        for (local, f) in a.files().iter().enumerate() {
            if not_wholeable(f).is_some() || a.whole(local) {
                continue;
            }
            if f.lines.len() > LARGE_FILE_LINES {
                huge += 1;
                continue;
            }
            picked.push(self.global(local));
        }
        let skipped = if huge > 0 {
            format!(", {huge} over {} lines skipped", group(LARGE_FILE_LINES))
        } else {
            String::new()
        };
        if picked.is_empty() {
            self.status = format!("No file left to open whole{skipped}");
            return;
        }
        picked.retain(|&g| self.has_repo(g));
        if picked.is_empty() {
            self.status = "Whole file needs a git repo (fixture)".into();
            return;
        }
        let keys: BTreeSet<(u16, String)> = picked
            .iter()
            .map(|&g| (self.diff.files()[g].repo, self.diff.files()[g].path.clone()))
            .collect();
        let pick = |f: &FileDiff| keys.contains(&(f.repo, f.path.clone()));
        self.diff.set_whole_where(true, pick);
        for v in self.subsets_mut() {
            v.diff.set_whole_where(true, pick);
        }
        let jobs: Vec<Job> = picked
            .iter()
            .filter(|&&g| self.diff.needs_texts(g))
            .filter_map(|&g| self.job(g))
            .collect();
        let n = picked.len();
        let done = format!("Whole file: {n} file{} open{skipped}", if n == 1 { "" } else { "s" });
        if jobs.is_empty() {
            self.status = done;
            return;
        }
        let total = jobs.len();
        match (self.texts_tx.clone(), self.wake.clone()) {
            (Some(sender), Some(wake)) => {
                self.status = format!("Whole file 0/{total}");
                let _ = std::thread::Builder::new().name("rain-whole".into()).spawn(move || {
                    let queue = Mutex::new(jobs.into_iter());
                    let out = Mutex::new((sender, 0usize));
                    std::thread::scope(|sc| {
                        for _ in 0..READERS.min(total) {
                            sc.spawn(|| {
                                loop {
                                    let Some((dir, resolved, f, key)) = queue.lock().ok().and_then(|mut q| q.next()) else {
                                        return;
                                    };
                                    let msg = load::read_texts(&dir, &resolved, &f, key);
                                    let Ok(mut o) = out.lock() else { return };
                                    o.1 += 1;
                                    let _ = o.0.send(msg);
                                    let _ = o.0.send(Msg::Progress(format!("Whole file {}/{total}", o.1)));
                                    wake();
                                }
                            });
                        }
                    });
                    if let Ok(o) = out.lock() {
                        let _ = o.0.send(Msg::Progress(done));
                    }
                    wake();
                });
            }
            _ => {
                for (dir, resolved, f, key) in jobs {
                    self.install_texts(load::read_texts(&dir, &resolved, &f, key));
                }
                self.status = done;
            }
        }
    }

    /// Why full-list file `g` can't be opened whole, if it can't.
    pub(crate) fn whole_blocker(&self, g: usize) -> Option<&'static str> {
        self.diff.files().get(g).and_then(|f| not_wholeable(f))
    }

    /// Whether full-list file `g`'s repo is a git folder its whole texts can be read from.
    pub(crate) fn has_repo(&self, g: usize) -> bool {
        self.diff.files().get(g).is_some_and(|f| {
            let r = f.repo as usize;
            matches!(self.resolved.get(r), Some(Some(_))) && self.repos.get(r).is_some_and(|r| !r.dir.as_os_str().is_empty())
        })
    }

    /// What reading full-list file `g`'s texts needs, when its repo has a git folder.
    fn job(&self, g: usize) -> Option<Job> {
        let f = self.diff.files().get(g)?;
        let r = f.repo as usize;
        let resolved = self.resolved.get(r)?.clone()?;
        let repo = self.repos.get(r).filter(|r| !r.dir.as_os_str().is_empty())?;
        Some((repo.dir.clone(), resolved, Arc::clone(f), self.keys[g].clone()))
    }

    /// Marks full-list file `g` whole (or not) in the full diff and every subset holding it, so
    /// a chapter or filter shows it the same way when its texts arrive.
    pub(crate) fn set_whole_everywhere(&mut self, g: usize, on: bool) {
        self.diff.set_whole(g, on);
        for v in self.subsets_mut() {
            if let Some(l) = v.files.iter().position(|&x| x == g) {
                v.diff.set_whole(l, on);
            }
        }
    }

    /// Gives full-list file `g` its texts in the main diff, and the subsets that show it the
    /// main diff's copy, so the file is copied at most once.
    pub(crate) fn set_texts_everywhere(&mut self, g: usize, old: Option<Arc<str>>, new: Option<Arc<str>>) {
        self.diff.set_texts(g, old, new);
        let Some(shared) = self.diff.files().get(g).cloned() else { return };
        for v in self.subsets_mut() {
            if let Some(l) = v.files.iter().position(|&x| x == g) {
                v.diff.share_file(l, Arc::clone(&shared));
            }
        }
    }

    /// After file `local` grew or shrank, brings the cursor's line back into view if it is there.
    fn keep_cursor(&mut self, local: usize, tx: &mut Tx) {
        if let Some((f, at)) = self.active().cursor()
            && f == local
        {
            self.active_mut().scroll_to_line(f, at, tx);
        }
    }
}

/// `500000` → `500,000`.
fn group(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
