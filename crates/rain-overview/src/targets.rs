//! Render targets (port of difit `server/overview/render.ts:25-68`) and the PR Lens view-scope
//! resolver (`renderer/scope.ts`), plus the render dry run validation uses for its error codes.

use std::collections::BTreeSet;

use rain_core::overview::{
    GraphDataModel, GraphEdge, GraphFlow, GraphLane, GraphLens, GraphNode, GraphRelation, GraphStore, GraphTable, GraphView,
    GraphViewScope, Issue, OverviewGraph, RenderTarget,
};

pub fn lens_title(lens: GraphLens) -> &'static str {
    match lens {
        GraphLens::Architecture => "Architecture",
        GraphLens::DataFlow => "Data flow",
        GraphLens::DataModel => "Data model",
    }
}

/// Every view whose lens is declared, in tree order, then one target per flow; with no views, one
/// target per drawable lens.
pub fn targets(graph: &OverviewGraph) -> Vec<RenderTarget> {
    fn visit(graph: &OverviewGraph, views: &[GraphView], depth: u32, out: &mut Vec<RenderTarget>) {
        for view in views {
            if graph.lenses.contains(&view.lens) {
                out.push(RenderTarget {
                    lens: view.lens,
                    view: Some(view.id.clone()),
                    flow: None,
                    title: view.title.clone(),
                    depth,
                });
            }
            visit(graph, &view.children, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    if graph.views.is_empty() {
        for &lens in &graph.lenses {
            let drawable = match lens {
                GraphLens::Architecture => true,
                GraphLens::DataFlow => !graph.flows.is_empty(),
                GraphLens::DataModel => graph.data_model.is_some(),
            };
            if drawable {
                out.push(RenderTarget {
                    lens,
                    view: None,
                    flow: None,
                    title: lens_title(lens).to_owned(),
                    depth: 0,
                });
            }
        }
    } else {
        visit(graph, &graph.views, 0, &mut out);
    }
    for flow in &graph.flows {
        out.push(RenderTarget {
            lens: GraphLens::DataFlow,
            view: None,
            flow: Some(flow.id.clone()),
            title: flow.title.clone(),
            depth: 0,
        });
    }
    out
}

/// The first target whose view is `defaultOpen`, else the first target.
pub fn default_target<'a>(graph: &OverviewGraph, targets: &'a [RenderTarget]) -> Option<&'a RenderTarget> {
    let mut open = BTreeSet::new();
    fn visit<'v>(views: &'v [GraphView], open: &mut BTreeSet<&'v str>) {
        for v in views {
            if v.default_open {
                open.insert(v.id.as_str());
            }
            visit(&v.children, open);
        }
    }
    visit(&graph.views, &mut open);
    targets
        .iter()
        .find(|t| t.view.as_deref().is_some_and(|v| open.contains(v)))
        .or_else(|| targets.first())
}

/// One tab name per target, no two alike: a title shared across lenses gains its lens
/// (`Vouchers · Data model`), a flow sharing a view's title gains `(flow)`, and a name still shared
/// gains a counter (`Vouchers 2`).
pub fn tab_names(targets: &[RenderTarget]) -> Vec<String> {
    let count = |names: &[String], n: &str| names.iter().filter(|x| x.as_str() == n).count();
    let titles: Vec<String> = targets
        .iter()
        .map(|t| {
            if t.title.trim().is_empty() {
                lens_title(t.lens).to_owned()
            } else {
                t.title.clone()
            }
        })
        .collect();
    let named: Vec<String> = targets
        .iter()
        .zip(&titles)
        .map(|(t, n)| {
            if count(&titles, n) < 2 {
                return n.clone();
            }
            let mixed = targets.iter().zip(&titles).any(|(o, m)| m == n && o.lens != t.lens);
            match (mixed, t.flow.is_some()) {
                (true, _) => format!("{n} · {}", lens_title(t.lens)),
                (false, true) => format!("{n} (flow)"),
                (false, false) => n.clone(),
            }
        })
        .collect();
    let mut seen: Vec<String> = Vec::new();
    named
        .iter()
        .map(|n| {
            let k = count(&seen, n);
            seen.push(n.clone());
            if k == 0 { n.clone() } else { format!("{n} {}", k + 1) }
        })
        .collect()
}

pub fn find_view<'a>(views: &'a [GraphView], id: &str) -> Option<&'a GraphView> {
    views
        .iter()
        .find_map(|v| if v.id == id { Some(v) } else { find_view(&v.children, id) })
}

/// The slice of a graph one picture draws.
#[derive(Clone, Debug, Default)]
pub struct ScopedGraph<'a> {
    pub lanes: Vec<&'a GraphLane>,
    pub nodes: Vec<&'a GraphNode>,
    pub edges: Vec<&'a GraphEdge>,
    pub flows: Vec<&'a GraphFlow>,
    pub stores: Vec<&'a GraphStore>,
    pub tables: Vec<&'a GraphTable>,
    pub relations: Vec<&'a GraphRelation>,
}

fn scope_model<'a>(model: Option<&'a GraphDataModel>, tables: Option<&BTreeSet<&str>>, out: &mut ScopedGraph<'a>) {
    let Some(model) = model else { return };
    out.tables = model
        .tables
        .iter()
        .filter(|t| tables.is_none_or(|ids| ids.contains(t.id.as_str())))
        .collect();
    let kept: BTreeSet<&str> = out.tables.iter().map(|t| t.id.as_str()).collect();
    let stores: BTreeSet<&str> = out.tables.iter().map(|t| t.store.as_str()).collect();
    out.stores = model.stores.iter().filter(|s| stores.contains(s.id.as_str())).collect();
    out.relations = model
        .relations
        .iter()
        .filter(|r| kept.contains(r.from.table.as_str()) && kept.contains(r.to.table.as_str()))
        .collect();
}

/// A selection pulls in the lanes its nodes sit in, both ends of every named edge and every
/// participant of a named flow; with no edges named, the edges between selected nodes come along.
pub fn resolve_scope<'a>(graph: &'a OverviewGraph, scope: &GraphViewScope) -> ScopedGraph<'a> {
    let mut out = ScopedGraph::default();
    match scope {
        GraphViewScope::All => {
            out.lanes = graph.lanes.iter().collect();
            out.nodes = graph.nodes.iter().collect();
            out.edges = graph.edges.iter().collect();
            out.flows = graph.flows.iter().collect();
            scope_model(graph.data_model.as_ref(), None, &mut out);
        }
        GraphViewScope::Selection {
            lanes,
            nodes,
            edges,
            flows,
            tables,
        } => {
            let sel_lanes: BTreeSet<&str> = lanes.iter().map(String::as_str).collect();
            let sel_edges: BTreeSet<&str> = edges.iter().map(String::as_str).collect();
            let sel_flows: BTreeSet<&str> = flows.iter().map(String::as_str).collect();
            out.flows = graph.flows.iter().filter(|f| sel_flows.contains(f.id.as_str())).collect();
            let mut node_ids: BTreeSet<&str> = nodes.iter().map(String::as_str).collect();
            node_ids.extend(
                graph
                    .nodes
                    .iter()
                    .filter(|n| sel_lanes.contains(n.lane.as_str()))
                    .map(|n| n.id.as_str()),
            );
            for e in graph.edges.iter().filter(|e| sel_edges.contains(e.id.as_str())) {
                node_ids.insert(&e.from);
                node_ids.insert(&e.to);
            }
            for f in &out.flows {
                node_ids.extend(f.participants.iter().map(|p| p.node.as_str()));
            }
            out.nodes = graph.nodes.iter().filter(|n| node_ids.contains(n.id.as_str())).collect();
            out.edges = if edges.is_empty() {
                graph
                    .edges
                    .iter()
                    .filter(|e| node_ids.contains(e.from.as_str()) && node_ids.contains(e.to.as_str()))
                    .collect()
            } else {
                graph.edges.iter().filter(|e| sel_edges.contains(e.id.as_str())).collect()
            };
            let mut lane_ids = sel_lanes;
            lane_ids.extend(out.nodes.iter().map(|n| n.lane.as_str()));
            out.lanes = graph.lanes.iter().filter(|l| lane_ids.contains(l.id.as_str())).collect();
            let table_ids: BTreeSet<&str> = tables.iter().map(String::as_str).collect();
            scope_model(graph.data_model.as_ref(), Some(&table_ids), &mut out);
        }
    }
    out
}

/// What the renderer would draw for `target`, or the error code PR Lens's `render()` raises.
pub fn scope_for<'a>(graph: &'a OverviewGraph, target: &RenderTarget) -> Result<ScopedGraph<'a>, Issue> {
    let path = match (&target.view, &target.flow) {
        (Some(v), _) => format!("graph.views[{v}]"),
        (None, Some(f)) => format!("graph.flows[{f}]"),
        (None, None) => "graph.lenses".to_owned(),
    };
    let fail = |code: &str, message: String| Issue {
        code: code.to_owned(),
        path: path.clone(),
        message,
    };
    let lens = if target.flow.is_some() { GraphLens::DataFlow } else { target.lens };
    if !graph.lenses.contains(&lens) {
        return Err(fail(
            "LENS_NOT_DECLARED",
            format!("this document does not declare the '{}' lens", lens_name(lens)),
        ));
    }
    let scoped = match (&target.view, &target.flow) {
        (_, Some(flow)) => resolve_scope(
            graph,
            &GraphViewScope::Selection {
                lanes: vec![],
                nodes: vec![],
                edges: vec![],
                flows: vec![flow.clone()],
                tables: vec![],
            },
        ),
        (Some(view), None) => match find_view(&graph.views, view) {
            Some(v) => resolve_scope(graph, &v.scope),
            None => return Err(fail("UNKNOWN_VIEW", format!("this document has no view '{view}'"))),
        },
        (None, None) => resolve_scope(graph, &GraphViewScope::All),
    };
    match lens {
        GraphLens::Architecture if scoped.nodes.is_empty() => {
            Err(fail("NOTHING_TO_RENDER", "no nodes are in scope for this view".to_owned()))
        }
        GraphLens::DataFlow if scoped.flows.is_empty() => {
            Err(fail("NO_FLOW_IN_SCOPE", "the data-flow lens needs a flow to draw".to_owned()))
        }
        GraphLens::DataModel if scoped.tables.is_empty() => {
            Err(fail("NOTHING_TO_RENDER", "no tables are in scope for this view".to_owned()))
        }
        _ => Ok(scoped),
    }
}

pub fn lens_name(lens: GraphLens) -> &'static str {
    match lens {
        GraphLens::Architecture => "architecture",
        GraphLens::DataFlow => "data-flow",
        GraphLens::DataModel => "data-model",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{sample, schema_sample};

    fn t(lens: GraphLens, view: Option<&str>, flow: Option<&str>, title: &str) -> RenderTarget {
        RenderTarget {
            lens,
            view: view.map(Into::into),
            flow: flow.map(Into::into),
            title: title.into(),
            depth: 0,
        }
    }

    #[test]
    fn lists_views_in_tree_order_plus_one_per_flow() {
        let g = sample().graph;
        let ts = targets(&g);
        assert_eq!(
            ts,
            vec![
                t(GraphLens::Architecture, Some("overview"), None, "Vouchers end to end"),
                t(GraphLens::Architecture, Some("api-only"), None, "Inside the API"),
                t(GraphLens::DataFlow, Some("add-voucher-flow"), None, "Adding a voucher"),
                t(GraphLens::DataFlow, None, Some("add-voucher"), "Adding a voucher"),
            ]
        );
        assert_eq!(default_target(&g, &ts), Some(&ts[0]));
    }

    #[test]
    fn tab_names_never_repeat() {
        let ts = targets(&sample().graph);
        assert_eq!(
            tab_names(&ts),
            [
                "Vouchers end to end",
                "Inside the API",
                "Adding a voucher",
                "Adding a voucher (flow)"
            ]
        );
        let mixed = [
            t(GraphLens::Architecture, Some("a"), None, "Vouchers"),
            t(GraphLens::DataModel, Some("b"), None, "Vouchers"),
            t(GraphLens::Architecture, Some("c"), None, "Vouchers"),
            t(GraphLens::DataModel, Some("d"), None, ""),
        ];
        let names = tab_names(&mixed);
        assert_eq!(
            names,
            [
                "Vouchers · Architecture",
                "Vouchers · Data model",
                "Vouchers · Architecture 2",
                "Data model"
            ]
        );
        let unique: BTreeSet<&String> = names.iter().collect();
        assert_eq!(unique.len(), names.len());
    }

    #[test]
    fn falls_back_to_drawable_lenses_without_views() {
        let mut g = sample().graph;
        g.views.clear();
        g.flows.clear();
        g.lenses = vec![GraphLens::Architecture, GraphLens::DataFlow, GraphLens::DataModel];
        assert_eq!(targets(&g), vec![t(GraphLens::Architecture, None, None, "Architecture")]);
    }

    #[test]
    fn nested_views_carry_depth_and_default_open_wins() {
        let mut g = sample().graph;
        let child = g.views.remove(1);
        g.views[0].default_open = false;
        g.views[0].children.push(GraphView {
            default_open: true,
            ..child
        });
        let ts = targets(&g);
        assert_eq!(ts[1].view.as_deref(), Some("api-only"));
        assert_eq!(ts[1].depth, 1);
        assert_eq!(default_target(&g, &ts).unwrap().view.as_deref(), Some("api-only"));
    }

    #[test]
    fn unknown_view_and_empty_scopes_raise_render_codes() {
        let g = sample().graph;
        let missing = t(GraphLens::Architecture, Some("missing"), None, "x");
        assert_eq!(scope_for(&g, &missing).unwrap_err().code, "UNKNOWN_VIEW");
        let flow = scope_for(&g, &t(GraphLens::DataFlow, None, Some("add-voucher"), "x")).unwrap();
        assert_eq!(flow.flows.len(), 1);
        assert_eq!(flow.nodes.len(), 4);
    }

    #[test]
    fn selection_scope_pulls_in_lanes_and_edges() {
        let g = sample().graph;
        let api = scope_for(&g, &t(GraphLens::Architecture, Some("api-only"), None, "x")).unwrap();
        assert_eq!(api.nodes.len(), 4);
        assert_eq!(api.lanes.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), ["api", "data"]);
        assert_eq!(api.edges.len(), 3);
        let schema = schema_sample().graph;
        let all = resolve_scope(&schema, &GraphViewScope::All);
        assert_eq!(all.tables.len(), schema.data_model.as_ref().unwrap().tables.len());
    }
}
