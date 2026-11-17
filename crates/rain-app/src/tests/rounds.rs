//! Reload in place, hunk-level viewed, the HEAD watch, Finish review, rounds and the since-round
//! lens, and the persisted reading place.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rain_core::{EventKind, Side, Verdict};
use rain_diffview::LineRef;

use super::*;

fn shift() -> Mods {
    Mods {
        shift: true,
        ..Mods::default()
    }
}

fn rev(dir: &Path, r: &str) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", r])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn commit_a(dir: &Path, body: String, msg: &str) {
    std::fs::write(dir.join("a.cs"), body).unwrap();
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-q", "-m", msg]);
}

/// `a.cs` of 80 lines; the second commit changes lines 10 and 60. Returns the repo and the
/// first commit (the base every test diffs from).
fn two_change_repo() -> (PathBuf, String) {
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
    commit_a(&d, numbered(80, |i| format!("int a{i};")), "one");
    let base = rev(&d, "HEAD");
    commit_a(
        &d,
        numbered(80, |i| match i {
            10 => "int ten;".into(),
            60 => "int sixty;".into(),
            _ => format!("int a{i};"),
        }),
        "two",
    );
    (d, base)
}

fn a_cs(edit: impl Fn(usize) -> Option<String>) -> String {
    numbered(80, |i| {
        edit(i).unwrap_or_else(|| match i {
            10 => "int ten;".into(),
            60 => "int sixty;".into(),
            _ => format!("int a{i};"),
        })
    })
}

fn repo_runtime(dir: &Path, base: &str) -> Runtime<App> {
    sized_runtime(repo_app_from(dir, base, Arc::new(Shared::default()), false), (1920.0, 1080.0))
}

fn reload(rt: &mut Runtime<App>) {
    rt.update(|s, _| s.reload());
    settle(rt);
}

#[test]
fn reload_keeps_the_cursor_on_the_same_text_after_lines_are_inserted_above() {
    let (d, base) = two_change_repo();
    let mut rt = repo_runtime(&d, &base);
    rt.update(|s, tx| s.goto_line(0, LineRef::new(Side::New, 60), tx));
    settle(&mut rt);
    let before = rt.state.active().cursor();
    assert_eq!(before.map(|(_, at)| at.line), Some(60));
    let top_before = rt.state.active().scroll_offset();
    let mut body = "// one\n// two\n// three\n".to_owned();
    body.push_str(&a_cs(|_| None));
    std::fs::write(d.join("a.cs"), body).unwrap();
    git(&d, &["commit", "-qam", "three lines on top"]);
    reload(&mut rt);
    let (l, at) = rt.state.active().cursor().expect("a cursor after the reload");
    assert_eq!((at.side, at.line), (Side::New, 63), "the same text, three lines down");
    assert_eq!(rt.state.active().side_line(l, Side::New, 63), Some("int sixty;"));
    assert_eq!(rt.state.phase, Phase::Ready);
    assert!(rt.state.status.starts_with("Reloaded"), "{}", rt.state.status);
    let drift = (rt.state.active().scroll_offset() - top_before).abs();
    assert!(drift < 200.0, "the reader stays about where it was ({drift} pt)");
}

#[test]
fn a_line_note_snapshotted_at_post_time_follows_its_code_after_a_reload() {
    use rain_core::{Anchor, Note, NoteKind};
    let (d, base) = two_change_repo();
    let mut rt = repo_runtime(&d, &base);
    let shared = rt.state.shared.clone();
    let at = |line| Anchor::Line { side: Side::New, line };
    let snap = shared.excerpt("", "a.cs", &at(60));
    assert_eq!(
        snap.map(|s| s.content).as_deref(),
        Some("int sixty;"),
        "the window installs its excerpt source"
    );
    {
        let excerpt = |r: &str, p: &str, a: &Anchor| shared.excerpt(r, p, a);
        let mut s = shared.lock();
        let note = Note {
            kind: NoteKind::Line,
            file_path: "a.cs".into(),
            anchor: Some(at(60)),
            body: "n".into(),
            ..Note::default()
        };
        rain_comments::add_notes(&mut s, vec![note], "", false);
        assert_eq!(rain_comments::snapshot_notes(&mut s, &excerpt), 1);
    }
    let mut body = "// one\n// two\n// three\n".to_owned();
    body.push_str(&a_cs(|_| None));
    std::fs::write(d.join("a.cs"), body).unwrap();
    git(&d, &["commit", "-qam", "three lines on top"]);
    reload(&mut rt);
    assert_eq!(shared.lock().notes[0].anchor, Some(at(63)), "the note moved with its line");
    assert!(
        shared.triage().is_some_and(|t| t["diffLoaded"] == true),
        "the loader publishes triage"
    );
}

#[test]
fn viewed_records_hunks_and_a_reload_reads_only_the_changed_hunk_unviewed() {
    let (d, base) = two_change_repo();
    let mut rt = repo_runtime(&d, &base);
    rt.update(|s, _| s.set_viewed(0, true));
    let rec = rt.state.mirror.viewed[0].clone();
    assert_eq!(rec.hunks.len(), 2, "both hunks recorded with the viewed mark");
    commit_a(
        &d,
        a_cs(|i| (i == 61).then(|| "int sixty_one;".into())),
        "change near the second hunk",
    );
    reload(&mut rt);
    assert!(!rt.state.is_viewed(0), "a changed file reads unviewed, as on GitHub");
    assert_eq!(rt.state.reloader.changed.get(&0), Some(&vec![1]), "only the second hunk changed");
    let first = rt.state.diff.hunk_hash(0, 0).unwrap();
    assert!(
        rec.hunks.iter().any(|h| h.hash == first),
        "the first hunk is the one that was viewed"
    );
    assert!(rt.find("changed-0").is_some(), "the header says so");
    assert!(rt.state.status.contains("1 file changed since you viewed"), "{}", rt.state.status);
    let (x, y) = rt.find("changed-0").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    let hunk2 = rt.state.diff.files()[0].hunks[1].new_start;
    assert_eq!(rt.state.active().cursor().map(|(_, at)| at.line), Some(hunk2));
    rt.update(|s, _| s.set_viewed(0, true));
    assert!(rt.state.reloader.changed.is_empty());
}

#[test]
fn the_head_watch_puts_up_a_banner_within_two_seconds() {
    let (d, base) = two_change_repo();
    let mut rt = repo_runtime(&d, &base);
    rt.update(|s, _| s.start_head_watch(Arc::new(|| {})));
    let started = Instant::now();
    while rt.state.head_watch_count() == 0 && started.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(rt.state.head_watch_count(), 1);
    std::thread::sleep(Duration::from_millis(300));
    git(&d, &["commit", "-q", "--allow-empty", "-m", "new"]);
    let at = Instant::now();
    while rt.state.reloader.banner.is_none() && at.elapsed() < Duration::from_secs(6) {
        std::thread::sleep(Duration::from_millis(20));
        rt.update(|s, tx| {
            s.rounds_poll(tx);
        });
    }
    // The watch itself is held to 2 s in rain-git's tests; this runs beside the whole suite.
    assert!(at.elapsed() < Duration::from_secs(5), "the banner took {:?}", at.elapsed());
    let b = rt.state.reloader.banner.clone().unwrap();
    assert_eq!(b.message(), format!("1 new commit on main ({})", &rev(&d, "HEAD")[..7]));
    settle(&mut rt);
    assert!(rt.find("reload-banner").is_some());
    press(&mut rt, Key::Text("R".into()), shift());
    assert!(rt.state.reloader.banner.is_none(), "a reload takes the banner down");
}

#[test]
fn finish_needs_a_verdict_then_records_the_round_and_an_event() {
    let mut rt = sized_runtime(app(story(), true), (1920.0, 1080.0));
    rt.update(|s, _| {
        s.set_viewed(0, true);
        s.status.clear();
    });
    press(&mut rt, Key::Text("F".into()), shift());
    assert!(rt.state.layout.is_open(Overlay::Finish));
    assert_eq!(rt.state.review.verdict, None, "no verdict is preselected");
    assert!(
        rt.state.review.summary.text.starts_with("Viewed 1 of"),
        "{}",
        rt.state.review.summary.text
    );
    let warnings = rt.state.finish_warnings();
    assert!(
        warnings.iter().any(|w| w.go_to_must && w.text.contains("Must file")),
        "{warnings:?}"
    );
    assert!(!rt.state.finish_ready());
    let ok = rt.update_if(|s, tx| s.submit_review(tx));
    assert!(!ok);
    assert!(rt.state.mirror.rounds.is_empty());
    for dark in [true, false] {
        for size in [(1920.0, 1080.0), (1366.0, 768.0)] {
            let mut r = sized_runtime(app(story(), true), size);
            r.env.system_dark = dark;
            r.update(|s, tx| s.finish_review(tx));
            settle(&mut r);
            assert!(
                r.find("finish-submit").is_some() && r.find("finish-verdict").is_some(),
                "{dark} {size:?}"
            );
        }
    }
    rt.update(|s, _| s.set_verdict(Verdict::Approve));
    let ok = rt.update_if(|s, tx| s.submit_review(tx));
    assert!(ok, "{:?}", rt.state.review.refused);
    let s = rt.state.mirror.clone();
    assert_eq!(s.rounds.len(), 1);
    let r = &s.rounds[0];
    assert_eq!((r.n, r.verdict), (1, Verdict::Approve));
    assert!(r.summary.starts_with("Viewed 1 of"));
    assert_eq!(r.viewed.len(), 1);
    assert!(!r.viewed[0].hunks.is_empty(), "the round keeps the viewed hunks");
    assert_eq!(s.round(), 2);
    let ev = rt.state.shared.events_since(0, &[EventKind::Submitted]);
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].data["round"], 1);
    assert!(!rt.state.layout.is_open(Overlay::Finish));
    assert_eq!(rt.state.status, "Round 1 submitted: Approve");
}

#[test]
fn go_to_first_must_closes_the_dialog_and_jumps() {
    let mut rt = sized_runtime(app(story(), true), (1920.0, 1080.0));
    press(&mut rt, Key::Text("F".into()), shift());
    let first = rt.state.must_unviewed()[0];
    let (x, y) = rt.find("finish-go-must").expect("Go to first");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(!rt.state.layout.is_open(Overlay::Finish));
    assert_eq!(rt.state.current(), Some(first));
}

#[test]
fn finish_refuses_after_a_reload_changed_the_sha() {
    let (d, base) = two_change_repo();
    let mut rt = repo_runtime(&d, &base);
    rt.update(|s, tx| s.finish_review(tx));
    commit_a(&d, a_cs(|i| (i == 30).then(|| "int thirty;".into())), "more");
    reload(&mut rt);
    rt.update(|s, _| s.set_verdict(Verdict::Comment));
    let ok = rt.update_if(|s, tx| s.submit_review(tx));
    assert!(!ok);
    assert!(rt.state.review.refused.as_deref().unwrap_or_default().contains("reloaded"));
    assert!(rt.state.mirror.rounds.is_empty());
    rt.update(|s, tx| s.finish_review(tx));
    rt.update(|s, _| s.set_verdict(Verdict::Comment));
    assert!(rt.update_if(|s, tx| s.submit_review(tx)), "a fresh dialog on the new sha submits");
}

#[test]
fn since_round_shows_only_the_hunks_changed_after_the_round() {
    let (d, base) = two_change_repo();
    let mut rt = repo_runtime(&d, &base);
    rt.update(|s, tx| {
        s.finish_review(tx);
        s.set_verdict(Verdict::RequestChanges);
        assert!(s.submit_review(tx));
    });
    commit_a(&d, a_cs(|i| (i == 61).then(|| "int sixty_one;".into())), "address the second hunk");
    reload(&mut rt);
    assert_eq!(rt.state.diff.files()[0].hunks.len(), 2);
    settle(&mut rt);
    assert!(rt.find("round-picker").is_some());
    rt.update(|s, tx| s.set_lens(Some(1), tx));
    settle(&mut rt);
    let a = rt.state.active();
    assert_eq!(a.files().len(), 1);
    assert_eq!(a.files()[0].hunks.len(), 1, "only the hunk changed since round 1");
    let h = &a.files()[0].hunks[0];
    assert!(h.new_start <= 61 && 61 < h.new_start + h.new_count, "{h:?}");
    assert_eq!(rt.state.review.lens.as_ref().map(|l| (l.round, l.hunks)), Some((1, 1)));
    assert!(rt.find("round-reset").is_some());
    let bar = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("filter-bar-text"));
    assert_eq!(
        bar.and_then(|e| e.text),
        Some(format!("1 of {} files changed since round 1", rt.state.keys.len())),
        "the bar names the round, not an empty filter"
    );
    reload(&mut rt);
    assert_eq!(rt.state.active().files()[0].hunks.len(), 1, "the lens survives a reload");
    let (x, y) = rt.find("round-reset").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(rt.state.review.lens.is_none());
    assert_eq!(rt.state.active().files()[0].hunks.len(), 2, "Show all puts every hunk back");
}

#[test]
fn the_place_round_trips_through_the_session_file() {
    let (d, base) = two_change_repo();
    let dir = temp_dir();
    let open = || {
        let mut a = App::new(
            Launch {
                session: "place".into(),
                repos: vec![RepoLaunch {
                    dir: d.clone(),
                    base: base.clone(),
                    target: "HEAD".into(),
                    ..RepoLaunch::default()
                }],
                ..Launch::default()
            },
            Arc::new(Shared::default()),
            Some(dir.join("sessions")),
            None,
        );
        a.load_now();
        sized_runtime(a, (1920.0, 1080.0))
    };
    let mut rt = open();
    rt.update(|s, tx| s.goto_line(0, LineRef::new(Side::New, 60), tx));
    settle(&mut rt);
    let place = rt.state.shared.lock().place.clone().expect("saved as it moved");
    assert_eq!((place.file.as_str(), place.line, place.side), ("repo/a.cs", Some(60), Side::New));
    assert_eq!(rt.state.shared.ui_state().line, Some(60), "published for /api/state");
    if let Some(a) = &rt.state.autosave {
        a.flush();
    }
    drop(rt);
    let rt = open();
    assert_eq!(rt.state.active().cursor().map(|(_, at)| at.line), Some(60), "restored on open");
    assert_eq!(App::last_session(&dir.join("sessions")).map(|(id, _)| id).as_deref(), Some("place"));
}

#[test]
fn last_session_is_the_newest_session_file() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(App::last_session(&dir), None);
    let now = std::time::SystemTime::now();
    for (name, age) in [("old", 60), ("new", 1), ("older", 120)] {
        let p = dir.join(format!("{name}.json"));
        std::fs::write(&p, "{}").unwrap();
        let f = std::fs::File::options().write(true).open(&p).unwrap();
        f.set_modified(now - Duration::from_secs(age)).unwrap();
    }
    std::fs::write(dir.join("newest.json.corrupt"), "x").unwrap();
    assert_eq!(App::last_session(&dir).map(|(id, _)| id).as_deref(), Some("new"));
}

#[test]
fn the_round_scenes_draw_what_they_name() {
    for (name, keys) in [
        (
            "finish",
            &["overlay-finish-review", "finish-verdict", "finish-submit", "finish-go-must"][..],
        ),
        ("since-round", &["round-picker", "round-reset"][..]),
        ("reload-banner", &["reload-banner", "reload-toast"][..]),
    ] {
        for dark in [true, false] {
            let mut a = app(story(), true);
            let setup = crate::spec().scenes.iter().find(|(n, _)| *n == name).map(|(_, f)| *f).unwrap();
            setup(&mut a);
            let mut rt = Runtime::new(a, view, (1366.0, 768.0), Env::new(dark, 1.0));
            rt.poll = Some(|s: &mut App, _, tx| s.poll(tx));
            rt.frame(0.0);
            settle(&mut rt);
            for k in keys {
                assert!(rt.find(k).is_some(), "{name} ({dark}): {k}");
            }
        }
    }
    let mut a = app(story(), true);
    let setup = crate::spec()
        .scenes
        .iter()
        .find(|(n, _)| *n == "since-round")
        .map(|(_, f)| *f)
        .unwrap();
    setup(&mut a);
    let lens = a.review.lens.clone().expect("the lens is on");
    assert_eq!(a.active().files().len(), lens.files.len());
    assert!(
        lens.files.iter().all(|&g| g >= 3),
        "the files viewed in round 1 drop out: {:?}",
        lens.files
    );
}

#[test]
fn the_reload_message_counts_what_moved() {
    use crate::app::ReloadCounts;
    assert_eq!(ReloadCounts::default().message(), "Reloaded");
    let c = ReloadCounts {
        moved: 3,
        addressed: 1,
        orphaned: 2,
        changed_files: 0,
    };
    assert_eq!(c.message(), "Reloaded · 3 moved · 1 addressed? · 2 orphaned");
}
