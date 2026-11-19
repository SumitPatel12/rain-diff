//! difit's comment imports (`comment add`, `--comment`, `POST /api/comment-imports`) and the
//! thread upsert of `POST /api/comments`.

use std::collections::HashMap;

use rain_core::{Anchor, Answer, CodeSnapshot, CommentImport, ImportBody, Message, Session, Target, TargetKind, Thread, ThreadKind};
use serde_json::Value;

use crate::clock::{new_id, now_iso};
use crate::ops::{artifact_snapshot, record_answer, split_known_repo, target_from_path};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportResult {
    /// Imports read (difit's `count`), whether or not they were new.
    pub count: usize,
    pub warnings: Vec<String>,
    /// blake3 of the normalised imports, so a caller can tell a repeated import.
    pub import_id: String,
    pub changed: bool,
}

/// Parses `comment add` JSON: one import object or an array of them, checked as difit's
/// `normalizeCommentImports` does (and with its error texts). Bodies come back trimmed.
pub fn from_difit_json(json: &str) -> Result<Vec<CommentImport>, String> {
    let v: Value = serde_json::from_str(json).map_err(|_| "Invalid --comment JSON".to_owned())?;
    from_value(v)
}

pub fn from_value(v: Value) -> Result<Vec<CommentImport>, String> {
    match v {
        Value::Array(items) => items.into_iter().map(entry).collect(),
        other => Ok(vec![entry(other)?]),
    }
}

fn field(name: &str) -> String {
    format!("Invalid comment import field: {name}")
}

/// difit's position check: `{"side":"old"|"new","line":n|{"start","end"}}`, lines from 1.
pub(crate) fn check_position(pos: Option<&Value>, field: impl Fn(&str) -> String) -> Result<(), String> {
    let pos = pos.and_then(Value::as_object).ok_or_else(|| field("position"))?;
    if !matches!(pos.get("side").and_then(Value::as_str), Some("old" | "new")) {
        return Err(field("position.side"));
    }
    let positive = |v: Option<&Value>| v.and_then(Value::as_u64).filter(|n| *n > 0 && *n <= u64::from(u32::MAX));
    match pos.get("line") {
        Some(Value::Object(r)) => match (positive(r.get("start")), positive(r.get("end"))) {
            (Some(s), Some(e)) if s <= e => Ok(()),
            _ => Err(field("position.line")),
        },
        l if positive(l).is_some() => Ok(()),
        _ => Err(field("position.line")),
    }
}

/// Rain's optional `span`: lines from 1, columns from 0, start not after end.
pub(crate) fn check_span(span: Option<&Value>, field: impl Fn(&str) -> String) -> Result<(), String> {
    let Some(span) = span.filter(|s| !s.is_null()) else {
        return Ok(());
    };
    let n = |k: &str| span.get(k).and_then(Value::as_u64).filter(|n| *n <= u64::from(u32::MAX));
    match (n("startLine"), n("startCol"), n("endLine"), n("endCol")) {
        (Some(sl), Some(sc), Some(el), Some(ec)) if sl >= 1 && (sl, sc) <= (el, ec) => Ok(()),
        _ => Err(field("span")),
    }
}

fn entry(mut v: Value) -> Result<CommentImport, String> {
    let Value::Object(o) = &mut v else {
        return Err("Comment import must be an object".into());
    };
    let kind = o.get("type").and_then(Value::as_str);
    if !matches!(kind, Some("thread" | "reply")) {
        return Err(field("type"));
    }
    if kind == Some("reply") && o.contains_key("threadId") {
        if o.get("threadId").and_then(Value::as_str).is_none_or(|t| t.trim().is_empty()) {
            return Err(field("threadId"));
        }
        o.remove("position");
        o.remove("span");
    } else {
        o.remove("threadId");
        if o.get("filePath").and_then(Value::as_str).is_none_or(|p| p.trim().is_empty()) {
            return Err(field("filePath"));
        }
        check_position(o.get("position"), field)?;
        check_span(o.get("span"), field)?;
    }
    if o.get("body").and_then(Value::as_str).is_none_or(|b| b.trim().is_empty()) {
        return Err(field("body"));
    }
    for name in ["createdAt", "updatedAt"] {
        if o.get(name).is_some_and(|t| t.as_str().is_none_or(|t| t.trim().is_empty())) {
            return Err(field(name));
        }
    }
    let mut imp: CommentImport = serde_json::from_value(v).map_err(|e| format!("Invalid comment import: {e}"))?;
    let b = body_mut(&mut imp);
    b.body = b.body.trim().to_owned();
    Ok(imp)
}

fn body_mut(i: &mut CommentImport) -> &mut ImportBody {
    match i {
        CommentImport::Thread(b) | CommentImport::Reply(b) => b,
    }
}

/// The thread dedupe key difit launch scripts use:`file|side|line-or-start-end|body`,
/// with a span's columns added so two spans on the same lines stay apart.
pub fn thread_key(file_path: &str, anchor: &Anchor, body: &str) -> String {
    format!("{file_path}|{}|{body}", position_key(anchor))
}

fn position_key(a: &Anchor) -> String {
    let side = match a.side() {
        rain_core::Side::Old => "old",
        rain_core::Side::New => "new",
    };
    match *a {
        Anchor::Line { line, .. } => format!("{side}|{line}"),
        Anchor::Range { start, end, .. } => format!("{side}|{start}-{end}"),
        Anchor::Span {
            start_line,
            start_col,
            end_line,
            end_col,
            ..
        } => format!("{side}|{start_line}-{end_line}@{start_col}-{end_col}"),
    }
}

fn same_position(a: &Anchor, b: &Anchor) -> bool {
    position_key(a) == position_key(b)
}

/// The thread sits at `path`/`anchor` now, or was posted there before its code moved.
fn at_position(t: &Thread, path: &str, anchor: &Anchor) -> bool {
    t.file_path == path && (same_position(&t.anchor, anchor) || t.original.as_ref().is_some_and(|o| same_position(o, anchor)))
}

fn author_of(a: Option<&str>) -> &str {
    a.map_or("", str::trim)
}

/// difit's `messageMatchesImport`: same id, or same author and body (and `createdAt` when the
/// import names one).
fn message_matches(m: &Message, b: &ImportBody) -> bool {
    if b.id.as_deref().is_some_and(|id| id == m.id) {
        return true;
    }
    author_of(m.author.as_deref()) == author_of(b.author.as_deref())
        && m.body == b.body
        && b.created_at.as_ref().is_none_or(|c| *c == m.created_at)
}

/// Appends `b` to `t` unless `t` already holds it; whether it did.
fn append_reply(t: &mut Thread, b: &ImportBody, created: String, updated: String) -> bool {
    if t.messages.iter().any(|m| message_matches(m, b)) {
        return false;
    }
    t.updated_at = max_ts(&t.updated_at, &updated);
    let id = b.id.clone().unwrap_or_else(new_id);
    record_answer(t, &id, b.author.as_deref(), &created);
    t.messages.push(Message {
        id,
        body: b.body.clone(),
        author: b.author.clone(),
        created_at: created,
        updated_at: updated,
    });
    true
}

fn max_ts(a: &str, b: &str) -> String {
    if a >= b { a.to_owned() } else { b.to_owned() }
}

/// `(repo, path, anchor)` → the anchored text of the loaded diff ([`rain_core::Shared::excerpt`]).
pub type Excerpt<'a> = &'a dyn Fn(&str, &str, &Anchor) -> Option<CodeSnapshot>;

/// Applies imports in order: a thread is skipped when one with its id, or with its file,
/// position and first-message body, exists; a reply joins the newest thread at its file and
/// position unless a matching message is already there, and warns when there is none. Threads
/// land in `repo` unless the import names one. Bumps `version` once when anything changed.
/// Nothing is snapshotted: see [`apply_with_excerpts`] for imports against a loaded diff.
pub fn apply(s: &mut Session, imports: Vec<CommentImport>, repo: &str) -> ImportResult {
    apply_inner(s, imports, repo, None)
}

/// [`apply`] against the loaded diff: a line thread without a snapshot takes one now from
/// `excerpt`, so it is anchored to the code it was written against; a thread whose lines the
/// diff does not show becomes a file-level thread, with a warning, rather than a silent line
/// thread.
pub fn apply_with_excerpts(s: &mut Session, imports: Vec<CommentImport>, repo: &str, excerpt: Excerpt<'_>) -> ImportResult {
    apply_inner(s, imports, repo, Some(excerpt))
}

fn apply_inner(s: &mut Session, imports: Vec<CommentImport>, repo: &str, excerpt: Option<Excerpt<'_>>) -> ImportResult {
    let import_id = blake3::hash(serde_json::to_string(&imports).unwrap_or_default().as_bytes())
        .to_hex()
        .to_string();
    let count = imports.len();
    let mut warnings = Vec::new();
    let mut changed = false;
    for mut imp in imports {
        let now = now_iso();
        let is_thread = matches!(imp, CommentImport::Thread(_));
        let b = body_mut(&mut imp);
        b.body = b.body.trim().to_owned();
        let (repo, file_path) = match &b.repo {
            Some(r) => (r.clone(), b.file_path.clone()),
            None if repo.is_empty() => split_known_repo(s, &b.file_path),
            None => (repo.to_owned(), b.file_path.clone()),
        };
        b.file_path = file_path;
        let created = b.created_at.clone().unwrap_or(now);
        let updated = b.updated_at.clone().unwrap_or_else(|| created.clone());
        if !is_thread && let Some(id) = b.thread_id.clone() {
            let Some(t) = s.threads.iter_mut().find(|t| t.id == id) else {
                warnings.push(format!("No thread with id {id}."));
                continue;
            };
            changed |= append_reply(t, b, created, updated);
            continue;
        }
        let Some(anchor) = b.anchor else {
            warnings.push(format!("Skipped comment import for {} because it has no position.", b.file_path));
            continue;
        };
        if is_thread {
            let dup = s.threads.iter().any(|t| {
                b.id.as_deref().is_some_and(|id| id == t.id)
                    || (t.repo == repo
                        && t.messages.first().map_or("", |m| m.body.as_str()) == b.body
                        && at_position(t, &b.file_path, &anchor))
            });
            if dup {
                continue;
            }
            let id = b.id.clone().unwrap_or_else(new_id);
            let mut target = b.target.clone().or_else(|| target_from_path(&b.file_path));
            let mut snapshot = b.code_snapshot.clone();
            if let Some(t) = &target
                && snapshot.is_none()
            {
                snapshot = artifact_snapshot(s, t);
            }
            if let Some(get) = excerpt
                && snapshot.is_none()
                && target.as_ref().is_none_or(|t| t.kind == TargetKind::Line)
            {
                snapshot = get(&repo, &b.file_path, &anchor);
                if snapshot.is_none() {
                    warnings.push(format!(
                        "{}:{} is outside the loaded diff; stored as a file-level thread.",
                        b.file_path,
                        difit_position_key(&anchor)
                    ));
                    target = Some(Target {
                        kind: TargetKind::File,
                        ..Target::default()
                    });
                }
            }
            let asks = b.kind == Some(ThreadKind::Question) && b.author.as_deref().is_none_or(|a| a.trim().is_empty());
            s.threads.push(Thread {
                id: id.clone(),
                file_path: b.file_path.clone(),
                created_at: created.clone(),
                updated_at: updated.clone(),
                anchor,
                code_snapshot: snapshot,
                messages: vec![Message {
                    id,
                    body: b.body.clone(),
                    author: b.author.clone(),
                    created_at: created,
                    updated_at: updated,
                }],
                repo,
                kind: b.kind,
                blocking: b.blocking,
                state: b.state.unwrap_or_default(),
                target,
                answer: b.answer.clone().or_else(|| asks.then(Answer::default)),
                ..Thread::default()
            });
            changed = true;
            continue;
        }
        let target = s
            .threads
            .iter_mut()
            .filter(|t| t.repo == repo && at_position(t, &b.file_path, &anchor))
            .rev()
            .max_by(|x, y| x.updated_at.cmp(&y.updated_at));
        let Some(t) = target else {
            warnings.push(format!(
                "Skipped reply import for {}:{} because no matching thread was found.",
                b.file_path,
                difit_position_key(&anchor)
            ));
            continue;
        };
        changed |= append_reply(t, b, created, updated);
    }
    if changed {
        s.bump_comments();
    }
    ImportResult {
        count,
        warnings,
        import_id,
        changed,
    }
}

/// difit's `getPositionKey` (`new:12`, `old:3-7`), for its warning text.
fn difit_position_key(a: &Anchor) -> String {
    let side = match a.side() {
        rain_core::Side::Old => "old",
        rain_core::Side::New => "new",
    };
    match a.lines() {
        (l, e) if l == e && matches!(a, Anchor::Line { .. }) => format!("{side}:{l}"),
        (l, e) => format!("{side}:{l}-{e}"),
    }
}

fn messages_match(a: &Message, b: &Message) -> bool {
    a.id == b.id || (author_of(a.author.as_deref()) == author_of(b.author.as_deref()) && a.body == b.body && a.created_at == b.created_at)
}

fn threads_match(a: &Thread, b: &Thread) -> bool {
    if a.id == b.id {
        return true;
    }
    if a.repo != b.repo || a.file_path != b.file_path || !same_position(&a.anchor, &b.anchor) {
        return false;
    }
    match (a.messages.first(), b.messages.first()) {
        (Some(x), Some(y)) => messages_match(x, y),
        _ => false,
    }
}

/// difit's `mergeCommentThreads` for a stale base: see [`upsert_threads`].
pub fn merge_threads(existing: &[Thread], incoming: &[Thread]) -> Vec<Thread> {
    upsert_threads(existing, incoming, false).0
}

/// Incoming threads join their match (newer message edits win, replies sorted by time after the
/// first message) or are appended; a stored thread the client didn't send is kept. The state,
/// kind, blocking flag and disposition come from the client when it read the current version
/// (`current`) or its copy is newer; otherwise a different value is left alone and the thread's
/// id is returned as a conflict.
pub fn upsert_threads(existing: &[Thread], incoming: &[Thread], current: bool) -> (Vec<Thread>, Vec<String>) {
    let mut out: Vec<Thread> = existing.to_vec();
    let mut by_id: HashMap<String, usize> = out.iter().enumerate().map(|(i, t)| (t.id.clone(), i)).collect();
    let mut conflicts = Vec::new();
    for inc in incoming {
        let found = by_id
            .get(&inc.id)
            .copied()
            .or_else(|| out.iter().position(|t| threads_match(t, inc)));
        let Some(i) = found else {
            by_id.insert(inc.id.clone(), out.len());
            out.push(inc.clone());
            continue;
        };
        let cur = &mut out[i];
        let newer = current || inc.updated_at > cur.updated_at;
        if newer {
            cur.state = inc.state;
            cur.state_sha.clone_from(&inc.state_sha);
            cur.state_at.clone_from(&inc.state_at);
            cur.dismiss_reason = inc.dismiss_reason;
            cur.kind = inc.kind;
            cur.blocking = inc.blocking;
            cur.disposition = inc.disposition;
        } else {
            if inc.disposition.is_some() {
                cur.disposition = inc.disposition;
            }
            let differs = (cur.state, &cur.state_sha, cur.dismiss_reason, cur.kind, cur.blocking)
                != (inc.state, &inc.state_sha, inc.dismiss_reason, inc.kind, inc.blocking);
            if differs {
                conflicts.push(cur.id.clone());
            }
        }
        let mut msgs = cur.messages.clone();
        for m in &inc.messages {
            match msgs.iter().position(|x| messages_match(x, m)) {
                Some(j) if m.updated_at >= msgs[j].updated_at => msgs[j] = m.clone(),
                Some(_) => {}
                None => msgs.push(m.clone()),
            }
        }
        if msgs.len() > 1 {
            msgs[1..].sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
        }
        let mut updated = max_ts(&cur.updated_at, &inc.updated_at);
        for m in &msgs {
            updated = max_ts(&updated, &m.updated_at);
        }
        if inc.created_at < cur.created_at {
            cur.created_at = inc.created_at.clone();
        }
        cur.updated_at = updated;
        if cur.code_snapshot.is_none() {
            cur.code_snapshot = inc.code_snapshot.clone();
        }
        if cur.target.is_none() {
            cur.target = inc.target.clone();
        }
        if cur.answer.is_none() {
            cur.answer = inc.answer.clone();
        }
        cur.messages = msgs;
    }
    (out, conflicts)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PostResult {
    /// The client's base was stale.
    pub merged: bool,
    /// Threads whose posted state, kind or blocking flag differs from the stored copy, which is
    /// newer and was kept.
    pub conflicts: Vec<String>,
}

/// `POST /api/comments`: upserts the threads ([`upsert_threads`]); a thread the client didn't
/// send is kept, since readback leaves out resolved threads and other repos'. Deleting is
/// `DELETE /api/comments/:id`. Bumps the versions only when the threads changed.
pub fn post(s: &mut Session, threads: &[Thread], base_version: Option<u64>) -> PostResult {
    let stale = base_version.is_some_and(|v| v != s.comments_version);
    let (next, conflicts) = upsert_threads(&s.threads, threads, !stale);
    if next != s.threads {
        s.threads = next;
        s.bump_comments();
    }
    PostResult { merged: stale, conflicts }
}

/// [`post`]; whether the base was stale.
pub fn post_threads(s: &mut Session, threads: Vec<Thread>, base_version: Option<u64>) -> bool {
    post(s, &threads, base_version).merged
}
