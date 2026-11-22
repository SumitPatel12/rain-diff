//! The `Ctrl+P` picker panel: fuzzy rows on the left, the selected file's first hunk on the right.

use rain_core::LineKind;
use rune::{Cursor, Drag, El, Key, KeyEvent, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Kit, TextInput};

use super::icons;
use super::ui::{group_tag, matched, sr};
use crate::app::App;
use crate::app::picker::{PICKER_TOP, Target};
use crate::theme::{TYPE, radius};

/// Lines the preview holds from the first change on; it scrolls through them both ways.
const PREVIEW_LINES: usize = 120;
/// The list's width, at most; it gives way to the preview in a narrow panel.
const LIST_W: f32 = 400.0;
/// How far in from the panel's edges a resize drag starts.
const GRIP: f32 = 6.0;

fn picker_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        Key::Down => {
            s.picker_move(true, tx);
            true
        }
        Key::Up => {
            s.picker_move(false, tx);
            true
        }
        Key::Enter => {
            s.picker_go(ev.mods.ctrl || ev.mods.cmd, tx);
            true
        }
        // Vim: Esc leaves the query for normal mode (`App::picker_normal_key`).
        Key::Escape if s.keymap_profile().vim_motions() => {
            s.picker.edit.focused = false;
            true
        }
        Key::Escape => {
            s.close_picker();
            true
        }
        _ => false,
    }
}

/// A drag strip on the panel's right edge, bottom edge or bottom-right corner.
fn resize_grip(key: &'static str, label: &'static str, cursor: Cursor, x: bool, y: bool) -> El {
    let e = el()
        .key(key)
        .role("separator")
        .label(label)
        .cursor(cursor)
        .on_drag(move |s: &mut App, d: &mut Drag, tx: &mut Tx| {
            tx.instant();
            let (dx, dy) = (if x { d.dx } else { 0.0 }, if y { d.dy } else { 0.0 });
            s.drag_picker(d.phase, dx, dy, d.cancelled());
        });
    match (x, y) {
        (true, false) => e.pin(None, Some(0.0), Some(0.0), Some(GRIP)).w(GRIP),
        (false, true) => e.pin(Some(0.0), None, Some(GRIP), Some(0.0)).h(GRIP),
        _ => e.pin(None, None, Some(0.0), Some(0.0)).size(GRIP * 2.0, GRIP * 2.0),
    }
}

/// The overlay's panel: the query, the rows at the left, the selected row's preview at the
/// right, and the keys at its foot.
pub(super) fn picker(s: &App, k: &Kit) -> Option<El> {
    let (surf, r) = sr(k);
    let mut rows = s.picker_rows();
    let marks = s.picker_marks();
    let folder_marks = s.picker_folder_marks();
    let previous = s.picker_leads_with_previous();
    let sel = s.picker.sel.min(rows.len().saturating_sub(1));
    let mut list = col().key("picker-rows").w_full().px(6.0).py(6.0);
    for (i, e) in rows.iter_mut().enumerate() {
        let mut line = row()
            .key(format!("pick-{i}"))
            .role("option")
            .label(format!("{} {}", e.kind, e.label))
            .w_full()
            .h(32.0)
            .px(8.0)
            .gap(8.0)
            .items_center()
            .radius(radius::BUTTON)
            .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                s.picker.sel = i;
                s.picker_go(false, tx);
            });
        let glyph = match &e.target {
            Target::File(_) => &icons::FILE,
            Target::Group(_) => &icons::TREE,
            Target::Brief(_) => &icons::BOOK,
            Target::Node(_) => &icons::MAP,
            Target::Step(_) => &icons::TOUR,
            Target::Thread(_) => &icons::CHAT,
        };
        line = line
            .child(match e.target {
                Target::File(g) => s.file_icons.file(s.keys[g].rsplit('/').next().unwrap_or_default(), 14.0),
                _ => icons::draw(glyph, 14.0, k.c.text_muted),
            })
            .child(matched(&e.label, marks.get(i).map_or(&[], Vec::as_slice), k).truncate().shrink());
        if let Target::File(g) = e.target {
            let f = &s.diff.files()[g];
            let viewed = s.is_viewed(g);
            // The folder takes the room the name leaves, so it is the one cut, at its far end.
            line = line.child(if e.detail.is_empty() {
                el().grow(1.0).a11y_hidden()
            } else {
                matched(std::mem::take(&mut e.detail), folder_marks.get(i).map_or(&[], Vec::as_slice), k)
                    .font(TYPE.small)
                    .color(k.c.text_muted)
                    .truncate_head()
                    .w(0.0)
                    .grow(1.0)
                    .min_w(0.0)
            });
            if i == 0 && previous {
                line = line.child(text("Previous").font(TYPE.micro).color(k.c.text_faint));
            }
            line = line.child(group_tag(s.group_of(g), k));
            line = line
                .child(text(format!("+{}", f.additions)).font(TYPE.micro_mono).color(k.c.success.fg))
                .child(text(format!("−{}", f.deletions)).font(TYPE.micro_mono).color(k.c.danger.fg))
                .child(if viewed {
                    icons::draw(&icons::CHECK, 14.0, r.accent)
                } else {
                    el().size(9.0, 9.0).radius(4.5).border(1.5, k.c.icon_faint).a11y_hidden()
                });
        } else {
            let ink = if matches!(e.target, Target::Thread(_)) {
                k.c.warning.fg
            } else {
                k.c.text_faint
            };
            line = line
                .child(el().grow(1.0).a11y_hidden())
                .child(text(e.kind).font(TYPE.micro).color(ink));
        }
        line = if i == sel {
            line.fill(surf.selected_row).checked(true)
        } else {
            k.interactive(line)
        };
        list = list.child(line);
    }
    if rows.is_empty() {
        list = list.child(text("Nothing matches").font(TYPE.small).color(k.c.text_muted).px(8.0).py(6.0));
    }
    let (w, h) = s.picker_size();
    let list_w = LIST_W.min(w * 0.45).floor();
    let preview = rows
        .get(sel)
        .map(|e| preview(s, &e.target, w - list_w - 1.0, k))
        .unwrap_or_else(col);
    let input = row()
        .key("picker-input-row")
        .w_full()
        .h(48.0)
        .px(14.0)
        .gap(10.0)
        .items_center()
        .child(icons::draw(&icons::SEARCH, 16.0, k.c.text_muted))
        .child(
            col().grow(1.0).min_w(0.0).child(
                TextInput::new(&s.picker.edit)
                    .placeholder("Go to a file, group, Brief section, Map node, step or thread")
                    .label("Go to")
                    .width(None)
                    .plain()
                    .on_edit(|s: &mut App| &mut s.picker.edit)
                    .on_change(|s: &mut App, tx: &mut Tx| s.picker_select(0, tx))
                    .on_key(picker_key)
                    .build(k)
                    .key("picker-input"),
            ),
        );
    let vim = s.keymap_profile().vim_motions();
    let normal = s.picker_normal();
    let hint = |t: &'static str| text(t).font(TYPE.small).color(k.c.text_faint);
    let mut foot = row()
        .key("picker-foot")
        .w_full()
        .h(34.0)
        .px(14.0)
        .gap(16.0)
        .items_center()
        .fill(surf.card_head);
    if vim {
        foot = foot.child(
            text(if normal { "NORMAL" } else { "INSERT" })
                .key("picker-mode")
                .font(TYPE.micro_mono)
                .color(if normal { r.accent } else { k.c.text_muted }),
        );
    }
    foot = if normal {
        foot.child(hint("j/k move"))
            .child(hint("Enter go"))
            .child(hint("i search").grow(1.0))
            .child(hint("Esc close"))
    } else {
        foot.child(hint("↑↓ move"))
            .child(hint("Enter go"))
            .child(hint("Ctrl+Enter go and zoom").grow(1.0))
            .child(hint(if vim { "Esc normal mode" } else { "Esc close" }))
    };
    let panel = col()
        .key("picker")
        .role("dialog")
        .label("Go to file")
        .w(w)
        .h(h)
        .radius(radius::ISLAND)
        .fill(r.raised)
        .border(1.0, surf.card_border)
        .shadow(24.0, 64.0, r.shadow)
        .clip_rounded(radius::ISLAND)
        .on_press(|_: &mut App| {})
        .child(input)
        .child(el().w_full().h(1.0).fill(surf.hairline))
        .child(
            row()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(
                    col()
                        .key("picker-list")
                        .w(list_w)
                        .h_full()
                        .scroll(ScrollSpec { stick_bottom: false })
                        .child(list),
                )
                .child(el().w(1.0).h_full().fill(surf.hairline).a11y_hidden())
                .child(preview),
        )
        .child(el().w_full().h(1.0).fill(surf.hairline))
        .child(foot)
        .child(resize_grip(
            "picker-grip-right",
            "Resize the picker's width",
            Cursor::ResizeColumn,
            true,
            false,
        ))
        .child(resize_grip(
            "picker-grip-bottom",
            "Resize the picker's height",
            Cursor::ResizeRow,
            false,
            true,
        ))
        .child(resize_grip(
            "picker-grip-corner",
            "Resize the picker",
            Cursor::Crosshair,
            true,
            true,
        ));
    Some(col().w_full().items_center().pt(PICKER_TOP).child(panel))
}

/// The selected file's first hunk as the diff draws it, or a note that there is none.
fn preview(s: &App, t: &Target, width: f32, k: &Kit) -> El {
    let (surf, r) = sr(k);
    let pane = col().key("picker-preview").w(0.0).grow(1.0).min_w(0.0).h_full().fill(surf.reader);
    let Target::File(g) = *t else {
        return pane.child(text("No preview").font(TYPE.small).color(k.c.text_faint).padding(14.0));
    };
    let f = &s.diff.files()[g];
    let key = &s.keys[g];
    let (dir, name) = key.rsplit_once('/').map_or(("", key.as_str()), |(d, n)| (d, n));
    let dir = if dir.is_empty() {
        String::new()
    } else {
        format!("{}/", dir.replacen('/', " · ", 1))
    };
    let start = f
        .lines
        .iter()
        .position(|l| l.kind != LineKind::Context)
        .unwrap_or(0)
        .saturating_sub(3);
    let hunk = f
        .hunks
        .iter()
        .find(|h| (h.lines.start as usize) <= start + 3 && (start + 3) < h.lines.end as usize)
        .or(f.hunks.first())
        .map(|h| rain_diffview::hunk_header(f, h))
        .unwrap_or_default();
    let pane = pane.child(
        row()
            .w_full()
            .h(34.0)
            .px(12.0)
            .gap(0.0)
            .items_center()
            .child(text(dir).font(TYPE.small).color(k.c.text_muted).truncate_head().shrink())
            .child(text(name.to_owned()).font(TYPE.label).color(k.c.text_base))
            .child(el().grow(1.0).a11y_hidden())
            .child(
                text(format!("first hunk · {hunk}"))
                    .font(TYPE.micro)
                    .color(k.c.text_faint)
                    .truncate()
                    .shrink(),
            ),
    );
    let num = rune::tok::TextStyle::new(12.0, 400.0).mono().lh(20.0);
    let mut body = col().key("picker-preview-lines").min_w(width).pb(6.0);
    for i in start..(start + PREVIEW_LINES).min(f.lines.len()) {
        let l = &f.lines[i];
        let (sign, ink, fill) = match l.kind {
            LineKind::Added => ("+", k.c.success.fg, r.add_bg),
            LineKind::Removed => ("−", k.c.danger.fg, r.del_bg),
            _ => (" ", k.c.text_faint, surf.reader),
        };
        let n = |n: u32| {
            row().w(38.0).pr(8.0).justify_end().child(
                text(if n == 0 { String::new() } else { n.to_string() })
                    .font(num)
                    .color(k.c.text_faint),
            )
        };
        body = body.child(
            row()
                .h(20.0)
                .pr(16.0)
                .fill(fill)
                .child(n(l.old))
                .child(n(l.new))
                .child(text(sign).font(crate::CODE).color(ink).w(18.0))
                .child(
                    text(rain_diffview::drawn_part(f.line_text(i as u32)).replace('\t', "    "))
                        .font(crate::CODE)
                        .color(k.c.text_base)
                        .single_line(),
                ),
        );
    }
    pane.child(
        col()
            .key("picker-preview-scroll")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll_xy()
            .child(body),
    )
}
