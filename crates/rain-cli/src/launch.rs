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
/// two repos, or two stories in one repo, never share a session file by accident. What else picks
/// the diff follows, so a different diff never reuses a window: `-uncommitted`, `-working` or
/// `-staged` for those targets, then `-<hash>` of the base, `--merge-base`,
/// `--include-untracked` and the other repos when any differs from a plain `rain open <target>`.
pub fn default_session(o: &OpenArgs, dirs: &[PathBuf], target: &str, base: &str) -> String {
    if let Some(f) = o.fixture.as_deref() {
        let stem = f.file_stem().map_or_else(|| "fixture".into(), |s| s.to_string_lossy().into_owned());
        return sanitize(&format!("fixture-{stem}"));
    }
    let dir = dirs.first().map_or(Path::new("."), PathBuf::as_path);
    let folder = folder_name(dir).unwrap_or_else(|| "rain".into());
    let label = if target.eq_ignore_ascii_case("HEAD") || is_special(target) {
        current_branch(dir).unwrap_or_else(|| "HEAD".into())
    } else {
        target.to_owned()
    };
    let mut id = format!("{folder}-{label}");
    if is_special(target) {
        id.push('-');
        id.push_str(match target.to_ascii_lowercase().as_str() {
            "working" => "working",
            "staged" => "staged",
            _ => "uncommitted",
        });
    }
    let plain = base == default_base(target) && !o.merge_base && !o.include_untracked && dirs.len() == 1;
    if !plain {
        let mut spec = format!("{base}\0{}\0{}", o.merge_base, o.include_untracked);
        for d in &dirs[1..] {
            spec.push('\0');
            spec.push_str(&absolute(d).to_string_lossy());
        }
        id.push_str(&format!("-{:08x}", fnv1a(spec.as_bytes()) as u32));
    }
    sanitize(&id)
}

/// FNV-1a: a stable hash, the same in every build.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
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
    let session = o.session.clone().unwrap_or_else(|| default_session(o, &dirs, &target, &base));
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
        let session = build_launch(&o).session;
        return background::spawn(args, |port| brief_hint(port, &session));
    }
    open(o, rune)
}

/// [`background::brief_hint`] when the window on `port` holds no brief.
fn brief_hint(port: u16, session: &str) -> Option<String> {
    (crate::client::has_brief(port) == Some(false)).then(|| background::brief_hint(session))
}

/// Prints the running window's handshake with `"reused":true` after handing it the start-up
/// comments, overview and `--goto` this call carried.
fn reuse(o: &OpenArgs, session: &str, lock: &rain_server::Lock) -> i32 {
    if o.clean {
        eprintln!("Warning: session {session} is open (pid {}); --clean ignored", lock.pid);
    }
    let port = lock.port.to_string();
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("rain"));
    for c in reuse_calls(o) {
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
    let line = background::with_extras(&background::reused_line(lock.port, lock.pid), lock.token.as_deref(), false);
    let hint = if o.background { brief_hint(lock.port, session) } else { None };
    background::relay(&line, hint.as_deref(), &mut std::io::stdout(), &mut std::io::stderr());
    0
}

/// The client calls that hand a running window what `o` carried. `--comment` imports keep no
/// author, as when the window opens fresh with them: they are the reviewer's.
fn reuse_calls(o: &OpenArgs) -> Vec<Vec<String>> {
    let mut calls: Vec<Vec<String>> = o
        .comments
        .iter()
        .map(|c| vec!["comment".into(), "add".into(), c.clone(), "--author=".into()])
        .collect();
    if let Some(ov) = &o.overview {
        calls.push(vec!["overview".into(), "set".into(), ov.display().to_string()]);
    }
    if let Some(g) = &o.goto {
        calls.push(vec!["goto".into(), g.clone()]);
    }
    calls
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
            if !background::is_child() {
                eprintln!("{}", background::READ_ONLY_WARNING);
            }
        } else if changed {
            save();
        }
        crate::sessions::write_record(&rain_comments::store::data_dir(), &launch);
        let line = background::handshake_line(handle.port, &handle.url, std::process::id());
        println!("{}", background::with_extras(&line, Some(&handle.token), handle.read_only));
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

    /// What `rain open --background` writes for `line` from a window that has, or has no, brief.
    fn opened(has: bool) -> (String, String) {
        let shared = Arc::new(Shared::default());
        if has {
            shared.lock().brief = Some(rain_core::brief::BriefDoc::default());
        }
        let server = rain_server::start(shared, Some(0)).unwrap();
        let line = background::handshake_line(server.port, &server.url, 1234);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        background::relay(&line, brief_hint(server.port, "s1").as_deref(), &mut out, &mut err);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("{line}\n"),
            "stdout is the handshake alone"
        );
        (line, String::from_utf8(err).unwrap())
    }

    #[test]
    fn open_hints_at_the_brief_on_stderr_only_when_there_is_none() {
        let (_, err) = opened(false);
        assert_eq!(
            err,
            "next: send a brief (intent, how, tour) with: rain brief set <file.md> --session s1; see docs/agent.md §4\n"
        );
        let (_, err) = opened(true);
        assert_eq!(err, "", "no hint once a brief is set");
    }

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
        let server = [PathBuf::from("C:/src/acme-server")];
        let plain = OpenArgs::default();
        let s = default_session(&plain, &server, "feature/ACME-1", "feature/ACME-1^");
        assert_eq!(s, "acme-server-feature_ACME-1");
        let fixture = OpenArgs {
            fixture: Some("fixtures/m.patch".into()),
            ..OpenArgs::default()
        };
        assert_eq!(default_session(&fixture, &[PathBuf::from(".")], "HEAD", "HEAD^"), "fixture-m");
    }

    #[test]
    fn a_different_diff_gets_a_different_default_session() {
        let id = |args: &[&str]| {
            let mut o = OpenArgs {
                repos: vec![PathBuf::from("C:/src/acme-server")],
                ..OpenArgs::default()
            };
            let words: Vec<&str> = args.iter().copied().filter(|a| !a.starts_with("--")).collect();
            o.target = words.first().map(|s| (*s).to_owned());
            o.base = words.get(1).map(|s| (*s).to_owned());
            o.merge_base = args.contains(&"--merge-base");
            o.include_untracked = args.contains(&"--include-untracked");
            if args.contains(&"--repo") {
                o.repos.push(PathBuf::from("C:/src/acme-app"));
            }
            build_launch(&o).session
        };
        let feature = id(&["feature/x"]);
        assert_eq!(feature, "acme-server-feature_x", "a plain open keeps its name");
        assert_eq!(id(&["feature/x", "feature/x^"]), feature, "the default base spelled out");
        let all = [
            feature,
            id(&["feature/x", "origin/development"]),
            id(&["feature/x", "origin/development", "--merge-base"]),
            id(&["feature/x", "origin/main", "--merge-base"]),
            id(&["feature/x", "--include-untracked"]),
            id(&["feature/x", "--repo"]),
            id(&["HEAD"]),
            id(&["."]),
            id(&["working"]),
            id(&["staged"]),
        ];
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert!(all[7].ends_with("-uncommitted"), "{}", all[7]);
        assert_eq!(id(&["feature/x", "origin/development", "--merge-base"]), all[2], "stable");
    }

    #[test]
    fn reused_windows_get_start_up_comments_without_an_author_as_fresh_ones_do() {
        use clap::Parser;

        use crate::args::{Cli, CommentCmd, Sub};

        let q = r#"{"type":"thread","filePath":"a.cs","position":{"side":"new","line":3},"body":"Why?","kind":"question"}"#;
        let o = OpenArgs {
            comments: vec![q.into()],
            goto: Some("a.cs:3".into()),
            ..OpenArgs::default()
        };
        let calls = reuse_calls(&o);
        assert_eq!(calls[0], ["comment", "add", q, "--author="]);
        assert_eq!(calls[1], ["goto", "a.cs:3"]);
        let cli = Cli::try_parse_from(
            std::iter::once("rain")
                .chain(calls[0].iter().map(String::as_str))
                .chain(["--port", "1"]),
        )
        .unwrap();
        let Some(Sub::Comment { cmd: CommentCmd::Add(a) }) = cli.command else {
            panic!("not a comment add")
        };
        let reused = crate::client::add_imports(&a).unwrap();
        let fresh = prepare_session(&o, &build_launch(&o), Session::default()).unwrap();
        assert_eq!(reused[0].body().author, None);
        assert_eq!(fresh.threads[0].messages[0].author, None, "both are the reviewer's question");
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
