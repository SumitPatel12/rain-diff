use std::time::Instant;

use rain_core::overview::{GraphLens, GraphViewScope, OverviewDocument, OverviewGraph, RenderTarget};
use rune::{Cx, El, Env, Runtime};
use rune_kit::Kit;

use super::*;

const SCHEMA: &str = include_str!("../../../fixtures/overview/sample-overview-schema.json");
const KINDS: &str = include_str!("../../../fixtures/overview/diagram-kinds.json");
const SAMPLES: [&str; 4] = [
    SCHEMA,
    include_str!("../../../fixtures/overview/sample-overview.json"),
    include_str!("../../../fixtures/overview/sample-overview-r1.json"),
    KINDS,
];

fn graph(text: &str) -> OverviewGraph {
    serde_json::from_str::<OverviewDocument>(text).unwrap().graph
}

fn lenses(g: &OverviewGraph) -> Vec<Layout> {
    let mut out = vec![architecture(g, &GraphViewScope::All), data_model(g, &GraphViewScope::All)];
    out.extend(g.flows.iter().filter_map(|f| data_flow(g, &f.id)));
    for v in scope::flatten_views(&g.views) {
        let t = RenderTarget {
            lens: v.lens,
            view: Some(v.id.clone()),
            flow: None,
            title: v.title.clone(),
            depth: 0,
        };
        out.extend(render(g, &t));
    }
    out
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

#[test]
fn layouts_are_deterministic() {
    for s in SAMPLES {
        let g = graph(s);
        assert_eq!(lenses(&g), lenses(&g));
    }
}

#[test]
fn atlas_has_every_scoped_item() {
    let g = graph(SCHEMA);
    let a = architecture(&g, &GraphViewScope::All).atlas();
    for n in &g.nodes {
        assert!(a.nodes.contains_key(&n.id), "node {}", n.id);
    }
    for e in &g.edges {
        assert!(a.edges.contains_key(&e.id), "edge {}", e.id);
    }
    for l in &g.lanes {
        assert!(a.lanes.contains_key(&l.id), "lane {}", l.id);
    }
    for f in &g.flows {
        let a = data_flow(&g, &f.id).unwrap().atlas();
        for m in &f.messages {
            assert!(a.messages[&f.id].contains_key(&m.id), "message {}", m.id);
        }
        for p in &f.participants {
            assert!(a.nodes.contains_key(&p.node), "participant {}", p.node);
        }
    }
    let dm = g.data_model.as_ref().unwrap();
    let a = data_model(&g, &GraphViewScope::All).atlas();
    for t in &dm.tables {
        assert!(a.tables.contains_key(&t.id), "table {}", t.id);
        for c in &t.columns {
            assert!(a.columns[&t.id].contains_key(&c.name), "column {}.{}", t.id, c.name);
        }
        for i in &t.indexes {
            assert!(a.indexes[&t.id].contains_key(&i.name), "index {}.{}", t.id, i.name);
        }
    }
    for r in &dm.relations {
        assert!(a.edges.contains_key(&r.id), "relation {}", r.id);
    }
}

#[test]
fn cards_in_one_lane_never_overlap() {
    for s in SAMPLES {
        let g = graph(s);
        for l in lenses(&g) {
            let cards: Vec<&Placed> = l
                .items
                .iter()
                .filter(|i| matches!(i.kind, PlacedKind::Card | PlacedKind::Table | PlacedKind::Participant))
                .collect();
            for (i, a) in cards.iter().enumerate() {
                for b in &cards[i + 1..] {
                    if a.parent == b.parent {
                        assert!(!overlap(a.rect, b.rect), "{:?} overlaps {:?} in {}", a.id, b.id, l.title);
                    }
                }
                let lane = a.parent.as_ref().and_then(|p| l.item(p));
                if let Some(lane) = lane {
                    let (r, o) = (a.rect, lane.rect);
                    assert!(
                        r[0] >= o[0] && r[1] >= o[1] && r[0] + r[2] <= o[0] + o[2] && r[1] + r[3] <= o[1] + o[3],
                        "{:?} leaves its lane",
                        a.id
                    );
                }
            }
            for it in &l.items {
                assert!(
                    it.rect[0] >= 0.0 && it.rect[1] >= 0.0 && it.rect[0] + it.rect[2] <= l.width && it.rect[1] + it.rect[3] <= l.height,
                    "{:?} off canvas",
                    it.id
                );
            }
        }
    }
}

#[test]
fn architecture_seats_by_rank_and_routes_orthogonally() {
    let g = graph(SCHEMA);
    let l = architecture(&g, &GraphViewScope::All);
    let y = |id: &str| l.item(&ItemId::Node(id.into())).unwrap().rect[1];
    assert!(y("account-page") < y("voucher-form") && y("voucher-form") < y("voucher-client"));
    assert!(y("vouchers-controller") < y("voucher-service") && y("voucher-service") < y("account-service"));
    let lane_x = |id: &str| l.item(&ItemId::Lane(id.into())).unwrap().rect[0];
    assert!(lane_x("web") < lane_x("api") && lane_x("api") < lane_x("data"));
    for r in &l.routes {
        for w in r.points.windows(2) {
            assert!(
                (w[0][0] - w[1][0]).abs() < 0.01 || (w[0][1] - w[1][1]).abs() < 0.01,
                "{:?} has a diagonal leg",
                r.id
            );
        }
    }
    let hero = l.route(&ItemId::Edge("client-to-api".into())).unwrap();
    assert_eq!(hero.label.as_ref().unwrap().0, "POST voucher");
    let tf = l.item(&ItemId::Node("voucher-form".into())).unwrap();
    assert_eq!(tf.badges, ["NEW"]);
}

#[test]
fn every_kind_lays_out_with_orthogonal_routes_and_a_dead_band() {
    let g = graph(KINDS);
    let l = architecture(&g, &GraphViewScope::All);
    for r in &l.routes {
        assert!(r.points.len() >= 2, "{:?}", r.id);
        for w in r.points.windows(2) {
            assert!(
                (w[0][0] - w[1][0]).abs() < 0.01 || (w[0][1] - w[1][1]).abs() < 0.01,
                "{:?} has a diagonal leg",
                r.id
            );
        }
    }
    assert_eq!(l.routes.len(), g.edges.len(), "self-loops and back edges are drawn too");
    let y = |id: &str| l.item(&ItemId::Node(id.into())).unwrap().rect[1];
    let living = g
        .nodes
        .iter()
        .filter(|n| n.delta != rain_core::overview::GraphDelta::Removed)
        .map(|n| y(&n.id))
        .fold(0.0, f32::max);
    assert!(y("legacy-gw") > living, "the removed card sits below everything alive");
    let long = l.item(&ItemId::Node("a-very-long-node".into())).unwrap();
    assert_eq!(long.title_size, crate::design::TITLE_SIZE_MIN);
    let pills: Vec<[f32; 4]> = l.routes.iter().filter_map(|r| r.label.as_ref().map(|(_, b)| *b)).collect();
    for it in l.items.iter().filter(|i| i.kind == PlacedKind::Card) {
        for p in &pills {
            assert!(!overlap(it.rect, *p), "a label pill covers {:?}", it.id);
        }
    }

    let f = data_flow(&g, "charge").unwrap();
    let kind = |id: &str| {
        f.route(&ItemId::Message {
            flow: "charge".into(),
            id: id.into(),
        })
        .unwrap()
        .kind
    };
    assert_eq!(
        [kind("m3"), kind("m5"), kind("m6")],
        [RouteKind::SelfCall, RouteKind::Return, RouteKind::Async]
    );
    assert!(
        f.item(&ItemId::Note {
            flow: "charge".into(),
            id: "m4".into()
        })
        .is_some()
    );
    assert_eq!(
        f.item(&ItemId::Repeat {
            flow: "charge".into(),
            id: "m6".into()
        })
        .unwrap()
        .label,
        "loop ×3"
    );
    let m6 = f
        .route(&ItemId::Message {
            flow: "charge".into(),
            id: "m6".into(),
        })
        .unwrap();
    assert_eq!(m6.label.as_ref().unwrap().0, "charge.succeeded ×3");
    let admin = f
        .item(&ItemId::Participant {
            flow: "charge".into(),
            node: "pay-page".into(),
        })
        .unwrap();
    assert_eq!(admin.label, "Admin");

    let m = data_model(&g, &GraphViewScope::All);
    let lanes: Vec<String> = m.items.iter().filter(|i| i.kind == PlacedKind::Lane).map(|i| i.id.key()).collect();
    assert_eq!(lanes, ["lane:sql", "lane:ledger-db", "lane:cache"]);
    let over = m.route(&ItemId::Relation("cache-order".into())).unwrap();
    assert_eq!(over.points.len(), 6, "a relation skipping a lane crosses above the lanes");
    assert!(over.points[2][1] < m.item(&ItemId::Lane("sql".into())).unwrap().rect[1]);
    assert_eq!(over.kind, RouteKind::Relation { enforced: false });
    let fold = m
        .item(&ItemId::Fold {
            table: "payments".into(),
            indexes: false,
        })
        .unwrap();
    assert_eq!(fold.label, "+ 4 unchanged columns");
    let ifold = m
        .item(&ItemId::Fold {
            table: "payments".into(),
            indexes: true,
        })
        .unwrap();
    assert_eq!(ifold.label, "+ 2 unchanged indexes");
    let amount = m
        .item(&ItemId::Column {
            table: "payments".into(),
            column: "Amount".into(),
        })
        .unwrap();
    assert_eq!(amount.previous.as_deref(), Some("money"));

    let v = scope::find_view(&g.views, "edges-only").unwrap();
    let e = architecture(&g, &v.scope);
    assert_eq!(e.routes.len(), 2);
    assert_eq!(e.items.iter().filter(|i| i.kind == PlacedKind::Card).count(), 3);
}

#[test]
fn a_selection_scope_draws_its_nodes_and_their_lanes() {
    let g = graph(SCHEMA);
    let v = scope::find_view(&g.views, "api-only").unwrap();
    let l = architecture(&g, &v.scope);
    let nodes: Vec<_> = l
        .items
        .iter()
        .filter(|i| i.kind == PlacedKind::Card)
        .map(|i| i.id.clone())
        .collect();
    assert_eq!(nodes.len(), 4);
    let lanes: Vec<_> = l.items.iter().filter(|i| i.kind == PlacedKind::Lane).map(|i| i.id.key()).collect();
    assert_eq!(lanes, ["lane:api", "lane:data"]);
    assert_eq!(l.routes.len(), 3);
}

#[test]
fn data_flow_rows_go_down_with_activations_and_a_dotted_return() {
    let g = graph(SCHEMA);
    let l = data_flow(&g, "add-voucher").unwrap();
    let ys: Vec<f32> = g.flows[0]
        .messages
        .iter()
        .map(|m| {
            l.route(&ItemId::Message {
                flow: "add-voucher".into(),
                id: m.id.clone(),
            })
            .unwrap()
            .points[0][1]
        })
        .collect();
    assert!(ys.windows(2).all(|w| w[0] < w[1]));
    let ret = l
        .route(&ItemId::Message {
            flow: "add-voucher".into(),
            id: "ok".into(),
        })
        .unwrap();
    assert_eq!(ret.kind, RouteKind::Return);
    assert!(ret.points[0][0] > ret.points[1][0], "the return runs right to left");
    let bars = l.items.iter().filter(|i| i.kind == PlacedKind::Activation).count();
    assert_eq!(bars, 3, "controller (answered), service and table (unanswered)");
    assert!(data_flow(&g, "nope").is_none());
}

#[test]
fn data_model_rows_chips_and_families() {
    let g = graph(SCHEMA);
    let l = data_model(&g, &GraphViewScope::All);
    let col = |t: &str, c: &str| {
        l.item(&ItemId::Column {
            table: t.into(),
            column: c.into(),
        })
        .unwrap()
    };
    assert_eq!(col("vouchers", "Id").chip, Some(Chip::Primary));
    assert_eq!(col("vouchers", "AccountId").chip, Some(Chip::Foreign));
    assert_eq!(col("vouchers", "AccountId").family, Some(0));
    assert_eq!(col("accounts", "Id").family, Some(0));
    assert_eq!(col("accounts", "CustomerId").family, Some(1));
    let ix = l
        .item(&ItemId::Index {
            table: "vouchers".into(),
            name: "UX_Vouchers_AccountId_Created".into(),
        })
        .unwrap();
    assert_eq!(ix.chip, Some(Chip::UniqueIndex));
    assert_eq!(ix.aside.as_deref(), Some("AccountId, CreatedAt"));
    let rel = l.route(&ItemId::Relation("voucher-account".into())).unwrap();
    let (from, to) = (rel.points[0], *rel.points.last().unwrap());
    assert!((from[1] - (col("vouchers", "AccountId").rect[1] + 10.0)).abs() < 0.01);
    assert!((to[1] - (col("accounts", "Id").rect[1] + 10.0)).abs() < 0.01);
    let t = l.item(&ItemId::Table("vouchers".into())).unwrap();
    assert_eq!(t.subtitle.as_deref(), Some("dbo.Vouchers · 4 columns"));
}

#[test]
fn a_tall_table_folds_its_unchanged_columns() {
    let mut v: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
    let cols: Vec<serde_json::Value> = (0..30)
        .map(|i| serde_json::json!({ "name": format!("C{i}"), "type": "int", "delta": "unchanged" }))
        .collect();
    v["graph"]["dataModel"]["tables"][2]["columns"].as_array_mut().unwrap().extend(cols);
    let g: OverviewGraph = serde_json::from_value::<OverviewDocument>(v).unwrap().graph;
    let l = data_model(&g, &GraphViewScope::All);
    let fold = l
        .item(&ItemId::Fold {
            table: "customers".into(),
            indexes: false,
        })
        .unwrap();
    assert_eq!(fold.label, "+ 23 unchanged columns");
    let shown = l
        .items
        .iter()
        .filter(|i| matches!(&i.id, ItemId::Column { table, .. } if table == "customers"))
        .count();
    assert_eq!(shown, 9, "the key and eight unchanged");
}

#[test]
fn related_lights_what_a_node_touches() {
    let g = graph(SCHEMA);
    let l = architecture(&g, &GraphViewScope::All);
    let lit = l.related(&ItemId::Node("voucher-service".into()));
    for id in ["vouchers-controller", "account-service", "voucher-table", "voucher-service"] {
        assert!(lit.contains(&ItemId::Node(id.into())), "{id}");
    }
    assert!(lit.contains(&ItemId::Edge("service-to-table".into())));
    assert!(!lit.contains(&ItemId::Node("account-page".into())));
}

#[test]
fn dashes_alternate_and_keep_their_length() {
    let d = dashes(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], 3.0, 2.0);
    assert_eq!(d.len(), 4);
    assert_eq!(d[0], vec![[0.0, 0.0], [3.0, 0.0]]);
    assert_eq!(d[1], vec![[5.0, 0.0], [8.0, 0.0]]);
    assert_eq!(d[2], vec![[10.0, 0.0], [10.0, 3.0]]);
    assert_eq!(d[3], vec![[10.0, 5.0], [10.0, 8.0]]);
    let corner = dashes(&[[0.0, 0.0], [2.0, 0.0], [2.0, 2.0]], 3.0, 1.0);
    assert_eq!(corner[0], vec![[0.0, 0.0], [2.0, 0.0], [2.0, 1.0]], "a dash runs round a corner");
}

#[test]
#[ignore = "release perf: cargo test --release -- --ignored --nocapture"]
fn sample_layout_takes_under_50ms() {
    let g = graph(SCHEMA);
    let t = Instant::now();
    let n = 200;
    for _ in 0..n {
        std::hint::black_box(lenses(&g));
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / n as f64;
    println!("diagram: all lenses + views of the schema sample laid out in {ms:.3} ms");
    assert!(ms <= 50.0);
}

struct T {
    layout: Layout,
    hover: Option<ItemId>,
    zoom: Zoom,
    events: Vec<DiagramEvent>,
}

fn view(s: &T, _: &mut Cx<T>) -> El {
    Diagram::new(&s.layout)
        .hover(s.hover.clone())
        .zoom(s.zoom)
        .viewport(1400.0, 900.0)
        .on_event(|s: &mut T, e, _| {
            if let DiagramEvent::Hover(h) = &e {
                s.hover = h.clone();
            }
            s.events.push(e);
        })
        .build(&Kit::neutral(true))
}

fn runtime(layout: Layout, zoom: Zoom) -> Runtime<T> {
    let mut rt = Runtime::new(
        T {
            layout,
            hover: None,
            zoom,
            events: Vec::new(),
        },
        view,
        (1400.0, 900.0),
        Env {
            system_dark: true,
            scale: 1.0,
        },
    );
    rt.frame(0.0);
    rt
}

#[test]
fn pressing_a_card_reports_its_node() {
    let g = graph(SCHEMA);
    let mut rt = runtime(architecture(&g, &GraphViewScope::All), Zoom::P100);
    let (x, y) = rt.find("node:voucher-service").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.016);
    assert!(
        rt.state.events.contains(&DiagramEvent::Node("voucher-service".into())),
        "{:?}",
        rt.state.events
    );
}

#[test]
fn pressing_a_column_and_a_message_reports_them() {
    let g = graph(SCHEMA);
    let mut rt = runtime(data_model(&g, &GraphViewScope::All), Zoom::P100);
    let (x, y) = rt.find("col:vouchers:AccountId").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.016);
    assert!(
        rt.state.events.contains(&DiagramEvent::Column {
            table: "vouchers".into(),
            column: "AccountId".into()
        }),
        "{:?}",
        rt.state.events
    );

    let mut rt = runtime(data_flow(&g, "add-voucher").unwrap(), Zoom::P100);
    let (x, y) = rt.find("msg:add-voucher:record#0").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.016);
    assert!(
        rt.state.events.contains(&DiagramEvent::Message {
            flow: "add-voucher".into(),
            id: "record".into()
        }),
        "{:?}",
        rt.state.events
    );
}

#[test]
fn hover_enters_then_leaves() {
    let g = graph(SCHEMA);
    let mut rt = runtime(architecture(&g, &GraphViewScope::All), Zoom::P100);
    let (x, y) = rt.find("node:voucher-form").unwrap();
    rt.pointer_move(x, y);
    rt.frame(0.016);
    assert_eq!(rt.state.hover, Some(ItemId::Node("voucher-form".into())));
    let (x2, y2) = rt.find("node:voucher-client").unwrap();
    rt.pointer_move(x2, y2);
    rt.frame(0.016);
    assert_eq!(rt.state.hover, Some(ItemId::Node("voucher-client".into())));
    rt.pointer_move(4.0, 4.0);
    rt.frame(0.016);
    assert_eq!(rt.state.hover, None);
    let hovers: Vec<_> = rt
        .state
        .events
        .iter()
        .filter(|e| matches!(e, DiagramEvent::Hover(_)))
        .cloned()
        .collect();
    assert_eq!(
        hovers,
        [
            DiagramEvent::Hover(Some(ItemId::Node("voucher-form".into()))),
            DiagramEvent::Hover(Some(ItemId::Node("voucher-client".into()))),
            DiagramEvent::Hover(None)
        ]
    );
}

#[test]
fn fit_zoom_fits_the_viewport_width() {
    let g = graph(SCHEMA);
    let l = architecture(&g, &GraphViewScope::All);
    assert!(l.width > 600.0);
    let d: Diagram<T> = Diagram::new(&l).viewport(600.0, 400.0).zoom(Zoom::Fit);
    assert!(d.scale() * l.width <= 600.0 + 0.01);
    assert_eq!(Diagram::<T>::new(&l).viewport(5000.0, 400.0).zoom(Zoom::Fit).scale(), 1.0);
    assert_eq!(Diagram::<T>::new(&l).zoom(Zoom::P150).scale(), 1.5);

    let mut rt = Runtime::new(
        T {
            layout: l.clone(),
            hover: None,
            zoom: Zoom::Fit,
            events: Vec::new(),
        },
        fit_view,
        (600.0, 400.0),
        Env {
            system_dark: true,
            scale: 1.0,
        },
    );
    rt.frame(0.0);
    let (r, _) = rt.presented("diagram-canvas").unwrap();
    assert!(r[2] <= 600.0 + 0.5, "{r:?}");
    let (x, y) = rt.find("node:voucher-service").unwrap();
    rt.pointer_move(x, y);
    rt.frame(0.016);
    assert_eq!(
        rt.state.hover,
        Some(ItemId::Node("voucher-service".into())),
        "hover hit-tests in zoomed points"
    );
}

#[test]
fn a_hover_fades_unrelated_nodes_to_a_readable_sixty_percent() {
    let g = graph(SCHEMA);
    let l = architecture(&g, &GraphViewScope::All);
    let nodes: Vec<ItemId> = l
        .items
        .iter()
        .filter(|it| matches!(&it.id, ItemId::Node(_)))
        .map(|it| it.id.clone())
        .collect();
    let (hover, other) = nodes
        .iter()
        .find_map(|h| {
            let lit = l.related(h);
            nodes.iter().find(|n| !lit.contains(*n)).map(|n| (h.clone(), n.clone()))
        })
        .expect("a node a hover doesn't relate");
    let ItemId::Node(name) = &other else { unreachable!() };
    let opacity = |hover: Option<ItemId>| {
        let mut rt = Runtime::new(
            T {
                layout: l.clone(),
                hover,
                zoom: Zoom::P100,
                events: Vec::new(),
            },
            fit_view,
            (2400.0, 1600.0),
            Env {
                system_dark: true,
                scale: 1.0,
            },
        );
        rt.frame(0.0);
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(&format!("node:{name}")))
            .map(|e| e.opacity)
            .unwrap()
    };
    let ratio = opacity(Some(hover)) / opacity(None);
    assert!((ratio - crate::draw::DIMMED).abs() < 0.01, "{ratio}");
    const { assert!(crate::draw::DIMMED >= 0.6, "faded, still readable") };
}

#[test]
fn fit_with_room_to_grow_enlarges_a_small_diagram_until_it_meets_an_edge() {
    let g = graph(SCHEMA);
    let l = architecture(&g, &GraphViewScope::All);
    let (vw, vh) = (l.width * 3.0, l.height * 1.5);
    assert_eq!(
        Diagram::<T>::new(&l).viewport(vw, vh).zoom(Zoom::Fit).scale(),
        1.0,
        "growth is opt-in"
    );
    let k = Diagram::<T>::new(&l).viewport(vw, vh).zoom(Zoom::Fit).fit_grow(2.0).scale();
    assert!((k - 1.5).abs() < 0.01, "height is the limit: {k}");
    let k = Diagram::<T>::new(&l)
        .viewport(l.width * 4.0, l.height * 4.0)
        .zoom(Zoom::Fit)
        .fit_grow(2.0)
        .scale();
    assert_eq!(k, 2.0, "never past the cap");
    let k = Diagram::<T>::new(&l)
        .viewport(l.width / 2.0, l.height * 4.0)
        .zoom(Zoom::Fit)
        .fit_grow(2.0)
        .scale();
    assert!((k - 0.5).abs() < 0.01, "a wide diagram still shrinks to the width: {k}");
}

fn fit_view(s: &T, _: &mut Cx<T>) -> El {
    Diagram::new(&s.layout)
        .hover(s.hover.clone())
        .zoom(Zoom::Fit)
        .viewport(600.0, 400.0)
        .on_event(|s: &mut T, e, _| {
            if let DiagramEvent::Hover(h) = e {
                s.hover = h;
            }
        })
        .build(&Kit::neutral(false))
}

#[test]
fn render_targets_pick_the_right_lens() {
    let g = graph(SCHEMA);
    let t = |lens, view: Option<&str>| RenderTarget {
        lens,
        view: view.map(str::to_owned),
        flow: None,
        title: "t".into(),
        depth: 0,
    };
    assert_eq!(
        render(&g, &t(GraphLens::DataFlow, Some("add-voucher-flow"))).unwrap().lens,
        GraphLens::DataFlow
    );
    assert_eq!(
        render(&g, &t(GraphLens::DataModel, Some("schema")))
            .unwrap()
            .items
            .iter()
            .filter(|i| i.kind == PlacedKind::Table)
            .count(),
        3
    );
}

struct M {
    layout: Layout,
    marks: std::collections::BTreeMap<String, NodeMark>,
    selected: Option<String>,
    events: Vec<DiagramEvent>,
}

fn marked_view(s: &M, _: &mut Cx<M>) -> El {
    Diagram::new(&s.layout)
        .zoom(Zoom::P100)
        .viewport(1400.0, 900.0)
        .marks(s.marks.clone())
        .selected(s.selected.clone())
        .on_event(|s: &mut M, e, _| s.events.push(e))
        .build(&Kit::neutral(false))
}

#[test]
fn marks_draw_viewed_bars_thread_badges_and_the_selection_ring() {
    let g = graph(SCHEMA);
    let marks = [
        ("voucher-service".to_owned(), NodeMark { viewed: 1.0, threads: 2 }),
        ("voucher-form".to_owned(), NodeMark { viewed: 0.5, threads: 0 }),
    ]
    .into_iter()
    .collect();
    let mut rt = Runtime::new(
        M {
            layout: architecture(&g, &GraphViewScope::All),
            marks,
            selected: Some("voucher-form".into()),
            events: Vec::new(),
        },
        marked_view,
        (1400.0, 900.0),
        Env {
            system_dark: false,
            scale: 1.0,
        },
    );
    rt.frame(0.0);
    let (card, _) = rt.presented("node:voucher-form").unwrap();
    let (bar, _) = rt.presented("mark:voucher-form#0").unwrap();
    assert!(
        bar[1] > card[1] && bar[1] < card[1] + card[3],
        "the bar sits inside the card: {bar:?} {card:?}"
    );
    assert!(
        rt.find("mark:voucher-form#1").is_none(),
        "no badge without threads, no tint below 100 %"
    );
    assert!(
        rt.find("mark:voucher-service#2").is_some(),
        "tint, bar and badge on a viewed node with threads"
    );
    assert!(rt.find("selected:voucher-form").is_some());
    assert!(rt.find("selected:voucher-service").is_none());
    let (x, y) = rt.find("mark:voucher-service#2").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.016);
    assert!(
        rt.state.events.contains(&DiagramEvent::Node("voucher-service".into())),
        "the badge presses its node"
    );
}

#[test]
fn stacked_lanes_keep_every_card_and_drop_only_cross_lane_lines() {
    let g = graph(SCHEMA);
    let l = architecture(&g, &GraphViewScope::All);
    let lanes = l.items.iter().filter(|i| i.kind == PlacedKind::Lane).count();
    assert!(lanes >= 2);
    let (s, hidden) = l.stacked().unwrap();
    let cards = |x: &Layout| x.items.iter().filter(|i| i.kind == PlacedKind::Card).count();
    assert_eq!(cards(&s), cards(&l));
    assert_eq!(s.routes.len() + hidden, l.routes.len());
    assert!(s.width < l.width, "{} vs {}", s.width, l.width);
    assert!(s.height > l.height);
    let mut ys: Vec<(f32, f32)> = s
        .items
        .iter()
        .filter(|i| i.kind == PlacedKind::Lane)
        .map(|i| (i.rect[1], i.rect[1] + i.rect[3]))
        .collect();
    ys.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(ys.windows(2).all(|w| w[0].1 <= w[1].0), "lanes do not overlap: {ys:?}");
    for it in s.items.iter().filter(|i| i.kind == PlacedKind::Card) {
        let lane = s.items.iter().find(|x| Some(&x.id) == it.parent.as_ref()).unwrap();
        assert!(it.rect[1] >= lane.rect[1] && it.rect[1] + it.rect[3] <= lane.rect[1] + lane.rect[3]);
    }
    assert!(
        data_flow(&g, "add-voucher").unwrap().stacked().is_none(),
        "no lanes, nothing to stack"
    );
}
