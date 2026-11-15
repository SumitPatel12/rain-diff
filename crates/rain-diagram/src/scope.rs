//! A view's scope turned into the elements to draw (port of PR Lens `renderer/scope.ts`).

use std::collections::BTreeSet;

use rain_core::overview::{
    GraphEdge, GraphFlow, GraphLane, GraphNode, GraphRelation, GraphStore, GraphTable, GraphView, GraphViewScope, OverviewGraph,
};

/// The slice of a graph one picture draws.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scoped<'g> {
    pub lanes: Vec<&'g GraphLane>,
    pub nodes: Vec<&'g GraphNode>,
    pub edges: Vec<&'g GraphEdge>,
    pub flows: Vec<&'g GraphFlow>,
    pub stores: Vec<&'g GraphStore>,
    pub tables: Vec<&'g GraphTable>,
    pub relations: Vec<&'g GraphRelation>,
}

fn model<'g>(graph: &'g OverviewGraph, tables: Option<&BTreeSet<&str>>, out: &mut Scoped<'g>) {
    let Some(m) = &graph.data_model else { return };
    out.tables = m
        .tables
        .iter()
        .filter(|t| tables.is_none_or(|s| s.contains(t.id.as_str())))
        .collect();
    let kept: BTreeSet<&str> = out.tables.iter().map(|t| t.id.as_str()).collect();
    let stores: BTreeSet<&str> = out.tables.iter().map(|t| t.store.as_str()).collect();
    out.stores = m.stores.iter().filter(|s| stores.contains(s.id.as_str())).collect();
    out.relations = m
        .relations
        .iter()
        .filter(|r| kept.contains(r.from.table.as_str()) && kept.contains(r.to.table.as_str()))
        .collect();
}

/// What `scope` draws. A selection pulls in the lanes its nodes sit in and both ends of every
/// edge it names; naming no edges brings the edges between the selected nodes.
pub fn resolve<'g>(graph: &'g OverviewGraph, scope: &GraphViewScope) -> Scoped<'g> {
    let mut out = Scoped::default();
    match scope {
        GraphViewScope::All => {
            out.lanes = graph.lanes.iter().collect();
            out.nodes = graph.nodes.iter().collect();
            out.edges = graph.edges.iter().collect();
            out.flows = graph.flows.iter().collect();
            model(graph, None, &mut out);
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
            let mut ids: BTreeSet<&str> = nodes.iter().map(String::as_str).collect();
            ids.extend(
                graph
                    .nodes
                    .iter()
                    .filter(|n| sel_lanes.contains(n.lane.as_str()))
                    .map(|n| n.id.as_str()),
            );
            for e in graph.edges.iter().filter(|e| sel_edges.contains(e.id.as_str())) {
                ids.extend([e.from.as_str(), e.to.as_str()]);
            }
            for f in &out.flows {
                ids.extend(f.participants.iter().map(|p| p.node.as_str()));
            }
            out.nodes = graph.nodes.iter().filter(|n| ids.contains(n.id.as_str())).collect();
            out.edges = if edges.is_empty() {
                graph
                    .edges
                    .iter()
                    .filter(|e| ids.contains(e.from.as_str()) && ids.contains(e.to.as_str()))
                    .collect()
            } else {
                graph.edges.iter().filter(|e| sel_edges.contains(e.id.as_str())).collect()
            };
            let mut lane_ids = sel_lanes;
            lane_ids.extend(out.nodes.iter().map(|n| n.lane.as_str()));
            out.lanes = graph.lanes.iter().filter(|l| lane_ids.contains(l.id.as_str())).collect();
            let tables: BTreeSet<&str> = tables.iter().map(String::as_str).collect();
            model(graph, Some(&tables), &mut out);
        }
    }
    out
}

/// The view with `id`, searching nested views too.
pub fn find_view<'g>(views: &'g [GraphView], id: &str) -> Option<&'g GraphView> {
    views
        .iter()
        .find_map(|v| if v.id == id { Some(v) } else { find_view(&v.children, id) })
}

/// Every view, parents before their children.
pub fn flatten_views(views: &[GraphView]) -> Vec<&GraphView> {
    views
        .iter()
        .flat_map(|v| std::iter::once(v).chain(flatten_views(&v.children)))
        .collect()
}
