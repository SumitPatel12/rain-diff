//! The reader's tab strip, `Overview | Changes N`, merged into the reader.

use rune::{El, col, el, row, text};

use super::hairline;
use crate::app::{App, ReaderTab};
use crate::theme::Look;

const STRIP_H: f32 = 40.0;

type Tab = (ReaderTab, &'static str, String, Option<usize>);

/// The strip on the panel surface; the active tab takes the reader's fill, with hairlines at its
/// sides and none under it.
pub(super) fn strip(s: &App, look: &Look) -> El {
    let on = s.reader_tab();
    let tabs: [Tab; 2] = [
        (ReaderTab::Overview, "tab-overview", "Overview".to_owned(), None),
        (
            ReaderTab::Changes,
            "tab-changes",
            "Changes".to_owned(),
            Some(s.active().files().len()),
        ),
    ];
    let line = look.s.hairline;
    let mut r = row().key("tab-strip").role("tablist").w_full().h(STRIP_H).fill(look.s.panel);
    for t in tabs {
        let active = t.0 == on;
        let tab = tab_el(look, t, active).h(STRIP_H - 1.0);
        let cell = col().h_full().child(tab).child(hairline(if active { look.s.reader } else { line }));
        r = if active {
            r.child(el().w(1.0).h_full().fill(line).a11y_hidden())
                .child(cell.fill(look.s.reader))
                .child(el().w(1.0).h_full().fill(line).a11y_hidden())
        } else {
            r.child(cell)
        };
    }
    r.child(col().grow(1.0).h_full().justify_end().child(hairline(line)))
}

fn label(look: &Look, name: String, count: Option<usize>, on: bool) -> El {
    let k = &look.kit;
    let mut r = row()
        .gap(6.0)
        .items_center()
        .a11y_hidden()
        .child(
            text(name)
                .font(k.ty.ui_medium)
                .color(if on { k.c.text_base } else { k.c.text_muted }),
        );
    if let Some(n) = count {
        r = r.child(text(n.to_string()).font(k.ty.small.features("'tnum' 1")).color(k.c.text_faint));
    }
    r
}

fn tab_el(look: &Look, (tab, key, name, count): Tab, on: bool) -> El {
    let said = match count {
        Some(n) => format!("{name}, {n} files"),
        None => name.clone(),
    };
    look.kit
        .interactive(
            row()
                .key(key)
                .role("tab")
                .label(said)
                .checked(on)
                .px(14.0)
                .items_center()
                .on_press(move |s: &mut App| s.set_reader_tab(tab)),
        )
        .child(label(look, name, count, on))
}
