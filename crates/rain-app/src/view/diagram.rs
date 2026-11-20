//! The Map, a view inside the reader island (like the Overview): the overview's diagrams in the
//! Clean style, one tab per view (names never repeat), nodes shaded by how much of their files
//! is viewed and badged with their threads, a side pane for the selected node (its files,
//! threads, Comment / Ask), and a legend. Lanes stack under [`STACK_WIDTH`].

use rain_core::command::Command;
use rain_diagram::{Diagram, DiagramStyle, Zoom};
use rune::{Cursor, Cx, El, Tx, col, el, row, text};
use rune_kit::Kit;

use super::icons;
use super::lab::{self, Slot};
use super::muted;
use super::overview::{artifact_composer, dot, thread_card};
use super::ui::{Activate, Ghost, outline, sr};
use crate::app::{App, ComposeMode, MapTab, Overlay};
use crate::theme::{TYPE, radius, space};

pub(super) const PANE_WIDTH: f32 = 300.0;
/// Below this width the Map stacks its lanes one under another.
pub(crate) const STACK_WIDTH: f32 = 700.0;

/// A segmented switch on the well: one item per label, `on` lifted, `keys` in each tooltip.
fn segments(
    key: &str,
    labels: &[String],
    on: Option<usize>,
    keys: Option<&str>,
    k: &Kit,
    pick: impl Fn(usize) -> Box<dyn Fn(&mut App)>,
) -> El {
    let (surf, _) = sr(k);
    let mut seg = row()
        .key(key.to_owned())
        .role("tablist")
        .padding(2.0)
        .radius(7.0)
        .fill(surf.well)
        .border(1.0, surf.hairline);
    for (i, l) in labels.iter().enumerate() {
        let active = on == Some(i);
        let f = pick(i);
        let mut t = row()
            .key(format!("{key}-{i}"))
            .role("tab")
            .label(l.clone())
            .checked(active)
            .h(24.0)
            .px(10.0)
            .items_center()
            .radius(radius::SMALL)
            .cursor(Cursor::Pointer)
            .focusable(true)
            .child(
                text(l.clone())
                    .font(TYPE.small)
                    .color(if active { k.c.text_base } else { k.c.text_muted })
                    .a11y_hidden(),
            )
            .on_press(move |s: &mut App| f(s));
        if let Some(keys) = keys {
            t = t.help(format!("{l}  {keys}"));
        }
        t = if active {
            t.fill(surf.card_head).border(1.0, surf.card_border)
        } else {
            k.interactive(t)
        };
        seg = seg.child(t);
    }
    seg
}

/// The Map in the reader: its toolbar, the canvas with the node pane beside it, the legend.
pub(super) fn map_view(s: &App, k: &Kit, cx: &mut Cx<App>) -> El {
    let (surf, roles) = sr(k);
    let style = s.seen_scheme.get().diagram();
    let targets = s.overview.as_ref().map(|l| l.targets.clone()).unwrap_or_default();
    let names = rain_overview::tab_names(&targets);
    let tabs_of = s.map_tabs();
    let labels: Vec<String> = tabs_of
        .iter()
        .map(|t| match *t {
            MapTab::Lab(i) => rain_lab::registry()[i].name().to_owned(),
            MapTab::View(i) => names.get(i).cloned().unwrap_or_default(),
        })
        .collect();
    let at = tabs_of.iter().position(|&t| t == s.map_tab());
    let chord = |c: Command| c.chord().map(|k| k.display()).unwrap_or_default();
    let view_keys = format!("{} {} switch views", chord(Command::NextMapView), chord(Command::PrevMapView));
    let tabs = segments("map-tabs", &labels, at, Some(&view_keys), k, move |i| {
        let t = tabs_of[i];
        Box::new(move |s: &mut App| s.pick_map_tab(t))
    });
    if s.diagram.lab_on
        && let Some(stage) = lab::stage(s, Slot::Map, cx.is_reduced_motion(), &style, k, &surf, &roles)
    {
        let header = row()
            .key("diagram-header")
            .w_full()
            .h(space::READER_TOOL)
            .pl(10.0)
            .pr(8.0)
            .gap(10.0)
            .items_center()
            .child(
                row()
                    .key("map-tabs-scroll")
                    .grow(1.0)
                    .min_w(0.0)
                    .scroll_x()
                    .scroll_edge_fade(24.0)
                    .child(tabs),
            )
            .child(
                el().pin(Some(0.0), None, Some(0.0), Some(0.0))
                    .h(1.0)
                    .fill(surf.hairline)
                    .a11y_hidden(),
            );
        return col()
            .key("overlay-map")
            .role("region")
            .label("Map")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .child(header)
            .child(col().key("map-lab").w_full().grow(1.0).min_h(0.0).child(stage));
    }
    let zi = match s.diagram.zoom {
        Zoom::Fit => Some(0),
        Zoom::P100 => Some(1),
        Zoom::P150 => Some(2),
        Zoom::Custom(_) => None,
    };
    let zoom = segments("map-zoom", &["Fit".into(), "100%".into(), "150%".into()], zi, None, k, |i| {
        Box::new(move |s: &mut App| {
            s.diagram.zoom = match i {
                0 => Zoom::Fit,
                1 => Zoom::P100,
                _ => Zoom::P150,
            }
        })
    });
    let (w, h) = cx.layout("diagram-body").map_or((1100.0, 700.0), |r| (r[2], r[3]));
    let base = s.layouts.get(s.diagram.target).and_then(Option::as_ref);
    let stacked = base.filter(|_| w < STACK_WIDTH).and_then(|l| l.stacked());
    let mut header = row()
        .key("diagram-header")
        .w_full()
        .h(space::READER_TOOL)
        .pl(10.0)
        .pr(8.0)
        .gap(10.0)
        .items_center()
        // Many views overflow the header: the tabs scroll sideways (wheel or drag), fading at
        // the cut edge, and the zoom stays put at the right.
        .child(
            row()
                .key("map-tabs-scroll")
                .grow(1.0)
                .min_w(0.0)
                .scroll_x()
                .scroll_edge_fade(24.0)
                .child(tabs),
        );
    if let Some((_, hidden)) = &stacked {
        header = header.child(muted(format!("Lanes stacked · {hidden} link(s) hidden"), k).key("map-stacked"));
    }
    header = header.child(zoom).child(
        el().pin(Some(0.0), None, Some(0.0), Some(0.0))
            .h(1.0)
            .fill(surf.hairline)
            .a11y_hidden(),
    );
    let body = match (&stacked, base) {
        (_, None) => muted("Nothing to draw for this view.", k).padding(24.0),
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
            .fill(style.background)
            .on_layout(|s: &mut App, r: [f32; 4]| s.map_resized(r[2]))
            .child(body),
    );
    if let Some(node) = &s.diagram.selected {
        main = main.child(side_pane(s, node, k));
    }
    col()
        .key("overlay-map")
        .role("region")
        .label("Map")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(header)
        .child(main)
        .child(legend(&style, k))
}

fn diagram(s: &App, l: &rain_diagram::Layout, style: DiagramStyle, (w, h): (f32, f32), k: &Kit) -> El {
    Diagram::new(l)
        .style(style)
        .hover(s.diagram.hover.clone())
        .zoom(s.diagram.zoom)
        .fit_grow(2.0)
        .fit_min(Zoom::READABLE)
        .viewport(w, h)
        .marks(s.map_marks())
        .selected(s.diagram.selected.clone())
        .on_event(|s: &mut App, e, tx: &mut Tx| s.diagram_event(e, tx))
        .build(k)
}

/// What the colours and marks mean, on the well at the canvas's foot.
fn legend(st: &DiagramStyle, k: &Kit) -> El {
    let (surf, _) = sr(k);
    let swatch = |c: rune::Color| el().w(10.0).h(10.0).radius(2.0).border(1.5, c);
    let item = |e: El, t: &str| {
        row()
            .gap(6.0)
            .items_center()
            .child(e)
            .child(text(t.to_owned()).font(TYPE.small).color(k.c.text_muted))
    };
    row()
        .key("map-legend")
        .role("note")
        .label("Legend")
        .w_full()
        .h(38.0)
        .px(12.0)
        .gap(16.0)
        .items_center()
        .fill(surf.card_head)
        .child(item(swatch(st.added), "added"))
        .child(item(swatch(st.modified), "modified"))
        .child(item(swatch(st.removed), "removed"))
        .child(item(el().w(14.0).h(3.0).radius(1.5).fill(st.viewed), "share viewed"))
        .child(item(el().w(10.0).h(10.0).radius(2.0).fill(st.thread.alpha(0.4)), "threads"))
        .child(item(el().w(10.0).h(10.0).radius(2.0).border(2.0, st.foreground), "selected"))
        .child(
            text(crate::keys::with_hints(
                "Click a node to select it, again to open its files",
                &[(Command::Comment, "comment"), (Command::Ask, "ask")],
            ))
            .font(TYPE.small)
            .color(k.c.text_faint),
        )
        .child(
            el().pin(Some(0.0), Some(0.0), Some(0.0), None)
                .h(1.0)
                .fill(surf.hairline)
                .a11y_hidden(),
        )
}

/// The selected node: its kind and name, its status and files with their viewed state, its
/// threads, then Open first hunk, Comment and Ask Claude at the foot.
fn side_pane(s: &App, node: &str, k: &Kit) -> El {
    let (surf, r) = sr(k);
    let n = s
        .overview
        .as_ref()
        .and_then(|l| l.document.graph.nodes.iter().find(|n| n.id == node).cloned());
    let pane = col()
        .key("map-pane")
        .role("complementary")
        .label("Selected node")
        .w(PANE_WIDTH)
        .h_full()
        .fill(surf.panel)
        .child(
            el().pin(Some(0.0), Some(0.0), None, Some(0.0))
                .w(1.0)
                .fill(surf.hairline)
                .a11y_hidden(),
        );
    let Some(n) = n else {
        return pane.child(muted(format!("{node} is not in this overview"), k).padding(14.0));
    };
    let threads = s.node_threads(node);
    let kind_badge = row()
        .h(18.0)
        .px(5.0)
        .items_center()
        .radius(radius::CHIP)
        .fill(surf.well)
        .border(1.0, surf.hairline)
        .child(
            text(n.kind.chars().take(3).collect::<String>().to_uppercase())
                .font(TYPE.mark)
                .color(k.c.text_muted),
        );
    let mut title = row()
        .w_full()
        .gap(8.0)
        .items_center()
        .child(kind_badge)
        .child(text(n.label.clone()).font(TYPE.title).color(k.c.text_base).shrink().truncate());
    if !threads.is_empty() {
        title = title.child(dot(threads.len(), k));
    }
    title = title.child(el().grow(1.0)).child(
        Ghost::new("map-pane-close")
            .icon(&icons::CLOSE)
            .help("Close  Esc")
            .height(24.0)
            .build(k)
            .on_activate(|s: &mut App| {
                s.close_map_pane();
            }),
    );
    let files_n = n.files.len();
    let mut top = col()
        .key("map-pane-body")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .padding(14.0)
        .gap(10.0)
        .scroll(rune::ScrollSpec { stick_bottom: false })
        .child(title)
        .child(muted(
            format!("{} · {files_n} file{}", n.kind, if files_n == 1 { "" } else { "s" }),
            k,
        ));
    let mut files = col().key("map-files").w_full().gap(2.0);
    for (i, f) in n.files.iter().enumerate() {
        let g = s.index_of(&f.path);
        let viewed = g.is_some_and(|g| s.is_viewed(g));
        let path = f.path.clone();
        files = files.child(
            k.interactive(
                row()
                    .key(format!("map-file-{i}"))
                    .w_full()
                    .h(26.0)
                    .px(6.0)
                    .gap(8.0)
                    .items_center()
                    .radius(radius::BUTTON)
                    .fill(surf.well)
                    .cursor(Cursor::Pointer)
                    .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                        s.close_overlay(Overlay::Map);
                        s.goto_cite(&path, None, rain_core::Side::New, tx);
                    }),
            )
            .child(if viewed {
                icons::draw(&icons::CHECK, 14.0, r.accent)
            } else {
                el().size(10.0, 10.0).radius(5.0).border(1.5, k.c.icon_faint).a11y_hidden()
            })
            .child(
                text(f.path.clone())
                    .font(TYPE.micro_mono)
                    .color(if g.is_some() { k.c.text_base } else { k.c.text_faint })
                    .truncate_head()
                    .shrink(),
            ),
        );
    }
    top = top.child(files);
    if !threads.is_empty() {
        top = top.child(text("Threads").font(TYPE.label).color(k.c.text_base));
    }
    for t in threads {
        top = top.child(thread_card(s, t, k));
    }
    if let Some(d) = &s.brief.composer
        && d.target.node.as_deref() == Some(node)
    {
        top = top.child(artifact_composer(s, d, k));
    }
    let (id, ask) = (node.to_owned(), node.to_owned());
    let foot = row()
        .key("map-pane-foot")
        .w_full()
        .h(44.0)
        .px(12.0)
        .gap(6.0)
        .items_center()
        .child(
            k.interactive_with(
                row()
                    .key("map-open")
                    .role("button")
                    .label("Open first hunk")
                    .h(26.0)
                    .px(10.0)
                    .center()
                    .radius(radius::BUTTON)
                    .fill(r.accent)
                    .cursor(Cursor::Pointer)
                    .child(text("Open first hunk").font(TYPE.small).color(r.on_accent).a11y_hidden()),
                k.c.contrast_hover,
                k.c.contrast_pressed,
            )
            .on_press_tx(|s: &mut App, tx: &mut Tx| s.open_selected_node(tx)),
        )
        .child(
            outline(
                "map-comment",
                "Comment",
                Command::Comment.chord().map(|c| c.display()).as_deref(),
                26.0,
                k,
            )
            .on_activate(move |s: &mut App| s.comment_on_node(&id, ComposeMode::Comment)),
        )
        .child(
            outline(
                "map-ask",
                "Ask Claude",
                Command::Ask.chord().map(|c| c.display()).as_deref(),
                26.0,
                k,
            )
            .on_activate(move |s: &mut App| s.comment_on_node(&ask, ComposeMode::Question)),
        )
        .child(
            el().pin(Some(0.0), Some(0.0), Some(0.0), None)
                .h(1.0)
                .fill(surf.hairline)
                .a11y_hidden(),
        );
    pane.child(top).child(foot)
}
