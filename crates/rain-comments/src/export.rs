//! What Claude reads back: `GET /api/comments-json` and `GET /api/comments-output`. Their
//! `version` is the session's `comments_version`. Only open threads are read back unless `all`
//! is asked for, so a resolved thread leaves readback as a deleted one did.

use rain_core::{Anchor, AnchorState, AnswerStatus, NoteKind, Session, Side, TargetKind, Thread, ThreadState};
use serde::Serialize;
use serde_json::{Value, json};

use crate::label::first_sentence;
use crate::ops::addressed_suggested;

fn in_repo<'a>(s: &'a Session, repo: Option<&'a str>, all: bool) -> impl Iterator<Item = &'a Thread> + 'a {
    s.threads
        .iter()
        .filter(move |t| repo.is_none_or(|r| t.repo == r) && (all || t.is_open()))
}

/// `{"version": n, "threads": [...]}`, open threads of `repo` only when given. A `Value` sorts
/// its keys; send [`comments_json_string`] where the bytes matter.
pub fn comments_json(s: &Session, repo: Option<&str>) -> Value {
    comments_json_with(s, repo, false)
}

/// [`comments_json`], with threads in every state when `all`.
pub fn comments_json_with(s: &Session, repo: Option<&str>, all: bool) -> Value {
    let threads: Vec<&Thread> = in_repo(s, repo, all).collect();
    json!({ "version": s.comments_version, "threads": threads })
}

#[derive(Serialize)]
struct CommentsJson<'a> {
    version: u64,
    threads: Vec<&'a Thread>,
}

/// [`comments_json`] as the exact bytes difit's `GET /api/comments-json` sends (its field order).
pub fn comments_json_string(s: &Session, repo: Option<&str>) -> String {
    comments_json_string_with(s, repo, false)
}

/// [`comments_json_string`], with threads in every state when `all`.
pub fn comments_json_string_with(s: &Session, repo: Option<&str>, all: bool) -> String {
    let body = CommentsJson {
        version: s.comments_version,
        threads: in_repo(s, repo, all).collect(),
    };
    serde_json::to_string(&body).unwrap_or_default()
}

/// difit's `formatCommentsOutput` over the session's open threads (empty text when there are
/// none, as its route sends). Additive lines for rain-only data follow a thread's location: for a
/// thread with round-3 data (a kind, a state other than open, a moved/outdated/orphaned anchor,
/// an artifact target or an answer) `[id …] kind · state · anchor`, the answer, and the code it
/// was written against (the orphan's last-seen snapshot when orphaned); then `Disposition: <d>`
/// when set and `Selection: <text>` for a span with a snapshot. Artifact threads read
/// `Brief › How 2 "…"`, `Map › node …`, `Note … on …`, `<path> (file)` or `Review (whole
/// change)` instead of a line. With no `repo` filter, threads of a named repo show
/// `<repo>/<path>`.
pub fn comments_output(s: &Session, repo: Option<&str>) -> String {
    comments_output_with(s, repo, false)
}

/// [`comments_output`], with threads in every state when `all`.
pub fn comments_output_with(s: &Session, repo: Option<&str>, all: bool) -> String {
    let threads: Vec<&Thread> = in_repo(s, repo, all).collect();
    if threads.is_empty() {
        return String::new();
    }
    let sections: Vec<String> = threads.iter().map(|t| thread_prompt(s, t, repo.is_none())).collect();
    let rule = "=".repeat(50);
    [
        "\n📝 Comments from review session:".to_owned(),
        rule.clone(),
        sections.join("\n=====\n"),
        rule,
        format!("Total comments: {}\n", threads.len()),
    ]
    .join("\n")
}

fn path_of(t: &Thread, with_repo: bool) -> String {
    match (with_repo && !t.repo.is_empty(), t.file_path.is_empty()) {
        (_, true) => "<unknown file>".to_owned(),
        (true, false) => format!("{}/{}", t.repo, t.file_path),
        (false, false) => t.file_path.clone(),
    }
}

fn lines_label(a: &Anchor) -> String {
    match *a {
        Anchor::Line { line, .. } => format!("L{line}"),
        _ => {
            let (x, y) = a.lines();
            format!("L{x}-L{y}")
        }
    }
}

fn location(s: &Session, t: &Thread, with_repo: bool) -> String {
    match t.target_kind() {
        TargetKind::Brief => brief_location(s, t),
        TargetKind::Node => node_location(s, t),
        TargetKind::Note => note_location(s, t),
        TargetKind::File => format!("{} (file)", path_of(t, with_repo)),
        TargetKind::Pr => "Review (whole change)".to_owned(),
        TargetKind::Line => {
            let old = if t.anchor.side() == Side::Old { " (old)" } else { "" };
            format!("{}:{}{old}", path_of(t, with_repo), lines_label(&t.anchor))
        }
    }
}

/// `Intent`, `How 2`, `Invariant 1`, … for a Brief section id.
fn section_label(s: &Session, id: &str) -> Option<(String, Option<String>)> {
    let doc = s.brief.as_ref()?;
    if doc.intent.as_ref().is_some_and(|x| x.id == id) {
        return Some(("Intent".into(), None));
    }
    if let Some(i) = doc.how.iter().position(|h| h.id == id) {
        return Some((format!("How {}", i + 1), Some(doc.how[i].title.clone())));
    }
    if let Some(i) = doc.invariants.iter().position(|v| v.id == id) {
        return Some((format!("Invariant {}", i + 1), Some(first_sentence(&doc.invariants[i].text, 60))));
    }
    let fixed = [
        (doc.not_changed.as_ref().map(|x| x.id.as_str()), "Not changed"),
        (doc.not_covered.as_ref().map(|x| x.id.as_str()), "Not covered"),
        (doc.verify.as_ref().map(|x| x.id.as_str()), "Verify by hand"),
    ];
    fixed
        .into_iter()
        .find(|(x, _)| *x == Some(id))
        .map(|(_, label)| (label.to_owned(), None))
}

fn brief_location(s: &Session, t: &Thread) -> String {
    let target = t.target.as_ref();
    let id = target.and_then(|x| x.section.as_deref()).unwrap_or("?");
    let mut out = match section_label(s, id) {
        Some((label, Some(title))) => format!("Brief › {label} \"{title}\""),
        Some((label, None)) => format!("Brief › {label}"),
        None => format!("Brief › {id}"),
    };
    if let Some(q) = target.and_then(|x| x.quote.as_deref()).filter(|q| !q.is_empty()) {
        out.push_str(&format!(" (quote: \"{q}\")"));
    }
    out
}

fn node_location(s: &Session, t: &Thread) -> String {
    let id = t.target.as_ref().and_then(|x| x.node.as_deref()).unwrap_or("?");
    let node = s.overview.as_ref().and_then(|o| o.graph.nodes.iter().find(|n| n.id == id));
    match node {
        Some(n) => match n.files.first() {
            Some(f) => format!("Map › node {id} ({}, {})", n.label, f.path),
            None => format!("Map › node {id} ({})", n.label),
        },
        None => format!("Map › node {id}"),
    }
}

fn note_location(s: &Session, t: &Thread) -> String {
    let id = t.target.as_ref().and_then(|x| x.note.as_deref()).unwrap_or("?");
    let Some(n) = s.notes.iter().find(|n| n.id == id) else {
        return format!("Note {id}");
    };
    let quote = first_sentence(&n.body, 60);
    match (n.kind, n.anchor) {
        (NoteKind::Story, _) => format!("Note {id} (story) \"{quote}\""),
        (_, Some(a)) => format!("Note {id} on {}:{} \"{quote}\"", n.key(), lines_label(&a)),
        (_, None) => format!("Note {id} on {} \"{quote}\"", n.key()),
    }
}

fn has_round3(t: &Thread) -> bool {
    t.kind.is_some()
        || t.state != ThreadState::Open
        || t.placement() != AnchorState::Placed
        || t.target.is_some()
        || t.answer.is_some()
        || t.orphan.is_some()
}

fn meta_line(t: &Thread) -> String {
    let mut kind = t.kind().label().to_owned();
    if t.blocking() {
        kind.push_str(" (blocking)");
    }
    let mut state = t.state.label().to_owned();
    if let Some(r) = t.dismiss_reason.filter(|_| t.state == ThreadState::Dismissed) {
        state.push_str(&format!(" ({})", r.label()));
    }
    if let Some(sha) = &t.state_sha {
        state.push_str(&format!(" in {sha}"));
    }
    let anchor = match t.placement() {
        AnchorState::Moved => {
            let how = t.anchor_how.map_or("exact", |h| h.label());
            match t.original {
                Some(o) if o != t.anchor => format!("moved({how}) from {}", lines_label(&o)),
                _ => format!("moved({how})"),
            }
        }
        AnchorState::Orphaned => match &t.orphan {
            Some(o) => format!("orphaned({})", o.reason.label()),
            None => "orphaned".to_owned(),
        },
        other => other.label().to_owned(),
    };
    let mut line = format!("[id {}] kind: {kind} · state: {state} · anchor: {anchor}", t.id);
    if addressed_suggested(t) {
        line.push_str(" · addressed?");
    }
    line
}

fn answer_line(t: &Thread) -> Option<String> {
    let a = t.answer.as_ref()?;
    let by = a.by.as_deref().map(|b| format!(" ({b})")).unwrap_or_default();
    Some(match a.status {
        AnswerStatus::Answered => format!("Answer: answered by claude{by}"),
        AnswerStatus::Failed => format!("Answer: failed{}", a.error.as_deref().map(|e| format!(": {e}")).unwrap_or_default()),
        other => format!("Answer: {}{by}", other.label()),
    })
}

fn fenced(before: &[String], content: &str, after: &[String]) -> String {
    let mut out = String::from("```\n");
    for l in before {
        out.push_str(&format!("  {l}\n"));
    }
    for l in content.split('\n') {
        out.push_str(&format!("> {l}\n"));
    }
    for l in after {
        out.push_str(&format!("  {l}\n"));
    }
    out.push_str("```");
    out
}

/// The code (or artifact text) the thread was written against.
fn excerpt(t: &Thread) -> Option<String> {
    if let Some(o) = &t.orphan {
        let snap = &o.snapshot;
        let at = o.last_seen_sha.as_deref().unwrap_or("an earlier revision");
        let place = match (snap.path.as_deref(), snap.lines) {
            (Some(p), Some((a, b))) if t.target_kind() == TargetKind::Line => {
                format!(" ({p}:{})", if a == b { format!("L{a}") } else { format!("L{a}-L{b}") })
            }
            _ => String::new(),
        };
        if snap.content.is_empty() {
            return Some(format!("Last seen at {at}{place}."));
        }
        return Some(format!(
            "Last seen at {at}{place}:\n{}",
            fenced(&snap.before, &snap.content, &snap.after)
        ));
    }
    let snap = t.code_snapshot.as_ref().filter(|c| !c.content.is_empty())?;
    match t.target_kind() {
        TargetKind::Line => Some(format!("Code:\n{}", fenced(&snap.before, &snap.content, &snap.after))),
        _ => None,
    }
}

fn thread_prompt(s: &Session, t: &Thread, with_repo: bool) -> String {
    let code = t.code_snapshot.as_ref().map(|c| c.content.as_str());
    let mut sections = vec![location(s, t, with_repo)];
    if has_round3(t) {
        sections.push(meta_line(t));
        sections.extend(answer_line(t));
        sections.extend(excerpt(t));
    }
    if let Some(d) = t.disposition {
        sections.push(format!("Disposition: {}", d.label()));
    }
    if let (Anchor::Span { .. }, Some(sel), TargetKind::Line) = (t.anchor, code, t.target_kind()) {
        sections.push(if sel.contains('\n') {
            format!("Selection:\n```\n{sel}\n```")
        } else {
            format!("Selection: {sel}")
        });
    }
    let code = code.filter(|_| t.target_kind() == TargetKind::Line);
    for (i, m) in t.messages.iter().enumerate() {
        if i > 0 {
            let author = m.author.as_deref().map(str::trim).filter(|a| !a.is_empty()).unwrap_or("Unknown");
            sections.push(format!("Reply {i} ({author})"));
        }
        sections.push(comment_content(&m.body, code));
    }
    sections.retain(|s| !s.is_empty());
    sections.join("\n")
}

/// A ```` ```suggestion ```` block: its code and byte range in the body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub code: String,
    pub start: usize,
    pub end: usize,
}

/// difit's `parseSuggestionBlocks`: each `` ```suggestion\n…``` `` (lazy, to the next fence),
/// its code without one trailing newline.
pub fn suggestions(body: &str) -> Vec<Suggestion> {
    const OPEN: &str = "```suggestion\n";
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = body[from..].find(OPEN) {
        let start = from + i;
        let code_at = start + OPEN.len();
        let Some(j) = body[code_at..].find("```") else { break };
        let code = &body[code_at..code_at + j];
        let end = code_at + j + 3;
        out.push(Suggestion {
            code: code.strip_suffix('\n').unwrap_or(code).to_owned(),
            start,
            end,
        });
        from = end;
    }
    out
}

/// difit's `formatCommentContent`: suggestion blocks become ORIGINAL/SUGGESTED fences.
fn comment_content(body: &str, code: Option<&str>) -> String {
    let blocks = suggestions(body);
    if blocks.is_empty() {
        return body.to_owned();
    }
    let mut out = String::new();
    let push = |out: &mut String, s: &str| {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(s);
    };
    let mut last = 0;
    for b in &blocks {
        let before = js_trim(&body[last..b.start]);
        if !before.is_empty() {
            push(&mut out, before);
        }
        if let Some(c) = code.filter(|c| !c.is_empty()) {
            push(&mut out, &format!("ORIGINAL:\n```\n{c}\n```"));
        }
        push(&mut out, &format!("SUGGESTED:\n```\n{}\n```", b.code));
        last = b.end;
    }
    let after = js_trim(&body[last..]);
    if !after.is_empty() {
        push(&mut out, after);
    }
    out
}

/// JavaScript's `String.prototype.trim`: Unicode white space plus the BOM.
fn js_trim(s: &str) -> &str {
    s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}')
}
