//! Serde mirror of difit's `src/types/overview.ts`: the overview document and the PR Lens
//! GraphDoc (contract 0.3.x) inside it, with zod's defaults applied on read.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const OVERVIEW_FORMAT: &str = "difit-overview/v1";

type Extra = BTreeMap<String, Value>;

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewRepo {
    pub id: String,
    pub dir: String,
    pub base: String,
    /// A sha, or `WORKTREE` for uncommitted work on top of HEAD.
    pub head: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadingOrderEntry {
    pub path: String,
    pub why: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk: Option<RiskLevel>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GraphRevision {
    #[default]
    Head,
    Base,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewNote {
    pub id: String,
    pub path: String,
    pub start_line: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<GraphRevision>,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IgnoreEntry {
    pub glob: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoReadingOrder {
    pub repo: String,
    #[serde(default)]
    pub review: Vec<ReadingOrderEntry>,
    #[serde(default)]
    pub ignore: Vec<IgnoreEntry>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PhaseStatus {
    Done,
    Pending,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewPhase {
    pub id: String,
    pub title: String,
    pub status: PhaseStatus,
    pub today: String,
    pub proposed: String,
    #[serde(default)]
    pub node_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewDocument {
    pub format: String,
    pub title: String,
    pub repos: Vec<OverviewRepo>,
    pub graph: OverviewGraph,
    #[serde(default)]
    pub reading_order: Vec<RepoReadingOrder>,
    #[serde(default)]
    pub phases: Vec<OverviewPhase>,
    #[serde(default)]
    pub notes: Vec<OverviewNote>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GraphDelta {
    Added,
    #[default]
    Modified,
    Removed,
    Unchanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GraphLens {
    Architecture,
    DataFlow,
    DataModel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphFileRef {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<GraphRevision>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphLane {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<GraphDelta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub delta: GraphDelta,
    pub lane: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub files: Vec<GraphFileRef>,
    #[serde(default)]
    pub badges: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeEmphasis {
    #[default]
    Normal,
    Hero,
    Muted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: String,
    pub delta: GraphDelta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub emphasis: EdgeEmphasis,
    #[serde(default)]
    pub animated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub files: Vec<GraphFileRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageKind {
    #[default]
    Sync,
    Async,
    Return,
    #[serde(rename = "self")]
    SelfCall,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphFlowMessage {
    pub id: String,
    pub from: String,
    pub to: String,
    pub label: String,
    #[serde(default)]
    pub kind: MessageKind,
    pub delta: GraphDelta,
    #[serde(default = "yes")]
    pub animated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default)]
    pub files: Vec<GraphFileRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowParticipant {
    pub node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GraphFlow {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub delta: GraphDelta,
    pub participants: Vec<FlowParticipant>,
    pub messages: Vec<GraphFlowMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum GraphViewScope {
    #[default]
    All,
    Selection {
        #[serde(default)]
        lanes: Vec<String>,
        #[serde(default)]
        nodes: Vec<String>,
        #[serde(default)]
        edges: Vec<String>,
        #[serde(default)]
        flows: Vec<String>,
        #[serde(default)]
        tables: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphView {
    pub id: String,
    pub title: String,
    pub lens: GraphLens,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub scope: GraphViewScope,
    #[serde(default)]
    pub default_open: bool,
    #[serde(default)]
    pub children: Vec<GraphView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum StepStage {
    View { view: String },
    Flow { flow: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum StepFocus {
    #[default]
    All,
    Selection {
        #[serde(default)]
        lanes: Vec<String>,
        #[serde(default)]
        nodes: Vec<String>,
        #[serde(default)]
        edges: Vec<String>,
        #[serde(default)]
        messages: Vec<String>,
        #[serde(default)]
        tables: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CiteRef {
    Node { node: String },
    Message { flow: String, message: String },
    View { view: String },
    Flow { flow: String },
    File { path: String },
    Table { table: String },
    Column { table: String, column: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepCite {
    pub text: String,
    #[serde(rename = "ref")]
    pub reference: CiteRef,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepDetail {
    pub text: String,
    pub cites: Vec<StepCite>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalkthroughStep {
    pub id: String,
    pub heading: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<StepStage>,
    #[serde(default)]
    pub focus: StepFocus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<StepDetail>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Walkthrough {
    pub steps: Vec<WalkthroughStep>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColumnKey {
    Primary,
    Foreign,
    Index,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphTableColumn {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<ColumnKey>,
    pub delta: GraphDelta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphTableIndex {
    pub name: String,
    pub columns: Vec<String>,
    #[serde(default)]
    pub unique: bool,
    pub delta: GraphDelta,
    #[serde(default)]
    pub files: Vec<GraphFileRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphTable {
    pub id: String,
    pub label: String,
    pub store: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub delta: GraphDelta,
    #[serde(default)]
    pub columns: Vec<GraphTableColumn>,
    #[serde(default)]
    pub indexes: Vec<GraphTableIndex>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default)]
    pub files: Vec<GraphFileRef>,
    #[serde(default)]
    pub badges: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphStore {
    pub id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<GraphDelta>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GraphRelationEnd {
    pub table: String,
    pub column: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRelation {
    pub id: String,
    pub from: GraphRelationEnd,
    pub to: GraphRelationEnd,
    pub delta: GraphDelta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default = "yes")]
    pub enforced: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphDataModel {
    pub stores: Vec<GraphStore>,
    pub tables: Vec<GraphTable>,
    #[serde(default)]
    pub relations: Vec<GraphRelation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChipTone {
    #[default]
    Neutral,
    Added,
    Modified,
    Removed,
    Hero,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatChip {
    pub label: String,
    pub value: String,
    #[serde(default)]
    pub tone: ChipTone,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphStats {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_changed: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<u32>,
    #[serde(default)]
    pub chips: Vec<StatChip>,
}

fn github() -> String {
    "github.com".to_owned()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceRepo {
    pub owner: String,
    pub name: String,
    #[serde(default = "github")]
    pub host: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvenanceRev {
    pub sha: String,
    #[serde(default, rename = "ref", skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub repo: ProvenanceRepo,
    pub base: ProvenanceRev,
    pub head: ProvenanceRev,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_request: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generator: Option<Value>,
}

/// The PR Lens GraphDoc after zod's defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewGraph {
    pub schema_version: String,
    /// Always `graph`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub lenses: Vec<GraphLens>,
    pub provenance: Provenance,
    pub lanes: Vec<GraphLane>,
    pub nodes: Vec<GraphNode>,
    #[serde(default)]
    pub edges: Vec<GraphEdge>,
    #[serde(default)]
    pub flows: Vec<GraphFlow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_model: Option<GraphDataModel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<GraphStats>,
    #[serde(default)]
    pub views: Vec<GraphView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walkthrough: Option<Walkthrough>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Value>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// One picture the overview can show: `view` and `flow` are exclusive; neither = the whole lens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderTarget {
    pub lens: GraphLens,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<String>,
    pub title: String,
    pub depth: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverviewCoverage {
    pub repo: Option<String>,
    pub review: Vec<String>,
    pub missing_review: Vec<String>,
    pub ignored: Vec<String>,
    pub generated: Vec<String>,
    pub uncovered: Vec<String>,
    pub formatting_only: Vec<String>,
}

/// difit's `Box` (renamed to stay clear of `std::boxed::Box`).
#[derive(Clone, Copy, Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct AtlasBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderAtlas {
    pub lanes: BTreeMap<String, AtlasBox>,
    pub nodes: BTreeMap<String, AtlasBox>,
    pub edges: BTreeMap<String, AtlasBox>,
    pub messages: BTreeMap<String, BTreeMap<String, AtlasBox>>,
    pub tables: BTreeMap<String, AtlasBox>,
    pub columns: BTreeMap<String, BTreeMap<String, AtlasBox>>,
    pub indexes: BTreeMap<String, BTreeMap<String, AtlasBox>>,
}

/// A validation finding; `path` is `graph.nodes[3].files[0].path` style.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub code: String,
    pub path: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const SAMPLES: [(&str, &str); 3] = [
        (
            "sample-overview.json",
            include_str!("../../../fixtures/overview/sample-overview.json"),
        ),
        (
            "sample-overview-r1.json",
            include_str!("../../../fixtures/overview/sample-overview-r1.json"),
        ),
        (
            "sample-overview-schema.json",
            include_str!("../../../fixtures/overview/sample-overview-schema.json"),
        ),
    ];

    /// Keys zod adds when it applies defaults; they may appear on re-serialisation.
    const DEFAULTED: &[&str] = &[
        "files",
        "badges",
        "edges",
        "flows",
        "views",
        "emphasis",
        "animated",
        "scope",
        "defaultOpen",
        "children",
        "columns",
        "indexes",
        "relations",
        "focus",
        "kind",
        "lanes",
        "nodes",
        "tables",
        "messages",
        "host",
        "enforced",
        "unique",
        "review",
        "ignore",
        "nodeIds",
        "readingOrder",
        "phases",
        "notes",
        "delta",
        "chips",
        "tone",
    ];

    fn keys(v: &Value, path: &str, out: &mut BTreeSet<String>) {
        match v {
            Value::Object(m) => {
                for (k, v) in m {
                    let p = format!("{path}.{k}");
                    out.insert(p.clone());
                    keys(v, &p, out);
                }
            }
            Value::Array(a) => a.iter().enumerate().for_each(|(i, v)| keys(v, &format!("{path}[{i}]"), out)),
            _ => {}
        }
    }

    #[test]
    fn samples_parse_and_keep_their_key_set() {
        for (name, text) in SAMPLES {
            let original: Value = serde_json::from_str(text).unwrap();
            let doc: OverviewDocument = serde_json::from_str(text).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(doc.format, OVERVIEW_FORMAT);
            let again = serde_json::to_value(&doc).unwrap();
            let (mut a, mut b) = (BTreeSet::new(), BTreeSet::new());
            keys(&original, "", &mut a);
            keys(&again, "", &mut b);
            let lost: Vec<_> = a.difference(&b).collect();
            assert!(lost.is_empty(), "{name} lost keys: {lost:?}");
            let added: Vec<_> = b
                .difference(&a)
                .filter(|k| !DEFAULTED.iter().any(|d| k.rsplit('.').next() == Some(*d)))
                .collect();
            assert!(added.is_empty(), "{name} gained non-default keys: {added:?}");
            for (k, v) in a.iter().filter_map(|k| pointer(&original, k).map(|v| (k, v))) {
                if !v.is_object() && !v.is_array() {
                    assert_eq!(pointer(&again, k), Some(v), "{name}: value of {k} changed");
                }
            }
            let twice: OverviewDocument = serde_json::from_value(again.clone()).unwrap();
            assert_eq!(serde_json::to_value(&twice).unwrap(), again, "{name}: not idempotent");
        }
    }

    fn pointer<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
        let mut cur = v;
        for part in path.split('.').skip(1) {
            let (name, idx) = match part.find('[') {
                Some(i) => (&part[..i], Some(&part[i..])),
                None => (part, None),
            };
            cur = cur.get(name)?;
            if let Some(idx) = idx {
                for n in idx.split(['[', ']']).filter(|s| !s.is_empty()) {
                    cur = cur.get(n.parse::<usize>().ok()?)?;
                }
            }
        }
        Some(cur)
    }

    #[test]
    fn defaults_follow_zod() {
        let doc: OverviewDocument = serde_json::from_str(SAMPLES[2].1).unwrap();
        let g = &doc.graph;
        assert_eq!(g.provenance.repo.host, "github.com");
        assert_eq!(g.edges[0].emphasis, EdgeEmphasis::Normal);
        assert!(!g.edges[0].animated);
        assert!(g.flows[0].messages[0].animated);
        assert!(g.data_model.as_ref().unwrap().relations[0].enforced);
        assert_eq!(g.views[0].scope, GraphViewScope::All);
        assert!(g.views[0].default_open);
        assert!(matches!(&g.views[1].scope, GraphViewScope::Selection { nodes, lanes, .. } if nodes.len() == 4 && lanes.is_empty()));
        assert_eq!(doc.reading_order[0].review[0].risk, Some(RiskLevel::Critical));
        assert!(
            matches!(&g.walkthrough.as_ref().unwrap().steps[3].detail.as_ref().unwrap().cites[0].reference,
            CiteRef::Column { table, column } if table == "vouchers" && column == "AccountId")
        );
    }

    #[test]
    fn unknown_fields_survive() {
        let mut v: Value = serde_json::from_str(SAMPLES[2].1).unwrap();
        v["future"] = Value::from(1);
        v["graph"]["layout"] = serde_json::json!({ "direction": "down", "rank": { "a": 2 } });
        let doc: OverviewDocument = serde_json::from_value(v).unwrap();
        let back = serde_json::to_value(&doc).unwrap();
        assert_eq!(back["future"], 1);
        assert_eq!(back["graph"]["layout"]["rank"]["a"], 2);
    }
}
