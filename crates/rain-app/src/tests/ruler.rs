use rune::{ElementInfo, Runtime};

use super::*;
use crate::app::RulerState;

fn el(rt: &Runtime<App>, key: &str) -> ElementInfo {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no {key}"))
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
fn wheel_scrolling_runs_no_view_for_the_ruler() {
    let mut rt = runtime(app(workspace("fixtures/m.patch"), false));
    let area = el(&rt, "diff-col").rect;
    let (x, y) = (area[0] + area[2] / 2.0, area[1] + area[3] / 2.0);
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
    let commits = rune::commit_generation() - commits;
    assert!(
        commits <= windows,
        "{commits} commits in 100 wheel frames: only the virtual list's {windows} window moves may commit"
    );
}
