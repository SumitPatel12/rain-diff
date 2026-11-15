//! difit's comment routes: `comment-imports`, `comments`, `comments-json`, `comments-output`,
//! `DELETE /api/comments/:id` (now resolves), plus `POST /api/comments/:id/state`.

use std::collections::HashMap;

use rain_comments::{comments_json_string_with, comments_output_with, import};
use rain_core::{AnswerStatus, EventKind, Session, Thread, ThreadKind, ThreadState};
use serde::Serialize;
use serde_json::{Value, json};
use tiny_http::Method;

use super::events::push;
use crate::{Ctx, Reply, Req, TEXT, error, json_reply, percent_decode, value};

pub(crate) fn route(ctx: &Ctx, r: &Req) -> Option<Reply> {
    let repo = r.repo();
    let all = r.flag("all");
    Some(match (r.method, r.path) {
        (Method::Get, "/api/comments-json") => json_reply(200, comments_json_string_with(&ctx.shared.lock(), repo, all)),
        (Method::Get, "/api/comments-output") => Reply {
            status: 200,
            content_type: TEXT,
            body: comments_output_with(&ctx.shared.lock(), repo, all),
        },
        (Method::Post, "/api/comment-imports") => comment_imports(ctx, repo, r.body),
        (Method::Post, "/api/comments") => post_comments(ctx, r.body),
        (Method::Post, p) if p.starts_with("/api/comments/") && p.ends_with("/state") => {
            let id = &p["/api/comments/".len()..p.len() - "/state".len()];
            post_state(ctx, &percent_decode(id), r.body)
        }
        (Method::Delete, p) if p.starts_with("/api/comments/") => resolve(ctx, &percent_decode(&p["/api/comments/".len()..])),
        _ => return None,
    })
}

/// Thread id → message count, to tell new threads and replies apart after an import.
fn message_counts(s: &Session) -> HashMap<String, usize> {
    s.threads.iter().map(|t| (t.id.clone(), t.messages.len())).collect()
}

/// One `thread` or `reply` event per thread an import created or replied to, plus an `ask`
/// for a new question the user asked (no author), so the window's answerer takes it up.
fn import_events(before: &HashMap<String, usize>, s: &Session) -> Vec<(EventKind, Option<String>, String, Value)> {
    let mut out = Vec::new();
    for t in &s.threads {
        let author = t.messages.last().and_then(|m| m.author.clone());
        match before.get(&t.id) {
            None => {
                out.push((
                    EventKind::Thread,
                    author.clone(),
                    t.id.clone(),
                    json!({ "filePath": t.file_path, "repo": t.repo }),
                ));
                let queued = t.answer.as_ref().is_some_and(|a| a.status == AnswerStatus::Queued);
                if t.kind == Some(ThreadKind::Question) && queued && author.as_deref().is_none_or(|a| a.trim().is_empty()) {
                    let question = t.messages.last().map(|m| m.body.clone()).unwrap_or_default();
                    out.push((
                        EventKind::Ask,
                        None,
                        t.id.clone(),
                        json!({ "threadId": t.id, "question": question, "filePath": t.file_path, "repo": t.repo, "target": t.target }),
                    ));
                }
            }
            Some(&n) if t.messages.len() > n => {
                let id = t.messages.last().map(|m| m.id.clone());
                out.push((EventKind::Reply, author, t.id.clone(), json!({ "messageId": id })));
            }
            Some(_) => {}
        }
    }
    out
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ImportReply {
    success: bool,
    changed: bool,
    count: usize,
    import_id: String,
    warnings: Vec<String>,
}

fn comment_imports(ctx: &Ctx, repo: Option<&str>, body: &str) -> Reply {
    let imports = serde_json::from_str::<Value>(body)
        .map_err(|e| e.to_string())
        .and_then(import::from_value);
    let Ok(imports) = imports else {
        return error(400, "Invalid comment import data");
    };
    let mut s = ctx.shared.lock();
    let before = message_counts(&s);
    let repo = repo.unwrap_or("");
    let r = if ctx.shared.has_excerpt_source() {
        import::apply_with_excerpts(&mut s, imports, repo, &|r, p, a| ctx.shared.excerpt(r, p, a))
    } else {
        import::apply(&mut s, imports, repo)
    };
    let new: Vec<String> = s
        .threads
        .iter()
        .filter(|t| !before.contains_key(&t.id))
        .map(|t| t.id.clone())
        .collect();
    for id in &new {
        rain_comments::ops::stamp_commit(&mut s, id);
    }
    let events = if r.changed { import_events(&before, &s) } else { Vec::new() };
    if r.changed {
        ctx.save(&s);
    }
    drop(s);
    for (kind, author, thread, data) in events {
        push(ctx, kind, author, Some(thread), data);
    }
    if r.changed {
        ctx.shared.notify();
    }
    value(
        200,
        &ImportReply {
            success: true,
            changed: r.changed,
            count: r.count,
            import_id: r.import_id,
            warnings: r.warnings,
        },
    )
}

#[derive(Serialize)]
struct PostReply<'a> {
    success: bool,
    merged: bool,
    version: u64,
    threads: &'a [Thread],
}

fn post_comments(ctx: &Ctx, body: &str) -> Reply {
    let Ok(v) = serde_json::from_str::<Value>(body) else {
        return error(400, "Invalid comment data");
    };
    let threads = match v.get("threads") {
        Some(t) => match serde_json::from_value::<Vec<Thread>>(t.clone()) {
            Ok(t) => t,
            Err(_) => return error(400, "Invalid comment data"),
        },
        None if v.get("comments").is_some() => {
            return error(400, "Invalid comment data: legacy `comments` payloads are not supported");
        }
        None => Vec::new(),
    };
    let base = v.get("baseVersion").and_then(Value::as_u64);
    let mut s = ctx.shared.lock();
    let before = s.comments_version;
    let counts = message_counts(&s);
    let merged = import::post_threads(&mut s, threads, base);
    let changed = s.comments_version != before;
    let events = if changed { import_events(&counts, &s) } else { Vec::new() };
    if changed {
        ctx.save(&s);
    }
    let reply = value(
        200,
        &PostReply {
            success: true,
            merged,
            version: s.comments_version,
            threads: &s.threads,
        },
    );
    drop(s);
    for (kind, author, thread, data) in events {
        push(ctx, kind, author, Some(thread), data);
    }
    if changed {
        ctx.shared.notify();
    }
    reply
}

/// Sets a thread's state; `false` when it already had it. `sha` and `reason` replace the old ones.
fn set_state(s: &mut Session, id: &str, state: ThreadState, sha: Option<String>, reason: Option<rain_core::DismissReason>) -> Option<bool> {
    let t = s.threads.iter_mut().find(|t| t.id == id)?;
    let reason = if state == ThreadState::Dismissed { reason } else { None };
    let sha = if state == ThreadState::Addressed { sha } else { None };
    if t.state == state && t.state_sha == sha && t.dismiss_reason == reason {
        return Some(false);
    }
    let now = rain_comments::clock::now_iso();
    t.state = state;
    t.state_sha = sha;
    t.dismiss_reason = reason;
    t.state_at = Some(now.clone());
    t.updated_at = now;
    s.bump_comments();
    Some(true)
}

fn state_changed(ctx: &Ctx, s: &Session, id: &str, by: Option<String>) {
    ctx.save(s);
    let state = s.threads.iter().find(|t| t.id == id).map(|t| (t.state, t.state_sha.clone()));
    let data = state.map_or(Value::Null, |(st, sha)| json!({ "state": st, "sha": sha }));
    push(ctx, EventKind::State, by, Some(id.to_owned()), data);
}

/// `DELETE /api/comments/:id`: difit deletes; Rain resolves (same reply), keeping the history.
/// The thread leaves the default readback; `?all=1` still shows it.
fn resolve(ctx: &Ctx, id: &str) -> Reply {
    let mut s = ctx.shared.lock();
    let Some(changed) = set_state(&mut s, id, ThreadState::Resolved, None, None) else {
        return error(404, &format!("Thread not found: {id}"));
    };
    if changed {
        state_changed(ctx, &s, id, None);
    }
    let version = s.comments_version;
    drop(s);
    if changed {
        ctx.shared.notify();
    }
    value(200, &json!({ "success": true, "threadId": id, "version": version }))
}

/// `POST /api/comments/:id/state` `{state, sha?, reason?, author?}`.
fn post_state(ctx: &Ctx, id: &str, body: &str) -> Reply {
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(body) else {
        return error(400, "Invalid state data");
    };
    let Some(state) = o.get("state").and_then(|v| serde_json::from_value::<ThreadState>(v.clone()).ok()) else {
        return error(400, "Invalid state field: state");
    };
    let text = |k: &str| {
        o.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let reason = match o.get("reason").filter(|v| !v.is_null()) {
        None => None,
        Some(v) => match serde_json::from_value::<rain_core::DismissReason>(v.clone()) {
            Ok(r) => Some(r),
            Err(_) => return error(400, "Invalid state field: reason"),
        },
    };
    if state == ThreadState::Dismissed && reason.is_none() {
        return error(400, "Dismissing needs a reason");
    }
    let mut s = ctx.shared.lock();
    let Some(changed) = set_state(&mut s, id, state, text("sha"), reason) else {
        return error(404, &format!("Thread not found: {id}"));
    };
    if changed {
        state_changed(ctx, &s, id, text("author"));
    }
    let version = s.comments_version;
    drop(s);
    if changed {
        ctx.shared.notify();
    }
    value(
        200,
        &json!({ "success": true, "threadId": id, "version": version, "changed": changed }),
    )
}
