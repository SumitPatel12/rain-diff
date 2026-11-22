//! Session mutations. Each one that changes something bumps `session.version`.

use std::collections::HashMap;

use rain_core::brief::BriefDoc;
use rain_core::comments::{BRIEF_PATH, MAP_PATH, NOTE_PATH_PREFIX, PR_PATH};
use rain_core::overview::OverviewGraph;
use rain_core::{
    Anchor, AnchorHow, AnchorState, Answer, AnswerStatus, Cite, CodeSnapshot, DiffSet, DismissReason, Disposition, Draft, FileDiff,
    FileStatus, LineKind, Message, Note, NoteKind, Orphan, OrphanReason, Session, Side, Target, TargetKind, Thread, ThreadKind,
    ThreadState, ViewedRecord,
};

use crate::clock::{new_id, now_iso};

/// What a new thread is beyond its anchor and first message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ThreadExtra {
    pub kind: Option<ThreadKind>,
    pub blocking: Option<bool>,
    pub target: Option<Target>,
}

/// Starts a thread and returns its id. The first message shares the thread's id, as difit's
/// imported threads do.
pub fn add_thread(
    s: &mut Session,
    repo: &str,
    file_path: &str,
    anchor: Anchor,
    body: &str,
    author: Option<&str>,
    snapshot: Option<CodeSnapshot>,
) -> String {
    add_thread_with(s, repo, file_path, anchor, body, author, snapshot, ThreadExtra::default())
}

/// [`add_thread`] with a kind, blocking flag and artifact target. A question the user asks starts
/// with its answer queued.
#[allow(clippy::too_many_arguments)]
pub fn add_thread_with(
    s: &mut Session,
    repo: &str,
    file_path: &str,
    anchor: Anchor,
    body: &str,
    author: Option<&str>,
    snapshot: Option<CodeSnapshot>,
    extra: ThreadExtra,
) -> String {
    let id = new_id();
    let now = now_iso();
    let asks = extra.kind == Some(ThreadKind::Question) && author.is_none_or(|a| a.trim().is_empty());
    let target = extra.target.or_else(|| target_from_path(file_path));
    s.threads.push(Thread {
        id: id.clone(),
        file_path: file_path.to_owned(),
        created_at: now.clone(),
        updated_at: now.clone(),
        anchor,
        code_snapshot: snapshot,
        messages: vec![Message {
            id: id.clone(),
            body: body.to_owned(),
            author: author.map(str::to_owned),
            created_at: now.clone(),
            updated_at: now,
        }],
        repo: repo.to_owned(),
        kind: extra.kind,
        blocking: extra.blocking,
        target,
        answer: asks.then(Answer::default),
        ..Thread::default()
    });
    s.bump_comments();
    id
}

/// The target a virtual `filePath` implies (`@brief`, `@map`, `@pr`, `@note:<id>`).
pub fn target_from_path(file_path: &str) -> Option<Target> {
    let kind = match file_path {
        BRIEF_PATH => TargetKind::Brief,
        MAP_PATH => TargetKind::Node,
        PR_PATH => TargetKind::Pr,
        p if p.starts_with(NOTE_PATH_PREFIX) => {
            return Some(Target {
                kind: TargetKind::Note,
                note: Some(p[NOTE_PATH_PREFIX.len()..].to_owned()),
                ..Target::default()
            });
        }
        _ => return None,
    };
    Some(Target { kind, ..Target::default() })
}

pub fn thread<'a>(s: &'a Session, id: &str) -> Option<&'a Thread> {
    s.threads.iter().find(|t| t.id == id)
}

/// `"server/src/x.cs"` → `("server", "src/x.cs")` when the session has several repos and `server`
/// is one of their ids; otherwise `("", path)`.
pub fn split_known_repo(s: &Session, path: &str) -> (String, String) {
    let (repo, rest) = rain_core::paths::split_repo_path(path);
    if s.repos.len() > 1 && !repo.is_empty() && s.repos.iter().any(|r| r.id == repo) {
        (repo.to_owned(), rest.to_owned())
    } else {
        (String::new(), path.to_owned())
    }
}

/// The id of the session's repo when it has exactly one.
pub fn sole_repo(s: &Session) -> Option<&str> {
    match s.repos.as_slice() {
        [only] => Some(&only.id),
        _ => None,
    }
}

/// Whether two thread repos are the same one: in a session of one repo (`sole`), a thread's repo
/// is that repo's id or left empty.
pub fn same_repo(sole: Option<&str>, a: &str, b: &str) -> bool {
    a == b || sole.is_some_and(|id| [a, b].iter().all(|r| r.is_empty() || *r == id))
}

fn thread_mut<'a>(s: &'a mut Session, id: &str) -> Option<&'a mut Thread> {
    s.threads.iter_mut().find(|t| t.id == id)
}

fn is_agent(author: Option<&str>) -> bool {
    author.map(str::trim).is_some_and(|a| !a.is_empty())
}

/// An agent's reply to a question, or to any thread whose answer is pending (an `@claude` reply
/// on a comment), marks it answered by that message.
pub(crate) fn record_answer(t: &mut Thread, message_id: &str, author: Option<&str>, at: &str) {
    let asked = t.answer.as_ref().is_some_and(|a| a.status.pending());
    if (t.kind() != ThreadKind::Question && !asked) || !is_agent(author) {
        return;
    }
    let a = t.answer.get_or_insert_with(Answer::default);
    if a.status == AnswerStatus::Answered && a.message_id.is_some() {
        return;
    }
    a.status = AnswerStatus::Answered;
    // The launcher's own reply re-marks it `launcher` once its run ends.
    a.by = Some("attached".to_owned());
    a.ended_at = Some(at.to_owned());
    a.message_id = Some(message_id.to_owned());
    a.error = None;
}

/// Whether the attached agent holds `t`'s pending question.
pub fn claimed(t: &Thread) -> bool {
    t.answer
        .as_ref()
        .is_some_and(|a| a.status == AnswerStatus::Answering && a.by.as_deref() == Some("attached"))
}

/// The attached agent takes `id`'s pending question, or with `release` gives it back to the
/// queue; `Some(true)` when that changed it, `None` for an unknown thread.
pub fn claim(s: &mut Session, id: &str, release: bool) -> Option<bool> {
    let t = thread_mut(s, id)?;
    if t.answer.as_ref().is_none_or(|a| !a.status.pending()) || claimed(t) != release {
        return Some(false);
    }
    let a = t.answer.get_or_insert_with(Answer::default);
    if release {
        *a = Answer::default();
    } else {
        a.status = AnswerStatus::Answering;
        a.by = Some("attached".to_owned());
        a.started_at = Some(now_iso());
        a.ended_at = None;
        a.error = None;
    }
    s.bump_comments();
    Some(true)
}

/// Appends a reply; returns the new message's id, or `None` for an unknown thread.
pub fn reply(s: &mut Session, thread_id: &str, body: &str, author: Option<&str>) -> Option<String> {
    let t = thread_mut(s, thread_id)?;
    let id = new_id();
    let now = now_iso();
    t.messages.push(Message {
        id: id.clone(),
        body: body.to_owned(),
        author: author.map(str::to_owned),
        created_at: now.clone(),
        updated_at: now.clone(),
    });
    record_answer(t, &id, author, &now);
    t.updated_at = now;
    s.bump_comments();
    Some(id)
}

pub fn edit_message(s: &mut Session, thread_id: &str, message_id: &str, body: &str) -> bool {
    let Some(t) = thread_mut(s, thread_id) else { return false };
    let Some(m) = t.messages.iter_mut().find(|m| m.id == message_id) else {
        return false;
    };
    if m.body == body {
        return false;
    }
    let now = now_iso();
    m.body = body.to_owned();
    m.updated_at = now.clone();
    t.updated_at = now;
    s.bump_comments();
    true
}

/// Removes one reply; the first message can't go alone (resolve the thread instead).
pub fn delete_message(s: &mut Session, thread_id: &str, message_id: &str) -> bool {
    let Some(t) = thread_mut(s, thread_id) else { return false };
    let Some(i) = t.messages.iter().position(|m| m.id == message_id) else {
        return false;
    };
    if i == 0 {
        return false;
    }
    let gone = t.messages.remove(i);
    if let Some(a) = t.answer.as_mut().filter(|a| a.message_id.as_deref() == Some(gone.id.as_str())) {
        match t.messages[1..].iter().rev().find(|m| is_agent(m.author.as_deref())) {
            Some(m) => a.message_id = Some(m.id.clone()),
            None => {
                a.status = AnswerStatus::Cancelled;
                a.message_id = None;
            }
        }
    }
    t.updated_at = now_iso();
    s.bump_comments();
    true
}

/// Removes a thread and its history. Rain's own actions resolve instead ([`resolve`]).
pub fn delete_thread(s: &mut Session, thread_id: &str) -> bool {
    let before = s.threads.len();
    s.threads.retain(|t| t.id != thread_id);
    let changed = s.threads.len() != before;
    if changed {
        s.bump_comments();
    }
    changed
}

/// Puts back a thread [`delete_thread`] removed, at index `at` (or last); nothing when a thread
/// with its id is already there. Whether anything changed.
pub fn restore_thread(s: &mut Session, t: Thread, at: usize) -> bool {
    if s.threads.iter().any(|x| x.id == t.id) {
        return false;
    }
    let at = at.min(s.threads.len());
    s.threads.insert(at, t);
    s.bump_comments();
    true
}

/// Moves a thread to `state`, keeping its messages. `sha` names the revision the change refers
/// to (`addressed in abc123`); `reason` is kept for `dismissed` only. Whether anything changed.
pub fn set_state(s: &mut Session, thread_id: &str, state: ThreadState, sha: Option<&str>, reason: Option<DismissReason>) -> bool {
    let Some(t) = thread_mut(s, thread_id) else { return false };
    let reason = if state == ThreadState::Dismissed {
        reason.or(Some(DismissReason::Other))
    } else {
        None
    };
    let sha = sha
        .map(str::to_owned)
        .or_else(|| (state == t.state).then(|| t.state_sha.clone()).flatten());
    if t.state == state && t.dismiss_reason == reason && t.state_sha == sha {
        return false;
    }
    let now = now_iso();
    t.state = state;
    t.state_sha = sha;
    t.dismiss_reason = reason;
    t.state_at = Some(now.clone());
    t.updated_at = now;
    s.bump_comments();
    true
}

/// `DELETE /api/comments/:id` and the card's Resolve: the thread leaves default readback but
/// keeps its history.
pub fn resolve(s: &mut Session, thread_id: &str) -> bool {
    set_state(s, thread_id, ThreadState::Resolved, None, None)
}

pub fn reopen(s: &mut Session, thread_id: &str) -> bool {
    set_state(s, thread_id, ThreadState::Open, None, None)
}

pub fn dismiss(s: &mut Session, thread_id: &str, reason: DismissReason) -> bool {
    set_state(s, thread_id, ThreadState::Dismissed, None, Some(reason))
}

/// The user agrees the thread was dealt with: state `addressed` (keeping the agent's sha) and
/// disposition `agree`, so it stops waiting on them.
pub fn confirm_addressed(s: &mut Session, thread_id: &str) -> bool {
    let sha = thread(s, thread_id).and_then(|t| t.state_sha.clone());
    let a = set_state(s, thread_id, ThreadState::Addressed, sha.as_deref(), None);
    let b = set_disposition(s, thread_id, Some(Disposition::Agree));
    a || b
}

/// The ladder's "addressed?" suggestion: still open, its code changed under it, and the agent
/// has replied since. Never applied without the user.
pub fn addressed_suggested(t: &Thread) -> bool {
    t.is_open()
        && t.placement() == AnchorState::Outdated
        && t.messages.len() > 1
        && t.messages.last().is_some_and(|m| is_agent(m.author.as_deref()))
}

pub fn set_disposition(s: &mut Session, thread_id: &str, d: Option<Disposition>) -> bool {
    let Some(t) = thread_mut(s, thread_id) else { return false };
    if t.disposition == d {
        return false;
    }
    t.disposition = d;
    t.updated_at = now_iso();
    s.bump_comments();
    true
}

/// `d`, or clears it when the thread already has `d` (a key pressed twice undoes itself).
pub fn toggle_disposition(s: &mut Session, thread_id: &str, d: Disposition) -> bool {
    let next = match thread(s, thread_id) {
        Some(t) if t.disposition == Some(d) => None,
        Some(_) => Some(d),
        None => return false,
    };
    set_disposition(s, thread_id, next)
}

pub fn set_viewed(s: &mut Session, repo: &str, file_path: &str, hash: &str) -> bool {
    let now = now_iso();
    match s.viewed.iter_mut().find(|v| v.repo == repo && v.file_path == file_path) {
        Some(v) if v.diff_content_hash == hash => return false,
        Some(v) => {
            v.diff_content_hash = hash.to_owned();
            v.viewed_at = now;
        }
        None => s.viewed.push(ViewedRecord {
            repo: repo.to_owned(),
            file_path: file_path.to_owned(),
            viewed_at: now,
            diff_content_hash: hash.to_owned(),
            ..ViewedRecord::default()
        }),
    }
    s.version += 1;
    true
}

pub fn clear_viewed(s: &mut Session, repo: &str, file_path: &str) -> bool {
    let before = s.viewed.len();
    s.viewed.retain(|v| !(v.repo == repo && v.file_path == file_path));
    let changed = s.viewed.len() != before;
    if changed {
        s.version += 1;
    }
    changed
}

/// Viewed, and the diff hasn't changed since (a changed file reads as unviewed, as on GitHub).
pub fn is_viewed(s: &Session, repo: &str, file_path: &str, current_hash: &str) -> bool {
    s.viewed
        .iter()
        .any(|v| v.repo == repo && v.file_path == file_path && v.diff_content_hash == current_hash)
}

/// Keeps one draft per anchor; empty text removes it.
pub fn set_draft(s: &mut Session, repo: &str, file_path: &str, anchor: Anchor, text: &str) -> bool {
    let pos = s
        .drafts
        .iter()
        .position(|d| d.repo == repo && d.file_path == file_path && d.anchor == anchor);
    match (pos, text.trim().is_empty()) {
        (Some(i), true) => {
            s.drafts.remove(i);
        }
        (None, true) => return false,
        (Some(i), false) if s.drafts[i].text == text => return false,
        (Some(i), false) => s.drafts[i].text = text.to_owned(),
        (None, false) => s.drafts.push(Draft {
            repo: repo.to_owned(),
            file_path: file_path.to_owned(),
            anchor,
            text: text.to_owned(),
            ..Draft::default()
        }),
    }
    s.version += 1;
    true
}

pub fn take_draft(s: &mut Session, repo: &str, file_path: &str, anchor: Anchor) -> Option<String> {
    let i = s
        .drafts
        .iter()
        .position(|d| d.repo == repo && d.file_path == file_path && d.anchor == anchor)?;
    s.version += 1;
    Some(s.drafts.remove(i).text)
}

/// The text an anchor covers in `file`, from the diff's own lines: whole lines joined by `\n`
/// for line and range anchors, the selected characters for a span. `None` when any covered line
/// is outside the diff's hunks.
pub fn anchor_text(file: &FileDiff, anchor: &Anchor) -> Option<String> {
    let side = anchor.side();
    let (first, last) = anchor.lines();
    if first == 0 || last < first || last - first > 100_000 {
        return None;
    }
    let lines: Vec<&str> = (first..=last)
        .map(|n| line_index(file, side, n).map(|i| file.line_text(i)))
        .collect::<Option<_>>()?;
    match *anchor {
        Anchor::Span { start_col, end_col, .. } => {
            let n = lines.len();
            let mut out = String::new();
            for (i, line) in lines.iter().enumerate() {
                let from = if i == 0 { start_col as usize } else { 0 };
                let to = if i + 1 == n { end_col as usize } else { usize::MAX };
                if i > 0 {
                    out.push('\n');
                }
                out.extend(line.chars().skip(from).take(to.saturating_sub(from)));
            }
            Some(out)
        }
        _ => Some(lines.join("\n")),
    }
}

/// Records the loaded commit a just-posted line thread's snapshot was taken against, so a later
/// load can map its lines through git and name the sha it was last seen at.
pub fn stamp_commit(s: &mut Session, id: &str) {
    let Some(t) = s.threads.iter().find(|t| t.id == id) else { return };
    if t.commit.is_some() || t.code_snapshot.is_none() || t.file_path.starts_with('@') {
        return;
    }
    let repo = s
        .repos
        .iter()
        .find(|r| r.id == t.repo)
        .or_else(|| (s.repos.len() == 1 && t.repo.is_empty()).then(|| &s.repos[0]));
    let sha = repo.map(|r| match t.anchor.side() {
        Side::New => r.target.clone(),
        Side::Old => r.base.clone(),
    });
    let Some(sha) = sha.filter(|h| h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit())) else {
        return;
    };
    if let Some(t) = s.threads.iter_mut().find(|t| t.id == id) {
        t.commit = Some(sha);
    }
}

const CONTEXT: u32 = 2;

fn line_at(file: &FileDiff, side: Side, n: u32) -> Option<&str> {
    (n > 0).then(|| line_index(file, side, n)).flatten().map(|i| file.line_text(i))
}

/// The anchored text of `file` as a post-time snapshot: the text, where it sat, and up to two
/// shown lines above and below for ranking candidates when it moves later.
pub fn snapshot_at(file: &FileDiff, anchor: &Anchor, path: &str) -> Option<CodeSnapshot> {
    let content = normalize_eol(&anchor_text(file, anchor)?);
    let side = anchor.side();
    let (first, last) = anchor.lines();
    let mut before: Vec<String> = (1..=CONTEXT)
        .map_while(|k| first.checked_sub(k).and_then(|n| line_at(file, side, n)).map(str::to_owned))
        .collect();
    before.reverse();
    let after = (1..=CONTEXT)
        .map_while(|k| line_at(file, side, last + k).map(str::to_owned))
        .collect();
    Some(CodeSnapshot {
        content,
        path: Some(path.to_owned()),
        lines: Some((first, last)),
        side: Some(side),
        before,
        after,
        ..CodeSnapshot::default()
    })
}

fn side_number(rec: &rain_core::LineRec, side: Side) -> u32 {
    match side {
        Side::Old => rec.old,
        Side::New => rec.new,
    }
}

/// Index into `file.lines` of line `n` on `side`, when a hunk shows it.
pub fn line_index(file: &FileDiff, side: Side, n: u32) -> Option<u32> {
    let span = |h: &rain_core::Hunk| match side {
        Side::Old => (h.old_start, h.old_count),
        Side::New => (h.new_start, h.new_count),
    };
    let h = &file.hunks[file.hunks.partition_point(|h| {
        let (start, count) = span(h);
        start + count <= n
    })..]
        .first()?;
    let (mut lo, mut hi) = (h.lines.start, h.lines.end);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let Some(j) = (mid..hi).find(|&j| side_number(&file.lines[j as usize], side) != 0) else {
            hi = mid;
            continue;
        };
        match side_number(&file.lines[j as usize], side).cmp(&n) {
            std::cmp::Ordering::Equal => return Some(j),
            std::cmp::Ordering::Less => lo = j + 1,
            std::cmp::Ordering::Greater => hi = mid,
        }
    }
    None
}

/// The anchor moved by `delta` lines, keeping a span's columns.
fn shifted(a: Anchor, delta: i64) -> Option<Anchor> {
    let mv = |n: u32| u32::try_from(i64::from(n) + delta).ok().filter(|&n| n > 0);
    Some(match a {
        Anchor::Line { side, line } => Anchor::Line { side, line: mv(line)? },
        Anchor::Range { side, start, end } => Anchor::Range {
            side,
            start: mv(start)?,
            end: mv(end)?,
        },
        Anchor::Span {
            side,
            start_line,
            start_col,
            end_line,
            end_col,
        } => Anchor::Span {
            side,
            start_line: mv(start_line)?,
            start_col,
            end_line: mv(end_line)?,
            end_col,
        },
    })
}

/// Where line `n` of the anchoring revision is now, through the `-U0` hunk headers
/// `[from_start, from_count, to_start, to_count]` between the two; `None` when a hunk changed it.
pub fn map_line(hunks: &[[u32; 4]], n: u32) -> Option<u32> {
    let mut delta: i64 = 0;
    for &[a, b, _, d] in hunks {
        if b == 0 {
            if a < n {
                delta += i64::from(d);
                continue;
            }
            break;
        }
        if n < a {
            break;
        }
        if n < a + b {
            return None;
        }
        delta += i64::from(d) - i64::from(b);
    }
    u32::try_from(i64::from(n) + delta).ok()
}

/// Line `n` sits in a hunk that removed it without replacing anything.
fn map_deleted(hunks: &[[u32; 4]], n: u32) -> bool {
    hunks.iter().any(|&[a, b, _, d]| b > 0 && d == 0 && (a..a + b).contains(&n))
}

/// Every place on the anchor's side whose text equals `snap`, in file order.
fn candidates(file: &FileDiff, anchor: Anchor, snap: &str) -> Vec<Anchor> {
    let side = anchor.side();
    let first = anchor.lines().0;
    let head = snap.split('\n').next().unwrap_or("");
    let is_span = matches!(anchor, Anchor::Span { .. });
    file.lines
        .iter()
        .enumerate()
        .filter(|(i, rec)| {
            side_number(rec, side) != 0 && {
                let t = file.line_text(*i as u32);
                if is_span { t.contains(head) } else { t == head }
            }
        })
        .filter_map(|(_, rec)| {
            let a = shifted(anchor, i64::from(side_number(rec, side)) - i64::from(first))?;
            anchor_text(file, &a).filter(|t| normalize_eol(t) == snap).map(|_| a)
        })
        .collect()
}

/// How many of the snapshot's neighbour lines sit next to `a` in `file`.
fn neighbours(file: &FileDiff, a: &Anchor, snap: &CodeSnapshot) -> usize {
    let side = a.side();
    let (first, last) = a.lines();
    let n = snap.before.len() as u32;
    let above = snap
        .before
        .iter()
        .enumerate()
        .filter(|(i, want)| {
            (first + *i as u32)
                .checked_sub(n)
                .and_then(|l| line_at(file, side, l))
                .is_some_and(|got| got == want.as_str())
        })
        .count();
    let below = snap
        .after
        .iter()
        .enumerate()
        .filter(|(i, want)| line_at(file, side, last + 1 + *i as u32).is_some_and(|got| got == want.as_str()))
        .count();
    above + below
}

enum Pick {
    None,
    One(Anchor),
    Ambiguous,
}

/// The candidate whose neighbours match best; without neighbours to compare, the nearest. Equal
/// best neighbour matches are ambiguous rather than guessed at.
fn pick(file: &FileDiff, anchor: Anchor, snap: &CodeSnapshot, text: &str) -> Pick {
    let found = candidates(file, anchor, text);
    let first = anchor.lines().0;
    if snap.before.is_empty() && snap.after.is_empty() {
        return found
            .into_iter()
            .min_by_key(|a| a.lines().0.abs_diff(first))
            .map_or(Pick::None, Pick::One);
    }
    let scored: Vec<(usize, Anchor)> = found.into_iter().map(|a| (neighbours(file, &a, snap), a)).collect();
    let Some(best) = scored.iter().map(|(n, _)| *n).max() else {
        return Pick::None;
    };
    let mut top = scored.into_iter().filter(|(n, _)| *n == best);
    match (top.next(), top.next()) {
        (Some((_, a)), None) => Pick::One(a),
        _ => Pick::Ambiguous,
    }
}

/// The snapshot's lines appear as removed lines of `file`: the code under a new-side anchor was
/// deleted.
fn removed_in(file: &FileDiff, text: &str) -> bool {
    let want: Vec<&str> = text.split('\n').collect();
    let n = want.len();
    file.lines.windows(n).enumerate().any(|(i, w)| {
        w.iter().all(|r| r.kind == LineKind::Removed) && want.iter().enumerate().all(|(k, line)| file.line_text((i + k) as u32) == *line)
    })
}

fn normalize_eol(s: &str) -> String {
    s.replace("\r\n", "\n").trim_end_matches(['\n', '\r']).to_owned()
}

/// Something anchored to code: a thread or a line note, as the ladder reads it.
struct Spot<'a> {
    repo: &'a str,
    path: &'a str,
    anchor: Anchor,
    snapshot: Option<&'a CodeSnapshot>,
    commit: Option<&'a str>,
    state: AnchorState,
}

#[derive(Debug, PartialEq)]
enum Step {
    Keep,
    /// First sight of its lines: take the snapshot.
    Snapshot(CodeSnapshot, Option<String>),
    Placed(Option<String>),
    Moved {
        anchor: Anchor,
        how: AnchorHow,
        commit: Option<String>,
        path: Option<String>,
    },
    Outdated,
    Orphaned(OrphanReason),
}

enum Lookup<'d> {
    Found(&'d FileDiff),
    Renamed(&'d FileDiff),
    Missing,
    Unsure,
}

fn in_repo(diff: &DiffSet, f: &FileDiff, repo: &str) -> bool {
    repo.is_empty() || diff.repos.get(f.repo as usize).is_none_or(|r| r.id == repo)
}

fn lookup<'d>(diff: &'d DiffSet, repo: &str, path: &str) -> Lookup<'d> {
    let mut hits = diff.files.iter().filter(|f| f.path == path && in_repo(diff, f, repo));
    match (hits.next(), hits.next()) {
        (Some(_), Some(_)) if repo.is_empty() => return Lookup::Unsure,
        (Some(f), _) => return Lookup::Found(f),
        _ => {}
    }
    let mut renamed = diff
        .files
        .iter()
        .filter(|f| f.old_path.as_deref() == Some(path) && f.path != path && in_repo(diff, f, repo));
    match (renamed.next(), renamed.next()) {
        (Some(_), Some(_)) if repo.is_empty() => Lookup::Unsure,
        (Some(f), _) => Lookup::Renamed(f),
        _ => Lookup::Missing,
    }
}

/// A line map is worth asking for: the spot's text left its lines and its revision moved.
fn wants_map<'d>(diff: &'d DiffSet, sp: &Spot, rev_of: &impl Fn(usize, Side) -> Option<String>) -> Option<(&'d FileDiff, String, String)> {
    let Lookup::Found(f) = lookup(diff, sp.repo, sp.path) else {
        return None;
    };
    let snap = normalize_eol(&sp.snapshot?.content);
    if anchor_text(f, &sp.anchor).is_some_and(|x| normalize_eol(&x) == snap) {
        return None;
    }
    let from = sp.commit?;
    let to = rev_of(f.repo as usize, sp.anchor.side())?;
    (from != to).then(|| (f, from.to_owned(), to))
}

/// The re-anchor ladder for one spot: same text in place → same text found through the line
/// map or among candidates ranked by neighbours → the renamed file → outdated when the
/// neighbours still match → orphaned with the reason.
fn ladder(diff: &DiffSet, sp: &Spot, rev_of: &impl Fn(usize, Side) -> Option<String>, map: Option<&[[u32; 4]]>) -> Step {
    let (f, renamed) = match lookup(diff, sp.repo, sp.path) {
        Lookup::Unsure => return Step::Keep,
        Lookup::Missing if sp.state == AnchorState::Orphaned => return Step::Keep,
        Lookup::Missing => return Step::Orphaned(OrphanReason::LeftDiff),
        Lookup::Found(f) => (f, false),
        Lookup::Renamed(f) => (f, true),
    };
    let side = sp.anchor.side();
    if f.status == FileStatus::Deleted && side == Side::New {
        return Step::Orphaned(OrphanReason::FileDeleted);
    }
    let now = rev_of(f.repo as usize, side);
    let new_path = renamed.then(|| f.path.clone());
    let here = anchor_text(f, &sp.anchor).map(|x| normalize_eol(&x));
    let Some(snap) = sp.snapshot else {
        return snapshot_at(f, &sp.anchor, &f.path).map_or(Step::Keep, |s| Step::Snapshot(s, now));
    };
    let text = normalize_eol(&snap.content);
    if here.as_deref() == Some(text.as_str()) {
        return if renamed {
            Step::Moved {
                anchor: sp.anchor,
                how: AnchorHow::Renamed,
                commit: now,
                path: new_path,
            }
        } else {
            Step::Placed(now)
        };
    }
    let how = if renamed { AnchorHow::Renamed } else { AnchorHow::Exact };
    let mapped = map.and_then(|m| {
        let (a, b) = sp.anchor.lines();
        let delta = i64::from(map_line(m, a)?) - i64::from(a);
        let moved = shifted(sp.anchor, delta)?;
        (map_line(m, b) == Some(moved.lines().1)).then_some(moved)
    });
    if let Some(a) = mapped
        && anchor_text(f, &a).is_none_or(|x| normalize_eol(&x) == text)
    {
        return Step::Moved {
            anchor: a,
            how,
            commit: now,
            path: new_path,
        };
    }
    match pick(f, sp.anchor, snap, &text) {
        Pick::One(a) => {
            return Step::Moved {
                anchor: a,
                how,
                commit: now,
                path: new_path,
            };
        }
        Pick::Ambiguous => return Step::Orphaned(OrphanReason::Ambiguous),
        Pick::None => {}
    }
    if renamed {
        return Step::Orphaned(OrphanReason::FileRenamed);
    }
    let deleted = (side == Side::New && removed_in(f, &text)) || map.is_some_and(|m| map_deleted(m, sp.anchor.lines().0));
    if deleted {
        return Step::Orphaned(OrphanReason::LineDeleted);
    }
    if here.is_none() {
        return Step::Keep;
    }
    let ctx = snap.before.len() + snap.after.len();
    if ctx == 0 || neighbours(f, &sp.anchor, snap) * 2 >= ctx {
        Step::Outdated
    } else {
        Step::Orphaned(OrphanReason::ContentRewritten)
    }
}

/// The orphan record: the post-time snapshot plus where it last sat.
fn orphan_of(snapshot: Option<&CodeSnapshot>, path: &str, anchor: &Anchor, sha: Option<&str>, reason: OrphanReason) -> Orphan {
    let mut snap = snapshot.cloned().unwrap_or_default();
    snap.path.get_or_insert_with(|| path.to_owned());
    snap.lines.get_or_insert(anchor.lines());
    snap.side.get_or_insert(anchor.side());
    Orphan {
        reason,
        last_seen_sha: sha.map(str::to_owned),
        snapshot: snap,
        at: now_iso(),
    }
}

type Fingerprint = (
    Anchor,
    bool,
    AnchorState,
    Option<AnchorHow>,
    bool,
    Option<String>,
    Option<OrphanReason>,
    String,
);

fn fingerprint(t: &Thread) -> Fingerprint {
    (
        t.anchor,
        t.outdated,
        t.anchor_state,
        t.anchor_how,
        t.code_snapshot.is_some(),
        t.commit.clone(),
        t.orphan.as_ref().map(|o| o.reason),
        t.file_path.clone(),
    )
}

fn apply_step(t: &mut Thread, step: Step) {
    match step {
        Step::Keep => {}
        Step::Snapshot(snap, commit) => {
            if let Some(p) = snap.path.clone().filter(|p| *p != t.file_path) {
                t.original.get_or_insert(t.anchor);
                t.file_path = p;
                t.set_anchor_state(AnchorState::Moved, Some(AnchorHow::Renamed));
            } else if t.anchor_state != AnchorState::Moved {
                t.set_anchor_state(AnchorState::Placed, None);
            } else {
                t.outdated = false;
                t.orphan = None;
            }
            t.code_snapshot = Some(snap);
            t.commit = commit;
        }
        Step::Placed(commit) => {
            match t.anchor_state {
                AnchorState::Moved => {
                    t.outdated = false;
                }
                _ => t.set_anchor_state(AnchorState::Placed, None),
            }
            if t.commit.is_none() {
                t.commit = commit;
            }
        }
        Step::Moved { anchor, how, commit, path } => {
            if anchor != t.anchor {
                t.original.get_or_insert(t.anchor);
                t.anchor = anchor;
            }
            if let Some(p) = path {
                t.original.get_or_insert(t.anchor);
                t.file_path = p;
            }
            t.set_anchor_state(AnchorState::Moved, Some(how));
            t.commit = commit;
        }
        Step::Outdated => t.set_anchor_state(AnchorState::Outdated, None),
        Step::Orphaned(reason) => {
            if t.anchor_state == AnchorState::Orphaned && t.orphan.as_ref().is_some_and(|o| o.reason == reason) {
                return;
            }
            let orphan = orphan_of(t.code_snapshot.as_ref(), &t.file_path, &t.anchor, t.commit.as_deref(), reason);
            t.set_anchor_state(AnchorState::Orphaned, None);
            t.orphan = Some(orphan);
        }
    }
}

fn spot(t: &Thread) -> Spot<'_> {
    Spot {
        repo: &t.repo,
        path: &t.file_path,
        anchor: t.anchor,
        snapshot: t.code_snapshot.as_ref(),
        commit: t.commit.as_deref(),
        state: t.anchor_state,
    }
}

/// Threads the code ladder runs on: line threads on real files.
fn on_code(t: &Thread) -> bool {
    !t.is_artifact() && matches!(t.target_kind(), TargetKind::Line)
}

/// File-level threads only need their file: present → placed, gone → orphaned.
fn file_level(diff: &DiffSet, t: &Thread) -> Step {
    match lookup(diff, &t.repo, &t.file_path) {
        Lookup::Found(_) if t.anchor_state == AnchorState::Orphaned => Step::Placed(None),
        Lookup::Renamed(f) => Step::Moved {
            anchor: t.anchor,
            how: AnchorHow::Renamed,
            commit: t.commit.clone(),
            path: Some(f.path.clone()),
        },
        Lookup::Missing => Step::Orphaned(OrphanReason::LeftDiff),
        _ => Step::Keep,
    }
}

/// Flags threads whose snapshot no longer matches the diff: see [`reanchor`] without revisions
/// to map through.
pub fn mark_outdated(s: &mut Session, diff: &DiffSet) -> usize {
    reanchor(s, diff, |_, _| None, |_, _, _| None)
}

/// Keeps threads on their code as the diff changes, one `line_map(thread, from, to)` call per
/// thread that needs one; see [`reanchor_batched`].
pub fn reanchor(
    s: &mut Session,
    diff: &DiffSet,
    rev_of: impl Fn(usize, Side) -> Option<String>,
    line_map: impl Fn(&Thread, &str, &str) -> Option<Vec<[u32; 4]>>,
) -> usize {
    let maps: HashMap<usize, Vec<[u32; 4]>> = s
        .threads
        .iter()
        .enumerate()
        .filter(|(_, t)| on_code(t))
        .filter_map(|(i, t)| {
            let (_, from, to) = wants_map(diff, &spot(t), &rev_of)?;
            Some((i, line_map(t, &from, &to)?))
        })
        .collect();
    run_ladder(s, diff, &rev_of, &maps)
}

/// The re-anchor ladder with the `-U0` line maps fetched once per `(repo, side, from, to)`;
/// `line_maps` returns one map per path, in order. Returns how many threads changed.
pub fn reanchor_batched(
    s: &mut Session,
    diff: &DiffSet,
    rev_of: impl Fn(usize, Side) -> Option<String>,
    line_maps: impl Fn(usize, Side, &str, &str, &[&str]) -> Vec<Option<Vec<[u32; 4]>>>,
) -> usize {
    type MapKey = (usize, Side, String, String);
    let mut groups: HashMap<MapKey, Vec<(usize, &str)>> = HashMap::new();
    for (i, t) in s.threads.iter().enumerate().filter(|(_, t)| on_code(t)) {
        if let Some((f, from, to)) = wants_map(diff, &spot(t), &rev_of) {
            groups
                .entry((f.repo as usize, t.anchor.side(), from, to))
                .or_default()
                .push((i, t.file_path.as_str()));
        }
    }
    let mut maps = HashMap::new();
    for ((repo, side, from, to), members) in groups {
        let mut paths: Vec<&str> = members.iter().map(|(_, p)| *p).collect();
        paths.sort_unstable();
        paths.dedup();
        let got = line_maps(repo, side, &from, &to, &paths);
        for (i, p) in members {
            if let Some(Some(m)) = paths.iter().position(|x| *x == p).and_then(|k| got.get(k)) {
                maps.insert(i, m.clone());
            }
        }
    }
    run_ladder(s, diff, &rev_of, &maps)
}

fn run_ladder(
    s: &mut Session,
    diff: &DiffSet,
    rev_of: &impl Fn(usize, Side) -> Option<String>,
    maps: &HashMap<usize, Vec<[u32; 4]>>,
) -> usize {
    let mut changed = 0;
    for (i, t) in s.threads.iter_mut().enumerate() {
        let step = if on_code(t) {
            ladder(diff, &spot(t), rev_of, maps.get(&i).map(Vec::as_slice))
        } else if t.target_kind() == TargetKind::File && !t.is_artifact() {
            file_level(diff, t)
        } else {
            continue;
        };
        let before = fingerprint(t);
        apply_step(t, step);
        if fingerprint(t) != before {
            changed += 1;
        }
    }
    if changed > 0 {
        s.bump_comments();
    }
    changed
}

/// Copies the ladder's anchor results (`done`, run on a copy taken as `base`) into `live`, for
/// every thread whose anchor nobody changed meanwhile; messages and state stay as they are now.
pub fn merge_anchors(live: &mut Session, base: &[Thread], done: &[Thread]) -> usize {
    let mut merged = 0;
    for d in done {
        let Some(b) = base.iter().find(|b| b.id == d.id) else { continue };
        let Some(l) = live.threads.iter_mut().find(|l| l.id == d.id) else {
            continue;
        };
        if fingerprint(d) == fingerprint(b) || fingerprint(l) != fingerprint(b) {
            continue;
        }
        l.anchor = d.anchor;
        l.original = d.original;
        l.file_path.clone_from(&d.file_path);
        l.code_snapshot.clone_from(&d.code_snapshot);
        l.commit.clone_from(&d.commit);
        l.outdated = d.outdated;
        l.anchor_state = d.anchor_state;
        l.anchor_how = d.anchor_how;
        l.orphan.clone_from(&d.orphan);
        merged += 1;
    }
    merged
}

/// Line notes through the same ladder as threads (no line maps): a note gets its snapshot the
/// first time its lines show, then moves, turns outdated or is orphaned with a reason. Overview
/// notes are left to the overview. Returns how many changed; bumps the notes version when any did.
pub fn reanchor_notes(s: &mut Session, diff: &DiffSet, rev_of: impl Fn(usize, Side) -> Option<String>) -> usize {
    let mut changed = 0;
    for n in &mut s.notes {
        let (NoteKind::Line, Some(anchor), false) = (n.kind, n.anchor, n.is_overview()) else {
            continue;
        };
        let sp = Spot {
            repo: &n.repo,
            path: &n.file_path,
            anchor,
            snapshot: n.code_snapshot.as_ref(),
            commit: n.commit.as_deref(),
            state: n.anchor_state,
        };
        let step = ladder(diff, &sp, &rev_of, None);
        let before = n.clone();
        apply_note_step(n, step);
        if *n != before {
            changed += 1;
        }
    }
    if changed > 0 {
        s.bump_notes();
    }
    changed
}

fn set_note_state(n: &mut Note, state: AnchorState, how: Option<AnchorHow>) {
    n.anchor_state = state;
    n.anchor_how = how;
    if state != AnchorState::Orphaned {
        n.orphan = None;
    }
}

fn apply_note_step(n: &mut Note, step: Step) {
    match step {
        Step::Keep => {}
        Step::Snapshot(snap, commit) => {
            if let Some(p) = snap.path.clone().filter(|p| *p != n.file_path) {
                n.file_path = p;
                set_note_state(n, AnchorState::Moved, Some(AnchorHow::Renamed));
            } else if n.anchor_state != AnchorState::Moved {
                set_note_state(n, AnchorState::Placed, None);
            }
            n.code_snapshot = Some(snap);
            n.commit = commit;
        }
        Step::Placed(commit) => {
            if n.anchor_state != AnchorState::Moved {
                set_note_state(n, AnchorState::Placed, None);
            }
            if n.commit.is_none() {
                n.commit = commit;
            }
        }
        Step::Moved { anchor, how, commit, path } => {
            n.anchor = Some(anchor);
            if let Some(p) = path {
                n.file_path = p;
            }
            set_note_state(n, AnchorState::Moved, Some(how));
            n.commit = commit;
        }
        Step::Outdated => set_note_state(n, AnchorState::Outdated, None),
        Step::Orphaned(reason) => {
            if n.anchor_state == AnchorState::Orphaned && n.orphan.as_ref().is_some_and(|o| o.reason == reason) {
                return;
            }
            let Some(anchor) = n.anchor else { return };
            n.orphan = Some(orphan_of(
                n.code_snapshot.as_ref(),
                &n.file_path,
                &anchor,
                n.commit.as_deref(),
                reason,
            ));
            n.anchor_state = AnchorState::Orphaned;
            n.anchor_how = None;
        }
    }
}

/// A brief cite that no longer points into the loaded diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaleCite {
    /// The section (or tour step) the cite sits in.
    pub section: String,
    pub cite: Cite,
    /// `file not in the diff` or `lines outside the diff`.
    pub reason: &'static str,
}

/// Brief cites checked against the loaded diff: a cite whose file left the diff, or whose lines
/// no hunk shows any more, is stale. Cite paths are `<repo>/<path>` keys (or bare paths).
pub fn reanchor_brief(doc: &BriefDoc, diff: &DiffSet) -> Vec<StaleCite> {
    doc.cites()
        .into_iter()
        .filter_map(|(section, c)| {
            let file = diff
                .files
                .iter()
                .find(|f| f.display_path(&diff.repos) == c.path || f.path == c.path);
            let reason = match (file, c.lines) {
                (None, _) => "file not in the diff",
                (Some(f), Some((a, b))) => {
                    let side = c.side.unwrap_or(Side::New);
                    let (a, b) = (u64::from(a), u64::from(b.max(a)));
                    let shown = f.hunks.iter().any(|h| {
                        let (start, count) = match side {
                            Side::Old => (h.old_start, h.old_count),
                            Side::New => (h.new_start, h.new_count),
                        };
                        let start = u64::from(start);
                        count > 0 && start <= b && start + u64::from(count) > a
                    });
                    if shown {
                        return None;
                    }
                    "lines outside the diff"
                }
                (Some(_), None) => return None,
            };
            Some(StaleCite {
                section: section.to_owned(),
                cite: c.clone(),
                reason,
            })
        })
        .collect()
}

/// A Brief section as a thread snapshot: its title, then its text.
pub fn brief_snapshot(doc: &BriefDoc, section: &str) -> Option<CodeSnapshot> {
    let s = doc.section(section)?;
    Some(CodeSnapshot {
        content: format!("{}\n{}", s.title, s.text).trim_end().to_owned(),
        path: Some(BRIEF_PATH.to_owned()),
        ..CodeSnapshot::default()
    })
}

/// A Map node as a thread snapshot: its label, kind and files, one per line.
pub fn node_snapshot(graph: &OverviewGraph, node: &str) -> Option<CodeSnapshot> {
    let n = graph.nodes.iter().find(|n| n.id == node)?;
    let mut content = format!("{}\n{}", n.label, n.kind);
    for f in &n.files {
        content.push('\n');
        content.push_str(&f.path);
    }
    Some(CodeSnapshot {
        content,
        path: Some(MAP_PATH.to_owned()),
        ..CodeSnapshot::default()
    })
}

/// A note as a thread snapshot: its body, with where it sat.
pub fn note_snapshot(n: &Note) -> CodeSnapshot {
    CodeSnapshot {
        content: n.body.clone(),
        path: Some(n.key()),
        lines: n.anchor.map(|a| a.lines()),
        side: n.anchor.map(|a| a.side()),
        ..CodeSnapshot::default()
    }
}

/// The snapshot an artifact thread gets at post time, from the session's Brief, Map and notes.
pub fn artifact_snapshot(s: &Session, target: &Target) -> Option<CodeSnapshot> {
    match target.kind {
        TargetKind::Brief => brief_snapshot(s.brief.as_ref()?, target.section.as_deref()?),
        TargetKind::Node => node_snapshot(&s.overview.as_ref()?.graph, target.node.as_deref()?),
        TargetKind::Note => s
            .notes
            .iter()
            .find(|n| Some(n.id.as_str()) == target.note.as_deref())
            .map(note_snapshot),
        _ => None,
    }
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// At least 80 % of the quote's words appear in `text`.
fn mostly_in(quote: &str, text: &str) -> bool {
    let q = words(quote);
    if q.is_empty() {
        return false;
    }
    let t = words(text);
    let hits = q.iter().filter(|w| t.contains(w)).count();
    hits * 5 >= q.len() * 4
}

fn set_target_state(t: &mut Thread, step: Step) {
    let reason = match step {
        Step::Orphaned(r) => r,
        Step::Placed(_) => {
            if t.anchor_state == AnchorState::Moved {
                t.outdated = false;
            } else if t.anchor_state != AnchorState::Placed || t.outdated {
                t.set_anchor_state(AnchorState::Placed, None);
            }
            return;
        }
        Step::Moved { how, .. } => {
            t.set_anchor_state(AnchorState::Moved, Some(how));
            return;
        }
        Step::Outdated => {
            t.set_anchor_state(AnchorState::Outdated, None);
            return;
        }
        Step::Keep | Step::Snapshot(..) => return,
    };
    if t.anchor_state == AnchorState::Orphaned && t.orphan.as_ref().is_some_and(|o| o.reason == reason) {
        return;
    }
    let orphan = orphan_of(t.code_snapshot.as_ref(), &t.file_path, &t.anchor, None, reason);
    t.set_anchor_state(AnchorState::Orphaned, None);
    t.orphan = Some(orphan);
}

/// Brief rule: same section id → placed, or outdated when its quote left the section; else the
/// section holding the quote (exact, then 80 % of its words) → moved(fuzzy) to that id; else
/// orphaned(section removed).
fn brief_step(t: &mut Thread, doc: &BriefDoc) -> Step {
    let Some(target) = t.target.as_mut() else { return Step::Keep };
    let quote = target.quote.clone().filter(|q| !q.trim().is_empty());
    let holds = |s: &rain_core::brief::SectionRef<'_>, q: &str| s.text.contains(q) || s.title.contains(q);
    let sections = doc.sections();
    if let Some(sec) = target.section.as_deref().and_then(|id| doc.section(id)) {
        let Some(q) = quote.as_deref().filter(|q| !holds(&sec, q)) else {
            return Step::Placed(None);
        };
        // Positional ids (and `How N ·` titles) shift when a section is inserted: the quote may
        // sit under another id now.
        let bare = q.split_once(" · ").map_or(q, |(_, rest)| rest);
        return match sections.iter().find(|s| s.id != sec.id && holds(s, bare)) {
            Some(s) => {
                target.section = Some(s.id.to_owned());
                Step::Moved {
                    anchor: t.anchor,
                    how: AnchorHow::Fuzzy,
                    commit: None,
                    path: None,
                }
            }
            None => Step::Outdated,
        };
    }
    let Some(q) = quote else {
        return Step::Orphaned(OrphanReason::SectionRemoved);
    };
    let found = sections
        .iter()
        .find(|s| holds(s, &q))
        .or_else(|| sections.iter().find(|s| mostly_in(&q, &format!("{} {}", s.title, s.text))));
    match found {
        Some(s) => {
            target.section = Some(s.id.to_owned());
            Step::Moved {
                anchor: t.anchor,
                how: AnchorHow::Fuzzy,
                commit: None,
                path: None,
            }
        }
        None => Step::Orphaned(OrphanReason::SectionRemoved),
    }
}

/// Map rule: same node id → placed; else a node with the label it had → moved(label) to that id;
/// else orphaned(node removed).
fn node_step(t: &mut Thread, graph: &OverviewGraph) -> Step {
    let label = t
        .code_snapshot
        .as_ref()
        .and_then(|s| s.content.lines().next())
        .map(str::to_owned)
        .or_else(|| t.target.as_ref().and_then(|x| x.quote.clone()));
    let Some(target) = t.target.as_mut() else { return Step::Keep };
    if target.node.as_deref().is_some_and(|id| graph.nodes.iter().any(|n| n.id == id)) {
        return Step::Placed(None);
    }
    match label.and_then(|l| graph.nodes.iter().find(|n| n.label == l)) {
        Some(n) => {
            target.node = Some(n.id.clone());
            Step::Moved {
                anchor: t.anchor,
                how: AnchorHow::Label,
                commit: None,
                path: None,
            }
        }
        None => Step::Orphaned(OrphanReason::NodeRemoved),
    }
}

/// Note rule: same note id → placed; else a note at the same key and lines (a replaced note) →
/// moved(fuzzy) to it; else orphaned(note removed).
fn note_step(t: &mut Thread, notes: &[Note]) -> Step {
    let snap = t.code_snapshot.clone();
    let Some(target) = t.target.as_mut() else { return Step::Keep };
    if target.note.as_deref().is_some_and(|id| notes.iter().any(|n| n.id == id)) {
        return Step::Placed(None);
    }
    let same_place = snap.as_ref().and_then(|s| {
        notes
            .iter()
            .find(|n| s.path.as_deref() == Some(n.key().as_str()) && s.lines == n.anchor.map(|a| a.lines()))
    });
    match same_place {
        Some(n) => {
            target.note = Some(n.id.clone());
            t.file_path = rain_core::comments::note_path(&n.id);
            Step::Moved {
                anchor: t.anchor,
                how: AnchorHow::Fuzzy,
                commit: None,
                path: None,
            }
        }
        None => Step::Orphaned(OrphanReason::NoteRemoved),
    }
}

/// The artifact survival rules after the Brief, the overview or the notes changed: threads on
/// Brief sections, Map nodes and notes stay, move to the section/node/note that now holds them,
/// turn outdated, or are orphaned with the post-time snapshot. Threads on an artifact the session
/// does not have at all (no brief, no overview) are left alone. Returns how many changed.
pub fn reanchor_targets(s: &mut Session) -> usize {
    let brief = s.brief.clone();
    let graph = s.overview.as_ref().map(|o| o.graph.clone());
    let mut notes = s.notes.clone();
    if let Some(o) = &s.overview {
        notes.extend(crate::notes::from_overview(o));
    }
    let mut changed = 0;
    for t in &mut s.threads {
        let before = (t.target.clone(), fingerprint(t));
        let step = match t.target_kind() {
            TargetKind::Brief => match &brief {
                Some(b) => brief_step(t, b),
                None => continue,
            },
            TargetKind::Node => match &graph {
                Some(g) => node_step(t, g),
                None => continue,
            },
            TargetKind::Note => note_step(t, &notes),
            _ => continue,
        };
        set_target_state(t, step);
        if (t.target.clone(), fingerprint(t)) != before {
            changed += 1;
        }
    }
    if changed > 0 {
        s.bump_comments();
    }
    changed
}

/// The thread waits on the user: it is open with no disposition and the agent had the last word
/// (an answered question included), or the ladder suggests it was addressed; or the agent marked
/// it addressed and the user has not confirmed. Messages with no author are the user's own, as
/// difit writes them.
pub fn waiting_on_user(t: &Thread, me: &str) -> bool {
    if t.disposition.is_some() {
        return false;
    }
    match t.state {
        ThreadState::Resolved | ThreadState::Dismissed => return false,
        ThreadState::Addressed => return true,
        ThreadState::Open => {}
    }
    addressed_suggested(t)
        || t.messages
            .last()
            .and_then(|m| m.author.as_deref())
            .map(str::trim)
            .is_some_and(|a| !a.is_empty() && !a.eq_ignore_ascii_case(me))
}

pub fn waiting_count(s: &Session, me: &str) -> usize {
    s.threads.iter().filter(|t| waiting_on_user(t, me)).count()
}

/// What a thread waits on the agent for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Owed {
    /// A question nobody holds, or one whose answer failed or timed out.
    Question,
    /// The user had the last word.
    Reply,
    /// The attached agent claimed the question and has not replied.
    Claimed,
}

/// The user wrote `m`: no author, as difit and the window write theirs, or `me`.
pub fn from_user(m: &Message, me: &str) -> bool {
    m.author
        .as_deref()
        .map(str::trim)
        .is_none_or(|a| a.is_empty() || a.eq_ignore_ascii_case(me))
}

/// The other side of [`waiting_on_user`]: a pending question the launcher does not hold, or a
/// thread still in play whose last word is the user's (said after the agent marked it addressed,
/// when it did).
pub fn waiting_on_agent(t: &Thread, me: &str) -> Option<Owed> {
    if matches!(t.state, ThreadState::Resolved | ThreadState::Dismissed) {
        return None;
    }
    if let Some(a) = t.answer.as_ref().filter(|a| a.status.pending()) {
        if claimed(t) {
            return Some(Owed::Claimed);
        }
        return (a.by.as_deref() != Some("launcher")).then_some(Owed::Question);
    }
    let last = t.messages.last().filter(|m| from_user(m, me))?;
    let addressed_since = t.state == ThreadState::Addressed && t.state_at.as_deref().is_some_and(|at| last.created_at.as_str() <= at);
    if addressed_since {
        return None;
    }
    let unanswered = t
        .answer
        .as_ref()
        .is_some_and(|a| matches!(a.status, AnswerStatus::Failed | AnswerStatus::Timeout));
    Some(if unanswered { Owed::Question } else { Owed::Reply })
}
