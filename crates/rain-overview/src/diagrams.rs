//! The overview's optional `diagrams` block: one document per animated style (`tour` reads
//! schema A, `flowchart` S, `lineage` B, `icicle` C). Each is checked against its schema, then
//! for ids that point nowhere; size budgets only warn.

use std::collections::BTreeSet;

use rain_core::overview::Issue;
use serde_json::Value;

use crate::schema::{DiagramSchema, dotted, violations};

/// The keys of `diagrams`, in the order the Map shows them, with the schema each reads.
pub const DIAGRAM_KINDS: [(&str, DiagramSchema); 4] = [
    ("tour", DiagramSchema::A),
    ("flowchart", DiagramSchema::S),
    ("lineage", DiagramSchema::B),
    ("icicle", DiagramSchema::C),
];

const INVALID_DIAGRAM: &str = "INVALID_DIAGRAM";

/// Over these a diagram still draws, but crowds: (kind, what is counted, at most).
const BUDGETS: &[(&str, &str, usize)] = &[
    ("tour", "entities", 14),
    ("tour", "tour", 8),
    ("flowchart", "shapes", 24),
    ("flowchart", "beats", 12),
    ("lineage", "fields", 30),
    ("icicle", "frames", 40),
];

fn issue(code: &str, path: String, message: impl Into<String>) -> Issue {
    Issue {
        code: code.to_owned(),
        path,
        message: message.into(),
    }
}

/// Every issue in `diagrams` (the block's value), at `diagrams.<kind>...` paths.
pub fn check(diagrams: &Value) -> Vec<Issue> {
    let Some(m) = diagrams.as_object() else {
        return vec![issue(INVALID_DIAGRAM, "diagrams".into(), "expected an object")];
    };
    let mut out = Vec::new();
    for k in m.keys() {
        if !DIAGRAM_KINDS.iter().any(|(n, _)| n == k) {
            out.push(issue(
                INVALID_DIAGRAM,
                "diagrams".into(),
                format!("Unrecognized key: \"{k}\" (expected tour, flowchart, lineage or icicle)"),
            ));
        }
    }
    for (kind, schema) in DIAGRAM_KINDS {
        let Some(doc) = m.get(kind) else { continue };
        let root = schema.spec();
        let at = format!("diagrams.{kind}");
        let shape: Vec<Issue> = violations(root, root, doc, "")
            .into_iter()
            .map(|(p, msg)| issue(INVALID_DIAGRAM, dotted(&at, &p), msg))
            .collect();
        if shape.is_empty() {
            references(kind, doc, &at, &mut out);
        }
        out.extend(shape);
    }
    out
}

/// Diagrams over their size budget, as warnings.
pub fn budget_warnings(diagrams: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for &(kind, what, max) in BUDGETS {
        let Some(doc) = diagrams.get(kind) else { continue };
        let n = if what == "frames" {
            frames(doc).len()
        } else {
            doc.get(what).and_then(Value::as_array).map_or(0, Vec::len)
        };
        if n > max {
            out.push(format!(
                "diagrams.{kind} has {n} {what}; keep it to {max} or fewer so it reads at a glance"
            ));
        }
    }
    out
}

fn ids<'a>(v: &'a Value, list: &str, key: &str) -> Vec<(usize, &'a str)> {
    v.get(list)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(i, x)| Some((i, x.get(key)?.as_str()?)))
        .collect()
}

fn unique<'a>(found: &[(String, &'a str)], what: &str, out: &mut Vec<Issue>) -> BTreeSet<&'a str> {
    let mut seen = BTreeSet::new();
    for (path, id) in found {
        if !seen.insert(*id) {
            out.push(issue(
                "DUPLICATE_ID",
                path.clone(),
                format!("{what} id '{id}' is used more than once"),
            ));
        }
    }
    seen
}

fn known(set: &BTreeSet<&str>, id: &str, path: String, what: &str, out: &mut Vec<Issue>) {
    if !set.contains(id) {
        out.push(issue(
            "BROKEN_REFERENCE",
            path,
            format!("names {what} '{id}', which the diagram does not have"),
        ));
    }
}

fn strs(v: Option<&Value>) -> impl Iterator<Item = (usize, &str)> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(i, x)| Some((i, x.as_str()?)))
}

/// Every call frame of an icicle document with its path.
fn frames(doc: &Value) -> Vec<(String, &Value)> {
    fn walk<'a>(f: &'a Value, at: String, out: &mut Vec<(String, &'a Value)>) {
        for (i, c) in f.get("children").and_then(Value::as_array).into_iter().flatten().enumerate() {
            walk(c, format!("{at}.children[{i}]"), out);
        }
        out.push((at, f));
    }
    let mut out = Vec::new();
    if let Some(c) = doc.get("calls").filter(|c| c.is_object()) {
        walk(c, "calls".into(), &mut out);
    }
    for (i, p) in doc.get("profiles").and_then(Value::as_array).into_iter().flatten().enumerate() {
        if let Some(c) = p.get("calls") {
            walk(c, format!("profiles[{i}].calls"), &mut out);
        }
    }
    out
}

/// Ids that must be unique and references that must land, per kind; the shape already passed.
fn references(kind: &str, doc: &Value, at: &str, out: &mut Vec<Issue>) {
    let p = |rest: String| format!("{at}.{rest}");
    match kind {
        "tour" => {
            let ents: Vec<(String, &str)> = ids(doc, "entities", "id")
                .into_iter()
                .map(|(i, id)| (p(format!("entities[{i}].id")), id))
                .collect();
            let ents = unique(&ents, "entity", out);
            let paths: Vec<(String, &str)> = ids(doc, "paths", "id")
                .into_iter()
                .map(|(i, id)| (p(format!("paths[{i}].id")), id))
                .collect();
            unique(&paths, "path", out);
            for (i, e) in doc["entities"].as_array().into_iter().flatten().enumerate() {
                if let Some(parent) = e.get("parent").and_then(Value::as_str) {
                    known(&ents, parent, p(format!("entities[{i}].parent")), "entity", out);
                }
            }
            for (i, path) in doc["paths"].as_array().into_iter().flatten().enumerate() {
                for (j, s) in path["steps"].as_array().into_iter().flatten().enumerate() {
                    for end in ["from", "to"] {
                        if let Some(id) = s.get(end).and_then(Value::as_str) {
                            known(&ents, id, p(format!("paths[{i}].steps[{j}].{end}")), "entity", out);
                        }
                    }
                    for (k, id) in strs(s.get("carries")) {
                        known(&ents, id, p(format!("paths[{i}].steps[{j}].carries[{k}]")), "entity", out);
                    }
                }
                if path["steps"].as_array().is_some_and(Vec::is_empty) {
                    out.push(issue(
                        INVALID_DIAGRAM,
                        p(format!("paths[{i}].steps")),
                        "a path needs at least one step",
                    ));
                }
            }
            for (i, t) in doc.get("tour").and_then(Value::as_array).into_iter().flatten().enumerate() {
                for (k, id) in strs(t.get("focus")) {
                    known(&ents, id, p(format!("tour[{i}].focus[{k}]")), "entity", out);
                }
            }
        }
        "flowchart" => {
            let shapes: Vec<(String, &str)> = ids(doc, "shapes", "id")
                .into_iter()
                .map(|(i, id)| (p(format!("shapes[{i}].id")), id))
                .collect();
            let shapes = unique(&shapes, "shape", out);
            for (i, b) in doc["beats"].as_array().into_iter().flatten().enumerate() {
                for (j, a) in b["actions"].as_array().into_iter().flatten().enumerate() {
                    let target = a.get("target").and_then(Value::as_str).unwrap_or_default();
                    let focus = a.get("do").and_then(Value::as_str) == Some("focus");
                    if !(target.is_empty() && focus) {
                        known(&shapes, target, p(format!("beats[{i}].actions[{j}].target")), "shape", out);
                    }
                }
            }
        }
        "lineage" => {
            let fields: Vec<(String, &str)> = ids(doc, "fields", "id")
                .into_iter()
                .map(|(i, id)| (p(format!("fields[{i}].id")), id))
                .collect();
            let fields = unique(&fields, "field", out);
            for (i, f) in doc.get("fields").and_then(Value::as_array).into_iter().flatten().enumerate() {
                if let Some(r) = f.get("references").and_then(Value::as_str) {
                    known(&fields, r, p(format!("fields[{i}].references")), "field", out);
                }
            }
            for (i, l) in doc.get("lineage").and_then(Value::as_array).into_iter().flatten().enumerate() {
                for end in ["from", "to"] {
                    if let Some(id) = l.get(end).and_then(Value::as_str) {
                        known(&fields, id, p(format!("lineage[{i}].{end}")), "field", out);
                    }
                }
                for (k, id) in strs(l.get("via")) {
                    known(&fields, id, p(format!("lineage[{i}].via[{k}]")), "field", out);
                }
            }
        }
        "icicle" => {
            let all = frames(doc);
            if all.is_empty() {
                out.push(issue(
                    INVALID_DIAGRAM,
                    at.to_owned(),
                    "an icicle needs calls or at least one profile",
                ));
            }
            let found: Vec<(String, &str)> = all
                .iter()
                .filter_map(|(path, f)| Some((p(format!("{path}.id")), f.get("id")?.as_str()?)))
                .collect();
            unique(&found, "frame", out);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn samples() -> Value {
        json!({
            "tour": serde_json::from_str::<Value>(include_str!("../../rain-lab/samples/a.json")).unwrap(),
            "flowchart": serde_json::from_str::<Value>(include_str!("../../rain-lab/samples/s.json")).unwrap(),
            "lineage": serde_json::from_str::<Value>(include_str!("../../rain-lab/samples/b.json")).unwrap(),
            "icicle": serde_json::from_str::<Value>(include_str!("../../rain-lab/samples/c.json")).unwrap(),
        })
    }

    #[test]
    fn the_playground_samples_pass() {
        assert_eq!(check(&samples()), Vec::new());
    }

    #[test]
    fn shape_faults_carry_dotted_paths() {
        let mut d = samples();
        d["tour"]["paths"][0]["steps"][1]["status"] = json!("moved");
        d["lineage"]["fields"][0]["extra"] = json!(1);
        d["icicle"].as_object_mut().unwrap().remove("title");
        d["other"] = json!({});
        let got: Vec<(String, String)> = check(&d).into_iter().map(|i| (i.code, i.path)).collect();
        for want in [
            ("INVALID_DIAGRAM", "diagrams.tour.paths[0].steps[1].status"),
            ("INVALID_DIAGRAM", "diagrams.lineage.fields[0]"),
            ("INVALID_DIAGRAM", "diagrams.icicle"),
            ("INVALID_DIAGRAM", "diagrams"),
        ] {
            assert!(got.iter().any(|(c, p)| c == want.0 && p == want.1), "{want:?} in {got:?}");
        }
    }

    #[test]
    fn ids_that_point_nowhere_are_named() {
        let mut d = samples();
        d["tour"]["paths"][0]["steps"][0]["to"] = json!("nowhere");
        d["flowchart"]["beats"][0]["actions"][0]["target"] = json!("ghost");
        d["lineage"]["lineage"][0]["from"] = json!("db:Gone.Id");
        let first = d["icicle"]["calls"]["id"].clone();
        d["icicle"]["calls"]["children"][0]["id"] = first;
        let got: Vec<(String, String)> = check(&d).into_iter().map(|i| (i.code, i.path)).collect();
        for want in [
            ("BROKEN_REFERENCE", "diagrams.tour.paths[0].steps[0].to"),
            ("BROKEN_REFERENCE", "diagrams.flowchart.beats[0].actions[0].target"),
            ("BROKEN_REFERENCE", "diagrams.lineage.lineage[0].from"),
            ("DUPLICATE_ID", "diagrams.icicle.calls.id"),
        ] {
            assert!(got.iter().any(|(c, p)| c == want.0 && p == want.1), "{want:?} in {got:?}");
        }
    }

    #[test]
    fn the_guide_s_examples_pass() {
        let guide = include_str!("../../../skills/rain/references/overview-guide.md");
        let start = guide.find("## 7a.").expect("§7a");
        let section = &guide[start..start + guide[start..].find("## 8.").expect("§8")];
        let docs: Vec<Value> = section
            .split("```json")
            .skip(1)
            .map(|b| serde_json::from_str(&b[..b.find("```").unwrap()]).unwrap())
            .collect();
        assert_eq!(docs.len(), 4);
        let d = json!({ "tour": docs[0], "flowchart": docs[1], "lineage": docs[2], "icicle": docs[3] });
        assert_eq!(check(&d), Vec::new());
        assert!(budget_warnings(&d).is_empty());
    }

    #[test]
    fn budgets_warn() {
        let mut d = samples();
        let e = d["tour"]["entities"][0].clone();
        let list = d["tour"]["entities"].as_array_mut().unwrap();
        while list.len() <= 14 {
            let mut x = e.clone();
            x["id"] = json!(format!("extra-{}", list.len()));
            list.push(x);
        }
        let w = budget_warnings(&d);
        assert!(w.iter().any(|w| w.starts_with("diagrams.tour has 15 entities")), "{w:?}");
        assert!(budget_warnings(&samples()).is_empty(), "{:?}", budget_warnings(&samples()));
    }
}
