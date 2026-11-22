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

/// One spelling per checkout or patch: the path on disk (its real case, links followed), or the
/// absolute path when nothing is there.
fn canonical(path: &Path) -> String {
    match std::fs::canonicalize(path) {
        Ok(p) => p.to_string_lossy().trim_start_matches(r"\\?\").to_owned(),
        Err(_) => absolute(path).to_string_lossy().into_owned(),
    }
}

/// `<repo folder>-<branch>` (or `-<target>` when the target names a revision other than HEAD), so
/// two repos, or two stories in one repo, never share a session file by accident. What else picks
/// the diff follows, so a different diff never reuses a window: `-uncommitted`, `-working` or
/// `-staged` for those targets, then a stable hash of the checkout paths, revisions,
/// `--merge-base` and `--include-untracked`. Whitespace and context only change how the same
/// diff is shown, so they stay out of the name.
pub fn default_session(o: &OpenArgs, dirs: &[PathBuf], target: &str, base: &str) -> String {
    if let Some(f) = o.fixture.as_deref() {
        let path = canonical(f);
        let stem = Path::new(&path)
            .file_stem()
            .map_or_else(|| "fixture".into(), |s| s.to_string_lossy().into_owned());
        return sanitize(&format!("fixture-{stem}-{:016x}", fnv1a(path.as_bytes())));
    }
    let dir = dirs.first().map_or(Path::new("."), PathBuf::as_path);
    let paths: Vec<String> = dirs.iter().map(|d| canonical(d)).collect();
    let folder = paths
        .first()
        .and_then(|p| Path::new(p).file_name())
        .map_or_else(|| "rain".into(), |n| n.to_string_lossy().into_owned());
    let mut id = format!("{folder}-{}", revision_label(dir, target));
    id.push_str(&format!("-{:016x}", identity(o, target, base, &paths)));
    sanitize(&id)
}

/// `<branch or target>`, then `-uncommitted`, `-working` or `-staged` for those targets.
fn revision_label(dir: &Path, target: &str) -> String {
    let mut label = if target.eq_ignore_ascii_case("HEAD") || is_special(target) {
        current_branch(dir).unwrap_or_else(|| "HEAD".into())
    } else {
        target.to_owned()
    };
    if is_special(target) {
        label.push('-');
        label.push_str(match target.to_ascii_lowercase().as_str() {
            "working" => "working",
            "staged" => "staged",
            _ => "uncommitted",
        });
    }
    label
}

/// The hash in an automatic session name. Changing what goes in renames every automatic session.
fn identity(o: &OpenArgs, target: &str, base: &str, paths: &[String]) -> u64 {
    let mut spec = format!("{target}\0{base}\0{}\0{}", o.merge_base, o.include_untracked);
    for p in paths {
        spec.push('\0');
        spec.push_str(p);
    }
    fnv1a(spec.as_bytes())
}

/// The automatic name before it carried the checkout hash.
fn legacy_default_session(o: &OpenArgs, dirs: &[PathBuf], target: &str, base: &str) -> String {
    if let Some(f) = o.fixture.as_deref() {
        let stem = f.file_stem().map_or_else(|| "fixture".into(), |s| s.to_string_lossy().into_owned());
        return sanitize(&format!("fixture-{stem}"));
    }
    let dir = dirs.first().map_or(Path::new("."), PathBuf::as_path);
    let folder = folder_name(dir).unwrap_or_else(|| "rain".into());
    let mut id = format!("{folder}-{}", revision_label(dir, target));
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

/// The session a launch without `--session` uses: [`default_session`], or the older name when
/// only that one is saved and it was last opened on this checkout, so an upgrade keeps its review.
fn automatic_session(o: &OpenArgs, dirs: &[PathBuf], target: &str, base: &str, data_dir: &Path) -> String {
    let new = default_session(o, dirs, target, base);
    let old = legacy_default_session(o, dirs, target, base);
    let saved = |id: &str| SessionFile::path_in(&data_dir.join("sessions"), id).exists();
    if !saved(&new) && saved(&old) && opened_here(data_dir, &old, o, dirs) {
        old
    } else {
        new
    }
}

/// Whether `session`'s launch record names this fixture or these checkouts. A session with no
/// readable record predates them and counts as this one.
fn opened_here(data_dir: &Path, session: &str, o: &OpenArgs, dirs: &[PathBuf]) -> bool {
    let record = std::fs::read_to_string(crate::sessions::record_path(data_dir, session))
        .ok()
        .and_then(|text| serde_json::from_str::<crate::sessions::LaunchRecord>(&text).ok());
    let Some(record) = record else { return true };
    match (&record.fixture, &o.fixture) {
        (Some(theirs), Some(ours)) => canonical(theirs) == canonical(ours),
        (None, None) => record.repos.iter().map(|d| canonical(d)).eq(dirs.iter().map(|d| canonical(d))),
        _ => false,
    }
}

/// FNV-1a: a stable hash, the same in every build.
fn fnv1a(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

pub fn build_launch(o: &OpenArgs) -> Launch {
    build_launch_in(o, &rain_comments::store::data_dir())
}

fn build_launch_in(o: &OpenArgs, data_dir: &Path) -> Launch {
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
        .unwrap_or_else(|| automatic_session(o, &dirs, &target, &base, data_dir));
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
        read_only_reason: None,
        session_notice: None,
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

/// The saved session, and whether it is the backup of a file that could not be read.
fn load_saved(file: &Path, clean: bool) -> Result<(Session, bool), String> {
    if clean {
        return Ok((Session::default(), false));
    }
    SessionFile::load_or_backup(file).map_err(|e| format!("cannot read session {}: {e}", file.display()))
}

/// For the window that owns the session: moves the unreadable file out of the way of the next
/// save, and says what happened.
fn set_unreadable_aside(file: &Path) -> String {
    let kept = SessionFile::set_aside(file)
        .inspect_err(|e| eprintln!("Rain: {} could not be moved aside: {e}", file.display()))
        .ok();
    let notice = SessionFile::backup_notice(file, kept.as_deref());
    eprintln!("Rain: {notice}");
    notice
}

fn open_window(o: &OpenArgs, rune: &RuneArgs) -> Result<(), String> {
    let launch = build_launch(o);
    let file = SessionFile::path(&launch.session);
    // Headless runs (snaps, scripts) read the saved session but never write it or serve routes.
    let live = !rune.headless;
    if live {
        rain_app::warm_gpu();
    }
    let (saved, from_backup) = load_saved(&file, launch.clean)?;
    let mut session_notice = from_backup.then(|| SessionFile::backup_notice(&file, None));
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
            // The server has already said why the lock could not be taken.
            if !background::is_child() && handle.lock_error.is_none() {
                eprintln!("{}", background::READ_ONLY_WARNING);
            }
        } else {
            if from_backup {
                session_notice = Some(set_unreadable_aside(&file));
            }
            if changed || from_backup {
                save();
            }
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
        read_only_reason: server
            .as_ref()
            .and_then(|h| h.lock_error.as_ref())
            .map(|e| format!("The session lock could not be taken ({e})")),
        session_notice,
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

    /// [`super::build_launch`] with no data folder, so no saved session can change a name.
    fn build_launch(o: &OpenArgs) -> Launch {
        build_launch_in(o, Path::new("no-data-folder"))
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rain-cli-launch-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sessions")).unwrap();
        d
    }

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
    fn an_unreadable_session_opens_from_its_backup_and_the_owner_moves_it_aside() {
        let data = temp("backup");
        let file = SessionFile::path_in(&data.join("sessions"), "s");
        let saved = Session {
            id: "s".into(),
            version: 4,
            ..Session::default()
        };
        SessionFile::save(&file, &saved).unwrap();
        SessionFile::save(&file, &saved).unwrap();
        assert_eq!(load_saved(&file, false).unwrap(), (saved.clone(), false));

        std::fs::write(&file, b"{\"threads\": [").unwrap();
        assert_eq!(load_saved(&file, false).unwrap(), (saved.clone(), true));
        assert_eq!(
            load_saved(&file, true).unwrap(),
            (Session::default(), false),
            "--clean reads nothing"
        );
        assert!(file.exists(), "a window that may be read-only moves nothing");

        let notice = set_unreadable_aside(&file);
        assert_eq!(
            notice,
            "s.json was unreadable: opened the save before it; the unreadable file is kept as s.json.corrupt"
        );
        assert_eq!(std::fs::read(file.with_extension("json.corrupt")).unwrap(), b"{\"threads\": [");
        SessionFile::save(&file, &saved).unwrap();
        assert_eq!(load_saved(&file, false).unwrap(), (saved, false), "the owner's save puts it back");

        std::fs::write(&file, b"bad").unwrap();
        std::fs::write(SessionFile::backup_path(&file), b"bad too").unwrap();
        let e = load_saved(&file, false).unwrap_err();
        assert!(e.starts_with("cannot read session "), "both unreadable fails as before: {e}");
        let _ = std::fs::remove_dir_all(data);
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
        assert!(s.starts_with("acme-server-feature_ACME-1-"));
        let fixture = OpenArgs {
            fixture: Some("fixtures/m.patch".into()),
            ..OpenArgs::default()
        };
        assert!(default_session(&fixture, &[PathBuf::from(".")], "HEAD", "HEAD^").starts_with("fixture-m-"));
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
        assert!(feature.starts_with("acme-server-feature_x-"), "the name stays readable");
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
        assert!(all[7].contains("-uncommitted-"), "{}", all[7]);
        assert_eq!(id(&["feature/x", "origin/development", "--merge-base"]), all[2], "stable");
    }

    #[test]
    fn automatic_sessions_follow_the_checkout_and_the_diff_not_how_it_is_shown() {
        let plain = OpenArgs {
            repos: vec![PathBuf::from("/one/repo")],
            target: Some("feature".into()),
            ..OpenArgs::default()
        };
        let mut variants = vec![plain.clone(); 3];
        variants[1].repos = vec![PathBuf::from("/two/repo")];
        variants[2].target = Some("feature/other".into());
        let ids: Vec<_> = variants.iter().map(|o| build_launch(o).session).collect();
        for (i, id) in ids.iter().enumerate() {
            for other in &ids[i + 1..] {
                assert_ne!(id, other, "different launch inputs must not silently reuse a window");
            }
        }
        let shown = OpenArgs {
            ignore_whitespace: true,
            context: plain.context + 7,
            ..plain.clone()
        };
        assert_eq!(
            build_launch(&shown).session,
            ids[0],
            "-w and --context reopen the review they are set on"
        );
        let fixture = |path: &str| {
            build_launch(&OpenArgs {
                fixture: Some(path.into()),
                ..OpenArgs::default()
            })
            .session
        };
        assert_ne!(fixture("/one/review.patch"), fixture("/two/review.patch"));
        let explicit = OpenArgs {
            session: Some("chosen".into()),
            ..plain
        };
        assert_eq!(build_launch(&explicit).session, "chosen");
    }

    #[test]
    fn one_checkout_spelled_differently_is_one_session() {
        let root = temp("spelling");
        let dir = root.join("Acme-Server");
        std::fs::create_dir_all(&dir).unwrap();
        let patch = root.join("Review.patch");
        std::fs::write(&patch, "").unwrap();
        let repo = |d: PathBuf| default_session(&OpenArgs::default(), &[d], "feature/x", "feature/x^");
        let fixture = |f: PathBuf| {
            let o = OpenArgs {
                fixture: Some(f),
                ..OpenArgs::default()
            };
            default_session(&o, &[PathBuf::from(".")], "HEAD", "HEAD^")
        };
        let name = repo(dir.clone());
        assert!(name.starts_with("Acme-Server-feature_x-"), "{name}");
        let mut trailing = dir.clone().into_os_string();
        trailing.push(std::path::MAIN_SEPARATOR_STR);
        assert_eq!(repo(trailing.into()), name, "a trailing separator");
        assert_eq!(repo(dir.join("..").join("Acme-Server")), name, "a detour");
        if cfg!(windows) {
            let upper = |p: &Path| PathBuf::from(p.to_string_lossy().to_uppercase());
            assert_eq!(repo(upper(&dir)), name, "another case");
            assert_eq!(fixture(upper(&patch)), fixture(patch.clone()), "another case");
        }
        assert_ne!(repo(root.join("missing")), name);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_session_saved_under_the_older_name_is_kept_on_its_checkout() {
        let data = temp("legacy");
        let o = OpenArgs {
            repos: vec![PathBuf::from("C:/src/acme-server")],
            target: Some("feature/x".into()),
            ..OpenArgs::default()
        };
        let named = |o: &OpenArgs| build_launch_in(o, &data).session;
        let new = named(&o);
        assert!(new.starts_with("acme-server-feature_x-"), "{new}");

        let old = SessionFile::path_in(&data.join("sessions"), "acme-server-feature_x");
        std::fs::write(&old, "{}").unwrap();
        assert_eq!(named(&o), "acme-server-feature_x", "saved before launch records");
        let record = |dir: &str| {
            let launch = Launch {
                session: "acme-server-feature_x".into(),
                repos: vec![RepoLaunch {
                    dir: PathBuf::from(dir),
                    ..RepoLaunch::default()
                }],
                ..Launch::default()
            };
            crate::sessions::write_record(&data, &launch);
        };
        record("C:/src/acme-server");
        assert_eq!(named(&o), "acme-server-feature_x", "last opened on this checkout");
        record("D:/play/acme-server");
        assert_eq!(named(&o), new, "another checkout's session of the same name is not taken");

        record("C:/src/acme-server");
        let explicit = OpenArgs {
            session: Some("chosen".into()),
            ..o.clone()
        };
        assert_eq!(named(&explicit), "chosen");
        let other_diff = OpenArgs {
            base: Some("origin/main".into()),
            ..o.clone()
        };
        assert!(named(&other_diff).starts_with("acme-server-feature_x-"), "its older name differs");
        std::fs::write(SessionFile::path_in(&data.join("sessions"), &new), "{}").unwrap();
        assert_eq!(named(&o), new, "once the new name is saved it wins");
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn a_fixture_session_saved_under_the_older_name_is_kept_for_that_patch() {
        let data = temp("legacy-fixture");
        let o = OpenArgs {
            fixture: Some("C:/patches/review.patch".into()),
            ..OpenArgs::default()
        };
        let new = build_launch_in(&o, &data).session;
        std::fs::write(SessionFile::path_in(&data.join("sessions"), "fixture-review"), "{}").unwrap();
        let record = |patch: &str| {
            let launch = Launch {
                session: "fixture-review".into(),
                fixture: Some(patch.into()),
                ..Launch::default()
            };
            crate::sessions::write_record(&data, &launch);
        };
        record("C:/patches/review.patch");
        assert_eq!(build_launch_in(&o, &data).session, "fixture-review");
        record("D:/other/review.patch");
        assert_eq!(build_launch_in(&o, &data).session, new);
        let _ = std::fs::remove_dir_all(data);
    }

    #[test]
    fn the_automatic_session_hash_is_pinned() {
        let o = OpenArgs {
            merge_base: true,
            ..OpenArgs::default()
        };
        let paths = ["/src/acme-server".to_owned(), "/src/acme-app".to_owned()];
        let hash = identity(&o, "feature/x", "origin/main", &paths);
        assert_eq!(
            format!("{hash:016x}"),
            "6a919c187c64d793",
            "changing this renames every saved automatic session"
        );
        assert_eq!(format!("{:016x}", fnv1a(b"/fixtures/m.patch")), "c6810be004a5d6a0");

        let dirs = [PathBuf::from("C:/src/acme-server"), PathBuf::from("C:/src/acme-app")];
        let spelled: Vec<String> = dirs.iter().map(|d| canonical(d)).collect();
        assert_eq!(
            default_session(&o, &dirs, "feature/x", "origin/main"),
            format!("acme-server-feature_x-{:016x}", identity(&o, "feature/x", "origin/main", &spelled))
        );
        let fixture = OpenArgs {
            fixture: Some("C:/patches/m.patch".into()),
            ..OpenArgs::default()
        };
        assert_eq!(
            default_session(&fixture, &[PathBuf::from(".")], "HEAD", "HEAD^"),
            format!("fixture-m-{:016x}", fnv1a(canonical(Path::new("C:/patches/m.patch")).as_bytes()))
        );
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
