//! Overview validation: a port of difit `server/overview/{schema,validate}.ts` plus the PR Lens
//! schema bounds (`schema/{primitives,graph}.ts`) and integrity rules (`schema/integrity.ts`).
//! Codes and paths match what difit reports so the agent side reads the same issues.

use std::collections::{BTreeMap, BTreeSet};

use rain_core::overview::{
    CiteRef, GraphFileRef, GraphFlow, GraphView, GraphViewScope, Issue, OVERVIEW_FORMAT, OverviewDocument, OverviewGraph, OverviewNote,
    OverviewPhase, OverviewRepo, RenderTarget, RepoReadingOrder, RiskLevel, StepFocus, StepStage,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::paths::{collect_file_refs, split_repo_path};
use crate::targets::{find_view, lens_name, scope_for, targets};

/// A document that passed every check, with what the renderer can draw and non-fatal findings.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub document: OverviewDocument,
    pub targets: Vec<RenderTarget>,
    pub warnings: Vec<String>,
}

const INVALID_OVERVIEW: &str = "INVALID_OVERVIEW";
const INVALID_DOCUMENT: &str = "INVALID_DOCUMENT";
const SUPPORTED_MAJOR: u64 = 0;
const SUPPORTED_MINORS: std::ops::RangeInclusive<u64> = 1..=3;
const MAX_VIEWS: usize = 128;
const NOTE_BUDGET: usize = 8;
/// `graph.nodes[].kind` values; `docs/agent.md` lists them.
pub const NODE_KINDS: &[&str] = &[
    "service",
    "app",
    "module",
    "function",
    "route",
    "job",
    "queue",
    "datastore",
    "cache",
    "external",
    "ui",
    "config",
    "test",
    "package",
    "other",
];
/// `graph.edges[].kind` values; `docs/agent.md` lists them.
pub const EDGE_KINDS: &[&str] = &["call", "http", "rpc", "event", "queue", "data", "dependency", "render", "other"];
const DOMAIN_CRITICAL: &[&str] = &[
    "payment",
    "charge",
    "invoice",
    "stripe",
    "account",
    "auth",
    "permission",
    "role",
    "delete",
    "sync",
    "dispatch",
    "schedul",
];

/// A path segment (folder or file name) whose first word starts with a [`DOMAIN_CRITICAL`] stem:
/// `Payments/`, `AccountService.cs`, `delete-user.ts`; not `shelf-account/` or `useAsync.ts`.
pub fn looks_domain_critical(path: &str) -> bool {
    path.split('/').any(|segment| {
        let mut first = String::new();
        let mut prev_lower = false;
        for c in segment.chars() {
            if !c.is_ascii_alphanumeric() || (prev_lower && c.is_ascii_uppercase()) {
                break;
            }
            prev_lower = c.is_ascii_lowercase();
            first.push(c.to_ascii_lowercase());
        }
        DOMAIN_CRITICAL.iter().any(|w| first.starts_with(w))
    })
}

fn issue(code: &str, path: impl Into<String>, message: impl Into<String>) -> Issue {
    Issue {
        code: code.to_owned(),
        path: path.into(),
        message: message.into(),
    }
}

// ---------- primitives (primitives.ts / schema.ts regexes, by hand) ----------

pub fn is_id(s: &str) -> bool {
    let n = s.chars().count();
    (1..=128).contains(&n)
        && s.starts_with(|c: char| c.is_ascii_alphanumeric())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "._:/-".contains(c))
}

pub fn is_repo_id(s: &str) -> bool {
    (1..=32).contains(&s.len())
        && s.starts_with(|c: char| c.is_ascii_lowercase())
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Repository-relative POSIX path: no leading `/`, drive letter, backslash or `..` segment.
pub fn is_repository_path(s: &str) -> bool {
    let drive = matches!(s.as_bytes(), [d, b':', ..] if d.is_ascii_alphabetic());
    let refused = drive || s.starts_with('/') || s.contains(['\\', '\n', '\r']) || s.split('/').any(|seg| seg == "..");
    (1..=1024).contains(&s.chars().count()) && !refused
}

fn is_sha(s: &str) -> bool {
    (7..=40).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn is_type_name(s: &str) -> bool {
    let n = s.chars().count();
    (1..=64).contains(&n)
        && !s.contains("--")
        && s.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "_ .,?!<>()[]|-".contains(c))
}

fn semver(s: &str) -> Option<(u64, u64, u64)> {
    let mut parts = s.split('.');
    let mut next = || {
        parts
            .next()
            .filter(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()))
            .and_then(|p| p.parse().ok())
    };
    let v = (next()?, next()?, next()?);
    parts.next().is_none().then_some(v)
}

/// zod's `z.iso.datetime()` default: `YYYY-MM-DDTHH:MM:SS[.fff]Z`.
fn is_iso_datetime(s: &str) -> bool {
    let b = s.as_bytes();
    let digits = |r: std::ops::Range<usize>| b.get(r).is_some_and(|d| d.iter().all(u8::is_ascii_digit));
    let fixed = b.len() >= 20
        && digits(0..4)
        && b[4] == b'-'
        && digits(5..7)
        && b[7] == b'-'
        && digits(8..10)
        && b[10] == b'T'
        && digits(11..13)
        && b[13] == b':'
        && digits(14..16)
        && b[16] == b':'
        && digits(17..19);
    fixed
        && match &b[19..] {
            [b'Z'] => true,
            [b'.', rest @ .., b'Z'] => !rest.is_empty() && rest.iter().all(u8::is_ascii_digit),
            _ => false,
        }
}

/// Collects structural findings under one code (`INVALID_OVERVIEW` or `INVALID_DOCUMENT`).
struct Shape<'a> {
    code: &'static str,
    out: &'a mut Vec<Issue>,
}

impl Shape<'_> {
    fn fail(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.out.push(issue(self.code, path, message));
    }
    fn text(&mut self, path: impl Into<String>, s: &str, min: usize, max: usize) {
        let n = s.chars().count();
        if n < min || n > max {
            self.fail(path, format!("must be {min} to {max} characters, got {n}"));
        }
    }
    fn opt_text(&mut self, path: impl Into<String>, s: Option<&String>, min: usize, max: usize) {
        if let Some(s) = s {
            self.text(path, s, min, max);
        }
    }
    fn label(&mut self, path: impl Into<String>, s: &str) {
        self.text(path, s, 1, 120);
    }
    fn summary(&mut self, path: impl Into<String>, s: Option<&String>) {
        self.opt_text(path, s, 1, 2000);
    }
    fn id(&mut self, path: impl Into<String>, s: &str) {
        if !is_id(s) {
            self.fail(
                path,
                format!("'{s}' must start alphanumeric and contain only letters, digits and . _ : / -"),
            );
        }
    }
    fn ids(&mut self, path: &str, ids: &[String], max: usize) {
        self.count(path, ids.len(), 0, max);
        for (i, id) in ids.iter().enumerate() {
            self.id(format!("{path}[{i}]"), id);
        }
    }
    fn repo_path(&mut self, path: impl Into<String>, s: &str) {
        if !is_repository_path(s) {
            self.fail(
                path,
                "must be a repository-relative POSIX path, without a drive letter, a backslash or a '..' segment",
            );
        }
    }
    fn count(&mut self, path: impl Into<String>, n: usize, min: usize, max: usize) {
        if n < min || n > max {
            self.fail(path, format!("must hold {min} to {max} items, got {n}"));
        }
    }
    fn range(&mut self, path: impl Into<String>, v: u32, min: u32, max: u32) {
        if v < min || v > max {
            self.fail(path, format!("must be between {min} and {max}, got {v}"));
        }
    }
    fn file_refs(&mut self, path: &str, files: &[GraphFileRef], max: usize) {
        self.count(path, files.len(), 0, max);
        for (i, f) in files.iter().enumerate() {
            self.file_ref(&format!("{path}[{i}]"), f);
        }
    }
    fn file_ref(&mut self, at: &str, f: &GraphFileRef) {
        self.repo_path(format!("{at}.path"), &f.path);
        if f.start_line == Some(0) {
            self.fail(format!("{at}.startLine"), "must be at least 1");
        }
        if f.end_line == Some(0) {
            self.fail(format!("{at}.endLine"), "must be at least 1");
        }
        match (f.start_line, f.end_line) {
            (None, Some(_)) => self.fail(format!("{at}.endLine"), "endLine requires startLine"),
            (Some(s), Some(e)) if e < s => self.fail(format!("{at}.endLine"), "endLine must be greater than or equal to startLine"),
            _ => {}
        }
    }
}

// ---------- serde shapes (no `flatten`, so serde_path_to_error can name the failing field) ----------

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Wrapper {
    format: String,
    title: String,
    repos: Vec<OverviewRepo>,
    graph: Value,
    #[serde(default)]
    reading_order: Vec<RepoReadingOrder>,
    #[serde(default)]
    phases: Vec<OverviewPhase>,
    #[serde(default)]
    notes: Vec<OverviewNote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    diagrams: Option<Value>,
}

/// Deserialized only to type-check the graph against PR Lens's schema; the fields are never read.
#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphShape {
    schema_version: String,
    kind: String,
    id: Option<String>,
    generated_at: Option<String>,
    title: String,
    summary: Option<String>,
    lenses: Vec<rain_core::overview::GraphLens>,
    provenance: rain_core::overview::Provenance,
    lanes: Vec<rain_core::overview::GraphLane>,
    nodes: Vec<rain_core::overview::GraphNode>,
    #[serde(default)]
    edges: Vec<rain_core::overview::GraphEdge>,
    #[serde(default)]
    flows: Vec<GraphFlow>,
    data_model: Option<rain_core::overview::GraphDataModel>,
    stats: Option<rain_core::overview::GraphStats>,
    #[serde(default)]
    views: Vec<GraphView>,
    walkthrough: Option<rain_core::overview::Walkthrough>,
    layout: Option<Value>,
}

fn serde_issue(code: &str, prefix: &str, err: serde_path_to_error::Error<serde_json::Error>) -> Issue {
    use serde_path_to_error::Segment;
    let mut path = prefix.to_owned();
    for seg in err.path().iter() {
        match seg {
            Segment::Seq { index } => path.push_str(&format!("[{index}]")),
            Segment::Map { key } | Segment::Enum { variant: key } => {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(key);
            }
            Segment::Unknown => {}
        }
    }
    let message = err.inner().to_string();
    if let Some(field) = message.strip_prefix("missing field `").and_then(|m| m.split('`').next()) {
        if !path.is_empty() {
            path.push('.');
        }
        path.push_str(field);
    }
    issue(code, path, message)
}

/// Keys of `original` that the typed read dropped: zod's strict objects refuse them.
fn unknown_keys(original: &Value, typed: &Value, path: &str, code: &str, skip: &[&str], out: &mut Vec<Issue>) {
    match (original, typed) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in a {
                if skip.contains(&k.as_str()) {
                    continue;
                }
                let at = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                match b.get(k) {
                    Some(w) => unknown_keys(v, w, &at, code, &[], out),
                    None if v.is_null() => out.push(issue(code, at, "null is not allowed; omit the key")),
                    None => out.push(issue(code, path, format!("Unrecognized key: \"{k}\""))),
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            for (i, (v, w)) in a.iter().zip(b).enumerate() {
                unknown_keys(v, w, &format!("{path}[{i}]"), code, &[], out);
            }
        }
        _ => {}
    }
}

/// Reads and validates an overview file.
pub fn load(path: impl AsRef<std::path::Path>) -> Result<Loaded, Vec<Issue>> {
    let text = std::fs::read_to_string(path.as_ref())
        .map_err(|e| vec![issue(INVALID_OVERVIEW, "", format!("cannot read {}: {e}", path.as_ref().display()))])?;
    let value: Value = serde_json::from_str(&text).map_err(|e| vec![issue(INVALID_OVERVIEW, "", format!("not JSON: {e}"))])?;
    load_value(value)
}

/// Validates a parsed overview: every issue difit's `validateOverview` reports, same codes and paths.
pub fn load_value(input: Value) -> Result<Loaded, Vec<Issue>> {
    let Some(obj) = input.as_object() else {
        return Err(vec![issue(INVALID_OVERVIEW, "", "expected an object")]);
    };
    let missing: Vec<Issue> = ["format", "title", "repos", "graph"]
        .into_iter()
        .filter(|k| !obj.contains_key(*k))
        .map(|k| issue(INVALID_OVERVIEW, k, "required"))
        .collect();
    if !missing.is_empty() {
        return Err(missing);
    }
    let wrapper: Wrapper = serde_path_to_error::deserialize(&input).map_err(|e| vec![serde_issue(INVALID_OVERVIEW, "", e)])?;

    let mut issues = Vec::new();
    let typed = serde_json::to_value(&wrapper).unwrap_or_default();
    unknown_keys(&input, &typed, "", INVALID_OVERVIEW, &["graph", "diagrams"], &mut issues);
    wrapper_shape(&wrapper, &mut issues);
    if !issues.is_empty() {
        return Err(issues);
    }

    let repo_ids = wrapper_rules(&wrapper, &mut issues);
    if let Some(d) = &wrapper.diagrams {
        issues.extend(crate::diagrams::check(d));
    }

    let graph = match parse_graph(&wrapper.graph) {
        Ok(g) => g,
        Err(mut graph_issues) => {
            issues.append(&mut graph_issues);
            return Err(issues);
        }
    };

    for r in collect_file_refs(&graph) {
        if !split_repo_path(&r.path).is_some_and(|(repo, rel)| !rel.is_empty() && repo_ids.contains(repo)) {
            issues.push(issue(
                "UNKNOWN_REPO_PREFIX",
                format!("graph.{}", r.json_path),
                format!("'{}' must start with a repo id from repos followed by '/'", r.path),
            ));
        }
    }
    let node_ids: BTreeSet<&str> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
    for (i, phase) in wrapper.phases.iter().enumerate() {
        for (j, id) in phase.node_ids.iter().enumerate() {
            if !node_ids.contains(id.as_str()) {
                issues.push(issue(
                    "BROKEN_REFERENCE",
                    format!("phases[{i}].nodeIds[{j}]"),
                    format!("phase '{}' names node '{id}', which the graph does not have", phase.id),
                ));
            }
        }
    }
    let targets = targets(&graph);
    if targets.is_empty() {
        issues.push(issue(
            "NO_RENDER_TARGETS",
            "graph.lenses",
            "no declared lens has anything to draw: add views, flows, a dataModel, or the architecture lens",
        ));
    }
    for t in &targets {
        if let Err(e) = scope_for(&graph, t) {
            issues.push(e);
        }
    }
    if !issues.is_empty() {
        return Err(issues);
    }

    let mut warnings = warnings(&wrapper, &graph);
    if let Some(d) = &wrapper.diagrams {
        warnings.extend(crate::diagrams::budget_warnings(d));
    }
    let Wrapper {
        format,
        title,
        repos,
        reading_order,
        phases,
        notes,
        diagrams,
        ..
    } = wrapper;
    Ok(Loaded {
        document: OverviewDocument {
            format,
            title,
            repos,
            graph,
            reading_order,
            phases,
            notes,
            diagrams: diagrams.and_then(|d| serde_json::from_value(d).ok()),
            extra: BTreeMap::new(),
        },
        targets,
        warnings,
    })
}

fn wrapper_shape(w: &Wrapper, out: &mut Vec<Issue>) {
    let mut s = Shape {
        code: INVALID_OVERVIEW,
        out,
    };
    if w.format != OVERVIEW_FORMAT {
        s.fail("format", format!("expected \"{OVERVIEW_FORMAT}\""));
    }
    s.text("title", &w.title, 1, 200);
    s.count("repos", w.repos.len(), 1, 8);
    for (i, r) in w.repos.iter().enumerate() {
        if !is_repo_id(&r.id) {
            s.fail(format!("repos[{i}].id"), "must match ^[a-z][a-z0-9-]{0,31}$");
        }
        s.text(format!("repos[{i}].dir"), &r.dir, 1, 128);
        if !is_sha(&r.base) {
            s.fail(format!("repos[{i}].base"), "must be a lowercase hex sha");
        }
        if !(is_sha(&r.head) || r.head == "WORKTREE") {
            s.fail(format!("repos[{i}].head"), "must be a lowercase hex sha or WORKTREE");
        }
    }
    s.count("readingOrder", w.reading_order.len(), 0, 8);
    for (i, o) in w.reading_order.iter().enumerate() {
        if !is_repo_id(&o.repo) {
            s.fail(format!("readingOrder[{i}].repo"), "must match ^[a-z][a-z0-9-]{0,31}$");
        }
        s.count(format!("readingOrder[{i}].review"), o.review.len(), 0, 256);
        for (j, e) in o.review.iter().enumerate() {
            s.repo_path(format!("readingOrder[{i}].review[{j}].path"), &e.path);
            s.text(format!("readingOrder[{i}].review[{j}].why"), &e.why, 1, 200);
        }
        s.count(format!("readingOrder[{i}].ignore"), o.ignore.len(), 0, 64);
        for (j, e) in o.ignore.iter().enumerate() {
            s.text(format!("readingOrder[{i}].ignore[{j}].glob"), &e.glob, 1, 256);
            s.text(format!("readingOrder[{i}].ignore[{j}].reason"), &e.reason, 1, 200);
        }
    }
    s.count("phases", w.phases.len(), 0, 32);
    for (i, p) in w.phases.iter().enumerate() {
        s.id(format!("phases[{i}].id"), &p.id);
        s.text(format!("phases[{i}].title"), &p.title, 1, 120);
        s.text(format!("phases[{i}].today"), &p.today, 1, 2000);
        s.text(format!("phases[{i}].proposed"), &p.proposed, 1, 2000);
        s.ids(&format!("phases[{i}].nodeIds"), &p.node_ids, 64);
    }
    s.count("notes", w.notes.len(), 0, 128);
    for (i, n) in w.notes.iter().enumerate() {
        s.id(format!("notes[{i}].id"), &n.id);
        s.repo_path(format!("notes[{i}].path"), &n.path);
        if n.start_line == 0 {
            s.fail(format!("notes[{i}].startLine"), "must be at least 1");
        }
        if n.end_line == Some(0) {
            s.fail(format!("notes[{i}].endLine"), "must be at least 1");
        }
        s.text(format!("notes[{i}].text"), &n.text, 1, 600);
    }
}

fn wrapper_rules<'a>(w: &'a Wrapper, out: &mut Vec<Issue>) -> BTreeSet<&'a str> {
    let mut repo_ids = BTreeSet::new();
    for (i, r) in w.repos.iter().enumerate() {
        if !repo_ids.insert(r.id.as_str()) {
            out.push(issue(
                "DUPLICATE_ID",
                format!("repos[{i}].id"),
                format!("repo id '{}' is used more than once", r.id),
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for (i, o) in w.reading_order.iter().enumerate() {
        if !repo_ids.contains(o.repo.as_str()) {
            out.push(issue(
                "UNKNOWN_REPO",
                format!("readingOrder[{i}].repo"),
                format!("reading order names repo '{}', which is not in repos", o.repo),
            ));
        }
        if !seen.insert(o.repo.as_str()) {
            out.push(issue(
                "DUPLICATE_ID",
                format!("readingOrder[{i}].repo"),
                format!("repo '{}' has more than one reading order entry", o.repo),
            ));
        }
        for (j, e) in o.review.iter().enumerate() {
            if let Some((repo, _)) = split_repo_path(&e.path).filter(|(r, _)| repo_ids.contains(r)) {
                out.push(issue(
                    INVALID_OVERVIEW,
                    format!("readingOrder[{i}].review[{j}].path"),
                    format!("reading order paths are repo-relative; drop the '{repo}/' prefix"),
                ));
            }
        }
    }
    let mut seen = BTreeSet::new();
    for (i, p) in w.phases.iter().enumerate() {
        if !seen.insert(p.id.as_str()) {
            out.push(issue(
                "DUPLICATE_ID",
                format!("phases[{i}].id"),
                format!("phase id '{}' is used more than once", p.id),
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for (i, n) in w.notes.iter().enumerate() {
        if !seen.insert(n.id.as_str()) {
            out.push(issue(
                "DUPLICATE_ID",
                format!("notes[{i}].id"),
                format!("note id '{}' is used more than once", n.id),
            ));
        }
        if !split_repo_path(&n.path).is_some_and(|(repo, rel)| !rel.is_empty() && repo_ids.contains(repo)) {
            out.push(issue(
                "UNKNOWN_REPO_PREFIX",
                format!("notes[{i}].path"),
                format!("'{}' must start with a repo id from repos followed by '/'", n.path),
            ));
        }
        if let Some(end) = n.end_line.filter(|&e| e < n.start_line) {
            out.push(issue(
                INVALID_OVERVIEW,
                format!("notes[{i}].endLine"),
                format!("endLine {end} is before startLine {}", n.start_line),
            ));
        }
    }
    repo_ids
}

/// PR Lens `safeParseGraphDoc`: structure first (stop there on failure), then version and integrity.
fn parse_graph(value: &Value) -> Result<OverviewGraph, Vec<Issue>> {
    if !value.is_object() {
        return Err(vec![issue(INVALID_DOCUMENT, "graph", "expected an object")]);
    }
    serde_path_to_error::deserialize::<_, GraphShape>(value).map_err(|e| vec![serde_issue(INVALID_DOCUMENT, "graph", e)])?;
    let graph: OverviewGraph = serde_json::from_value(value.clone()).map_err(|e| vec![issue(INVALID_DOCUMENT, "graph", e.to_string())])?;

    let mut issues = Vec::new();
    let mut bare = graph.clone();
    bare.extra.clear();
    let typed = serde_json::to_value(&bare).unwrap_or_default();
    unknown_keys(value, &typed, "graph", INVALID_DOCUMENT, &[], &mut issues);
    graph_shape(&graph, &mut issues);
    if !issues.is_empty() {
        return Err(issues);
    }
    if !semver(&graph.schema_version).is_some_and(|(maj, min, _)| maj == SUPPORTED_MAJOR && SUPPORTED_MINORS.contains(&min)) {
        issues.push(issue(
            "UNSUPPORTED_SCHEMA_VERSION",
            "graph.schemaVersion",
            format!(
                "document targets schema version {}; this package implements 0.3.0",
                graph.schema_version
            ),
        ));
    }
    integrity(&graph, &mut issues);
    if issues.is_empty() { Ok(graph) } else { Err(issues) }
}

fn graph_shape(g: &OverviewGraph, out: &mut Vec<Issue>) {
    let mut s = Shape {
        code: INVALID_DOCUMENT,
        out,
    };
    if semver(&g.schema_version).is_none() {
        s.fail("graph.schemaVersion", "must be a semver string, e.g. 0.1.0");
    }
    if g.kind != "graph" {
        s.fail("graph.kind", "expected \"graph\"");
    }
    if let Some(id) = &g.id {
        s.id("graph.id", id);
    }
    if g.generated_at.as_deref().is_some_and(|d| !is_iso_datetime(d)) {
        s.fail("graph.generatedAt", "must be an ISO datetime");
    }
    s.label("graph.title", &g.title);
    s.summary("graph.summary", g.summary.as_ref());
    s.count("graph.lenses", g.lenses.len(), 1, 8);

    let p = &g.provenance;
    s.text("graph.provenance.repo.owner", &p.repo.owner, 1, 64);
    s.text("graph.provenance.repo.name", &p.repo.name, 1, 128);
    s.text("graph.provenance.repo.host", &p.repo.host, 1, 128);
    for (side, rev) in [("base", &p.base), ("head", &p.head)] {
        if !is_sha(&rev.sha) {
            s.fail(format!("graph.provenance.{side}.sha"), "must be a lowercase hex git object name");
        }
        s.opt_text(format!("graph.provenance.{side}.ref"), rev.reference.as_ref(), 1, 255);
    }
    if let Some(pr) = &p.pull_request {
        if pr.get("number").and_then(Value::as_u64).is_none_or(|n| n < 1) {
            s.fail("graph.provenance.pullRequest.number", "must be an integer of at least 1");
        }
        if pr
            .get("title")
            .is_some_and(|t| !t.as_str().is_some_and(|t| (1..=512).contains(&t.chars().count())))
        {
            s.fail("graph.provenance.pullRequest.title", "must be 1 to 512 characters");
        }
        if pr.get("url").is_some_and(|u| !u.as_str().is_some_and(|u| u.contains("://"))) {
            s.fail("graph.provenance.pullRequest.url", "must be a URL");
        }
        strict_value(&mut s, pr, "graph.provenance.pullRequest", &["number", "title", "url"]);
    }
    if let Some(gen_) = &p.generator {
        for (k, max, required) in [("name", 64, true), ("version", 32, false), ("model", 128, false)] {
            match gen_.get(k).map(Value::as_str) {
                None if !required => {}
                Some(Some(v)) if (1..=max).contains(&v.chars().count()) => {}
                _ => s.fail(format!("graph.provenance.generator.{k}"), format!("must be 1 to {max} characters")),
            }
        }
        strict_value(&mut s, gen_, "graph.provenance.generator", &["name", "version", "model"]);
    }

    s.count("graph.lanes", g.lanes.len(), 1, 16);
    for (i, l) in g.lanes.iter().enumerate() {
        let at = format!("graph.lanes[{i}]");
        s.id(format!("{at}.id"), &l.id);
        s.label(format!("{at}.label"), &l.label);
        if let Some(sub) = &l.subtitle {
            s.label(format!("{at}.subtitle"), sub);
        }
        if let Some(o) = l.order {
            s.range(format!("{at}.order"), o, 0, 64);
        }
        s.summary(format!("{at}.summary"), l.summary.as_ref());
    }
    s.count("graph.nodes", g.nodes.len(), 1, 256);
    for (i, n) in g.nodes.iter().enumerate() {
        let at = format!("graph.nodes[{i}]");
        s.id(format!("{at}.id"), &n.id);
        s.label(format!("{at}.label"), &n.label);
        if !NODE_KINDS.contains(&n.kind.as_str()) {
            s.fail(
                format!("{at}.kind"),
                format!("'{}' is not one of {}", n.kind, NODE_KINDS.join(", ")),
            );
        }
        s.id(format!("{at}.lane"), &n.lane);
        if let Some(gr) = &n.group {
            s.id(format!("{at}.group"), gr);
        }
        if let Some(sub) = &n.subtitle {
            s.label(format!("{at}.subtitle"), sub);
        }
        s.summary(format!("{at}.summary"), n.summary.as_ref());
        s.file_refs(&format!("{at}.files"), &n.files, 64);
        s.count(format!("{at}.badges"), n.badges.len(), 0, 6);
        for (j, b) in n.badges.iter().enumerate() {
            s.label(format!("{at}.badges[{j}]"), b);
        }
    }
    s.count("graph.edges", g.edges.len(), 0, 512);
    for (i, e) in g.edges.iter().enumerate() {
        let at = format!("graph.edges[{i}]");
        s.id(format!("{at}.id"), &e.id);
        s.id(format!("{at}.from"), &e.from);
        s.id(format!("{at}.to"), &e.to);
        if !EDGE_KINDS.contains(&e.kind.as_str()) {
            s.fail(
                format!("{at}.kind"),
                format!("'{}' is not one of {}", e.kind, EDGE_KINDS.join(", ")),
            );
        }
        if let Some(l) = &e.label {
            s.label(format!("{at}.label"), l);
        }
        s.summary(format!("{at}.summary"), e.summary.as_ref());
        s.file_refs(&format!("{at}.files"), &e.files, 32);
    }
    s.count("graph.flows", g.flows.len(), 0, 16);
    for (i, f) in g.flows.iter().enumerate() {
        let at = format!("graph.flows[{i}]");
        s.id(format!("{at}.id"), &f.id);
        s.label(format!("{at}.title"), &f.title);
        s.summary(format!("{at}.summary"), f.summary.as_ref());
        s.count(format!("{at}.participants"), f.participants.len(), 2, 12);
        for (j, p) in f.participants.iter().enumerate() {
            s.id(format!("{at}.participants[{j}].node"), &p.node);
            if let Some(l) = &p.label {
                s.label(format!("{at}.participants[{j}].label"), l);
            }
        }
        s.count(format!("{at}.messages"), f.messages.len(), 1, 64);
        for (j, m) in f.messages.iter().enumerate() {
            let mt = format!("{at}.messages[{j}]");
            s.id(format!("{mt}.id"), &m.id);
            s.id(format!("{mt}.from"), &m.from);
            s.id(format!("{mt}.to"), &m.to);
            s.label(format!("{mt}.label"), &m.label);
            if let Some(r) = m.repeat {
                s.range(format!("{mt}.repeat"), r, 1, 1_000_000);
            }
            s.summary(format!("{mt}.note"), m.note.as_ref());
            s.file_refs(&format!("{mt}.files"), &m.files, 32);
            if (m.kind == rain_core::overview::MessageKind::SelfCall) != (m.from == m.to) {
                s.fail(format!("{mt}.kind"), "kind 'self' and from === to must agree");
            }
            if let Some(payload) = &m.payload {
                payload_shape(&mut s, payload, &format!("{mt}.payload"));
            }
        }
    }
    if let Some(model) = &g.data_model {
        s.count("graph.dataModel.stores", model.stores.len(), 1, 16);
        for (i, st) in model.stores.iter().enumerate() {
            let at = format!("graph.dataModel.stores[{i}]");
            s.id(format!("{at}.id"), &st.id);
            s.label(format!("{at}.label"), &st.label);
            if let Some(p) = &st.platform {
                s.label(format!("{at}.platform"), p);
            }
            if let Some(o) = st.order {
                s.range(format!("{at}.order"), o, 0, 64);
            }
        }
        s.count("graph.dataModel.tables", model.tables.len(), 1, 64);
        for (i, t) in model.tables.iter().enumerate() {
            let at = format!("graph.dataModel.tables[{i}]");
            s.id(format!("{at}.id"), &t.id);
            s.label(format!("{at}.label"), &t.label);
            s.id(format!("{at}.store"), &t.store);
            if let Some(n) = &t.name {
                s.label(format!("{at}.name"), n);
            }
            s.count(format!("{at}.columns"), t.columns.len(), 1, 64);
            for (j, c) in t.columns.iter().enumerate() {
                s.label(format!("{at}.columns[{j}].name"), &c.name);
                if !is_type_name(&c.ty) {
                    s.fail(
                        format!("{at}.columns[{j}].type"),
                        "must be a type name, with no default, literal or value in it",
                    );
                }
                if c.previous_type.as_deref().is_some_and(|p| !is_type_name(p)) {
                    s.fail(
                        format!("{at}.columns[{j}].previousType"),
                        "must be a type name, with no default, literal or value in it",
                    );
                }
            }
            s.count(format!("{at}.indexes"), t.indexes.len(), 0, 32);
            for (j, x) in t.indexes.iter().enumerate() {
                s.label(format!("{at}.indexes[{j}].name"), &x.name);
                s.count(format!("{at}.indexes[{j}].columns"), x.columns.len(), 1, 16);
                for (k, c) in x.columns.iter().enumerate() {
                    s.label(format!("{at}.indexes[{j}].columns[{k}]"), c);
                }
                s.file_refs(&format!("{at}.indexes[{j}].files"), &x.files, 8);
            }
            s.summary(format!("{at}.summary"), t.summary.as_ref());
            s.file_refs(&format!("{at}.files"), &t.files, 64);
            s.count(format!("{at}.badges"), t.badges.len(), 0, 6);
            for (j, b) in t.badges.iter().enumerate() {
                s.label(format!("{at}.badges[{j}]"), b);
            }
        }
        s.count("graph.dataModel.relations", model.relations.len(), 0, 256);
        for (i, r) in model.relations.iter().enumerate() {
            let at = format!("graph.dataModel.relations[{i}]");
            s.id(format!("{at}.id"), &r.id);
            for (end, e) in [("from", &r.from), ("to", &r.to)] {
                s.id(format!("{at}.{end}.table"), &e.table);
                s.label(format!("{at}.{end}.column"), &e.column);
            }
            if let Some(l) = &r.label {
                s.label(format!("{at}.label"), l);
            }
        }
    }
    if let Some(stats) = &g.stats {
        s.count("graph.stats.chips", stats.chips.len(), 0, 8);
        for (i, c) in stats.chips.iter().enumerate() {
            s.label(format!("graph.stats.chips[{i}].label"), &c.label);
            s.text(format!("graph.stats.chips[{i}].value"), &c.value, 1, 32);
        }
    }
    views_shape(&mut s, &g.views, "graph.views");
    if let Some(w) = &g.walkthrough {
        s.count("graph.walkthrough.steps", w.steps.len(), 2, 12);
        for (i, st) in w.steps.iter().enumerate() {
            let at = format!("graph.walkthrough.steps[{i}]");
            s.id(format!("{at}.id"), &st.id);
            s.text(format!("{at}.heading"), &st.heading, 1, 48);
            s.text(format!("{at}.body"), &st.body, 1, 140);
            match &st.stage {
                Some(StepStage::View { view }) => s.id(format!("{at}.stage.view"), view),
                Some(StepStage::Flow { flow }) => s.id(format!("{at}.stage.flow"), flow),
                None => {}
            }
            if let StepFocus::Selection {
                lanes,
                nodes,
                edges,
                messages,
                tables,
            } = &st.focus
            {
                s.ids(&format!("{at}.focus.lanes"), lanes, 64);
                s.ids(&format!("{at}.focus.nodes"), nodes, 256);
                s.ids(&format!("{at}.focus.edges"), edges, 512);
                s.ids(&format!("{at}.focus.messages"), messages, 64);
                s.ids(&format!("{at}.focus.tables"), tables, 64);
                if lanes.len() + nodes.len() + edges.len() + messages.len() + tables.len() == 0 {
                    s.fail(format!("{at}.focus"), "a selection must name at least one element");
                }
            }
            if let Some(d) = &st.detail {
                s.text(format!("{at}.detail.text"), &d.text, 1, 240);
                s.count(format!("{at}.detail.cites"), d.cites.len(), 1, 8);
                for (j, c) in d.cites.iter().enumerate() {
                    let ct = format!("{at}.detail.cites[{j}]");
                    s.text(format!("{ct}.text"), &c.text, 1, 240);
                    cite_shape(&mut s, &c.reference, &format!("{ct}.ref"));
                }
            }
        }
    }
    if let Some(layout) = &g.layout {
        layout_shape(&mut s, layout);
    }
}

fn strict_value(s: &mut Shape, v: &Value, at: &str, known: &[&str]) {
    match v.as_object() {
        Some(o) => {
            for k in o.keys().filter(|k| !known.contains(&k.as_str())) {
                s.fail(at, format!("Unrecognized key: \"{k}\""));
            }
        }
        None => s.fail(at, "expected an object"),
    }
}

fn views_shape(s: &mut Shape, views: &[GraphView], at: &str) {
    s.count(at, views.len(), 0, 32);
    for (i, v) in views.iter().enumerate() {
        let vt = format!("{at}[{i}]");
        s.id(format!("{vt}.id"), &v.id);
        s.label(format!("{vt}.title"), &v.title);
        s.summary(format!("{vt}.summary"), v.summary.as_ref());
        if let GraphViewScope::Selection {
            lanes,
            nodes,
            edges,
            flows,
            tables,
        } = &v.scope
        {
            s.ids(&format!("{vt}.scope.lanes"), lanes, 64);
            s.ids(&format!("{vt}.scope.nodes"), nodes, 256);
            s.ids(&format!("{vt}.scope.edges"), edges, 512);
            s.ids(&format!("{vt}.scope.flows"), flows, 32);
            s.ids(&format!("{vt}.scope.tables"), tables, 64);
            if lanes.len() + nodes.len() + edges.len() + flows.len() + tables.len() == 0 {
                s.fail(format!("{vt}.scope"), "a selection must name at least one element");
            }
        }
        views_shape(s, &v.children, &format!("{vt}.children"));
    }
}

fn cite_shape(s: &mut Shape, r: &CiteRef, at: &str) {
    match r {
        CiteRef::Node { node } => s.id(format!("{at}.node"), node),
        CiteRef::Message { flow, message } => {
            s.id(format!("{at}.flow"), flow);
            s.id(format!("{at}.message"), message);
        }
        CiteRef::View { view } => s.id(format!("{at}.view"), view),
        CiteRef::Flow { flow } => s.id(format!("{at}.flow"), flow),
        CiteRef::File { path } => s.repo_path(format!("{at}.path"), path),
        CiteRef::Table { table } => s.id(format!("{at}.table"), table),
        CiteRef::Column { table, column } => {
            s.id(format!("{at}.table"), table);
            s.label(format!("{at}.column"), column);
        }
    }
}

fn json_depth(v: &Value) -> usize {
    match v {
        Value::Array(a) => 1 + a.iter().map(json_depth).max().unwrap_or(0),
        Value::Object(o) => 1 + o.values().map(json_depth).max().unwrap_or(0),
        _ => 0,
    }
}

fn payload_shape(s: &mut Shape, p: &Value, at: &str) {
    strict_value(s, p, at, &["request", "response"]);
    if p.get("request").is_none() && p.get("response").is_none() {
        s.fail(at, "a payload carries at least one side");
    }
    for side in ["request", "response"] {
        let Some(v) = p.get(side) else { continue };
        let st = format!("{at}.{side}");
        strict_value(s, v, &st, &["type", "shape", "sample", "before", "source", "changedPaths"]);
        match v.get("type").and_then(Value::as_str) {
            Some(t) => s.label(format!("{st}.type"), t),
            None => s.fail(format!("{st}.type"), "required"),
        }
        if v.get("shape")
            .is_some_and(|sh| !sh.as_str().is_some_and(|t| !t.is_empty() && t.len() <= 2048))
        {
            s.fail(format!("{st}.shape"), "must be 1 to 2048 bytes");
        }
        for k in ["sample", "before"] {
            let Some(sample) = v.get(k) else { continue };
            if sample.is_string() {
                s.fail(format!("{st}.{k}"), "must be an inline JSON value, not a string");
            }
            if sample.to_string().len() > 4096 {
                s.fail(format!("{st}.{k}"), "must be at most 4096 bytes when serialised");
            }
            if json_depth(sample) > 8 {
                s.fail(format!("{st}.{k}"), "must be at most 8 levels deep");
            }
        }
        if v.get("before").is_some() && v.get("sample").is_none() {
            s.fail(format!("{st}.before"), "before needs a sample to differ from");
        }
        if let Some(src) = v.get("source") {
            match serde_json::from_value::<GraphFileRef>(src.clone()) {
                Ok(f) => {
                    s.file_ref(&format!("{st}.source"), &f);
                    strict_value(s, src, &format!("{st}.source"), &["path", "startLine", "endLine", "revision"]);
                }
                Err(e) => s.fail(format!("{st}.source"), e.to_string()),
            }
        }
        if let Some(paths) = v.get("changedPaths") {
            match paths.as_array() {
                Some(a) if a.len() <= 64 && a.iter().all(|p| p.as_str().is_some_and(|t| (1..=200).contains(&t.len()))) => {}
                _ => s.fail(format!("{st}.changedPaths"), "must be at most 64 JSON paths of 1 to 200 characters"),
            }
        }
    }
}

fn layout_shape(s: &mut Shape, layout: &Value) {
    strict_value(s, layout, "graph.layout", &["direction", "laneOrder", "rank"]);
    if layout
        .get("direction")
        .is_some_and(|d| !matches!(d.as_str(), Some("right" | "down")))
    {
        s.fail("graph.layout.direction", "must be 'right' or 'down'");
    }
    if let Some(order) = layout.get("laneOrder") {
        match order.as_array() {
            Some(a) if a.len() <= 64 => {
                for (i, id) in a.iter().enumerate() {
                    match id.as_str() {
                        Some(id) => s.id(format!("graph.layout.laneOrder[{i}]"), id),
                        None => s.fail(format!("graph.layout.laneOrder[{i}]"), "expected a string"),
                    }
                }
            }
            _ => s.fail("graph.layout.laneOrder", "must be at most 64 lane ids"),
        }
    }
    if let Some(rank) = layout.get("rank") {
        match rank.as_object() {
            Some(o) => {
                for (k, v) in o {
                    if !is_id(k) || v.as_u64().is_none_or(|r| r > 256) {
                        s.fail(
                            format!("graph.layout.rank.{k}"),
                            "must map a node id to a layer index from 0 to 256",
                        );
                    }
                }
            }
            None => s.fail("graph.layout.rank", "expected an object"),
        }
    }
}

fn duplicates<'a>(ids: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen = BTreeSet::new();
    let mut repeated = Vec::new();
    for id in ids {
        if !seen.insert(id) && !repeated.contains(&id) {
            repeated.push(id);
        }
    }
    repeated
}

fn flatten_views<'a>(views: &'a [GraphView], prefix: &str, out: &mut Vec<(&'a GraphView, String)>) {
    for (i, v) in views.iter().enumerate() {
        let path = format!("{prefix}[{i}]");
        out.push((v, path.clone()));
        flatten_views(&v.children, &format!("{path}.children"), out);
    }
}

/// Message ids a step's stage draws, `None` for no stage, `Some(None)` for a stage that names nothing known.
fn staged_messages<'a>(stage: Option<&StepStage>, g: &'a OverviewGraph) -> Option<Option<BTreeSet<&'a str>>> {
    fn ids<'a>(flows: impl Iterator<Item = &'a GraphFlow>) -> BTreeSet<&'a str> {
        flows.flat_map(|f| f.messages.iter().map(|m| m.id.as_str())).collect()
    }
    match stage? {
        StepStage::Flow { flow } => Some(g.flows.iter().find(|f| &f.id == flow).map(|f| ids(std::iter::once(f)))),
        StepStage::View { view } => Some(find_view(&g.views, view).map(|v| match &v.scope {
            GraphViewScope::All => ids(g.flows.iter()),
            GraphViewScope::Selection { flows, .. } => ids(g.flows.iter().filter(|f| flows.contains(&f.id))),
        })),
    }
}

/// Each cite is searched for from where the previous one ended (`citeStarts`).
pub fn cite_starts(text: &str, cites: &[&str]) -> Vec<Option<usize>> {
    let mut cursor = 0;
    cites
        .iter()
        .map(|c| {
            let start = text.get(cursor..).and_then(|rest| rest.find(c)).map(|i| i + cursor)?;
            cursor = start + c.len();
            Some(start)
        })
        .collect()
}

fn cite_stands(r: &CiteRef, g: &OverviewGraph) -> bool {
    let tables = || g.data_model.iter().flat_map(|m| m.tables.iter());
    match r {
        CiteRef::Node { node } => g.nodes.iter().any(|n| &n.id == node),
        CiteRef::Message { flow, message } => g.flows.iter().any(|f| &f.id == flow && f.messages.iter().any(|m| &m.id == message)),
        CiteRef::View { view } => find_view(&g.views, view).is_some(),
        CiteRef::Flow { flow } => g.flows.iter().any(|f| &f.id == flow),
        CiteRef::File { .. } => true,
        CiteRef::Table { table } => tables().any(|t| &t.id == table),
        CiteRef::Column { table, column } => tables().any(|t| &t.id == table && t.columns.iter().any(|c| &c.name == column)),
    }
}

fn cited_place(r: &CiteRef) -> String {
    match r {
        CiteRef::Node { node } => format!("node '{node}'"),
        CiteRef::Message { flow, message } => format!("step '{message}' of flow '{flow}'"),
        CiteRef::View { view } => format!("view '{view}'"),
        CiteRef::Flow { flow } => format!("flow '{flow}'"),
        CiteRef::File { path } => format!("file '{path}'"),
        CiteRef::Table { table } => format!("table '{table}'"),
        CiteRef::Column { table, column } => format!("column '{column}' of table '{table}'"),
    }
}

/// Port of PR Lens `graphIntegrityIssues`; paths carry the `graph.` prefix difit adds.
fn integrity(g: &OverviewGraph, out: &mut Vec<Issue>) {
    let mut broken = |path: String, message: String| out.push(issue("BROKEN_REFERENCE", format!("graph.{path}"), message));
    let mut dups: Vec<(String, String)> = Vec::new();

    let lane_ids: BTreeSet<&str> = g.lanes.iter().map(|l| l.id.as_str()).collect();
    let node_ids: BTreeSet<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
    let flow_ids: BTreeSet<&str> = g.flows.iter().map(|f| f.id.as_str()).collect();
    let edge_ids: BTreeSet<&str> = g.edges.iter().map(|e| e.id.as_str()).collect();
    let table_ids: BTreeSet<&str> = g.data_model.iter().flat_map(|m| m.tables.iter().map(|t| t.id.as_str())).collect();

    for id in duplicates(g.lanes.iter().map(|l| l.id.as_str())) {
        dups.push(("lanes".into(), format!("duplicate lane id '{id}'")));
    }
    for id in duplicates(g.nodes.iter().map(|n| n.id.as_str())) {
        dups.push(("nodes".into(), format!("duplicate node id '{id}'")));
    }
    for id in duplicates(g.edges.iter().map(|e| e.id.as_str())) {
        dups.push(("edges".into(), format!("duplicate edge id '{id}'")));
    }
    for id in duplicates(g.flows.iter().map(|f| f.id.as_str())) {
        dups.push(("flows".into(), format!("duplicate flow id '{id}'")));
    }
    for (i, n) in g.nodes.iter().enumerate() {
        if !lane_ids.contains(n.lane.as_str()) {
            broken(
                format!("nodes[{i}].lane"),
                format!("node '{}' references unknown lane '{}'", n.id, n.lane),
            );
        }
    }
    for (i, e) in g.edges.iter().enumerate() {
        for (end, id) in [("from", &e.from), ("to", &e.to)] {
            if !node_ids.contains(id.as_str()) {
                broken(
                    format!("edges[{i}].{end}"),
                    format!("edge '{}' references unknown node '{id}'", e.id),
                );
            }
        }
    }
    for (fi, f) in g.flows.iter().enumerate() {
        let participants: BTreeSet<&str> = f.participants.iter().map(|p| p.node.as_str()).collect();
        for id in duplicates(f.participants.iter().map(|p| p.node.as_str())) {
            dups.push((format!("flows[{fi}].participants"), format!("participant '{id}' listed twice")));
        }
        for id in duplicates(f.messages.iter().map(|m| m.id.as_str())) {
            dups.push((format!("flows[{fi}].messages"), format!("duplicate message id '{id}'")));
        }
        for (i, p) in f.participants.iter().enumerate() {
            if !node_ids.contains(p.node.as_str()) {
                broken(
                    format!("flows[{fi}].participants[{i}].node"),
                    format!("flow '{}' references unknown node '{}'", f.id, p.node),
                );
            }
        }
        for (i, m) in f.messages.iter().enumerate() {
            if !participants.contains(m.from.as_str()) {
                broken(
                    format!("flows[{fi}].messages[{i}].from"),
                    format!("message '{}' sends from '{}', which is not a participant", m.id, m.from),
                );
            }
            if !participants.contains(m.to.as_str()) {
                broken(
                    format!("flows[{fi}].messages[{i}].to"),
                    format!("message '{}' sends to '{}', which is not a participant", m.id, m.to),
                );
            }
        }
    }
    if !g.flows.is_empty() && !g.lenses.contains(&rain_core::overview::GraphLens::DataFlow) {
        broken(
            "lenses".into(),
            "document carries flows but does not declare the 'data-flow' lens".into(),
        );
    }
    if let Some(model) = &g.data_model {
        let store_ids: BTreeSet<&str> = model.stores.iter().map(|s| s.id.as_str()).collect();
        let columns: BTreeMap<&str, BTreeSet<&str>> = model
            .tables
            .iter()
            .map(|t| (t.id.as_str(), t.columns.iter().map(|c| c.name.as_str()).collect()))
            .collect();
        for id in duplicates(model.stores.iter().map(|s| s.id.as_str())) {
            dups.push(("dataModel.stores".into(), format!("duplicate store id '{id}'")));
        }
        for id in duplicates(model.tables.iter().map(|t| t.id.as_str())) {
            dups.push(("dataModel.tables".into(), format!("duplicate table id '{id}'")));
        }
        for id in duplicates(model.relations.iter().map(|r| r.id.as_str())) {
            dups.push(("dataModel.relations".into(), format!("duplicate relation id '{id}'")));
        }
        for (i, t) in model.tables.iter().enumerate() {
            if !store_ids.contains(t.store.as_str()) {
                broken(
                    format!("dataModel.tables[{i}].store"),
                    format!("table '{}' references unknown store '{}'", t.id, t.store),
                );
            }
            for name in duplicates(t.columns.iter().map(|c| c.name.as_str())) {
                dups.push((
                    format!("dataModel.tables[{i}].columns"),
                    format!("table '{}' lists column '{name}' twice", t.id),
                ));
            }
            let mut seen = BTreeSet::new();
            for (j, x) in t.indexes.iter().enumerate() {
                if !seen.insert(x.name.as_str()) {
                    dups.push((
                        format!("dataModel.tables[{i}].indexes[{j}].name"),
                        format!("table '{}' lists index '{}' twice", t.id, x.name),
                    ));
                }
            }
        }
        for (i, r) in model.relations.iter().enumerate() {
            for (end, e) in [("from", &r.from), ("to", &r.to)] {
                match columns.get(e.table.as_str()) {
                    None => broken(
                        format!("dataModel.relations[{i}].{end}.table"),
                        format!("relation '{}' references unknown table '{}'", r.id, e.table),
                    ),
                    Some(cols) if !cols.contains(e.column.as_str()) => broken(
                        format!("dataModel.relations[{i}].{end}.column"),
                        format!(
                            "relation '{}' references column '{}', which table '{}' does not have",
                            r.id, e.column, e.table
                        ),
                    ),
                    _ => {}
                }
            }
        }
        if !g.lenses.contains(&rain_core::overview::GraphLens::DataModel) {
            broken(
                "lenses".into(),
                "document carries a data model but does not declare the 'data-model' lens".into(),
            );
        }
    }

    let mut views = Vec::new();
    flatten_views(&g.views, "views", &mut views);
    for id in duplicates(views.iter().map(|(v, _)| v.id.as_str())) {
        dups.push(("views".into(), format!("duplicate view id '{id}'")));
    }
    let mut invalid = Vec::new();
    if views.len() > MAX_VIEWS {
        invalid.push((
            "views".to_owned(),
            format!(
                "drill-down tree carries {} views; at 2 paired themes only {MAX_VIEWS} fit a render manifest",
                views.len()
            ),
        ));
    }
    for (v, path) in &views {
        if !g.lenses.contains(&v.lens) {
            broken(
                format!("{path}.lens"),
                format!(
                    "view '{}' uses lens '{}', which the document does not declare",
                    v.id,
                    lens_name(v.lens)
                ),
            );
        }
        if let GraphViewScope::Selection {
            lanes,
            nodes,
            edges,
            flows,
            tables,
        } = &v.scope
        {
            let scoped: [(&str, &str, &[String], &BTreeSet<&str>); 5] = [
                ("lanes", "lane", lanes, &lane_ids),
                ("nodes", "node", nodes, &node_ids),
                ("edges", "edge", edges, &edge_ids),
                ("flows", "flow", flows, &flow_ids),
                ("tables", "table", tables, &table_ids),
            ];
            for (collection, singular, ids, known) in scoped {
                for (i, id) in ids.iter().enumerate() {
                    if !known.contains(id.as_str()) {
                        broken(
                            format!("{path}.scope.{collection}[{i}]"),
                            format!("view '{}' scopes unknown {singular} '{id}'", v.id),
                        );
                    }
                }
            }
        }
    }

    if let Some(w) = &g.walkthrough {
        for id in duplicates(w.steps.iter().map(|s| s.id.as_str())) {
            dups.push(("walkthrough.steps".into(), format!("duplicate step id '{id}'")));
        }
        for (i, step) in w.steps.iter().enumerate() {
            let at = format!("walkthrough.steps[{i}]");
            match &step.stage {
                Some(StepStage::View { view }) if find_view(&g.views, view).is_none() => broken(
                    format!("{at}.stage.view"),
                    format!("step '{}' stages unknown view '{view}'", step.id),
                ),
                Some(StepStage::Flow { flow }) if !flow_ids.contains(flow.as_str()) => broken(
                    format!("{at}.stage.flow"),
                    format!("step '{}' stages unknown flow '{flow}'", step.id),
                ),
                _ => {}
            }
            if let StepFocus::Selection {
                lanes,
                nodes,
                edges,
                messages,
                tables,
            } = &step.focus
            {
                let focused: [(&str, &str, &[String], &BTreeSet<&str>); 4] = [
                    ("lanes", "lane", lanes, &lane_ids),
                    ("nodes", "node", nodes, &node_ids),
                    ("edges", "edge", edges, &edge_ids),
                    ("tables", "table", tables, &table_ids),
                ];
                for (collection, singular, ids, known) in focused {
                    for (j, id) in ids.iter().enumerate() {
                        if !known.contains(id.as_str()) {
                            broken(
                                format!("{at}.focus.{collection}[{j}]"),
                                format!("step '{}' focuses unknown {singular} '{id}'", step.id),
                            );
                        }
                    }
                }
                match staged_messages(step.stage.as_ref(), g) {
                    Some(Some(on_stage)) => {
                        for (j, id) in messages.iter().enumerate() {
                            if !on_stage.contains(id.as_str()) {
                                broken(
                                    format!("{at}.focus.messages[{j}]"),
                                    format!("step '{}' focuses '{id}', which no flow on its stage carries", step.id),
                                );
                            }
                        }
                    }
                    None if !messages.is_empty() => invalid.push((
                        format!("{at}.focus.messages"),
                        format!("step '{}' focuses flow steps but names no stage to draw them on", step.id),
                    )),
                    _ => {}
                }
            }
            if let Some(detail) = &step.detail {
                let texts: Vec<&str> = detail.cites.iter().map(|c| c.text.as_str()).collect();
                let starts = cite_starts(&detail.text, &texts);
                for (j, cite) in detail.cites.iter().enumerate() {
                    if starts[j].is_none() {
                        let message = if detail.text.contains(&cite.text) {
                            format!(
                                "step '{}' cites '{}' out of the order its detail says it, or over words another cite took",
                                step.id, cite.text
                            )
                        } else {
                            format!("step '{}' cites '{}', which its detail does not say", step.id, cite.text)
                        };
                        broken(format!("{at}.detail.cites[{j}].text"), message);
                    }
                    if !cite_stands(&cite.reference, g) {
                        broken(
                            format!("{at}.detail.cites[{j}].ref"),
                            format!(
                                "step '{}' cites {}, which this document does not have",
                                step.id,
                                cited_place(&cite.reference)
                            ),
                        );
                    }
                }
            }
        }
    }

    if let Some(layout) = &g.layout {
        for (i, id) in layout.get("laneOrder").and_then(Value::as_array).into_iter().flatten().enumerate() {
            if let Some(id) = id.as_str().filter(|id| !lane_ids.contains(id)) {
                broken(format!("layout.laneOrder[{i}]"), format!("unknown lane '{id}'"));
            }
        }
        for id in layout.get("rank").and_then(Value::as_object).into_iter().flat_map(|o| o.keys()) {
            if !node_ids.contains(id.as_str()) {
                broken(format!("layout.rank.{id}"), format!("unknown node '{id}'"));
            }
        }
    }

    out.extend(dups.into_iter().map(|(p, m)| issue("DUPLICATE_ID", format!("graph.{p}"), m)));
    out.extend(invalid.into_iter().map(|(p, m)| issue(INVALID_DOCUMENT, format!("graph.{p}"), m)));
}

fn warnings(w: &Wrapper, g: &OverviewGraph) -> Vec<String> {
    let mut out = Vec::new();
    for (i, r) in w.repos.iter().enumerate() {
        if r.head == "WORKTREE" {
            out.push(format!("repos[{i}].head is WORKTREE: '{}' includes uncommitted work", r.id));
        }
    }
    if !w.repos.iter().any(|r| r.head == g.provenance.head.sha) {
        out.push("graph.provenance.head.sha matches no repos[].head".to_owned());
    }
    for (i, n) in g.nodes.iter().enumerate() {
        if n.files.is_empty() {
            out.push(format!("graph.nodes[{i}] ('{}') has no files", n.id));
        }
    }
    for (i, s) in g.walkthrough.iter().flat_map(|w| w.steps.iter()).enumerate() {
        if s.stage.is_none() {
            out.push(format!(
                "graph.walkthrough.steps[{i}] ('{}') has no stage; it plays over the current picture",
                s.id
            ));
        }
    }
    for (i, t) in g.data_model.iter().flat_map(|m| m.tables.iter()).enumerate() {
        let cols: BTreeSet<&str> = t.columns.iter().map(|c| c.name.as_str()).collect();
        for (j, x) in t.indexes.iter().enumerate() {
            for (k, c) in x.columns.iter().enumerate() {
                if !cols.contains(c.as_str()) {
                    out.push(format!(
                        "graph.dataModel.tables[{i}].indexes[{j}].columns[{k}] ('{c}') is not a column of the table"
                    ));
                }
            }
        }
    }
    for (i, o) in w.reading_order.iter().enumerate() {
        let unrated: Vec<&str> = o.review.iter().filter(|r| r.risk.is_none()).map(|r| r.path.as_str()).collect();
        if !unrated.is_empty() {
            let first = unrated.iter().take(5).copied().collect::<Vec<_>>().join(", ");
            out.push(format!(
                "readingOrder[{i}]: {} of {} review files have no risk (first: {first})",
                unrated.len(),
                o.review.len()
            ));
        }
        for (j, r) in o.review.iter().enumerate() {
            if let Some(risk @ (RiskLevel::Low | RiskLevel::Medium)) = r.risk
                && looks_domain_critical(&r.path)
            {
                let risk = if risk == RiskLevel::Low { "low" } else { "medium" };
                out.push(format!(
                    "readingOrder[{i}].review[{j}] ('{}') is rated {risk} but the path looks domain-critical",
                    r.path
                ));
            }
        }
    }
    let mut per_repo: BTreeMap<&str, usize> = BTreeMap::new();
    for n in &w.notes {
        if let Some((repo, _)) = split_repo_path(&n.path) {
            *per_repo.entry(repo).or_default() += 1;
        }
    }
    for (repo, count) in per_repo {
        if count > NOTE_BUDGET {
            out.push(format!("notes: {count} notes for repo '{repo}' (guide budget is {NOTE_BUDGET})"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{SAMPLE, SAMPLE_R1, SAMPLE_SCHEMA, json};
    use serde_json::json;

    fn issues_of(v: Value) -> Vec<Issue> {
        match load_value(v) {
            Ok(_) => panic!("expected the overview to be invalid"),
            Err(i) => i,
        }
    }

    fn has(issues: &[Issue], code: &str, path: &str) -> bool {
        issues.iter().any(|i| i.code == code && i.path == path)
    }

    fn assert_issue(v: Value, code: &str, path: &str) {
        let issues = issues_of(v);
        assert!(has(&issues, code, path), "expected {code}@{path}, got {issues:#?}");
    }

    #[test]
    fn samples_load_without_issues() {
        for (name, text) in [("sample", SAMPLE), ("r1", SAMPLE_R1), ("schema", SAMPLE_SCHEMA)] {
            let loaded = load_value(json(text)).unwrap_or_else(|e| panic!("{name}: {e:#?}"));
            assert!(!loaded.targets.is_empty(), "{name}");
        }
        let loaded = load_value(json(SAMPLE)).unwrap();
        assert!(loaded.document.graph.nodes[0].badges.is_empty());
        assert!(
            loaded
                .warnings
                .contains(&"repos[1].head is WORKTREE: 'app' includes uncommitted work".to_owned())
        );
    }

    #[test]
    fn load_reads_a_file() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/overview/sample-overview.json");
        assert!(load(path).is_ok());
        assert_eq!(load("does/not/exist.json").unwrap_err()[0].code, INVALID_OVERVIEW);
    }

    #[test]
    fn every_overview_fixture_passes_the_validator() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/overview");
        let mut n = 0;
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.extension().is_some_and(|x| x == "json") {
                n += 1;
                if let Err(issues) = load(&p) {
                    panic!("{}: {issues:?}", p.display());
                }
            }
        }
        assert!(n >= 4, "{n} fixtures");
    }

    const SAMPLE_DIAGRAMS: &str = include_str!("../../../fixtures/overview/sample-overview-diagrams.json");

    #[test]
    fn diagrams_are_kept_and_checked_and_an_overview_without_them_is_unchanged() {
        let loaded = load_value(json(SAMPLE_DIAGRAMS)).unwrap();
        let d = loaded.document.diagrams.as_ref().expect("diagrams kept");
        assert!(d.tour.is_some() && d.flowchart.is_some() && d.lineage.is_some() && d.icicle.is_some());
        let plain = load_value(json(SAMPLE_SCHEMA)).unwrap();
        assert_eq!(plain.document.diagrams, None);
        let mut both = json(SAMPLE_DIAGRAMS);
        both.as_object_mut().unwrap().remove("diagrams");
        assert_eq!(load_value(both).unwrap().document, plain.document);

        let mut bad = json(SAMPLE_DIAGRAMS);
        bad["diagrams"]["tour"]["paths"][0]["steps"][0]["from"] = json!("nowhere");
        bad["diagrams"]["icicle"]["calls"]["before"] = json!("slow");
        let issues = issues_of(bad);
        assert!(
            has(&issues, "BROKEN_REFERENCE", "diagrams.tour.paths[0].steps[0].from"),
            "{issues:#?}"
        );
        assert!(has(&issues, "INVALID_DIAGRAM", "diagrams.icicle.calls.before"), "{issues:#?}");
        let mut list = json(SAMPLE_DIAGRAMS);
        list["diagrams"] = json!([]);
        assert_issue(list, "INVALID_DIAGRAM", "diagrams");
    }

    #[test]
    fn no_render_targets() {
        let mut d = json(SAMPLE);
        d["graph"]["lenses"] = json!(["data-model"]);
        d["graph"]["flows"] = json!([]);
        d["graph"]["views"] = json!([]);
        d["graph"].as_object_mut().unwrap().remove("walkthrough");
        assert_issue(d, "NO_RENDER_TARGETS", "graph.lenses");
    }

    #[test]
    fn missing_or_wrong_format() {
        let mut d = json(SAMPLE);
        d.as_object_mut().unwrap().remove("format");
        assert_issue(d, INVALID_OVERVIEW, "format");
        let mut d = json(SAMPLE);
        d["format"] = json!("difit-overview/v2");
        assert_issue(d, INVALID_OVERVIEW, "format");
    }

    #[test]
    fn broken_edge_reference() {
        let mut d = json(SAMPLE);
        d["graph"]["edges"][0]["to"] = json!("nope");
        assert_issue(d, "BROKEN_REFERENCE", "graph.edges[0].to");
    }

    #[test]
    fn file_paths_need_a_known_repo_prefix() {
        for bad in ["account-page.component.ts", "mobile/src/app.ts", "server/"] {
            let mut d = json(SAMPLE);
            d["graph"]["nodes"][0]["files"][0]["path"] = json!(bad);
            assert_issue(d, "UNKNOWN_REPO_PREFIX", "graph.nodes[0].files[0].path");
        }
    }

    #[test]
    fn reading_order_rules() {
        let mut d = json(SAMPLE);
        d["readingOrder"][0]["repo"] = json!("mobile");
        assert_issue(d, "UNKNOWN_REPO", "readingOrder[0].repo");
        let mut d = json(SAMPLE);
        d["readingOrder"][0]["review"][0]["path"] = json!("server/src/Api/VouchersController.cs");
        assert_issue(d, INVALID_OVERVIEW, "readingOrder[0].review[0].path");
        let mut d = json(SAMPLE);
        d["readingOrder"][1]["repo"] = json!("server");
        assert_issue(d, "DUPLICATE_ID", "readingOrder[1].repo");
    }

    #[test]
    fn phase_and_note_rules() {
        let mut d = json(SAMPLE);
        d["phases"][0]["nodeIds"] = json!(["ghost"]);
        assert_issue(d, "BROKEN_REFERENCE", "phases[0].nodeIds[0]");
        let mut d = json(SAMPLE);
        d["notes"][1]["id"] = d["notes"][0]["id"].clone();
        assert_issue(d, "DUPLICATE_ID", "notes[1].id");
        let mut d = json(SAMPLE);
        d["notes"][0]["path"] = json!("mobile/src/app.ts");
        assert_issue(d, "UNKNOWN_REPO_PREFIX", "notes[0].path");
        let mut d = json(SAMPLE);
        d["notes"][0]["startLine"] = json!(20);
        d["notes"][0]["endLine"] = json!(10);
        assert_issue(d, INVALID_OVERVIEW, "notes[0].endLine");
        let mut d = json(SAMPLE);
        d["phases"][1]["id"] = json!("api");
        assert_issue(d, "DUPLICATE_ID", "phases[1].id");
    }

    #[test]
    fn duplicate_ids_in_wrapper_and_graph() {
        let mut d = json(SAMPLE);
        d["repos"][1]["id"] = json!("server");
        assert_issue(d, "DUPLICATE_ID", "repos[1].id");
        let mut d = json(SAMPLE);
        d["graph"]["nodes"][1]["id"] = json!("account-page");
        assert_issue(d, "DUPLICATE_ID", "graph.nodes");
        let mut d = json(SAMPLE);
        d["graph"]["flows"][0]["messages"][1]["id"] = json!("submit");
        assert_issue(d, "DUPLICATE_ID", "graph.flows[0].messages");
        let mut d = json(SAMPLE_SCHEMA);
        let first = d["graph"]["dataModel"]["tables"][0]["indexes"][0].clone();
        d["graph"]["dataModel"]["tables"][0]["indexes"].as_array_mut().unwrap().push(first);
        let n = d["graph"]["dataModel"]["tables"][0]["indexes"].as_array().unwrap().len() - 1;
        assert_issue(d, "DUPLICATE_ID", &format!("graph.dataModel.tables[0].indexes[{n}].name"));
        let mut d = json(SAMPLE);
        d["graph"]["views"][1]["id"] = json!("overview");
        assert_issue(d, "DUPLICATE_ID", "graph.views");
    }

    #[test]
    fn flow_view_without_data_flow_lens() {
        let mut d = json(SAMPLE);
        d["graph"]["lenses"] = json!(["architecture"]);
        let issues = issues_of(d);
        assert!(has(&issues, "BROKEN_REFERENCE", "graph.lenses"));
        assert!(has(&issues, "BROKEN_REFERENCE", "graph.views[2].lens"));
    }

    #[test]
    fn data_flow_view_that_scopes_no_flow() {
        let mut d = json(SAMPLE);
        d["graph"]["views"][2]["scope"] = json!({ "kind": "selection", "nodes": ["voucher-form"] });
        assert_issue(d, "NO_FLOW_IN_SCOPE", "graph.views[add-voucher-flow]");
    }

    #[test]
    fn broken_references_across_the_graph() {
        type Mutate = fn(&mut Value);
        let cases: Vec<(Mutate, &str)> = vec![
            (
                (|d: &mut Value| d["graph"]["nodes"][0]["lane"] = json!("ghost")),
                "graph.nodes[0].lane",
            ),
            (
                (|d: &mut Value| d["graph"]["flows"][0]["participants"][0]["node"] = json!("ghost")),
                "graph.flows[0].participants[0].node",
            ),
            (
                (|d: &mut Value| d["graph"]["flows"][0]["messages"][0]["to"] = json!("account-page")),
                "graph.flows[0].messages[0].to",
            ),
            (
                (|d: &mut Value| d["graph"]["views"][1]["scope"]["nodes"][0] = json!("ghost")),
                "graph.views[1].scope.nodes[0]",
            ),
            (
                (|d: &mut Value| d["graph"]["walkthrough"]["steps"][0]["stage"]["view"] = json!("ghost")),
                "graph.walkthrough.steps[0].stage.view",
            ),
            (
                (|d: &mut Value| d["graph"]["walkthrough"]["steps"][0]["focus"]["nodes"][0] = json!("ghost")),
                "graph.walkthrough.steps[0].focus.nodes[0]",
            ),
            (
                (|d: &mut Value| d["graph"]["walkthrough"]["steps"][1]["focus"]["messages"][0] = json!("ghost")),
                "graph.walkthrough.steps[1].focus.messages[0]",
            ),
            (
                (|d: &mut Value| d["graph"]["walkthrough"]["steps"][0]["detail"]["cites"][0]["ref"]["node"] = json!("ghost")),
                "graph.walkthrough.steps[0].detail.cites[0].ref",
            ),
            (
                (|d: &mut Value| d["graph"]["walkthrough"]["steps"][0]["detail"]["cites"][0]["text"] = json!("not in the text")),
                "graph.walkthrough.steps[0].detail.cites[0].text",
            ),
            (
                (|d: &mut Value| d["graph"]["layout"] = json!({ "laneOrder": ["ghost"] })),
                "graph.layout.laneOrder[0]",
            ),
        ];
        for (mutate, path) in cases {
            let mut d = json(SAMPLE);
            mutate(&mut d);
            assert_issue(d, "BROKEN_REFERENCE", path);
        }
    }

    #[test]
    fn broken_references_in_the_data_model() {
        let mut d = json(SAMPLE_SCHEMA);
        d["graph"]["dataModel"]["relations"][0]["from"]["column"] = json!("Ghost");
        assert_issue(d, "BROKEN_REFERENCE", "graph.dataModel.relations[0].from.column");
        let mut d = json(SAMPLE_SCHEMA);
        d["graph"]["dataModel"]["relations"][0]["to"]["table"] = json!("ghost");
        assert_issue(d, "BROKEN_REFERENCE", "graph.dataModel.relations[0].to.table");
        let mut d = json(SAMPLE_SCHEMA);
        d["graph"]["dataModel"]["tables"][0]["store"] = json!("ghost");
        assert_issue(d, "BROKEN_REFERENCE", "graph.dataModel.tables[0].store");
    }

    #[test]
    fn index_column_outside_the_table_only_warns() {
        let mut d = json(SAMPLE_SCHEMA);
        d["graph"]["dataModel"]["tables"][0]["indexes"][0]["columns"]
            .as_array_mut()
            .unwrap()
            .push(json!("Ghost"));
        let loaded = load_value(d).unwrap();
        assert!(
            loaded
                .warnings
                .contains(&"graph.dataModel.tables[0].indexes[0].columns[2] ('Ghost') is not a column of the table".to_owned())
        );
    }

    #[test]
    fn structural_errors_carry_paths() {
        let mut d = json(SAMPLE);
        d["graph"]["nodes"][2]["kind"] = json!("spaceship");
        assert_issue(d, INVALID_DOCUMENT, "graph.nodes[2].kind");
        let mut d = json(SAMPLE);
        d["graph"]["nodes"][1].as_object_mut().unwrap().remove("lane");
        assert_issue(d, INVALID_DOCUMENT, "graph.nodes[1].lane");
        let mut d = json(SAMPLE);
        d["graph"]["lanes"][0]["id"] = json!("-bad id");
        assert_issue(d, INVALID_DOCUMENT, "graph.lanes[0].id");
        let mut d = json(SAMPLE);
        d["graph"]["nodes"][0]["files"][0]["path"] = json!("app/../x.ts");
        assert_issue(d, INVALID_DOCUMENT, "graph.nodes[0].files[0].path");
        let mut d = json(SAMPLE);
        d["graph"]["walkthrough"]["steps"][0]["heading"] = json!("x".repeat(49));
        assert_issue(d, INVALID_DOCUMENT, "graph.walkthrough.steps[0].heading");
        let mut d = json(SAMPLE);
        d["graph"]["lanes"][0]["colour"] = json!("red");
        assert_issue(d, INVALID_DOCUMENT, "graph.lanes[0]");
        let mut d = json(SAMPLE);
        d["graph"]["flows"][0]["messages"][0]["kind"] = json!("self");
        assert_issue(d, INVALID_DOCUMENT, "graph.flows[0].messages[0].kind");
        let mut d = json(SAMPLE);
        d["repos"][0]["id"] = json!("Server");
        assert_issue(d, INVALID_OVERVIEW, "repos[0].id");
        let mut d = json(SAMPLE);
        d["extra"] = json!(true);
        assert_issue(d, INVALID_OVERVIEW, "");
        let mut d = json(SAMPLE);
        d["graph"]["views"][1]["scope"] = json!({ "kind": "selection" });
        assert_issue(d, INVALID_DOCUMENT, "graph.views[1].scope");
    }

    #[test]
    fn schema_version_range() {
        for (v, ok) in [
            ("0.3.0", true),
            ("0.1.7", true),
            ("0.4.0", false),
            ("1.0.0", false),
            ("0.0.1", false),
        ] {
            let mut d = json(SAMPLE);
            d["graph"]["schemaVersion"] = json!(v);
            assert_eq!(load_value(d.clone()).is_ok(), ok, "{v}");
            if !ok {
                assert_issue(d, "UNSUPPORTED_SCHEMA_VERSION", "graph.schemaVersion");
            }
        }
        let mut d = json(SAMPLE);
        d["graph"]["schemaVersion"] = json!("three");
        assert_issue(d, INVALID_DOCUMENT, "graph.schemaVersion");
    }

    #[test]
    fn focus_messages_without_a_stage() {
        let mut d = json(SAMPLE);
        d["graph"]["walkthrough"]["steps"][1].as_object_mut().unwrap().remove("stage");
        assert_issue(d, INVALID_DOCUMENT, "graph.walkthrough.steps[1].focus.messages");
    }

    #[test]
    fn warnings_match_difit() {
        let mut d = json(SAMPLE);
        for e in d["readingOrder"][0]["review"].as_array_mut().unwrap() {
            e.as_object_mut().unwrap().remove("risk");
        }
        let w = load_value(d).unwrap().warnings;
        assert_eq!(
            w.iter().filter(|w| w.contains("have no risk")).collect::<Vec<_>>(),
            [
                "readingOrder[0]: 4 of 4 review files have no risk (first: src/Api/VouchersController.cs, src/Services/VoucherService.cs, src/Services/AccountService.cs, src/Data/Voucher.cs)"
            ]
        );
        let mut d = json(SAMPLE);
        d["readingOrder"][0]["review"][2]["risk"] = json!("low");
        assert!(load_value(d).unwrap().warnings.contains(
            &"readingOrder[0].review[2] ('src/Services/AccountService.cs') is rated low but the path looks domain-critical".to_owned()
        ));
        let mut d = json(SAMPLE);
        d["readingOrder"][0]["review"][2]["path"] = json!("src/app/shelf-account/shelf-account-label.component.ts");
        d["readingOrder"][0]["review"][2]["risk"] = json!("low");
        assert!(
            !load_value(d).unwrap().warnings.iter().any(|w| w.contains("domain-critical")),
            "shelf-account is not account handling"
        );
        let mut d = json(SAMPLE);
        d["notes"] = (0..9)
            .map(|i| json!({ "id": format!("n{i}"), "path": "server/src/Api/VouchersController.cs", "startLine": i + 1, "text": "Why." }))
            .collect();
        assert!(
            load_value(d)
                .unwrap()
                .warnings
                .contains(&"notes: 9 notes for repo 'server' (guide budget is 8)".to_owned())
        );
        assert!(
            load_value(json(SAMPLE_R1))
                .unwrap()
                .warnings
                .iter()
                .any(|w| w.contains("have no risk"))
        );
    }

    #[test]
    fn several_faults_are_all_reported() {
        let mut d = json(SAMPLE);
        d["repos"][1]["id"] = json!("server");
        d["phases"][0]["nodeIds"] = json!(["ghost"]);
        d["graph"]["nodes"][0]["files"][0]["path"] = json!("mobile/src/app.ts");
        let codes: Vec<String> = issues_of(d).iter().map(|i| format!("{}@{}", i.code, i.path)).collect();
        for want in [
            "DUPLICATE_ID@repos[1].id",
            "BROKEN_REFERENCE@phases[0].nodeIds[0]",
            "UNKNOWN_REPO_PREFIX@graph.nodes[0].files[0].path",
        ] {
            assert!(codes.contains(&want.to_owned()), "{want} missing from {codes:?}");
        }
    }

    #[test]
    fn primitives() {
        assert!(is_id("a.b_c:d/e-f") && !is_id("") && !is_id("-a") && !is_id("a b"));
        assert!(is_repo_id("acme-server") && !is_repo_id("Server") && !is_repo_id("1x"));
        assert!(is_repository_path("src/a.cs") && !is_repository_path("/a") && !is_repository_path("C:x") && !is_repository_path("a\\b"));
        assert!(!is_repository_path("a/../b") && is_repository_path("a/..b"));
        assert!(is_type_name("numeric(5,4)") && is_type_name("text[]") && !is_type_name("int = 0") && !is_type_name("a--b"));
        assert!(is_iso_datetime("2026-11-06T10:00:00Z") && is_iso_datetime("2026-11-06T10:00:00.123Z") && !is_iso_datetime("2026-11-06"));
        assert_eq!(cite_starts("a b a", &["a", "a", "a"]), vec![Some(0), Some(4), None]);
    }

    #[test]
    fn domain_critical_matches_the_first_word_of_a_segment() {
        for p in [
            "src/Services/AccountService.cs",
            "src/payments/a.ts",
            "src/AUTH/x.cs",
            "src/app/delete-user.ts",
            "src/Scheduling/Job.cs",
        ] {
            assert!(looks_domain_critical(p), "{p}");
        }
        for p in [
            "src/app/shelf-account/shelf-account-label.component.ts",
            "src/hooks/useAsync.ts",
            "src/ShelfItemCodes/Controller.cs",
        ] {
            assert!(!looks_domain_critical(p), "{p}");
        }
    }
}
