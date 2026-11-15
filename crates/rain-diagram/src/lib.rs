//! Native PR Lens lenses: the overview's GraphDoc laid out (architecture lanes, data-flow
//! sequence, data-model ER) and drawn with Rune boxes, text and shapes, with an atlas of every
//! item, press and hover events, a hover veil and zoom. No SVG and no JavaScript.

mod arch;
pub mod design;
mod draw;
mod flow;
mod layout;
#[cfg(feature = "mermaid-compare")]
pub mod mermaid;
mod model;
pub mod scope;

use rain_core::overview::{GraphLens, GraphViewScope, OverviewGraph, RenderTarget};

pub use arch::{badge_width, delta_badge, fitted_badges, lane_header};
pub use design::DiagramStyle;
pub use draw::{Diagram, DiagramEvent, NodeMark, Zoom, dashes, kind_glyph, node_of, rounded};
pub use flow::{message_label, message_pitch};
pub use layout::{Chip, ItemId, Layout, Placed, PlacedKind, Route, RouteKind, hit_segments};
pub use model::{fold_columns, fold_indexes, key_families};

/// The architecture lens of what `scope` selects.
pub fn architecture(graph: &OverviewGraph, scope: &GraphViewScope) -> Layout {
    arch::architecture(graph, &scope::resolve(graph, scope), &graph.title)
}

/// Flow `flow_id` as a sequence diagram; `None` when the graph has no such flow.
pub fn data_flow(graph: &OverviewGraph, flow_id: &str) -> Option<Layout> {
    flow::data_flow(graph, flow_id)
}

/// The data-model lens of what `scope` selects.
pub fn data_model(graph: &OverviewGraph, scope: &GraphViewScope) -> Layout {
    model::data_model(graph, &scope::resolve(graph, scope), &graph.title)
}

/// The picture a render target names: a view's scope, a flow, or the whole lens. A data-flow
/// target without a flow draws the first flow its view (or the graph) holds.
pub fn render(graph: &OverviewGraph, target: &RenderTarget) -> Option<Layout> {
    let view = target.view.as_deref().and_then(|v| scope::find_view(&graph.views, v));
    let all = GraphViewScope::All;
    let sc = view.map_or(&all, |v| &v.scope);
    let mut out = match target.lens {
        GraphLens::Architecture => architecture(graph, sc),
        GraphLens::DataModel => data_model(graph, sc),
        GraphLens::DataFlow => {
            let flow = match &target.flow {
                Some(f) => f.clone(),
                None => scope::resolve(graph, sc)
                    .flows
                    .first()
                    .map(|f| f.id.clone())
                    .or_else(|| graph.flows.first().map(|f| f.id.clone()))?,
            };
            data_flow(graph, &flow)?
        }
    };
    out.title = target.title.clone();
    Some(out)
}

#[cfg(test)]
mod tests;
