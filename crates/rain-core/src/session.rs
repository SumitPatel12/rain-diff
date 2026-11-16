//! A review session: threads, viewed files, drafts and the overview, shared between the UI
//! thread and the HTTP routes.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::brief::BriefDoc;
use crate::comments::{Anchor, CodeSnapshot, DismissReason, Disposition, Draft, Thread, ThreadState, ViewedRecord};
use crate::diff::Side;
use crate::notes::Note;
use crate::overview::OverviewDocument;
use crate::tags::Group;

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReaderMode {
    /// Every file one under another.
    #[default]
    Stream,
    /// One item at a time.
    Single,
}

/// Split or unified as the user chose it; `Auto` picks per window width.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LayoutMode {
    Auto,
    #[default]
    Split,
    Unified,
}

/// Where the reviewer was, by file key and line rather than index, so it survives a reload.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReadingPlace {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub side: Side,
    pub scroll: f32,
    pub reader: ReaderMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chapter: Option<String>,
}

/// A group marked viewed without reading, with the reason logged for the review record.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AcceptedGroup {
    pub group: Group,
    pub files: Vec<String>,
    pub reason: String,
    pub at: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub undone: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Approve,
    RequestChanges,
    #[default]
    Comment,
}

/// A thread's outcome at the end of a round.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ThreadOutcome {
    pub thread: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disposition: Option<Disposition>,
    pub state: ThreadState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dismiss_reason: Option<DismissReason>,
}

/// One Finish review: what was decided against which shas.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReviewRound {
    pub n: u32,
    pub at: String,
    pub verdict: Verdict,
    pub summary: String,
    pub shas: Vec<SessionRepo>,
    pub viewed: Vec<ViewedRecord>,
    pub dispositions: Vec<ThreadOutcome>,
    pub questions_open: Vec<String>,
    pub drafts_unsent: usize,
    pub accepted: Vec<AcceptedGroup>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PresenceState {
    #[default]
    Offline,
    Idle,
    Working,
    Blocked,
    Done,
    Failed,
}

impl PresenceState {
    pub fn label(&self) -> &'static str {
        match self {
            PresenceState::Offline => "offline",
            PresenceState::Idle => "idle",
            PresenceState::Working => "working",
            PresenceState::Blocked => "blocked",
            PresenceState::Done => "done",
            PresenceState::Failed => "failed",
        }
    }
}

/// What the attached agent last said it was doing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Presence {
    pub state: PresenceState,
    /// The thread or file it is working on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seen_at: Option<String>,
}

/// Per-session layout the user chose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionUi {
    pub reader: ReaderMode,
    pub list_open: bool,
    pub zoomed: bool,
    pub brief_folded: bool,
}

impl Default for SessionUi {
    fn default() -> SessionUi {
        SessionUi {
            reader: ReaderMode::Stream,
            list_open: true,
            zoomed: false,
            brief_folded: false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionRepo {
    pub id: String,
    pub repository_id: String,
    pub base: String,
    pub target: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Session {
    pub id: String,
    pub repos: Vec<SessionRepo>,
    pub threads: Vec<Thread>,
    pub viewed: Vec<ViewedRecord>,
    pub drafts: Vec<Draft>,
    pub overview: Option<OverviewDocument>,
    pub self_repo: Option<String>,
    pub links: BTreeMap<String, String>,
    pub overview_version: u64,
    /// Bumped on every change, so readers can tell what they last saw.
    pub version: u64,
    /// Bumped only when threads change: the `version` of difit's comment routes, so a viewed mark
    /// between a client's read and write doesn't turn its replace into a merge.
    pub comments_version: u64,
    /// Agent notes; never part of comment readback.
    pub notes: Vec<Note>,
    pub notes_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brief: Option<BriefDoc>,
    #[serde(skip_serializing_if = "is_default")]
    pub brief_version: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rounds: Vec<ReviewRound>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<ReadingPlace>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub accepted: Vec<AcceptedGroup>,
    #[serde(skip_serializing_if = "is_default")]
    pub ui: SessionUi,
    /// The agent's presence as last seen; persisted so a reopened window can say when.
    #[serde(skip_serializing_if = "is_default")]
    pub presence: Presence,
    /// Verify by hand items the reviewer ticked, by their text.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub verified: Vec<String>,
}

impl Session {
    /// Records a change to the brief.
    pub fn bump_brief(&mut self) {
        self.version += 1;
        self.brief_version += 1;
    }

    /// The round the review is in: one past the last finished round.
    pub fn round(&self) -> u32 {
        self.rounds.last().map_or(1, |r| r.n + 1)
    }

    /// Records a change to the threads.
    pub fn bump_comments(&mut self) {
        self.version += 1;
        self.comments_version += 1;
    }

    /// Records a change to the notes, leaving `comments_version` alone.
    pub fn bump_notes(&mut self) {
        self.version += 1;
        self.notes_version += 1;
    }
}

/// Something another thread asks the open window to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiRequest {
    /// Show `path` (`<repo>/<path>` in multi-repo sessions), at `line` of `side` when given.
    Goto {
        path: String,
        line: Option<u32>,
        side: Side,
    },
    GotoThread(String),
    /// Reload the diff, keeping the reading place.
    Reload,
    /// Save and close the window, as WM_CLOSE does.
    Close,
    /// Spotlight `lines` of `path` in the reader; `step` is `(i, n)` when a tour drives it.
    Highlight {
        path: String,
        lines: (u32, u32),
        side: Side,
        caption: Option<String>,
        step: Option<(u32, u32)>,
    },
    /// Bring the window to the front.
    Focus,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventKind {
    #[default]
    Thread,
    Reply,
    Disposition,
    State,
    Viewed,
    Ask,
    Answer,
    Submitted,
    Reload,
    Presence,
    Brief,
    Notes,
    Overview,
}

/// One typed change, for `/api/events` readers that want deltas instead of a dump.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    /// Assigned by [`Shared::push_event`]; strictly increasing from 1.
    #[serde(default)]
    pub seq: u64,
    #[serde(default)]
    pub at: String,
    pub kind: EventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub data: serde_json::Value,
}

/// How many events [`Shared`] keeps; older ones drop off the front.
pub const EVENT_RING_CAP: usize = 500;

/// What the window shows, published for `GET /api/state`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct UiState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub side: Side,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<(u32, u32)>,
    pub mode: LayoutMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    pub zoomed: bool,
    pub reader: ReaderMode,
}

/// Questions the answering runtime is working on, for `health` and `/api/state`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskCounts {
    pub running: usize,
    pub queued: usize,
}

#[derive(Default)]
struct EventRing {
    seq: u64,
    buf: VecDeque<Event>,
}

/// `(repo, path, anchor)` → the anchored text of the loaded diff.
pub type ExcerptSource = Box<dyn Fn(&str, &str, &Anchor) -> Option<CodeSnapshot> + Send + Sync>;
/// A brief → the `check` object of `GET /api/brief`.
pub type BriefCheck = Box<dyn Fn(&BriefDoc) -> serde_json::Value + Send + Sync>;

/// One repo's diff as the window loaded it: repo-relative paths. Kept in memory, never saved.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoFiles {
    pub id: String,
    pub files: Vec<String>,
    pub generated: Vec<String>,
    pub formatting_only: Vec<String>,
}

type Wake = Box<dyn Fn() + Send + Sync>;

/// The session behind a mutex plus a hook that wakes the UI after another thread changes it.
#[derive(Default)]
pub struct Shared {
    pub session: Mutex<Session>,
    wake: Mutex<Option<Wake>>,
    requests: Mutex<Vec<UiRequest>>,
    changed: Condvar,
    diff: Mutex<Option<Vec<RepoFiles>>>,
    events: Mutex<EventRing>,
    event_added: Condvar,
    excerpt_source: Mutex<Option<ExcerptSource>>,
    brief_check: Mutex<Option<BriefCheck>>,
    ui_state: Mutex<UiState>,
    triage: Mutex<Option<serde_json::Value>>,
    asks: Mutex<AskCounts>,
    stale: std::sync::atomic::AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Shared {
    pub fn new(session: Session) -> Shared {
        Shared {
            session: Mutex::new(session),
            ..Shared::default()
        }
    }

    /// Queues a request for the UI thread and wakes it.
    pub fn push_request(&self, r: UiRequest) {
        self.requests.lock().unwrap_or_else(|e| e.into_inner()).push(r);
        self.notify();
    }

    /// The queued requests, oldest first, leaving the queue empty.
    pub fn take_requests(&self) -> Vec<UiRequest> {
        std::mem::take(&mut *self.requests.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// The session, recovering it if a panicking thread poisoned the lock.
    pub fn lock(&self) -> MutexGuard<'_, Session> {
        self.session.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The window publishes its diff's files here after each load, for the HTTP routes.
    pub fn set_diff_files(&self, repos: Vec<RepoFiles>) {
        *self.diff.lock().unwrap_or_else(|e| e.into_inner()) = Some(repos);
    }

    /// The files of the loaded diff per repo; `None` until the window has loaded one.
    pub fn diff_files(&self) -> Option<Vec<RepoFiles>> {
        self.diff.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Whether a load has published its file lists, without copying them.
    pub fn has_diff_files(&self) -> bool {
        self.diff.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    /// Runs `f` on the published file lists without copying them. Never take the session lock
    /// inside `f`: callers take that one first.
    pub fn with_diff_files<R>(&self, f: impl FnOnce(Option<&[RepoFiles]>) -> R) -> R {
        f(self.diff.lock().unwrap_or_else(|e| e.into_inner()).as_deref())
    }

    pub fn set_wake(&self, f: impl Fn() + Send + Sync + 'static) {
        *self.wake.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(f));
    }

    /// Wakes the UI and every [`Shared::wait_change`] caller.
    pub fn notify(&self) {
        self.notify_waiters();
        if let Some(f) = self.wake.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            f();
        }
    }

    /// Wakes [`Shared::wait_change`] callers only: for changes the UI thread made itself.
    pub fn notify_waiters(&self) {
        self.changed.notify_all();
    }

    /// Blocks until the session `version` is above `since` or `timeout` passes; the session as
    /// it is then (locked), and whether the version moved past `since`.
    pub fn wait_change(&self, since: u64, timeout: Duration) -> (MutexGuard<'_, Session>, bool) {
        let (s, _) = self
            .changed
            .wait_timeout_while(self.lock(), timeout, |s| s.version <= since)
            .unwrap_or_else(|e| e.into_inner());
        let moved = s.version > since;
        (s, moved)
    }

    /// Appends `e` with the next `seq` (overwriting the one given), drops the oldest past
    /// [`EVENT_RING_CAP`], wakes [`Shared::wait_events`] callers and returns the seq.
    pub fn push_event(&self, mut e: Event) -> u64 {
        let mut ring = lock(&self.events);
        ring.seq += 1;
        e.seq = ring.seq;
        ring.buf.push_back(e);
        while ring.buf.len() > EVENT_RING_CAP {
            ring.buf.pop_front();
        }
        let seq = ring.seq;
        drop(ring);
        self.event_added.notify_all();
        seq
    }

    /// The seq of the newest event; 0 before any.
    pub fn event_seq(&self) -> u64 {
        lock(&self.events).seq
    }

    /// Events after `since` whose kind is in `kinds` (every kind when empty), oldest first.
    pub fn events_since(&self, since: u64, kinds: &[EventKind]) -> Vec<Event> {
        lock(&self.events)
            .buf
            .iter()
            .filter(|e| e.seq > since && (kinds.is_empty() || kinds.contains(&e.kind)))
            .cloned()
            .collect()
    }

    /// Blocks until an event after `since` matches `keep` or `timeout` passes; the matches.
    pub fn wait_events(&self, since: u64, timeout: Duration, keep: impl Fn(&Event) -> bool) -> Vec<Event> {
        let pick = |r: &EventRing| -> Vec<Event> { r.buf.iter().filter(|e| e.seq > since && keep(e)).cloned().collect() };
        let (ring, _) = self
            .event_added
            .wait_timeout_while(lock(&self.events), timeout, |r| pick(r).is_empty())
            .unwrap_or_else(|e| e.into_inner());
        pick(&ring)
    }

    /// The window installs a reader of its loaded diff, so imports can snapshot at post time.
    pub fn set_excerpt_source(&self, f: ExcerptSource) {
        *lock(&self.excerpt_source) = Some(f);
    }

    pub fn has_excerpt_source(&self) -> bool {
        lock(&self.excerpt_source).is_some()
    }

    /// The anchored text of the loaded diff; `None` before a load or outside the diff.
    pub fn excerpt(&self, repo: &str, path: &str, anchor: &Anchor) -> Option<CodeSnapshot> {
        lock(&self.excerpt_source).as_ref().and_then(|f| f(repo, path, anchor))
    }

    pub fn set_brief_check(&self, f: BriefCheck) {
        *lock(&self.brief_check) = Some(f);
    }

    /// The installed check of `doc`, or an empty object.
    pub fn brief_check(&self, doc: &BriefDoc) -> serde_json::Value {
        lock(&self.brief_check)
            .as_ref()
            .map_or_else(|| serde_json::Value::Object(Default::default()), |f| f(doc))
    }

    pub fn set_ui_state(&self, s: UiState) {
        *lock(&self.ui_state) = s;
    }

    pub fn ui_state(&self) -> UiState {
        lock(&self.ui_state).clone()
    }

    /// The loader publishes `GET /api/triage`'s body here after each load.
    pub fn set_triage(&self, v: serde_json::Value) {
        *lock(&self.triage) = Some(v);
    }

    pub fn triage(&self) -> Option<serde_json::Value> {
        lock(&self.triage).clone()
    }

    pub fn set_asks(&self, c: AskCounts) {
        *lock(&self.asks) = c;
    }

    pub fn asks(&self) -> AskCounts {
        *lock(&self.asks)
    }

    /// HEAD moved since the diff was loaded (`health.stale`).
    pub fn set_stale(&self, stale: bool) {
        self.stale.store(stale, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn stale(&self) -> bool {
        self.stale.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn presence(&self) -> Presence {
        self.lock().presence.clone()
    }

    /// Stores the agent's presence without bumping `version`, so it wakes no comment waiter.
    pub fn set_presence(&self, p: Presence) {
        self.lock().presence = p;
    }

    /// Reads and writes the presence under one lock, so a heartbeat cannot undo a concurrent post.
    pub fn update_presence<R>(&self, f: impl FnOnce(&mut Presence) -> R) -> R {
        f(&mut self.lock().presence)
    }
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared").field("session", &self.session).finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn notify_calls_the_wake_hook() {
        let shared = Shared::default();
        shared.notify();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        shared.set_wake(move || {
            h.fetch_add(1, Ordering::SeqCst);
        });
        shared.lock().version += 1;
        shared.notify();
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(shared.lock().version, 1);
    }

    #[test]
    fn wait_change_returns_at_once_wakes_on_notify_and_times_out() {
        let shared = Arc::new(Shared::default());
        shared.lock().version = 3;
        let (s, moved) = shared.wait_change(2, Duration::from_secs(30));
        assert!(moved && s.version == 3);
        drop(s);

        let at = std::time::Instant::now();
        let (s, moved) = shared.wait_change(3, Duration::from_millis(50));
        drop(s);
        assert!(!moved && at.elapsed() >= Duration::from_millis(50));

        let other = shared.clone();
        let waiter = std::thread::spawn(move || {
            let (s, moved) = other.wait_change(3, Duration::from_secs(30));
            (s.version, moved)
        });
        std::thread::sleep(Duration::from_millis(50));
        shared.lock().version += 1;
        shared.notify_waiters();
        assert_eq!(waiter.join().unwrap(), (4, true));
    }

    #[test]
    fn session_round_trips() {
        let s = Session {
            id: "ACME-1".into(),
            version: 3,
            ..Session::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Session>(&json).unwrap(), s);
        assert_eq!(serde_json::from_str::<Session>("{}").unwrap(), Session::default());
    }

    #[test]
    fn session_with_notes_round_trips_and_bump_notes_spares_comments_version() {
        let mut s = Session::default();
        s.notes.push(crate::notes::Note {
            id: "n".into(),
            kind: crate::notes::NoteKind::Story,
            body: "b".into(),
            created_at: "t".into(),
            updated_at: "t".into(),
            ..crate::notes::Note::default()
        });
        s.bump_notes();
        assert_eq!((s.version, s.notes_version, s.comments_version), (1, 1, 0));
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Session>(&json).unwrap(), s);
    }

    #[test]
    fn requests_queue_in_order_and_wake_the_ui() {
        let shared = Shared::default();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        shared.set_wake(move || {
            h.fetch_add(1, Ordering::SeqCst);
        });
        shared.push_request(UiRequest::Goto {
            path: "server/a.cs".into(),
            line: Some(3),
            side: Side::New,
        });
        shared.push_request(UiRequest::GotoThread("t1".into()));
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        let got = shared.take_requests();
        assert_eq!(got.len(), 2);
        assert_eq!(got[1], UiRequest::GotoThread("t1".into()));
        assert!(shared.take_requests().is_empty());
        shared.push_request(UiRequest::Highlight {
            path: "server/a.cs".into(),
            lines: (3, 5),
            side: Side::New,
            caption: None,
            step: Some((2, 9)),
        });
        shared.push_request(UiRequest::Reload);
        assert_eq!(shared.take_requests()[1], UiRequest::Reload);
    }

    const ROUND2: &str = include_str!("../../../fixtures/sessions/round2.json");

    #[test]
    fn round_two_session_round_trips_byte_identically() {
        let s: Session = serde_json::from_str(ROUND2).unwrap();
        assert_eq!(s.threads.len(), 3);
        assert_eq!(s.threads[0].kind(), crate::comments::ThreadKind::Issue);
        assert!(s.threads[0].blocking());
        assert!(s.threads.iter().all(|t| t.state == ThreadState::Open));
        assert_eq!(s.threads[1].kind(), crate::comments::ThreadKind::Comment);
        assert_eq!(s.threads[2].placement(), crate::comments::AnchorState::Outdated);
        assert_eq!((s.ui.clone(), s.round()), (SessionUi::default(), 1));
        let back = String::from_utf8(serde_json::to_vec_pretty(&s).unwrap()).unwrap();
        assert_eq!(back, ROUND2.trim_end());
    }

    #[test]
    fn round_three_session_fields_round_trip() {
        let mut s = Session {
            id: "ACME-1".into(),
            ..Session::default()
        };
        s.brief = Some(BriefDoc {
            title: "t".into(),
            ..BriefDoc::default()
        });
        s.bump_brief();
        s.place = Some(ReadingPlace {
            file: "server/a.cs".into(),
            line: Some(4),
            scroll: 120.5,
            reader: ReaderMode::Single,
            ..ReadingPlace::default()
        });
        s.accepted.push(AcceptedGroup {
            group: Group::FormattingOnly,
            files: vec!["a".into()],
            reason: "1 formatting-only accepted without reading (-w empty)".into(),
            at: "t".into(),
            undone: false,
        });
        s.rounds.push(ReviewRound {
            n: 1,
            verdict: Verdict::RequestChanges,
            dispositions: vec![ThreadOutcome {
                thread: "t1".into(),
                disposition: Some(Disposition::Dismiss),
                state: ThreadState::Dismissed,
                dismiss_reason: Some(DismissReason::Style),
            }],
            ..ReviewRound::default()
        });
        s.ui.list_open = false;
        s.presence = Presence {
            state: PresenceState::Working,
            on: Some("F1".into()),
            ..Presence::default()
        };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["rounds"][0]["verdict"], "request-changes");
        assert_eq!(v["ui"]["listOpen"], false);
        assert_eq!(v["accepted"][0]["group"], "formatting-only");
        assert_eq!(serde_json::from_value::<Session>(v).unwrap(), s);
        assert_eq!((s.round(), s.brief_version, s.version), (2, 1, 1));
    }

    fn ev(kind: EventKind, author: Option<&str>) -> Event {
        Event {
            kind,
            author: author.map(str::to_owned),
            ..Event::default()
        }
    }

    #[test]
    fn events_filter_by_kind_and_since_and_cap_the_ring() {
        let shared = Shared::default();
        assert_eq!(shared.push_event(ev(EventKind::Thread, None)), 1);
        shared.push_event(ev(EventKind::Reply, Some("claude")));
        shared.push_event(ev(EventKind::Ask, None));
        let got = shared.events_since(0, &[EventKind::Reply, EventKind::Ask]);
        assert_eq!(got.iter().map(|e| e.seq).collect::<Vec<_>>(), [2, 3]);
        assert_eq!(shared.events_since(2, &[]).len(), 1);
        assert_eq!(shared.events_since(0, &[EventKind::Viewed]).len(), 0);
        for _ in 0..EVENT_RING_CAP {
            shared.push_event(ev(EventKind::Viewed, None));
        }
        let all = shared.events_since(0, &[]);
        assert_eq!(all.len(), EVENT_RING_CAP);
        assert_eq!(all[0].seq, 4, "the first three dropped off");
        assert_eq!(shared.event_seq(), 3 + EVENT_RING_CAP as u64);
        let v = serde_json::to_value(&all[0]).unwrap();
        assert_eq!(v, serde_json::json!({"seq":4,"at":"","kind":"viewed"}));
    }

    #[test]
    fn wait_events_skips_unwanted_events_and_times_out() {
        let shared = Arc::new(Shared::default());
        let other = shared.clone();
        let waiter = std::thread::spawn(move || {
            other.wait_events(0, Duration::from_secs(30), |e| {
                e.kind == EventKind::Reply && e.author.as_deref() != Some("claude")
            })
        });
        std::thread::sleep(Duration::from_millis(30));
        shared.push_event(ev(EventKind::Reply, Some("claude")));
        std::thread::sleep(Duration::from_millis(30));
        shared.push_event(ev(EventKind::Reply, None));
        let got = waiter.join().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].seq, 2);
        let at = std::time::Instant::now();
        assert!(shared.wait_events(2, Duration::from_millis(40), |_| true).is_empty());
        assert!(at.elapsed() >= Duration::from_millis(40));
    }

    #[test]
    fn hooks_default_empty_and_use_what_the_window_installs() {
        let shared = Shared::default();
        let a = Anchor::Line { side: Side::New, line: 2 };
        assert!(shared.excerpt("", "a.cs", &a).is_none());
        assert_eq!(shared.brief_check(&BriefDoc::default()), serde_json::json!({}));
        shared.set_excerpt_source(Box::new(|repo, path, a| {
            Some(CodeSnapshot {
                content: format!("{repo}/{path}:{}", a.end_line()),
                ..CodeSnapshot::default()
            })
        }));
        assert_eq!(shared.excerpt("server", "a.cs", &a).unwrap().content, "server/a.cs:2");
        shared.set_brief_check(Box::new(|d| serde_json::json!({"title": d.title})));
        assert_eq!(shared.brief_check(&BriefDoc::default())["title"], "");
        shared.set_ui_state(UiState {
            file: Some("a.cs".into()),
            zoomed: true,
            ..UiState::default()
        });
        assert!(shared.ui_state().zoomed);
        assert_eq!(serde_json::to_value(shared.ui_state()).unwrap()["mode"], "split");
        assert!(shared.triage().is_none());
        shared.set_triage(serde_json::json!({"shape":"dense"}));
        assert_eq!(shared.triage().unwrap()["shape"], "dense");
        shared.set_asks(AskCounts { running: 1, queued: 2 });
        assert_eq!(shared.asks().queued, 2);
        let before = shared.lock().version;
        shared.set_presence(Presence {
            state: PresenceState::Idle,
            ..Presence::default()
        });
        assert_eq!(shared.presence().state, PresenceState::Idle);
        assert_eq!(shared.lock().version, before);
    }
}
