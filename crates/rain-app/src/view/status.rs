//! The status line on the ground: where you are and two key hints at the left; the latest
//! message, what waits on you, the agent's presence and the diff layout picker at the right.

use rain_core::command::Command;
use rain_diffview::DiffMode;
use rune::{Cursor, El, Tx, row, text};
use rune_kit::{Menu, MenuItem, Placement};

use super::icons;
use super::ui::{kbd, sr};
use crate::app::App;
use crate::settings::ViewLayout;
use crate::theme::{Look, TYPE, radius, space};

/// `File 7 of 23` for the reader's file, by the walk order; `Overview` on that tab; `Zoomed`.
fn place(s: &App) -> Option<String> {
    match &s.phase {
        crate::app::Phase::Loading(_) => return Some("Loading".into()),
        crate::app::Phase::Error(_) => return Some("Could not load".into()),
        crate::app::Phase::Ready => {}
    }
    if s.layout.is_open(crate::app::Overlay::Tour) && s.tour.stage == crate::app::TourStage::Step {
        let n = s.tour_views().len();
        return Some(format!("Tour · step {} of {n}", (s.tour.step + 1).min(n)));
    }
    if s.layout.is_open(crate::app::Overlay::Conversation) {
        return Some("Conversation".into());
    }
    if s.layout.is_zoomed() {
        return Some("Zoomed".into());
    }
    if s.layout.is_open(crate::app::Overlay::Map) && s.has_map() {
        let targets = s.overview.as_ref().map(|l| l.targets.clone()).unwrap_or_default();
        let name = rain_overview::tab_names(&targets)
            .get(s.diagram.target)
            .cloned()
            .unwrap_or_default();
        return Some(if name.is_empty() { "Map".into() } else { format!("Map · {name}") });
    }
    if s.composer.is_none()
        && let Some(sp) = s.active().span_selection().filter(|_| s.selection_toolbar_shown())
    {
        return Some(format!("Selected {} chars", sp.text.chars().count()));
    }
    if s.find.open && s.find.edit.focused && !s.find.query.is_empty() {
        let n = s.find.hits.len();
        return Some(if n == 0 {
            "Find · no matches".into()
        } else {
            format!("Find · {} of {n}", (s.find.at + 1).min(n))
        });
    }
    if s.reader_tab() == crate::app::ReaderTab::Overview {
        return Some(if s.brief_doc().is_some() {
            "Overview".into()
        } else {
            "Overview · no brief".into()
        });
    }
    let g = s.current()?;
    let l = s.listing();
    let i = l.order.iter().position(|&x| x == g)?;
    Some(format!("File {} of {}", i + 1, l.order.len()))
}

fn chord(c: Command) -> String {
    c.chord().map(|k| k.display()).unwrap_or_default()
}

/// Up to four (key, what) hints for what the reader is doing, from the keymap.
pub(crate) fn hint_pairs(s: &App) -> Vec<(String, String)> {
    let pairs: Vec<(Command, &str)> = if s.phase != crate::app::Phase::Ready {
        return vec![("Shift+R".into(), "reload".into()), ("Ctrl+K".into(), "palette".into())];
    } else if s.brief.peek.open.is_some() {
        return vec![("Enter".into(), "open in reader".into()), ("Esc".into(), "close peek".into())];
    } else if s.layout.is_open(crate::app::Overlay::Tour) && s.tour.stage == crate::app::TourStage::Step && s.composer.is_none() {
        return vec![
            ("Space".into(), s.tour_space_does().into()),
            ("Esc".into(), "leave the tour".into()),
        ];
    } else if s.layout.is_open(crate::app::Overlay::Conversation) && s.composer.is_none() && s.reply_to.is_none() {
        return vec![("w".into(), "next waiting".into()), ("a".into(), "confirm".into())];
    } else if s.composer.is_none() && s.active().span_selection().is_some() && s.selection_toolbar_shown() {
        return vec![("c".into(), "comment on the selection".into()), ("?".into(), "ask about it".into())];
    } else if s.layout.is_open(crate::app::Overlay::Map) && s.has_map() && s.composer.is_none() {
        return vec![("c".into(), "comment on node".into()), ("Esc".into(), "back to changes".into())];
    } else if s.find.open && s.find.edit.focused {
        return vec![("Enter".into(), "next".into()), ("Shift+Enter".into(), "previous".into())];
    } else if s.layout.is_zoomed() && s.composer.is_none() && s.reply_to.is_none() {
        return vec![
            ("f".into(), "files · C conversation · m map · b overview".into()),
            ("Shift+Esc".into(), "restore".into()),
        ];
    } else if s.composer.is_some() || s.reply_to.is_some() {
        return vec![("Ctrl+Enter".into(), "send".into()), ("Esc".into(), "keeps a draft".into())];
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
            (Command::ViewedAndNext, "viewed + next"),
            (Command::NextFile, "next file"),
            (Command::Comment, "comment"),
            (Command::Palette, "commands"),
        ]
    };
    pairs.into_iter().map(|(c, what)| (chord(c), what.to_owned())).collect()
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
        .unwrap_or_else(|| "Diff layout: Auto / Split / Unified · s toggles split and unified".into());
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
    let ink = if narrow { k.c.warning.fg } else { k.c.text_base };
    let trigger = k
        .interactive(
            row()
                .key("status-layout")
                .role("button")
                .label(why.clone())
                .help(why)
                .h(22.0)
                .px(6.0)
                .gap(4.0)
                .items_center()
                .radius(radius::SMALL)
                .fill(k.c.hover)
                .cursor(Cursor::Pointer)
                .focusable(true)
                .expanded(s.menus.layout)
                .child(text(shown).font(TYPE.small).color(ink).a11y_hidden())
                .child(icons::draw(&icons::CHEV_D, 14.0, ink)),
        )
        .on_press(|s: &mut App| {
            let open = !s.menus.layout;
            s.menus.close();
            s.menus.layout = open;
        });
    menu.trigger(trigger)
        .open(s.menus.layout)
        .on_close(|s: &mut App| {
            s.menus.close();
        })
        .build(k, layer)
}

/// The latest message: the status text, else the agent's latest word.
fn latest(s: &App) -> Option<(bool, String)> {
    if !s.status.is_empty() && s.status != crate::app::layout::ZOOMED && !s.status.starts_with("Selected ") {
        return Some((false, s.status.clone()));
    }
    s.latest_agent_message().map(|m| (true, m))
}

pub(super) fn status_bar(s: &App, look: &Look, layer: &mut Vec<El>) -> El {
    let k = &look.kit;
    let (_, r) = sr(k);
    let place = place(s);
    let pairs: Vec<(String, String)> = hint_pairs(s).into_iter().take(2).collect();
    let mut said: Vec<String> = place.iter().cloned().collect();
    said.extend(pairs.iter().map(|(k, w)| format!("{k} {w}")));
    let mut left = row()
        .key("status-hints")
        .label(said.join("  ·  "))
        .gap(14.0)
        .items_center()
        .shrink()
        .min_w(0.0);
    if let Some(p) = place {
        left = left.child(text(p).key("status-where").font(TYPE.small).color(k.c.text_base).a11y_hidden());
    }
    for (i, (key, what)) in pairs.into_iter().enumerate() {
        left = left.child(
            row()
                .key(format!("status-hint-{i}"))
                .gap(5.0)
                .items_center()
                .a11y_hidden()
                .child(kbd(key, k))
                .child(text(what).font(TYPE.small).color(k.c.text_muted)),
        );
    }
    let mut bar = row()
        .key("status-line")
        .w_full()
        .h(space::STATUS)
        .px(14.0)
        .gap(14.0)
        .items_center()
        .fill(look.s.bar)
        .child(left);
    let mut msg = row().key("status-latest").grow(1.0).shrink().min_w(0.0).gap(6.0).items_center();
    if let Some((agent, m)) = latest(s) {
        if agent {
            msg = msg.child(icons::draw(&icons::SPARK, 14.0, r.note));
        }
        msg = msg.child(
            text(m)
                .key("status-text")
                .font(TYPE.small)
                .color(k.c.text_muted)
                .truncate()
                .shrink()
                .min_w(0.0),
        );
    }
    bar = bar.child(msg);
    let waiting = s.waiting_count();
    if waiting > 0 {
        bar = bar.child(
            k.interactive(
                row()
                    .key("status-waiting")
                    .role("button")
                    .label(format!("{waiting} waiting on you: next (w)"))
                    .help("Next thread waiting on you  w")
                    .h(22.0)
                    .px(6.0)
                    .gap(6.0)
                    .items_center()
                    .radius(radius::SMALL)
                    .cursor(Cursor::Pointer)
                    .on_press_tx(|s: &mut App, tx: &mut Tx| s.next_waiting(tx)),
            )
            .child(
                text(format!("{waiting} waiting"))
                    .font(TYPE.small)
                    .color(k.c.warning.fg)
                    .a11y_hidden(),
            ),
        );
    }
    bar.child(super::conversation::presence_dot(s, k))
        .child(layout_menu(s, look, layer))
}
