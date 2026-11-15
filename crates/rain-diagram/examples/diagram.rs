//! The three PR Lens lenses of `fixtures/overview/sample-overview-schema.json` (and of
//! `diagram-kinds.json`, `kinds-*` scenes), drawn natively.
//! `cargo run -p rain-diagram --example diagram -- --snap out.png --scene model-light --scale 1.5`.
//! Keys: 1/2/3 switch lens, d document, t theme, 0 fit, = and - zoom. With `--features mermaid-compare` the
//! `mermaid-*` scenes draw the same graph through rune-mermaid.

use std::path::PathBuf;
use std::sync::OnceLock;

use rain_core::OverviewDocument;
use rain_core::overview::{GraphViewScope, OverviewGraph};
use rain_diagram::{Diagram, DiagramEvent, DiagramStyle, ItemId, Layout, Zoom};
use rune::tok::type_;
use rune::{AppSpec, Cx, El, Key, KeyEvent, Tx, col, row, text};
use rune_kit::Kit;

/// The window size `Zoom::Fit` fits into.
const SIZE: (f32, f32) = (1440.0, 900.0);
const DOCS: [&str; 2] = [
    include_str!("../../../fixtures/overview/sample-overview-schema.json"),
    include_str!("../../../fixtures/overview/diagram-kinds.json"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lens {
    Architecture,
    Flow,
    Model,
    #[cfg(feature = "mermaid-compare")]
    Mermaid(rain_core::overview::GraphLens),
}

struct Doc {
    graph: OverviewGraph,
    layouts: [Layout; 3],
}

struct Demo {
    docs: Vec<Doc>,
    doc: usize,
    lens: Lens,
    dark: Option<bool>,
    hover: Option<ItemId>,
    zoom: Zoom,
    last: String,
}

fn init() -> Demo {
    let docs = DOCS
        .iter()
        .map(|text| {
            let graph = serde_json::from_str::<OverviewDocument>(text).expect("fixture parses").graph;
            let layouts = [
                rain_diagram::architecture(&graph, &GraphViewScope::All),
                rain_diagram::data_flow(&graph, &graph.flows[0].id).expect("fixture has a flow"),
                rain_diagram::data_model(&graph, &GraphViewScope::All),
            ];
            Doc { graph, layouts }
        })
        .collect();
    Demo {
        docs,
        doc: 0,
        lens: Lens::Architecture,
        dark: None,
        hover: None,
        zoom: Zoom::P100,
        last: String::new(),
    }
}

fn view(s: &Demo, cx: &mut Cx<Demo>) -> El {
    let dark = s.dark.unwrap_or(cx.system_dark());
    let k = Kit::neutral(dark);
    let style = DiagramStyle::for_theme(dark);
    let (w, h) = SIZE;
    let doc = &s.docs[s.doc];
    let title = match s.lens {
        Lens::Architecture => "Architecture",
        Lens::Flow => "Data flow",
        Lens::Model => "Data model",
        #[cfg(feature = "mermaid-compare")]
        Lens::Mermaid(_) => "rune-mermaid comparison",
    };
    let header = row()
        .w_full()
        .px(16.0)
        .py(10.0)
        .gap(16.0)
        .fill(k.c.layer_01)
        .child(
            text(format!("{} · {title}", doc.graph.title))
                .font(type_::TITLE)
                .color(k.c.text_base),
        )
        .child(text(s.last.clone()).font(type_::BODY).color(k.c.text_muted));
    let body = match s.lens {
        #[cfg(feature = "mermaid-compare")]
        Lens::Mermaid(lens) => {
            let src = rain_diagram::mermaid::to_mermaid(&doc.graph, lens);
            let mstyle = rune_mermaid::DiagramStyle::neutral(dark).min_scale(0.0);
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .padding(16.0)
                .fill(style.background)
                .scroll_xy()
                .child(rune_mermaid::render(&src, &mstyle).unwrap_or_else(|| text(src)))
        }
        _ => {
            let layout = match s.lens {
                Lens::Flow => &doc.layouts[1],
                Lens::Model => &doc.layouts[2],
                _ => &doc.layouts[0],
            };
            let d = Diagram::new(layout)
                .style(style)
                .hover(s.hover.clone())
                .zoom(s.zoom)
                .viewport(w, h - 48.0)
                .on_event(|s: &mut Demo, e, _| match e {
                    DiagramEvent::Hover(h) => s.hover = h,
                    DiagramEvent::Zoom(z) => s.zoom = Zoom::Custom(z),
                    other => s.last = format!("{other:?}"),
                })
                .build(&k);
            col().w_full().grow(1.0).min_h(0.0).child(d)
        }
    };
    col().w_full().h_full().fill(style.background).child(header).child(body)
}

fn on_key(s: &mut Demo, e: &KeyEvent, _: &mut Tx) -> bool {
    let Key::Text(t) = &e.key else { return false };
    match t.as_str() {
        "1" => s.lens = Lens::Architecture,
        "2" => s.lens = Lens::Flow,
        "3" => s.lens = Lens::Model,
        "d" => s.doc = (s.doc + 1) % s.docs.len(),
        "t" => s.dark = Some(!s.dark.unwrap_or(true)),
        "0" => s.zoom = Zoom::Fit,
        "=" => s.zoom = Zoom::Custom(zoom_of(s) * 1.25),
        "-" => s.zoom = Zoom::Custom(zoom_of(s) / 1.25),
        _ => return false,
    }
    true
}

fn zoom_of(s: &Demo) -> f32 {
    match s.zoom {
        Zoom::Custom(k) => k,
        Zoom::P150 => 1.5,
        _ => 1.0,
    }
}

const MONO: [&str; 3] = [
    "JetBrainsMonoNerdFontMono-Regular.ttf",
    "JetBrainsMonoNerdFontMono-Medium.ttf",
    "JetBrainsMonoNerdFontMono-Bold.ttf",
];
const SANS: [&str; 3] = ["segoeui.ttf", "seguisb.ttf", "segoeuib.ttf"];

/// The installed JetBrains Mono and Segoe UI, as `rain_app::fonts` loads them.
fn fonts() -> (&'static [&'static [u8]], (&'static str, &'static str)) {
    static FONTS: OnceLock<Vec<&'static [u8]>> = OnceLock::new();
    let files = FONTS.get_or_init(|| {
        let dirs: Vec<PathBuf> = [
            std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Microsoft\\Windows\\Fonts")),
            std::env::var_os("WINDIR").map(|w| PathBuf::from(w).join("Fonts")),
        ]
        .into_iter()
        .flatten()
        .collect();
        let read = |n: &str| {
            dirs.iter()
                .find_map(|d| std::fs::read(d.join(n)).ok())
                .map(|b| &*Box::leak(b.into_boxed_slice()))
        };
        let mono: Vec<&'static [u8]> = MONO.iter().filter_map(|f| read(f)).collect();
        let sans: Vec<&'static [u8]> = SANS.iter().filter_map(|f| read(f)).collect();
        if mono.is_empty() || sans.is_empty() {
            Vec::new()
        } else {
            sans.into_iter().chain(mono).collect()
        }
    });
    if files.is_empty() {
        (&[], ("Inter", "ui-monospace, Inter"))
    } else {
        (files.as_slice(), ("Segoe UI", "JetBrainsMono Nerd Font Mono, ui-monospace"))
    }
}

fn main() {
    let (fonts, families) = fonts();
    rune::run(AppSpec {
        scenes: &[
            ("architecture", |s: &mut Demo| s.lens = Lens::Architecture),
            ("architecture-dark", |s: &mut Demo| {
                s.lens = Lens::Architecture;
                s.dark = Some(true)
            }),
            ("architecture-light", |s: &mut Demo| {
                s.lens = Lens::Architecture;
                s.dark = Some(false)
            }),
            ("architecture-hover", |s: &mut Demo| {
                s.lens = Lens::Architecture;
                s.dark = Some(true);
                s.hover = Some(ItemId::Node("voucher-service".into()))
            }),
            ("flow", |s: &mut Demo| s.lens = Lens::Flow),
            ("flow-dark", |s: &mut Demo| {
                s.lens = Lens::Flow;
                s.dark = Some(true)
            }),
            ("flow-light", |s: &mut Demo| {
                s.lens = Lens::Flow;
                s.dark = Some(false)
            }),
            ("model", |s: &mut Demo| s.lens = Lens::Model),
            ("model-dark", |s: &mut Demo| {
                s.lens = Lens::Model;
                s.dark = Some(true)
            }),
            ("model-light", |s: &mut Demo| {
                s.lens = Lens::Model;
                s.dark = Some(false)
            }),
            ("kinds-architecture", |s: &mut Demo| {
                s.doc = 1;
                s.dark = Some(true)
            }),
            ("kinds-flow", |s: &mut Demo| {
                s.doc = 1;
                s.lens = Lens::Flow;
                s.dark = Some(false)
            }),
            ("kinds-model", |s: &mut Demo| {
                s.doc = 1;
                s.lens = Lens::Model;
                s.dark = Some(true)
            }),
            ("fit", |s: &mut Demo| {
                s.doc = 1;
                s.lens = Lens::Architecture;
                s.dark = Some(true);
                s.zoom = Zoom::Fit
            }),
            #[cfg(feature = "mermaid-compare")]
            ("mermaid-architecture", |s: &mut Demo| {
                s.lens = Lens::Mermaid(rain_core::overview::GraphLens::Architecture);
                s.dark = Some(true)
            }),
            #[cfg(feature = "mermaid-compare")]
            ("mermaid-flow", |s: &mut Demo| {
                s.lens = Lens::Mermaid(rain_core::overview::GraphLens::DataFlow);
                s.dark = Some(true)
            }),
            #[cfg(feature = "mermaid-compare")]
            ("mermaid-model", |s: &mut Demo| {
                s.lens = Lens::Mermaid(rain_core::overview::GraphLens::DataModel);
                s.dark = Some(true)
            }),
        ],
        on_key: Some(on_key),
        fonts,
        families,
        ..AppSpec::new("Rain diagram", SIZE, init, view)
    });
}
