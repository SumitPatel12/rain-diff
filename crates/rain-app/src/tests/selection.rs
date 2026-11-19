use rain_comments::ComposerMode;
use rain_core::{Anchor, EventKind, LineKind, Side, ThreadKind};
use rain_diffview::LineRef;
use rune::{ElementInfo, Key, Mods, Runtime};

use super::*;
use crate::app::Overlay;

fn tick(rt: &mut Runtime<App>) {
    rt.frame(1.0 / 60.0);
}

fn live(rt: &Runtime<App>, key: &str) -> bool {
    rt.elements().iter().any(|e| !e.exiting && e.key.as_deref() == Some(key))
}

fn el(rt: &Runtime<App>, key: &str) -> ElementInfo {
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && e.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no element {key}"))
}

fn click(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_move(x, y);
    tick(rt);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// The first file's added lines (new numbers) with at least `min` characters of code.
fn added_lines(rt: &Runtime<App>, min: usize) -> Vec<u32> {
    let f = &rt.state.diff.files()[0];
    f.lines
        .iter()
        .filter(|l| l.kind == LineKind::Added && f.text[l.start as usize..l.end as usize].trim().len() >= min)
        .map(|l| l.new)
        .collect()
}

/// The code text of new line `n` of the first file, in either layout.
fn code(rt: &Runtime<App>, n: u32) -> ElementInfo {
    let a = rt.state.active();
    let row = a
        .row_of_line(0, LineRef::new(Side::New, n))
        .and_then(|r| a.row_key(r as usize))
        .unwrap();
    let ends = [format!("/{row}/new/code/text"), format!("/{row}/code/text")];
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && ends.iter().any(|end| e.path.ends_with(end.as_str())))
        .unwrap_or_else(|| panic!("no code text for line {n}"))
}

/// Presses at the start of line `from`'s code and drags 120pt into line `to`'s; `release` lets
/// go. Where the pointer ends.
fn drag(rt: &mut Runtime<App>, from: u32, to: u32, release: bool) -> (f32, f32) {
    let (a, b) = (code(rt, from), code(rt, to));
    let (x0, y0) = (a.rect[0] + 1.0, a.rect[1] + a.rect[3] / 2.0);
    let (x1, y1) = (b.rect[0] + 120.0, b.rect[1] + b.rect[3] / 2.0);
    rt.pointer_move(x0, y0);
    tick(rt);
    rt.pointer_down(x0, y0);
    tick(rt);
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        rt.pointer_move(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
        tick(rt);
    }
    if release {
        rt.pointer_up(x1, y1);
        settle(rt);
    }
    (x1, y1)
}

/// The story fixture with text dragged over two added lines of the first file.
fn selected() -> (Runtime<App>, u32, u32) {
    let mut rt = runtime(app(story(), true));
    let lines = added_lines(&rt, 12);
    let (a, b) = (lines[0], lines[1]);
    let _ = drag(&mut rt, a, b, true);
    (rt, a, b)
}

#[test]
fn releasing_a_text_selection_shows_the_toolbar_beside_it() {
    let (rt, a, _) = selected();
    let span = rt.state.active().span_selection().cloned().expect("a span");
    assert_eq!((span.file, span.side, span.start_line), (0, Side::New, a));
    for key in ["sel-toolbar", "sel-comment", "sel-ask", "sel-copy"] {
        assert!(live(&rt, key), "{key}");
    }
    let bar = el(&rt, "sel-toolbar").rect;
    let first = code(&rt, a).rect;
    let column = el(&rt, "diff-col").rect;
    assert!(bar[1] + bar[3] <= first[1] + 1.0, "above the first line: {bar:?} vs {first:?}");
    assert!(first[1] - (bar[1] + bar[3]) < 20.0, "next to it, not far off: {bar:?} vs {first:?}");
    assert!(
        bar[0] >= column[0] && bar[0] + bar[2] <= column[0] + column[2],
        "inside the diff column"
    );
}

#[test]
fn no_toolbar_while_the_drag_is_still_going() {
    let mut rt = runtime(app(story(), true));
    let lines = added_lines(&rt, 12);
    let (x, y) = drag(&mut rt, lines[0], lines[1], false);
    settle(&mut rt);
    assert!(!live(&rt, "sel-toolbar"));
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(live(&rt, "sel-toolbar"), "it shows on release");
}

#[test]
fn clearing_the_selection_or_esc_hides_the_toolbar() {
    let (mut rt, _, _) = selected();
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!live(&rt, "sel-toolbar"), "Esc hides it");
    assert!(rt.state.active().span_selection().is_some(), "and keeps the selection for c / ?");

    let (mut rt, _, _) = selected();
    rt.update(|s, tx| {
        s.active_mut().set_span_selection(None);
        tx.clear_text_selection();
    });
    settle(&mut rt);
    assert!(!live(&rt, "sel-toolbar"), "a cleared selection takes it away");
}

#[test]
fn comment_opens_a_floating_card_on_the_selection() {
    let (mut rt, a, b) = selected();
    let quoted = rt.state.active().span_selection().unwrap().text.clone();
    let last = code(&rt, b).rect;
    click(&mut rt, "sel-comment");
    let c = rt.state.composer.clone().expect("a composer");
    assert!(matches!(c.anchor, Anchor::Span { start_line, end_line, .. } if start_line == a && end_line == b));
    assert_eq!(c.preview, quoted, "the selected text is quoted");
    assert_eq!(rt.state.composer_ui.mode, ComposerMode::Comment);
    assert!(rt.state.layout.is_open(Overlay::Composer));
    assert!(rt.state.composer_ui.card);
    assert!(live(&rt, "overlay-composer") && live(&rt, "composer-card"));
    assert!(!live(&rt, "sel-toolbar"), "the composer replaces the toolbar");
    let card = el(&rt, "overlay-composer").rect;
    assert!(card[1] >= last[1] + last[3], "below the selection: {card:?} vs {last:?}");
    assert!(card[1] - (last[1] + last[3]) < 40.0, "right under it");
    typed(&mut rt, "Unchecked amount");
    press(&mut rt, Key::Enter, ctrl());
    let t = rt.state.shared.lock().threads.last().cloned().unwrap();
    assert_eq!(t.kind, Some(ThreadKind::Comment));
    assert!(matches!(t.anchor, Anchor::Span { .. }));
    assert!(!live(&rt, "overlay-composer"));
}

#[test]
fn ask_opens_question_mode_and_esc_keeps_a_draft() {
    let (mut rt, _, _) = selected();
    click(&mut rt, "sel-ask");
    assert_eq!(rt.state.composer_ui.mode, ComposerMode::Question);
    assert!(live(&rt, "question-hint"));
    typed(&mut rt, "why here");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.composer.is_none() && !live(&rt, "overlay-composer"));
    assert_eq!(
        rt.state.shared.lock().drafts.last().map(|d| d.text.clone()).as_deref(),
        Some("why here")
    );

    let (mut rt, _, _) = selected();
    click(&mut rt, "sel-ask");
    typed(&mut rt, "Why here?");
    press(&mut rt, Key::Enter, ctrl());
    let t = rt.state.shared.lock().threads.last().cloned().unwrap();
    assert_eq!(t.kind, Some(ThreadKind::Question));
    assert_eq!(rt.state.shared.events_since(0, &[EventKind::Ask]).len(), 1);
}

#[test]
fn copy_puts_the_selection_on_the_clipboard() {
    let (mut rt, _, _) = selected();
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    let want = rt.state.active().span_selection().unwrap().text.clone();
    click(&mut rt, "sel-copy");
    assert_eq!(clipboard.text().as_deref().map(String::as_str), Some(want.as_str()));
}

#[test]
fn keys_still_open_the_inline_composer_on_a_selection() {
    for key in ["c", "?"] {
        let (mut rt, a, _) = selected();
        typed(&mut rt, key);
        let c = rt.state.composer.clone().expect("a composer");
        assert!(matches!(c.anchor, Anchor::Span { start_line, .. } if start_line == a));
        assert!(!rt.state.composer_ui.long && !rt.state.composer_ui.card, "{key} stays inline");
        assert!(live(&rt, "composer"), "{key}: under the line");
        assert!(!live(&rt, "overlay-composer"));
    }
}

#[test]
fn a_line_range_shows_the_toolbar_at_its_last_row() {
    let mut rt = runtime(app(story(), true));
    let lines = added_lines(&rt, 12);
    let (a, b) = (lines[0], lines[1]);
    rt.update(move |s, tx| {
        let range = rain_diffview::LineRange {
            file: 0,
            start: LineRef::new(Side::New, a),
            end: LineRef::new(Side::New, b),
        };
        s.active_mut().set_selection(Some(range));
        s.lines_selected(Some(range));
        let _ = tx;
    });
    settle(&mut rt);
    assert!(live(&rt, "sel-toolbar"));
    let bar = el(&rt, "sel-toolbar").rect;
    let last = code(&rt, b).rect;
    let a = rt.state.active();
    let key = a
        .row_of_line(0, LineRef::new(Side::New, b))
        .and_then(|r| a.row_key(r as usize))
        .unwrap();
    let row = el(&rt, &key).rect;
    assert!(
        bar[1] >= last[1] + last[3] && bar[1] - (last[1] + last[3]) < 20.0,
        "under the last row: {bar:?} vs {last:?}"
    );
    assert!(
        (row[0] + row[2] - (bar[0] + bar[2]) - 16.0).abs() < 4.0,
        "at the row's right end: {bar:?} in {row:?}"
    );
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    let want = rt.state.active().selected_text().expect("the lines' text");
    click(&mut rt, "sel-copy");
    assert_eq!(clipboard.text().as_deref().map(String::as_str), Some(want.as_str()));
}

#[test]
fn selection_scenes_show_their_surface_inside_the_window_at_both_sizes() {
    for (name, want) in [
        ("selection-toolbar", "sel-toolbar"),
        ("selection-composer", "composer-card"),
        ("context-menu", "diff-menu-menu"),
    ] {
        for size in [(1366.0, 768.0), (1920.0, 1080.0)] {
            let mut a = app(story(), true);
            crate::tests::shell_scene(name)(&mut a);
            let mut rt = sized_runtime(a, size);
            settle(&mut rt);
            assert!(live(&rt, want), "{name} at {size:?} shows {want}");
            let r = el(&rt, want).rect;
            assert!(
                r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= size.0 && r[1] + r[3] <= size.1,
                "{name} at {size:?}: {want} {r:?} inside the window"
            );
            if name == "selection-toolbar" {
                let column = el(&rt, "diff-col").rect;
                assert!(
                    r[0] >= column[0] && r[0] + r[2] <= column[0] + column[2],
                    "{size:?}: in the diff column"
                );
                let sel = rt.state.active().span_selection().cloned().expect("the scene selects text");
                assert!(!sel.text.trim().is_empty());
            }
        }
    }
}

/// Right-clicks new line `n` of the first file.
fn right_click(rt: &mut Runtime<App>, n: u32) {
    let r = code(rt, n).rect;
    let (x, y) = (r[0] + 20.0, r[1] + r[3] / 2.0);
    rt.pointer_move(x, y);
    tick(rt);
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(rt);
}

fn menu_item(rt: &Runtime<App>, label: &str) -> String {
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && e.role == "menuitem" && e.label.as_deref() == Some(label))
        .and_then(|e| e.key)
        .unwrap_or_else(|| panic!("no menu item {label}"))
}

#[test]
fn the_right_click_menu_runs_what_the_keys_run_on_the_clicked_line() {
    let mut rt = runtime(app(story(), true));
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    let n = added_lines(&rt, 1)[1];
    right_click(&mut rt, n);
    assert!(live(&rt, "diff-menu-menu"), "the menu opens");
    let key = menu_item(&rt, "Copy location");
    click(&mut rt, &key);
    assert!(!live(&rt, "diff-menu-menu"), "an item closes it");
    let want = format!("{}:{n}", rt.state.keys[0]);
    assert_eq!(
        clipboard.text().as_deref().map(String::as_str),
        Some(want.as_str()),
        "what Shift+Y copies"
    );

    let was = rt.state.is_viewed(0);
    right_click(&mut rt, n);
    let key = menu_item(&rt, "Toggle viewed");
    click(&mut rt, &key);
    assert_eq!(rt.state.is_viewed(0), !was, "what v does");
    // Viewed folds the file and brings the next one up; unmark it and go back so its lines are
    // there to click again.
    if rt.state.is_viewed(0) {
        assert!(rt.state.active().collapsed(0), "viewed folds the file");
        rt.update(|s, tx| {
            s.set_viewed(0, false);
            s.active_mut().scroll_to_file(0, tx);
        });
        for _ in 0..3 {
            settle(&mut rt);
        }
    }

    right_click(&mut rt, n);
    let key = menu_item(&rt, &format!("Comment on line {n}"));
    click(&mut rt, &key);
    let c = rt.state.composer.clone().expect("a composer");
    assert_eq!(
        c.anchor,
        Anchor::Line { side: Side::New, line: n },
        "on the clicked line, as c on it"
    );
    assert_eq!(rt.state.composer_ui.mode, ComposerMode::Comment);
    assert!(rt.state.composer_ui.card && live(&rt, "overlay-composer"), "floated at the click");
}

#[test]
fn with_a_selection_the_menu_acts_on_it() {
    let (mut rt, a, b) = selected();
    let quoted = rt.state.active().span_selection().unwrap().text.clone();
    right_click(&mut rt, b);
    let key = menu_item(&rt, "Ask about selection");
    click(&mut rt, &key);
    let c = rt.state.composer.clone().expect("a composer");
    assert!(matches!(c.anchor, Anchor::Span { start_line, .. } if start_line == a));
    assert_eq!(c.preview, quoted);
    assert_eq!(rt.state.composer_ui.mode, ComposerMode::Question);
}
