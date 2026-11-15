//! The same graph as Mermaid source, to compare our layout with `rune_mermaid::render`.

use rain_core::overview::{ColumnKey, EdgeEmphasis, GraphDelta, GraphLens, MessageKind, OverviewGraph};

fn id(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect()
}

fn quote(s: &str) -> String {
    s.replace('"', "'")
}

/// `flowchart LR` with a subgraph per lane, `sequenceDiagram` for the first flow, or `erDiagram`.
pub fn to_mermaid(graph: &OverviewGraph, lens: GraphLens) -> String {
    let mut out = String::new();
    match lens {
        GraphLens::Architecture => {
            out.push_str("flowchart LR\n");
            for lane in &graph.lanes {
                out.push_str(&format!("  subgraph {}[\"{}\"]\n", id(&lane.id), quote(&lane.label)));
                for n in graph.nodes.iter().filter(|n| n.lane == lane.id) {
                    let tag = match n.delta {
                        GraphDelta::Added => " (new)",
                        GraphDelta::Modified => " (changed)",
                        GraphDelta::Removed => " (removed)",
                        GraphDelta::Unchanged => "",
                    };
                    out.push_str(&format!("    {}[\"{}{tag}\"]\n", id(&n.id), quote(&n.label)));
                }
                out.push_str("  end\n");
            }
            for e in &graph.edges {
                let arrow = match (e.delta, e.emphasis) {
                    (GraphDelta::Removed, _) => "-.->",
                    (_, EdgeEmphasis::Hero) => "==>",
                    _ => "-->",
                };
                match &e.label {
                    Some(l) => out.push_str(&format!("  {} {arrow}|{}| {}\n", id(&e.from), quote(l), id(&e.to))),
                    None => out.push_str(&format!("  {} {arrow} {}\n", id(&e.from), id(&e.to))),
                }
            }
        }
        GraphLens::DataFlow => {
            out.push_str("sequenceDiagram\n");
            let Some(flow) = graph.flows.first() else { return out };
            for p in &flow.participants {
                let label = p
                    .label
                    .clone()
                    .or_else(|| graph.nodes.iter().find(|n| n.id == p.node).map(|n| n.label.clone()))
                    .unwrap_or_else(|| p.node.clone());
                out.push_str(&format!("  participant {} as {}\n", id(&p.node), quote(&label)));
            }
            for m in &flow.messages {
                let arrow = match m.kind {
                    MessageKind::Sync | MessageKind::SelfCall => "->>",
                    MessageKind::Async => "-)",
                    MessageKind::Return => "-->>",
                };
                out.push_str(&format!("  {}{arrow}{}: {}\n", id(&m.from), id(&m.to), quote(&m.label)));
            }
        }
        GraphLens::DataModel => {
            out.push_str("erDiagram\n");
            let Some(dm) = &graph.data_model else { return out };
            let name = |t: &str| dm.tables.iter().find(|x| x.id == t).map_or_else(|| id(t), |x| id(&x.label));
            for t in &dm.tables {
                out.push_str(&format!("  {} {{\n", id(&t.label)));
                for c in &t.columns {
                    let key = match c.key {
                        Some(ColumnKey::Primary) => " PK",
                        Some(ColumnKey::Foreign) => " FK",
                        _ => "",
                    };
                    out.push_str(&format!("    {} {}{key}\n", id(&c.ty), id(&c.name)));
                }
                out.push_str("  }\n");
            }
            for r in &dm.relations {
                out.push_str(&format!(
                    "  {} }}o--|| {} : \"{}\"\n",
                    name(&r.from.table),
                    name(&r.to.table),
                    quote(&r.from.column)
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lens_renders_through_rune_mermaid() {
        let text = include_str!("../../../fixtures/overview/sample-overview-schema.json");
        let g = serde_json::from_str::<rain_core::OverviewDocument>(text).unwrap().graph;
        let style = rune_mermaid::DiagramStyle::neutral(true);
        for lens in [GraphLens::Architecture, GraphLens::DataFlow, GraphLens::DataModel] {
            let src = to_mermaid(&g, lens);
            assert!(rune_mermaid::render(&src, &style).is_some(), "{lens:?}:\n{src}");
        }
    }
}
