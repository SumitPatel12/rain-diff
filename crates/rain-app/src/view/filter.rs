//! The filter bar above the diff while a file filter is active, and the list's chips popover.

use rain_tree::{TreeFilter, TreeView};
use rune::{El, Icon, Tx, col, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Kit, Placement, popover};

use crate::app::App;

/// Repos, tags and toggles the filter has on.
fn chips_on(s: &App) -> usize {
    let f = &s.tree.filter;
    f.repos.len() + f.tags.len() + usize::from(f.unviewed_only) + usize::from(f.commented_only)
}

static FUNNEL: Icon = Icon {
    name: "rain-funnel",
    view: 16.0,
    body: r#"<path d="M2.5 3.5h11L9.25 8.25v4.25l-2.5 1.25V8.25z" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round" fill="none"/>"#,
};

/// The funnel at the filter box's end, with how many chips are on; it opens the chips popover.
pub(super) fn chips_button(s: &App, k: &Kit) -> El {
    let on = chips_on(s);
    let button = IconButton::new(&FUNNEL)
        .variant(if on > 0 || s.menus.chips {
            IconButtonVariant::Neutral
        } else {
            IconButtonVariant::GhostMuted
        })
        .control_size(ControlSize::Regular)
        .label(if on > 0 {
            format!("Filter chips, {on} on")
        } else {
            "Filter chips".into()
        })
        .help("Filter by repo, group, unviewed or comments")
        .on_press(|s: &mut App| {
            let open = !s.menus.chips;
            s.menus.close();
            s.menus.chips = open;
        })
        .build(k)
        .key("filter-chips-button")
        .expanded(s.menus.chips);
    if on == 0 {
        return button;
    }
    button.child(
        text(on.to_string())
            .key("filter-chips-count")
            .font(k.ty.text_xsmall_medium)
            .color(k.c.text_accent)
            .absolute(18.0, -4.0)
            .a11y_hidden(),
    )
}

/// The repo, tag, unviewed and comments chips under `⚲`; they stay open while toggled.
pub(super) fn chips_popover(s: &App, k: &Kit, layer: &mut Vec<El>) {
    if !s.menus.chips || !(s.sidebar.is_open() || s.layout.is_open(crate::app::Overlay::Files)) {
        return;
    }
    layer.push(rune_kit::scrim("filter-chips-scrim", |s: &mut App| s.menus.chips = false));
    let chips = TreeView::new(|s: &App| &s.tree, |s: &mut App| &mut s.tree)
        .chips_only()
        .on_filter_change(|s: &mut App, f: &TreeFilter, tx: &mut Tx| s.on_filter_change(f, tx))
        .build(s, k);
    let surface = popover::raised(k, col().key("filter-chips").w(300.0).py(4.0).child(chips), k.radius.xl);
    layer.push(popover::anchored(k, surface, "filter-chips-button", Placement::BottomEnd, 6.0));
}

/// "12 of 302 files — server, Must review, 'vouchers'".
pub(super) fn filter_bar(s: &App, k: &Kit) -> Option<El> {
    if !s.filter_active() {
        return None;
    }
    let n = s.filter.subset.as_ref().map_or(0, |v| v.files.len());
    let msg = format!("{n} of {} files — {}", s.keys.len(), filter_terms(s));
    Some(
        row()
            .key("filter-bar")
            .role("status")
            .label(msg.clone())
            .w_full()
            .px(12.0)
            .py(4.0)
            .gap(10.0)
            .items_center()
            .fill(k.c.layer_01)
            .child(
                text(msg)
                    .key("filter-bar-text")
                    .font(k.ty.small)
                    .color(k.c.text_muted)
                    .truncate()
                    .grow(1.0)
                    .shrink()
                    .a11y_hidden(),
            )
            .child(
                Button::new("Clear")
                    .variant(ButtonVariant::Ghost)
                    .help("Clear filter (Esc)")
                    .on_press(|s: &mut App| {
                        s.clear_filter();
                    })
                    .build(k)
                    .key("filter-clear"),
            ),
    )
}

/// The filter's terms in chip order: repos, tags, the toggles, then the quoted text.
fn filter_terms(s: &App) -> String {
    let f = &s.tree.filter;
    let mut terms: Vec<String> = s
        .tree
        .repos()
        .iter()
        .filter(|(r, _)| f.repos.contains(r))
        .map(|(_, name)| name.clone())
        .collect();
    terms.extend(f.tags.iter().map(|t| t.label().to_owned()));
    if f.unviewed_only {
        terms.push("Unviewed".into());
    }
    if f.commented_only {
        terms.push("Comments".into());
    }
    if !f.text.trim().is_empty() {
        terms.push(format!("'{}'", f.text.trim()));
    }
    terms.join(", ")
}
