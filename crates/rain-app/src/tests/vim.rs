use std::time::Instant;

use rune::{Key, KeyEvent, Mods, Runtime};

use super::*;
use crate::settings::{Density, ViewLayout};

/// A patch adding `v.rs`: a signature, a long line, an empty line, then `n` short lines.
fn vim_patch(n: usize) -> PathBuf {
    let long: Vec<String> = (0..60).map(|i| format!("w{i:02}")).collect();
    let mut lines = vec![
        "fn alpha(beta, gamma) {".to_owned(),
        format!("    let long = [{}];", long.join(", ")),
        String::new(),
    ];
    lines.extend((4..=n).map(|i| format!("    line_{i} = value_{i};")));
    let d = temp_dir();
    std::fs::create_dir_all(&d).unwrap();
    let mut p = format!(
        "diff --git a/v.rs b/v.rs\nnew file mode 100644\n--- /dev/null\n+++ b/v.rs\n@@ -0,0 +1,{} @@\n",
        lines.len()
    );
    for l in &lines {
        p.push('+');
        p.push_str(l);
        p.push('\n');
    }
    let path = d.join("v.patch");
    std::fs::write(&path, p).unwrap();
    path
}

fn vim_rt(n: usize) -> Runtime<App> {
    let mut rt = runtime(app(vim_patch(n), false));
    rt.update(|s, _| s.set_layout(ViewLayout::Unified));
    settle(&mut rt);
    rt
}

fn keys(rt: &mut Runtime<App>, k: &str) {
    typed(rt, k);
}

fn shift() -> Mods {
    Mods {
        shift: true,
        ..Mods::default()
    }
}

fn ctrl_shift() -> Mods {
    Mods { shift: true, ..ctrl() }
}

fn at(rt: &Runtime<App>) -> (u32, usize) {
    let a = rt.state.active();
    (a.cursor().unwrap().1.line, a.cursor_byte().unwrap())
}

/// The cursor row's top, less the scroll offset: where it sits in the viewport, by estimate.
fn on_screen(rt: &Runtime<App>) -> f32 {
    let a = rt.state.active();
    let (f, l) = a.cursor().unwrap();
    a.row_y(a.row_of_line(f, l).unwrap() as usize) - a.scroll_offset()
}

#[test]
fn the_block_is_a_square_one_cell_wide_and_a_click_moves_it() {
    let mut rt = vim_rt(20);
    keys(&mut rt, "jw");
    settle(&mut rt);
    let blocks: Vec<_> = rt.elements().into_iter().filter(|e| e.key.as_deref() == Some("block")).collect();
    assert_eq!(blocks.len(), 1, "one block on the cursor's line");
    let b = blocks[0].rect;
    let text = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("text") && e.rect[1] == b[1])
        .unwrap()
        .rect;
    assert!(b[0] > text[0], "past `fn `: {b:?} {text:?}");
    assert!(b[2] > 4.0 && b[2] < 14.0 && b[3] >= 14.0, "a cell-sized square: {b:?}");
    let (line, _) = at(&rt);
    let (x, y) = (text[0] + 1.0, b[1] + b[3] / 2.0);
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert_eq!(at(&rt), (line, 0), "a click at the line's start puts the cursor there");
}

#[test]
fn vim_motions_reach_the_diff_ahead_of_the_keymap() {
    let mut rt = vim_rt(200);
    keys(&mut rt, "j");
    assert_eq!(at(&rt), (1, 0));
    keys(&mut rt, "w");
    assert_eq!(at(&rt), (1, 3));
    keys(&mut rt, "$j");
    assert_eq!(at(&rt), (2, 315), "$ then j keeps to the end");
    keys(&mut rt, "0");
    assert_eq!(at(&rt), (2, 0));
    keys(&mut rt, "^f,;");
    assert_eq!(at(&rt), (2, 24));
    keys(&mut rt, "G");
    assert_eq!(at(&rt), (200, 4));
    keys(&mut rt, "gg");
    assert_eq!(at(&rt), (1, 0));
    keys(&mut rt, "5G");
    assert_eq!(at(&rt), (5, 4));
    keys(&mut rt, "3j");
    assert_eq!(at(&rt), (8, 4));
    press(&mut rt, Key::Command('d'), ctrl());
    let down = at(&rt).0;
    assert!(down > 20, "Ctrl+D moves half a page: {down}");
    press(&mut rt, Key::Command('u'), ctrl());
    assert_eq!(at(&rt).0, 8, "Ctrl+U comes back as far");
}

#[test]
fn the_displaced_commands_answer_their_new_keys_only() {
    let mut a = app(vim_patch(40), false);
    crate::seed_threads(&mut a);
    let mut rt = runtime(a);
    rt.update(|s, _| s.set_layout(ViewLayout::Unified));
    settle(&mut rt);
    keys(&mut rt, "j");
    let g = rt.state.current().unwrap();

    keys(&mut rt, "w");
    assert!(rt.state.focused_thread.is_none(), "w is a word, not the next waiting thread");
    keys(&mut rt, "i");
    assert!(rt.state.focused_thread.is_some(), "i is the next waiting thread");
    press(&mut rt, Key::Escape, Mods::default());

    let viewed = rt.state.is_viewed(g);
    keys(&mut rt, "v");
    assert_eq!(rt.state.is_viewed(g), viewed, "v starts visual mode");
    assert!(rt.state.active().visual().is_some());
    press(&mut rt, Key::Escape, Mods::default());
    keys(&mut rt, "V");
    assert!(rt.state.active().visual().is_some_and(|v| v.line), "V is visual lines, not accept");
    press(&mut rt, Key::Escape, Mods::default());
    press(&mut rt, Key::Text(" ".into()), shift());
    assert_eq!(rt.state.is_viewed(g), !viewed, "Shift+Space toggles viewed");
    press(&mut rt, Key::Text(" ".into()), shift());
    assert_eq!(rt.state.is_viewed(g), viewed);
    keys(&mut rt, "j");

    keys(&mut rt, "l");
    assert!(rt.state.sidebar.is_open(), "l moves right, the list stays");
    keys(&mut rt, "f");
    keys(&mut rt, "x");
    assert!(!rt.state.tree.filter_edit.focused, "f finds in the line");
    keys(&mut rt, "b");
    assert_eq!(rt.state.reader_tab(), crate::app::ReaderTab::Changes, "b is a word back");
    let whole = rt.state.is_whole(g);
    keys(&mut rt, "e");
    assert_eq!(rt.state.is_whole(g), whole, "e is a word end");

    let density = rt.state.settings.density;
    press(&mut rt, Key::Command('d'), ctrl());
    assert_eq!(rt.state.settings.density, density, "Ctrl+D is half a page");
    press(&mut rt, Key::Command('d'), ctrl_shift());
    assert_ne!(rt.state.settings.density, density, "Ctrl+Shift+D is density");
    assert!(matches!(rt.state.settings.density, Density::Compact | Density::Comfortable));

    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    keys(&mut rt, "y");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(clipboard.text().is_none(), "y waits for a motion; it copies no location");
}

#[test]
fn typing_in_the_composer_never_moves_the_cursor() {
    let mut rt = vim_rt(40);
    keys(&mut rt, "jw");
    let before = at(&rt);
    keys(&mut rt, "c");
    assert!(rt.state.composer.is_some() && rt.state.typing());
    keys(&mut rt, "w$Gggjkhlvy0e");
    assert_eq!(at(&rt), before);
    assert!(rt.state.active().visual().is_none());
    assert_eq!(rt.state.composer_edit.text, "w$Gggjkhlvy0e");
}

#[test]
fn visual_mode_says_so_and_comments_asks_or_yanks_its_text() {
    let mut rt = vim_rt(40);
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    let place = |rt: &Runtime<App>| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("status-where"))
            .and_then(|e| e.text)
    };
    keys(&mut rt, "jwv");
    assert_eq!(place(&rt).as_deref(), Some("-- VISUAL --"));
    keys(&mut rt, "e");
    keys(&mut rt, "c");
    let c = rt.state.composer.clone().expect("c on the selection opens the composer");
    assert_eq!(
        c.anchor,
        rain_core::Anchor::Span {
            side: rain_core::Side::New,
            start_line: 1,
            start_col: 3,
            end_line: 1,
            end_col: 8
        }
    );
    press(&mut rt, Key::Escape, Mods::default());
    press(&mut rt, Key::Escape, Mods::default());
    keys(&mut rt, "Vj");
    assert_eq!(place(&rt).as_deref(), Some("-- VISUAL LINE --"));
    keys(&mut rt, "y");
    assert_eq!(
        clipboard.text().as_deref().map(String::as_str),
        Some(
            "fn alpha(beta, gamma) {\n    let long = [w00, w01, w02, w03, w04, w05, w06, w07, w08, w09, w10, w11, w12, w13, w14, w15, w16, w17, w18, w19, w20, w21, w22, w23, w24, w25, w26, w27, w28, w29, w30, w31, w32, w33, w34, w35, w36, w37, w38, w39, w40, w41, w42, w43, w44, w45, w46, w47, w48, w49, w50, w51, w52, w53, w54, w55, w56, w57, w58, w59];\n"
        )
    );
    assert_eq!(rt.state.status, "Copied 2 lines");
    assert_ne!(place(&rt).as_deref(), Some("-- VISUAL LINE --"));
    keys(&mut rt, "Gyy");
    assert_eq!(clipboard.text().as_deref().map(String::as_str), Some("    line_40 = value_40;\n"));
    keys(&mut rt, "V");
    keys(&mut rt, "?");
    assert_eq!(rt.state.composer_ui.mode, rain_comments::ComposerMode::Question);
}

#[test]
fn cursor_keys_rebuild_only_the_rows_they_touch_and_idle_frames_commit_nothing() {
    let mut rt = vim_rt(400);
    keys(&mut rt, "jj");
    let (builds, commits) = (rain_diffview::line_builds(), rune::commit_generation());
    for _ in 0..100 {
        rt.key(KeyEvent {
            key: Key::Text("l".into()),
            mods: Mods::default(),
        });
        rt.frame(1.0 / 60.0);
    }
    let (b, c) = (rain_diffview::line_builds() - builds, rune::commit_generation() - commits);
    assert_eq!(at(&rt).1, 100);
    println!("100 l: {b} line builds, {c} commits");
    assert!(b <= 100, "100 l presses built {b} lines: the cursor's row once each");
    assert!(c <= 110, "{c} commits for 100 l presses");

    let (builds, commits) = (rain_diffview::line_builds(), rune::commit_generation());
    for _ in 0..50 {
        rt.key(KeyEvent {
            key: Key::Text("j".into()),
            mods: Mods::default(),
        });
        rt.frame(1.0 / 60.0);
    }
    let (b, c) = (rain_diffview::line_builds() - builds, rune::commit_generation() - commits);
    println!("50 j: {b} line builds, {c} commits");
    assert!(
        b <= 50 * 3,
        "50 j presses built {b} lines: the two cursor rows and one scrolled in each"
    );
    assert!(c <= 110, "{c} commits for 50 j presses");

    settle(&mut rt);
    let commits = rune::commit_generation();
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(
        rune::commit_generation() - commits,
        0,
        "a waiting count or cursor never wakes the view"
    );
    rt.check_live_only(false);
    let commits = rune::commit_generation();
    keys(&mut rt, "3g");
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(
        rune::commit_generation() - commits,
        0,
        "a count and a waiting g run no view and poll nothing"
    );
}

#[test]
fn scrolling_keeps_three_lines_of_margin_and_never_jumps() {
    let mut rt = vim_rt(300);
    let line = rt.state.active().metrics().line;
    keys(&mut rt, "j");
    let h = rt.state.active().viewport_height();
    let mut last = rt.state.active().scroll_offset();
    for _ in 0..120 {
        keys(&mut rt, "j");
        let y = rt.state.active().scroll_offset();
        assert!(y >= last && y - last <= line + 0.5, "j scrolled from {last} to {y}");
        assert!(
            on_screen(&rt) + 4.0 * line <= h + 0.5,
            "three lines below the cursor: {} of {h}",
            on_screen(&rt)
        );
        last = y;
    }
    for _ in 0..60 {
        keys(&mut rt, "k");
        let y = rt.state.active().scroll_offset();
        assert!(y <= last && last - y <= line + 0.5, "k scrolled from {last} to {y}");
        last = y;
    }
    let top = rt.state.active().metrics().header;
    assert!(
        on_screen(&rt) >= top + 3.0 * line - 0.5,
        "three lines above, under the sticky header"
    );

    let before = on_screen(&rt);
    let y = rt.state.active().scroll_offset();
    press(&mut rt, Key::Command('d'), ctrl());
    let moved = rt.state.active().scroll_offset() - y;
    assert!((moved - h / 2.0).abs() <= line, "half a viewport: {moved} of {h}");
    assert!((on_screen(&rt) - before).abs() <= 0.5, "the cursor keeps its place on screen");
    press(&mut rt, Key::Command('u'), ctrl());
    assert!((rt.state.active().scroll_offset() - y).abs() <= 0.5);

    keys(&mut rt, "gg");
    assert_eq!(rt.state.active().scroll_offset(), 0.0);
    keys(&mut rt, "G");
    let [_, max, ..] = rt.state.ruler.live.get();
    assert!((rt.state.active().scroll_offset() - max).abs() <= 1.0, "G reaches the end");
}

#[test]
fn long_lines_scroll_sideways_only_as_far_as_the_block_needs() {
    let mut rt = vim_rt(40);
    keys(&mut rt, "jj");
    for _ in 0..20 {
        keys(&mut rt, "l");
        assert_eq!(rt.state.active().scroll_offset_x(), 0.0, "on screen: no sideways scroll");
    }
    keys(&mut rt, "$");
    let x = rt.state.active().scroll_offset_x();
    assert!(x > 0.0, "the end of a long line scrolls into view");
    keys(&mut rt, "h");
    assert_eq!(rt.state.active().scroll_offset_x(), x, "a step back on screen keeps the scroll");
    keys(&mut rt, "0");
    assert_eq!(rt.state.active().scroll_offset_x(), 0.0, "0 goes back to the start");
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

fn hold(rt: &mut Runtime<App>, key: &str, n: usize) -> (f64, f64, u64) {
    let commits = rune::commit_generation();
    let mut times = Vec::with_capacity(n);
    for _ in 0..n {
        let t = Instant::now();
        rt.key(KeyEvent {
            key: Key::Text(key.into()),
            mods: Mods::default(),
        });
        rt.frame(1.0 / 60.0);
        times.push(t.elapsed().as_secs_f64() * 1e3);
    }
    let commits = rune::commit_generation() - commits;
    (pct(&mut times.clone(), 0.5), pct(&mut times, 0.95), commits)
}

/// A patch adding one `n`-line file whose lines are 40 to 120 columns of code.
fn big_patch(n: usize) -> PathBuf {
    let d = temp_dir();
    std::fs::create_dir_all(&d).unwrap();
    let mut p = format!("diff --git a/big.rs b/big.rs\nnew file mode 100644\n--- /dev/null\n+++ b/big.rs\n@@ -0,0 +1,{n} @@\n");
    for i in 0..n {
        let pad = "x".repeat(i % 80);
        p.push_str(&format!("+    let value_{i} = compute(alpha, beta, {pad}); // note {i}\n"));
    }
    let path = d.join("big.patch");
    std::fs::write(&path, p).unwrap();
    path
}

/// Frame cost of holding keys on a 20k-line file:
/// `cargo test -p rain-app holding_keys -- --ignored --nocapture`.
#[test]
#[ignore]
fn holding_keys_frame_cost() {
    let mut rt = runtime(app(big_patch(20_000), false));
    typed(&mut rt, "j");
    for key in ["j", "k", "j", "k", "l", "h", "w", "b"] {
        let (p50, p95, commits) = hold(&mut rt, key, 300);
        println!("hold {key} x300: p50 {p50:.2} ms, p95 {p95:.2} ms, {commits} commits");
    }
}
