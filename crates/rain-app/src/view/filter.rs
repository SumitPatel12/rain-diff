//! The filter strip above the diff while a file filter is active, the funnel by the list's
//! filter field, its popover of toggles, and the list's "N files hidden" foot.

use rain_tree::{TreeFilter, TreeView};
use rune::{Cursor, El, Tx, col, el, row, text};
use rune_kit::{Kit, Placement, popover};

use super::icons;
use super::ui::{Ghost, outline, sr};
use crate::app::App;
use crate::theme::{TYPE, radius};

/// Repos, tags and toggles the filter has on.
fn chips_on(s: &App) -> usize {
    let f = &s.tree.filter;
    f.repos.len() + f.tags.len() + usize::from(f.unviewed_only) + usize::from(f.commented_only)
}

/// The funnel at the filter field's end, with a badge for how many toggles are on; it opens
/// the toggles popover.
pub(super) fn chips_button(s: &App, k: &Kit) -> El {
    let (_, r) = sr(k);
    let on = chips_on(s);
    let mut b = Ghost::new("filter-chips-button")
        .icon(&icons::FUNNEL)
        .help("Filter by repo, tag, unviewed or comments")
        .on(s.menus.chips)
        .build(k)
        .label(if on > 0 {
            format!("Filter toggles, {on} on")
        } else {
            "Filter toggles".into()
        })
        .expanded(s.menus.chips)
        .on_press(|s: &mut App| {
            let open = !s.menus.chips;
            s.menus.close();
            s.menus.chips = open;
        });
    if on > 0 {
        b = b.child(
            row()
                .key("filter-chips-count")
                .absolute(15.0, -1.0)
                .min_w(14.0)
                .h(14.0)
                .px(3.0)
                .center()
                .radius(7.0)
                .fill(r.accent)
                .a11y_hidden()
                .child(
                    text(on.to_string())
                        .font(rune::tok::TextStyle::new(9.0, 700.0).lh(14.0))
                        .color(r.on_accent),
                ),
        );
    }
    b
}

/// The repo, tag, unviewed and comments toggles under the funnel; they stay open while toggled.
pub(super) fn chips_popover(s: &App, k: &Kit, layer: &mut Vec<El>) {
    if !s.menus.chips || !(s.sidebar.is_open() || s.layout.is_open(crate::app::Overlay::Files)) {
        return;
    }
    layer.push(rune_kit::scrim("filter-chips-scrim", |s: &mut App| s.menus.chips = false));
    let chips = TreeView::new(|s: &App| &s.tree, |s: &mut App| &mut s.tree)
        .chips_only()
        .on_filter_change(|s: &mut App, f: &TreeFilter, tx: &mut Tx| s.on_filter_change(f, tx))
        .build(s, k);
    let (surf, _) = sr(k);
    let clear = k
        .interactive(
            row()
                .key("filter-clear-all")
                .role("button")
                .label("Clear all")
                .w_full()
                .h(28.0)
                .px(12.0)
                .items_center()
                .radius(radius::SMALL)
                .cursor(Cursor::Pointer)
                .help("Clear all  Ctrl+Shift+L")
                .child(text("Clear all").font(TYPE.small).color(k.c.text_muted).grow(1.0).a11y_hidden()),
        )
        .on_press(|s: &mut App| {
            s.clear_filter();
        });
    let card = col()
        .key("filter-chips")
        .w(250.0)
        .py(4.0)
        .child(chips)
        .child(el().w_full().h(1.0).my(2.0).fill(surf.hairline).a11y_hidden())
        .child(row().w_full().px(4.0).child(clear));
    let surface = popover::raised(k, card, radius::CARD);
    layer.push(popover::anchored(k, surface, "filter-chips-button", Placement::BottomStart, 6.0));
}

/// "4 of 10 files — server, Must review" with Clear, on the card head band.
pub(super) fn filter_bar(s: &App, k: &Kit) -> Option<El> {
    if !s.filter_active() {
        return None;
    }
    let (surf, _) = sr(k);
    let n = s.filter.subset.as_ref().map_or(0, |v| v.files.len());
    let msg = format!("{n} of {} files — {}", s.keys.len(), filter_terms(s));
    Some(
        row()
            .key("filter-bar")
            .role("status")
            .label(msg.clone())
            .w_full()
            .min_h(34.0)
            .pl(12.0)
            .pr(10.0)
            .py(4.0)
            .gap(10.0)
            .items_center()
            .fill(surf.card_head)
            .child(
                text(msg)
                    .key("filter-bar-text")
                    .font(TYPE.small)
                    .color(k.c.text_muted)
                    .truncate()
                    .grow(1.0)
                    .shrink()
                    .a11y_hidden(),
            )
            .child(
                outline("filter-clear", "Clear", Some("Ctrl+Shift+L"), 24.0, k)
                    .help("Clear the filter  Ctrl+Shift+L · Esc")
                    .on_press(|s: &mut App| {
                        s.clear_filter();
                    }),
            )
            .child(
                el().pin(Some(0.0), None, Some(0.0), Some(0.0))
                    .h(1.0)
                    .fill(surf.hairline)
                    .a11y_hidden(),
            ),
    )
}

/// The list's foot while filtering: `12 files hidden · Clear filter`.
pub(super) fn hidden_foot(s: &App, k: &Kit) -> Option<El> {
    if !s.filter_active() {
        return None;
    }
    let shown = s.filter.subset.as_ref().map_or(0, |v| v.files.len());
    let hidden = s.keys.len().saturating_sub(shown);
    if hidden == 0 {
        return None;
    }
    Some(
        row()
            .key("list-hidden")
            .w_full()
            .px(14.0)
            .py(8.0)
            .gap(4.0)
            .items_center()
            .child(
                text(if hidden == 1 {
                    "1 file hidden ·".to_owned()
                } else {
                    format!("{hidden} files hidden ·")
                })
                .font(TYPE.small)
                .color(k.c.text_faint),
            )
            .child(
                row()
                    .key("list-hidden-clear")
                    .role("button")
                    .label("Clear filter")
                    .cursor(Cursor::Pointer)
                    .child(text("Clear filter").font(TYPE.small).color(k.c.text_accent).a11y_hidden())
                    .on_press(|s: &mut App| {
                        s.clear_filter();
                    }),
            ),
    )
}

/// The filter's terms in toggle order: repos, tags, the toggles, then the quoted text.
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
