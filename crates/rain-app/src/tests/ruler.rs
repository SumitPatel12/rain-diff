use std::path::PathBuf;

use rain_core::{Anchor, Side};
use rain_diffview::LineRef;
use rune::{ElementInfo, Key, Mods, Runtime};

use super::*;
use crate::app::RulerState;

const TRACK: f32 = 400.0;

/// One file, two hunks a hundred lines apart; "needle" on three new lines.
fn two_hunks() -> PathBuf {
    let d = temp_dir();
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join("two-hunks.patch");
    std::fs::write(
        &p,
        "diff --git a/src/a.cs b/src/a.cs\nindex 1111111..2222222 100644\n--- a/src/a.cs\n+++ b/src/a.cs\n\
         @@ -1,4 +1,4 @@\n line1\n-old2\n+needle2\n line3\n line4\n\
         @@ -100,4 +100,5 @@\n line100\n-old101\n+needle101\n+needle102\n line102\n line103\n",
    )
    .unwrap();
    p
}

fn el(rt: &Runtime<App>, key: &str) -> ElementInfo {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no {key}"))
}

/// The marks named `name` (`Mark::name`).
macro_rules! named {
    ($marks:expr, $name:expr) => {
        $marks.iter().filter(|m| m.name() == $name).collect::<Vec<_>>()
    };
}

/// Where the active diff's row `row` lands on a `TRACK`-long track.
fn bucket(s: &App, row: u32) -> f32 {
    let d = s.active();
    (d.row_y(row as usize) / d.rows_height() * TRACK).floor()
}

#[test]
fn the_ruler_is_the_diffs_scrollbar() {
    let rt = runtime(app(workspace("fixtures/m.patch"), false));
    let r = el(&rt, "ruler");
    assert_eq!(r.role, "scrollbar");
    assert_eq!(r.label.as_deref(), Some("Diff position"));
    assert_eq!(r.rect[2], RulerState::WIDTH);
    let area = el(&rt, "diff-area");
    assert!(
        (r.rect[3] - area.rect[3]).abs() < 1.0,
        "full height: {:?} vs {:?}",
        r.rect,
        area.rect
    );
    assert!(
        (r.rect[0] + r.rect[2] - (area.rect[0] + area.rect[2])).abs() < 1.0,
        "at the right edge"
    );
    assert!(
        (rt.state.ruler.track.get() - r.rect[3]).abs() < 0.5,
        "the track height follows the layout"
    );
    let [_, max, viewport, _] = rt.state.ruler.live.get();
    assert!(
        max > 1000.0 && viewport > 100.0,
        "the viewport reports where it is: {max} {viewport}"
    );
    let t = rt.state.ruler.thumb.get();
    assert!(t[1] == 0.0 && t[3] > 0.0 && t[3] < r.rect[3], "a thumb at the top: {t:?}");
}

#[test]
fn hunks_files_and_threads_land_on_their_rows() {
    let mut rt = runtime(app(two_hunks(), false));
    {
        let mut s = rt.state.shared.lock();
        rain_comments::ops::add_thread(
            &mut s,
            "",
            "src/a.cs",
            Anchor::Line {
                side: Side::New,
                line: 101,
            },
            "Why two?",
            Some("claude"),
            None,
        );
    }
    settle(&mut rt);
    let s = &rt.state;
    assert_eq!(s.diff.annotations().len(), 1, "the thread is placed");
    let marks = s.ruler_marks(TRACK);
    let hunks = named!(marks, "hunk");
    assert_eq!(hunks.len(), 2, "{marks:?}");
    let d = s.active();
    assert_eq!(hunks[0].y, bucket(s, d.hunk_row(0, 0).unwrap()));
    assert_eq!(hunks[1].y, bucket(s, d.hunk_row(0, 1).unwrap()));
    assert!(hunks[0].y < hunks[1].y && hunks[0].h >= 2.0);
    assert_eq!(hunks[0].added(), Some(0.5));
    assert!((hunks[1].added().unwrap() - 2.0 / 3.0).abs() < 1e-6);
    let files = named!(marks, "file");
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].y, 0.0);
    let threads = named!(marks, "waiting");
    assert_eq!(threads.len(), 1, "claude's thread waits on the user: {marks:?}");
    let row = d.row_of_line(0, LineRef::new(Side::New, 101)).unwrap();
    assert_eq!(threads[0].y, bucket(s, row));
    assert!(named!(marks, "comment").is_empty() && named!(marks, "note").is_empty());
}

#[test]
fn find_hits_add_ticks() {
    let mut rt = runtime(app(two_hunks(), false));
    assert!(named!(rt.state.ruler_marks(TRACK), "find").is_empty());
    typed(&mut rt, "/");
    typed(&mut rt, "needle");
    rt.state.run_find();
    assert_eq!(rt.state.find.hits.len(), 3);
    let marks = rt.state.ruler_marks(TRACK);
    let ticks = named!(marks, "find");
    assert_eq!(ticks.len(), 3, "{marks:?}");
    assert!(ticks.windows(2).all(|w| w[0].y < w[1].y));
}

#[test]
fn overview_notes_show_as_note_ticks() {
    let rt = runtime(app(story(), true));
    let notes = rt.state.diff.annotations().iter().filter(|a| a.id.starts_with("note:")).count();
    assert!(notes > 0, "the sample overview has notes");
    assert!(!named!(rt.state.ruler_marks(TRACK), "note").is_empty());
}

#[test]
fn a_filter_shows_its_own_files_on_the_ruler() {
    let mut rt = runtime(app(story(), true));
    let all = named!(rt.state.ruler_marks(TRACK), "file").len();
    assert_eq!(all, rt.state.keys.len());
    rt.update(|s, _| {
        s.apply_filter(&rain_tree::TreeFilter {
            repos: [0].into(),
            ..Default::default()
        })
    });
    settle(&mut rt);
    let n = rt.state.filter.subset.as_ref().unwrap().files.len();
    assert!(n < all);
    assert_eq!(named!(rt.state.ruler_marks(TRACK), "file").len(), n);
}

fn press_at(rt: &mut Runtime<App>, frac: f32) {
    let r = el(rt, "ruler").rect;
    rt.pointer_down(r[0] + r[2] / 2.0, r[1] + r[3] * frac);
    settle(rt);
    rt.pointer_up(r[0] + r[2] / 2.0, r[1] + r[3] * frac);
    settle(rt);
}

#[test]
fn a_press_on_the_track_jumps_there() {
    let mut rt = runtime(app(workspace("fixtures/m.patch"), false));
    rt.update(|s, tx| {
        s.active_mut().cursor_down(tx);
    });
    settle(&mut rt);
    assert!(rt.state.active().cursor().is_some());
    press_at(&mut rt, 0.9);
    let [offset, max, ..] = rt.state.ruler.live.get();
    assert!(offset >= 0.85 * max, "{offset} of {max}");
    assert!(rt.state.active().cursor().is_none(), "the jump leaves the cursor behind");
    let g = rt.state.ruler_file_at(0.9).unwrap();
    assert_eq!(rt.state.status, rt.state.keys[g], "the status names the file pointed at");
    let t = rt.state.ruler.thumb.get();
    let track = rt.state.ruler.track.get();
    assert!(t[1] + t[3] > 0.8 * track, "the thumb followed: {t:?} on {track}");
    press_at(&mut rt, 0.0);
    assert_eq!(rt.state.ruler.live.get()[0], 0.0);
}

#[test]
fn dragging_scrolls_continuously() {
    let mut rt = runtime(app(workspace("fixtures/m.patch"), false));
    let r = el(&rt, "ruler").rect;
    let x = r[0] + r[2] / 2.0;
    rt.pointer_down(x, r[1] + r[3] * 0.1);
    settle(&mut rt);
    let mut last = rt.state.ruler.live.get()[0];
    for frac in [0.2, 0.35, 0.5, 0.65, 0.8] {
        rt.pointer_move(x, r[1] + r[3] * frac);
        rt.frame(1.0 / 60.0);
        rt.frame(1.0 / 60.0);
        let now = rt.state.ruler.live.get()[0];
        assert!(now > last, "{frac}: {now} after {last}");
        last = now;
    }
    rt.pointer_up(x, r[1] + r[3] * 0.8);
    settle(&mut rt);
    assert_eq!(rt.state.ruler.pointing, None);
}

#[test]
fn marks_rebuild_only_when_what_they_show_changes() {
    let mut rt = runtime(app(two_hunks(), false));
    let b0 = rt.state.ruler.builds.get();
    assert!(b0 >= 1);
    for _ in 0..5 {
        press(&mut rt, Key::Text("j".into()), Mods::default());
    }
    assert_eq!(rt.state.ruler.builds.get(), b0, "the cursor moving keeps the marks");
    {
        let mut s = rt.state.shared.lock();
        rain_comments::ops::add_thread(&mut s, "", "src/a.cs", Anchor::Line { side: Side::New, line: 2 }, "x", None, None);
    }
    settle(&mut rt);
    assert_eq!(rt.state.ruler.builds.get(), b0 + 1, "a thread rebuilds them once");
    typed(&mut rt, "/");
    typed(&mut rt, "needle");
    rt.state.run_find();
    settle(&mut rt);
    assert!(rt.state.ruler.builds.get() > b0 + 1, "find hits rebuild them");
    let b1 = rt.state.ruler.builds.get();
    settle(&mut rt);
    assert_eq!(rt.state.ruler.builds.get(), b1);
}

#[test]
fn wheel_scrolling_runs_no_view_for_the_ruler() {
    let mut rt = runtime(app(workspace("fixtures/m.patch"), false));
    let area = el(&rt, "diff-col").rect;
    let (x, y) = (area[0] + area[2] / 2.0, area[1] + area[3] / 2.0);
    let builds = rt.state.ruler.builds.get();
    let thumb0 = rt.state.ruler.thumb.get();
    let commits = rune::commit_generation();
    let (mut windows, mut seen) = (0, rt.state.active().visible_rows());
    for _ in 0..100 {
        rt.wheel(x, y, -20.0);
        rt.frame(1.0 / 60.0);
        let now = rt.state.active().visible_rows();
        if now != seen {
            windows += 1;
            seen = now;
        }
    }
    let [offset, ..] = rt.state.ruler.live.get();
    assert!(offset >= 1900.0, "scrolled {offset}");
    assert!(rt.state.ruler.thumb.get()[1] > thumb0[1], "the thumb moved with no view run");
    assert_eq!(rt.state.ruler.builds.get(), builds);
    let commits = rune::commit_generation() - commits;
    assert!(
        commits <= windows,
        "{commits} commits in 100 wheel frames: only the virtual list's {windows} window moves may commit"
    );
}

#[test]
fn a_mark_build_is_cheap() {
    for (name, a) in [
        ("m.patch", app(workspace("fixtures/m.patch"), false)),
        ("story.patch", app(story(), true)),
    ] {
        let t = std::time::Instant::now();
        for i in 0..20 {
            a.ruler_marks(TRACK + i as f32);
        }
        let ms = t.elapsed().as_secs_f64() * 1e3 / 20.0;
        println!("ruler marks on {name}: {} rows, {ms:.3} ms per build", a.active().row_count());
        assert!(ms < 50.0, "{name}: {ms} ms");
    }
}
