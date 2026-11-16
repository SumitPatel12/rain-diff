//! `GET/PUT /api/brief`: the agent's narrative (intent, how, invariants, tour) for this change.

use std::collections::BTreeSet;

use rain_core::brief::{BRIEF_FORMAT, BriefDoc, RepoSha};
use rain_core::{EventKind, RepoFiles, Session};
use rain_overview::brief::{CheckInput, check};
use serde_json::{Value, json};
use tiny_http::Method;

use super::events::push;
use crate::{Ctx, Reply, Req, error, value};

pub(crate) fn route(ctx: &Ctx, r: &Req) -> Option<Reply> {
    Some(match (r.method, r.path) {
        (Method::Get, "/api/brief") => get(ctx),
        (Method::Put, "/api/brief") if r.markdown => put_markdown(ctx, r.body),
        (Method::Put, "/api/brief") => put(ctx, r.body),
        _ => return None,
    })
}

/// `{"brief","version","stale","check"}`; `brief` is null until an agent sets one.
fn get(ctx: &Ctx) -> Reply {
    let s = ctx.shared.lock();
    let stale = s.brief.as_ref().is_some_and(|b| stale(&s, b));
    let check = s.brief.as_ref().map_or(Value::Null, |b| check_of(ctx, &s, b));
    value(
        200,
        &json!({ "brief": s.brief, "version": s.brief_version, "stale": stale, "check": check }),
    )
}

/// The window's installed check when it has one, else the brief against the files the window
/// published: phantom paths, files not cited, stale shas (no line check without the hunks).
fn check_of(ctx: &Ctx, s: &Session, b: &BriefDoc) -> Value {
    let installed = ctx.shared.brief_check(b);
    if installed.as_object().is_none_or(|o| !o.is_empty()) {
        return installed;
    }
    let Some(repos) = ctx.shared.diff_files() else {
        return installed;
    };
    let (keys, noise) = keys_of(&repos);
    let targets: Vec<(String, String)> = s.repos.iter().map(|r| (r.id.clone(), r.target.clone())).collect();
    let c = check(
        b,
        &CheckInput {
            keys: &keys,
            noise: &noise,
            targets: &targets,
            shown: None,
        },
    );
    serde_json::to_value(c).unwrap_or(installed)
}

/// Every diff file as the window keys it (`<repo>/<path>` with several repos), and the generated
/// and formatting-only ones a brief need not cite.
fn keys_of(repos: &[RepoFiles]) -> (Vec<String>, BTreeSet<String>) {
    let key = |r: &RepoFiles, p: &str| {
        if repos.len() > 1 {
            rain_core::paths::join(&r.id, p)
        } else {
            p.to_owned()
        }
    };
    let keys = repos.iter().flat_map(|r| r.files.iter().map(move |p| key(r, p))).collect();
    let noise = repos
        .iter()
        .flat_map(|r| r.generated.iter().chain(&r.formatting_only).map(move |p| key(r, p)))
        .collect();
    (keys, noise)
}

/// The brief names a sha for one of the session's repos that is not the one loaded. Either sha
/// may be short.
pub(crate) fn stale(s: &Session, b: &BriefDoc) -> bool {
    b.written_against.iter().any(|w| {
        s.repos
            .iter()
            .find(|r| r.id == w.repo || (s.repos.len() == 1 && w.repo.is_empty()))
            .is_some_and(|r| {
                let same = r.target.starts_with(&w.sha) || w.sha.starts_with(&r.target);
                !r.target.is_empty() && !w.sha.is_empty() && !same
            })
    })
}

/// Stores the brief, then moves, outdates or orphans the threads on its sections (a section
/// removed orphans its threads with the text they were posted on).
fn put(ctx: &Ctx, body: &str) -> Reply {
    let v = match serde_json::from_str::<Value>(body) {
        Ok(v @ Value::Object(_)) => v,
        _ => return error(400, "Invalid brief: the body must be a JSON object"),
    };
    match serde_json::from_value::<BriefDoc>(v) {
        Ok(d) => store(ctx, d),
        Err(e) => error(400, &format!("Invalid brief: {e}")),
    }
}

/// The Brief as one Markdown document; `writtenAgainst` is the session's loaded shas.
fn put_markdown(ctx: &Ctx, body: &str) -> Reply {
    let shas: Vec<RepoSha> = ctx
        .shared
        .lock()
        .repos
        .iter()
        .filter(|r| !r.target.is_empty())
        .map(|r| RepoSha {
            repo: r.id.clone(),
            sha: r.target.clone(),
        })
        .collect();
    match BriefDoc::from_markdown(body, &shas) {
        Ok(d) => store(ctx, d),
        Err(e) => error(400, &format!("Invalid brief: {e}")),
    }
}

fn store(ctx: &Ctx, mut doc: BriefDoc) -> Reply {
    if !doc.format.is_empty() && doc.format != BRIEF_FORMAT {
        return error(400, &format!("Invalid brief: format must be {BRIEF_FORMAT}"));
    }
    doc.assign_ids();
    let mut s = ctx.shared.lock();
    s.brief = Some(doc);
    s.bump_brief();
    let moved = rain_comments::ops::reanchor_targets(&mut s);
    let saved = ctx.save(&s);
    let version = s.brief_version;
    let stale = s.brief.as_ref().is_some_and(|b| stale(&s, b));
    drop(s);
    ctx.write(saved);
    push(
        ctx,
        EventKind::Brief,
        Some("claude".into()),
        None,
        json!({ "briefVersion": version, "threadsMoved": moved }),
    );
    ctx.shared.notify();
    value(
        200,
        &json!({ "success": true, "version": version, "stale": stale, "threadsMoved": moved }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_join_repos_only_when_there_are_several() {
        let one = vec![RepoFiles {
            id: "server".into(),
            files: vec!["src/A.cs".into(), "gen/B.g.cs".into()],
            generated: vec!["gen/B.g.cs".into()],
            formatting_only: vec![],
        }];
        let (keys, noise) = keys_of(&one);
        assert_eq!(keys, ["src/A.cs", "gen/B.g.cs"]);
        assert!(noise.contains("gen/B.g.cs"));
        let mut two = one.clone();
        two.push(RepoFiles {
            id: "app".into(),
            files: vec!["a.ts".into()],
            ..RepoFiles::default()
        });
        assert_eq!(keys_of(&two).0, ["server/src/A.cs", "server/gen/B.g.cs", "app/a.ts"]);
    }
}
