//! Shared helpers; each area's tests live in its own file.

mod ask;
mod board;
mod brief;
mod composer;
mod extras;
mod filter;
mod layout;
mod listing;
mod notes;
mod overview;
mod peek;
mod remote;
mod rest;
mod rounds;
mod ruler;
mod selection;
mod shell;
mod theme;
mod tour;
mod verify;
mod wholefile;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rain_core::{Launch, RepoLaunch, Shared};
use rune::{Env, Key, KeyEvent, Mods, Runtime};

use super::*;

pub(crate) fn workspace(p: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(p)
}

pub(crate) fn story() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/story.patch")
}

pub(crate) fn overview() -> PathBuf {
    workspace("fixtures/overview/sample-overview-schema.json")
}

pub(crate) fn temp_dir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!("rain-app-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// A scene's setup by name.
pub(crate) fn shell_scene(name: &str) -> fn(&mut App) {
    crate::spec().scenes.iter().find(|(n, _)| *n == name).map(|(_, f)| *f).unwrap()
}

pub(crate) fn app_with(launch: Launch) -> App {
    let dir = temp_dir();
    let mut a = App::new(
        launch,
        Arc::new(Shared::default()),
        Some(dir.join("sessions")),
        Some(dir.join("settings.json")),
    );
    a.load_now();
    a
}

pub(crate) fn app(fixture: PathBuf, with_overview: bool) -> App {
    app_with(Launch {
        session: "t".into(),
        fixture: Some(fixture),
        overview: with_overview.then(overview),
        ..Launch::default()
    })
}

pub(crate) fn runtime(a: App) -> Runtime<App> {
    sized_runtime(a, (1600.0, 1000.0))
}

pub(crate) fn sized_runtime(a: App, size: (f32, f32)) -> Runtime<App> {
    let mut rt = Runtime::new(
        a,
        view,
        size,
        Env {
            system_dark: true,
            scale: 1.0,
        },
    );
    rt.on_key = Some(on_key);
    rt.poll = Some(|s: &mut App, _, tx| s.poll(tx));
    rt.frame(0.0);
    settle(&mut rt);
    rt
}

pub(crate) fn settle(rt: &mut Runtime<App>) {
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
}

pub(crate) fn press(rt: &mut Runtime<App>, key: Key, mods: Mods) {
    rt.key(KeyEvent { key, mods });
    settle(rt);
}

pub(crate) fn typed(rt: &mut Runtime<App>, s: &str) {
    for c in s.chars() {
        press(rt, Key::Text(c.to_string()), Mods::default());
    }
}

pub(crate) fn ctrl() -> Mods {
    Mods {
        ctrl: true,
        cmd: true,
        ..Mods::default()
    }
}

pub(crate) fn git(dir: &std::path::Path, args: &[&str]) {
    let out = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
}

pub(crate) fn numbered(n: usize, edit: impl Fn(usize) -> String) -> String {
    (1..=n).map(|i| edit(i) + "\n").collect()
}

/// A repo whose last commit changes line 40 of a.cs and only re-indents ws.cs.
pub(crate) fn scratch_repo() -> PathBuf {
    let d = temp_dir().join("repo");
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    for (k, v) in [
        ("user.name", "t"),
        ("user.email", "t@t.invalid"),
        ("core.autocrlf", "false"),
        ("commit.gpgsign", "false"),
    ] {
        git(&d, &["config", k, v]);
    }
    std::fs::write(d.join("a.cs"), numbered(80, |i| format!("int a{i};"))).unwrap();
    std::fs::write(d.join("ws.cs"), "class W {\n    int x;\n}\n").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "one"]);
    std::fs::write(
        d.join("a.cs"),
        numbered(80, |i| if i == 40 { "int forty;".into() } else { format!("int a{i};") }),
    )
    .unwrap();
    std::fs::write(d.join("ws.cs"), "class W {\n\tint x;\n}\n").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "two"]);
    d
}

pub(crate) fn repo_app(dir: &std::path::Path, shared: Arc<Shared>, ignore_whitespace: bool) -> App {
    repo_app_from(dir, "HEAD~1", shared, ignore_whitespace)
}

pub(crate) fn repo_app_from(dir: &std::path::Path, base: &str, shared: Arc<Shared>, ignore_whitespace: bool) -> App {
    let mut a = App::new(
        Launch {
            session: "t".into(),
            repos: vec![RepoLaunch {
                dir: dir.to_path_buf(),
                base: base.into(),
                target: "HEAD".into(),
                ..RepoLaunch::default()
            }],
            ignore_whitespace,
            ..Launch::default()
        },
        shared,
        None,
        None,
    );
    a.load_now();
    a
}
