//! The find bar across the top of the diff: the query, `2 of 6`, previous and next, the keys,
//! and close.

use rune::{El, Key, KeyEvent, Tx, col, el, row, text};
use rune_kit::{Kit, TextInput};

use super::icons;
use super::ui::{Activate, Ghost, sr};
use crate::app::App;
use crate::theme::TYPE;

pub(super) fn find_bar(s: &App, k: &Kit) -> El {
    let (surf, _) = sr(k);
    let count = if s.find.query.is_empty() {
        String::new()
    } else if s.find.hits.is_empty() {
        "no matches".into()
    } else {
        format!("{} of {}", (s.find.at + 1).min(s.find.hits.len()), s.find.hits.len())
    };
    let ink = if !s.find.query.is_empty() && s.find.hits.is_empty() {
        k.c.danger.fg
    } else {
        k.c.text_muted
    };
    row()
        .key("find")
        .w_full()
        .h(40.0)
        .pl(10.0)
        .pr(8.0)
        .gap(8.0)
        .items_center()
        .fill(surf.card_head)
        .child(
            col().w(360.0).child(
                TextInput::new(&s.find.edit)
                    .placeholder("Find in diff")
                    .label("Find")
                    .leading_icon(&icons::SEARCH)
                    .width(None)
                    .on_edit(|s: &mut App| &mut s.find.edit)
                    .on_change(|s: &mut App, _: &mut Tx| {
                        if s.active().files().iter().map(|f| f.text.len()).sum::<usize>() < 8 << 20 {
                            s.run_find();
                        }
                    })
                    .on_key(|s: &mut App, ev: &KeyEvent, tx: &mut Tx| match ev.key {
                        Key::Enter => {
                            s.find_step(!ev.mods.shift, tx);
                            true
                        }
                        Key::Escape => {
                            s.close_find();
                            true
                        }
                        _ => false,
                    })
                    .build(k),
            ),
        )
        .child(text(count).key("find-count").font(TYPE.small).color(ink).min_w(64.0))
        .child(
            Ghost::new("find-prev")
                .icon(&icons::CHEV_U)
                .help("Previous match  Shift+Enter")
                .height(24.0)
                .build(k)
                .on_activate_tx(|s: &mut App, tx: &mut Tx| s.find_step(false, tx)),
        )
        .child(
            Ghost::new("find-next")
                .icon(&icons::CHEV_D)
                .help("Next match  Enter")
                .height(24.0)
                .build(k)
                .on_activate_tx(|s: &mut App, tx: &mut Tx| s.find_step(true, tx)),
        )
        .child(el().grow(1.0).a11y_hidden())
        .child(text("Enter next · Shift+Enter previous").font(TYPE.small).color(k.c.text_faint))
        .child(
            Ghost::new("find-close")
                .icon(&icons::CLOSE)
                .help("Close find  Esc")
                .height(24.0)
                .build(k)
                .on_activate(|s: &mut App| s.close_find()),
        )
        .child(
            el().pin(Some(0.0), None, Some(0.0), Some(0.0))
                .h(1.0)
                .fill(surf.hairline)
                .a11y_hidden(),
        )
}
