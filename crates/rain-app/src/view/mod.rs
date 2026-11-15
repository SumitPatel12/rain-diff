//! The window: a thin shell of toolbar, list, reader, overlays and status line in the look from
//! theme.rs. Each feature draws its own part from its module.

mod board;
mod composer;
mod conversation;
mod diagram;
mod diff;
mod filter;
mod find;
mod finish;
pub(crate) mod messages;
mod notes;
mod overview;
mod palette;
mod peek;
mod picker;
mod ruler;
mod selection;
mod sidebar;
mod status;
mod tabs;
mod toolbar;
mod tour;
mod wholefile;

use rune::{Color, Cx, El, col, el, row, text};
use rune_kit::{Kit, SplitView};

use crate::app::{App, Overlay, Phase};
use crate::settings::Density;
use crate::theme::{self, Look, Scheme};

/// rune-kit's toolbar band; overlays start under it.
const TOOLBAR_HEIGHT: f32 = 48.0;
const STATUS_HEIGHT: f32 = 28.0;

fn pad(s: &App) -> f32 {
    if s.settings.density == Density::Compact { 6.0 } else { 10.0 }
}

#[cfg(test)]
pub(crate) fn toolbar_title(s: &App) -> String {
    toolbar::title(s)
}

#[cfg(test)]
pub(crate) use overview::{cite_target, demote_headings, inline_cites, link_cites, parse_cite};
pub(crate) use peek::code as peek_code;

pub fn scheme_of(s: &App, cx: &Cx<App>) -> Scheme {
    s.settings.theme.scheme(cx.system_dark())
}

/// The window's width as last laid out, so narrow rules hold from the first frames.
pub(crate) fn window_width(s: &App) -> f32 {
    rune::memo::layout_size("app").map_or(s.window_width, |size| size[0])
}

pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let scheme = scheme_of(s, cx);
    s.seen_scheme.set(scheme);
    let look = theme::look(scheme);
    let k = &look.kit;
    let mut layer: Vec<El> = Vec::new();
    let body = SplitView::new(reader(s, &look))
        .sidebar(&s.sidebar, |s: &mut App| &mut s.sidebar, sidebar::pane(s, &look))
        .toolbar(toolbar::toolbar(s, &look, &mut layer))
        .titlebar_inset(cx.titlebar_inset())
        .frame_inset(0.0)
        .build(&look.shell);
    let mut root = col()
        .key("app")
        .w_full()
        .h_full()
        .on_layout(|s: &mut App, r: [f32; 4]| s.set_window_size((r[2], r[3])))
        .fill(look.s.bar)
        .child(col().w_full().grow(1.0).min_h(0.0).child(body))
        .child(status::status_bar(s, &look, &mut layer));
    if s.phase == Phase::Ready {
        for o in s.layout.overlays.clone() {
            if let Some(e) = overlay(s, o, &look, cx) {
                root = root.child(e.absolute(0.0, 0.0));
            }
        }
        if let Some(p) = peek::layer(s, &look) {
            root = root.child(p);
        }
    }
    root = root.children(selection::toolbar(s, k, cx));
    if let Some(t) = conversation::toasts(s, k) {
        root = root.child(t);
    }
    if s.palette.open {
        root = root.child(palette::palette_overlay(s, k).absolute(0.0, 0.0));
    }
    if s.phase == Phase::Ready {
        filter::chips_popover(s, k, &mut layer);
        diff::file_menu(s, k, &mut layer);
    }
    root.children(layer)
}

/// One overlay over the reader; `None` for overlays whose module draws nothing yet.
fn overlay(s: &App, o: Overlay, look: &Look, cx: &mut Cx<App>) -> Option<El> {
    let k = &look.kit;
    let panel = match o {
        Overlay::Files => side_panel(o, sidebar::sidebar(s, look), 380.0, k),
        Overlay::Map => diagram::map_overlay(s, k, cx),
        Overlay::Conversation => conversation::conversation(s, k)?,
        Overlay::Tour => return tour::tour(s, k),
        Overlay::Picker => picker::picker(s, k)?,
        Overlay::Composer => return composer::composer(s, k),
        Overlay::Finish => finish::finish(s, k)?,
    };
    Some(
        col()
            .key(format!("scrim-{}", overlay_key(o)))
            .w_full()
            .h_full()
            .pt(TOOLBAR_HEIGHT)
            .pb(STATUS_HEIGHT)
            .fill(k.alpha(k.c.scrim, 0.5))
            .on_press(move |s: &mut App| {
                s.close_overlay(o);
            })
            .child(panel),
    )
}

/// `overlay-files`, `overlay-map`, `overlay-finish-review`.
pub(crate) fn overlay_key(o: Overlay) -> String {
    format!("overlay-{}", o.label().to_lowercase().replace(' ', "-"))
}

/// A floating panel at the reader's left edge with a title row and a close button.
fn side_panel(o: Overlay, content: El, width: f32, k: &Kit) -> El {
    let (key, title) = (overlay_key(o), o.label());
    let panel = col()
        .key(key.clone())
        .role("dialog")
        .label(title.to_owned())
        .w(width)
        .h_full()
        .gap(6.0)
        .padding(8.0)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_base)
        .on_press(|_: &mut App| {})
        .child(
            row().w_full().items_center().child(label(title, k).grow(1.0)).child(
                rune_kit::Button::new("Close (Esc)")
                    .variant(rune_kit::ButtonVariant::Ghost)
                    .on_press(move |s: &mut App| {
                        s.close_overlay(o);
                    })
                    .build(k)
                    .key(format!("{key}-close")),
            ),
        )
        .child(col().w_full().grow(1.0).min_h(0.0).child(content));
    k.elevate(panel, &k.elevation.overlay)
}

fn label(t: impl Into<String>, k: &Kit) -> El {
    text(t.into()).font(k.ty.ui).color(k.c.text_base)
}

fn muted(t: impl Into<String>, k: &Kit) -> El {
    text(t.into()).font(k.ty.small).color(k.c.text_muted)
}

fn list_row(key: String, k: &Kit, s: &App) -> El {
    let p = pad(s);
    k.interactive(
        row()
            .key(key)
            .role("button")
            .w_full()
            .px(8.0)
            .py(p * 0.6)
            .gap(8.0)
            .items_center()
            .radius(k.radius.sm),
    )
}

fn lines_label(a: &rain_core::Anchor) -> String {
    match a.lines() {
        (s, e) if s == e => format!("L{s}"),
        (s, e) => format!("L{s}-{e}"),
    }
}

// ----- the reader -----

/// The reader: the tab strip, banners, then the Overview or the find bar, filter and diff.
fn reader(s: &App, look: &Look) -> El {
    let k = &look.kit;
    let pane = col()
        .key("main")
        .w_full()
        .h_full()
        .min_w(0.0)
        .min_h(0.0)
        .fill(look.s.reader)
        .on_layout(|s: &mut App, r: [f32; 4]| s.set_diff_width(r[2]));
    let pane = match &s.phase {
        Phase::Loading(msg) => pane
            .items_center()
            .justify_center()
            .gap(8.0)
            .child(label("Loading", k))
            .child(muted(msg.clone(), k)),
        Phase::Error(msg) => pane.padding(24.0).child(
            col()
                .key("error")
                .role("alert")
                .gap(8.0)
                .padding(16.0)
                .radius(k.radius.lg)
                .fill(k.c.danger.bg)
                .border(1.0, k.c.danger.border)
                .child(text("Could not load the diff").font(k.ty.ui_medium).color(k.c.danger.fg))
                .child(text(msg.clone()).key("error-text").font(k.ty.mono).color(k.c.text_base).wrap())
                .child(muted("Fix the repo or refs, then press Shift+R to reload.", k)),
        ),
        Phase::Ready if s.layout.is_zoomed() => ready(s, k, pane),
        Phase::Ready => ready(s, k, pane.child(tabs::strip(s, look))),
    };
    let line = look.s.hairline;
    let top = el()
        .key("reader-top-line")
        .pin(Some(0.0), Some(0.0), Some(0.0), None)
        .h(1.0)
        .fill(line)
        .a11y_hidden();
    let pane = pane.child(top);
    if !s.sidebar.is_open() {
        return pane;
    }
    pane.child(
        el().key("reader-side-line")
            .pin(Some(0.0), Some(0.0), None, Some(0.0))
            .w(1.0)
            .fill(line)
            .a11y_hidden(),
    )
}

/// A 1 point line across its parent.
fn hairline(c: Color) -> El {
    el().w_full().h(1.0).fill(c).a11y_hidden()
}

fn ready(s: &App, k: &Kit, mut pane: El) -> El {
    if !s.overview_issues.is_empty() {
        let more = match s.overview_issues.len() {
            1 => String::new(),
            n => format!(" (+{} more)", n - 1),
        };
        pane = pane.child(messages::banner(
            "overview-issues",
            format!("Overview not loaded: {}{more}", s.overview_issues[0]),
            messages::Tone::Warning,
            None,
            k,
        ));
    }
    for strip in finish::reader_strips(s, k) {
        pane = pane.child(strip);
    }
    if s.reader_tab() == crate::app::ReaderTab::Overview {
        return pane.child(overview::page(s, k));
    }
    if s.find.open {
        pane = pane.child(find::find_bar(s, k));
    }
    if let Some(bar) = filter::filter_bar(s, k) {
        pane = pane.child(bar);
    }
    if let Some(card) = board::single_card(s, k) {
        return pane.child(card);
    }
    if s.active().files().is_empty() {
        let msg = if s.filter_active() {
            "No files match the filter. Esc clears it."
        } else {
            "No changes."
        };
        return pane.child(col().w_full().grow(1.0).items_center().justify_center().child(muted(msg, k)));
    }
    pane.child(
        row()
            .key("diff-area")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .child(
                col()
                    .key("diff-col")
                    .w(0.0)
                    .grow(1.0)
                    .min_w(0.0)
                    .min_h(0.0)
                    .child(diff::diff_view(s, k)),
            )
            .child(ruler::ruler(s, k)),
    )
}
