//! Edits under a working-tree or index diff raise the reload banner; the pollers are stepped by
//! hand at made-up times, so nothing here waits.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;
use crate::app::Stale;
use crate::app::edits::{MAX_WAIT, MIN_WAIT, Poller};

const HOUR: Duration = Duration::from_secs(3600);

/// [`scratch_repo`] with an uncommitted change to `a.cs`.
fn dirty_repo() -> PathBuf {
    let d = scratch_repo();
    std::fs::write(d.join("a.cs"), numbered(80, |i| format!("int b{i};"))).unwrap();
    d
}

fn open_repos(dirs: &[&Path], target: &str, untracked: bool) -> Runtime<App> {
    let mut a = App::new(
        Launch {
            session: "t".into(),
            repos: dirs
                .iter()
                .map(|d| RepoLaunch {
                    dir: d.to_path_buf(),
                    base: "HEAD".into(),
                    target: target.into(),
                    include_untracked: untracked,
                    ..RepoLaunch::default()
                })
                .collect(),
            ..Launch::default()
        },
        Arc::new(Shared::default()),
        None,
        None,
    );
    a.load_now();
    let rt = runtime(a);
    assert_eq!(rt.state.phase, Phase::Ready);
    rt
}

fn open(dir: &Path, target: &str, untracked: bool) -> Runtime<App> {
    open_repos(&[dir], target, untracked)
}

/// Steps `pollers` at `now` and draws two frames: how many checked, and how many of those
/// found a change.
fn tick(rt: &mut Runtime<App>, pollers: &mut [Poller], now: Instant) -> (usize, usize) {
    let (check, focus) = (rt.state.check(), rt.state.edit_focus());
    let ran: Vec<bool> = pollers
        .iter_mut()
        .filter_map(|p| p.tick(now, focus, |dir| check.edits(dir)))
        .collect();
    rt.frame(1.0 / 60.0);
    rt.frame(1.0 / 60.0);
    (ran.len(), ran.iter().filter(|c| **c).count())
}

fn edits_banner() -> Option<Stale> {
    Some(Stale::Edits { repo: None })
}

#[test]
fn an_edit_to_a_tracked_file_raises_the_banner_once_and_a_reload_takes_it_down() {
    let d = dirty_repo();
    let mut rt = open(&d, ".", false);
    let t0 = Instant::now();
    let mut ps = rt.state.edit_pollers(t0);
    assert_eq!(ps.len(), 1);
    assert_eq!(tick(&mut rt, &mut ps, t0), (0, 0), "nothing is due on opening");
    assert_eq!(tick(&mut rt, &mut ps, t0 + MIN_WAIT), (1, 0));
    assert_eq!(rt.state.reloader.banner, None);

    std::fs::write(d.join("ws.cs"), "class W {\n    int edited;\n}\n").unwrap();
    assert_eq!(tick(&mut rt, &mut ps, t0 + HOUR), (1, 1));
    assert_eq!(rt.state.reloader.banner, edits_banner());
    assert!(rt.state.shared.stale());
    settle(&mut rt);
    assert!(rt.find("reload-banner").is_some());

    let commits = rt.stats.commits;
    std::fs::write(d.join("a.cs"), numbered(90, |i| format!("int c{i};"))).unwrap();
    assert_eq!(
        tick(&mut rt, &mut ps, t0 + HOUR * 2),
        (1, 0),
        "a file already in the diff, edited again"
    );
    std::fs::write(d.join("ws.cs"), "class W {\n    int edited_again;\n}\n").unwrap();
    assert_eq!(tick(&mut rt, &mut ps, t0 + HOUR * 3), (1, 0));
    assert_eq!(rt.stats.commits, commits, "more edits under the banner redraw nothing");
    assert_eq!(rt.state.reloader.banner, edits_banner());

    press(
        &mut rt,
        Key::Text("R".into()),
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    assert_eq!(rt.state.reloader.banner, None, "a reload takes the banner down");
    assert!(!rt.state.shared.stale());
    assert_eq!(
        tick(&mut rt, &mut ps, t0 + HOUR * 4),
        (1, 0),
        "and compares with the diff it loaded"
    );
    assert_eq!(rt.state.reloader.banner, None);
}

#[test]
fn a_second_edit_to_a_file_already_in_the_diff_raises_the_banner() {
    let d = dirty_repo();
    let mut rt = open(&d, "working", false);
    let t0 = Instant::now();
    let mut ps = rt.state.edit_pollers(t0);
    std::fs::write(d.join("a.cs"), numbered(90, |i| format!("int c{i};"))).unwrap();
    assert_eq!(tick(&mut rt, &mut ps, t0 + HOUR), (1, 1));
    assert_eq!(rt.state.reloader.banner, edits_banner());
}

#[test]
fn an_untracked_file_raises_the_banner_only_when_the_diff_lists_untracked_files() {
    for untracked in [true, false] {
        let d = dirty_repo();
        let mut rt = open(&d, ".", untracked);
        let listed = rt.state.keys.iter().any(|k| k.ends_with("new.cs"));
        assert!(!listed);
        let t0 = Instant::now();
        let mut ps = rt.state.edit_pollers(t0);
        std::fs::write(d.join("new.cs"), "class N {}\n").unwrap();
        let found = usize::from(untracked);
        assert_eq!(tick(&mut rt, &mut ps, t0 + HOUR), (1, found), "untracked {untracked}");
        assert_eq!(rt.state.reloader.banner, edits_banner().filter(|_| untracked));

        std::fs::remove_file(d.join("new.cs")).unwrap();
        assert_eq!(tick(&mut rt, &mut ps, t0 + HOUR * 2), (1, found));
        assert_eq!(rt.state.reloader.banner, None, "taken away again, the diff is as loaded");
        assert!(!rt.state.shared.stale());
    }
}

#[test]
fn an_ignored_file_raises_nothing() {
    let d = scratch_repo();
    std::fs::write(d.join(".gitignore"), "*.log\nout/\n").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "ignore"]);
    std::fs::write(d.join("a.cs"), numbered(80, |i| format!("int b{i};"))).unwrap();
    let mut rt = open(&d, ".", true);
    let t0 = Instant::now();
    let mut ps = rt.state.edit_pollers(t0);
    std::fs::write(d.join("build.log"), "noise\n").unwrap();
    std::fs::create_dir_all(d.join("out")).unwrap();
    std::fs::write(d.join("out/a.cs"), "class Built {}\n").unwrap();
    assert_eq!(tick(&mut rt, &mut ps, t0 + HOUR), (1, 0));
    assert_eq!(rt.state.reloader.banner, None);
}

#[test]
fn a_staged_diff_notices_the_index_and_not_the_working_tree() {
    let d = dirty_repo();
    git(&d, &["add", "a.cs"]);
    let mut rt = open(&d, "staged", false);
    let t0 = Instant::now();
    let mut ps = rt.state.edit_pollers(t0);
    assert_eq!(ps.len(), 1);
    std::fs::write(d.join("ws.cs"), "class W {\n    int edited;\n}\n").unwrap();
    assert_eq!(tick(&mut rt, &mut ps, t0 + HOUR), (1, 0), "an unstaged edit is not in this diff");
    git(&d, &["add", "ws.cs"]);
    assert_eq!(tick(&mut rt, &mut ps, t0 + HOUR * 2), (1, 1));
    assert_eq!(rt.state.reloader.banner, edits_banner());
}

#[test]
fn a_commit_range_or_a_fixture_is_never_polled() {
    let d = scratch_repo();
    let rt = runtime(repo_app(&d, Arc::new(Shared::default()), false));
    assert_eq!(rt.state.phase, Phase::Ready);
    assert!(rt.state.edit_pollers(Instant::now()).is_empty());
    let rt = runtime(app(story(), false));
    assert!(rt.state.edit_pollers(Instant::now()).is_empty());
}

#[test]
fn without_the_focus_nothing_is_checked_and_its_return_checks_once() {
    let d = dirty_repo();
    let mut rt = open(&d, ".", false);
    let t0 = Instant::now();
    let mut ps = rt.state.edit_pollers(t0);
    rt.update(|s, _| s.window_focus(false));
    std::fs::write(d.join("ws.cs"), "class W {\n    int edited;\n}\n").unwrap();
    for step in 1..=48 {
        assert_eq!(tick(&mut rt, &mut ps, t0 + HOUR * step), (0, 0));
    }
    assert_eq!(rt.state.reloader.banner, None);

    let back = t0 + HOUR * 49;
    rt.update(|s, _| s.window_focus(true));
    assert_eq!(tick(&mut rt, &mut ps, back), (1, 1), "the moment the reviewer comes back");
    assert_eq!(rt.state.reloader.banner, edits_banner());
    assert_eq!(tick(&mut rt, &mut ps, back), (0, 0), "once");
    assert_eq!(tick(&mut rt, &mut ps, back + MIN_WAIT / 2), (0, 0));
}

#[test]
fn an_idle_working_tree_window_rebuilds_nothing() {
    let d = dirty_repo();
    let mut rt = open(&d, ".", true);
    settle(&mut rt);
    let t0 = Instant::now();
    let mut ps = rt.state.edit_pollers(t0);
    let commits = rt.stats.commits;
    let mut checks = 0;
    for step in 1..=120 {
        let (ran, found) = tick(&mut rt, &mut ps, t0 + MAX_WAIT * step);
        assert_eq!(found, 0);
        checks += ran;
    }
    assert!(checks > 0 && checks <= 120, "{checks} checks in an hour");
    assert_eq!(rt.stats.commits, commits, "an hour of checks that find nothing redraws nothing");
    assert_eq!(rt.state.reloader.banner, None);
}

#[test]
fn each_repo_is_checked_on_its_own_and_the_banner_names_the_edited_one() {
    let a = dirty_repo();
    let b = dirty_repo();
    let shop = b.with_file_name("Acme.Shop");
    std::fs::rename(&b, &shop).unwrap();
    let mut rt = open_repos(&[&a, &shop], ".", false);
    let t0 = Instant::now();
    let mut ps = rt.state.edit_pollers(t0);
    assert_eq!(ps.len(), 2);
    let i = ps.iter().position(|p| p.dir.ends_with("Acme.Shop")).unwrap();
    std::fs::write(shop.join("ws.cs"), "class W {\n    int edited;\n}\n").unwrap();
    let other = 1 - i;
    assert_eq!(
        tick(&mut rt, &mut ps[other..=other], t0 + HOUR),
        (1, 0),
        "the other repo's check reads only its own"
    );
    assert_eq!(rt.state.reloader.banner, None);
    assert_eq!(tick(&mut rt, &mut ps[i..=i], t0 + HOUR), (1, 1));
    let banner = rt.state.reloader.banner.clone().unwrap();
    assert_eq!(banner.message(), "Files in Acme.Shop changed since this diff loaded");
}
