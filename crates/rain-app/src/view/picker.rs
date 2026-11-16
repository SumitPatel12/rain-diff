//! The `Ctrl+P` picker panel: fuzzy rows on the left, the selected file's first hunk on the right.

use rain_core::LineKind;
use rune::{El, Key, KeyEvent, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Kit, TextInput};

use super::icons;
use super::ui::{group_tag, kbd, sr};
use crate::app::App;
use crate::app::picker::Target;
use crate::theme::{TYPE, radius};

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

/// The overlay's panel: the query, the rows at the left, the selected row's preview at the
/// right, and the keys at its foot.
pub(super) fn picker(s: &App, k: &Kit) -> Option<El> {
    let (surf, r) = sr(k);
    let rows = s.picker_rows();
    let sel = s.picker.sel.min(rows.len().saturating_sub(1));
    let mut list = col().key("picker-rows").w_full().px(6.0).py(6.0);
    for (i, e) in rows.iter().enumerate() {
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
            .child(icons::draw(glyph, 14.0, k.c.text_muted))
            .child(text(e.label.clone()).font(TYPE.ui).color(k.c.text_base).truncate().shrink());
        if let Target::File(g) = e.target {
            let f = &s.diff.files()[g];
            let viewed = s.is_viewed(g);
            line = line.child(group_tag(s.group_of(g), k));
            line = line
                .child(text(format!("+{}", f.additions)).font(TYPE.micro_mono).color(k.c.success.fg))
                .child(text(format!("−{}", f.deletions)).font(TYPE.micro_mono).color(k.c.danger.fg))
                .child(el().grow(1.0).a11y_hidden())
                .child(if viewed {
                    icons::draw(&icons::CHECK, 14.0, r.accent)
                } else {
                    el().size(9.0, 9.0).radius(4.5).border(1.5, k.c.text_faint).a11y_hidden()
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
    let preview = rows.get(sel).map(|e| preview(s, &e.target, k)).unwrap_or_else(col);
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
                    .on_change(|s: &mut App, _: &mut Tx| s.picker.sel = 0)
                    .on_key(picker_key)
                    .build(k)
                    .key("picker-input"),
            ),
        )
        .child(kbd("Ctrl+P", k));
    let foot = row()
        .w_full()
        .h(34.0)
        .px(14.0)
        .gap(16.0)
        .items_center()
        .fill(surf.card_head)
        .child(text("↑↓ move").font(TYPE.small).color(k.c.text_faint))
        .child(text("Enter go").font(TYPE.small).color(k.c.text_faint))
        .child(text("Ctrl+Enter go and zoom").font(TYPE.small).color(k.c.text_faint).grow(1.0))
        .child(text("Esc close").font(TYPE.small).color(k.c.text_faint));
    let panel = col()
        .key("picker")
        .role("dialog")
        .label("Go to file")
        .w(940.0)
        .h(520.0)
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
                        .w(400.0)
                        .h_full()
                        .scroll(ScrollSpec { stick_bottom: false })
                        .child(list),
                )
                .child(el().w(1.0).h_full().fill(surf.hairline).a11y_hidden())
                .child(preview),
        )
        .child(el().w_full().h(1.0).fill(surf.hairline))
        .child(foot);
    Some(col().w_full().items_center().pt(34.0).child(panel))
}

/// The selected file's first hunk as the diff draws it, or a note that there is none.
fn preview(s: &App, t: &Target, k: &Kit) -> El {
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
    let mut pane = pane.child(
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
        pane = pane.child(
            row()
                .w_full()
                .h(20.0)
                .fill(fill)
                .child(n(l.old))
                .child(n(l.new))
                .child(text(sign).font(crate::CODE).color(ink).w(18.0))
                .child(
                    text(f.line_text(i as u32).replace('\t', "    "))
                        .font(crate::CODE)
                        .color(k.c.text_base)
                        .truncate()
                        .shrink(),
                ),
        );
    }
    pane
}
