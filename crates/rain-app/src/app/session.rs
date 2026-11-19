//! The session mirror: changes from other threads, mutations, viewed marks and the queue.

use std::collections::{BTreeMap, HashSet};

use rain_comments::ops;
use rain_core::{FileTag, HunkViewed, Session, Thread};

use super::{App, ME};

impl App {
    /// Picks up session changes made elsewhere (HTTP imports, overview updates).
    pub(crate) fn sync_session(&mut self) -> bool {
        let fresh = {
            let mut s = self.shared.lock();
            if s.version == self.seen_version {
                return false;
            }
            // The overview is the biggest member and changes only with its version: while that
            // stands, the mirror's copy is moved over instead of cloning the session's.
            if s.overview_version == self.mirror.overview_version && s.overview.is_some() == self.mirror.overview.is_some() {
                let overview = s.overview.take();
                let mut fresh = s.clone();
                s.overview = overview;
                fresh.overview = self.mirror.overview.take();
                fresh
            } else {
                s.clone()
            }
        };
        let (version, overview_version) = (fresh.version, fresh.overview_version);
        let threads_changed = fresh.threads != self.mirror.threads || fresh.drafts != self.mirror.drafts;
        let notes_changed = super::notes::notes_changed(&self.mirror, &fresh);
        let notes_before = self.mirror.notes.len();
        self.mirror = fresh;
        self.seen_version = version;
        self.recompute_viewed();
        let overview_changed = overview_version != self.seen_overview_version;
        if overview_changed {
            self.seen_overview_version = overview_version;
            self.reload_overview_from_session();
        }
        if notes_changed {
            self.on_notes_changed(notes_before);
        }
        self.after_session_change(threads_changed || overview_changed || notes_changed);
        true
    }

    fn reload_overview_from_session(&mut self) {
        let Some(doc) = self.mirror.overview.clone() else { return };
        let Ok(v) = serde_json::to_value(doc) else { return };
        match rain_overview::load_value(v) {
            Ok(l) => {
                self.overview = Some(l);
                self.overview_issues.clear();
                self.retag();
                if self.in_chapter() {
                    self.enter_chapter(self.chapter);
                }
            }
            Err(issues) => {
                self.overview_issues = issues
                    .into_iter()
                    .map(|i| format!("{} {}: {}", i.code, i.path, i.message))
                    .collect();
            }
        }
    }

    /// Recomputes tags from the overview over the loaded files (reading order is kept).
    fn retag(&mut self) {
        let Some(l) = &self.overview else { return };
        let covs: Vec<_> = self
            .repos
            .iter()
            .enumerate()
            .filter(|(_, r)| l.document.repos.iter().any(|o| o.id == r.id))
            .map(|(i, r)| {
                let files = self.diff.files();
                let paths: Vec<&str> = files.iter().filter(|f| f.repo as usize == i).map(|f| f.path.as_str()).collect();
                let generated: Vec<&str> = files
                    .iter()
                    .filter(|f| f.repo as usize == i && f.generated)
                    .map(|f| f.path.as_str())
                    .collect();
                rain_overview::coverage(&l.document, Some(&r.id), &paths, &generated, &[])
            })
            .collect();
        let mut tags = rain_overview::tags(&l.document, &covs);
        tags.retain(|_, t| *t != FileTag::Uncovered);
        for it in &self.board.items {
            tags.entry(it.key.clone()).or_insert_with(|| it.group.tag());
        }
        for (i, k) in self.keys.iter().enumerate() {
            let f = &self.diff.files()[i];
            tags.entry(k.clone())
                .or_insert(if f.generated { FileTag::Generated } else { FileTag::Review });
        }
        self.tags = tags;
        self.tree.set_tags(&self.tags);
        self.rebuild_overview_views();
    }

    fn after_session_change(&mut self, annotations_changed: bool) {
        if annotations_changed {
            let counts = comment_counts(&self.mirror, self);
            self.tree
                .set_comment_counts(self.keys.iter().map(|k| (k.as_str(), counts.get(k).copied().unwrap_or(0))));
        }
        self.tree
            .set_viewed_many(self.keys.iter().map(String::as_str).zip(self.viewed_files.iter().copied()));
        if let Some(t) = &self.focused_thread
            && ops::thread(&self.mirror, t).is_none()
        {
            self.focused_thread = None;
        }
        self.drop_hidden_reply();
        if annotations_changed {
            self.rebuild_annotations();
        }
    }

    /// Runs `f` on the shared session, schedules a save and refreshes the mirror.
    pub fn mutate<R>(&mut self, f: impl FnOnce(&mut Session) -> R) -> R {
        let r = f(&mut self.shared.lock());
        self.shared.notify_waiters();
        if let Some(a) = &self.autosave {
            a.touch();
        }
        self.sync_session();
        r
    }

    /// Viewed, and the diff hasn't changed since (a changed file reads as unviewed, as on GitHub).
    pub fn is_viewed(&self, file: usize) -> bool {
        self.viewed_files.get(file).copied().unwrap_or(false)
    }

    /// Rebuilds the per-file viewed marks from the mirror; run whenever the mirror or the diff is replaced.
    pub(crate) fn recompute_viewed(&mut self) {
        let marks: HashSet<(&str, &str, &str)> = self
            .mirror
            .viewed
            .iter()
            .map(|v| (v.repo.as_str(), v.file_path.as_str(), v.diff_content_hash.as_str()))
            .collect();
        self.viewed_files = (0..self.diff.files().len())
            .map(|i| marks.contains(&(self.repo_tag(i), self.diff.files()[i].path.as_str(), self.hashes[i].as_str())))
            .collect();
    }

    /// Marks a file viewed or not in the tree and the session.
    pub fn set_viewed(&mut self, file: usize, on: bool) {
        let key = self.keys[file].clone();
        self.tree.set_viewed(&key, on);
        self.persist_viewed(file, on);
    }

    /// The session side of a viewed change the tree already shows; a viewed mark records each
    /// hunk's id and text hash, so a reload can tell which hunks changed since.
    pub fn persist_viewed(&mut self, file: usize, on: bool) {
        let repo = self.repo_tag(file).to_owned();
        let path = self.diff.files()[file].path.clone();
        let hash = self.hashes[file].clone();
        let hunks = if on { self.hunk_marks(file) } else { Vec::new() };
        self.reloader.changed.remove(&file);
        self.fold_viewed(file, on);
        self.mutate(|s| {
            if on {
                ops::set_viewed(s, &repo, &path, &hash) | set_hunks(s, &repo, &path, hunks)
            } else {
                ops::clear_viewed(s, &repo, &path)
            }
        });
    }

    /// A viewed file folds to its header in the diff; unmarking opens it again.
    pub(crate) fn fold_viewed(&mut self, file: usize, on: bool) {
        if let Some(l) = self.local(file) {
            self.active_mut().set_collapsed(l, on);
        }
        if self.subset().is_some() {
            self.diff.set_collapsed(file, on);
        }
    }

    pub fn persist_viewed_key(&mut self, key: &str, on: bool) {
        if let Some(i) = self.index_of(key) {
            self.persist_viewed(i, on);
        }
    }

    /// Marks every file in `keys` viewed in the session, in one change.
    pub fn persist_viewed_keys(&mut self, keys: &[String]) {
        let items: Vec<(String, String, String, Vec<HunkViewed>)> = keys
            .iter()
            .filter_map(|k| self.index_of(k))
            .map(|i| {
                (
                    self.repo_tag(i).to_owned(),
                    self.diff.files()[i].path.clone(),
                    self.hashes[i].clone(),
                    self.hunk_marks(i),
                )
            })
            .collect();
        if items.is_empty() {
            return;
        }
        for k in keys {
            if let Some(i) = self.index_of(k) {
                self.reloader.changed.remove(&i);
            }
        }
        self.mutate(|s| {
            for (repo, path, hash, hunks) in items {
                ops::set_viewed(s, &repo, &path, &hash);
                set_hunks(s, &repo, &path, hunks);
            }
        });
    }

    pub fn waiting_count(&self) -> usize {
        ops::waiting_count(&self.mirror, ME)
    }
}

/// Sets the hunks of a viewed record; true when they changed.
fn set_hunks(s: &mut Session, repo: &str, path: &str, hunks: Vec<HunkViewed>) -> bool {
    match s.viewed.iter_mut().find(|v| v.repo == repo && v.file_path == path) {
        Some(v) if v.hunks != hunks => {
            v.hunks = hunks;
            s.version += 1;
            true
        }
        _ => false,
    }
}

pub(crate) fn file_of_thread(app: &App, t: &Thread) -> Option<usize> {
    if !t.repo.is_empty() {
        return app.index_of(&rain_core::paths::join(&t.repo, &t.file_path));
    }
    if app.repos.len() == 1 {
        let id = &app.repos[0].id;
        return app.index_of(&rain_core::paths::join(id, &t.file_path));
    }
    app.diff.files().iter().position(|f| f.path == t.file_path)
}

pub(crate) fn comment_counts(s: &Session, app: &App) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    for t in &s.threads {
        if let Some(i) = file_of_thread(app, t) {
            *out.entry(app.keys[i].clone()).or_insert(0) += 1;
        }
    }
    out
}
