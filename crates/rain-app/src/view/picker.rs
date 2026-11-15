//! The `Ctrl+P` picker panel: fuzzy rows on the left, the selected file's first hunk on the right.

use rain_core::LineKind;
use rune::{El, Key, KeyEvent, ScrollSpec, Tx, col, row, text};
use rune_kit::{Kit, TextInput};

use super::board::group_color;
use crate::app::App;
use crate::app::picker::Target;

/// Lines of the first hunk the preview shows.
const PREVIEW_LINES: usize = 14;

fn picker_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        Key::Down => {
            s.picker_move(true);
            true
        }
        Key::Up => {
            s.picker_move(false);
            true
        }
        Key::Enter => {
            s.picker_go(ev.mods.ctrl || ev.mods.cmd, tx);
            true
        }
        Key::Escape => {
            s.close_picker();
            true
        }
        _ => false,
    }
}

/// The overlay's panel.
pub(super) fn picker(s: &App, k: &Kit) -> Option<El> {
    let rows = s.picker_rows();
    let sel = s.picker.sel.min(rows.len().saturating_sub(1));
    let mut list = col().key("picker-rows").w_full().gap(1.0);
    for (i, e) in rows.iter().enumerate() {
        let mut r = row()
            .key(format!("pick-{i}"))
            .role("option")
            .label(format!("{} {}", e.kind, e.label))
            .w_full()
            .px(8.0)
            .py(4.0)
            .gap(8.0)
            .items_center()
            .radius(k.radius.sm)
            .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                s.picker.sel = i;
                s.picker_go(false, tx);
            });
        let (glyph, color) = match &e.target {
            Target::File(g) => {
                let gr = s.group_of(*g);
                (crate::app::board::core_group(gr).glyph().to_owned(), group_color(gr, k))
            }
            Target::Group(_) => ("≡".into(), k.c.text_faint),
            Target::Brief(_) => ("◆".into(), k.c.icon_accent),
            Target::Node(_) => ("▦".into(), k.c.icon_accent),
            Target::Step(_) => ("▶".into(), k.c.icon_accent),
            Target::Thread(_) => ("●".into(), k.c.warning.icon),
        };
        r = r
            .child(text(glyph).font(k.ty.small.mono()).color(color).w(26.0))
            .child(text(e.label.clone()).font(k.ty.ui).color(k.c.text_base).truncate().shrink())
            .child(
                text(e.detail.clone())
                    .font(k.ty.small)
                    .color(k.c.text_faint)
                    .truncate()
                    .grow(1.0)
                    .shrink()
                    .min_w(0.0),
            );
        if let Target::File(g) = e.target {
            let f = &s.diff.files()[g];
            let viewed = s.is_viewed(g);
            r = r
                .child(
                    text(format!("+{} −{}", f.additions, f.deletions))
                        .font(k.ty.small.mono())
                        .color(k.c.text_muted),
                )
                .child(
                    text(if viewed { "✓" } else { "○" })
                        .font(k.ty.small)
                        .color(if viewed { k.c.success.icon } else { k.c.icon_faint }),
                );
        } else {
            r = r.child(text(e.kind).font(k.ty.small).color(k.c.text_faint));
        }
        if i == sel {
            r = r.fill(k.c.layer_03).checked(true);
        }
        list = list.child(r);
    }
    if rows.is_empty() {
        list = list.child(text("Nothing matches").font(k.ty.small).color(k.c.text_muted).px(8.0).py(6.0));
    }
    let preview = rows.get(sel).map(|e| preview(s, &e.target, k)).unwrap_or_else(col);
    let panel = col()
        .key("picker")
        .role("dialog")
        .label("Go to file")
        .w(1000.0)
        .max_w(1400.0)
        .h(520.0)
        .gap(6.0)
        .padding(8.0)
        .radius(k.radius.lg)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_base)
        .on_press(|_: &mut App| {})
        .child(
            TextInput::new(&s.picker.edit)
                .placeholder("Go to a file, group, Brief section, Map node, step or thread")
                .label("Go to")
                .width(None)
                .on_edit(|s: &mut App| &mut s.picker.edit)
                .on_change(|s: &mut App, _: &mut Tx| s.picker.sel = 0)
                .on_key(picker_key)
                .build(k)
                .key("picker-input"),
        )
        .child(
            row()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .gap(8.0)
                .child(
                    col()
                        .key("picker-list")
                        .w(680.0)
                        .h_full()
                        .scroll(ScrollSpec { stick_bottom: false })
                        .child(list),
                )
                .child(preview),
        )
        .child(
            text("Enter go · Ctrl+Enter go and stay zoomed · ↑↓ choose · Esc close")
                .font(k.ty.small)
                .color(k.c.text_faint),
        );
    Some(col().w_full().items_center().pt(24.0).child(k.elevate(panel, &k.elevation.overlay)))
}

/// The selected file's first hunk, or the entry's detail.
fn preview(s: &App, t: &Target, k: &Kit) -> El {
    let pane = col()
        .key("picker-preview")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .padding(8.0)
        .gap(1.0)
        .radius(k.radius.md)
        .fill(k.c.bg_deep);
    let Target::File(g) = *t else {
        return pane.child(text("No preview").font(k.ty.small).color(k.c.text_faint));
    };
    let f = &s.diff.files()[g];
    let mut pane = pane.child(text(s.keys[g].clone()).font(k.ty.small_medium).color(k.c.text_base).truncate());
    if let Some(i) = s.item(g) {
        pane = pane.child(
            text(format!("why: {}", i.reason.text))
                .font(k.ty.small)
                .color(k.c.text_muted)
                .truncate(),
        );
    }
    let start = f
        .lines
        .iter()
        .position(|l| l.kind != LineKind::Context)
        .unwrap_or(0)
        .saturating_sub(3);
    for i in start..(start + PREVIEW_LINES).min(f.lines.len()) {
        let l = &f.lines[i];
        let (mark, color) = match l.kind {
            LineKind::Added => ("+", k.c.success.fg),
            LineKind::Removed => ("-", k.c.danger.fg),
            _ => (" ", k.c.text_muted),
        };
        let n = if l.new != 0 { l.new } else { l.old };
        pane = pane.child(
            text(format!("{n:>5} {mark} {}", f.line_text(i as u32)))
                .font(crate::CODE)
                .color(color)
                .truncate(),
        );
    }
    pane
}
