//! The window: the top bar and status line on the ground, and between them the islands (the
//! list, the reader and side panels as rounded panels with an 8 point gap), overlays above, in
//! the look from theme.rs. Each feature draws its own part from its module.

mod board;
mod composer;
mod conversation;
mod diagram;
mod diff;
mod filter;
mod find;
mod finish;
pub(crate) mod icons;
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
pub(crate) mod ui;
mod wholefile;

use rune::{Color, Cursor, Cx, Drag, El, Phase as DragPhase, col, el, row, text};
use rune_kit::Kit;

use crate::app::{App, Overlay, Phase};
use crate::theme::{self, Look, Scheme, radius, space};

/// The top bar's height; overlays start under it.
const TOOLBAR_HEIGHT: f32 = space::TOP_BAR;
const STATUS_HEIGHT: f32 = space::STATUS;

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
    let top = toolbar::top_bar(s, &look, &mut layer);
    let mut root = col()
        .key("app")
        .w_full()
        .h_full()
        .on_layout(|s: &mut App, r: [f32; 4]| s.set_window_size((r[2], r[3])))
        .fill(look.s.bar)
        .child(top)
        .child(islands(s, &look, cx))
        .child(status::status_bar(s, &look, &mut layer));
    if s.phase == Phase::Ready {
        for o in s.layout.overlays.clone() {
            if let Some(e) = overlay(s, o, &look) {
                root = root.child(e.absolute(0.0, 0.0));
            }
        }
        if let Some(p) = peek::layer(s, &look) {
            root = root.child(p);
        }
    }
    root = root.children(selection::toolbar(s, k, cx));
    if s.palette.open {
        root = root.child(palette::palette_overlay(s, k).absolute(0.0, 0.0));
    }
    if s.phase == Phase::Ready {
        filter::chips_popover(s, k, &mut layer);
        diff::file_menu(s, k, &mut layer);
    }
    root.children(layer)
}

/// A rounded panel on the ground: clipped to its 10 point corners, with a faint edge.
pub(crate) fn island(e: El, fill: Color, look: &Look) -> El {
    e.radius(radius::ISLAND)
        .fill(fill)
        .clip_rounded(radius::ISLAND)
        .border(1.0, look.kit.alpha(Color::BLACK, 0.25))
}

/// The list island, the gap that resizes it, and the reader island.
fn islands(s: &App, look: &Look, cx: &mut Cx<App>) -> El {
    let mut body = row().key("body").w_full().grow(1.0).min_h(0.0).px(space::GAP);
    if s.sidebar.is_open() {
        body = body
            .child(island(sidebar::pane(s, look), look.s.panel, look).w(s.sidebar.width()).h_full())
            .child(grip(look));
    }
    body = body.child(island(reader(s, look, cx), look.s.reader, look).grow(1.0).min_w(0.0).h_full());
    if s.phase == Phase::Ready
        && s.layout.is_open(Overlay::Conversation)
        && let Some(c) = conversation::conversation(s, &look.kit)
    {
        body = body
            .child(el().w(space::GAP).a11y_hidden())
            .child(island(c, look.s.panel, look).h_full());
    }
    body
}

/// The gap between the list and the reader: dragging it resizes the list (220–520 points);
/// hovering shows a line down its middle; a double press resets the width.
fn grip(look: &Look) -> El {
    let line = el()
        .key("list-grip-line")
        .pin(Some(3.0), Some(8.0), None, Some(8.0))
        .w(2.0)
        .radius(1.0)
        .fill(look.s.strong_line)
        .group_opacity(0.0, 1.0)
        .a11y_hidden();
    el().key("list-grip")
        .role("separator")
        .label("Resize the file list")
        .help("Drag to resize the list (220–520)")
        .w(space::GAP)
        .h_full()
        .cursor(Cursor::ResizeColumn)
        .hover_group()
        .child(line)
        .on_drag(|s: &mut App, d: &mut Drag, tx| match d.phase {
            DragPhase::Down => d.data = s.sidebar.width(),
            DragPhase::Move => {
                tx.instant();
                s.sidebar.drag_to(d.data + d.dx);
            }
            DragPhase::Up => {
                if d.cancelled() {
                    s.sidebar.set_width(d.data);
                    s.sidebar.set_open(true);
                }
                s.settings.list_open = s.sidebar.is_open();
                s.save_settings();
                s.apply_split();
            }
        })
        .on_double_press(|s: &mut App| {
            s.sidebar.reset_width();
            s.apply_split();
        })
}

/// A whole-reader state (loading, error, empty): a 520 point box in the middle of the island.
fn state_box(content: El) -> El {
    col().w_full().grow(1.0).items_center().justify_center().child(content.w(520.0))
}

/// One overlay over the reader; `None` for overlays whose module draws nothing yet.
fn overlay(s: &App, o: Overlay, look: &Look) -> Option<El> {
    let k = &look.kit;
    let panel = match o {
        Overlay::Files => side_panel(o, sidebar::sidebar(s, look), 300.0, look),
        // The Map is a view inside the reader island.
        Overlay::Map => return None,
        // The Conversation is a third island beside the reader.
        Overlay::Conversation => return None,
        Overlay::Tour => return tour::tour(s, k),
        Overlay::Picker => picker::picker(s, k)?,
        Overlay::Composer => return composer::composer(s, k),
        Overlay::Finish => finish::finish(s, k)?,
    };
    // The floating files leave the reader readable beside them.
    let scrim = if o == Overlay::Files {
        Color::TRANSPARENT
    } else {
        k.alpha(k.c.scrim, 0.58)
    };
    Some(
        col()
            .key(format!("scrim-{}", overlay_key(o)))
            .w_full()
            .h_full()
            .pt(TOOLBAR_HEIGHT)
            .pb(STATUS_HEIGHT)
            .fill(scrim)
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

/// A floating island over the reader's left edge with a title row and a close button.
fn side_panel(o: Overlay, content: El, width: f32, look: &Look) -> El {
    let k = &look.kit;
    let (key, title) = (overlay_key(o), o.label());
    let close = ui::Ghost::new(format!("{key}-close"))
        .icon(&icons::CLOSE)
        .help("Close  Esc")
        .height(24.0)
        .build(k)
        .on_press(move |s: &mut App| {
            s.close_overlay(o);
        });
    let panel = col()
        .key(key.clone())
        .role("dialog")
        .label(title.to_owned())
        .w(width)
        .grow(1.0)
        .min_h(0.0)
        .radius(radius::ISLAND)
        .fill(look.s.panel)
        .border(1.0, look.s.card_border)
        .shadow(18.0, 40.0, look.r.shadow)
        .clip_rounded(radius::ISLAND)
        .on_press(|_: &mut App| {})
        .child(
            row()
                .w_full()
                .h(38.0)
                .pl(12.0)
                .pr(8.0)
                .items_center()
                .child(text(title.to_owned()).font(theme::TYPE.label).color(k.c.text_base).grow(1.0))
                .child(close),
        )
        .child(col().w_full().grow(1.0).min_h(0.0).child(content));
    col().h_full().pl(14.0).pt(12.0).pb(14.0).child(panel)
}

fn muted(t: impl Into<String>, k: &Kit) -> El {
    text(t.into()).font(k.ty.small).color(k.c.text_muted)
}

// ----- the reader -----

/// The reader: the tab strip, banners, then the Overview or the find bar, filter and diff.
fn reader(s: &App, look: &Look, cx: &mut Cx<App>) -> El {
    let pane = reader_pane(s, look, cx);
    match conversation::toasts(s, &look.kit) {
        // Toasts stack inside the reader, over its bottom right, clear of the status line and
        // of the Map's node pane.
        Some(t) => {
            let right = if s.layout.is_open(Overlay::Map) && s.diagram.selected.is_some() {
                diagram::PANE_WIDTH + 14.0
            } else {
                14.0
            };
            pane.child(t.pin(None, None, Some(right), Some(14.0)))
        }
        None => pane,
    }
}

fn reader_pane(s: &App, look: &Look, cx: &mut Cx<App>) -> El {
    let k = &look.kit;
    let pane = col()
        .key("main")
        .w_full()
        .h_full()
        .min_w(0.0)
        .min_h(0.0)
        .fill(look.s.reader)
        .on_layout(|s: &mut App, r: [f32; 4]| s.set_diff_width(r[2]));
    match &s.phase {
        Phase::Loading(msg) => pane.child(state_box(
            col()
                .key("loading")
                .gap(4.0)
                .child(
                    row()
                        .w_full()
                        .h(3.0)
                        .radius(2.0)
                        .fill(look.s.well)
                        .child(el().w(208.0).h(3.0).ml(104.0).radius(2.0).fill(look.r.accent)),
                )
                .child(text("Loading").font(theme::TYPE.h2).color(k.c.text_base).pt(10.0))
                .child(muted(msg.clone(), k)),
        )),
        Phase::Error(msg) => pane.child(state_box(
            col()
                .key("error")
                .role("alert")
                .gap(8.0)
                .child(icons::draw(&icons::WARN, 24.0, k.c.danger.fg))
                .child(text("Could not load the diff").font(theme::TYPE.h2).color(k.c.text_base))
                .child(
                    text(msg.clone())
                        .key("error-text")
                        .font(theme::TYPE.small.mono())
                        .color(k.c.danger.fg)
                        .wrap()
                        .w_full()
                        .px(12.0)
                        .py(10.0)
                        .radius(radius::BUTTON)
                        .fill(look.s.well)
                        .border(1.0, k.c.danger.border),
                )
                .child(
                    row()
                        .gap(4.0)
                        .items_center()
                        .child(muted("Fix the repo or refs, then press", k))
                        .child(ui::kbd("Shift+R", k))
                        .child(muted("to reload.", k)),
                ),
        )),
        Phase::Ready if s.layout.is_zoomed() => ready(s, k, pane, cx),
        Phase::Ready => ready(s, k, pane.child(tabs::strip(s, look)), cx),
    }
}

fn ready(s: &App, k: &Kit, mut pane: El, cx: &mut Cx<App>) -> El {
    if !s.overview_issues.is_empty() {
        let more = match s.overview_issues.len() {
            1 => String::new(),
            n => format!(" (+{} more)", n - 1),
        };
        pane = pane.child(messages::inset_banner(
            "overview-issues",
            format!("Overview not loaded: {}{more}", s.overview_issues[0]),
            messages::Tone::Warning,
            &[],
            k,
        ));
    }
    for strip in finish::reader_strips(s, k) {
        pane = pane.child(strip);
    }
    if s.layout.is_open(Overlay::Map) && s.has_map() {
        return pane.child(diagram::map_view(s, k, cx));
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
        let (title, msg) = if s.filter_active() {
            ("No files match the filter.", "Esc clears it.")
        } else {
            ("No changes.", "Rain reloads when new commits arrive.")
        };
        return pane.child(state_box(
            col()
                .key("empty")
                .gap(6.0)
                .child(icons::draw(&icons::FILE, 24.0, k.c.text_faint))
                .child(text(title).font(theme::TYPE.h2).color(k.c.text_base))
                .child(muted(msg, k)),
        ));
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
