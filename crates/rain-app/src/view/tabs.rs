//! The reader's tab strip, `Overview | Changes N`: merged into the reader (Zed) or filled and
//! underlined on the reader island (Islands).

use rune::{El, col, el, row, text};

use super::hairline;
use crate::app::{App, ReaderTab};
use crate::theme::{Look, TabStyle};

const STRIP_H: f32 = 40.0;

pub(super) fn strip(s: &App, look: &Look) -> El {
    let on = s.reader_tab();
    let tabs = [
        (ReaderTab::Overview, "tab-overview", "Overview".to_owned(), None),
        (
            ReaderTab::Changes,
            "tab-changes",
            "Changes".to_owned(),
            Some(s.active().files().len()),
        ),
    ];
    match look.shape.tab_style {
        TabStyle::Merge => merged(look, on, tabs),
        TabStyle::Underline => underlined(look, on, tabs),
    }
}

type Tab = (ReaderTab, &'static str, String, Option<usize>);

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

/// Zed: the strip on the panel surface; the active tab takes the reader's fill, with hairlines
/// at its sides and none under it.
fn merged(look: &Look, on: ReaderTab, tabs: [Tab; 2]) -> El {
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

/// Islands: the strip is the top of the reader island; the active tab is filled and underlined.
fn underlined(look: &Look, on: ReaderTab, tabs: [Tab; 2]) -> El {
    let k = &look.kit;
    let mut r = row().key("tab-strip-tabs").w_full().grow(1.0).px(8.0).gap(4.0).items_center();
    for t in tabs {
        let active = t.0 == on;
        let mut tab = tab_el(look, t, active).h(30.0).radius(k.radius.md);
        if active {
            tab = tab.fill(look.s.tab_active).child(
                el().key("underline")
                    .pin(Some(8.0), None, Some(8.0), Some(0.0))
                    .h(2.0)
                    .radius(1.0)
                    .fill(k.c.border_focus)
                    .a11y_hidden(),
            );
        }
        r = r.child(tab);
    }
    col()
        .key("tab-strip")
        .role("tablist")
        .w_full()
        .h(STRIP_H)
        .fill(look.s.reader)
        .child(r)
        .child(hairline(look.s.hairline))
}
