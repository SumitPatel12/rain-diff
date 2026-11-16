//! `GET /api/health`, `POST /api/reload`, `POST /api/close`.

use rain_core::{EventKind, UiRequest};
use serde_json::{Value, json};
use tiny_http::Method;

use super::events::push;
use super::presence;
use crate::{Ctx, Reply, Req, value};

pub(crate) fn route(ctx: &Ctx, r: &Req) -> Option<Reply> {
    Some(match (r.method, r.path) {
        (Method::Get, "/api/health") => health(ctx),
        (Method::Post, "/api/reload") => {
            ctx.shared.push_request(UiRequest::Reload);
            push(ctx, EventKind::Reload, author(r.body), None, Value::Null);
            value(200, &json!({ "success": true, "version": ctx.shared.lock().version }))
        }
        (Method::Post, "/api/close") => {
            ctx.shared.push_request(UiRequest::Close);
            value(200, &json!({ "success": true }))
        }
        _ => return None,
    })
}

/// The optional `{"author"}` of a lifecycle request body.
fn author(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()?
        .get("author")?
        .as_str()
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(str::to_owned)
}

fn health(ctx: &Ctx) -> Reply {
    let diff_loaded = ctx.shared.has_diff_files();
    let agent = presence::view(ctx);
    let asks = ctx.shared.asks();
    let s = ctx.shared.lock();
    let target_sha: Vec<Value> = s
        .repos
        .iter()
        .map(|r| json!({ "repo": r.id, "base": r.base, "target": r.target }))
        .collect();
    value(
        200,
        &json!({
            "ok": true,
            "app": "rain",
            "appVersion": env!("CARGO_PKG_VERSION"),
            "pid": std::process::id(),
            "session": s.id,
            "version": s.version,
            "repos": s.repos.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            "threads": s.threads.len(),
            "notes": s.notes.len(),
            "targetSha": target_sha,
            "diffLoaded": diff_loaded,
            "stale": ctx.shared.stale(),
            "round": s.round(),
            "agent": { "state": agent["state"], "seenAgo": agent["seenAgo"] },
            "asks": asks,
            "lock": ctx.lock.as_ref().map(|p| json!({ "path": p })),
        }),
    )
}
