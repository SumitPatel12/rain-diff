//! The status bar: where the reader is and two hints at the left; the latest message, what waits
//! on you, the agent's presence and the diff layout at the right.

use rain_core::command::Command;
use rain_diffview::DiffMode;
use rune::{El, Tx, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Menu, MenuItem, Placement};

use super::STATUS_HEIGHT;
use crate::app::App;
use crate::settings::ViewLayout;
use crate::theme::Look;

/// `File 7 of 23` for the reader's file, by the walk order; `Overview` on that tab.
fn place(s: &App) -> Option<String> {
    if s.reader_tab() == crate::app::ReaderTab::Overview {
        return Some("Overview".into());
    }
    let g = s.current()?;
    let order = s.walk_order();
    let i = order.iter().position(|&x| x == g)?;
    Some(format!("File {} of {}", i + 1, order.len()))
}

fn chord(c: Command) -> String {
    c.chord().map(|k| k.display()).unwrap_or_default()
}

/// Up to four hints for what the reader is doing, from the keymap.
pub(crate) fn hints(s: &App) -> Vec<String> {
    let pairs: Vec<(Command, &str)> = if s.composer.is_some() || s.reply_to.is_some() {
        return vec!["Ctrl+Enter send".into(), "Esc keeps a draft".into()];
    } else if s
        .focused_thread
        .as_deref()
        .and_then(|id| rain_comments::ops::thread(&s.mirror, id))
        .is_some_and(|t| !rain_comments::ui::takes_dispositions(t) && !rain_comments::ui::confirmable(t))
    {
        vec![(Command::Reply, "reply"), (Command::Escape, "unfocus")]
    } else if s.focused_thread.is_some() {
        vec![
            (Command::Agree, "agree"),
            (Command::Dismiss, "dismiss"),
            (Command::Reply, "reply"),
            (Command::Escape, "unfocus"),
        ]
    } else {
        vec![
            (Command::ViewedAndNext, "viewed+next"),
            (Command::NextFile, "next file"),
            (Command::Comment, "comment"),
            (Command::Palette, "commands"),
        ]
    };
    pairs.into_iter().map(|(c, what)| format!("{} {what}", chord(c))).collect()
}

/// `Split ▾`: Auto, Split and Unified; says `Split · narrow` while too narrow to split.
fn layout_menu(s: &App, look: &Look, layer: &mut Vec<El>) -> El {
    let k = &look.kit;
    let narrow = s.settings.layout == ViewLayout::Split && s.effective_mode() == DiffMode::Unified;
    let shown = if narrow {
        "Split · narrow".to_owned()
    } else {
        s.settings.layout.label().to_owned()
    };
    let why = s
        .layout_reason()
        .unwrap_or_else(|| "Diff layout: auto splits when it fits · s toggles split and unified".into());
    let mut menu = Menu::new("layout-menu").placement(Placement::TopEnd).min_width(180.0);
    for l in ViewLayout::ALL {
        menu = menu.item(
            MenuItem::new(l.label())
                .checked(s.settings.layout == l)
                .on_select(move |s: &mut App| {
                    s.menus.close();
                    s.set_layout(l);
                }),
        );
    }
    let trigger = Button::new(format!("{shown} ▾"))
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Mini)
        .help(why.clone())
        .on_press(|s: &mut App| {
            let open = !s.menus.layout;
            s.menus.close();
            s.menus.layout = open;
        })
        .build(k)
        .key("status-layout")
        .label(why);
    menu.trigger(trigger)
        .open(s.menus.layout)
        .on_close(|s: &mut App| {
            s.menus.close();
        })
        .build(k, layer)
}

pub(super) fn status_bar(s: &App, look: &Look, layer: &mut Vec<El>) -> El {
    let k = &look.kit;
    let mut left: Vec<String> = Vec::new();
    if let Some(p) = place(s) {
        left.push(p);
    }
    left.extend(hints(s).into_iter().take(2));
    let mut bar = row()
        .key("status")
        .w_full()
        .h(STATUS_HEIGHT)
        .px(12.0)
        .gap(12.0)
        .items_center()
        .fill(look.s.bar)
        .child(
            text(left.join("  ·  "))
                .key("status-hints")
                .font(k.ty.small)
                .color(k.c.text_muted)
                .truncate()
                .shrink(),
        )
        .child(row().grow(1.0).a11y_hidden())
        .child(
            text(s.status.clone())
                .key("status-text")
                .font(k.ty.small)
                .color(k.c.text_base)
                .truncate()
                .shrink()
                .min_w(0.0),
        );
    let waiting = s.waiting_count();
    if waiting > 0 {
        bar = bar.child(
            k.interactive(
                row()
                    .key("status-waiting")
                    .role("button")
                    .label(format!("{waiting} waiting on you: next (w)"))
                    .help("Next thread waiting on you (w)")
                    .h(20.0)
                    .px(6.0)
                    .gap(6.0)
                    .items_center()
                    .radius(k.radius.sm)
                    .on_press_tx(|s: &mut App, tx: &mut Tx| s.next_waiting(tx)),
            )
            .child(
                text(format!("{waiting} waiting"))
                    .font(k.ty.small_medium)
                    .color(k.c.warning.fg)
                    .a11y_hidden(),
            )
            .child(text("w").font(k.ty.small).color(k.c.text_faint).a11y_hidden()),
        );
    }
    bar = bar
        .child(super::conversation::presence_dot(s, k))
        .child(layout_menu(s, look, layer));
    bar.child(
        rune::el()
            .key("status-line")
            .pin(Some(0.0), Some(0.0), Some(0.0), None)
            .h(1.0)
            .fill(look.s.hairline)
            .a11y_hidden(),
    )
}
