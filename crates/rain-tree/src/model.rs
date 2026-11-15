use std::collections::{BTreeMap, BTreeSet, HashMap};

use rain_core::{FileDiff, FileStatus, FileTag, RepoInfo};
use rune::{TextEdit, Tx};
use rune_kit::{TreeModel, natural_cmp};

/// The element key of the tree's root, which is also the scope [`TreeState::reveal`] scrolls in.
pub const TREE_KEY: &str = "rain-tree";

/// What a node of the tree is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// A repository root, by its index in the diff's repos.
    Repo(u16),
    Dir,
    /// A file, by its index in [`TreeState::files`].
    File(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// The row's name: a file name, a folder (`a/b` when compacted) or a repo id.
    pub name: String,
    pub kind: NodeKind,
}

impl Node {
    pub fn label(&self) -> &str {
        &self.name
    }
}

/// One changed file as the tree shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    /// `<repoId>/<path>`: the tree node id and the key every callback reports.
    pub key: String,
    pub repo: u16,
    pub path: String,
    pub old_path: Option<String>,
    pub name: String,
    pub status: FileStatus,
    pub binary: bool,
    pub generated: bool,
    pub additions: u32,
    pub deletions: u32,
    pub comments: u32,
    pub tag: FileTag,
    pub viewed: bool,
    /// The key, lowercased once for the text filter.
    search: String,
    /// Where the file name starts in `search`.
    name_at: usize,
}

/// What the tree shows. An empty `tags` set means every tag; an empty `repos` set every repo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeFilter {
    pub tags: BTreeSet<FileTag>,
    /// Repo indexes, as in [`FileEntry::repo`].
    pub repos: BTreeSet<u16>,
    pub unviewed_only: bool,
    pub commented_only: bool,
    /// Whitespace-separated terms; each must match the path (see [`text_matches`]).
    pub text: String,
}

impl TreeFilter {
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty() && self.repos.is_empty() && !self.unviewed_only && !self.commented_only && self.text.trim().is_empty()
    }

    /// The single tag the filter narrows to, when it narrows to exactly one.
    pub fn single_tag(&self) -> Option<FileTag> {
        match self.tags.len() {
            1 => self.tags.iter().next().copied(),
            _ => None,
        }
    }
}

/// Viewed counts for the header: overall and for must-review files, ignoring the filter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    pub viewed: u32,
    pub total: u32,
    pub must_viewed: u32,
    pub must_total: u32,
}

/// The file tree's data: the files, their tags, viewed marks and comment counts, the filter
/// and the outline (a rune-kit [`TreeModel`]) the view draws.
pub struct TreeState {
    pub files: Vec<FileEntry>,
    pub model: TreeModel<Node>,
    pub filter: TreeFilter,
    /// The text box bound to `filter.text`.
    pub filter_edit: TextEdit,
    by_key: HashMap<String, u32>,
    matches: Vec<bool>,
    /// Per tag: files that pass every filter but the tag one (what the chip would show).
    tag_counts: BTreeMap<FileTag, u32>,
    /// Per repo index: files that pass every filter but the repo one.
    repo_counts: BTreeMap<u16, u32>,
    /// The repo roots in tree order: (repo index, shown name).
    repos: Vec<(u16, String)>,
    /// Per tag over all files: (viewed, total).
    tag_progress: BTreeMap<FileTag, (u32, u32)>,
    /// Per folder or repo node: (viewed, total) over the files the filter shows under it.
    folder_counts: HashMap<String, (u32, u32)>,
    progress: Progress,
    /// Points of the tree's viewport, for Page Up and Page Down.
    pub(crate) viewport: f32,
}

impl TreeState {
    /// Builds the tree: one root per repo in launch order, folders before files in natural
    /// order, single-folder chains compacted, everything open. A file missing from `tags` is
    /// `Generated` when git says so, else `Review`.
    pub fn build(
        files: &[impl std::borrow::Borrow<FileDiff>],
        repos: &[RepoInfo],
        tags: &BTreeMap<String, FileTag>,
        viewed: impl Fn(&str) -> bool,
        comment_counts: &BTreeMap<String, u32>,
    ) -> TreeState {
        let entries: Vec<FileEntry> = files
            .iter()
            .map(|f| {
                let f: &FileDiff = f.borrow();
                let key = f.display_path(repos);
                let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_owned();
                let tag = tags
                    .get(&key)
                    .copied()
                    .unwrap_or(if f.generated { FileTag::Generated } else { FileTag::Review });
                let search = key.to_lowercase();
                let name_at = search.rfind('/').map_or(0, |i| i + 1);
                FileEntry {
                    viewed: viewed(&key),
                    comments: comment_counts.get(&key).copied().unwrap_or(0),
                    key,
                    repo: f.repo,
                    path: f.path.clone(),
                    old_path: f.old_path.clone(),
                    name,
                    status: f.status,
                    binary: f.binary,
                    generated: f.generated,
                    additions: f.additions,
                    deletions: f.deletions,
                    tag,
                    search,
                    name_at,
                }
            })
            .collect();
        let model = outline(&entries, repos);
        let by_key = entries.iter().enumerate().map(|(i, e)| (e.key.clone(), i as u32)).collect();
        let roots = model
            .children(None)
            .filter_map(|id| match model.get(id) {
                Some(Node {
                    name,
                    kind: NodeKind::Repo(r),
                }) => Some((*r, name.clone())),
                _ => None,
            })
            .collect();
        let mut s = TreeState {
            repo_counts: BTreeMap::new(),
            repos: roots,
            matches: vec![true; entries.len()],
            files: entries,
            model,
            filter: TreeFilter::default(),
            filter_edit: TextEdit::default(),
            by_key,
            tag_counts: BTreeMap::new(),
            tag_progress: BTreeMap::new(),
            folder_counts: HashMap::new(),
            progress: Progress::default(),
            viewport: 0.0,
        };
        s.model.expand_all();
        s.refilter();
        s
    }

    pub fn file(&self, key: &str) -> Option<&FileEntry> {
        self.by_key.get(key).map(|&i| &self.files[i as usize])
    }

    /// Applies `filter`: keeps the matching files and the folders above them, and recounts the
    /// chips, the folder checkboxes and the progress. An empty filter restores the folders'
    /// own open state.
    pub fn apply(&mut self, filter: &TreeFilter) {
        if &self.filter != filter {
            self.filter = filter.clone();
        }
        if self.filter_edit.text != filter.text {
            self.filter_edit.text = filter.text.clone();
            let end = self.filter_edit.text.len();
            self.filter_edit.set_caret(end);
        }
        self.refilter();
    }

    fn refilter(&mut self) {
        let terms: Vec<String> = self.filter.text.split_whitespace().map(str::to_lowercase).collect();
        let f = &self.filter;
        let mut counts: BTreeMap<FileTag, u32> = FileTag::ALL.iter().map(|&t| (t, 0)).collect();
        let mut repo_counts: BTreeMap<u16, u32> = self.repos.iter().map(|&(r, _)| (r, 0)).collect();
        for (i, e) in self.files.iter().enumerate() {
            let rest = (!f.unviewed_only || !e.viewed) && (!f.commented_only || e.comments > 0) && text_matches(e, &terms);
            let repo_ok = f.repos.is_empty() || f.repos.contains(&e.repo);
            let tag_ok = f.tags.is_empty() || f.tags.contains(&e.tag);
            if rest && repo_ok {
                *counts.entry(e.tag).or_default() += 1;
            }
            if rest && tag_ok {
                *repo_counts.entry(e.repo).or_default() += 1;
            }
            self.matches[i] = rest && repo_ok && tag_ok;
        }
        self.tag_counts = counts;
        self.repo_counts = repo_counts;
        if self.filter.is_empty() {
            self.model.clear_filter();
        } else {
            let matches = &self.matches;
            self.model.filter(|n| matches!(n.kind, NodeKind::File(i) if matches[i as usize]));
        }
        self.recount();
    }

    /// Recounts progress, tag progress and folder checkbox counts from scratch.
    fn recount(&mut self) {
        let mut p = Progress::default();
        let mut tags: BTreeMap<FileTag, (u32, u32)> = FileTag::ALL.iter().map(|&t| (t, (0, 0))).collect();
        let mut folders: HashMap<String, (u32, u32)> = HashMap::new();
        for (i, e) in self.files.iter().enumerate() {
            let v = u32::from(e.viewed);
            p.total += 1;
            p.viewed += v;
            if e.tag == FileTag::MustReview {
                p.must_total += 1;
                p.must_viewed += v;
            }
            let t = tags.entry(e.tag).or_default();
            t.0 += v;
            t.1 += 1;
            if self.matches[i] {
                let mut at = self.model.parent(&e.key);
                while let Some(id) = at {
                    match folders.get_mut(id) {
                        Some(c) => *c = (c.0 + v, c.1 + 1),
                        None => {
                            folders.insert(id.to_owned(), (v, 1));
                        }
                    }
                    at = self.model.parent(id);
                }
            }
        }
        self.progress = p;
        self.tag_progress = tags;
        self.folder_counts = folders;
    }

    pub fn progress(&self) -> Progress {
        self.progress
    }

    /// Files carrying `tag` that pass the rest of the filter: the chip's count.
    pub fn tag_count(&self, tag: FileTag) -> u32 {
        self.tag_counts.get(&tag).copied().unwrap_or(0)
    }

    /// Files of repo `repo` that pass the rest of the filter: the repo chip's count.
    pub fn repo_count(&self, repo: u16) -> u32 {
        self.repo_counts.get(&repo).copied().unwrap_or(0)
    }

    /// The repo roots in tree order, as (repo index, shown name); only repos with files.
    pub fn repos(&self) -> &[(u16, String)] {
        &self.repos
    }

    /// (viewed, total) of every file carrying `tag`, whatever the filter.
    pub fn tag_progress(&self, tag: FileTag) -> (u32, u32) {
        self.tag_progress.get(&tag).copied().unwrap_or((0, 0))
    }

    /// (viewed, total) of the shown files under a folder or repo node.
    pub fn folder_counts(&self, id: &str) -> (u32, u32) {
        self.folder_counts.get(id).copied().unwrap_or((0, 0))
    }

    /// Whether the filter shows the file `key`.
    pub fn is_shown(&self, key: &str) -> bool {
        self.by_key.get(key).is_some_and(|&i| self.matches[i as usize])
    }

    /// The keys of the files the filter shows, in tree order.
    pub fn shown_keys(&self) -> Vec<&str> {
        (0..self.model.visible_len())
            .filter_map(|r| self.model.row(r))
            .filter_map(|(n, _)| match n.kind {
                NodeKind::File(i) if self.matches[i as usize] => Some(self.files[i as usize].key.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Sets a file's viewed mark; returns whether it changed. Under an unviewed-only filter the
    /// file stays until the filter is applied again, so a mark doesn't yank the row away.
    pub fn set_viewed(&mut self, key: &str, on: bool) -> bool {
        let Some(&i) = self.by_key.get(key) else { return false };
        let e = &mut self.files[i as usize];
        if e.viewed == on {
            return false;
        }
        e.viewed = on;
        self.recount();
        true
    }

    /// Sets the viewed mark of every shown file under `id` (a folder or repo) to `on`, or of
    /// `id` itself when it is a file. Returns the keys that changed.
    pub fn set_viewed_under(&mut self, id: &str, on: bool) -> Vec<String> {
        let targets: Vec<u32> = self
            .model
            .subtree(id)
            .filter_map(|n| match n.kind {
                NodeKind::File(i) if self.matches[i as usize] => Some(i),
                _ => None,
            })
            .collect();
        self.mark(targets, on)
    }

    /// Marks every file carrying `tag` viewed; returns the keys that changed.
    pub fn mark_tag_viewed(&mut self, tag: FileTag) -> Vec<String> {
        let targets = (0..self.files.len() as u32)
            .filter(|&i| self.files[i as usize].tag == tag)
            .collect();
        self.mark(targets, true)
    }

    fn mark(&mut self, targets: Vec<u32>, on: bool) -> Vec<String> {
        let mut changed = Vec::new();
        for i in targets {
            let e = &mut self.files[i as usize];
            if e.viewed != on {
                e.viewed = on;
                changed.push(e.key.clone());
            }
        }
        if !changed.is_empty() {
            self.recount();
        }
        changed
    }

    pub fn set_comment_count(&mut self, key: &str, n: u32) {
        if let Some(&i) = self.by_key.get(key) {
            self.files[i as usize].comments = n;
            if self.filter.commented_only {
                self.refilter();
            }
        }
    }

    /// Replaces every file's tag (an overview arrived or changed) and re-applies the filter.
    pub fn set_tags(&mut self, tags: &BTreeMap<String, FileTag>) {
        for e in &mut self.files {
            e.tag = tags
                .get(&e.key)
                .copied()
                .unwrap_or(if e.generated { FileTag::Generated } else { FileTag::Review });
        }
        self.refilter();
    }

    /// Opens the folders above `key`, selects it and scrolls to it, as the tree following the
    /// diff does. Shows the file even when the filter hides it.
    pub fn reveal(&mut self, key: &str, tx: &mut Tx) -> bool {
        self.model.reveal(key, tx)
    }

    /// The file under the keyboard cursor.
    pub fn cursor_file(&self) -> Option<&FileEntry> {
        let id = self.model.cursor()?;
        match self.model.get(id)?.kind {
            NodeKind::File(i) => Some(&self.files[i as usize]),
            _ => None,
        }
    }
}

/// Whether every term matches the entry: as a substring of its path, or as a subsequence of
/// its file name (`vouchers` finds `VouchersController.cs` and `voucher.service.ts`). Terms are lowercase.
pub fn text_matches(e: &FileEntry, terms: &[String]) -> bool {
    terms
        .iter()
        .all(|t| e.search.contains(t.as_str()) || is_subsequence(t, &e.search[e.name_at..]))
}

fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

fn outline(files: &[FileEntry], repos: &[RepoInfo]) -> TreeModel<Node> {
    #[derive(Default)]
    struct Dir {
        dirs: Vec<(String, Dir)>,
        index: HashMap<String, usize>,
        files: Vec<u32>,
    }
    impl Dir {
        fn dir(&mut self, name: &str) -> &mut Dir {
            let i = match self.index.get(name) {
                Some(&i) => i,
                None => {
                    self.dirs.push((name.to_owned(), Dir::default()));
                    self.index.insert(name.to_owned(), self.dirs.len() - 1);
                    self.dirs.len() - 1
                }
            };
            &mut self.dirs[i].1
        }
    }
    fn emit(model: &mut TreeModel<Node>, files: &[FileEntry], parent: Option<&str>, prefix: &str, mut dir: Dir) {
        dir.dirs.sort_by(|a, b| natural_cmp(&a.0, &b.0));
        for (mut name, mut d) in dir.dirs {
            let mut id = rain_core::paths::join(prefix, &name);
            while d.files.is_empty() && d.dirs.len() == 1 {
                let (n, inner) = d.dirs.pop().unwrap_or_default();
                name = format!("{name}/{n}");
                id = format!("{id}/{n}");
                d = inner;
            }
            model.insert(parent, id.clone(), Node { name, kind: NodeKind::Dir });
            emit(model, files, Some(&id), &id, d);
        }
        dir.files
            .sort_by(|&a, &b| natural_cmp(&files[a as usize].name, &files[b as usize].name));
        for i in dir.files {
            let e = &files[i as usize];
            model.insert(
                parent,
                e.key.clone(),
                Node {
                    name: e.name.clone(),
                    kind: NodeKind::File(i),
                },
            );
        }
    }

    let mut model = TreeModel::new().key(TREE_KEY);
    let mut groups: Vec<Dir> = (0..=repos.len()).map(|_| Dir::default()).collect();
    for (i, e) in files.iter().enumerate() {
        let g = (e.repo as usize).min(repos.len());
        let mut parts: Vec<&str> = e.path.split('/').filter(|p| !p.is_empty()).collect();
        parts.pop();
        let dir = parts.into_iter().fold(&mut groups[g], |d, p| d.dir(p));
        dir.files.push(i as u32);
    }
    let orphans = groups.pop().unwrap_or_default();
    for (r, (repo, group)) in repos.iter().zip(groups).enumerate() {
        if group.files.is_empty() && group.dirs.is_empty() {
            continue;
        }
        let (id, name) = if repo.id.is_empty() {
            (
                format!("#repo{r}"),
                if repo.name.is_empty() {
                    format!("repo {}", r + 1)
                } else {
                    repo.name.clone()
                },
            )
        } else {
            (repo.id.clone(), repo.id.clone())
        };
        model.insert(
            None,
            id.clone(),
            Node {
                name,
                kind: NodeKind::Repo(r as u16),
            },
        );
        emit(&mut model, files, Some(&id), &repo.id, group);
    }
    emit(&mut model, files, None, "", orphans);
    model
}
