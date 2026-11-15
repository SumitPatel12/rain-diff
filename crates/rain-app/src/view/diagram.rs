//! The Map overlay: the overview's diagrams over the reader in the Clean style, one tab per
//! view (names never repeat), nodes shaded by how much of their files is viewed and badged with
//! their threads, a side pane for the selected node (its files, threads, Comment / Ask), and a
//! legend. Lanes stack under [`STACK_WIDTH`].

use rain_diagram::{Diagram, DiagramStyle, Zoom};
use rune::{Cx, El, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, SegmentedControl};

use super::muted;
use super::overview::{artifact_composer, dot, thread_card};
use crate::app::{App, ComposeMode, Overlay};

const PANE_WIDTH: f32 = 320.0;
/// Below this width the Map stacks its lanes one under another.
pub(crate) const STACK_WIDTH: f32 = 700.0;

pub(super) fn map_overlay(s: &App, k: &Kit, cx: &mut Cx<App>) -> El {
    let style = s.seen_scheme.get().diagram();
    let targets = s.overview.as_ref().map(|l| l.targets.clone()).unwrap_or_default();
    let names = rain_overview::tab_names(&targets);
    let mut picker = row().key("map-tabs").role("tablist").gap(4.0).items_center();
    for (i, name) in names.iter().enumerate() {
        if s.layouts.get(i).is_none_or(Option::is_none) {
            continue;
        }
        let v = if i == s.diagram.target {
            ButtonVariant::Outline
        } else {
            ButtonVariant::Ghost
        };
        picker = picker.child(
            Button::new(name.clone())
                .variant(v)
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut App| s.pick_map_view(i))
                .build(k)
                .key(format!("target-{i}")),
        );
    }
    let zi = match s.diagram.zoom {
        Zoom::Fit => Some(0),
        Zoom::P100 => Some(1),
        Zoom::P150 => Some(2),
        Zoom::Custom(_) => None,
    };
    let zoom = SegmentedControl::new(&["Fit", "100%", "150%"], zi)
        .label("Zoom")
        .on_pick(|s: &mut App, i, _: &mut Tx| {
            s.diagram.zoom = match i {
                0 => Zoom::Fit,
                1 => Zoom::P100,
                _ => Zoom::P150,
            }
        })
        .build(k);
    let (w, h) = cx.layout("diagram-body").map_or((1400.0, 800.0), |r| (r[2], r[3]));
    let base = s.layouts.get(s.diagram.target).and_then(Option::as_ref);
    let stacked = base.filter(|_| w < STACK_WIDTH).and_then(|l| l.stacked());
    let mut header = row()
        .key("diagram-header")
        .w_full()
        .px(16.0)
        .py(8.0)
        .gap(12.0)
        .items_center()
        .fill(k.c.layer_01)
        .child(
            Button::new("Close (Esc)")
                .variant(ButtonVariant::Ghost)
                .on_press(|s: &mut App| {
                    s.close_overlay(Overlay::Map);
                })
                .build(k)
                .key("map-close"),
        )
        .child(text("Map").font(k.ty.ui_medium).color(k.c.text_base))
        .child(row().grow(1.0).shrink().child(picker));
    if let Some((_, hidden)) = &stacked {
        header = header.child(muted(format!("Lanes stacked · {hidden} link(s) hidden"), k).key("map-stacked"));
    }
    header = header.child(zoom);
    let body = match (&stacked, base) {
        (_, None) => muted("Nothing to draw for this view.", k),
        (Some((l, _)), _) => diagram(s, l, style, (w, h), k),
        (None, Some(l)) => diagram(s, l, style, (w, h), k),
    };
    let mut main = row().key("map-main").w_full().grow(1.0).min_h(0.0).child(
        col()
            .key("diagram-body")
            .w(0.0)
            .grow(1.0)
            .h_full()
            .min_w(0.0)
            .on_layout(|s: &mut App, r: [f32; 4]| s.map_resized(r[2]))
            .child(body),
    );
    if let Some(node) = &s.diagram.selected {
        main = main.child(side_pane(s, node, k));
    }
    let panel = col()
        .key("overlay-map")
        .role("dialog")
        .label("Map")
        .w_full()
        .h_full()
        .radius(k.radius.lg)
        .clip_rounded(k.radius.lg)
        .fill(style.background)
        .border(1.0, k.c.border_base)
        .on_press(|_: &mut App| {})
        .child(header)
        .child(main)
        .child(legend(&style, k));
    col().w_full().h_full().padding(16.0).child(k.elevate(panel, &k.elevation.overlay))
}

fn diagram(s: &App, l: &rain_diagram::Layout, style: DiagramStyle, (w, h): (f32, f32), k: &Kit) -> El {
    Diagram::new(l)
        .style(style)
        .hover(s.diagram.hover.clone())
        .zoom(s.diagram.zoom)
        .fit_grow(2.0)
        .viewport(w, h)
        .marks(s.map_marks())
        .selected(s.diagram.selected.clone())
        .on_event(|s: &mut App, e, tx: &mut Tx| s.diagram_event(e, tx))
        .build(k)
}

/// What the colours and marks mean.
fn legend(st: &DiagramStyle, k: &Kit) -> El {
    let swatch = |c: rune::Color| el().w(10.0).h(10.0).radius(2.0).fill(c);
    let item = |e: El, t: &str| row().gap(4.0).items_center().child(e).child(muted(t.to_owned(), k));
    row()
        .key("map-legend")
        .role("note")
        .label("Legend")
        .w_full()
        .px(16.0)
        .py(6.0)
        .gap(14.0)
        .items_center()
        .fill(k.c.layer_01)
        .child(item(swatch(st.added), "added"))
        .child(item(swatch(st.modified), "modified"))
        .child(item(swatch(st.removed), "removed"))
        .child(item(el().w(18.0).h(3.0).radius(1.5).fill(st.viewed), "share of files viewed"))
        .child(item(el().w(14.0).h(14.0).radius(7.0).fill(st.thread), "threads"))
        .child(item(el().w(12.0).h(12.0).radius(3.0).border(2.0, st.thread), "selected"))
        .child(muted(
            "Hover a node to see it here · click opens its first hunk · c / ? comment or ask",
            k,
        ))
}

/// The selected node: what it is, its files with their viewed state, its threads, and Comment /
/// Ask Claude.
fn side_pane(s: &App, node: &str, k: &Kit) -> El {
    let n = s
        .overview
        .as_ref()
        .and_then(|l| l.document.graph.nodes.iter().find(|n| n.id == node).cloned());
    let mut c = col()
        .key("map-pane")
        .role("complementary")
        .label("Selected node")
        .w(PANE_WIDTH)
        .h_full()
        .gap(8.0)
        .padding(12.0)
        .fill(k.c.layer_01)
        .scroll(rune::ScrollSpec { stick_bottom: false });
    let Some(n) = n else {
        return c.child(muted(format!("{node} is not in this overview"), k));
    };
    let threads = s.node_threads(node);
    let mut title = row()
        .w_full()
        .gap(8.0)
        .items_center()
        .child(text(n.label.clone()).font(k.ty.ui_medium).color(k.c.text_base).shrink().truncate());
    if !threads.is_empty() {
        title = title.child(dot(threads.len(), k));
    }
    title = title.child(el().grow(1.0)).child(
        Button::new("✕")
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Small)
            .help("Clear the selection")
            .on_press(|s: &mut App| s.diagram.selected = None)
            .build(k)
            .key("map-pane-close"),
    );
    c = c.child(title).child(muted(n.kind.clone(), k));
    let (id, ask) = (node.to_owned(), node.to_owned());
    c = c.child(
        row()
            .gap(4.0)
            .flex_wrap()
            .child(
                Button::new("Open first hunk")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Small)
                    .on_press_tx(|s: &mut App, tx: &mut Tx| s.open_selected_node(tx))
                    .build(k)
                    .key("map-open"),
            )
            .child(
                Button::new("Comment")
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Small)
                    .shortcut("c")
                    .on_press(move |s: &mut App| s.comment_on_node(&id, ComposeMode::Comment))
                    .build(k)
                    .key("map-comment"),
            )
            .child(
                Button::new("Ask Claude")
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Small)
                    .shortcut("?")
                    .on_press(move |s: &mut App| s.comment_on_node(&ask, ComposeMode::Question))
                    .build(k)
                    .key("map-ask"),
            ),
    );
    let mut files = col().key("map-files").w_full().gap(2.0);
    for (i, f) in n.files.iter().enumerate() {
        let g = s.index_of(&f.path);
        let mark = match g {
            Some(g) if s.is_viewed(g) => "✓",
            Some(_) => "○",
            None => "–",
        };
        let path = f.path.clone();
        files = files.child(
            row()
                .key(format!("map-file-{i}"))
                .w_full()
                .gap(6.0)
                .cursor(rune::Cursor::Pointer)
                .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                    s.close_overlay(Overlay::Map);
                    s.goto_cite(&path, None, rain_core::Side::New, tx);
                })
                .child(text(mark).font(k.ty.small).color(k.c.text_muted))
                .child(
                    text(f.path.rsplit('/').next().unwrap_or(&f.path).to_owned())
                        .font(k.ty.mono)
                        .color(if g.is_some() { k.c.text_base } else { k.c.text_faint })
                        .truncate()
                        .shrink(),
                ),
        );
    }
    c = c.child(files);
    for t in threads {
        c = c.child(thread_card(s, t, k));
    }
    if let Some(d) = &s.brief.composer
        && d.target.node.as_deref() == Some(node)
    {
        c = c.child(artifact_composer(s, d, k));
    }
    c
}
