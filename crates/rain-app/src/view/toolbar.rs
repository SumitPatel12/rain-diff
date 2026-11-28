//! The top bar on the ground: the Rain mark, the list toggle, the review's identity, progress,
//! the `⋯` views-and-settings menu and the one filled button, Finish review….

use rain_core::FileTag;
use rain_core::command::Command;
use rune::{El, Tx, el, rich, row, text};
use rune_kit::{Kit, Menu, MenuEntry, MenuItem, Placement};

use super::icons;
use super::ui::{Activate, Ghost, primary};
use crate::app::{App, MoreSub};
use crate::settings::{Density, Theme};
use crate::theme::{Look, TYPE, radius, space};

/// Below this window width the title leaves out the repos and refs (they stay in its tooltip).
const NARROW: f32 = 1500.0;

/// The document's title, else the session's id.
fn heading(s: &App) -> Option<String> {
    let named = s.overview.as_ref().map(|l| l.document.title.clone()).filter(|t| !t.is_empty());
    named.or_else(|| (!s.mirror.id.is_empty()).then(|| s.mirror.id.clone()))
}

/// The repos joined with ` + ` (`server + web`).
fn repo_names(s: &App) -> String {
    s.repos
        .iter()
        .map(|r| if r.id.is_empty() { r.name.as_str() } else { r.id.as_str() })
        .filter(|n| !n.is_empty())
        .collect::<Vec<_>>()
        .join(" + ")
}

/// `server + web · main → feature/vouchers`: the repos, then base to target.
fn refs(s: &App) -> String {
    let repos = repo_names(s);
    let mut parts = Vec::new();
    if !repos.is_empty() {
        parts.push(repos);
    }
    if let Some(r) = s.repos.first() {
        parts.push(format!("{} → {}", r.base, r.target));
    }
    parts.join(" · ")
}

/// `ACME-1234 Vouchers at checkout · server + web · development → feature/vouchers`.
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

const METER_W: f32 = 132.0;

/// The Rain mark (a rounded square with three rain strokes) and the wordmark, with a hairline
/// after them.
fn mark(look: &Look) -> El {
    let k = &look.kit;
    row()
        .key("rain-mark")
        .h(22.0)
        .gap(7.0)
        .pr(14.0)
        .items_center()
        .help("Rain")
        .label("Rain")
        .child(
            row()
                .size(22.0, 22.0)
                .radius(radius::BUTTON)
                .fill(look.s.selected_row)
                .child(icons::draw(&icons::RAIN, 22.0, look.r.accent)),
        )
        .child(
            text("Rain")
                .font(rune::tok::TextStyle {
                    weight: 700.0,
                    tracking: 0.2,
                    ..TYPE.ui
                })
                .color(k.c.text_muted)
                .a11y_hidden(),
        )
        .child(
            el().pin(None, Some(0.0), Some(0.0), Some(0.0))
                .w(1.0)
                .fill(look.s.hairline)
                .a11y_hidden(),
        )
}

/// `[▰▰▱▱] Must 1/4  3 / 8 viewed`: Must review and Review files viewed, Must's share first.
fn meter(s: &App, look: &Look) -> El {
    let k = &look.kit;
    let p = progress(s);
    let (done, total) = (p.must.0 + p.review.0, p.must.1 + p.review.1);
    let w = |n: usize| {
        if total == 0 {
            0.0
        } else {
            (METER_W * n as f32 / total as f32 * 2.0).round() / 2.0
        }
    };
    let bar = row()
        .key("meter-bar")
        .w(METER_W)
        .h(6.0)
        .radius(3.0)
        .fill(look.s.well)
        .border(1.0, look.s.hairline)
        .clip_rounded(3.0)
        .child(el().key("meter-must-seg").w(w(p.must.0)).h(6.0).fill(look.r.must))
        .child(el().key("meter-review-seg").w(w(p.review.0)).h(6.0).fill(look.r.accent));
    let said = format!(
        "Viewed {done} of {total} reviewable files. Must review {}/{}, Review {}/{}; {} skim or grouped files not counted",
        p.must.0, p.must.1, p.review.0, p.review.1, p.rest
    );
    let tnum = TYPE.small.features("'tnum' 1");
    let mut r = row()
        .key("meter")
        .role("status")
        .label(said.clone())
        .help(said)
        .gap(10.0)
        .items_center()
        .child(bar.a11y_hidden());
    if p.must.1 > 0 {
        r = r.child(
            text(format!("Must {}/{}", p.must.0, p.must.1))
                .key("meter-must")
                .font(rune::tok::TextStyle { weight: 600.0, ..tnum })
                .color(if p.must.0 == p.must.1 { k.c.text_muted } else { k.c.danger.fg })
                .a11y_hidden(),
        );
    }
    let count = format!("{done} / {total} viewed");
    let lead = done.to_string().len();
    r.child(
        rich(count, vec![rune::Span::new(0, lead).color(k.c.text_base)])
            .key("meter-text")
            .font(tnum)
            .color(k.c.text_muted)
            .a11y_hidden(),
    )
}

fn chord(c: Command) -> String {
    c.chord().map(|k| k.display()).unwrap_or_default()
}

/// A menu row that runs `c`, showing its icon and key.
fn command_item(label: &str, icon: &'static rune::Icon, c: Command) -> MenuItem {
    let mut item = MenuItem::new(label).icon(icon).on_select_tx(move |s: &mut App, tx: &mut Tx| {
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
        .icon(&icons::CONTRAST)
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

/// `⋯`: the palette and Go to, the views, the layout, Appearance (theme, density), keys and
/// reload, each with its key.
fn more_menu(s: &App, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut menu = Menu::new("more").placement(Placement::BottomEnd).min_width(260.0);
    // A closed menu draws only its trigger.
    if s.menus.more {
        menu = more_items(s, menu);
    }
    let trigger = Ghost::new("more-menu")
        .icon(&icons::MORE)
        .help("Views and settings")
        .on(s.menus.more)
        .build(k)
        .expanded(s.menus.more)
        .on_activate(|s: &mut App| {
            let open = !s.menus.more;
            s.menus.close();
            s.menus.more = open;
        });
    menu.trigger(trigger)
        .open(s.menus.more)
        .on_close(|s: &mut App| {
            s.menus.close();
        })
        .build(k, layer)
}

fn more_items(s: &App, menu: Menu) -> Menu {
    let open = s.menus.more_sub;
    let mut appearance: Vec<MenuEntry> = vec![MenuEntry::Label("Theme".into())];
    appearance.extend(Theme::PICKS.map(|(t, l)| pick(l, t, s.settings.theme, App::set_theme)));
    appearance.push(MenuEntry::Separator);
    appearance.push(MenuEntry::Label("Density".into()));
    appearance.extend(
        [("Compact", Density::Compact), ("Comfortable", Density::Comfortable)]
            .map(|(l, d)| pick(l, d, s.settings.density, App::set_density)),
    );
    let mut menu = menu
        .item(command_item("Command palette", &icons::KBD, Command::Palette))
        .item(command_item("Go to file or anything", &icons::SEARCH, Command::Picker))
        .separator()
        .item(command_item("Overview / Changes", &icons::BOOK, Command::Brief));
    if s.has_map() {
        menu = menu.item(command_item("Map", &icons::MAP, Command::Map));
    }
    menu.item(command_item("Tour", &icons::TOUR, Command::Tour))
        .item(command_item("Conversation", &icons::CHAT, Command::Conversation))
        .item(command_item("Diagram playground", &icons::DIAGRAM, Command::DiagramPlayground))
        .item(command_item("Find in diff", &icons::SEARCH, Command::Find))
        .item(command_item("Filter files", &icons::FUNNEL, Command::FilterFiles))
        .separator()
        .item(command_item("File list", &icons::SIDEBAR, Command::ToggleList))
        .item(command_item("Zoom", &icons::ZOOM, Command::Zoom))
        .item(command_item("One file at a time", &icons::FILE, Command::ToggleReaderMode))
        .item(command_item("Expand all context", &icons::EXPAND, Command::ExpandAllContext))
        .item(command_item("Collapse all context", &icons::FOLD_ALL, Command::CollapseAllContext))
        .item(command_item("Collapse all files", &icons::FOLD_ALL, Command::CollapseAllFiles))
        .item(command_item("Expand all files", &icons::UNFOLD_ALL, Command::ExpandAllFiles))
        .separator()
        .item(sub("Appearance", MoreSub::Appearance, open, appearance))
        .item(command_item("Keyboard shortcuts", &icons::KBD, Command::KeySheet))
        .item(command_item("Reload", &icons::RELOAD, Command::Reload))
        .separator()
        .item(
            MenuItem::new("Make it rain (break preview)")
                .icon(&icons::RAIN)
                .on_select(|s: &mut App| {
                    s.menus.close();
                    s.rest_preview();
                }),
        )
}

/// The review's title, then its repos and refs (left out below 1 500 points; the tooltip keeps
/// them).
fn identity(s: &App, look: &Look) -> El {
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
                .font(TYPE.title)
                .color(k.c.text_base)
                .truncate()
                .shrink()
                .min_w(0.0)
                .a11y_hidden(),
        );
    }
    if narrow {
        return lead;
    }
    let mut refs = row().key("title-refs").gap(6.0).items_center().shrink().min_w(0.0).a11y_hidden();
    let repos = repo_names(s);
    if !repos.is_empty() {
        refs = refs.child(text(repos).font(TYPE.small).color(k.c.text_muted).truncate().shrink());
    }
    if let Some(r) = s.repos.first() {
        if refs_shown(s) {
            refs = refs.child(text("·").font(TYPE.small).color(k.c.text_faint));
        }
        refs = refs
            .child(text(r.base.clone()).font(TYPE.micro_mono).color(k.c.text_muted))
            .child(text("→").font(TYPE.small).color(k.c.text_faint))
            .child(text(r.target.clone()).font(TYPE.micro_mono).color(k.c.text_base));
    }
    lead.child(refs)
}

fn refs_shown(s: &App) -> bool {
    !repo_names(s).is_empty()
}

/// `inset`: the points the macOS traffic lights take at the leading edge (`Cx::titlebar_inset`).
pub(super) fn top_bar(s: &App, look: &Look, inset: f32, layer: &mut Vec<El>) -> El {
    let k = &look.kit;
    let list_open = s.sidebar.is_open() && !s.layout.is_zoomed();
    let toggle = Ghost::new("list-toggle")
        .icon(&icons::SIDEBAR)
        .help(crate::keys::tip(
            if list_open { "Hide the file list" } else { "Show the file list" },
            Command::ToggleList,
        ))
        .build(k)
        .expanded(list_open)
        .on_activate(|s: &mut App| s.toggle_list());
    row()
        .key("toolbar")
        .w_full()
        .h(space::TOP_BAR)
        .pl(10.0 + inset)
        .pr(12.0)
        .window_drag()
        .gap(10.0)
        .items_center()
        .fill(look.s.bar)
        .child(mark(look))
        .child(toggle)
        .child(identity(s, look))
        .child(row().grow(1.0).a11y_hidden())
        .child(meter(s, look))
        .child(more_menu(s, k, layer))
        .child(
            primary("finish-review", "Finish review…", k)
                .help(crate::keys::tip("Verdict and summary for this round", Command::FinishReview))
                .on_activate_tx(|s: &mut App, tx: &mut Tx| s.finish_review(tx)),
        )
}
