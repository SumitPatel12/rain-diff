//! One module per route family; [`route`] asks each in turn.

pub(crate) mod brief;
pub(crate) mod compat;
pub(crate) mod events;
pub(crate) mod lifecycle;
pub(crate) mod lock;
pub(crate) mod notes;
pub(crate) mod overview;
pub(crate) mod presence;
pub(crate) mod review;
pub(crate) mod state;
pub(crate) mod triage;

use tiny_http::Method;

use crate::{Ctx, Reply, Req, error};

type Route = fn(&Ctx, &Req) -> Option<Reply>;

const ROUTES: [Route; 11] = [
    lifecycle::route,
    compat::route,
    notes::route,
    overview::route,
    events::route,
    state::route,
    presence::route,
    brief::route,
    triage::route,
    review::route,
    lock::route,
];

pub(crate) fn route(ctx: &Ctx, r: &Req) -> Reply {
    if let Some(why) = &ctx.read_only
        && changes_session(r)
    {
        return error(423, why);
    }
    ROUTES
        .iter()
        .find_map(|f| f(ctx, r))
        .unwrap_or_else(|| error(404, &format!("Not found: {}", r.path)))
}

/// A write that would be lost in a read-only window; reload, close, presence, goto and
/// highlight only steer the window and still work there.
fn changes_session(r: &Req) -> bool {
    !matches!(r.method, Method::Get | Method::Head)
        && !matches!(
            r.path,
            "/api/reload" | "/api/close" | "/api/presence" | "/api/goto" | "/api/highlight"
        )
}
