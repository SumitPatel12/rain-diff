//! The composer with its Comment and Question modes: inline under the anchored line, as a
//! floating panel on an artifact, or as the draggable long editor with a Markdown preview.

use rain_comments::{Composer, ComposerMode};
use rain_core::{Draft, ThreadKind};
use rune::{El, Tx, col, row, text};
use rune_kit::{Kit, Markdown};

use crate::app::App;

const PANEL_W: f32 = 640.0;
const LONG_W: f32 = 980.0;

/// The composer's title: the anchor (`server/src/X.cs:L27-L31`) or the artifact.
fn title(s: &App) -> String {
    if let Some(a) = &s.composer_ui.artifact {
        return a.title.clone();
    }
    match &s.composer {
        Some(c) => match c.anchor.lines() {
            (a, b) if a == b => format!("on line {a}"),
            (a, b) => format!("on lines {a}–{b}"),
        },
        None => String::new(),
    }
}

fn editing(s: &App) -> bool {
    s.composer.as_ref().is_some_and(|c| c.edit_of.is_some())
}

/// The floating card's title: the mode and the place, `Comment on ask.rs · line 13`.
fn card_title(s: &App) -> String {
    let what = if s.composer_ui.mode == ComposerMode::Question {
        "Ask about"
    } else {
        "Comment on"
    };
    if let Some(a) = &s.composer_ui.artifact {
        return format!("{what} {}", a.title);
    }
    let Some(c) = &s.composer else { return what.to_owned() };
    let key = s.keys.get(c.file).map_or("", String::as_str);
    let name = key.rsplit('/').next().unwrap_or(key);
    match c.anchor.lines() {
        (a, b) if a == b => format!("{what} {name} · line {a}"),
        (a, b) => format!("{what} {name} · lines {a}–{b}"),
    }
}

/// The composer wired to the app, `rows` tall; `bare` inside a card that frames and titles it.
fn wired(s: &App, k: &Kit, rows: (usize, usize), long: bool, bare: bool) -> El {
    let mut c = Composer::new(&s.composer_edit, |s: &mut App| &mut s.composer_edit)
        .anchor_label(title(s))
        .rows(rows.0, rows.1)
        .on_submit(|s: &mut App, body: String, _: &mut Tx| s.submit_composer(body))
        .on_cancel(|s: &mut App, _: &mut Tx| s.cancel_composer());
    if let Some(p) = s.composer.as_ref().map(|c| c.preview.clone()) {
        c = c.preview(p);
    }
    if editing(s) {
        return c.placeholder("Edit the message… (Ctrl+Enter to save, Esc to cancel)").build(k);
    }
    c = c
        .mode(s.composer_ui.mode)
        .kind(s.composer_ui.kind, s.composer_ui.blocking)
        .on_mode(|s: &mut App, m: ComposerMode, _: &mut Tx| s.set_composer_mode(m))
        .on_kind(|s: &mut App, kd: ThreadKind, _: &mut Tx| s.set_composer_kind(kd))
        .on_blocking(|s: &mut App, _: &mut Tx| s.toggle_composer_blocking())
        .on_expand(|s: &mut App, _: &mut Tx| s.toggle_long_editor())
        .kind_menu(s.composer_ui.kind_menu, |s: &mut App, _: &mut Tx| {
            s.composer_ui.kind_menu = !s.composer_ui.kind_menu;
        })
        .look(super::diff::card_look(k));
    if s.composer.is_some() {
        c = c.on_suggest(|s: &mut App, _: &mut Tx| s.insert_suggestion());
    }
    if bare {
        c = c.bare();
    }
    let built = c.build(k);
    if long { built.key("long-composer") } else { built }
}

/// The composer under its line in the diff.
pub(super) fn inline(s: &App, k: &Kit) -> El {
    wired(s, k, (3, 12), false, false)
}

/// What stays under the line while the long editor has the text.
pub(super) fn long_note(k: &Kit) -> El {
    row()
        .key("composer")
        .role("group")
        .label("Comment composer")
        .w_full()
        .gap(8.0)
        .padding(6.0)
        .radius(k.radius.md)
        .border(1.0, k.c.border_muted)
        .border_dash(4.0, 3.0)
        .center_y()
        .child(
            text("Writing in the long editor")
                .font(k.ty.small_medium)
                .color(k.c.text_muted)
                .grow(1.0),
        )
        .child(
            rune_kit::Button::new("⤢ Back here")
                .variant(rune_kit::ButtonVariant::Ghost)
                .control_size(rune_kit::ControlSize::Small)
                .on_press(|s: &mut App| s.toggle_long_editor())
                .build(k)
                .key("long-back"),
        )
}

/// An unsent draft on its line: a dashed mark with its first line; `c` or `?` reopens it.
pub(super) fn draft_mark(d: &Draft, k: &Kit) -> El {
    let first = rain_comments::label::first_sentence(&d.text, 90);
    let what = if d.kind == Some(ThreadKind::Question) {
        "Draft question"
    } else {
        "Draft"
    };
    row()
        .key("draft")
        .role("note")
        .label(what)
        .w_full()
        .gap(8.0)
        .px(8.0)
        .py(4.0)
        .radius(k.radius.md)
        .border(1.0, k.c.border_strong)
        .border_dash(2.0, 3.0)
        .center_y()
        .child(text(what).font(k.ty.small_medium).color(k.c.text_muted))
        .child(text(first).font(k.ty.small).color(k.c.text_base).truncate().grow(1.0).shrink())
        .child(text("c to continue").font(k.ty.small).color(k.c.text_faint))
}

/// The overlay: the long editor (a draggable window with a preview), or the floating composer of
/// an artifact. Not modal: it brings no scrim and leaves the reader usable.
pub(super) fn composer(s: &App, k: &Kit) -> Option<El> {
    if s.composer.is_none() && s.composer_ui.artifact.is_none() {
        return None;
    }
    if s.composer_ui.long && s.composer_ui.card {
        return Some(card(s, k));
    }
    let long = s.composer_ui.long;
    let (x, y) = if long {
        s.composer_ui.long_position()
    } else {
        (s.window_width.max(PANEL_W + 40.0) / 2.0 - PANEL_W / 2.0, 24.0)
    };
    let width = if long { LONG_W } else { PANEL_W };
    let grip = row()
        .key("composer-grip")
        .role("toolbar")
        .label(if long { "Long editor" } else { "Composer" })
        .w_full()
        .h(26.0)
        .px(10.0)
        .gap(8.0)
        .center_y()
        .fill(k.c.layer_02)
        .child(
            text(if long { "Long editor · drag to move" } else { "Composer" })
                .font(k.ty.small_medium)
                .color(k.c.text_muted)
                .grow(1.0),
        )
        .on_drag(|s: &mut App, d: &mut rune::Drag, _: &mut Tx| s.drag_long_editor(d.phase, d.dx, d.dy));
    let mut body = row().key("composer-body").w_full().gap(8.0).padding(8.0).items_stretch();
    if long {
        body = body
            .child(col().w(0.0).grow(1.0).min_w(0.0).child(wired(s, k, (16, 28), true, false)))
            .child(
                col()
                    .key("composer-preview")
                    .role("region")
                    .label("Preview")
                    .w(0.0)
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(6.0)
                    .padding(8.0)
                    .radius(k.radius.md)
                    .fill(k.c.layer_02)
                    .child(text("Preview").font(k.ty.small_medium).color(k.c.text_faint))
                    .child(Markdown::new(preview_text(s)).build(k)),
            );
    } else {
        body = body.child(wired(s, k, (4, 14), false, false));
    }
    let panel = col()
        .key("overlay-composer")
        .role("dialog")
        .label("Composer")
        .w(width)
        .radius(k.radius.lg)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_base)
        .clip_rounded(k.radius.lg)
        .on_press(|_: &mut App| {})
        .child(grip)
        .child(body);
    let panel = k.elevate(panel, &k.elevation.overlay).absolute(x, y);
    Some(col().key("composer-layer").w_full().h_full().pt(super::TOOLBAR_HEIGHT).child(panel))
}

/// The composer floated beside the selection it was opened from: the inline composer in a raised
/// card the grip drags, kept inside the window.
fn card(s: &App, k: &Kit) -> El {
    let (x, y) = s.composer_ui.long_position();
    let (x, y) = card_inside(x, y, s.window_width);
    let grip = row()
        .key("composer-grip")
        .role("toolbar")
        .label("Composer")
        .help("Drag to move · Ctrl+1 comment · Ctrl+2 ask")
        .w_full()
        .h(32.0)
        .px(12.0)
        .center_y()
        .child(
            text(card_title(s))
                .key("composer-title")
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .truncate()
                .grow(1.0)
                .shrink(),
        )
        .on_drag(|s: &mut App, d: &mut rune::Drag, _: &mut Tx| s.drag_long_editor(d.phase, d.dx, d.dy));
    let panel = col()
        .key("overlay-composer")
        .role("dialog")
        .label("Composer")
        .w(CARD_W)
        .clip_rounded(k.radius.xl)
        .on_press(|_: &mut App| {})
        .child(grip)
        .child(
            col()
                .key("composer-card")
                .w_full()
                .px(12.0)
                .pb(12.0)
                .child(wired(s, k, (4, 14), false, true)),
        );
    let panel = rune_kit::popover::raised(k, panel, k.radius.xl).absolute(x, y);
    col().key("composer-layer").w_full().h_full().child(panel)
}

/// The card's width, and the height it is placed for before it has been laid out.
pub(super) const CARD_W: f32 = PANEL_W;
pub(super) const CARD_H: f32 = 260.0;

/// `(x, y)` moved so a card there stays inside a window `width` wide and below the toolbar band.
fn card_inside(x: f32, y: f32, width: f32) -> (f32, f32) {
    let right = if width > 0.0 { (width - CARD_W - 8.0).max(8.0) } else { x };
    (x.clamp(8.0, right.max(8.0)), y.max(super::TOOLBAR_HEIGHT + 4.0))
}

fn preview_text(s: &App) -> String {
    let t = s.composer_edit.text.trim();
    if t.is_empty() {
        return "_Nothing to preview yet._".to_owned();
    }
    let snap = s
        .composer
        .as_ref()
        .and_then(|c| {
            s.diff
                .files()
                .get(c.file)
                .and_then(|f| rain_comments::ops::anchor_text(f, &c.anchor))
        })
        .map(|content| rain_core::CodeSnapshot {
            content,
            ..rain_core::CodeSnapshot::default()
        });
    rain_comments::ui::display_body(t, snap.as_ref())
}
