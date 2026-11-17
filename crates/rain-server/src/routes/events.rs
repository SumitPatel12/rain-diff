//! `GET /api/events`: the round-2 long poll (whole threads and notes) when `kinds` is absent;
//! typed deltas from the session's event ring when it is given.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use rain_comments::comments_json;
use rain_core::{Event, EventKind};
use serde_json::{Value, json};
use tiny_http::Method;

use super::presence;
use crate::{Ctx, EVENTS_MAX_TIMEOUT, EVENTS_TIMEOUT, Reply, Req, error, value};

/// How often a waiting `for=agent` poll refreshes the agent's heartbeat.
const SLICE: Duration = Duration::from_secs(5);

pub(crate) fn route(ctx: &Ctx, r: &Req) -> Option<Reply> {
    if !(r.method == &Method::Get && r.path == "/api/events") {
        return None;
    }
    Some(match r.query.get("kinds") {
        None => legacy(ctx, r.query),
        Some(kinds) => typed(ctx, r, kinds),
    })
}

/// Records an event in the session's ring; `author` is who caused it (`None` = the window).
pub(crate) fn push(ctx: &Ctx, kind: EventKind, author: Option<String>, thread: Option<String>, data: Value) {
    ctx.shared.push_event(Event {
        seq: 0,
        at: rain_comments::clock::now_iso(),
        kind,
        author,
        thread,
        data,
    });
}

fn num(query: &BTreeMap<String, String>, k: &str) -> Option<Result<u64, std::num::ParseIntError>> {
    query.get(k).filter(|v| !v.is_empty()).map(|v| v.parse::<u64>())
}

fn timeout(query: &BTreeMap<String, String>) -> Result<u64, Reply> {
    match num(query, "timeout") {
        None => Ok(EVENTS_TIMEOUT),
        Some(Ok(t)) => Ok(t.min(EVENTS_MAX_TIMEOUT)),
        Some(Err(_)) => Err(error(400, "Invalid events field: timeout")),
    }
}

/// `?since=&timeout=`: waits (on this request's own thread) until the session `version` passes
/// `since` (default: the version now) or `timeout` seconds pass.
fn legacy(ctx: &Ctx, query: &BTreeMap<String, String>) -> Reply {
    let since = match num(query, "since") {
        None => ctx.shared.lock().version,
        Some(Ok(v)) => v,
        Some(Err(_)) => return error(400, "Invalid events field: since"),
    };
    let timeout = match timeout(query) {
        Ok(t) => t,
        Err(r) => return r,
    };
    let (s, changed) = ctx.shared.wait_change(since, Duration::from_secs(timeout));
    // Built as values under the lock (no string round trip); the reply is written after it.
    let field = |mut v: Value, k: &str| v.get_mut(k).map(Value::take).unwrap_or_else(|| json!([]));
    let threads = field(comments_json(&s, None), "threads");
    let notes = field(rain_comments::notes_json(&s, None, None), "notes");
    let version = s.version;
    drop(s);
    value(
        200,
        &json!({ "version": version, "changed": changed, "threads": threads, "notes": notes }),
    )
}

fn parse_kinds(list: &str) -> Result<Vec<EventKind>, Reply> {
    list.split(',')
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .map(|k| {
            serde_json::from_value::<EventKind>(Value::String(k.to_owned()))
                .map_err(|_| error(400, &format!("Invalid events field: kinds ({k})")))
        })
        .collect()
}

/// `?kinds=reply,ask[&since=<seq>][&for=agent][&author=claude][&timeout=]`: waits until an event
/// of those kinds (all kinds when the list is empty) newer than `since` (default: the newest now)
/// exists, leaving out events by `author`. `for=agent` marks the caller as the attached agent
/// and, with no `author`, leaves out `claude`'s own events.
fn typed(ctx: &Ctx, r: &Req, kinds: &str) -> Reply {
    let kinds = match parse_kinds(kinds) {
        Ok(k) => k,
        Err(e) => return e,
    };
    let since = match num(r.query, "since") {
        None => ctx.shared.event_seq(),
        Some(Ok(v)) => v,
        Some(Err(_)) => return error(400, "Invalid events field: since"),
    };
    let timeout = match timeout(r.query) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let agent = r.query.get("for").is_some_and(|v| v == "agent");
    let skip = r
        .query
        .get("author")
        .map(|a| a.trim().to_owned())
        .or_else(|| agent.then(|| "claude".to_owned()))
        .filter(|a| !a.is_empty());
    let keep = |e: &Event| {
        (kinds.is_empty() || kinds.contains(&e.kind))
            && skip
                .as_deref()
                .is_none_or(|a| !e.author.as_deref().is_some_and(|by| by.trim().eq_ignore_ascii_case(a)))
    };
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let (seq, events) = loop {
        if agent {
            presence::heartbeat(ctx);
        }
        let mark = ctx.shared.event_seq();
        let slice = SLICE.min(deadline.saturating_duration_since(Instant::now()));
        let events = ctx.shared.wait_events(since, slice, keep);
        if let Some(last) = events.last() {
            break (last.seq, events);
        }
        if Instant::now() >= deadline {
            break (mark.max(since), events);
        }
    };
    let version = ctx.shared.lock().version;
    value(
        200,
        &json!({
            "version": version,
            "seq": seq,
            "changed": !events.is_empty(),
            "gap": ctx.shared.events_lost_since(since),
            "events": events,
            "presence": presence::view(ctx),
        }),
    )
}
