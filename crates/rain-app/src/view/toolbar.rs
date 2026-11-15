//! The top bar: the title, one progress meter, the `⋯` menu and the one filled button.

use rain_core::FileTag;
use rain_core::command::Command;
use rune::{El, Tx, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, Menu, MenuEntry, MenuItem, Placement, Toolbar};

use crate::app::{App, MoreSub};
use crate::settings::{Chrome, Density, Theme};
use crate::theme::{Look, NARROW};

/// The document's title, else the session's id.
fn heading(s: &App) -> Option<String> {
    let named = s.overview.as_ref().map(|l| l.document.title.clone()).filter(|t| !t.is_empty());
    named.or_else(|| (!s.mirror.id.is_empty()).then(|| s.mirror.id.clone()))
}

/// `server, app  4857d0f → dae9833`: the repos, then base to target.
fn refs(s: &App) -> String {
    let repos: Vec<&str> = s
        .repos
        .iter()
        .map(|r| if r.id.is_empty() { r.name.as_str() } else { r.id.as_str() })
        .filter(|n| !n.is_empty())
        .collect();
    let mut parts = Vec::new();
    if !repos.is_empty() {
        parts.push(repos.join(", "));
    }
    if let Some(r) = s.repos.first() {
        parts.push(format!("{} → {}", r.base, r.target));
    }
    parts.join("  ")
}

/// `ACME-1234 · server, app  development → feature/vouchers`.
pub(crate) fn title(s: &App) -> String {
    let refs = refs(s);
    match heading(s) {
        Some(h) if refs.is_empty() => h,
        Some(h) => format!("{h} · {refs}"),
        None => refs,
    }
}

/// Viewed and total for Must review and Review; Skim and the rest are not counted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Progress {
    pub must: (usize, usize),
    pub review: (usize, usize),
    pub rest: usize,
}

pub(crate) fn progress(s: &App) -> Progress {
    let mut p = Progress::default();
    for (g, key) in s.keys.iter().enumerate() {
        let slot = match s.tags.get(key).copied().unwrap_or(FileTag::Review) {
            FileTag::MustReview => &mut p.must,
            FileTag::Review => &mut p.review,
            _ => {
                p.rest += 1;
                continue;
            }
        };
        slot.1 += 1;
        if s.is_viewed(g) {
            slot.0 += 1;
        }
    }
    p
}

const METER_W: f32 = 72.0;

/// `▰▰▱ Must 5/17 · 21/88`: Must review and Review files viewed, Must's share first and in red.
fn meter(s: &App, k: &Kit) -> El {
    let p = progress(s);
    let (done, total) = (p.must.0 + p.review.0, p.must.1 + p.review.1);
    let w = |n: usize| {
        if total == 0 {
            0.0
        } else {
            (METER_W * n as f32 / total as f32).round()
        }
    };
    let bar = row()
        .key("meter-bar")
        .w(METER_W)
        .h(4.0)
        .radius(2.0)
        .fill(k.c.layer_03)
        .clip_rounded(2.0)
        .child(
            row()
                .w(w(p.must.1))
                .h(4.0)
                .fill(k.alpha(k.c.danger.icon, 0.35))
                .child(el().w(w(p.must.0)).h(4.0).fill(k.c.danger.icon)),
        )
        .child(el().w(w(p.review.0)).h(4.0).fill(k.c.success.icon));
    let said = format!(
        "{done} of {total} files to review viewed. Must review {}/{}, Review {}/{}; {} skim or ignored files not counted",
        p.must.0, p.must.1, p.review.0, p.review.1, p.rest
    );
    let tnum = k.ty.small_medium.features("'tnum' 1");
    let mut r = row()
        .key("meter")
        .role("status")
        .label(said.clone())
        .help(said)
        .gap(8.0)
        .items_center()
        .px(6.0)
        .child(bar.a11y_hidden());
    if p.must.1 > 0 {
        r = r.child(
            text(format!("Must {}/{}", p.must.0, p.must.1))
                .key("meter-must")
                .font(tnum)
                .color(if p.must.0 == p.must.1 { k.c.text_muted } else { k.c.danger.fg })
                .a11y_hidden(),
        );
    }
    r.child(
        text(format!("{done} / {total}"))
            .key("meter-text")
            .font(tnum)
            .color(k.c.text_muted)
            .a11y_hidden(),
    )
}

fn chord(c: Command) -> String {
    c.chord().map(|k| k.display()).unwrap_or_default()
}

/// A menu row that runs `c`, showing its key.
fn command_item(label: &str, c: Command) -> MenuItem {
    let mut item = MenuItem::new(label).on_select_tx(move |s: &mut App, tx: &mut Tx| {
        s.menus.close();
        s.run(c, tx);
    });
    let key = chord(c);
    if !key.is_empty() {
        item = item.shortcut(key);
    }
    item
}

fn sub(label: &str, which: MoreSub, open: Option<MoreSub>, entries: Vec<MenuEntry>) -> MenuItem {
    MenuItem::new(label)
        .on_select(move |s: &mut App| {
            s.menus.more_sub = if s.menus.more_sub == Some(which) { None } else { Some(which) };
        })
        .submenu(entries, open == Some(which))
}

fn pick<T: Copy + PartialEq + 'static>(label: &str, value: T, now: T, set: fn(&mut App, T)) -> MenuEntry {
    MenuEntry::item(MenuItem::new(label).checked(value == now).on_select(move |s: &mut App| {
        s.menus.close();
        set(s, value);
    }))
}

/// `⋯`: the views, Appearance (chrome, theme, density), keys and the palette.
fn more_menu(s: &App, k: &Kit, layer: &mut Vec<El>) -> El {
    let open = s.menus.more_sub;
    let chrome = Chrome::ALL
        .iter()
        .map(|&c| pick(c.label(), c, s.chrome(), App::set_chrome))
        .collect();
    let theme = [("System", Theme::System), ("Light", Theme::Light), ("Dark", Theme::Dark)]
        .map(|(l, t)| pick(l, t, s.settings.theme, App::set_theme))
        .into();
    let density = [("Compact", Density::Compact), ("Comfortable", Density::Comfortable)]
        .map(|(l, d)| pick(l, d, s.settings.density, App::set_density))
        .into();
    let mut menu = Menu::new("more")
        .placement(Placement::BottomEnd)
        .min_width(240.0)
        .item(command_item("Overview / Changes", Command::Brief));
    if s.has_map() {
        menu = menu.item(command_item("Map", Command::Map));
    }
    menu = menu
        .item(command_item("Tour", Command::Tour))
        .item(command_item("Conversation", Command::Conversation))
        .item(command_item("Find in diff", Command::Find))
        .item(command_item("Filter files", Command::FilterFiles))
        .separator()
        .group_label("Appearance")
        .item(sub("Chrome", MoreSub::Chrome, open, chrome))
        .item(sub("Theme", MoreSub::Theme, open, theme))
        .item(sub("Density", MoreSub::Density, open, density))
        .separator()
        .item(command_item("Keyboard shortcuts", Command::KeySheet))
        .item(command_item("Command palette", Command::Palette));
    let trigger = Button::new("⋯")
        .variant(ButtonVariant::GhostMuted)
        .control_size(ControlSize::Regular)
        .help("More: views, appearance, keys")
        .on_press(|s: &mut App| {
            let open = !s.menus.more;
            s.menus.close();
            s.menus.more = open;
        })
        .build(k)
        .key("more-menu");
    menu.trigger(trigger)
        .open(s.menus.more)
        .on_close(|s: &mut App| {
            s.menus.close();
        })
        .build(k, layer)
}

pub(super) fn toolbar(s: &App, look: &Look, layer: &mut Vec<El>) -> Toolbar {
    let k = &look.kit;
    let width = super::window_width(s);
    let narrow = width > 0.0 && width < NARROW;
    let mut lead = row()
        .key("title")
        .gap(10.0)
        .items_center()
        .min_w(0.0)
        .shrink()
        .label(title(s))
        .help(format!("{}\nChanges from base to target", title(s)));
    if let Some(h) = heading(s) {
        lead = lead.child(
            text(h)
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .truncate()
                .shrink()
                .min_w(0.0)
                .a11y_hidden(),
        );
    }
    let refs = refs(s);
    if !narrow && !refs.is_empty() {
        lead = lead.child(
            text(refs)
                .key("title-refs")
                .font(k.ty.small)
                .color(k.c.text_muted)
                .truncate()
                .shrink()
                .min_w(0.0)
                .a11y_hidden(),
        );
    }
    let trailing = row()
        .gap(6.0)
        .items_center()
        .child(meter(s, k))
        .child(more_menu(s, k, layer))
        .child(
            Button::new("Finish review…")
                .variant(ButtonVariant::Submit)
                .help("Verdict and summary for this round (Shift+F)")
                .on_press_tx(|s: &mut App, tx: &mut Tx| s.finish_review(tx))
                .build(k)
                .key("finish-review"),
        );
    Toolbar::new().leading(lead).trailing(trailing)
}
