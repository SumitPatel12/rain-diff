//! Opening the window: session from disk, start-up comments and overview, the HTTP routes, the
//! handshake line, then the app.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use rain_comments::store::sanitize;
use rain_comments::{SessionFile, from_difit_json, import};
use rain_core::overview::Issue;
use rain_core::{Launch, RepoLaunch, Session, Shared};
use rain_server::{ServerOptions, set_overview, start_with};

use crate::args::OpenArgs;
use crate::passthrough::RuneArgs;
use crate::{background, exit_log};

const SPECIAL_TARGETS: [&str; 4] = [".", "working", "staged", "WORKTREE"];

fn is_special(target: &str) -> bool {
    SPECIAL_TARGETS.iter().any(|t| t.eq_ignore_ascii_case(target))
}

/// difit's default base: the target's parent, `staged` for unstaged changes, `HEAD` for the
/// working tree or the index.
pub fn default_base(target: &str) -> String {
    match target {
        "working" => "staged".into(),
        t if is_special(t) => "HEAD".into(),
        t => format!("{t}^"),
    }
}

fn absolute(dir: &Path) -> PathBuf {
    std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf())
}

fn folder_name(dir: &Path) -> Option<String> {
    absolute(dir).file_name().map(|n| n.to_string_lossy().into_owned())
}

fn current_branch(dir: &Path) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(["rev-parse", "--abbrev-ref", "HEAD"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok().filter(|o| o.status.success())?;
    let branch = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!branch.is_empty()).then_some(branch)
}

/// `<repo folder>-<branch>` (or `-<target>` when the target names a revision other than HEAD), so
/// two repos, or two stories in one repo, never share a session file by accident.
pub fn default_session(dir: &Path, target: &str, fixture: Option<&Path>) -> String {
    if let Some(f) = fixture {
        let stem = f.file_stem().map_or_else(|| "fixture".into(), |s| s.to_string_lossy().into_owned());
        return sanitize(&format!("fixture-{stem}"));
    }
    let folder = folder_name(dir).unwrap_or_else(|| "rain".into());
    let label = if target.eq_ignore_ascii_case("HEAD") || is_special(target) {
        current_branch(dir).unwrap_or_else(|| "HEAD".into())
    } else {
        target.to_owned()
    };
    sanitize(&format!("{folder}-{label}"))
}

pub fn build_launch(o: &OpenArgs) -> Launch {
    let target = o.target.clone().unwrap_or_else(|| "HEAD".into());
    let base = o.base.clone().unwrap_or_else(|| default_base(&target));
    let dirs: Vec<PathBuf> = if o.repos.is_empty() {
        vec![PathBuf::from(".")]
    } else {
        o.repos.clone()
    };
    let repos: Vec<RepoLaunch> = dirs
        .iter()
        .map(|d| RepoLaunch {
            dir: absolute(d),
            base: base.clone(),
            target: target.clone(),
            merge_base: o.merge_base,
            include_untracked: o.include_untracked,
        })
        .collect();
    let session = o
        .session
        .clone()
        .unwrap_or_else(|| default_session(&dirs[0], &target, o.fixture.as_deref()));
    Launch {
        session,
        repos,
        ignore_whitespace: o.ignores_whitespace(),
        port: o.port,
        comments: o.comments.iter().filter_map(|c| c.strip_prefix('@')).map(PathBuf::from).collect(),
        overview: o.overview.as_deref().map(absolute),
        goto: o.goto.clone(),
        clean: o.clean,
        fixture: o.fixture.as_deref().map(absolute),
        context: Some(o.context),
        read_only: false,
    }
}

/// `--comment` value: JSON, or `@file` holding JSON.
fn comment_text(arg: &str) -> Result<String, String> {
    match arg.strip_prefix('@') {
        Some(file) => std::fs::read_to_string(file).map_err(|e| format!("Cannot read {file}: {e}")),
        None => Ok(arg.to_owned()),
    }
}

/// difit's `printInvalid`.
pub fn format_issues(issues: &[Issue]) -> String {
    let mut s = format!("Invalid overview ({} issue(s))", issues.len());
    for i in issues {
        s.push_str(&format!("\n  {}: {} [{}]", i.path, i.message, i.code));
    }
    s
}

/// Applies `--clean`, `--comment` and `--overview` to the saved session.
pub fn prepare_session(o: &OpenArgs, launch: &Launch, saved: Session) -> Result<Session, String> {
    let mut session = if launch.clean { Session::default() } else { saved };
    if session.id.is_empty() {
        session.id = launch.session.clone();
    }
    for c in &o.comments {
        let imports = from_difit_json(&comment_text(c)?)?;
        for w in import::apply(&mut session, imports, "").warnings {
            eprintln!("Warning: {w}");
        }
    }
    if let Some(path) = &launch.overview {
        let loaded = rain_overview::load(path).map_err(|issues| format_issues(&issues))?;
        for w in &loaded.warnings {
            eprintln!("warning: {w}");
        }
        let self_repo = self_folder(launch).and_then(|f| rain_overview::self_repo(&loaded.document, &f).map(str::to_owned));
        set_overview(&mut session, loaded.document, self_repo, BTreeMap::new());
    }
    Ok(session)
}

/// The checkout folder the overview's self repo is matched against: a one-repo window's only
/// repo. A multi-repo window has no self repo (`selfRepo` stays null unless `--self` names one).
fn self_folder(launch: &Launch) -> Option<String> {
    match launch.repos.as_slice() {
        [only] => folder_name(&only.dir),
        _ => None,
    }
}

fn log_path(session: &str) -> PathBuf {
    rain_comments::store::data_dir()
        .join("logs")
        .join(format!("{}-{}.log", sanitize(session), std::process::id()))
}

/// The window form: `--last` picks the session, a window already serving it is reused,
/// `--background` starts a detached copy, else the window opens here.
pub fn open_or_reuse(o: OpenArgs, rune: &RuneArgs, args: &[String]) -> i32 {
    let o = if o.last {
        match crate::sessions::last_open(&o, &rain_comments::store::data_dir()) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("Error: {e}");
                return 1;
            }
        }
    } else {
        o
    };
    if !rune.headless {
        let session = build_launch(&o).session;
        if let Ok(lock) = crate::sessions::live_lock(&SessionFile::dir(), &session) {
            return reuse(&o, &session, &lock);
        }
    }
    if o.background && !background::is_child() {
        return background::spawn(args);
    }
    open(o, rune)
}

/// Prints the running window's handshake with `"reused":true` after handing it the start-up
/// comments, overview and `--goto` this call carried.
fn reuse(o: &OpenArgs, session: &str, lock: &rain_server::Lock) -> i32 {
    if o.clean {
        eprintln!("Warning: session {session} is open (pid {}); --clean ignored", lock.pid);
    }
    let port = lock.port.to_string();
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rain"));
    let mut calls: Vec<Vec<String>> = o.comments.iter().map(|c| vec!["comment".into(), "add".into(), c.clone()]).collect();
    if let Some(ov) = &o.overview {
        calls.push(vec!["overview".into(), "set".into(), ov.display().to_string()]);
    }
    if let Some(g) = &o.goto {
        calls.push(vec!["goto".into(), g.clone()]);
    }
    for c in calls {
        let code = crate::run(
            std::iter::once(exe.display().to_string())
                .chain(c)
                .chain(["--port".to_owned(), port.clone()])
                .collect(),
        );
        // Warnings only warn, as they do when the window opens fresh.
        if code != 0 && code != crate::client::EXIT_WARNINGS {
            return code;
        }
    }
    println!("{}", background::reused_line(lock.port, lock.pid));
    0
}

pub fn open(o: OpenArgs, rune: &RuneArgs) -> i32 {
    exit_log::install_panic_hook(exit_log::write);
    match open_window(&o, rune) {
        Ok(()) => 0,
        Err(msg) => {
            eprintln!("Error: {msg}");
            1
        }
    }
}

fn open_window(o: &OpenArgs, rune: &RuneArgs) -> Result<(), String> {
    let launch = build_launch(o);
    let file = SessionFile::path(&launch.session);
    // Headless runs (snaps, scripts) read the saved session but never write it or serve routes.
    let live = !rune.headless;
    if live {
        rain_app::warm_gpu();
    }
    let saved = if launch.clean {
        Session::default()
    } else {
        SessionFile::load(&file).map_err(|e| format!("cannot read session {}: {e}", file.display()))?
    };
    let loaded_version = saved.version;
    let session = prepare_session(o, &launch, saved)?;
    let changed = launch.clean || session.version != loaded_version;
    let shared = Arc::new(Shared::new(session));
    // A snapshot, so it can't land after a save of a request that came in first.
    let save = || {
        let snap = SessionFile::snapshot(&file, &shared.lock());
        if let Err(e) = snap.and_then(rain_comments::Snapshot::write) {
            eprintln!("Rain: saving {} failed: {e}", file.display());
        }
    };
    let server = if live {
        let opts = ServerOptions {
            preferred: launch.port,
            session_file: Some(file.clone()),
            repo_folder: self_folder(&launch),
        };
        let handle = start_with(shared.clone(), opts).map_err(|e| format!("cannot start the comment server: {e}"))?;
        if handle.read_only {
            eprintln!(
                "Warning: another window holds session {}; changes here won't be saved",
                launch.session
            );
        } else if changed {
            save();
        }
        crate::sessions::write_record(&rain_comments::store::data_dir(), &launch);
        println!("{}", background::handshake_line(handle.port, &handle.url, std::process::id()));
        let _ = std::io::stdout().flush();
        if background::is_child() {
            background::redirect_stdio(&log_path(&launch.session));
        }
        exit_log::log_console_events();
        exit_log::record(&format!("started: session {}, port {}", launch.session, handle.port));
        Some(handle)
    } else {
        None
    };
    // The session already holds the start-up comments; the app re-applying them would duplicate
    // repo-less imports in a multi-repo window, where it files them under the first repo.
    let launch = Launch {
        comments: Vec::new(),
        port: server.as_ref().map(|h| h.port).or(launch.port),
        read_only: server.as_ref().is_some_and(|h| h.read_only),
        ..launch
    };
    let read_only = launch.read_only;
    let close_requested = rain_app::run(launch, shared.clone());
    if let Some(handle) = server {
        exit_log::record(if close_requested {
            "window closed: close requested (close button, Alt+F4 or WM_CLOSE)"
        } else {
            "window closed: the event loop ended without a close request"
        });
        let port = handle.port;
        // Saved before the lock goes, so a window opened next reads this save.
        handle.stop_then(|| {
            if !read_only {
                save();
            }
        });
        exit_log::record(&format!("comment server on port {port} stopped"));
        exit_log::record("exit 0: normal exit");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bases_follow_difit() {
        assert_eq!(default_base("HEAD"), "HEAD^");
        assert_eq!(default_base("."), "HEAD");
        assert_eq!(default_base("staged"), "HEAD");
        assert_eq!(default_base("working"), "staged");
    }

    #[test]
    fn launch_from_the_story_form() {
        let o = OpenArgs {
            repos: vec!["a".into(), "b".into()],
            session: Some("review-ACME-1".into()),
            target: Some("HEAD".into()),
            base: Some("origin/development".into()),
            merge_base: true,
            comments: vec!["@c.json".into(), "[]".into()],
            ..OpenArgs::default()
        };
        let l = build_launch(&o);
        assert_eq!(l.session, "review-ACME-1");
        assert_eq!(l.repos.len(), 2);
        assert!(
            l.repos
                .iter()
                .all(|r| r.dir.is_absolute() && r.merge_base && r.base == "origin/development")
        );
        assert!(!l.ignore_whitespace);
        assert_eq!(l.comments, [PathBuf::from("c.json")]);
        let wide = build_launch(&OpenArgs { context: 10, ..o });
        assert_eq!(wide.context, Some(10));
    }

    #[test]
    fn default_session_names_the_folder_and_revision() {
        let s = default_session(Path::new("C:/src/acme-server"), "feature/ACME-1", None);
        assert_eq!(s, "acme-server-feature_ACME-1");
        assert_eq!(
            default_session(Path::new("."), "HEAD", Some(Path::new("fixtures/m.patch"))),
            "fixture-m"
        );
    }

    #[test]
    fn start_up_comments_and_overview_land_in_the_session() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let o = OpenArgs {
            session: Some("t".into()),
            repos: vec![root.join("fixtures")],
            comments: vec![format!("@{}", root.join("fixtures/difit/import.json").display())],
            overview: Some(root.join("fixtures/overview/sample-overview-schema.json")),
            ..OpenArgs::default()
        };
        let l = build_launch(&o);
        let s = prepare_session(&o, &l, Session::default()).unwrap();
        assert_eq!(s.id, "t");
        assert_eq!(s.threads.len(), 3);
        assert_eq!(s.threads[0].messages.len(), 3);
        assert!(s.overview.is_some());
        assert_eq!(s.overview_version, 1);

        let with_repos = |repos: &[&str]| OpenArgs {
            repos: repos.iter().map(PathBuf::from).collect(),
            comments: Vec::new(),
            ..o.clone()
        };
        let self_of = |o: &OpenArgs| prepare_session(o, &build_launch(o), Session::default()).unwrap().self_repo;
        assert_eq!(self_of(&with_repos(&["C:/src/acme-server"])).as_deref(), Some("server"));
        assert_eq!(
            self_of(&with_repos(&["C:/src/acme-server", "C:/src/acme-app"])),
            None,
            "a multi-repo window has no self repo"
        );

        let bad = OpenArgs {
            comments: vec![r#"{"type":"nope"}"#.into()],
            ..o
        };
        assert_eq!(
            prepare_session(&bad, &l, Session::default()).unwrap_err(),
            "Invalid comment import field: type"
        );
    }
}
