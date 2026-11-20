//! The command palette (`Ctrl+K`): a raised card over a scrim, its query on top, rows grouped
//! under their kind (Commands, Motions, Settings, Filters, Notes, Chapters, Threads, Files) with the key
//! or place at the end, and the keys at its foot.

use rune::{El, Key, KeyEvent, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Kit, TextInput};

use super::icons;
use super::ui::sr;
use crate::app::App;
use crate::palette;
use crate::theme::{TYPE, radius};

const ROW_H: f32 = 32.0;
const LABEL_H: f32 = 26.0;

fn palette_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let n = palette::filtered(s, &s.palette.edit.text).len();
    match ev.key {
        Key::Down => {
            palette_select(s, (s.palette.sel + 1).min(n.saturating_sub(1)), tx);
            true
        }
        Key::Up => {
            palette_select(s, s.palette.sel.saturating_sub(1), tx);
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

/// Selects row `i` and scrolls the rows just enough to show it; the first row brings its group's
/// title along.
fn palette_select(s: &mut App, i: usize, tx: &mut Tx) {
    s.palette.sel = i;
    let row = if i == 0 { "pal-group-0".to_owned() } else { format!("pal-{i}") };
    tx.scroll_into_view_in("palette-rows", row);
}

/// `Command` as the group's title, `Commands`.
fn group_title(kind: &str) -> String {
    match kind {
        "File" | "Note" | "Chapter" | "Thread" | "Command" | "Setting" | "Filter" | "Motion" => format!("{kind}s"),
        k => k.to_owned(),
    }
}

pub(super) fn palette_overlay(s: &App, k: &Kit) -> El {
    let (surf, r) = sr(k);
    let rows = palette::filtered(s, &s.palette.edit.text);
    let sel = s.palette.sel.min(rows.len().saturating_sub(1));
    let mut list = col().w_full().px(6.0).pb(6.0);
    let mut height = 0.0;
    let mut last: Option<&str> = None;
    for (i, e) in rows.iter().enumerate() {
        if last != Some(e.kind) {
            list = list.child(
                text(group_title(e.kind))
                    .key(format!("pal-group-{i}"))
                    .font(TYPE.micro)
                    .color(k.c.text_faint)
                    .h(LABEL_H)
                    .px(8.0)
                    .pt(8.0),
            );
            height += LABEL_H;
            last = Some(e.kind);
        }
        let on = e.hint == "on";
        let mut line = row()
            .key(format!("pal-{i}"))
            .role("option")
            .label(e.label.clone())
            .w_full()
            .h(ROW_H)
            .px(8.0)
            .gap(10.0)
            .items_center()
            .radius(radius::BUTTON)
            .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                s.palette.sel = i;
                palette::run_selected(s, tx);
            })
            .child(text(e.label.clone()).font(TYPE.ui).color(k.c.text_base).shrink().truncate());
        if on {
            line = line.child(
                row()
                    .h(18.0)
                    .px(6.0)
                    .items_center()
                    .radius(radius::CHIP)
                    .fill(k.c.success.bg)
                    .child(text("on").font(TYPE.micro).color(k.c.success.fg)),
            );
        }
        line = line.child(el().grow(1.0).a11y_hidden());
        if !on && !e.hint.is_empty() {
            let mono = e.kind == "Command" || e.kind == "Motion";
            line = line.child(
                text(e.hint.clone())
                    .font(if mono { TYPE.micro_mono } else { TYPE.small })
                    .color(k.c.text_faint)
                    .truncate()
                    .shrink(),
            );
        }
        line = if i == sel {
            line.fill(surf.selected_row).checked(true)
        } else {
            k.interactive(line)
        };
        list = list.child(line);
        height += ROW_H;
    }
    if rows.is_empty() {
        list = list.child(text("Nothing matches").font(TYPE.small).color(k.c.text_muted).padding(12.0));
        height = 44.0;
    }
    let input = row()
        .key("palette-input")
        .w_full()
        .h(48.0)
        .px(14.0)
        .gap(10.0)
        .items_center()
        .child(icons::draw(&icons::KBD, 16.0, k.c.text_muted))
        .child(
            col().grow(1.0).min_w(0.0).child(
                TextInput::new(&s.palette.edit)
                    .placeholder("Type a command, setting, file, note or thread")
                    .label("Command palette")
                    .width(None)
                    .plain()
                    .on_edit(|s: &mut App| &mut s.palette.edit)
                    .on_change(|s: &mut App, tx: &mut Tx| palette_select(s, 0, tx))
                    .on_key(palette_key)
                    .build(k),
            ),
        );
    let foot = row()
        .key("palette-foot")
        .w_full()
        .h(34.0)
        .px(14.0)
        .gap(16.0)
        .items_center()
        .fill(surf.card_head)
        .child(text("↑↓ move").font(TYPE.small).color(k.c.text_faint))
        .child(text("Enter run").font(TYPE.small).color(k.c.text_faint))
        .child(
            text("Settings show on when active")
                .font(TYPE.small)
                .color(k.c.text_faint)
                .grow(1.0),
        )
        .child(text("Esc close").font(TYPE.small).color(k.c.text_faint));
    let panel = col()
        .key("palette")
        .role("dialog")
        .label("Command palette")
        .w(640.0)
        .max_h(560.0)
        .radius(radius::ISLAND)
        .fill(r.raised)
        .border(1.0, surf.card_border)
        .shadow(24.0, 64.0, r.shadow)
        .clip_rounded(radius::ISLAND)
        .on_press(|_: &mut App| {})
        .child(input)
        .child(el().w_full().h(1.0).fill(surf.hairline))
        .child(
            col()
                .key("palette-rows")
                .w_full()
                .h(height.clamp(44.0, 420.0))
                .scroll(ScrollSpec { stick_bottom: false })
                .child(list),
        )
        .child(el().w_full().h(1.0).fill(surf.hairline))
        .child(foot);
    col()
        .key("palette-scrim")
        .w_full()
        .h_full()
        .items_center()
        .pt(78.0)
        .fill(k.c.scrim)
        .on_press(|s: &mut App| s.close_palette())
        .child(panel)
}
