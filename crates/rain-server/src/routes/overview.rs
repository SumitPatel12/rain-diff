//! `GET/PUT /api/overview` (difit's overview routes).

use std::collections::BTreeMap;

use rain_core::overview::{Issue, OverviewCoverage, OverviewDocument, RenderTarget};
use rain_core::{RepoFiles, Session};
use serde::Serialize;
use serde_json::{Value, json};
use tiny_http::Method;

use super::events::push;
use crate::{Ctx, Reply, Req, value};

pub(crate) fn route(ctx: &Ctx, r: &Req) -> Option<Reply> {
    Some(match (r.method, r.path) {
        (Method::Get, "/api/overview") => {
            // Session, then file lists: the order the brief route takes them in.
            let s = ctx.shared.lock();
            ctx.shared.with_diff_files(|diff| value(200, &overview_state(&s, diff)))
        }
        (Method::Put, "/api/overview") => put_overview(ctx, r.body),
        _ => return None,
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OverviewState<'a> {
    version: u64,
    overview: &'a OverviewDocument,
    self_repo: Option<&'a str>,
    links: &'a BTreeMap<String, String>,
    targets: Vec<RenderTarget>,
    default_target: Option<RenderTarget>,
    coverage: OverviewCoverage,
    coverage_by_repo: Vec<OverviewCoverage>,
    diff_loaded: bool,
}

fn repo_coverage(doc: &OverviewDocument, r: &RepoFiles) -> OverviewCoverage {
    fn strs(v: &[String]) -> Vec<&str> {
        v.iter().map(String::as_str).collect()
    }
    rain_overview::coverage(doc, Some(&r.id), &strs(&r.files), &strs(&r.generated), &strs(&r.formatting_only))
}

/// `GET /api/overview`: difit's `OverviewState` without `stepParts`, plus `coverageByRepo` (every
/// overview repo in the window's diff) and `diffLoaded`. Coverage is empty until the window has
/// loaded its diff (`diff` is `None`).
pub fn overview_state(s: &Session, diff: Option<&[RepoFiles]>) -> Value {
    let Some(doc) = &s.overview else {
        return json!({ "version": s.overview_version, "overview": null });
    };
    let targets = rain_overview::targets(&doc.graph);
    let default_target = rain_overview::default_target(&doc.graph, &targets).cloned();
    let by_repo: Vec<OverviewCoverage> = diff
        .unwrap_or_default()
        .iter()
        .filter(|r| doc.repos.iter().any(|o| o.id == r.id))
        .map(|r| repo_coverage(doc, r))
        .collect();
    let coverage = by_repo
        .iter()
        .find(|c| c.repo.is_some() && c.repo == s.self_repo)
        .cloned()
        .unwrap_or_else(|| rain_overview::coverage::empty_coverage(s.self_repo.as_deref()));
    let state = OverviewState {
        version: s.overview_version,
        overview: doc,
        self_repo: s.self_repo.as_deref(),
        links: &s.links,
        default_target,
        targets,
        coverage,
        coverage_by_repo: by_repo,
        diff_loaded: diff.is_some(),
    };
    serde_json::to_value(state).unwrap_or(Value::Null)
}

fn issue(code: &str, path: &str, message: String) -> Issue {
    Issue {
        code: code.into(),
        path: path.into(),
        message,
    }
}

fn issues_reply(error: &str, issues: &[Issue]) -> Reply {
    value(400, &json!({ "error": error, "issues": issues }))
}

/// difit's `parseLinks`: an object of repo id to http(s) URL, trailing slashes dropped.
pub fn parse_links(v: Option<&Value>) -> Result<BTreeMap<String, String>, Issue> {
    let Some(v) = v else { return Ok(BTreeMap::new()) };
    let Some(obj) = v.as_object() else {
        return Err(issue("INVALID_LINKS", "links", "links must be an object".into()));
    };
    let mut out = BTreeMap::new();
    for (repo, url) in obj {
        let http = url.as_str().filter(|u| {
            ["http://", "https://"]
                .iter()
                .any(|p| u.len() > p.len() && u.get(..p.len()).is_some_and(|h| h.eq_ignore_ascii_case(p)))
        });
        let Some(url) = http else {
            return Err(issue(
                "INVALID_LINKS",
                &format!("links.{repo}"),
                format!("links.{repo} must be an http:// or https:// URL"),
            ));
        };
        out.insert(repo.clone(), url.trim_end_matches('/').to_owned());
    }
    Ok(out)
}

/// Stores a validated overview in the session and bumps its version.
pub fn set_overview(s: &mut Session, doc: OverviewDocument, self_repo: Option<String>, links: BTreeMap<String, String>) {
    s.overview = Some(doc);
    s.self_repo = self_repo;
    s.links = links;
    s.overview_version += 1;
    s.version += 1;
}

fn put_overview(ctx: &Ctx, body: &str) -> Reply {
    let body: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let request = match &body {
        Value::Object(o) if !o.contains_key("format") => body.clone(),
        _ => json!({ "overview": body }),
    };
    let links = match parse_links(request.get("links").filter(|l| !l.is_null())) {
        Ok(l) => l,
        Err(i) => return issues_reply("Invalid links", &[i]),
    };
    let loaded = match rain_overview::load_value(request.get("overview").cloned().unwrap_or(Value::Null)) {
        Ok(l) => l,
        Err(issues) => return issues_reply("Invalid overview", &issues),
    };
    let doc = loaded.document;
    let self_repo = match request.get("self").filter(|v| !v.is_null()) {
        None => ctx
            .repo_folder
            .as_deref()
            .and_then(|f| rain_overview::self_repo(&doc, f))
            .map(str::to_owned),
        Some(Value::String(id)) if doc.repos.iter().any(|r| r.id == *id) => Some(id.clone()),
        Some(Value::String(id)) => {
            return issues_reply(
                "Unknown repo",
                &[issue(
                    "UNKNOWN_REPO",
                    "self",
                    format!("self names repo '{id}', which is not in repos"),
                )],
            );
        }
        Some(_) => return issues_reply("Unknown repo", &[issue("UNKNOWN_REPO", "self", "self must be a repo id".into())]),
    };
    let mut s = ctx.shared.lock();
    set_overview(&mut s, doc, self_repo, links);
    rain_comments::ops::reanchor_targets(&mut s);
    let saved = ctx.save(&s);
    let version = s.overview_version;
    let reply = value(
        200,
        &json!({ "success": true, "version": version, "selfRepo": s.self_repo, "warnings": loaded.warnings }),
    );
    drop(s);
    ctx.write(saved);
    push(
        ctx,
        rain_core::EventKind::Overview,
        None,
        None,
        json!({ "overviewVersion": version }),
    );
    ctx.shared.notify();
    reply
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_need_http_urls() {
        let ok = parse_links(Some(&json!({ "app": "http://localhost:4967/" }))).unwrap();
        assert_eq!(ok["app"], "http://localhost:4967");
        assert_eq!(parse_links(Some(&json!([]))).unwrap_err().path, "links");
        assert_eq!(parse_links(Some(&json!({ "x": "ftp://a" }))).unwrap_err().path, "links.x");
        assert!(parse_links(None).unwrap().is_empty());
    }
}
