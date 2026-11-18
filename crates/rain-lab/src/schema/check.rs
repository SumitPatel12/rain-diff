//! The JSON Schemas in `schema/` held to the serde types and to the samples, with the validator
//! `rain overview validate` runs on an overview's diagrams (`rain_overview::schema`).

use rain_overview::schema::{DiagramSchema, resolve, types, violations};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{CallTrees, Scene, SchemaId, States, Trace};

fn spec(id: SchemaId) -> Value {
    match id {
        SchemaId::A => DiagramSchema::A,
        SchemaId::B => DiagramSchema::B,
        SchemaId::C => DiagramSchema::C,
        SchemaId::S => DiagramSchema::S,
    }
    .spec()
    .clone()
}

/// Every way `v` breaks `node`, as `pointer: message`.
fn errors(root: &Value, node: &Value, v: &Value, at: &str) -> Vec<String> {
    violations(root, node, v, at)
        .into_iter()
        .map(|(p, m)| format!("{p}: {m}"))
        .collect()
}

/// A document for `node` with every property set, every enum and nullable at its first
/// non-null choice, and every array one item long (as long as `minItems` asks); arrays stop
/// nesting `depth` levels down.
fn full(root: &Value, node: &Value, depth: usize) -> Value {
    let node = resolve(root, node);
    if let Some(any) = node.get("anyOf").and_then(Value::as_array) {
        let branch = any
            .iter()
            .find(|n| !types(resolve(root, n)).contains(&"null"))
            .expect("a non-null branch");
        return full(root, branch, depth);
    }
    if let Some(e) = node.get("enum").and_then(Value::as_array) {
        return e.iter().find(|x| !x.is_null()).cloned().expect("a non-null value");
    }
    let ty = types(node)
        .into_iter()
        .find(|t| *t != "null")
        .unwrap_or_else(|| panic!("no type in {node}"));
    match ty {
        "object" => {
            let props = node.get("properties").and_then(Value::as_object).cloned().unwrap_or_default();
            Value::Object(props.iter().map(|(k, p)| (k.clone(), full(root, p, depth + 1))).collect())
        }
        "array" => {
            let n = node
                .get("minItems")
                .and_then(Value::as_u64)
                .unwrap_or(if depth > 8 { 0 } else { 1 });
            Value::Array((0..n).map(|_| full(root, &node["items"], depth + 1)).collect())
        }
        "string" => json!("x"),
        "number" => json!(1.5),
        "integer" => json!(2),
        "boolean" => json!(true),
        other => panic!("type {other}"),
    }
}

/// Every place in `v` with the schema node that governs it, by JSON pointer.
fn sites(root: &Value, node: &Value, v: &Value, at: &str, out: &mut Vec<(String, Value)>) {
    let node = resolve(root, node);
    if let Some(any) = node.get("anyOf").and_then(Value::as_array) {
        if let Some(branch) = any.iter().find(|n| errors(root, n, v, at).is_empty()) {
            sites(root, branch, v, at, out);
        }
        return;
    }
    out.push((at.to_owned(), node.clone()));
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if let Some(p) = node.get("properties").and_then(|p| p.get(k)) {
                    sites(root, p, x, &format!("{at}/{k}"), out);
                }
            }
        }
        Value::Array(a) => {
            for (i, x) in a.iter().enumerate() {
                sites(root, &node["items"], x, &format!("{at}/{i}"), out);
            }
        }
        _ => {}
    }
}

fn round<T: DeserializeOwned + Serialize>(v: &Value) -> Result<Value, String> {
    let t: T = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    serde_json::to_value(&t).map_err(|e| e.to_string())
}

fn reads<T: DeserializeOwned>(v: &Value) -> bool {
    serde_json::from_value::<T>(v.clone()).is_ok()
}

/// The sample is valid before and after a trip through `T`; a document with every property set
/// comes back unchanged; leaving a property out fails exactly when the schema requires it; each
/// enum value reads and one outside the enum doesn't.
fn agree<T: DeserializeOwned + Serialize>(id: SchemaId) {
    let root = spec(id);
    let l = id.letter();
    let sample: Value = serde_json::from_str(id.raw()).unwrap();
    assert_eq!(errors(&root, &root, &sample, ""), Vec::<String>::new(), "sample {l}");
    let back = round::<T>(&sample).unwrap();
    assert_eq!(errors(&root, &root, &back, ""), Vec::<String>::new(), "sample {l} after serde");

    let doc = full(&root, &root, 0);
    assert_eq!(errors(&root, &root, &doc, ""), Vec::<String>::new(), "{l}: the full document");
    assert_eq!(
        round::<T>(&doc),
        Ok(doc.clone()),
        "{l}: the type keeps every property and adds none"
    );

    let mut at = Vec::new();
    sites(&root, &root, &doc, "", &mut at);
    for (p, node) in &at {
        if let Some(props) = node.get("properties").and_then(Value::as_object)
            && doc.pointer(p).is_some_and(Value::is_object)
        {
            let required: Vec<&str> = node
                .get("required")
                .and_then(Value::as_array)
                .map(|r| r.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            for k in props.keys() {
                let mut less = doc.clone();
                less.pointer_mut(p).and_then(Value::as_object_mut).unwrap().remove(k);
                assert_eq!(
                    !reads::<T>(&less),
                    required.contains(&k.as_str()),
                    "{l}: {p}/{k} is required by the type exactly when the schema says so"
                );
            }
        }
        if let Some(e) = node.get("enum").and_then(Value::as_array) {
            let mut other = doc.clone();
            for x in e.iter().filter(|x| !x.is_null()) {
                *other.pointer_mut(p).unwrap() = x.clone();
                assert!(reads::<T>(&other), "{l}: {p} = {x} reads");
            }
            *other.pointer_mut(p).unwrap() = json!("not-in-the-enum");
            assert!(!reads::<T>(&other), "{l}: {p} refuses a value outside the enum");
        }
    }
}

#[test]
fn schema_a_matches_the_trace_and_its_sample() {
    agree::<Trace>(SchemaId::A);
}

#[test]
fn schema_b_matches_the_states_and_its_sample() {
    agree::<States>(SchemaId::B);
}

#[test]
fn schema_c_matches_the_call_trees_and_its_sample() {
    agree::<CallTrees>(SchemaId::C);
}

#[test]
fn schema_s_matches_the_scene_and_its_sample() {
    agree::<Scene>(SchemaId::S);
}

#[test]
fn the_validator_catches_what_it_should() {
    let root = spec(SchemaId::B);
    let bad = json!({
        "title": "t",
        "fields": [{ "id": "db:A.b", "stage": "table", "extra": 1, "after": { "name": "b" } }],
        "lineage": [{ "from": "x" }]
    });
    let e = errors(&root, &root, &bad, "");
    for want in [
        "/fields/0/stage: \"table\" is not one of",
        "/fields/0: unknown key extra",
        "/fields/0/after: type is required",
        "/lineage/0: to is required",
    ] {
        assert!(e.iter().any(|x| x.starts_with(want)), "{want} in {e:?}");
    }
    let s = spec(SchemaId::S);
    let short = json!({ "title": "t", "size": [1], "shapes": [], "beats": [] });
    assert_eq!(errors(&s, &s, &short, ""), vec!["/size: 1 items".to_owned()]);
}
