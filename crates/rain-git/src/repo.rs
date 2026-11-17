use std::collections::{BTreeSet, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::{Component, Path, PathBuf};

use rain_core::{FileDiff, FileDiffBuilder, FileStatus, LineKind, RepoLaunch, Side};
use sha2::{Digest, Sha256};

use crate::GitError;
use crate::cmd::{self, Streaming};
use crate::generated::{self, HEADER_BYTES, HEADER_LINES};
use crate::parse::{self, Patch};

/// What the diff's new side is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetKind {
    Commit,
    /// `.` / `WORKTREE`: all uncommitted changes against the base.
    WorkingTree,
    /// `working`: unstaged changes (working tree against the index).
    Unstaged,
    /// `staged`: the index against the base.
    Staged,
}

impl TargetKind {
    pub fn of(target: &str) -> TargetKind {
        match target {
            "." => TargetKind::WorkingTree,
            "working" => TargetKind::Unstaged,
            "staged" => TargetKind::Staged,
            t if t.eq_ignore_ascii_case("worktree") => TargetKind::WorkingTree,
            _ => TargetKind::Commit,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    /// Full commit id of the base (the merge base when one was used).
    pub base: String,
    /// Full commit id, or the special target as given (`.`, `working`, `staged`, `WORKTREE`).
    pub target: String,
    pub merge_base_used: bool,
    pub working_tree: bool,
    pub kind: TargetKind,
}

impl Resolved {
    /// The revision [`blob`] reads for one side of the diff.
    pub fn rev(&self, side: Side) -> &str {
        match side {
            Side::Old if self.kind == TargetKind::Unstaged => "staged",
            Side::Old => &self.base,
            Side::New => &self.target,
        }
    }

    fn range_args(&self) -> Vec<&str> {
        match self.kind {
            TargetKind::Commit => vec![&self.base, &self.target],
            TargetKind::WorkingTree => vec![&self.base],
            TargetKind::Unstaged => vec![],
            TargetKind::Staged => vec!["--cached", &self.base],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffOpts {
    pub ignore_whitespace: bool,
    pub include_untracked: bool,
    pub context: u32,
    /// Concurrent `git diff` processes over disjoint path groups; 0 = one per core (max 8),
    /// 1 = a single process.
    pub jobs: usize,
}

impl Default for DiffOpts {
    fn default() -> Self {
        DiffOpts {
            ignore_whitespace: false,
            include_untracked: false,
            context: 3,
            jobs: 0,
        }
    }
}

/// Below this many changed files one `git diff` is cheaper than listing them first.
const PARALLEL_MIN_FILES: usize = 16;
/// Keeps each process's pathspec well under Windows' 32 767-character command line.
const MAX_PATHSPEC_BYTES: usize = 24 * 1024;

fn check_repo(repo: &Path) -> Result<(), GitError> {
    match cmd::run(repo, &["rev-parse", "--git-dir"]) {
        Ok(_) => Ok(()),
        Err(GitError::Exited { stderr, .. }) => Err(GitError::NotARepo {
            dir: repo.to_path_buf(),
            stderr,
        }),
        Err(e) => Err(e),
    }
}

/// The working tree's top folder for `dir` (a checkout or any folder inside one); `dir` itself
/// when git can't tell (a bare repository, or no repository: [`resolve`] reports that). Every
/// path git prints is relative to it.
pub fn toplevel(dir: &Path) -> Result<PathBuf, GitError> {
    match cmd::run_text(dir, &["rev-parse", "--show-toplevel"]) {
        Ok(top) if !top.is_empty() => Ok(std::path::absolute(&top).unwrap_or_else(|_| PathBuf::from(top))),
        Ok(_) | Err(GitError::Exited { .. }) => Ok(dir.to_path_buf()),
        Err(e) => Err(e),
    }
}

fn rev_parse(repo: &Path, rev: &str) -> Result<String, GitError> {
    if rev.starts_with('-') {
        return Err(GitError::BadRef {
            rev: rev.to_owned(),
            stderr: "revisions may not start with '-'".into(),
        });
    }
    let spec = format!("{rev}^{{commit}}");
    match cmd::run_text(repo, &["rev-parse", "--verify", "--quiet", "--end-of-options", &spec]) {
        Ok(id) if !id.is_empty() => Ok(id),
        Ok(_) => Err(GitError::BadRef {
            rev: rev.to_owned(),
            stderr: String::new(),
        }),
        Err(GitError::Exited { stderr, .. }) => Err(GitError::BadRef {
            rev: rev.to_owned(),
            stderr,
        }),
        Err(e) => Err(e),
    }
}

/// Resolves base and target to commit ids, through `git merge-base <target> <base>` when asked
/// (`HEAD` stands in for a working-tree or index target, as in difit).
pub fn resolve(repo: &Path, launch: &RepoLaunch) -> Result<Resolved, GitError> {
    check_repo(repo)?;
    let kind = TargetKind::of(&launch.target);
    let base = rev_parse(repo, &launch.base)?;
    let target = match kind {
        TargetKind::Commit => rev_parse(repo, &launch.target)?,
        _ => launch.target.clone(),
    };
    let base = if launch.merge_base {
        let tip = if kind == TargetKind::Commit { target.as_str() } else { "HEAD" };
        cmd::run_text(repo, &["merge-base", tip, &base])?
    } else {
        base
    };
    Ok(Resolved {
        base,
        target,
        merge_base_used: launch.merge_base,
        working_tree: matches!(kind, TargetKind::WorkingTree | TargetKind::Unstaged),
        kind,
    })
}

fn diff_args<'a>(r: &'a Resolved, extra: &[&'a str], ignore_whitespace: bool) -> Vec<&'a str> {
    let mut args = vec![
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--color=never",
        "--no-relative",
        "--submodule=short",
        "--src-prefix=a/",
        "--dst-prefix=b/",
    ];
    args.extend_from_slice(extra);
    if ignore_whitespace {
        args.push("-w");
    }
    args.extend(r.range_args());
    args.push("--");
    args
}

/// `git diff` of `r`, parsed as it streams, with untracked files and generated flags added.
pub fn diff_patch(repo: &Path, repo_index: u16, r: &Resolved, opts: &DiffOpts) -> Result<Patch, GitError> {
    let jobs = match opts.jobs {
        0 => std::thread::available_parallelism().map_or(1, |n| n.get()).min(8),
        n => n,
    };
    let listed = if jobs >= 2 || opts.ignore_whitespace {
        name_status(repo, r)?
    } else {
        None
    };
    let groups = listed.as_deref().and_then(|e| path_groups(e, jobs));
    let mut patch = match groups {
        Some(groups) => std::thread::scope(|s| {
            let workers: Vec<_> = groups
                .iter()
                .map(|g| s.spawn(move || diff_stream(repo, repo_index, r, opts, g)))
                .collect();
            let mut all = Patch::default();
            for w in workers {
                let p = w
                    .join()
                    .map_err(|_| GitError::Io(std::io::Error::other("diff worker panicked")))??;
                all.files.extend(p.files);
                all.meta.extend(p.meta);
            }
            Ok::<_, GitError>(all)
        })?,
        None => diff_stream(repo, repo_index, r, opts, &[])?,
    };
    if opts.ignore_whitespace
        && let Some(listed) = &listed
    {
        mark_formatting_only(&mut patch, listed, repo_index);
    }
    if opts.include_untracked && r.working_tree {
        for f in untracked(repo, repo_index)? {
            patch.files.push(f);
            patch.meta.push(parse::FileMeta::default());
        }
    }
    mark_generated(repo, r, &mut patch.files);
    Ok(patch)
}

/// One `git diff`, limited to `paths` when given, parsed as it streams.
fn diff_stream(repo: &Path, repo_index: u16, r: &Resolved, opts: &DiffOpts, paths: &[String]) -> Result<Patch, GitError> {
    let unified = format!("-U{}", opts.context);
    let mut args = vec!["--literal-pathspecs"];
    args.extend(diff_args(r, &["--find-renames", &unified], opts.ignore_whitespace));
    args.extend(paths.iter().map(String::as_str));
    let (proc, stdout) = Streaming::spawn(repo, &args, None)?;
    let patch = parse::parse_reader(BufReader::with_capacity(1 << 18, stdout), repo_index)?;
    proc.finish()?;
    Ok(patch)
}

/// One changed path from `--name-status`: its status letter and one path (two for a rename or copy).
struct Listed {
    status: u8,
    /// The `097` of `R097`.
    score: Option<u8>,
    paths: Vec<String>,
}

/// Every changed path, whitespace included, or `None` when a path is one git would quote.
fn name_status(repo: &Path, r: &Resolved) -> Result<Option<Vec<Listed>>, GitError> {
    let args = diff_args(r, &["--name-status", "-z", "--find-renames"], false);
    let out = cmd::run(repo, &args)?;
    let mut entries = Vec::new();
    let mut tokens = out.split(|&b| b == 0).filter(|t| !t.is_empty());
    while let Some(status) = tokens.next() {
        let n = if matches!(status.first(), Some(b'R' | b'C')) { 2 } else { 1 };
        let paths: Vec<String> = tokens.by_ref().take(n).map(|p| String::from_utf8_lossy(p).into_owned()).collect();
        if paths.len() < n || paths.iter().any(|p| p.contains('\n') || p.starts_with('"')) {
            return Ok(None);
        }
        entries.push(Listed {
            status: status.first().copied().unwrap_or(b'M'),
            score: std::str::from_utf8(status.get(1..).unwrap_or_default())
                .ok()
                .and_then(|s| s.parse().ok()),
            paths,
        });
    }
    Ok(Some(entries))
}

/// One changed path as `git diff --name-status` lists it: enough to paint a file list before the
/// patch is read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameStatus {
    pub status: FileStatus,
    pub path: String,
    pub old_path: Option<String>,
    /// Rename or copy similarity in percent.
    pub similarity: Option<u8>,
}

/// Every changed path of `r` with its status, or `None` when a path is one git would quote.
pub fn changed_paths(repo: &Path, r: &Resolved) -> Result<Option<Vec<NameStatus>>, GitError> {
    Ok(name_status(repo, r)?.map(|listed| {
        listed
            .into_iter()
            .map(|e| {
                let status = match e.status {
                    b'A' => FileStatus::Added,
                    b'D' => FileStatus::Deleted,
                    b'R' => FileStatus::Renamed,
                    b'C' => FileStatus::Copied,
                    _ => FileStatus::Modified,
                };
                let mut paths = e.paths.into_iter();
                let first = paths.next().unwrap_or_default();
                match paths.next() {
                    Some(new) => NameStatus {
                        status,
                        path: new,
                        old_path: Some(first),
                        similarity: e.score,
                    },
                    None => NameStatus {
                        status,
                        path: first,
                        old_path: None,
                        similarity: None,
                    },
                }
            })
            .collect()
    }))
}

/// The commit `HEAD` points at, for noticing new commits under an open window.
pub fn head_sha(repo: &Path) -> Result<String, GitError> {
    rev_parse(repo, "HEAD")
}

/// The repository's own git folder (`HEAD`, `index`) and the folder shared by its worktrees
/// (`refs/`, `packed-refs`); the same folder outside a linked worktree.
pub fn git_dirs(repo: &Path) -> Result<(PathBuf, PathBuf), GitError> {
    let out = cmd::run_text(repo, &["rev-parse", "--path-format=absolute", "--git-dir", "--git-common-dir"])?;
    let mut it = out.lines().map(|l| PathBuf::from(l.trim()));
    let git_dir = it.next().filter(|p| !p.as_os_str().is_empty()).ok_or_else(|| GitError::NotARepo {
        dir: repo.to_path_buf(),
        stderr: String::new(),
    })?;
    let common = it.next().unwrap_or_else(|| git_dir.clone());
    Ok((git_dir, common))
}

/// What a commit target points at now, against the commit the window loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Moved {
    pub from: String,
    pub to: String,
    /// Commits on `to` that `from` lacks; `None` when history was rewritten.
    pub commits: Option<u32>,
    /// The branch checked out (`HEAD` targets) or the target as given.
    pub branch: String,
}

/// Whether `target` (as launched) now names another commit than `loaded`. Working-tree and
/// index targets compare `HEAD` with `head_at_load`.
pub fn target_moved(repo: &Path, target: &str, loaded: &str, head_at_load: &str) -> Result<Option<Moved>, GitError> {
    let kind = TargetKind::of(target);
    let (from, to) = match kind {
        TargetKind::Commit => (loaded.to_owned(), rev_parse(repo, target)?),
        _ => (head_at_load.to_owned(), head_sha(repo)?),
    };
    if from == to || from.is_empty() {
        return Ok(None);
    }
    // `behind ahead`: `from` is an ancestor of `to` when nothing is behind.
    let commits = cmd::run_text(repo, &["rev-list", "--left-right", "--count", &format!("{from}...{to}")])
        .ok()
        .and_then(|out| {
            let mut it = out.split_whitespace().map(|n| n.parse::<u32>().ok());
            match (it.next()??, it.next()??) {
                (0, ahead) => Some(ahead),
                _ => None,
            }
        });
    let branch = if kind == TargetKind::Commit && target != "HEAD" {
        target.to_owned()
    } else {
        cmd::run_text(repo, &["symbolic-ref", "--short", "-q", "HEAD"])
            .ok()
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| "HEAD".to_owned())
    };
    Ok(Some(Moved { from, to, commits, branch }))
}

/// The changed paths split into contiguous groups (a rename's two paths kept together, so each
/// group detects the same renames as one whole-tree diff), or `None` for a single process.
fn path_groups(entries: &[Listed], jobs: usize) -> Option<Vec<Vec<String>>> {
    if jobs < 2 || entries.len() < PARALLEL_MIN_FILES {
        return None;
    }
    let per = entries.len().div_ceil(jobs);
    let groups: Vec<Vec<String>> = entries
        .chunks(per)
        .map(|c| c.iter().flat_map(|e| e.paths.iter().cloned()).collect())
        .collect();
    let too_long = groups
        .iter()
        .any(|g| g.iter().map(|p| p.len() + 3).sum::<usize>() > MAX_PATHSPEC_BYTES);
    (!too_long).then_some(groups)
}

/// Under `-w`, a text file with no hunks left, or one `-w` dropped altogether, changed only in
/// whitespace; dropped ones come back as empty entries so the tree still lists them.
fn mark_formatting_only(patch: &mut Patch, listed: &[Listed], repo_index: u16) {
    let present: HashSet<String> = patch.files.iter().map(|f| f.path.clone()).collect();
    for e in listed {
        let Some(path) = e.paths.last() else { continue };
        if present.contains(path) || e.status == b'D' {
            continue;
        }
        let status = match e.status {
            b'A' => FileStatus::Added,
            b'R' => FileStatus::Renamed,
            b'C' => FileStatus::Copied,
            _ => FileStatus::Modified,
        };
        let mut b = FileDiffBuilder::new(repo_index, path.clone(), status);
        if e.paths.len() == 2 {
            b.file_mut().old_path = Some(e.paths[0].clone());
        }
        patch.files.push(b.finish());
        patch.meta.push(parse::FileMeta::default());
    }
    patch.formatting_only = patch
        .files
        .iter()
        .zip(&patch.meta)
        .filter(|(f, m)| {
            f.hunks.is_empty() && !f.binary && !f.contents_unavailable && f.status == FileStatus::Modified && m.old_mode == m.new_mode
        })
        .map(|(f, _)| f.path.clone())
        .collect();
}

pub fn diff(repo: &Path, repo_index: u16, r: &Resolved, opts: &DiffOpts) -> Result<Vec<FileDiff>, GitError> {
    Ok(diff_patch(repo, repo_index, r, opts)?.files)
}

/// Untracked files bigger than this aren't read: they list as "contents unavailable", the way
/// git treats files over `core.bigFileThreshold` as binary.
pub const UNTRACKED_MAX_BYTES: u64 = 16 * 1024 * 1024;

/// Untracked, non-ignored files as Added diffs read from disk (the index is left alone). One that
/// can't be read (locked, access denied) or is over [`UNTRACKED_MAX_BYTES`] lists with no contents.
pub fn untracked(repo: &Path, repo_index: u16) -> Result<Vec<FileDiff>, GitError> {
    let out = cmd::run(repo, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    let mut files = Vec::new();
    // A trailing '/' is a nested repository, not a file.
    for raw in out.split(|&b| b == 0).filter(|p| !p.is_empty() && !p.ends_with(b"/")) {
        let path = String::from_utf8_lossy(raw).into_owned();
        match read_capped(&repo.join(&path)) {
            Ok(Some(bytes)) => files.push(added_file(repo_index, path, &bytes)),
            Ok(None) => files.push(unavailable_file(repo_index, path)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                eprintln!("Rain: untracked {path} can't be read: {e}");
                files.push(unavailable_file(repo_index, path));
            }
        }
    }
    Ok(files)
}

/// The file's bytes, or `None` when it is over [`UNTRACKED_MAX_BYTES`].
fn read_capped(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let f = std::fs::File::open(path)?;
    if f.metadata()?.len() > UNTRACKED_MAX_BYTES {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    f.take(UNTRACKED_MAX_BYTES + 1).read_to_end(&mut bytes)?;
    Ok((bytes.len() as u64 <= UNTRACKED_MAX_BYTES).then_some(bytes))
}

fn unavailable_file(repo_index: u16, path: String) -> FileDiff {
    let mut b = FileDiffBuilder::new(repo_index, path, FileStatus::Added);
    b.file_mut().contents_unavailable = true;
    b.finish()
}

/// A whole file as one all-Added hunk; binary (a NUL in the first 8 KiB) has no lines.
pub fn added_file(repo_index: u16, path: String, bytes: &[u8]) -> FileDiff {
    let mut b = FileDiffBuilder::new(repo_index, path, FileStatus::Added);
    if bytes[..bytes.len().min(8000)].contains(&0) {
        let f = b.file_mut();
        f.binary = true;
        f.contents_unavailable = true;
        return b.finish();
    }
    let text = String::from_utf8_lossy(bytes);
    let body = text.strip_suffix('\n').unwrap_or(&text);
    if !text.is_empty() {
        let count = body.split('\n').count() as u32;
        b.reserve(count as usize, body.len());
        b.push_hunk(0, 0, 1, count, "");
        for (i, l) in body.split('\n').enumerate() {
            b.push_line(LineKind::Added, 0, i as u32 + 1, l.strip_suffix('\r').unwrap_or(l));
        }
        if !text.ends_with('\n') {
            b.mark_no_newline();
        }
    }
    b.finish()
}

/// Sets `generated` from the path, `linguist-generated` attributes and header markers.
fn mark_generated(repo: &Path, r: &Resolved, files: &mut [FileDiff]) {
    for f in files.iter_mut() {
        f.generated = generated::generated_path(&f.path);
    }
    let candidates: Vec<&str> = files.iter().filter(|f| !f.generated).map(|f| f.path.as_str()).collect();
    let attrs = linguist_generated(repo, r, &candidates).unwrap_or_default();
    let mut need_blob = Vec::new();
    for (i, f) in files.iter_mut().enumerate() {
        if f.generated || f.binary || f.contents_unavailable {
            continue;
        }
        if attrs.contains(&f.path) {
            f.generated = true;
            continue;
        }
        match header_from_diff(f) {
            Some(h) => f.generated = generated::generated_header(&f.path, &h),
            None if f.status != FileStatus::Deleted => need_blob.push(i),
            None => {}
        }
    }
    if need_blob.is_empty() {
        return;
    }
    let paths: Vec<&str> = need_blob.iter().map(|&i| files[i].path.as_str()).collect();
    let heads = heads(repo, r.rev(Side::New), &paths).unwrap_or_default();
    for (i, head) in need_blob.into_iter().zip(heads) {
        if let Some(head) = head {
            files[i].generated = generated::generated(&files[i].path, &head);
        }
    }
}

/// The file's header from its own diff, when the first hunk shows the file's first lines.
pub fn header_from_diff(f: &FileDiff) -> Option<String> {
    let h = f.hunks.first()?;
    let (side, start) = if f.status == FileStatus::Deleted {
        (LineKind::Added, h.old_start)
    } else {
        (LineKind::Removed, h.new_start)
    };
    if start > 1 {
        return None;
    }
    let mut out = String::new();
    let mut n = 0;
    for i in h.lines.clone() {
        if f.lines[i as usize].kind == side {
            continue;
        }
        if n > 0 {
            out.push('\n');
        }
        out.push_str(f.line_text(i));
        n += 1;
        if n == HEADER_LINES || out.len() >= HEADER_BYTES {
            break;
        }
    }
    let whole = f.hunks.len() == 1 && matches!(f.status, FileStatus::Added | FileStatus::Deleted);
    (n == HEADER_LINES || out.len() >= HEADER_BYTES || whole).then(|| generated::header_text(out.as_bytes()))
}

/// Paths whose `linguist-generated` attribute is true at the target.
fn linguist_generated(repo: &Path, r: &Resolved, paths: &[&str]) -> Result<HashSet<String>, GitError> {
    if paths.is_empty() {
        return Ok(HashSet::new());
    }
    let mut args = vec!["check-attr", "--stdin", "-z"];
    match r.kind {
        TargetKind::Commit => args.extend(["--source", r.target.as_str()]),
        TargetKind::Staged => args.push("--cached"),
        _ => {}
    }
    args.push("linguist-generated");
    let mut input = Vec::with_capacity(paths.iter().map(|p| p.len() + 1).sum());
    for p in paths {
        input.extend_from_slice(p.as_bytes());
        input.push(0);
    }
    let (proc, mut stdout) = Streaming::spawn(repo, &args, Some(input))?;
    let mut out = Vec::new();
    stdout.read_to_end(&mut out)?;
    proc.finish()?;
    let fields: Vec<&[u8]> = out.split(|&b| b == 0).collect();
    Ok(fields
        .chunks_exact(3)
        .filter(|c| c[1] == b"linguist-generated" && c[2] == b"true")
        .map(|c| String::from_utf8_lossy(c[0]).into_owned())
        .collect())
}

/// The first [`HEADER_BYTES`] of each path at `rev`, `None` where it does not exist.
fn heads(repo: &Path, rev: &str, paths: &[&str]) -> Result<Vec<Option<Vec<u8>>>, GitError> {
    if matches!(TargetKind::of(rev), TargetKind::WorkingTree | TargetKind::Unstaged) {
        return Ok(paths
            .iter()
            .map(|p| {
                let mut head = Vec::with_capacity(HEADER_BYTES);
                let file = std::fs::File::open(worktree_path(repo, p).ok()?).ok()?;
                file.take(HEADER_BYTES as u64).read_to_end(&mut head).ok()?;
                Some(head)
            })
            .collect());
    }
    let prefix = if rev == "staged" { "" } else { rev };
    let mut input = Vec::new();
    for p in paths {
        if !p.contains('\n') {
            input.extend_from_slice(format!("{prefix}:{p}\n").as_bytes());
        }
    }
    let (proc, stdout) = Streaming::spawn(repo, &["cat-file", "--batch"], Some(input))?;
    let mut rd = BufReader::new(stdout);
    let mut out = Vec::with_capacity(paths.len());
    let mut line = String::new();
    for p in paths {
        if p.contains('\n') {
            out.push(None);
            continue;
        }
        line.clear();
        rd.read_line(&mut line)?;
        let size = line.trim_end().rsplit(' ').next().and_then(|s| s.parse::<u64>().ok());
        let Some(size) = size.filter(|_| !line.trim_end().ends_with(" missing")) else {
            out.push(None);
            continue;
        };
        let mut head = Vec::with_capacity(HEADER_BYTES.min(size as usize));
        (&mut rd).take(size.min(HEADER_BYTES as u64)).read_to_end(&mut head)?;
        std::io::copy(
            &mut (&mut rd).take(size.saturating_sub(HEADER_BYTES as u64) + 1),
            &mut std::io::sink(),
        )?;
        out.push(Some(head));
    }
    drop(rd);
    proc.finish()?;
    Ok(out)
}

fn worktree_path(repo: &Path, path: &str) -> Result<PathBuf, GitError> {
    let rel = Path::new(path);
    let escapes = rel.is_absolute()
        || path.is_empty()
        || rel
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_) | Component::RootDir));
    if escapes {
        return Err(GitError::BadPath(path.to_owned()));
    }
    Ok(repo.join(rel))
}

/// The bytes of `path` at `rev`; `.`/`WORKTREE`/`working` read the working tree, `staged`
/// the index.
pub fn blob(repo: &Path, rev: &str, path: &str) -> Result<Vec<u8>, GitError> {
    match TargetKind::of(rev) {
        TargetKind::WorkingTree | TargetKind::Unstaged => Ok(std::fs::read(worktree_path(repo, path)?)?),
        TargetKind::Staged => {
            worktree_path(repo, path)?;
            cmd::run(repo, &["cat-file", "blob", &format!(":{path}")])
        }
        TargetKind::Commit => {
            worktree_path(repo, path)?;
            if rev.starts_with('-') {
                return Err(GitError::BadRef {
                    rev: rev.to_owned(),
                    stderr: String::new(),
                });
            }
            cmd::run(repo, &["cat-file", "blob", &format!("{rev}:{path}")])
        }
    }
}

/// The commit one side of `r` shows, when that side is a commit (not the index or working tree).
pub fn side_commit(r: &Resolved, side: Side) -> Option<&str> {
    match side {
        Side::Old if r.kind != TargetKind::Unstaged => Some(&r.base),
        Side::New if r.kind == TargetKind::Commit => Some(&r.target),
        _ => None,
    }
}

/// `git diff -U0` hunk headers `[from_start, from_count, to_start, to_count]` of `path` from
/// commit `from` to what `side` of `r` shows now: how its lines moved.
pub fn line_map(repo: &Path, r: &Resolved, side: Side, from: &str, path: &str) -> Result<Vec<[u32; 4]>, GitError> {
    check_commit_id(from)?;
    worktree_path(repo, path)?;
    let mut args = vec![
        "--literal-pathspecs",
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--color=never",
        "-U0",
    ];
    args.extend(line_map_range(r, side, from));
    args.extend(["--", path]);
    let out = cmd::run(repo, &args)?;
    Ok(String::from_utf8_lossy(&out).lines().filter_map(zero_hunk).collect())
}

/// [`line_map`] for many paths with one `git diff -U0` per pathspec batch; `out[i]` is the map
/// of `paths[i]` (empty when it did not change).
pub fn line_maps(repo: &Path, r: &Resolved, side: Side, from: &str, paths: &[&str]) -> Result<Vec<Vec<[u32; 4]>>, GitError> {
    check_commit_id(from)?;
    for p in paths {
        worktree_path(repo, p)?;
    }
    let mut out = vec![Vec::new(); paths.len()];
    let mut at: std::collections::HashMap<&str, Vec<usize>> = std::collections::HashMap::new();
    for (i, p) in paths.iter().enumerate() {
        at.entry(p).or_default().push(i);
    }
    let unique: BTreeSet<&str> = paths.iter().copied().collect();
    let mut batch: Vec<&str> = Vec::new();
    let mut bytes = 0;
    let mut batches = Vec::new();
    for p in unique {
        if !batch.is_empty() && bytes + p.len() + 3 > MAX_PATHSPEC_BYTES {
            batches.push(std::mem::take(&mut batch));
            bytes = 0;
        }
        bytes += p.len() + 3;
        batch.push(p);
    }
    if !batch.is_empty() {
        batches.push(batch);
    }
    for batch in batches {
        let mut args = vec![
            "--literal-pathspecs",
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--color=never",
            "--no-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            "-U0",
        ];
        args.extend(line_map_range(r, side, from));
        args.push("--");
        args.extend(batch.iter().copied());
        let text = cmd::run(repo, &args)?;
        let mut cur: Option<&Vec<usize>> = None;
        for l in String::from_utf8_lossy(&text).lines() {
            if let Some(rest) = l.strip_prefix("diff --git ") {
                let (a, b) = parse::git_paths(rest);
                cur = b
                    .as_deref()
                    .and_then(|p| at.get(p))
                    .or_else(|| a.as_deref().and_then(|p| at.get(p)));
            } else if let (Some(ix), Some(h)) = (cur, zero_hunk(l)) {
                for &i in ix {
                    out[i].push(h);
                }
            }
        }
    }
    Ok(out)
}

fn check_commit_id(from: &str) -> Result<(), GitError> {
    if from.len() < 7 || !from.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(GitError::BadRef {
            rev: from.to_owned(),
            stderr: "not a commit id".into(),
        });
    }
    Ok(())
}

fn line_map_range<'a>(r: &'a Resolved, side: Side, from: &'a str) -> Vec<&'a str> {
    match (side, r.kind) {
        (Side::Old, TargetKind::Unstaged) | (Side::New, TargetKind::Staged) => vec!["--cached", from],
        (Side::Old, _) => vec![from, r.base.as_str()],
        (Side::New, TargetKind::Commit) => vec![from, r.target.as_str()],
        (Side::New, _) => vec![from],
    }
}

/// `[from_start, from_count, to_start, to_count]` of a `-U0` hunk header line.
fn zero_hunk(l: &str) -> Option<[u32; 4]> {
    let range = |s: &str| -> Option<(u32, u32)> {
        let (a, b) = s.split_once(',').unwrap_or((s, "1"));
        Some((a.parse().ok()?, b.parse().ok()?))
    };
    let mut parts = l.strip_prefix("@@ -")?.split(' ');
    let (a, b) = range(parts.next()?)?;
    let (c, d) = range(parts.next()?.strip_prefix('+')?)?;
    Some([a, b, c, d])
}

/// difit's `repositoryId`: SHA-256 hex of the absolute repository path as Node's
/// `path.resolve` writes it (no trailing separator, symlinks not followed).
pub fn repository_id(repo: &Path) -> String {
    let abs = std::path::absolute(repo).unwrap_or_else(|_| repo.to_path_buf());
    let mut s = abs.to_string_lossy().into_owned();
    while s.len() > 1 && (s.ends_with('\\') || s.ends_with('/')) && !s.ends_with(":\\") {
        s.pop();
    }
    let digest = Sha256::digest(s.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Paths of `files` (each with at least one changed line) whose changes all vanish under `-w`.
pub fn formatting_only(repo: &Path, r: &Resolved, files: &[&str]) -> Result<BTreeSet<String>, GitError> {
    if files.is_empty() {
        return Ok(BTreeSet::new());
    }
    let changed = changed_under_w(repo, r)?;
    Ok(files.iter().filter(|p| !changed.contains(**p)).map(|p| (*p).to_owned()).collect())
}

/// Paths that still have changed lines under `git diff -w --numstat` (both sides of a rename).
pub fn changed_under_w(repo: &Path, r: &Resolved) -> Result<HashSet<String>, GitError> {
    let args = diff_args(r, &["--numstat", "-z"], true);
    let out = cmd::run(repo, &args)?;
    let tokens: Vec<&[u8]> = out.split(|&b| b == 0).collect();
    let mut changed: HashSet<&[u8]> = HashSet::new();
    let mut i = 0;
    while i < tokens.len() {
        let mut parts = tokens[i].splitn(3, |&b| b == b'\t');
        let (Some(add), Some(del), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
            i += 1;
            continue;
        };
        let zero = add == b"0" && del == b"0";
        if path.is_empty() {
            if !zero {
                changed.extend(tokens.get(i + 1..(i + 3).min(tokens.len())).unwrap_or_default());
            }
            i += 3;
        } else {
            if !zero {
                changed.insert(path);
            }
            i += 1;
        }
    }
    Ok(changed.into_iter().map(|p| String::from_utf8_lossy(p).into_owned()).collect())
}
