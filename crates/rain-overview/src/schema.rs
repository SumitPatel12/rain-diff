//! The diagram JSON Schemas (`crates/rain-lab/schema/*.schema.json`) and a small validator for
//! what they use: `type`, `properties`, `required`, `additionalProperties`, `items`, `minItems`,
//! `maxItems`, `enum`, `anyOf` and `$ref` into `$defs`. rain-lab's tests hold the schemas to its
//! serde types with the same validator.

use std::sync::OnceLock;

use serde_json::{Value, json};

/// One of the four diagram schemas, by its letter in rain-lab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagramSchema {
    A,
    B,
    C,
    S,
}

impl DiagramSchema {
    fn raw(self) -> &'static str {
        match self {
            DiagramSchema::A => include_str!("../../rain-lab/schema/a.schema.json"),
            DiagramSchema::B => include_str!("../../rain-lab/schema/b.schema.json"),
            DiagramSchema::C => include_str!("../../rain-lab/schema/c.schema.json"),
            DiagramSchema::S => include_str!("../../rain-lab/schema/s.schema.json"),
        }
    }

    /// The schema, parsed once. The files are compiled in and tested, so a parse failure is a bug.
    pub fn spec(self) -> &'static Value {
        static SPECS: [OnceLock<Value>; 4] = [const { OnceLock::new() }; 4];
        let i = self as usize;
        SPECS[i].get_or_init(|| serde_json::from_str(self.raw()).unwrap_or_else(|e| panic!("schema {self:?}: {e}")))
    }
}

/// `node`, its `$ref` followed.
pub fn resolve<'a>(root: &'a Value, node: &'a Value) -> &'a Value {
    match node.get("$ref").and_then(Value::as_str) {
        Some(r) => {
            let name = r.strip_prefix("#/$defs/").unwrap_or_else(|| panic!("{r}: only local refs"));
            let def = root["$defs"].get(name).unwrap_or_else(|| panic!("{r}: no such def"));
            resolve(root, def)
        }
        None => node,
    }
}

fn is(ty: &str, v: &Value) -> bool {
    match ty {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "number" => v.is_number(),
        "integer" => v.is_i64() || v.is_u64(),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        other => panic!("type {other}"),
    }
}

/// The types a schema node allows.
pub fn types(node: &Value) -> Vec<&str> {
    match node.get("type") {
        Some(Value::String(t)) => vec![t.as_str()],
        Some(Value::Array(ts)) => ts.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

/// A value's JSON for a message, cut short.
fn brief(v: &Value) -> String {
    let s = v.to_string();
    match s.char_indices().nth(60) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s,
    }
}

/// Every way `v` breaks `node`, each as (JSON pointer, message).
fn validate(root: &Value, node: &Value, v: &Value, at: &str, out: &mut Vec<(String, String)>) {
    let node = resolve(root, node);
    if let Some(any) = node.get("anyOf").and_then(Value::as_array) {
        if any.iter().any(|n| violations(root, n, v, at).is_empty()) {
            return;
        }
        // One branch of the value's type: its faults say more than "no branch fits".
        let typed: Vec<&Value> = any.iter().filter(|n| types(resolve(root, n)).iter().any(|t| is(t, v))).collect();
        match typed[..] {
            [one] => validate(root, one, v, at, out),
            _ => out.push((at.to_owned(), "fits no anyOf branch".to_owned())),
        }
        return;
    }
    let ts = types(node);
    if !ts.is_empty() && !ts.iter().any(|t| is(t, v)) {
        out.push((at.to_owned(), format!("expected {ts:?}, found {}", brief(v))));
        return;
    }
    if let Some(e) = node.get("enum").and_then(Value::as_array)
        && !e.contains(v)
    {
        let allowed: Vec<String> = e.iter().map(Value::to_string).collect();
        out.push((at.to_owned(), format!("{v} is not one of {}", allowed.join(", "))));
    }
    match v {
        Value::Object(m) => {
            let props = node.get("properties").and_then(Value::as_object);
            for r in node.get("required").and_then(Value::as_array).into_iter().flatten() {
                let r = r.as_str().unwrap_or_default();
                if !m.contains_key(r) {
                    out.push((at.to_owned(), format!("{r} is required")));
                }
            }
            for (k, x) in m {
                match props.and_then(|p| p.get(k)) {
                    Some(p) => validate(root, p, x, &format!("{at}/{k}"), out),
                    None if node.get("additionalProperties") == Some(&json!(false)) => {
                        out.push((at.to_owned(), format!("unknown key {k}")))
                    }
                    None => {}
                }
            }
        }
        Value::Array(a) => {
            let n = a.len() as u64;
            if node.get("minItems").and_then(Value::as_u64).is_some_and(|m| n < m)
                || node.get("maxItems").and_then(Value::as_u64).is_some_and(|m| n > m)
            {
                out.push((at.to_owned(), format!("{n} items")));
            }
            if let Some(items) = node.get("items") {
                for (i, x) in a.iter().enumerate() {
                    validate(root, items, x, &format!("{at}/{i}"), out);
                }
            }
        }
        _ => {}
    }
}

/// Every way `v` breaks schema node `node` of `root`, each as (JSON pointer under `at`, message).
pub fn violations(root: &Value, node: &Value, v: &Value, at: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    validate(root, node, v, at, &mut out);
    out
}

/// `/paths/0/steps/1` under `diagrams.tour` as `diagrams.tour.paths[0].steps[1]`.
pub fn dotted(prefix: &str, pointer: &str) -> String {
    let mut out = prefix.to_owned();
    for seg in pointer.split('/').skip(1) {
        if !seg.is_empty() && seg.bytes().all(|b| b.is_ascii_digit()) {
            out.push_str(&format!("[{seg}]"));
        } else {
            if !out.is_empty() {
                out.push('.');
            }
            out.push_str(seg);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schemas_parse_and_paths_read_like_the_overview_s() {
        for s in [DiagramSchema::A, DiagramSchema::B, DiagramSchema::C, DiagramSchema::S] {
            assert!(s.spec().is_object(), "{s:?}");
        }
        assert_eq!(
            dotted("diagrams.tour", "/paths/0/steps/1/from"),
            "diagrams.tour.paths[0].steps[1].from"
        );
        assert_eq!(dotted("diagrams.tour", ""), "diagrams.tour");
    }
}
