//! The command palette overlay (`Ctrl+K`).

use rune::{El, Key, KeyEvent, ScrollSpec, Tx, col, row, text};
use rune_kit::{Kit, TextInput};

use crate::app::App;
use crate::palette;

fn palette_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let n = palette::filtered(s, &s.palette.edit.text).len();
    match ev.key {
        Key::Down => {
            s.palette.sel = (s.palette.sel + 1).min(n.saturating_sub(1));
            true
        }
        Key::Up => {
            s.palette.sel = s.palette.sel.saturating_sub(1);
            true
        }
        Key::Enter => {
            palette::run_selected(s, tx);
            true
        }
        Key::Escape => {
            s.close_palette();
            true
        }
        _ => false,
    }
}

pub(super) fn palette_overlay(s: &App, k: &Kit) -> El {
    let rows = palette::filtered(s, &s.palette.edit.text);
    let sel = s.palette.sel.min(rows.len().saturating_sub(1));
    let mut list = col().w_full().gap(1.0);
    for (i, e) in rows.iter().enumerate() {
        let mut r = row()
            .key(format!("pal-{i}"))
            .role("option")
            .label(e.label.clone())
            .w_full()
            .px(10.0)
            .py(5.0)
            .gap(10.0)
            .items_center()
            .radius(k.radius.sm)
            .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                s.palette.sel = i;
                palette::run_selected(s, tx);
            })
            .child(text(e.kind).font(k.ty.small).color(k.c.text_faint).w(64.0))
            .child(
                text(e.label.clone())
                    .font(k.ty.ui)
                    .color(k.c.text_base)
                    .grow(1.0)
                    .shrink()
                    .truncate(),
            )
            .child(text(e.hint.clone()).font(k.ty.small).color(k.c.text_muted));
        if i == sel {
            r = r.fill(k.c.layer_03).checked(true);
        }
        list = list.child(r);
    }
    let panel = col()
        .key("palette")
        .role("dialog")
        .label("Command palette")
        .w(680.0)
        .max_h(560.0)
        .gap(6.0)
        .padding(8.0)
        .radius(k.radius.lg)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_base)
        .on_press(|_: &mut App| {})
        .child(
            TextInput::new(&s.palette.edit)
                .placeholder("Type a command, file, chapter or thread")
                .label("Command palette")
                .width(None)
                .on_edit(|s: &mut App| &mut s.palette.edit)
                .on_change(|s: &mut App, _: &mut Tx| s.palette.sel = 0)
                .on_key(palette_key)
                .build(k),
        )
        .child(
            col()
                .key("palette-rows")
                .w_full()
                .h((rows.len() as f32 * 31.0).clamp(31.0, 470.0))
                .scroll(ScrollSpec { stick_bottom: false })
                .child(list),
        );
    let panel = k.elevate(panel, &k.elevation.overlay);
    col()
        .key("palette-scrim")
        .w_full()
        .h_full()
        .items_center()
        .pt(72.0)
        .fill(k.c.scrim)
        .on_press(|s: &mut App| s.close_palette())
        .child(panel)
}
