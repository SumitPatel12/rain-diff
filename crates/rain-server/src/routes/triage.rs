//! `GET /api/triage`: the window's ordered, grouped file list and facts, for an agent to read
//! instead of the whole diff.

use serde_json::json;
use tiny_http::Method;

use crate::{Ctx, Reply, Req, value};

pub(crate) fn route(ctx: &Ctx, r: &Req) -> Option<Reply> {
    if !(r.method == &Method::Get && r.path == "/api/triage") {
        return None;
    }
    Some(match ctx.shared.triage() {
        Some(t) => value(200, &t),
        None => value(200, &json!({ "diffLoaded": false, "groups": [], "items": [] })),
    })
}
