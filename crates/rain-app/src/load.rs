//! Everything the window shows that comes from disk: each repo's diff (or a `.patch` fixture),
//! the overview with its tags and reading order, the session's outdated flags and comment
//! imports. Runs on a loader thread; the UI thread only installs the result.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use rain_core::overview::OverviewCoverage;
use rain_core::{DiffSet, FileDiff, FileTag, Launch, RepoInfo, RepoLaunch, Shared, diff_content_hash};
use rain_git::{DiffOpts, Resolved};
use rain_overview::Loaded;
use rain_triage::Triage;

pub const LARGE_FILE_LINES: usize = 20_000;

pub struct Data {
    pub repos: Vec<RepoInfo>,
    /// In reading order.
    pub files: Vec<FileDiff>,
    /// `<repoId>/<path>` per file.
    pub keys: Vec<String>,
    pub hashes: Vec<String>,
    pub resolved: Vec<Option<Resolved>>,
    /// Per repo, the mark of a working-tree or index diff, taken before the diff was read.
    pub edit_marks: Vec<Option<u64>>,
    pub overview: Option<Loaded>,
    /// Validation issues or load errors of the overview, for the banner.
    pub overview_issues: Vec<String>,
    /// Every file's tag: the overview's when there is one, else the triage group's.
    pub tags: BTreeMap<String, FileTag>,
    /// Groups, reasons and facts; `triage.items` is in the same order as `keys`.
    pub triage: Triage,
    /// Blocks that moved within the diff, by the files' order here.
    pub moves: rain_core::moves::Moves,
    pub timings: Timings,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timings {
    pub diff_ms: f64,
    pub overview_ms: f64,
    pub total_ms: f64,
}

pub enum Msg {
    Progress(String),
    Done(Box<Result<Data, String>>),
    Warm,
    /// A file's whole texts, by file key, for expanding its unchanged ranges.
    Texts {
        key: String,
        old: Option<Arc<str>>,
        new: Option<Arc<str>>,
    },
    /// Several [`Msg::Texts`] to install together, so the rows are laid out once for them all.
    Batch(Vec<Msg>),
    /// Git's answer for the since-round lens, for the install it was asked under.
    Lens {
        round: u32,
        installs: u32,
        announce: bool,
        maps: crate::app::LineMaps,
    },
}

/// Reads both sides of a file whole (what `git show` would print), for [`Msg::Texts`].
pub fn read_texts(dir: &Path, r: &Resolved, f: &FileDiff, key: String) -> Msg {
    use rain_core::{FileStatus, Side};
    let read = |side, path: &str| {
        rain_git::blob(dir, r.rev(side), path)
            .ok()
            .map(|b| Arc::<str>::from(String::from_utf8_lossy(&b).as_ref()))
    };
    let old_path = f.old_path.as_deref().unwrap_or(&f.path);
    Msg::Texts {
        key,
        old: (f.status != FileStatus::Added).then(|| read(Side::Old, old_path)).flatten(),
        new: (f.status != FileStatus::Deleted).then(|| read(Side::New, &f.path)).flatten(),
    }
}

fn ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

fn leaf(dir: &Path) -> String {
    let abs = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    abs.file_name()
        .map(|n| n.to_string_lossy().trim_start_matches(r"\\?\").to_owned())
        .unwrap_or_else(|| "repo".to_owned())
}

/// The overview from `--overview`, else the one the session already holds. Issues are
/// formatted for the banner; a valid document passed on the command line also goes into the
/// session so the HTTP routes and the next run see it.
fn load_overview(launch: &Launch, shared: &Shared) -> (Option<Loaded>, Vec<String>) {
    let fmt = |issues: Vec<rain_core::overview::Issue>| {
        issues
            .into_iter()
            .map(|i| format!("{} {}: {}", i.code, i.path, i.message))
            .collect::<Vec<_>>()
    };
    if let Some(path) = &launch.overview {
        return match rain_overview::load(path) {
            Ok(l) => {
                let mut s = shared.lock();
                if s.overview.as_ref() != Some(&l.document) {
                    s.overview = Some(l.document.clone());
                    s.overview_version += 1;
                    s.version += 1;
                }
                (Some(l), Vec::new())
            }
            Err(issues) => (None, fmt(issues)),
        };
    }
    let doc = shared.lock().overview.clone();
    match doc.map(serde_json::to_value) {
        Some(Ok(v)) => match rain_overview::load_value(v) {
            Ok(l) => (Some(l), Vec::new()),
            Err(issues) => (None, fmt(issues)),
        },
        _ => (None, Vec::new()),
    }
}

/// Splits a fixture's files into repos by their first folder when the overview names that
/// folder as a repo (`server/src/x.cs` → repo `server`, `src/x.cs`).
fn split_fixture(files: Vec<FileDiff>, overview: Option<&Loaded>, stem: &str) -> (Vec<RepoInfo>, Vec<FileDiff>) {
    let ids: Vec<&str> = overview.map_or_else(Vec::new, |l| l.document.repos.iter().map(|r| r.id.as_str()).collect());
    let mut repos: Vec<RepoInfo> = Vec::new();
    let mut out = Vec::with_capacity(files.len());
    let repo_of = |id: &str, repos: &mut Vec<RepoInfo>| -> u16 {
        if let Some(i) = repos.iter().position(|r| r.id == id) {
            return i as u16;
        }
        repos.push(RepoInfo {
            id: id.to_owned(),
            name: if id.is_empty() { stem.to_owned() } else { id.to_owned() },
            base: "fixture".into(),
            target: "fixture".into(),
            ..RepoInfo::default()
        });
        (repos.len() - 1) as u16
    };
    for mut f in files {
        let split = f
            .path
            .split_once('/')
            .filter(|(head, _)| ids.contains(head))
            .map(|(h, t)| (h.to_owned(), t.to_owned()));
        match split {
            Some((head, tail)) => {
                f.repo = repo_of(&head, &mut repos);
                f.path = tail;
                let prefix = format!("{head}/");
                f.old_path = f.old_path.map(|p| p.strip_prefix(&prefix).map_or(p.clone(), str::to_owned));
            }
            None => f.repo = repo_of("", &mut repos),
        }
        out.push(f);
    }
    if repos.is_empty() {
        repo_of("", &mut repos);
    }
    (repos, out)
}

type Fixture = (Vec<RepoInfo>, Vec<FileDiff>, Vec<Option<u8>>);

fn load_fixture(path: &Path, overview: Option<&Loaded>) -> Result<Fixture, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open fixture {}: {e}", path.display()))?;
    let patch = rain_git::parse_reader(std::io::BufReader::with_capacity(256 * 1024, file), 0)
        .map_err(|e| format!("cannot read fixture {}: {e}", path.display()))?;
    let mut files = patch.files;
    for f in &mut files {
        if !f.generated {
            f.generated = rain_git::generated(&f.path, &[]);
        }
    }
    let similarity = patch.meta.iter().map(|m| m.similarity).collect();
    let stem = path
        .file_stem()
        .map_or_else(|| "fixture".to_owned(), |s| s.to_string_lossy().into_owned());
    let (repos, files) = split_fixture(files, overview, &stem);
    Ok((repos, files, similarity))
}

struct GitRepo {
    info: RepoInfo,
    resolved: Resolved,
    edit_mark: Option<u64>,
    files: Vec<FileDiff>,
    similarity: Vec<Option<u8>>,
    formatting_only: Vec<String>,
}

fn load_repo(i: usize, rl: &RepoLaunch, launch: &Launch, overview: Option<&Loaded>) -> Result<GitRepo, String> {
    // Git prints top-relative paths, so a window opened in a subfolder still runs git at the top.
    let top = rain_git::toplevel(&rl.dir).map_err(|e| e.to_string())?;
    let dir = &top;
    let resolved = rain_git::resolve(dir, rl).map_err(|e| e.to_string())?;
    let defaults = DiffOpts::default();
    let opts = DiffOpts {
        ignore_whitespace: launch.ignore_whitespace,
        include_untracked: rl.include_untracked,
        context: launch.context.unwrap_or(defaults.context),
        ..defaults
    };
    // Before the diff, so an edit made while it is read still differs from the mark.
    let edit_mark = rain_git::edit_mark(dir, &resolved, rl.include_untracked).ok().flatten();
    // Without `-w`, a second `-w --numstat` run finds the whitespace-only files; it runs beside the
    // diff because on a 1M-line change each git run costs about a second.
    let (patch, changed_under_w) = std::thread::scope(|s| {
        let numstat = (!launch.ignore_whitespace).then(|| s.spawn(|| rain_git::changed_under_w(dir, &resolved)));
        let patch = rain_git::diff_patch(dir, i as u16, &resolved, &opts);
        (patch, numstat.and_then(|h| h.join().ok()?.ok()))
    });
    let patch = patch.map_err(|e| e.to_string())?;
    let formatting_only = match changed_under_w {
        Some(changed) => patch
            .files
            .iter()
            .filter(|f| f.additions + f.deletions > 0 && !changed.contains(&f.path))
            .map(|f| f.path.clone())
            .collect(),
        None => patch.formatting_only,
    };
    let mut similarity: Vec<Option<u8>> = patch.meta.iter().map(|m| m.similarity).collect();
    similarity.resize(patch.files.len(), None);
    let files = patch.files;
    let name = leaf(&rl.dir);
    let id = overview
        .and_then(|l| rain_overview::self_repo(&l.document, &name).map(str::to_owned))
        .unwrap_or_else(|| name.clone());
    Ok(GitRepo {
        info: RepoInfo {
            id,
            dir: dir.clone(),
            name,
            base: rl.base.clone(),
            target: rl.target.clone(),
            resolved_base: resolved.base.clone(),
            resolved_target: resolved.target.clone(),
            merge_base: rl.merge_base,
            repository_id: rain_git::repository_id(&rl.dir),
        },
        resolved,
        edit_mark,
        files,
        similarity,
        formatting_only,
    })
}

/// Loads everything for `launch`, reporting progress through `progress`. Blocking.
pub fn load(launch: &Launch, shared: &Shared, progress: &dyn Fn(String)) -> Result<Data, String> {
    let t0 = Instant::now();
    let (overview, overview_issues) = load_overview(launch, shared);
    let overview_ms = ms(t0);
    let t1 = Instant::now();
    let mut resolved = Vec::new();
    let mut edit_marks = Vec::new();
    let mut formatting: Vec<Vec<String>> = Vec::new();
    let mut similarity: Vec<Option<u8>> = Vec::new();
    let (repos, files) = if let Some(path) = &launch.fixture {
        progress(format!("Reading {}", path.display()));
        let (repos, files, sim) = load_fixture(path, overview.as_ref())?;
        resolved = vec![None; repos.len()];
        formatting = vec![Vec::new(); repos.len()];
        similarity = sim;
        (repos, files)
    } else {
        let launches: Vec<RepoLaunch> = if launch.repos.is_empty() {
            vec![RepoLaunch::default()]
        } else {
            launch.repos.clone()
        };
        let mut repos = Vec::new();
        let mut files = Vec::new();
        for (i, rl) in launches.iter().enumerate() {
            progress(format!("git diff {} ({}/{})", leaf(&rl.dir), i + 1, launches.len()));
            let r = load_repo(i, rl, launch, overview.as_ref())?;
            repos.push(r.info);
            resolved.push(Some(r.resolved));
            edit_marks.push(r.edit_mark);
            formatting.push(r.formatting_only);
            similarity.extend(r.similarity);
            files.extend(r.files);
        }
        (repos, files)
    };
    let diff_ms = ms(t1);
    progress(format!("{} files", files.len()));
    shared.set_diff_files(
        repos
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let mine = || files.iter().filter(move |f| f.repo as usize == i);
                rain_core::RepoFiles {
                    id: r.id.clone(),
                    files: mine().map(|f| f.path.clone()).collect(),
                    generated: mine().filter(|f| f.generated).map(|f| f.path.clone()).collect(),
                    formatting_only: formatting.get(i).cloned().unwrap_or_default(),
                }
            })
            .collect(),
    );

    let keys: Vec<String> = files.iter().map(|f| f.display_path(&repos)).collect();
    let mut tags: BTreeMap<String, FileTag> = BTreeMap::new();
    let mut ignored: BTreeSet<String> = BTreeSet::new();
    if let Some(l) = &overview {
        let covs: Vec<OverviewCoverage> = repos
            .iter()
            .enumerate()
            .filter(|(_, r)| l.document.repos.iter().any(|o| o.id == r.id))
            .map(|(i, r)| {
                let mine: Vec<&FileDiff> = files.iter().filter(|f| f.repo as usize == i).collect();
                let paths: Vec<&str> = mine.iter().map(|f| f.path.as_str()).collect();
                let generated: Vec<&str> = mine.iter().filter(|f| f.generated).map(|f| f.path.as_str()).collect();
                let fmt: Vec<&str> = formatting.get(i).map_or_else(Vec::new, |v| v.iter().map(String::as_str).collect());
                rain_overview::coverage(&l.document, Some(&r.id), &paths, &generated, &fmt)
            })
            .collect();
        tags = rain_overview::tags(&l.document, &covs);
        tags.retain(|_, t| *t != FileTag::Uncovered);
        for cov in &covs {
            let repo = cov.repo.as_deref().unwrap_or_default();
            ignored.extend(cov.ignored.iter().map(|p| rain_core::paths::join(repo, p)));
        }
    }
    let formatting_sets: Vec<HashSet<&str>> = formatting.iter().map(|v| v.iter().map(String::as_str).collect()).collect();
    let formatting_keys: BTreeSet<String> = keys
        .iter()
        .zip(&files)
        .filter(|(_, f)| formatting_sets.get(f.repo as usize).is_some_and(|v| v.contains(f.path.as_str())))
        .map(|(k, _)| k.clone())
        .collect();
    let rated = overview.as_ref().map(|l| rain_overview::rated(&l.document));
    let moves = rain_core::moves::detect(&files);
    let moved_keys: BTreeSet<String> = keys
        .iter()
        .zip(&files)
        .enumerate()
        .filter(|(i, (_, f))| moves.only_moves(*i, f))
        .map(|(_, (k, _))| k.clone())
        .collect();
    let triage = rain_triage::classify(&rain_triage::Input {
        files: &files,
        keys: &keys,
        similarity: &similarity,
        formatting_only: Some(&formatting_keys),
        moved_only: Some(&moved_keys),
        rated: rated.as_ref(),
        ignored: Some(&ignored),
        round: 1,
    });
    for it in &triage.items {
        tags.entry(it.key.clone()).or_insert_with(|| it.group.tag());
    }
    if let Ok(serde_json::Value::Object(mut v)) = serde_json::to_value(&triage) {
        v.insert("diffLoaded".into(), true.into());
        shared.set_triage(serde_json::Value::Object(v));
    }

    let order: Vec<&str> = triage.items.iter().map(|i| i.key.as_str()).collect();
    let pos: BTreeMap<&str, usize> = order.iter().enumerate().map(|(i, k)| (*k, i)).collect();
    let rank = |i: usize| pos.get(keys[i].as_str()).copied().unwrap_or(order.len() + i);
    let mut by_rank: Vec<usize> = (0..keys.len()).collect();
    by_rank.sort_by_key(|&i| rank(i));
    let mut placed = vec![0u32; keys.len()];
    for (new, &old) in by_rank.iter().enumerate() {
        placed[old] = new as u32;
    }
    let moves = moves.reordered(&placed);
    let mut paired: Vec<(usize, String, FileDiff)> = keys
        .into_iter()
        .zip(files)
        .enumerate()
        .map(|(i, (k, f))| (pos.get(k.as_str()).copied().unwrap_or(order.len() + i), k, f))
        .collect();
    paired.sort_by_key(|(p, _, _)| *p);
    let (keys, files): (Vec<String>, Vec<FileDiff>) = paired.into_iter().map(|(_, k, f)| (k, f)).unzip();
    let hashes: Vec<String> = files.iter().map(diff_content_hash).collect();

    let mut set = DiffSet {
        repos,
        files,
        ignore_whitespace: launch.ignore_whitespace,
    };
    {
        let rev_of = |repo: usize, side| {
            let r = resolved.get(repo)?.as_ref()?;
            rain_git::side_commit(r, side).map(str::to_owned)
        };
        let line_maps = |repo: usize, side, from: &str, _: &str, paths: &[&str]| {
            let (Some(r), Some(Some(res))) = (set.repos.get(repo), resolved.get(repo)) else {
                return vec![None; paths.len()];
            };
            match rain_git::line_maps(&r.dir, res, side, from, paths) {
                Ok(maps) => maps.into_iter().map(Some).collect(),
                Err(_) => paths.iter().map(|p| rain_git::line_map(&r.dir, res, side, from, p).ok()).collect(),
            }
        };
        // On a copy, so the HTTP routes don't wait on the git runs; writes made meanwhile keep
        // their threads, and only the anchors of threads nobody touched are merged back.
        let mut work = shared.lock().clone();
        let seen = work.comments_version;
        let notes_seen = work.notes_version;
        let base = work.threads.clone();
        if rain_comments::ops::reanchor_batched(&mut work, &set, rev_of, line_maps) > 0 {
            let mut s = shared.lock();
            if s.comments_version == seen {
                s.threads = std::mem::take(&mut work.threads);
                s.bump_comments();
            } else if rain_comments::ops::merge_anchors(&mut s, &base, &work.threads) > 0 {
                s.bump_comments();
            }
            drop(s);
            shared.notify_waiters();
        }
        if rain_comments::ops::reanchor_notes(&mut work, &set, rev_of) > 0 {
            let mut s = shared.lock();
            if s.notes_version == notes_seen {
                s.notes = std::mem::take(&mut work.notes);
                s.bump_notes();
            }
            drop(s);
            shared.notify_waiters();
        }
        let mut s = shared.lock();
        let repos: Vec<rain_core::SessionRepo> = set
            .repos
            .iter()
            .filter(|r| !r.resolved_target.is_empty())
            .map(|r| rain_core::SessionRepo {
                id: r.id.clone(),
                repository_id: r.repository_id.clone(),
                base: r.resolved_base.clone(),
                target: r.resolved_target.clone(),
            })
            .collect();
        if !repos.is_empty() && s.repos != repos {
            s.repos = repos;
            s.version += 1;
        }
        let default_repo = if set.repos.len() == 1 {
            String::new()
        } else {
            set.repos[0].id.clone()
        };
        for path in &launch.comments {
            match std::fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|j| rain_comments::from_difit_json(&j))
            {
                Ok(imports) => {
                    rain_comments::apply(&mut s, imports, &default_repo);
                }
                Err(e) => progress(format!("comments {}: {e}", path.display())),
            }
        }
    }
    let files = std::mem::take(&mut set.files);
    Ok(Data {
        repos: set.repos,
        files,
        keys,
        hashes,
        resolved,
        edit_marks,
        overview,
        overview_issues,
        tags,
        triage,
        moves,
        timings: Timings {
            diff_ms,
            overview_ms,
            total_ms: ms(t0),
        },
    })
}

/// Runs [`load`] and warms the highlighter on a thread, sending results to `tx` and calling
/// `wake` after each message.
pub fn spawn(launch: Launch, shared: Arc<Shared>, tx: std::sync::mpsc::Sender<Msg>, wake: Arc<dyn Fn() + Send + Sync>) {
    let _ = std::thread::Builder::new().name("rain-loader".into()).spawn(move || {
        let send = |m: Msg| {
            let _ = tx.send(m);
            wake();
        };
        let progress = |p: String| send(Msg::Progress(p));
        // A panic still ends the loading screen, with its message, instead of leaving it up forever.
        let data = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| load(&launch, &shared, &progress))).unwrap_or_else(|p| {
            let why = p
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| p.downcast_ref::<&str>().copied())
                .unwrap_or("unknown panic");
            Err(format!("loading failed: {why}"))
        });
        let warm = needs_warm(&data);
        send(Msg::Done(Box::new(data)));
        if warm {
            rain_syntax::warm();
        }
        send(Msg::Warm);
    });
}

/// Whether a loaded file is coloured by syntect, which is worth warming before colour goes on.
fn needs_warm(data: &Result<Data, String>) -> bool {
    data.as_ref()
        .is_ok_and(|d| d.files.iter().any(|f| !rain_syntax::fast_covers(rain_diffview::language(&f.path))))
}

/// Where the session file lives for `launch`: the named session, else the fixture's or first
/// repo's folder name.
pub fn session_id(launch: &Launch) -> String {
    if !launch.session.is_empty() {
        return launch.session.clone();
    }
    if let Some(f) = &launch.fixture {
        return f
            .file_stem()
            .map_or_else(|| "fixture".to_owned(), |s| s.to_string_lossy().into_owned());
    }
    let dir = launch.repos.first().map_or_else(|| PathBuf::from("."), |r| r.dir.clone());
    leaf(&dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_files_split_into_overview_repos() {
        let mk = |p: &str| FileDiff {
            path: p.into(),
            ..FileDiff::default()
        };
        let doc: Loaded = rain_overview::load(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/overview/sample-overview-schema.json"
        ))
        .unwrap();
        let (repos, files) = split_fixture(vec![mk("server/src/a.cs"), mk("app/x.ts"), mk("README.md")], Some(&doc), "m");
        assert_eq!(repos.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["server", "app", ""]);
        assert_eq!(files[0].path, "src/a.cs");
        assert_eq!(files[1].repo, 1);
        assert_eq!(files[2].display_path(&repos), "README.md");
        let (repos, _) = split_fixture(vec![mk("server/src/a.cs")], None, "m");
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "m");
    }

    #[test]
    fn files_load_in_the_triage_order_with_a_tag_each() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for overview in [None, Some(root.join("fixtures/overview/sample-overview.json"))] {
            let launch = Launch {
                fixture: Some(root.join("fixtures/m.patch")),
                overview: overview.clone(),
                ..Launch::default()
            };
            let d = load(&launch, &Shared::default(), &|_| {}).unwrap();
            let order: Vec<&str> = d.triage.items.iter().map(|i| i.key.as_str()).collect();
            assert_eq!(order, d.keys.iter().map(String::as_str).collect::<Vec<_>>(), "{overview:?}");
            assert_eq!(d.files.len(), d.keys.len());
            assert!(d.keys.iter().all(|k| d.tags.contains_key(k)));
            assert!(d.files.iter().zip(&d.keys).all(|(f, k)| f.display_path(&d.repos) == *k));
        }
        let plain = Launch {
            fixture: Some(root.join("fixtures/m.patch")),
            ..Launch::default()
        };
        let d = load(&plain, &Shared::default(), &|_| {}).unwrap();
        let mig = d.keys.iter().find(|k| k.contains("/Migrations/")).unwrap();
        assert_eq!(d.tags[mig], FileTag::MustReview);
        assert_eq!(d.triage.facts.files, d.keys.len());
    }
}
