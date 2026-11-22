//! `GET /api/next`: what waits on the agent, in the order to take it up. Read-only: it claims
//! nothing and is no heartbeat.

use rain_comments::ops::{self, Owed};
use rain_comments::thread_location;
use rain_core::{Session, Thread, ThreadState};
use serde_json::{Map, Value, json};
use tiny_http::Method;

use super::brief::stale;
use crate::{Ctx, Reply, Req, value};

/// The reviewer writes with no author, as the window does.
const REVIEWER: &str = "";
/// Chars of the reviewer's text an item carries; `comment get` has the whole thread.
const TEXT_MAX: usize = 400;

pub(crate) fn route(ctx: &Ctx, r: &Req) -> Option<Reply> {
    (r.method == &Method::Get && r.path == "/api/next").then(|| next(ctx))
}

/// `{"waiting","round","seq","items":[…]}`: questions, threads to reply to, questions claimed
/// and not answered (each group oldest first), then the last round's verdict and the brief.
/// `seq` is the newest event before the read, for `watch --since`.
fn next(ctx: &Ctx) -> Reply {
    let seq = ctx.shared.event_seq();
    let s = ctx.shared.lock();
    let mut owed: Vec<(Owed, &str, &Thread)> = s
        .threads
        .iter()
        .filter_map(|t| {
            Some((
                ops::waiting_on_agent(t, REVIEWER)?,
                said(t).map_or("", |m| m.created_at.as_str()),
                t,
            ))
        })
        .collect();
    owed.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    let mut items: Vec<Value> = owed.iter().map(|(o, _, t)| thread_item(&s, *o, t)).collect();
    items.extend(verdict_item(&s));
    items.extend(brief_item(&s));
    let out = json!({ "waiting": items.len(), "round": s.round(), "seq": seq, "items": items });
    drop(s);
    value(200, &out)
}

/// The reviewer's last message, else the thread's last.
fn said(t: &Thread) -> Option<&rain_core::Message> {
    t.messages.iter().rev().find(|m| ops::from_user(m, REVIEWER)).or(t.messages.last())
}

fn trimmed(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= TEXT_MAX {
        return text.to_owned();
    }
    let cut: String = text.chars().take(TEXT_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

fn thread_item(s: &Session, owed: Owed, t: &Thread) -> Value {
    let mut o = match serde_json::to_value(t.anchor) {
        Ok(Value::Object(o)) => o,
        _ => Map::new(),
    };
    let kind = match owed {
        Owed::Question => "question",
        Owed::Reply => "reply",
        Owed::Claimed => "claimed",
    };
    o.insert("kind".into(), json!(kind));
    o.insert("threadId".into(), json!(t.id));
    o.insert("filePath".into(), json!(t.file_path));
    if !t.repo.is_empty() {
        o.insert("repo".into(), json!(t.repo));
    }
    o.insert("where".into(), json!(thread_location(s, t)));
    o.insert("threadKind".into(), json!(t.kind()));
    if t.blocking() {
        o.insert("blocking".into(), json!(true));
    }
    o.insert("state".into(), json!(t.state));
    if let Some(target) = &t.target {
        o.insert("target".into(), json!(target));
    }
    if let Some(a) = &t.answer {
        o.insert("answer".into(), json!(a));
    }
    if let Some(m) = said(t) {
        o.insert("text".into(), json!(trimmed(&m.body)));
        o.insert("at".into(), json!(m.created_at));
    }
    Value::Object(o)
}

/// The last finished round, until the agent answers it: a reply or an `addressed` after it, or
/// another sha loaded.
fn verdict_item(s: &Session) -> Option<Value> {
    let r = s.rounds.last()?;
    let at = r.at.as_str();
    let shas = |repos: &[rain_core::SessionRepo]| repos.iter().map(|x| (x.id.clone(), x.target.clone())).collect::<Vec<_>>();
    let answered = shas(&r.shas) != shas(&s.repos)
        || s.threads.iter().any(|t| {
            (t.state == ThreadState::Addressed && t.state_at.as_deref().is_some_and(|x| x >= at))
                || t.messages
                    .iter()
                    .rev()
                    .take_while(|m| m.created_at.as_str() >= at)
                    .any(|m| !ops::from_user(m, REVIEWER))
        });
    (!answered).then(|| json!({ "kind": "verdict", "round": r.n, "verdict": r.verdict, "text": trimmed(&r.summary), "at": r.at }))
}

fn brief_item(s: &Session) -> Option<Value> {
    let reason = match &s.brief {
        None => "missing",
        Some(b) if stale(s, b) => "stale",
        Some(_) => return None,
    };
    Some(json!({ "kind": "brief", "reason": reason }))
}
