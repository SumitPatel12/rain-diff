//! The Verify by hand checklist floating over the window while reading the diff: folded to its
//! head (`2 of 5 done`) until opened; tick an item to strike it, edit or remove any item, add your
//! own; drag the head to move the card, double-click it to put it back.

use rain_core::command::Command;
use rain_core::verify::ItemSource;
use rune::{Cursor, Drag, El, Key, KeyEvent, Tx, col, el, row, text};
use rune_kit::{Kit, TextInput};

use super::icons;
use super::ui::{Activate, Ghost, sr};
use crate::app::App;
use crate::theme::{TYPE, radius};

const WIDTH: f32 = 320.0;

/// The card at the window's top-left, moved to its place by its live offset.
pub(super) fn card(s: &App, k: &Kit) -> Option<El> {
    if !s.verify_shown() {
        return None;
    }
    s.verify_card.place(s.mirror.verify.at);
    let items = s.verify_items();
    let (surf, r) = sr(k);
    let open = s.verify_open();
    let done = items.iter().filter(|i| s.verify_done(&i.text)).count();
    let count = format!("{done} of {} done", items.len());
    let head = row()
        .key("verify-head")
        .role("button")
        .label(format!("Verify by hand, {count}"))
        .expanded(open)
        .h(34.0)
        .px(12.0)
        .gap(8.0)
        .items_center()
        .cursor(Cursor::Grab)
        .focusable(true)
        .help(format!(
            "{}  {} · drag to move, double-click to put back",
            if open { "Fold" } else { "Open the checklist" },
            Command::ToggleVerify.chord().map(|c| c.display()).unwrap_or_default()
        ))
        .on_activate(|s: &mut App| s.press_verify_head())
        .on_double_press(|s: &mut App| s.reset_verify_position())
        .on_drag(|s: &mut App, d: &mut Drag, tx: &mut Tx| s.verify_drag(d, tx))
        .child(icons::draw(&icons::CHECK, 14.0, r.accent))
        .child(text("Verify by hand").font(TYPE.label).color(k.c.text_base).a11y_hidden())
        .child(
            text(count)
                .key("verify-count")
                .font(TYPE.small.features("'tnum' 1"))
                .color(if done == items.len() && done > 0 {
                    r.accent
                } else {
                    k.c.text_faint
                })
                .a11y_hidden(),
        )
        .child(el().grow(1.0).a11y_hidden())
        .child(icons::draw(
            if open { &icons::CHEV_D } else { &icons::CHEV_U },
            14.0,
            k.c.text_muted,
        ));
    let mut c = col()
        .key("verify")
        .role("region")
        .label("Verify by hand")
        .absolute(0.0, 0.0)
        .live_offset(&s.verify_card.pos)
        .on_layout_tx(|s: &mut App, r: [f32; 4], tx: &mut Tx| s.set_verify_size(r, tx))
        .radius(radius::CARD)
        .fill(r.raised)
        .border(1.0, surf.card_border)
        .shadow(12.0, 32.0, r.shadow)
        .clip_rounded(radius::CARD)
        .on_press(|_: &mut App| {})
        .child(k.interactive(head));
    if !open {
        return Some(c);
    }
    c = c.w(WIDTH).child(el().w_full().h(1.0).fill(surf.hairline).a11y_hidden());
    let mut list = col()
        .key("verify-items")
        .w_full()
        .max_h(320.0)
        .scroll(rune::ScrollSpec { stick_bottom: false })
        .py(4.0);
    for (i, item) in items.iter().enumerate() {
        if s.verify_card.editing.as_ref() == Some(&item.of) {
            list = list.child(edit_row(s, k));
            continue;
        }
        list = list.child(item_row(s, k, i, item));
    }
    if items.is_empty() {
        list = list.child(
            text("No checks left. Add one below.")
                .font(TYPE.small)
                .color(k.c.text_faint)
                .px(12.0)
                .py(6.0),
        );
    }
    let add = TextInput::new(&s.verify_card.add)
        .placeholder("Add a check")
        .label("Add a check")
        .leading_icon(&icons::PLUS)
        .width(None)
        .height(26.0)
        .on_edit(|s: &mut App| &mut s.verify_card.add)
        .on_key(|s: &mut App, ev: &KeyEvent, _: &mut Tx| match ev.key {
            Key::Enter => {
                s.add_verify_item();
                true
            }
            Key::Escape => {
                s.leave_verify_add();
                true
            }
            _ => false,
        })
        .build(k);
    let mut foot = row()
        .w_full()
        .px(8.0)
        .py(8.0)
        .gap(6.0)
        .items_center()
        .child(col().key("verify-add").grow(1.0).min_w(0.0).child(add));
    if s.mirror.verify.at.is_some() {
        foot = foot.child(
            Ghost::new("verify-reset")
                .icon(&icons::RELOAD)
                .help("Reset position")
                .height(26.0)
                .build(k)
                .on_activate(|s: &mut App| s.reset_verify_position()),
        );
    }
    Some(
        c.child(list)
            .child(el().w_full().h(1.0).fill(surf.hairline).a11y_hidden())
            .child(foot),
    )
}

fn item_row(s: &App, k: &Kit, i: usize, item: &rain_core::verify::VerifyItem) -> El {
    let (_, r) = sr(k);
    let ticked = s.verify_done(&item.text);
    let mut label = text(item.text.clone())
        .font(TYPE.small)
        .color(if ticked { k.c.text_faint } else { k.c.text_base })
        .wrap()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .a11y_hidden();
    if ticked {
        label = label.strike(0, item.text.len(), None);
    }
    let mark = if ticked {
        row()
            .size(16.0, 16.0)
            .center()
            .radius(radius::KEY)
            .fill(r.accent)
            .child(icons::draw(&icons::CHECK, 12.0, r.on_accent))
    } else {
        row().size(16.0, 16.0).radius(radius::KEY).border(1.5, k.c.text_muted)
    };
    let mine = match item.source {
        ItemSource::Brief => "",
        ItemSource::Edited => ", edited",
        ItemSource::Added => ", yours",
    };
    let (what, of2) = (item.text.clone(), item.of.clone());
    let check = k.interactive(
        row()
            .key(format!("verify-{i}"))
            .role("checkbox")
            .label(format!("{}{mine}", item.text))
            .checked(ticked)
            .grow(1.0)
            .min_w(0.0)
            .pl(12.0)
            .py(6.0)
            .gap(10.0)
            .items_start()
            .cursor(Cursor::Pointer)
            .focusable(true)
            .on_press(move |s: &mut App| s.toggle_verified(&what))
            .child(mark.mt(1.0).a11y_hidden())
            .child(label),
    );
    let (of3, words2) = (item.of.clone(), item.text.clone());
    row()
        .key(format!("verify-row-{i}"))
        .w_full()
        .pr(6.0)
        .gap(2.0)
        .items_start()
        .child(check)
        .child(
            Ghost::new(format!("verify-edit-{i}"))
                .icon(&icons::EDIT)
                .help(format!("Edit “{}”", item.text))
                .height(24.0)
                .build(k)
                .mt(2.0)
                .on_activate(move |s: &mut App| s.start_verify_edit(of3.clone(), &words2)),
        )
        .child(
            Ghost::new(format!("verify-remove-{i}"))
                .icon(&icons::CLOSE)
                .help(format!("Remove “{}”", item.text))
                .height(24.0)
                .build(k)
                .mt(2.0)
                .on_activate(move |s: &mut App| s.remove_verify_item(&of2)),
        )
}

fn edit_row(s: &App, k: &Kit) -> El {
    let field = TextInput::new(&s.verify_card.edit)
        .placeholder("The check")
        .label("Edit the check")
        .width(None)
        .height(26.0)
        .on_edit(|s: &mut App| &mut s.verify_card.edit)
        .on_key(|s: &mut App, ev: &KeyEvent, _: &mut Tx| match ev.key {
            Key::Enter => {
                s.save_verify_edit();
                true
            }
            Key::Escape => {
                s.cancel_verify_edit();
                true
            }
            _ => false,
        })
        .build(k);
    row()
        .key("verify-editing")
        .w_full()
        .px(8.0)
        .py(4.0)
        .child(col().key("verify-input").grow(1.0).min_w(0.0).child(field))
}
