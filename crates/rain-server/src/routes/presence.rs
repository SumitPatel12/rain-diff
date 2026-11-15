//! `POST /api/presence`: what the attached agent is doing. A `for=agent` long poll is a
//! heartbeat too; with no heartbeat for [`OFFLINE_AFTER`] the agent reads as offline.

use std::time::{SystemTime, UNIX_EPOCH};

use rain_core::{EventKind, Presence, PresenceState};
use serde_json::{Value, json};
use tiny_http::Method;

use super::events::push;
use crate::{Ctx, Reply, Req, error, value};

pub const OFFLINE_AFTER_SECS: u64 = 60;

pub(crate) fn route(ctx: &Ctx, r: &Req) -> Option<Reply> {
    Some(match (r.method, r.path) {
        (Method::Post, "/api/presence") => post(ctx, r.body),
        (Method::Get, "/api/presence") => value(200, &view(ctx)),
        _ => return None,
    })
}

fn post(ctx: &Ctx, body: &str) -> Reply {
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(body) else {
        return error(400, "Invalid presence data");
    };
    let Some(state) = o.get("state").and_then(|v| serde_json::from_value::<PresenceState>(v.clone()).ok()) else {
        return error(400, "Invalid presence field: state");
    };
    let text = |k: &str| {
        o.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    set(
        ctx,
        Presence {
            state,
            on: text("on"),
            reason: text("reason"),
            seen_at: Some(rain_comments::clock::now_iso()),
        },
    );
    value(200, &json!({ "success": true }))
}

/// Stores `next`; a change of state, subject or reason is an event and wakes the window.
fn set(ctx: &Ctx, next: Presence) {
    let changed = ctx.shared.update_presence(|p| {
        let changed = (p.state, &p.on, &p.reason) != (next.state, &next.on, &next.reason);
        *p = next.clone();
        changed
    });
    announce(ctx, changed.then_some(next));
}

fn announce(ctx: &Ctx, changed: Option<Presence>) {
    if let Some(p) = changed {
        push(ctx, EventKind::Presence, Some("claude".into()), p.on.clone(), json!(p));
        ctx.shared.notify();
    }
}

/// The caller is the attached agent: refresh `seenAt`, and wake an offline agent as idle.
pub(crate) fn heartbeat(ctx: &Ctx) {
    let woke = ctx.shared.update_presence(|p| {
        let wake = matches!(p.state, PresenceState::Offline) || seen_ago(p).is_none_or(|s| s > OFFLINE_AFTER_SECS);
        let changed = wake && (p.state, &p.on, &p.reason) != (PresenceState::Idle, &None, &None);
        if wake {
            p.state = PresenceState::Idle;
            p.on = None;
            p.reason = None;
        }
        p.seen_at = Some(rain_comments::clock::now_iso());
        changed.then(|| p.clone())
    });
    announce(ctx, woke);
}

/// Seconds since the agent was last heard from.
pub(crate) fn seen_ago(p: &Presence) -> Option<u64> {
    let then = millis_from_iso(p.seen_at.as_deref()?)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_millis() as i64;
    Some(u64::try_from((now - then).max(0) / 1000).unwrap_or(0))
}

/// `{state, on, reason, seenAt, seenAgo}`; `state` reads `offline` once the heartbeat is stale.
pub(crate) fn view(ctx: &Ctx) -> Value {
    let p = ctx.shared.presence();
    let ago = seen_ago(&p);
    let state = if ago.is_none_or(|s| s > OFFLINE_AFTER_SECS) {
        PresenceState::Offline
    } else {
        p.state
    };
    json!({ "state": state, "on": p.on, "reason": p.reason, "seenAt": p.seen_at, "seenAgo": ago })
}

/// `YYYY-MM-DDTHH:MM:SS[.mmm]Z` → ms since the Unix epoch.
fn millis_from_iso(s: &str) -> Option<i64> {
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let ms = if s.as_bytes().get(19) == Some(&b'.') {
        n(20..23).unwrap_or(0)
    } else {
        0
    };
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 86_400 + hh * 3600 + mm * 60 + ss) * 1000) + ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rain_comments::clock::iso_from_millis;

    #[test]
    fn iso_round_trips() {
        for ms in [0, 951_782_400_000, 1_793_959_200_123] {
            assert_eq!(millis_from_iso(&iso_from_millis(ms)), Some(ms));
        }
        assert_eq!(millis_from_iso("nope"), None);
    }
}
