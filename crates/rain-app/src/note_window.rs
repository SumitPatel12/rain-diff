//! The Intent window: a note popped out of the diff into its own OS window (one, reused: a
//! second pop-out replaces the first), with Show in diff, Keep on top and Esc to close.

use std::sync::OnceLock;

use rune::{AppHandle, AppSpec, Cursor, Cx, El, Key, KeyEvent, Tx, WindowHandle, WindowOptions, WindowSpec, col, el, row, text};
use rune_kit::Markdown;

use crate::app::App;
use crate::settings::Theme;
use crate::theme::{Scheme, TYPE, radius};
use crate::view::icons;

/// What the window shows: the note as owned data, so it crosses from the main window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoteContent {
    pub id: String,
    /// `Intent` or `Note`.
    pub label: String,
    /// `· claude · L23–24`.
    pub meta: String,
    pub title: Option<String>,
    pub body: String,
    /// `server/src/Api/VouchersController.cs:23-24`, when it sits on a file.
    pub place: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoteWindow {
    pub content: Option<NoteContent>,
    pub scheme: Option<Scheme>,
    pub on_top: bool,
}

struct Handles {
    window: (AppHandle<NoteWindow>, WindowHandle),
    main: AppHandle<App>,
}

static HANDLES: OnceLock<Handles> = OnceLock::new();

/// Sends `content` to the window and brings it to the front; false where there is no such
/// window (tests, headless renders).
pub(crate) fn show(content: NoteContent, scheme: Scheme) -> bool {
    let Some(h) = HANDLES.get() else { return false };
    let (app, window) = &h.window;
    app.update(move |w: &mut NoteWindow, _| {
        w.content = Some(content);
        w.scheme = Some(scheme);
    });
    window.open();
    window.focus();
    true
}

/// Closes the window (the note was brought back).
pub(crate) fn hide() {
    if let Some(h) = HANDLES.get() {
        h.window.1.close();
    }
}

/// Registers the hidden Intent window on `app`; `main` reaches the main window for Show in diff.
pub(crate) fn register(app: &mut rune::App<App>, main: AppHandle<App>) {
    let (fonts, families) = crate::fonts();
    let spec = AppSpec {
        scenes: &[("sample", sample)],
        fonts,
        families,
        on_key: Some(on_key),
        min_size: Some((420.0, 220.0)),
        ..AppSpec::new("Rain · Intent", (640.0, 420.0), NoteWindow::default, view)
    };
    let spec = WindowSpec::from_app("note", rune::App::new(spec).on_close(on_close)).options(WindowOptions {
        visible: false,
        ..WindowOptions::default()
    });
    let window = app.window(spec);
    let _ = HANDLES.set(Handles { window, main });
}

/// Esc, the close button or Bring back: the note shows inline again.
fn on_close(_: &mut NoteWindow, _: &mut Tx) {
    if let Some(h) = HANDLES.get() {
        h.main.update(|s: &mut App, _| s.popped_note = None);
    }
}

fn on_key(_: &mut NoteWindow, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if ev.key == Key::Escape {
        tx.close_window();
        return true;
    }
    false
}

fn show_in_diff(w: &mut NoteWindow) {
    let Some(c) = &w.content else { return };
    let id = c.id.clone();
    if let Some(h) = HANDLES.get() {
        h.main.update(move |s: &mut App, tx: &mut Tx| s.goto_note(&id, tx));
        h.main.focus();
    }
}

fn toggle_on_top(w: &mut NoteWindow) {
    w.on_top = !w.on_top;
    if let Some(h) = HANDLES.get() {
        h.window.1.set_always_on_top(w.on_top);
    }
}

pub fn view(w: &NoteWindow, cx: &mut Cx<NoteWindow>) -> El {
    let scheme = w.scheme.unwrap_or_else(|| Theme::System.scheme(cx.system_dark()));
    let look = crate::theme::look(scheme);
    let k = &look.kit;
    let root = col().key("note-window").w_full().h_full().fill(look.s.reader);
    let Some(c) = &w.content else {
        return root.child(
            text("Pop out a note to read it here")
                .font(TYPE.ui)
                .color(k.c.text_muted)
                .padding(24.0),
        );
    };
    let button = |key: &str, label: &str, on: bool| {
        k.interactive(
            row()
                .key(key.to_owned())
                .role("button")
                .label(label.to_owned())
                .checked(on)
                .h(26.0)
                .px(8.0)
                .items_center()
                .radius(radius::BUTTON)
                .border(1.0, look.s.card_border)
                .fill(if on { k.c.pressed } else { rune::Color::TRANSPARENT })
                .cursor(Cursor::Pointer)
                .child(text(label.to_owned()).font(TYPE.small).color(k.c.text_base).a11y_hidden()),
        )
    };
    let mut meta = c.meta.clone();
    if let Some(p) = &c.place {
        let path = p.rsplit_once(':').map_or(p.as_str(), |(path, _)| path);
        meta = format!("{meta} · {}", path.replacen('/', " · ", 1));
    }
    let head = row()
        .key("note-window-head")
        .w_full()
        .px(20.0)
        .pt(14.0)
        .pb(6.0)
        .gap(6.0)
        .items_center()
        .child(icons::draw(&icons::SPARK, 14.0, look.r.note))
        .child(text(c.label.clone()).font(TYPE.label).color(look.r.note))
        .child(text(meta).font(TYPE.small).color(k.c.text_faint).truncate().shrink());
    let mut body = col().key("note-window-body").w_full().px(20.0).pb(14.0).gap(8.0);
    if let Some(t) = &c.title {
        body = body.child(text(t.clone()).font(TYPE.h2).color(k.c.text_base));
    }
    body = body.child(Markdown::new(c.body.clone()).selectable(true).build(k));
    let switch = {
        let track = row()
            .w(26.0)
            .h(14.0)
            .radius(7.0)
            .px(2.0)
            .items_center()
            .fill(if w.on_top { look.r.accent } else { k.c.layer_03 })
            .child(
                el().size(10.0, 10.0)
                    .radius(5.0)
                    .fill(if w.on_top { look.r.on_accent } else { k.c.text_base }),
            );
        let track = if w.on_top { track.justify_end() } else { track };
        row()
            .key("note-window-top")
            .role("checkbox")
            .label("Keep on top")
            .checked(w.on_top)
            .gap(8.0)
            .items_center()
            .cursor(Cursor::Pointer)
            .child(track.a11y_hidden())
            .child(text("Keep on top").font(TYPE.small).color(k.c.text_base).a11y_hidden())
            .on_press(toggle_on_top)
    };
    let mut foot = row()
        .key("note-window-foot")
        .w_full()
        .h(44.0)
        .px(14.0)
        .gap(12.0)
        .items_center()
        .fill(look.s.card_head);
    if c.place.is_some() {
        foot = foot.child(button("note-window-show", "Show in diff", false).on_press(show_in_diff));
    }
    foot = foot
        .child(switch)
        .child(el().grow(1.0).a11y_hidden())
        .child(text("Esc closes").font(TYPE.small).color(k.c.text_faint));
    root.child(head)
        .child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(rune::ScrollSpec { stick_bottom: false })
                .child(body),
        )
        .child(el().w_full().h(1.0).fill(look.s.hairline))
        .child(foot)
}

/// Headless scene: the notes scene's line note, popped out.
fn sample(w: &mut NoteWindow) {
    let mut a = crate::init();
    crate::scene_base(&mut a);
    crate::seed_notes(&mut a);
    let id = a.mirror.notes.iter().rev().find(|n| n.anchor.is_some()).map(|n| n.id.clone());
    w.content = id.and_then(|id| a.note_content(&id));
    w.scheme = crate::theme_flag().map(|t| t.scheme(true));
}
