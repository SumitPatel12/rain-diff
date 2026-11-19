//! Comment threads in difit's JSON shape, plus additive fields (`span`, `disposition`, `repo`,
//! `outdated`, `commit`, `original`) that difit readers ignore.

use serde::{Deserialize, Serialize};

use crate::diff::Side;

/// Where a thread sits. Columns are 0-based char offsets into the line text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "AnchorWire", into = "AnchorWire")]
pub enum Anchor {
    Line {
        side: Side,
        line: u32,
    },
    Range {
        side: Side,
        start: u32,
        end: u32,
    },
    Span {
        side: Side,
        start_line: u32,
        start_col: u32,
        end_line: u32,
        end_col: u32,
    },
}

impl Anchor {
    pub fn side(&self) -> Side {
        match *self {
            Anchor::Line { side, .. } | Anchor::Range { side, .. } | Anchor::Span { side, .. } => side,
        }
    }

    /// First and last line covered.
    pub fn lines(&self) -> (u32, u32) {
        match *self {
            Anchor::Line { line, .. } => (line, line),
            Anchor::Range { start, end, .. } => (start, end),
            Anchor::Span { start_line, end_line, .. } => (start_line, end_line),
        }
    }

    /// The line a thread renders under: the last covered line.
    pub fn end_line(&self) -> u32 {
        self.lines().1
    }
}

/// Line 1 of the new side: where artifact threads (`@brief`, `@map`, …) sit.
impl Default for Anchor {
    fn default() -> Anchor {
        Anchor::Line { side: Side::New, line: 1 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
enum LineSpec {
    One(u32),
    Range { start: u32, end: u32 },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct Position {
    side: Side,
    line: LineSpec,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpanWire {
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
struct AnchorWire {
    position: Position,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    span: Option<SpanWire>,
}

impl From<AnchorWire> for Anchor {
    fn from(w: AnchorWire) -> Anchor {
        let side = w.position.side;
        match (w.span, w.position.line) {
            (Some(s), _) => Anchor::Span {
                side,
                start_line: s.start_line,
                start_col: s.start_col,
                end_line: s.end_line,
                end_col: s.end_col,
            },
            (None, LineSpec::One(line)) => Anchor::Line { side, line },
            (None, LineSpec::Range { start, end }) => Anchor::Range { side, start, end },
        }
    }
}

impl From<Anchor> for AnchorWire {
    fn from(a: Anchor) -> AnchorWire {
        match a {
            Anchor::Line { side, line } => AnchorWire {
                position: Position {
                    side,
                    line: LineSpec::One(line),
                },
                span: None,
            },
            Anchor::Range { side, start, end } => AnchorWire {
                position: Position {
                    side,
                    line: LineSpec::Range { start, end },
                },
                span: None,
            },
            Anchor::Span {
                side,
                start_line,
                start_col,
                end_line,
                end_col,
            } => AnchorWire {
                position: Position {
                    side,
                    line: LineSpec::Range {
                        start: start_line,
                        end: end_line,
                    },
                },
                span: Some(SpanWire {
                    start_line,
                    start_col,
                    end_line,
                    end_col,
                }),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Disposition {
    Agree,
    Dismiss,
    Ask,
    Defer,
}

impl Disposition {
    pub fn label(&self) -> &'static str {
        match self {
            Disposition::Agree => "agree",
            Disposition::Dismiss => "dismiss",
            Disposition::Ask => "ask",
            Disposition::Defer => "defer",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// The anchored text as it was when the thread was posted. For artifact threads `content` is the
/// section, node or note text instead of code.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CodeSnapshot {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<(u32, u32)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<Side>,
    /// Up to two lines above and below the anchor, for ranking candidates when it moves.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// `T`, or its default when a newer build wrote a value this one doesn't know, so one unknown
/// variant doesn't fail the whole session.
fn lenient<'de, D: serde::Deserializer<'de>, T: serde::de::DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

/// The first of a posted thread's `field: value` pairs this build doesn't know. Session files
/// load leniently (a newer build's values fall back to defaults); API input is checked strictly
/// so a misspelled state is refused rather than quietly reopened.
pub fn unknown_value(thread: &serde_json::Value) -> Option<String> {
    fn bad<T: serde::de::DeserializeOwned>(v: &serde_json::Value, field: &str) -> Option<String> {
        let x = v.get(field).filter(|x| !x.is_null())?;
        serde_json::from_value::<T>(x.clone()).is_err().then(|| format!("{field}: {x}"))
    }
    bad::<ThreadKind>(thread, "kind")
        .or_else(|| bad::<ThreadState>(thread, "state"))
        .or_else(|| bad::<Disposition>(thread, "disposition"))
        .or_else(|| bad::<DismissReason>(thread, "dismissReason"))
        .or_else(|| bad::<AnchorState>(thread, "anchorState"))
}

fn lenient_orphan_reason<'de, D: serde::Deserializer<'de>>(d: D) -> Result<OrphanReason, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or(OrphanReason::Ambiguous))
}

/// An unknown status reads as cancelled: never queued again on its own, and retryable.
fn lenient_answer_status<'de, D: serde::Deserializer<'de>>(d: D) -> Result<AnswerStatus, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or(AnswerStatus::Cancelled))
}

fn lenient_target_kind<'de, D: serde::Deserializer<'de>>(d: D) -> Result<TargetKind, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or(TargetKind::File))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThreadKind {
    #[default]
    Comment,
    Issue,
    Question,
    Nit,
    Suggestion,
    Praise,
}

impl ThreadKind {
    pub const ALL: [ThreadKind; 6] = [
        ThreadKind::Comment,
        ThreadKind::Issue,
        ThreadKind::Question,
        ThreadKind::Nit,
        ThreadKind::Suggestion,
        ThreadKind::Praise,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            ThreadKind::Comment => "comment",
            ThreadKind::Issue => "issue",
            ThreadKind::Question => "question",
            ThreadKind::Nit => "nit",
            ThreadKind::Suggestion => "suggestion",
            ThreadKind::Praise => "praise",
        }
    }
}

/// A parsed conventional-comment prefix: `F3 issue (blocking): rest`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefix<'a> {
    pub finding: Option<&'a str>,
    pub kind: ThreadKind,
    /// `(blocking)` → true, `(non-blocking)` → false, absent → None.
    pub blocking: Option<bool>,
    pub rest: &'a str,
}

/// The prefix of `body` when its first line starts with a known kind followed by `:`. Kinds with
/// no [`ThreadKind`] of their own (`note`, `todo`, `chore`, …) read as [`ThreadKind::Comment`].
pub fn parse_prefix(body: &str) -> Option<Prefix<'_>> {
    let s = body.trim_start();
    let (finding, s) = match s.split_once(char::is_whitespace) {
        Some((f, rest)) if f.len() > 1 && f.starts_with(['F', 'f']) && f[1..].bytes().all(|b| b.is_ascii_digit()) => {
            (Some(f), rest.trim_start())
        }
        _ => (None, s),
    };
    let word_end = s.find(|c: char| !(c.is_ascii_alphabetic() || c == '-')).unwrap_or(s.len());
    let kind = match s[..word_end].to_ascii_lowercase().as_str() {
        "issue" | "bug" | "problem" => ThreadKind::Issue,
        "suggestion" | "suggest" => ThreadKind::Suggestion,
        "question" => ThreadKind::Question,
        "nitpick" | "nit" => ThreadKind::Nit,
        "praise" => ThreadKind::Praise,
        "note" | "thought" | "info" | "todo" | "chore" => ThreadKind::Comment,
        _ => return None,
    };
    let mut s = s[word_end..].trim_start();
    let mut blocking = None;
    if let Some(inner) = s.strip_prefix('(') {
        let close = inner.find(')')?;
        let decor = inner[..close].to_ascii_lowercase();
        blocking = if decor.contains("non-blocking") || decor.contains("nonblocking") {
            Some(false)
        } else if decor.contains("blocking") {
            Some(true)
        } else {
            None
        };
        s = inner[close + 1..].trim_start();
    }
    let rest = s.strip_prefix(':')?.trim_start();
    Some(Prefix {
        finding,
        kind,
        blocking,
        rest,
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThreadState {
    #[default]
    Open,
    Addressed,
    Resolved,
    Dismissed,
}

impl ThreadState {
    pub fn label(&self) -> &'static str {
        match self {
            ThreadState::Open => "open",
            ThreadState::Addressed => "addressed",
            ThreadState::Resolved => "resolved",
            ThreadState::Dismissed => "dismissed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DismissReason {
    FalsePositive,
    OutOfScope,
    Style,
    Duplicate,
    Other,
}

impl DismissReason {
    pub const ALL: [DismissReason; 5] = [
        DismissReason::FalsePositive,
        DismissReason::OutOfScope,
        DismissReason::Style,
        DismissReason::Duplicate,
        DismissReason::Other,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            DismissReason::FalsePositive => "false positive",
            DismissReason::OutOfScope => "out of scope",
            DismissReason::Style => "style",
            DismissReason::Duplicate => "duplicate",
            DismissReason::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnchorState {
    #[default]
    Placed,
    Moved,
    Outdated,
    Orphaned,
}

impl AnchorState {
    pub fn label(&self) -> &'static str {
        match self {
            AnchorState::Placed => "placed",
            AnchorState::Moved => "moved",
            AnchorState::Outdated => "outdated",
            AnchorState::Orphaned => "orphaned",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AnchorHow {
    Exact,
    Fuzzy,
    Renamed,
    SectionId,
    NodeId,
    Label,
}

impl AnchorHow {
    pub fn label(&self) -> &'static str {
        match self {
            AnchorHow::Exact => "exact",
            AnchorHow::Fuzzy => "fuzzy",
            AnchorHow::Renamed => "renamed",
            AnchorHow::SectionId => "section id",
            AnchorHow::NodeId => "node id",
            AnchorHow::Label => "label",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OrphanReason {
    LineDeleted,
    FileDeleted,
    FileRenamed,
    ContentRewritten,
    SectionRemoved,
    NodeRemoved,
    NoteRemoved,
    Ambiguous,
    /// The file is no longer part of the diff (its change was reverted), not deleted.
    LeftDiff,
}

impl OrphanReason {
    pub fn label(&self) -> &'static str {
        match self {
            OrphanReason::LineDeleted => "line deleted",
            OrphanReason::FileDeleted => "file deleted",
            OrphanReason::FileRenamed => "file renamed",
            OrphanReason::ContentRewritten => "content rewritten",
            OrphanReason::SectionRemoved => "section removed",
            OrphanReason::NodeRemoved => "node removed",
            OrphanReason::NoteRemoved => "note removed",
            OrphanReason::Ambiguous => "ambiguous",
            OrphanReason::LeftDiff => "not in the diff",
        }
    }
}

/// Why and where an anchor was lost; `snapshot` is a copy of the post-time snapshot plus where it
/// last sat, kept so the old code stays readable across rounds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Orphan {
    #[serde(alias = "orphanReason", deserialize_with = "lenient_orphan_reason")]
    pub reason: OrphanReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_sha: Option<String>,
    pub snapshot: CodeSnapshot,
    #[serde(default)]
    pub at: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TargetKind {
    #[default]
    Line,
    File,
    Pr,
    Brief,
    Node,
    Note,
}

/// What a thread is about when it is not (only) a line of code.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    #[serde(deserialize_with = "lenient_target_kind")]
    pub kind: TargetKind,
    /// Brief section id (`how.2`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// Selected text, or the section title when nothing was selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// The Map view the node was picked in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Virtual `filePath`s of artifact threads.
pub const BRIEF_PATH: &str = "@brief";
pub const MAP_PATH: &str = "@map";
pub const PR_PATH: &str = "@pr";
pub const NOTE_PATH_PREFIX: &str = "@note:";

pub fn note_path(note_id: &str) -> String {
    format!("{NOTE_PATH_PREFIX}{note_id}")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnswerStatus {
    #[default]
    Queued,
    Answering,
    Answered,
    Failed,
    Timeout,
    Cancelled,
}

impl AnswerStatus {
    pub fn label(&self) -> &'static str {
        match self {
            AnswerStatus::Queued => "queued",
            AnswerStatus::Answering => "answering",
            AnswerStatus::Answered => "answered",
            AnswerStatus::Failed => "failed",
            AnswerStatus::Timeout => "timed out",
            AnswerStatus::Cancelled => "cancelled",
        }
    }

    /// Still waiting on an answerer.
    pub fn pending(&self) -> bool {
        matches!(self, AnswerStatus::Queued | AnswerStatus::Answering)
    }
}

/// The state of the agent's answer to a question thread.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    #[serde(deserialize_with = "lenient_answer_status")]
    pub status: AnswerStatus,
    /// `attached` or `launcher`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

/// A conversation on an anchor: `messages[0]` opens it, the rest are replies.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: String,
    pub file_path: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(flatten)]
    pub anchor: Anchor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_snapshot: Option<CodeSnapshot>,
    pub messages: Vec<Message>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub disposition: Option<Disposition>,
    /// Repo id the thread belongs to; empty in single-repo sessions.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub outdated: bool,
    /// The revision of the anchor's side when its snapshot was taken, so a later diff can map
    /// the lines forward.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// Where the thread was first placed, once it has moved with its code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original: Option<Anchor>,
    /// Stated kind; `None` on round-2 threads, read through [`Thread::kind`].
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub kind: Option<ThreadKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocking: Option<bool>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "is_default")]
    pub state: ThreadState,
    /// The sha a state change refers to (`addressed` in `abc123`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_at: Option<String>,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub dismiss_reason: Option<DismissReason>,
    /// Set through [`Thread::set_anchor_state`] so `outdated` stays in step.
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "is_default")]
    pub anchor_state: AnchorState,
    #[serde(default, deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub anchor_how: Option<AnchorHow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orphan: Option<Orphan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Answer>,
}

impl Thread {
    /// The stated kind, else the one the first message's prefix names, else `Comment`.
    pub fn kind(&self) -> ThreadKind {
        self.kind.unwrap_or_else(|| self.prefix().map_or(ThreadKind::Comment, |p| p.kind))
    }

    /// The stated blocking flag, else the prefix's.
    pub fn blocking(&self) -> bool {
        self.blocking.or_else(|| self.prefix().and_then(|p| p.blocking)).unwrap_or(false)
    }

    fn prefix(&self) -> Option<Prefix<'_>> {
        parse_prefix(&self.messages.first()?.body)
    }

    /// The anchor state, reading a round-2 `outdated` flag as `Outdated`.
    pub fn placement(&self) -> AnchorState {
        match self.anchor_state {
            AnchorState::Placed | AnchorState::Moved if self.outdated => AnchorState::Outdated,
            s => s,
        }
    }

    pub fn set_anchor_state(&mut self, state: AnchorState, how: Option<AnchorHow>) {
        self.anchor_state = state;
        self.anchor_how = how;
        self.outdated = matches!(state, AnchorState::Outdated | AnchorState::Orphaned);
        if state != AnchorState::Orphaned {
            self.orphan = None;
        }
    }

    /// The revision the snapshot was taken against (stored as `commit` since round 2).
    pub fn snapshot_sha(&self) -> Option<&str> {
        self.commit.as_deref()
    }

    pub fn is_open(&self) -> bool {
        self.state == ThreadState::Open
    }

    /// On the Brief, the Map, a note or the whole change rather than a file.
    pub fn is_artifact(&self) -> bool {
        self.file_path.starts_with('@')
    }

    pub fn target_kind(&self) -> TargetKind {
        self.target.as_ref().map_or(TargetKind::Line, |t| t.kind)
    }
}

/// The fields shared by thread and reply imports. `filePath` and `position` are required except
/// on a reply that names its `threadId`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file_path: String,
    #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_snapshot: Option<CodeSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ThreadKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocking: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Answer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<ThreadState>,
}

/// difit's `CommentImport`: `{"type":"thread"|"reply", ...}`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CommentImport {
    Thread(ImportBody),
    Reply(ImportBody),
}

impl CommentImport {
    pub fn body(&self) -> &ImportBody {
        match self {
            CommentImport::Thread(b) | CommentImport::Reply(b) => b,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewedRecord {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo: String,
    pub file_path: String,
    pub viewed_at: String,
    pub diff_content_hash: String,
    /// Hunks viewed one by one; empty when the whole file was.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hunks: Vec<HunkViewed>,
}

/// A viewed hunk by its stable id and the hash of its text when viewed.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct HunkViewed {
    pub id: String,
    pub hash: String,
}

/// An unsent comment, kept across restarts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo: String,
    pub file_path: String,
    #[serde(flatten)]
    pub anchor: Anchor,
    pub text: String,
    /// The composer's mode and type: `question` for Question mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ThreadKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREADS: &str = r#"[{"id":"t1","filePath":"src/a.ts","createdAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-01-01T00:00:00.000Z","position":{"side":"new","line":12},"messages":[{"id":"m1","body":"Why?","author":"claude","createdAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-01-01T00:00:00.000Z"},{"id":"m2","body":"Because.","createdAt":"2026-01-02T00:00:00.000Z","updatedAt":"2026-01-02T00:00:00.000Z"}]},{"id":"t2","filePath":"src/b.cs","createdAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-01-01T00:00:00.000Z","position":{"side":"old","line":{"start":3,"end":7}},"codeSnapshot":{"content":"int x;","language":"csharp"},"messages":[{"id":"m3","body":"range","createdAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-01-01T00:00:00.000Z"}],"disposition":"agree"},{"id":"t3","filePath":"src/c.cs","createdAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-01-01T00:00:00.000Z","position":{"side":"new","line":{"start":4,"end":5}},"span":{"startLine":4,"startCol":8,"endLine":5,"endCol":2},"messages":[{"id":"m4","body":"span","createdAt":"2026-01-01T00:00:00.000Z","updatedAt":"2026-01-01T00:00:00.000Z"}],"repo":"server","outdated":true}]"#;

    #[test]
    fn difit_threads_round_trip_byte_stable() {
        let threads: Vec<Thread> = serde_json::from_str(THREADS).unwrap();
        assert_eq!(threads[0].anchor, Anchor::Line { side: Side::New, line: 12 });
        assert_eq!(
            threads[1].anchor,
            Anchor::Range {
                side: Side::Old,
                start: 3,
                end: 7
            }
        );
        assert_eq!(
            threads[2].anchor,
            Anchor::Span {
                side: Side::New,
                start_line: 4,
                start_col: 8,
                end_line: 5,
                end_col: 2
            }
        );
        assert_eq!(threads[0].messages.len(), 2);
        assert_eq!(threads[1].disposition, Some(Disposition::Agree));
        assert_eq!(serde_json::to_string(&threads).unwrap(), THREADS);
    }

    #[test]
    fn span_writes_a_difit_readable_range_position() {
        let a = Anchor::Span {
            side: Side::New,
            start_line: 2,
            start_col: 1,
            end_line: 2,
            end_col: 9,
        };
        let v = serde_json::to_value(a).unwrap();
        assert_eq!(v["position"]["line"]["start"], 2);
        assert_eq!(v["position"]["line"]["end"], 2);
        assert_eq!(v["span"]["endCol"], 9);
    }

    #[test]
    fn imports_parse_thread_and_reply() {
        let json = r#"[{"type":"thread","filePath":"a.ts","position":{"side":"new","line":3},"body":"hi","author":"claude"},
                      {"type":"reply","filePath":"a.ts","position":{"side":"new","line":3},"body":"yo"}]"#;
        let imports: Vec<CommentImport> = serde_json::from_str(json).unwrap();
        assert!(matches!(&imports[0], CommentImport::Thread(b) if b.body == "hi" && b.author.as_deref() == Some("claude")));
        assert!(matches!(&imports[1], CommentImport::Reply(b) if b.anchor == Some(Anchor::Line { side: Side::New, line: 3 })));
        let back: Vec<CommentImport> = serde_json::from_str(&serde_json::to_string(&imports).unwrap()).unwrap();
        assert_eq!(back, imports);
    }

    #[test]
    fn reply_by_thread_id_needs_no_path_or_position() {
        let json = r#"{"type":"reply","threadId":"t1","body":"done"}"#;
        let imp: CommentImport = serde_json::from_str(json).unwrap();
        let b = imp.body();
        assert_eq!((b.thread_id.as_deref(), b.file_path.as_str(), b.anchor), (Some("t1"), "", None));
        assert_eq!(
            serde_json::to_string(&imp).unwrap(),
            r#"{"type":"reply","body":"done","threadId":"t1"}"#
        );
    }

    #[test]
    fn drafts_and_viewed_round_trip() {
        let d = Draft {
            repo: String::new(),
            file_path: "x".into(),
            anchor: Anchor::Range {
                side: Side::New,
                start: 1,
                end: 2,
            },
            text: "t".into(),
            ..Draft::default()
        };
        let s = serde_json::to_string(&d).unwrap();
        assert_eq!(
            s,
            r#"{"filePath":"x","position":{"side":"new","line":{"start":1,"end":2}},"text":"t"}"#
        );
        assert_eq!(serde_json::from_str::<Draft>(&s).unwrap(), d);
        let v = ViewedRecord {
            repo: "server".into(),
            file_path: "x".into(),
            viewed_at: "t".into(),
            diff_content_hash: "h".into(),
            hunks: vec![HunkViewed {
                id: "a1".into(),
                hash: "b2".into(),
            }],
        };
        assert_eq!(
            serde_json::from_str::<ViewedRecord>(&serde_json::to_string(&v).unwrap()).unwrap(),
            v
        );
    }

    #[test]
    fn kind_and_blocking_derive_from_the_prefix_when_unstated() {
        let mut threads: Vec<Thread> = serde_json::from_str(THREADS).unwrap();
        assert_eq!(threads[0].kind(), ThreadKind::Comment);
        threads[0].messages[0].body = "F1 issue (blocking): The amount is unchecked.".into();
        assert_eq!((threads[0].kind(), threads[0].blocking()), (ThreadKind::Issue, true));
        assert!(threads[0].kind.is_none(), "derived, never written back");
        threads[0].kind = Some(ThreadKind::Question);
        assert_eq!(threads[0].kind(), ThreadKind::Question);
        let p = parse_prefix("nit (non-blocking): spacing").unwrap();
        assert_eq!(
            (p.kind, p.blocking, p.finding, p.rest),
            (ThreadKind::Nit, Some(false), None, "spacing")
        );
        assert_eq!(parse_prefix("todo: later").unwrap().kind, ThreadKind::Comment);
        assert!(parse_prefix("Why was this removed?").is_none());
        assert!(parse_prefix("issue without colon").is_none());
    }

    #[test]
    fn round_three_fields_round_trip_in_camel_case() {
        let t = Thread {
            id: "q1".into(),
            file_path: BRIEF_PATH.into(),
            created_at: "t".into(),
            updated_at: "t".into(),
            messages: vec![Message {
                id: "m".into(),
                body: "why the same unit of work?".into(),
                ..Message::default()
            }],
            kind: Some(ThreadKind::Question),
            state: ThreadState::Dismissed,
            dismiss_reason: Some(DismissReason::OutOfScope),
            anchor_state: AnchorState::Orphaned,
            anchor_how: Some(AnchorHow::SectionId),
            orphan: Some(Orphan {
                reason: OrphanReason::SectionRemoved,
                last_seen_sha: Some("a1b2c3d".into()),
                snapshot: CodeSnapshot {
                    content: "One Voucher row".into(),
                    lines: Some((3, 4)),
                    side: Some(Side::New),
                    before: vec!["x".into()],
                    ..CodeSnapshot::default()
                },
                at: "t".into(),
            }),
            target: Some(Target {
                kind: TargetKind::Brief,
                section: Some("how.2".into()),
                quote: Some("same unit of work".into()),
                ..Target::default()
            }),
            answer: Some(Answer {
                status: AnswerStatus::Answering,
                by: Some("launcher".into()),
                ..Answer::default()
            }),
            ..Thread::default()
        };
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["anchorState"], "orphaned");
        assert_eq!(v["anchorHow"], "section-id");
        assert_eq!(v["dismissReason"], "out-of-scope");
        assert_eq!(v["orphan"]["reason"], "section-removed");
        assert_eq!(v["orphan"]["lastSeenSha"], "a1b2c3d");
        assert_eq!(v["orphan"]["snapshot"]["lines"], serde_json::json!([3, 4]));
        assert_eq!(v["target"]["kind"], "brief");
        assert_eq!(v["answer"]["status"], "answering");
        assert_eq!(v["position"]["line"], 1);
        assert_eq!(serde_json::from_value::<Thread>(v).unwrap(), t);
        assert!(t.is_artifact() && !t.is_open());
        assert_eq!(t.target_kind(), TargetKind::Brief);
    }

    #[test]
    fn anchor_state_keeps_outdated_in_step() {
        let mut threads: Vec<Thread> = serde_json::from_str(THREADS).unwrap();
        assert_eq!(threads[2].placement(), AnchorState::Outdated, "round-2 flag reads as outdated");
        let t = &mut threads[0];
        t.set_anchor_state(AnchorState::Orphaned, None);
        assert!(t.outdated);
        t.set_anchor_state(AnchorState::Moved, Some(AnchorHow::Exact));
        assert!(!t.outdated && t.placement() == AnchorState::Moved);
    }

    #[test]
    fn imports_carry_kind_target_answer_and_state() {
        let json = r#"{"type":"thread","filePath":"@brief","position":{"side":"new","line":1},"body":"why?","kind":"question","target":{"kind":"brief","section":"how.2"},"answer":{"status":"queued"},"state":"open"}"#;
        let imp: CommentImport = serde_json::from_str(json).unwrap();
        let b = imp.body();
        assert_eq!(b.kind, Some(ThreadKind::Question));
        assert_eq!(b.target.as_ref().unwrap().section.as_deref(), Some("how.2"));
        assert_eq!(b.answer.as_ref().unwrap().status, AnswerStatus::Queued);
        assert_eq!(b.state, Some(ThreadState::Open));
        assert_eq!(serde_json::to_string(&imp).unwrap(), json);
    }

    #[test]
    fn a_session_from_a_newer_build_with_unknown_variants_still_loads() {
        let json = r#"{"id":"x","future":{"a":1},"threads":[{"id":"t1","filePath":"a.cs","createdAt":"1","updatedAt":"1","position":{"side":"new","line":3},
            "messages":[{"id":"t1","body":"b","createdAt":"1","updatedAt":"1","mood":"calm"}],
            "kind":"riddle","state":"snoozed","disposition":"shrug","dismissReason":"vibes","anchorState":"drifting","anchorHow":"psychic",
            "orphan":{"reason":"moon-phase","snapshot":{"content":"c"}},"target":{"kind":"slide"},"answer":{"status":"pondering"}}]}"#;
        let s: crate::Session = serde_json::from_str(json).expect("unknown values never fail the session");
        let t = &s.threads[0];
        assert_eq!(
            (t.kind, t.state, t.disposition, t.dismiss_reason),
            (None, ThreadState::Open, None, None)
        );
        assert_eq!((t.anchor_state, t.anchor_how), (AnchorState::Placed, None));
        assert_eq!(t.orphan.as_ref().unwrap().reason, OrphanReason::Ambiguous);
        assert_eq!(t.target.as_ref().unwrap().kind, TargetKind::File);
        assert_eq!(t.answer.as_ref().unwrap().status, AnswerStatus::Cancelled);
        let known: Thread = serde_json::from_str(r#"{"id":"t","filePath":"a","createdAt":"1","updatedAt":"1","position":{"side":"new","line":1},"messages":[],"kind":"nit","state":"resolved"}"#).unwrap();
        assert_eq!((known.kind, known.state), (Some(ThreadKind::Nit), ThreadState::Resolved));
    }
}
