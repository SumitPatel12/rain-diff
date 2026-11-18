//! Schema A: what happens, in order. Entities (components, functions, tables, data objects), the
//! paths a request takes through them as ordered steps, and a tour whose prose links back to
//! entities and paths with `[[id]]` spans. Spec: `schema/a.schema.json`.

use serde::{Deserialize, Serialize};

use super::Status;

pub const RAW: &str = include_str!("../../samples/a.json");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Trace {
    pub title: String,
    pub entities: Vec<Entity>,
    pub paths: Vec<TracePath>,
    #[serde(default)]
    pub tour: Vec<TourStep>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    pub label: String,
    /// `web` or `ui`, `api` or `domain`, `data` or `db`: its tier on the map.
    pub layer: String,
    /// The entity it sits inside: a component's page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// A data object's fields worth naming when it travels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TracePath {
    pub id: String,
    pub name: String,
    pub steps: Vec<TraceStep>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraceStep {
    pub from: String,
    pub to: String,
    /// What the call says: `RecordVoucher(id, amount)`.
    pub call: String,
    /// The data objects travelling with it.
    #[serde(default)]
    pub carries: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fragment: Option<Fragment>,
    pub status: Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Fragment {
    Alt,
    Loop,
    Opt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TourStep {
    pub title: String,
    /// `[[id]]` names an entity or a path.
    pub text: String,
    #[serde(default)]
    pub focus: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraKey>,
}

/// Limits on the camera while a step plays.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraKey {
    /// The most it zooms in while framing the step's focus.
    pub zoom: f32,
}

/// A piece of tour text: plain, or a `[[id]]` link.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Span<'a> {
    Text(&'a str),
    Link(&'a str),
}

/// `text` cut into plain runs and `[[id]]` links, in order.
pub fn spans(text: &str) -> Vec<Span<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("[[") {
        let Some(close) = rest[open + 2..].find("]]") else { break };
        if open > 0 {
            out.push(Span::Text(&rest[..open]));
        }
        out.push(Span::Link(&rest[open + 2..open + 2 + close]));
        rest = &rest[open + 2 + close + 2..];
    }
    if !rest.is_empty() {
        out.push(Span::Text(rest));
    }
    out
}

impl Trace {
    pub fn entity(&self, id: &str) -> Option<&Entity> {
        self.entities.iter().find(|e| e.id == id)
    }

    pub fn path(&self, id: &str) -> Option<&TracePath> {
        self.paths.iter().find(|p| p.id == id)
    }
}
