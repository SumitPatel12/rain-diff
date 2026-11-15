//! The find bar over the diff.

use rune::{El, Key, KeyEvent, Tx, col, row};
use rune_kit::{Kit, TextInput};

use super::muted;
use crate::app::App;

pub(super) fn find_bar(s: &App, k: &Kit) -> El {
    let count = if s.find.query.is_empty() {
        String::new()
    } else if s.find.hits.is_empty() {
        "no matches".into()
    } else {
        format!("{} of {}", (s.find.at + 1).min(s.find.hits.len()), s.find.hits.len())
    };
    row()
        .key("find")
        .w_full()
        .px(12.0)
        .py(6.0)
        .gap(8.0)
        .items_center()
        .fill(k.c.layer_01)
        .child(
            col().grow(1.0).child(
                TextInput::new(&s.find.edit)
                    .placeholder("Find in diff (Enter next, Shift+Enter previous, Esc close)")
                    .label("Find")
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
        .child(muted(count, k))
}
