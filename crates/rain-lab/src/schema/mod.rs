//! The four data shapes the playground's styles read, each with one sample describing the same
//! change ("Vouchers at checkout"), so the styles can be compared side by side. Ids are shared
//! across samples wherever they name the same thing (`voucher-service` is the same class in all).
//!
//! Each schema is specified in `schema/*.schema.json` and described in `schema/README.md`;
//! serde reads the samples (unknown keys ignored), and the tests hold the specs, the types and
//! the samples to each other.

pub mod calls;
pub mod scene;
pub mod state;
pub mod trace;

#[cfg(test)]
mod check;

use std::sync::OnceLock;

pub use calls::CallTrees;
pub use scene::Scene;
pub use state::States;
pub use trace::Trace;

/// One of the four schemas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SchemaId {
    /// Trace, paths and tour.
    A,
    /// Fields before and after.
    B,
    /// Call trees before and after.
    C,
    /// A hand-written scene script.
    S,
}

impl SchemaId {
    pub const ALL: [SchemaId; 4] = [SchemaId::A, SchemaId::B, SchemaId::C, SchemaId::S];

    /// The one letter on a chip.
    pub fn letter(self) -> &'static str {
        match self {
            SchemaId::A => "A",
            SchemaId::B => "B",
            SchemaId::C => "C",
            SchemaId::S => "S",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            SchemaId::A => "Trace, paths and tour",
            SchemaId::B => "Fields before and after",
            SchemaId::C => "Call trees before and after",
            SchemaId::S => "Scene script",
        }
    }

    /// The sample's JSON as written.
    pub fn raw(self) -> &'static str {
        match self {
            SchemaId::A => trace::RAW,
            SchemaId::B => state::RAW,
            SchemaId::C => calls::RAW,
            SchemaId::S => scene::RAW,
        }
    }
}

/// One document per schema: the samples, or a review's diagrams.
#[derive(Clone, Debug)]
pub struct Samples {
    pub a: Trace,
    pub b: States,
    pub c: CallTrees,
    pub s: Scene,
}

impl Samples {
    /// Parses every sample; the error names the schema and serde's path.
    pub fn parse() -> Result<Samples, String> {
        fn one<T: serde::de::DeserializeOwned>(id: SchemaId) -> Result<T, String> {
            serde_json::from_str(id.raw()).map_err(|e| format!("sample {}: {e}", id.letter()))
        }
        Ok(Samples {
            a: one(SchemaId::A)?,
            b: one(SchemaId::B)?,
            c: one(SchemaId::C)?,
            s: one(SchemaId::S)?,
        })
    }
}

/// A review's diagrams read into the styles' types: the documents, with the built-in samples
/// standing in for the kinds the overview leaves out, and the registry indices of the styles it
/// carries, in the registry's order. A document that doesn't read is left out.
pub fn review_diagrams(d: &rain_core::overview::OverviewDiagrams) -> (Samples, Vec<usize>) {
    fn read<T: serde::de::DeserializeOwned>(v: &Option<serde_json::Value>) -> Option<T> {
        v.as_ref().and_then(|v| T::deserialize(v).ok())
    }
    let mut out = samples().clone();
    let mut ids = Vec::new();
    if let Some(a) = read(&d.tour) {
        out.a = a;
        ids.push("tour");
    }
    if let Some(s) = read(&d.flowchart) {
        out.s = s;
        ids.push("flowchart");
    }
    if let Some(b) = read(&d.lineage) {
        out.b = b;
        ids.push("lineage");
    }
    if let Some(c) = read(&d.icicle) {
        out.c = c;
        ids.push("icicle");
    }
    let styles = crate::registry()
        .iter()
        .enumerate()
        .filter(|(_, s)| ids.contains(&s.id()))
        .map(|(i, _)| i)
        .collect();
    (out, styles)
}

/// The samples, parsed on first use. The samples are compiled in and tested, so a parse
/// failure is a bug in this crate.
pub fn samples() -> &'static Samples {
    static SAMPLES: OnceLock<Samples> = OnceLock::new();
    SAMPLES.get_or_init(|| Samples::parse().expect("the compiled-in samples parse"))
}

/// What a change did to one thing, worked out from its before and after.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Added,
    Changed,
    Removed,
    Same,
}

impl Status {
    /// Added when only `after` exists, removed when only `before` does, changed when they
    /// differ.
    pub fn of<T: PartialEq>(before: Option<&T>, after: Option<&T>) -> Status {
        match (before, after) {
            (None, Some(_)) => Status::Added,
            (Some(_), None) => Status::Removed,
            (Some(a), Some(b)) if a != b => Status::Changed,
            _ => Status::Same,
        }
    }
}
