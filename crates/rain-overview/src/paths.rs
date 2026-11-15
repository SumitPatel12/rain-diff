//! Port of difit `server/overview/paths.ts`: repo-prefixed paths and every file reference a graph carries.

use rain_core::overview::{GraphFileRef, OverviewGraph};

/// `server/src/A.cs` → `("server", "src/A.cs")`; `None` when there is no repo segment before a `/`.
pub fn split_repo_path(path: &str) -> Option<(&str, &str)> {
    match path.find('/') {
        Some(i) if i > 0 => Some((&path[..i], &path[i + 1..])),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectedFileRef {
    pub path: String,
    /// Relative to the graph, e.g. `nodes[3].files[0].path`.
    pub json_path: String,
}

fn add_files(out: &mut Vec<CollectedFileRef>, files: &[GraphFileRef], at: &str) {
    for (i, f) in files.iter().enumerate() {
        out.push(CollectedFileRef {
            path: f.path.clone(),
            json_path: format!("{at}[{i}].path"),
        });
    }
}

pub fn collect_file_refs(graph: &OverviewGraph) -> Vec<CollectedFileRef> {
    let mut refs = Vec::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        add_files(&mut refs, &node.files, &format!("nodes[{i}].files"));
    }
    for (i, edge) in graph.edges.iter().enumerate() {
        add_files(&mut refs, &edge.files, &format!("edges[{i}].files"));
    }
    for (i, flow) in graph.flows.iter().enumerate() {
        for (j, message) in flow.messages.iter().enumerate() {
            let at = format!("flows[{i}].messages[{j}]");
            add_files(&mut refs, &message.files, &format!("{at}.files"));
            for side in ["request", "response"] {
                let source = message.payload.as_ref().and_then(|p| p.get(side)).and_then(|s| s.get("source"));
                if let Some(path) = source.and_then(|s| s.get("path")).and_then(|p| p.as_str()) {
                    refs.push(CollectedFileRef {
                        path: path.to_owned(),
                        json_path: format!("{at}.payload.{side}.source.path"),
                    });
                }
            }
        }
    }
    if let Some(model) = &graph.data_model {
        for (i, table) in model.tables.iter().enumerate() {
            add_files(&mut refs, &table.files, &format!("dataModel.tables[{i}].files"));
            for (j, index) in table.indexes.iter().enumerate() {
                add_files(&mut refs, &index.files, &format!("dataModel.tables[{i}].indexes[{j}].files"));
            }
        }
    }
    if let Some(walkthrough) = &graph.walkthrough {
        for (i, step) in walkthrough.steps.iter().enumerate() {
            for (j, cite) in step.detail.iter().flat_map(|d| d.cites.iter()).enumerate() {
                if let rain_core::overview::CiteRef::File { path } = &cite.reference {
                    refs.push(CollectedFileRef {
                        path: path.clone(),
                        json_path: format!("walkthrough.steps[{i}].detail.cites[{j}].ref.path"),
                    });
                }
            }
        }
    }
    refs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{sample, schema_sample};

    #[test]
    fn split_repo_path_splits_or_refuses() {
        assert_eq!(split_repo_path("server/src/A.cs"), Some(("server", "src/A.cs")));
        assert_eq!(split_repo_path("noslash"), None);
        assert_eq!(split_repo_path("/abs"), None);
    }

    #[test]
    fn collects_every_file_ref_of_the_fixture() {
        let refs = collect_file_refs(&sample().graph);
        assert_eq!(refs.len(), 10);
        assert!(refs.contains(&CollectedFileRef {
            path: "server/src/Api/VouchersController.cs".into(),
            json_path: "nodes[3].files[0].path".into()
        }));
        assert!(refs.contains(&CollectedFileRef {
            path: "server/src/Api/VouchersController.cs".into(),
            json_path: "flows[0].messages[1].files[0].path".into()
        }));
    }

    #[test]
    fn includes_index_files() {
        assert!(collect_file_refs(&schema_sample().graph).contains(&CollectedFileRef {
            path: "server/src/Migrations/20260101000000_AddVoucher.cs".into(),
            json_path: "dataModel.tables[0].indexes[0].files[0].path".into()
        }));
    }

    #[test]
    fn includes_payload_sources_and_file_cites() {
        let mut doc = sample();
        doc.graph.flows[0].messages[0].payload =
            Some(serde_json::json!({ "request": { "type": "VoucherRequest", "source": { "path": "app/src/app/vouchers/voucher.ts" } } }));
        doc.graph.walkthrough.as_mut().unwrap().steps[0].detail.as_mut().unwrap().cites[2].reference = rain_core::overview::CiteRef::File {
            path: "server/src/Services/VoucherService.cs".into(),
        };
        let paths: Vec<_> = collect_file_refs(&doc.graph).into_iter().map(|r| r.json_path).collect();
        assert!(paths.contains(&"flows[0].messages[0].payload.request.source.path".to_owned()));
        assert!(paths.contains(&"walkthrough.steps[0].detail.cites[2].ref.path".to_owned()));
    }
}
