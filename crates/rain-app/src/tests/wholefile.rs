use std::path::{Path, PathBuf};
use std::sync::Arc;

use rain_core::{Shared, Side};
use rain_diffview::LineRef;
use rune::{Key, Mods, Runtime};

use super::*;

/// a.cs (one change at line 40 of 80), b.cs (two changes in 120 lines), new.cs added, gone.cs deleted.
pub(super) fn whole_repo() -> PathBuf {
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
    std::fs::write(d.join("b.cs"), numbered(120, |i| format!("int b{i};"))).unwrap();
    std::fs::write(d.join("gone.cs"), numbered(5, |i| format!("int g{i};"))).unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "one"]);
    std::fs::write(
        d.join("a.cs"),
        numbered(80, |i| if i == 40 { "int forty;".into() } else { format!("int a{i};") }),
    )
    .unwrap();
    std::fs::write(
        d.join("b.cs"),
        numbered(120, |i| match i {
            10 => "int ten;".into(),
            100 => "int hundred;".into(),
            _ => format!("int b{i};"),
        }),
    )
    .unwrap();
    std::fs::write(d.join("new.cs"), numbered(6, |i| format!("int n{i};"))).unwrap();
    std::fs::remove_file(d.join("gone.cs")).unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-q", "-m", "two"]);
    d
}

fn unified(dir: &Path) -> Runtime<App> {
    let mut a = repo_app(dir, Arc::new(Shared::default()), false);
    a.settings.layout = ViewLayout::Unified;
    a.apply_split();
    runtime(a)
}

fn file(rt: &Runtime<App>, name: &str) -> usize {
    rt.state.keys.iter().position(|k| k.ends_with(name)).unwrap()
}

/// Rows file `f` takes in the active diff, header included.
fn rows_of(rt: &Runtime<App>, f: usize) -> usize {
    let d = rt.state.active();
    let starts = d.file_rows();
    let end = starts.get(f + 1).map_or(d.row_count(), |&r| r as usize);
    end - starts[f] as usize
}

/// The help of `whole-{g}` in file `g`'s `⋯` menu, which it opens.
fn help_of(rt: &mut Runtime<App>, key: &str) -> Option<String> {
    let g = key.strip_prefix("whole-").and_then(|g| g.parse().ok());
    rt.update(|s, _| s.menus.file = g);
    settle(rt);
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .and_then(|e| e.help)
}

fn on_file(rt: &mut Runtime<App>, f: usize, line: u32) {
    rt.update(|s, tx| s.active_mut().scroll_to_line(f, LineRef::new(Side::New, line), tx));
    settle(rt);
    assert_eq!(rt.state.current(), Some(rt.state.global(f)));
}

#[test]
fn e_shows_the_current_file_whole_and_e_again_folds_it_and_drops_the_texts() {
    let dir = whole_repo();
    let mut rt = unified(&dir);
    let a = file(&rt, "a.cs");
    on_file(&mut rt, a, 40);
    let folded = rows_of(&rt, a);
    assert!(!rt.state.diff.expandable(a));
    assert_eq!(
        help_of(&mut rt, &format!("whole-{a}")).as_deref(),
        Some("Show the whole file with changes inline  e")
    );

    typed(&mut rt, "e");
    assert!(rt.state.diff.whole(a));
    assert!(rt.state.diff.expandable(a), "both blobs were read");
    assert_eq!(rows_of(&rt, a), 1 + 80 + 1, "header, every new line and the removed one");
    assert!(rows_of(&rt, a) > folded);
    assert_eq!(rt.state.current(), Some(a), "the cursor's file stays the current one");
    assert_eq!(help_of(&mut rt, &format!("whole-{a}")).as_deref(), Some("Show only the changes  e"));

    typed(&mut rt, "e");
    assert!(!rt.state.diff.whole(a));
    assert!(!rt.state.diff.expandable(a), "texts dropped to free memory");
    assert_eq!(rows_of(&rt, a), folded);
    assert!(rt.state.status.contains("changes only"), "{}", rt.state.status);
}

#[test]
fn the_header_button_toggles_the_file_whole() {
    let dir = whole_repo();
    let mut rt = unified(&dir);
    let b = file(&rt, "b.cs");
    rt.update(|s, tx| s.active_mut().scroll_to_file(b, tx));
    settle(&mut rt);
    let key = format!("whole-{b}");
    let (x, y) = rt.find(&key).expect("the file's header row shows Whole file");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(rt.state.diff.whole(b));
    assert_eq!(rows_of(&rt, b), 1 + 120 + 2, "two changes, both removed lines kept");
    for gap in 0..=rt.state.diff.files()[b].hunks.len() {
        assert_eq!(rt.state.diff.hidden(b, gap), Some(0), "gap {gap} open, the trailing one included");
    }
}

#[test]
fn a_whole_file_stays_whole_while_reading_on() {
    let dir = whole_repo();
    let mut rt = unified(&dir);
    let a = file(&rt, "a.cs");
    on_file(&mut rt, a, 40);
    typed(&mut rt, "e");
    typed(&mut rt, "]");
    typed(&mut rt, "n");
    press(&mut rt, Key::Text(" ".into()), Mods::default());
    assert!(rt.state.diff.whole(a));
    assert_eq!(rows_of(&rt, a), 82);
}

#[test]
fn shift_e_opens_every_file_and_counts_them() {
    let dir = whole_repo();
    let mut rt = unified(&dir);
    typed(&mut rt, "E");
    let (a, b) = (file(&rt, "a.cs"), file(&rt, "b.cs"));
    for f in [a, b] {
        assert!(rt.state.diff.whole(f));
        assert!(rt.state.diff.expandable(f));
    }
    assert_eq!(rows_of(&rt, a), 82);
    assert_eq!(rows_of(&rt, b), 123);
    assert_eq!(rt.state.status, "Whole file: 2 files open");
    let (n, g) = (file(&rt, "new.cs"), file(&rt, "gone.cs"));
    assert!(
        !rt.state.diff.whole(n) && !rt.state.diff.whole(g),
        "added and deleted files are whole already"
    );
    typed(&mut rt, "E");
    assert_eq!(rt.state.status, "No file left to open whole");
}

/// Frames until `done` holds, for up to a minute.
fn wait(rt: &mut Runtime<App>, done: &dyn Fn(&App) -> bool) {
    let t = std::time::Instant::now();
    while !done(&rt.state) && t.elapsed().as_secs() < 60 {
        rt.frame(1.0 / 60.0);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// `dir` HEAD~1..HEAD loaded on the loader thread, as the window does, in unified mode.
fn threaded(dir: &Path) -> Runtime<App> {
    let mut a = App::new(
        rain_core::Launch {
            session: "t".into(),
            repos: vec![rain_core::RepoLaunch {
                dir: dir.to_path_buf(),
                base: "HEAD~1".into(),
                target: "HEAD".into(),
                ..rain_core::RepoLaunch::default()
            }],
            ..rain_core::Launch::default()
        },
        Arc::new(Shared::default()),
        None,
        None,
    );
    a.start_loading(Arc::new(|| {}));
    let mut rt = runtime(a);
    wait(&mut rt, &|s| s.phase == Phase::Ready);
    rt.update(|s, _| {
        s.settings.layout = ViewLayout::Unified;
        s.apply_split();
    });
    assert_eq!(rt.state.phase, Phase::Ready);
    rt
}

#[test]
fn shift_e_reads_off_the_ui_thread_and_reports_progress() {
    let dir = whole_repo();
    let mut rt = threaded(&dir);
    typed(&mut rt, "E");
    wait(&mut rt, &|s| s.status == "Whole file: 2 files open");
    assert_eq!(rt.state.status, "Whole file: 2 files open");
    let a = file(&rt, "a.cs");
    assert!(rt.state.diff.whole(a) && rt.state.diff.expandable(a));
    assert_eq!(rows_of(&rt, a), 82);
}

#[test]
fn added_and_deleted_files_are_already_whole() {
    let dir = whole_repo();
    let mut rt = unified(&dir);
    for (name, side) in [("new.cs", Side::New), ("gone.cs", Side::Old)] {
        let f = file(&rt, name);
        rt.update(|s, tx| s.active_mut().scroll_to_line(f, LineRef::new(side, 1), tx));
        settle(&mut rt);
        assert_eq!(rt.state.current(), Some(f));
        let before = rows_of(&rt, f);
        typed(&mut rt, "e");
        assert_eq!(rt.state.status, "Already the whole file");
        assert!(!rt.state.diff.whole(f));
        assert_eq!(rows_of(&rt, f), before);
        assert_eq!(help_of(&mut rt, &format!("whole-{f}")).as_deref(), Some("Already the whole file"));
    }
}

#[test]
fn a_fixture_has_no_whole_files() {
    let mut rt = runtime(app(workspace("fixtures/m.patch"), false));
    typed(&mut rt, "e");
    assert_eq!(rt.state.status, "Whole file needs a git repo (fixture)");
    assert!(!rt.state.diff.whole(0));
    typed(&mut rt, "E");
    assert_eq!(rt.state.status, "Whole file needs a git repo (fixture)");
    assert!((0..rt.state.keys.len()).all(|f| !rt.state.diff.whole(f)));
    rt.update(|s, _| s.menus.file = Some(0));
    settle(&mut rt);
    assert!(rt.find("file-menu").is_some());
    assert!(rt.find("whole-0").is_none(), "no toggle without a repo");
}

#[test]
fn a_filtered_view_and_the_full_diff_open_the_file_together() {
    let dir = whole_repo();
    let mut rt = unified(&dir);
    let (a, b) = (file(&rt, "a.cs"), file(&rt, "b.cs"));
    rt.update(|s, _| {
        let sub = crate::app::Subset::new(s, vec![b, a], None);
        s.filter.subset = Some(sub);
    });
    settle(&mut rt);
    on_file(&mut rt, 1, 40);
    assert_eq!(rt.state.current(), Some(a));
    typed(&mut rt, "e");
    assert!(rt.state.active().whole(1) && rt.state.active().expandable(1));
    assert!(rt.state.diff.whole(a) && rt.state.diff.expandable(a), "the full diff follows");
    assert_eq!(rows_of(&rt, 1), 82);
    assert!(
        std::sync::Arc::ptr_eq(&rt.state.active().files()[1], &rt.state.diff.files()[a]),
        "the subset shares the full diff's copy instead of cloning the file"
    );
    typed(&mut rt, "e");
    assert!(!rt.state.active().whole(1) && !rt.state.diff.whole(a));
    assert!(!rt.state.active().expandable(1) && !rt.state.diff.expandable(a));
}

#[test]
fn a_whole_file_and_opened_ranges_survive_a_new_filter_subset() {
    let dir = whole_repo();
    let mut rt = unified(&dir);
    let (a, b) = (file(&rt, "a.cs"), file(&rt, "b.cs"));
    rt.update(|s, _| s.filter.subset = Some(crate::app::Subset::new(s, vec![a, b], None)));
    settle(&mut rt);
    on_file(&mut rt, 0, 40);
    typed(&mut rt, "e");
    assert_eq!(rows_of(&rt, 0), 82);
    rt.update(|s, _| {
        s.active_mut().request_expand(1, 0, rain_diffview::Expand::All);
        s.fetch_texts(1);
    });
    settle(&mut rt);
    let opened_b = rows_of(&rt, 1);
    rt.update(|s, _| {
        let prev = s.filter.subset.take();
        s.filter.subset = Some(crate::app::Subset::new(s, vec![b, a], prev.as_ref()));
    });
    settle(&mut rt);
    assert!(
        rt.state.active().whole(1) && rt.state.active().expandable(1),
        "still whole, texts kept"
    );
    assert_eq!(rows_of(&rt, 1), 82);
    assert_eq!(rows_of(&rt, 0), opened_b, "b's opened range carried over");
}

#[test]
fn a_failed_read_ends_the_whole_file_ask() {
    let dir = whole_repo();
    let mut rt = unified(&dir);
    let a = file(&rt, "a.cs");
    rt.update(|s, _| {
        s.diff.set_whole(a, true);
        let key = s.keys[a].clone();
        s.install_texts(crate::load::Msg::Texts { key, old: None, new: None });
    });
    assert!(!rt.state.diff.whole(a) && !rt.state.diff.needs_texts(a));
    assert!(rt.state.status.starts_with("Could not read"), "{}", rt.state.status);
}

#[test]
fn texts_landing_after_the_file_was_folded_back_are_dropped() {
    let dir = whole_repo();
    let mut rt = unified(&dir);
    let a = file(&rt, "a.cs");
    rt.update(|s, _| {
        let key = s.keys[a].clone();
        s.install_texts(crate::load::Msg::Texts {
            key,
            old: Some(Arc::from("x\n")),
            new: Some(Arc::from("y\n")),
        });
    });
    assert!(!rt.state.diff.expandable(a));
}

/// `RAIN_WHOLE_REPO=<fixtures/gen/s/repo> cargo test --release -p rain-app whole_file_timings -- --ignored --nocapture`
#[test]
#[ignore = "perf: needs a generated tier repo in RAIN_WHOLE_REPO"]
fn whole_file_timings() {
    let Some(dir) = std::env::var_os("RAIN_WHOLE_REPO").map(PathBuf::from) else {
        println!("RAIN_WHOLE_REPO not set: skipped");
        return;
    };
    let mut a = repo_app(&dir, Arc::new(Shared::default()), false);
    a.settings.layout = ViewLayout::Unified;
    a.apply_split();
    let mut rt = runtime(a);
    let f = (0..rt.state.keys.len())
        .filter(|&g| rt.state.whole_blocker(g).is_none())
        .max_by_key(|&g| rt.state.diff.files()[g].lines.len())
        .expect("a modified file");
    let first = rt.state.diff.files()[f].lines.iter().find(|l| l.new != 0).unwrap().new;
    on_file(&mut rt, f, first);
    let rows = rt.state.diff.row_count();
    let t = std::time::Instant::now();
    rt.update(|s, tx| s.toggle_whole_file(tx));
    let toggle_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = std::time::Instant::now();
    rt.frame(1.0 / 60.0);
    let frame_ms = t.elapsed().as_secs_f64() * 1000.0;
    settle(&mut rt);
    let bytes: usize = {
        let d = &rt.state.diff.files()[f];
        d.old_text.as_ref().map_or(0, |s| s.len()) + d.new_text.as_ref().map_or(0, |s| s.len())
    };
    println!(
        "whole {} ({} diff lines): blob read + set_whole {toggle_ms:.1} ms, next frame {frame_ms:.1} ms, rows {rows} -> {}, texts {bytes} B",
        rt.state.keys[f],
        rt.state.diff.files()[f].lines.len(),
        rt.state.diff.row_count()
    );
    typed(&mut rt, "e");
    let t = std::time::Instant::now();
    rt.update(|s, tx| s.expand_all_context(tx));
    println!(
        "expand all ({} files, sequential reads on the UI thread): {:.0} ms, rows {}, status {:?}",
        rt.state.keys.len(),
        t.elapsed().as_secs_f64() * 1000.0,
        rt.state.diff.row_count(),
        rt.state.status
    );
    let mut rt = threaded(&dir);
    let t = std::time::Instant::now();
    rt.update(|s, tx| s.expand_all_context(tx));
    let ui_ms = t.elapsed().as_secs_f64() * 1000.0;
    wait(&mut rt, &|s| s.status.starts_with("Whole file:"));
    println!(
        "expand all off the UI thread (parallel readers): UI {ui_ms:.1} ms, all texts in {:.0} ms, status {:?}",
        t.elapsed().as_secs_f64() * 1000.0,
        rt.state.status
    );
}

#[test]
fn split_shows_the_whole_file_too() {
    let dir = whole_repo();
    let mut rt = sized_runtime(repo_app(&dir, Arc::new(Shared::default()), false), (1920.0, 1080.0));
    assert_eq!(rt.state.effective_mode(), rain_diffview::DiffMode::Split);
    let a = file(&rt, "a.cs");
    on_file(&mut rt, a, 40);
    typed(&mut rt, "e");
    assert!(rt.state.diff.whole(a));
    assert_eq!(rows_of(&rt, a), 1 + 80, "the changed line pairs into one row");
}
