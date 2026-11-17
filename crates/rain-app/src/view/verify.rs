//! The Verify by hand checklist floating at the reader's bottom left while reading the diff:
//! tick an item to strike it; the head folds it to a pill with the count.

use rain_core::command::Command;
use rune::{Cursor, El, col, el, row, text};
use rune_kit::Kit;

use super::icons;
use super::ui::sr;
use crate::app::App;
use crate::theme::{TYPE, radius};

const WIDTH: f32 = 320.0;

pub(super) fn card(s: &App, k: &Kit) -> Option<El> {
    let items = s.verify_items();
    if items.is_empty() {
        return None;
    }
    let (surf, r) = sr(k);
    let open = !s.brief.verify_shut;
    let done = items.iter().filter(|i| s.verify_done(i)).count();
    let head = row()
        .key("verify-head")
        .role("button")
        .label(format!("Verify by hand, {done} of {} done", items.len()))
        .expanded(open)
        .h(34.0)
        .px(12.0)
        .gap(8.0)
        .items_center()
        .cursor(Cursor::Pointer)
        .focusable(true)
        .help(format!(
            "{}  {}",
            if open { "Fold to a pill" } else { "Open the checklist" },
            Command::ToggleVerify.chord().map(|c| c.display()).unwrap_or_default()
        ))
        .on_press(|s: &mut App| s.toggle_verify_card())
        .child(icons::draw(&icons::CHECK, 14.0, r.accent))
        .child(text("Verify by hand").font(TYPE.label).color(k.c.text_base).a11y_hidden())
        .child(
            text(format!("{done}/{}", items.len()))
                .font(TYPE.small.features("'tnum' 1"))
                .color(if done == items.len() { r.accent } else { k.c.text_faint })
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
        let ticked = s.verify_done(item);
        let what = item.clone();
        let mut label = text(item.clone())
            .font(TYPE.small)
            .color(if ticked { k.c.text_faint } else { k.c.text_base })
            .wrap()
            .grow(1.0)
            .shrink()
            .min_w(0.0)
            .a11y_hidden();
        if ticked {
            label = label.strike(0, item.len(), None);
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
        list = list.child(
            k.interactive(
                row()
                    .key(format!("verify-{i}"))
                    .role("checkbox")
                    .label(item.clone())
                    .checked(ticked)
                    .w_full()
                    .px(12.0)
                    .py(6.0)
                    .gap(10.0)
                    .items_start()
                    .cursor(Cursor::Pointer)
                    .focusable(true)
                    .on_press(move |s: &mut App| s.toggle_verified(&what))
                    .child(mark.mt(1.0).a11y_hidden())
                    .child(label),
            ),
        );
    }
    Some(c.child(list))
}
