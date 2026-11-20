//! The reader island's toolbar: the `Overview | Changes N` switch, then Find, Tour, Map and the
//! Conversation with its waiting count.

use rain_core::command::Command;
use rune::{Cursor, El, Tx, el, row, text};

use super::icons;
use super::ui::{Activate, Ghost, vsep};
use crate::app::{App, Overlay, ReaderTab};
use crate::theme::{Look, TYPE, radius, space};

type Tab = (ReaderTab, &'static str, &'static str, Option<usize>);

/// The segmented switch on the well; the active side lifts onto the card head.
fn switch(s: &App, look: &Look) -> El {
    let k = &look.kit;
    let on = s.reader_tab();
    let tabs: [Tab; 2] = [
        (ReaderTab::Overview, "tab-overview", "Overview", None),
        (ReaderTab::Changes, "tab-changes", "Changes", Some(s.active().files().len())),
    ];
    let mut seg = row()
        .key("tab-strip")
        .role("tablist")
        .label(crate::keys::tip("Overview or Changes", Command::Brief))
        .padding(2.0)
        .radius(7.0)
        .fill(look.s.well)
        .border(1.0, look.s.hairline);
    // The Map takes the reader, so neither side lifts while it is open.
    let map = s.layout.is_open(Overlay::Map);
    for (tab, key, name, count) in tabs {
        let active = tab == on && !map;
        let said = match count {
            Some(n) => format!("{name}, {n} files"),
            None => name.to_owned(),
        };
        let mut t = row()
            .key(key)
            .role("tab")
            .label(said)
            .help(crate::keys::tip(name, Command::Brief))
            .checked(tab == on)
            .h(24.0)
            .px(10.0)
            .gap(6.0)
            .items_center()
            .radius(radius::SMALL)
            .cursor(Cursor::Pointer)
            .on_press(move |s: &mut App| s.set_reader_tab(tab))
            .child(
                text(name)
                    .font(TYPE.small)
                    .color(if active { k.c.text_base } else { k.c.text_muted })
                    .a11y_hidden(),
            );
        if let Some(n) = count {
            t = t.child(
                text(n.to_string())
                    .font(TYPE.small.features("'tnum' 1"))
                    .color(k.c.text_faint)
                    .a11y_hidden(),
            );
        }
        t = if active {
            t.fill(look.s.card_head).border(1.0, look.s.card_border)
        } else {
            k.interactive(t)
        };
        seg = seg.child(t);
    }
    seg
}

fn run(c: Command) -> impl Fn(&mut App, &mut Tx) + 'static {
    move |s: &mut App, tx: &mut Tx| {
        let _ = s.run(c, tx);
    }
}

/// The toolbar across the top of the reader island, a hairline under it.
pub(super) fn strip(s: &App, look: &Look) -> El {
    let k = &look.kit;
    let mut r = row()
        .key("reader-tool")
        .w_full()
        .h(space::READER_TOOL)
        // The 30 point tab strip leaves 5 above and below; the same on its left.
        .pl((space::READER_TOOL - 30.0) / 2.0)
        .pr(8.0)
        .gap(4.0)
        .items_center()
        .child(switch(s, look))
        .children(s.single_mode().then(|| {
            k.interactive(
                row()
                    .key("tool-one-at-a-time")
                    .role("button")
                    .label("One at a time")
                    .help(match Command::ToggleReaderMode.chord() {
                        Some(c) => format!("One file at a time · {} back to the stream", c.display()),
                        None => "One file at a time".to_owned(),
                    })
                    .ml(8.0)
                    .h(20.0)
                    .px(6.0)
                    .items_center()
                    .radius(radius::CHIP)
                    .border(1.0, look.s.card_border)
                    .cursor(rune::Cursor::Pointer)
                    .child(text("One at a time").font(TYPE.micro).color(look.r.accent).a11y_hidden()),
            )
            .on_press_tx(|s: &mut App, tx: &mut Tx| s.toggle_reader_mode(tx))
        }))
        .child(row().grow(1.0).a11y_hidden())
        .child(
            Ghost::new("tool-find")
                .icon(&icons::SEARCH)
                .help(crate::keys::tip("Find in diff", Command::Find))
                .on(s.find.open)
                .build(k)
                .on_activate_tx(run(Command::Find)),
        )
        .child(
            Ghost::new("tool-tour")
                .icon(&icons::TOUR)
                .label("Tour")
                .help(crate::keys::tip("Tour", Command::Tour))
                .on(s.layout.is_open(Overlay::Tour))
                .build(k)
                .on_activate_tx(run(Command::Tour)),
        );
    if s.has_map() {
        r = r.child(
            Ghost::new("tool-map")
                .icon(&icons::MAP)
                .label("Map")
                .help(crate::keys::tip("Map", Command::Map))
                .on(s.layout.is_open(Overlay::Map))
                .build(k)
                .on_activate_tx(run(Command::Map)),
        );
    }
    let waiting = s.waiting_count();
    let mut conv = Ghost::new("tool-conversation")
        .icon(&icons::CHAT)
        .label("Conversation")
        .help(crate::keys::tip("Conversation", Command::Conversation))
        .on(s.layout.is_open(Overlay::Conversation));
    if waiting > 0 {
        conv = conv.child(
            row()
                .key("tool-conversation-badge")
                .min_w(16.0)
                .h(16.0)
                .px(4.0)
                .center()
                .radius(8.0)
                .fill(k.c.warning.bg)
                .border(1.0, k.c.warning.border)
                .child(text(waiting.to_string()).font(TYPE.micro).color(k.c.warning.fg)),
        );
    }
    r.child(vsep(k))
        .child(conv.build(k).on_activate_tx(run(Command::Conversation)))
        .child(
            el().key("reader-tool-line")
                .pin(Some(0.0), None, Some(0.0), Some(0.0))
                .h(1.0)
                .fill(look.s.hairline)
                .a11y_hidden(),
        )
}
