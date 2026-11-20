use std::sync::Arc;

use rain_core::keymap::Profile;
use rain_core::{Launch, Shared};
use rune::{Key, Mods, Runtime};

use super::*;
use crate::app::layout::Overlay;
use crate::settings::{PICKER_MIN, Settings};

fn app_at(dir: &std::path::Path) -> App {
    let mut a = App::new(
        Launch {
            session: "t".into(),
            fixture: Some(story()),
            overview: Some(overview()),
            ..Launch::default()
        },
        Arc::new(Shared::default()),
        Some(dir.join("sessions")),
        Some(dir.join("settings.json")),
    );
    a.load_now();
    a
}

fn rect(rt: &Runtime<App>, key: &str) -> [f32; 4] {
    rt.presented(key).unwrap_or_else(|| panic!("{key} is drawn")).0
}

/// Whether `row` is drawn wholly inside the viewport `list`.
fn shown_in(rt: &Runtime<App>, list: &str, row: &str) -> bool {
    let (l, r) = (rect(rt, list), rect(rt, row));
    r[1] >= l[1] - 0.5 && r[1] + r[3] <= l[1] + l[3] + 0.5
}

fn drag(rt: &mut Runtime<App>, key: &str, dx: f32, dy: f32) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("{key}"));
    rt.pointer_down(x, y);
    rt.frame(1.0 / 60.0);
    for i in 1..=4 {
        let f = i as f32 / 4.0;
        rt.pointer_move(x + dx * f, y + dy * f);
        rt.frame(1.0 / 60.0);
    }
    rt.pointer_up(x + dx, y + dy);
    settle(rt);
}

fn open(rt: &mut Runtime<App>) {
    press(rt, Key::Command('p'), ctrl());
    assert!(rt.state.layout.is_open(Overlay::Picker));
}

#[test]
fn the_picker_resizes_by_its_edges_and_reopens_at_the_saved_size() {
    let dir = temp_dir();
    let mut rt = runtime(app_at(&dir));
    open(&mut rt);
    let r = rect(&rt, "picker");
    assert_eq!((r[2], r[3]), (940.0, 520.0), "the default size");
    let list = rect(&rt, "picker-list")[2];

    drag(&mut rt, "picker-grip-right", 50.0, 30.0);
    let r = rect(&rt, "picker");
    assert_eq!(
        (r[2], r[3]),
        (1040.0, 520.0),
        "the right edge: width only, both sides as it stays centred"
    );
    assert_eq!(
        rect(&rt, "picker-list")[2],
        list,
        "the list keeps its width; the preview takes the rest"
    );
    drag(&mut rt, "picker-grip-bottom", 40.0, 60.0);
    assert_eq!(rect(&rt, "picker")[3], 580.0, "the bottom edge: height only");
    drag(&mut rt, "picker-grip-corner", -100.0, -100.0);
    let r = rect(&rt, "picker");
    assert_eq!((r[2], r[3]), (840.0, 480.0), "the corner: both");
    assert_eq!(rect(&rt, "picker-list")[2], 378.0, "a narrow panel gives the list 45%");

    drag(&mut rt, "picker-grip-corner", -2000.0, -2000.0);
    let r = rect(&rt, "picker");
    assert_eq!((r[2], r[3]), PICKER_MIN, "never below the minimum");
    drag(&mut rt, "picker-grip-corner", 3000.0, 3000.0);
    let r = rect(&rt, "picker");
    assert_eq!(r[2], 1600.0 - 40.0, "never wider than the window");
    assert!(r[1] + r[3] <= 1000.0 - 30.0, "never past the status bar: {r:?}");
    drag(&mut rt, "picker-grip-corner", -500.0, -200.0);
    let saved = rt.state.settings.picker_size;
    assert_eq!(saved, (rect(&rt, "picker")[2], rect(&rt, "picker")[3]));

    let text = std::fs::read_to_string(dir.join("settings.json")).unwrap();
    assert!(text.contains("\"pickerSize\""), "{text}");
    assert_eq!(Settings::load(&dir.join("settings.json")).picker_size, saved);
    press(&mut rt, Key::Enter, Mods::default());

    let mut again = runtime(app_at(&dir));
    open(&mut again);
    let r = rect(&again, "picker");
    assert_eq!((r[2], r[3]), saved, "a new window reopens it at the saved size");
}

#[test]
fn an_old_or_odd_settings_file_gives_the_default_picker_size() {
    let default = Settings::default().picker_size;
    assert_eq!(default, (940.0, 520.0));
    let old = Settings::parse(br#"{"theme": "dark", "sidebarWidth": 300}"#).expect("an old file loads");
    assert_eq!((old.picker_size, old.sidebar_width), (default, 300.0));
    for odd in [r#""big""#, "[1, 2]", "[-5, 10]", "null", "[1e300, 600]"] {
        let s = Settings::parse(format!(r#"{{"pickerSize": {odd}, "sidebarWidth": 300}}"#).as_bytes())
            .unwrap_or_else(|e| panic!("{odd} still loads: {e}"));
        assert_eq!(s.sidebar_width, 300.0, "{odd}: the rest is kept");
        assert!(s.picker_size == default || odd.contains("1e300"), "{odd}: {:?}", s.picker_size);
    }
    let s = Settings::parse(br#"{"pickerSize": [700, 400]}"#).unwrap();
    assert_eq!(s.picker_size, (700.0, 400.0));
}

#[test]
fn moving_the_picker_selection_keeps_its_row_in_view() {
    let mut rt = runtime(app(story(), true));
    open(&mut rt);
    let n = rt.state.picker_rows().len();
    assert!(n as f32 * 32.0 > rect(&rt, "picker-list")[3] + 64.0, "enough rows to scroll: {n}");
    for _ in 0..n + 3 {
        press(&mut rt, Key::Down, Mods::default());
        let sel = rt.state.picker.sel;
        assert!(shown_in(&rt, "picker-list", &format!("pick-{sel}")), "row {sel} after Down");
    }
    assert_eq!(rt.state.picker.sel, n - 1);
    assert!(!shown_in(&rt, "picker-list", "pick-0"), "the list scrolled");
    for _ in 0..n {
        press(&mut rt, Key::Up, Mods::default());
        let sel = rt.state.picker.sel;
        assert!(shown_in(&rt, "picker-list", &format!("pick-{sel}")), "row {sel} after Up");
    }
    assert_eq!(rt.state.picker.sel, 0);
}

#[test]
fn moving_the_palette_selection_keeps_its_row_in_view() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| s.open_palette());
    settle(&mut rt);
    let n = crate::palette::filtered(&rt.state, "").len();
    assert!(n > 20, "enough rows to scroll: {n}");
    for _ in 0..n + 3 {
        press(&mut rt, Key::Down, Mods::default());
        let sel = rt.state.palette.sel;
        assert!(shown_in(&rt, "palette-rows", &format!("pal-{sel}")), "row {sel} after Down");
    }
    assert_eq!(rt.state.palette.sel, n - 1);
    for _ in 0..n {
        press(&mut rt, Key::Up, Mods::default());
        let sel = rt.state.palette.sel;
        assert!(shown_in(&rt, "palette-rows", &format!("pal-{sel}")), "row {sel} after Up");
    }
    assert_eq!(rt.state.palette.sel, 0);
    assert!(shown_in(&rt, "palette-rows", "pal-group-0"), "the top shows its group's title");
}

fn mode(rt: &Runtime<App>) -> Option<String> {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("picker-mode"))
        .and_then(|e| e.text)
}

#[test]
fn vim_mode_opens_the_picker_in_insert_and_esc_gives_normal_mode() {
    let mut rt = runtime(app(story(), true));
    assert!(rt.state.keymap_profile().vim_motions(), "Vim is the default profile");
    open(&mut rt);
    assert!(!rt.state.picker_normal());
    assert_eq!(mode(&rt).as_deref(), Some("INSERT"));
    typed(&mut rt, "jk");
    assert_eq!(rt.state.picker.edit.text, "jk", "j and k type in insert mode");
    let at = rt.state.active().cursor();
    for _ in 0..2 {
        press(&mut rt, Key::Backspace, Mods::default());
    }

    press(&mut rt, Key::Down, Mods::default());
    assert_eq!(rt.state.picker.sel, 1, "↓ moves in insert mode");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.layout.is_open(Overlay::Picker), "Esc leaves insert mode, not the picker");
    assert!(rt.state.picker_normal());
    assert_eq!(mode(&rt).as_deref(), Some("NORMAL"));
    assert!(!rt.state.picker.edit.focused, "the query box lets go");
    assert!(rt.elements().iter().all(|e| e.caret.is_none()), "no caret in normal mode");

    let n = rt.state.picker_rows().len();
    typed(&mut rt, "jj");
    assert_eq!(rt.state.picker.sel, 3, "j moves down");
    typed(&mut rt, "k");
    assert_eq!(rt.state.picker.sel, 2, "k moves up");
    press(&mut rt, Key::Down, Mods::default());
    press(&mut rt, Key::Up, Mods::default());
    press(&mut rt, Key::Up, Mods::default());
    assert_eq!(rt.state.picker.sel, 1, "the arrows too");
    press(
        &mut rt,
        Key::Text("G".into()),
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    assert_eq!(rt.state.picker.sel, n - 1, "G: the bottom");
    assert!(shown_in(&rt, "picker-list", &format!("pick-{}", n - 1)));
    typed(&mut rt, "g");
    assert_eq!(rt.state.picker.sel, n - 1, "one g waits");
    typed(&mut rt, "g");
    assert_eq!(rt.state.picker.sel, 0, "gg: the top");
    assert!(shown_in(&rt, "picker-list", "pick-0"));
    typed(&mut rt, "xzc");
    assert_eq!(rt.state.picker.edit.text, "", "normal mode types nothing");
    assert!(
        rt.state.layout.is_open(Overlay::Picker) && rt.state.composer.is_none(),
        "nor runs the reader's keys"
    );
    assert_eq!(rt.state.active().cursor(), at, "the diff never moved");

    typed(&mut rt, "i");
    assert!(!rt.state.picker_normal() && rt.state.picker.edit.focused, "i: insert again");
    typed(&mut rt, "vsc");
    assert_eq!(rt.state.picker.edit.text, "vsc");
    press(&mut rt, Key::Escape, Mods::default());
    assert_eq!(rt.state.picker.edit.text, "vsc", "the query stays");
    typed(&mut rt, "/");
    assert!(rt.state.picker.edit.focused, "/: insert again");
    assert_eq!(rt.state.picker.edit.text, "vsc", "the / isn't typed");
    press(&mut rt, Key::Escape, Mods::default());
    press(&mut rt, Key::Enter, Mods::default());
    assert!(!rt.state.layout.is_open(Overlay::Picker), "Enter goes from normal mode");
    assert!(rt.state.keys[rt.state.current().unwrap()].ends_with("VouchersController.cs"));

    open(&mut rt);
    press(&mut rt, Key::Escape, Mods::default());
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.layout.is_open(Overlay::Picker), "Esc in normal mode closes");
    assert!(!rt.state.picker_normal());
}

#[test]
fn without_vim_the_picker_has_no_modes_and_esc_closes() {
    for p in [Profile::VsCode, Profile::JetBrains] {
        let mut rt = runtime(app(story(), true));
        rt.update(|s, _| s.set_profile(p));
        settle(&mut rt);
        rt.update(|s, tx| s.open_picker(tx));
        settle(&mut rt);
        assert_eq!(mode(&rt), None, "{p:?}: no mode tag");
        typed(&mut rt, "jk");
        assert_eq!(rt.state.picker.edit.text, "jk", "{p:?}");
        press(&mut rt, Key::Escape, Mods::default());
        assert!(!rt.state.layout.is_open(Overlay::Picker), "{p:?}: Esc closes");
        assert!(!rt.state.picker_normal());
    }
}
