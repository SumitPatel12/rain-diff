//! The Conversation overlay (sections, filters, orphans), the agent's presence, `w`, and the
//! toasts that say what the agent did.

use std::collections::BTreeMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rain_comments::ops;
use rain_core::{Anchor, AnchorState, Disposition, Presence, PresenceState, Thread, ThreadKind, ThreadState};
use rain_diffview::{LineRef, Spot};
use rune::Tx;

use super::{App, ME, Overlay};

/// With no heartbeat for this long the agent reads as offline.
pub const OFFLINE_AFTER_SECS: u64 = 60;
/// Toasts kept at once; the oldest goes first.
pub const MAX_TOASTS: usize = 3;
/// How long a toast stays after it shows or the pointer leaves it.
pub const TOAST_FOR: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Section {
    Waiting,
    Addressed,
    Yours,
    Orphaned,
    Settled,
}

impl Section {
    pub const ALL: [Section; 5] = [
        Section::Waiting,
        Section::Addressed,
        Section::Yours,
        Section::Orphaned,
        Section::Settled,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Waiting => "WAITING ON YOU",
            Section::Addressed => "ADDRESSED? (confirm)",
            Section::Yours => "YOURS",
            Section::Orphaned => "ORPHANED",
            Section::Settled => "SETTLED",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Section::Waiting => "waiting",
            Section::Addressed => "addressed",
            Section::Yours => "yours",
            Section::Orphaned => "orphaned",
            Section::Settled => "settled",
        }
    }
}

fn is_agent(author: Option<&str>) -> bool {
    author.map(str::trim).is_some_and(|a| !a.is_empty() && !a.eq_ignore_ascii_case(ME))
}

/// Whether the agent opened the thread.
pub fn by_agent(t: &Thread) -> bool {
    t.messages.first().is_some_and(|m| is_agent(m.author.as_deref()))
}

/// The one section a thread sits in: settled first, then orphans, then what needs confirming,
/// then what waits on the user; everything else is the user's, waiting on the agent.
pub fn section_of(t: &Thread) -> Section {
    if matches!(t.state, ThreadState::Resolved | ThreadState::Dismissed)
        || matches!(t.disposition, Some(Disposition::Agree | Disposition::Dismiss))
    {
        return Section::Settled;
    }
    if t.placement() == AnchorState::Orphaned {
        return Section::Orphaned;
    }
    if t.state == ThreadState::Addressed || ops::addressed_suggested(t) {
        return Section::Addressed;
    }
    if ops::waiting_on_user(t, ME) {
        return Section::Waiting;
    }
    Section::Yours
}

/// An open question still waiting for its answer.
pub fn open_question(t: &Thread) -> bool {
    t.is_open() && t.kind() == ThreadKind::Question && section_of(t) == Section::Yours
}

/// The overlay's filter chips; all off shows everything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConvFilter {
    pub waiting: bool,
    pub agent: bool,
    pub mine: bool,
    pub state: Option<ThreadState>,
}

impl ConvFilter {
    pub fn keeps(&self, t: &Thread) -> bool {
        (!self.waiting || matches!(section_of(t), Section::Waiting | Section::Addressed))
            && (!self.agent || by_agent(t))
            && (!self.mine || !by_agent(t))
            && self.state.is_none_or(|s| t.state == s)
    }

    pub fn is_empty(&self) -> bool {
        *self == ConvFilter::default()
    }
}

/// What a toast's one button does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToastAction {
    Thread(String),
    Orphaned,
    /// Lets the local command answer questions (asked once).
    AllowAsk,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentToast {
    pub key: String,
    pub title: String,
    pub action: Option<ToastAction>,
    /// When it showed, or when the pointer last left it.
    pub since: Instant,
    pub hovered: bool,
}

impl AgentToast {
    /// Waits on the user (Allow) or on a running answer, so it stays until that ends.
    pub fn sticky(&self) -> bool {
        matches!(self.action, Some(ToastAction::AllowAsk)) || self.key.starts_with("ask-")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Seen {
    messages: usize,
    orphaned: bool,
}

#[derive(Default)]
pub struct ConversationState {
    pub filter: ConvFilter,
    /// Settled starts folded.
    pub settled_open: bool,
    /// The orphan whose `Close…` reasons are showing.
    pub closing: Option<String>,
    /// The agent's presence as of the last poll.
    pub presence: Presence,
    pub toasts: Vec<AgentToast>,
    /// Per thread, what the last poll saw; `None` until the first load.
    seen: Option<BTreeMap<String, Seen>>,
    seen_version: u64,
    /// The key list the last poll saw; a new allocation means a load installed.
    load_mark: usize,
    /// `w` passed the last waiting item; the next `w` starts over.
    w_wrapped: bool,
    /// The cursor a remote highlight left; moving it clears the spotlight.
    spot_cursor: Option<(usize, LineRef)>,
}

/// Seconds since `seen_at` (an ISO timestamp), at `now_ms`.
pub fn seen_ago(p: &Presence, now_ms: i64) -> Option<u64> {
    let then = millis_from_iso(p.seen_at.as_deref()?)?;
    Some(u64::try_from((now_ms - then).max(0) / 1000).unwrap_or(0))
}

/// The state to show: the reported one, or offline once the heartbeat is stale.
pub fn effective_state(p: &Presence, now_ms: i64) -> PresenceState {
    match seen_ago(p, now_ms) {
        Some(s) if s <= OFFLINE_AFTER_SECS => p.state,
        _ => PresenceState::Offline,
    }
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
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

/// `F1` from the thread's prefix, else `VoucherService.cs:27` for a line, else the opening words in
/// quotes. Never the id: it means nothing to the reader.
pub fn short_name(t: &Thread) -> String {
    let body = t.messages.first().map_or("", |m| m.body.as_str());
    let prefix = rain_core::comments::parse_prefix(body);
    if let Some(f) = prefix.as_ref().and_then(|p| p.finding) {
        return f.to_owned();
    }
    if t.target_kind() == rain_core::TargetKind::Line && !t.is_artifact() {
        let name = t.file_path.rsplit(['/', '\\']).next().unwrap_or(&t.file_path);
        return format!("{name}:{}", t.anchor.lines().0);
    }
    let words = rain_comments::label::first_sentence(prefix.as_ref().map_or(body, |p| p.rest), 40);
    if words.is_empty() {
        "a comment".to_owned()
    } else {
        format!("“{words}”")
    }
}

impl App {
    /// `C`: the Conversation overlay.
    pub fn toggle_conversation(&mut self, _tx: &mut Tx) {
        if self.toggle_overlay(Overlay::Conversation) {
            self.status = "Conversation · w next waiting · Esc closes".into();
        }
    }

    /// The threads of `section` the filter keeps, in reading order (file, then line).
    pub fn conversation_section(&self, section: Section) -> Vec<&Thread> {
        let mut out: Vec<&Thread> = self
            .mirror
            .threads
            .iter()
            .filter(|t| section_of(t) == section && self.conversation.filter.keeps(t))
            .collect();
        out.sort_by_key(|t| (self.file_of_thread(t).unwrap_or(usize::MAX), t.anchor.lines().0));
        out
    }

    /// What `w` walks: waiting threads, then `addressed?`, then open questions.
    pub fn waiting_walk(&self) -> Vec<String> {
        let mut all: Vec<&Thread> = self.mirror.threads.iter().collect();
        all.sort_by_key(|t| (self.file_of_thread(t).unwrap_or(usize::MAX), t.anchor.lines().0));
        let pick = |f: &dyn Fn(&Thread) -> bool| -> Vec<String> { all.iter().filter(|t| f(t)).map(|t| t.id.clone()).collect() };
        let mut ids = pick(&|t| section_of(t) == Section::Waiting);
        ids.extend(pick(&|t| section_of(t) == Section::Addressed));
        ids.extend(pick(&open_question));
        ids
    }

    /// `w`: the next item waiting on you after the focused one; past the last it says so and
    /// the next `w` starts over.
    pub fn next_waiting(&mut self, tx: &mut Tx) {
        let walk = self.waiting_walk();
        if walk.is_empty() {
            self.status = "Nothing waiting on you".into();
            return;
        }
        let at = match (&self.focused_thread, self.conversation.w_wrapped) {
            (_, true) => 0,
            (Some(f), false) => walk.iter().position(|id| id == f).map_or(0, |i| i + 1),
            (None, false) => 0,
        };
        self.conversation.w_wrapped = false;
        let Some(id) = walk.get(at).cloned() else {
            self.conversation.w_wrapped = true;
            self.status = format!("That was the last of {} waiting on you · w starts over", walk.len());
            return;
        };
        self.show_thread(&id, tx);
        if let Some(t) = ops::thread(&self.mirror, &id) {
            let what = match section_of(t) {
                Section::Addressed => "addressed? confirm with a",
                Section::Waiting => "waiting on you",
                _ => "open question",
            };
            self.status = format!("{}/{} · {} · {what}", at + 1, walk.len(), short_name(t));
        }
    }

    /// Jumps to a thread in the reader; one that sits on no line of the diff (an artifact or an
    /// orphan) is shown in the Conversation instead.
    pub fn show_thread(&mut self, id: &str, tx: &mut Tx) {
        let Some(t) = ops::thread(&self.mirror, id).cloned() else { return };
        let placed = t.placement() != AnchorState::Orphaned && !t.is_artifact() && self.file_of_thread(&t).is_some();
        if placed {
            self.close_overlay(Overlay::Conversation);
            self.goto_thread(id, tx);
        } else {
            self.focused_thread = Some(t.id.clone());
            self.open_overlay(Overlay::Conversation);
        }
    }

    pub fn toggle_conversation_filter(&mut self, which: &str) {
        let f = &mut self.conversation.filter;
        match which {
            "waiting" => f.waiting = !f.waiting,
            "agent" => {
                f.agent = !f.agent;
                f.mine &= !f.agent;
            }
            "mine" => {
                f.mine = !f.mine;
                f.agent &= !f.mine;
            }
            _ => {
                f.state = match f.state {
                    None => Some(ThreadState::Open),
                    Some(ThreadState::Open) => Some(ThreadState::Addressed),
                    Some(ThreadState::Addressed) => Some(ThreadState::Resolved),
                    Some(ThreadState::Resolved) => Some(ThreadState::Dismissed),
                    Some(ThreadState::Dismissed) => None,
                }
            }
        }
    }

    pub fn confirm_addressed(&mut self, id: &str) {
        self.mutate(|s| ops::confirm_addressed(s, id));
        self.status = "Confirmed addressed".into();
    }

    pub fn reopen_thread(&mut self, id: &str) {
        self.mutate(|s| ops::reopen(s, id));
        self.status = "Reopened".into();
    }

    /// Re-anchors an orphan on the cursor's line, taking that code as its new snapshot.
    pub fn attach_orphan_here(&mut self, id: &str) {
        let Some((local, at)) = self.active().cursor() else {
            self.status = "Put the cursor on a line in the reader, then Attach here".into();
            return;
        };
        let g = self.global(local);
        let Some(f) = self.diff.files().get(g) else { return };
        let anchor = Anchor::Line {
            side: at.side,
            line: at.line,
        };
        let path = f.path.clone();
        let snapshot = ops::snapshot_at(f, &anchor, &path);
        let repo = self.repo_tag(g).to_owned();
        let done = self.mutate(|s| {
            let Some(t) = s.threads.iter_mut().find(|t| t.id == id) else {
                return false;
            };
            t.original.get_or_insert(t.anchor);
            t.repo = repo;
            t.file_path = path;
            t.anchor = anchor;
            t.target = None;
            t.commit = None;
            if snapshot.is_some() {
                t.code_snapshot = snapshot;
            }
            t.set_anchor_state(AnchorState::Placed, None);
            t.updated_at = rain_comments::clock::now_iso();
            s.bump_comments();
            true
        });
        if done {
            self.conversation.closing = None;
            self.rebuild_annotations();
            self.status = format!("Attached to {}:{}", self.keys[g], at.line);
        }
    }

    /// `Close…` on an orphan: resolved with the reason as the last message; the orphan and its
    /// snapshot stay.
    pub fn close_orphan(&mut self, id: &str, reason: &str) {
        self.mutate(|s| {
            ops::reply(s, id, &format!("Closed: {reason}"), None);
            ops::resolve(s, id)
        });
        self.conversation.closing = None;
        self.status = format!("Closed · {reason}");
    }

    pub fn dismiss_toast(&mut self, key: &str) {
        self.conversation.toasts.retain(|t| t.key != key);
    }

    pub fn toast_action(&mut self, key: &str, tx: &mut Tx) {
        let Some(t) = self.conversation.toasts.iter().find(|t| t.key == key).cloned() else {
            return;
        };
        self.dismiss_toast(key);
        match t.action {
            Some(ToastAction::Thread(id)) => self.show_thread(&id, tx),
            Some(ToastAction::Orphaned) => {
                self.conversation.filter = ConvFilter::default();
                self.open_overlay(Overlay::Conversation);
                self.status = "Orphaned comments keep the code they were written on".into();
            }
            Some(ToastAction::AllowAsk) if !self.settings.ask.allowed => {
                self.apply_ask_item(super::ask::AskItem::Allow);
            }
            Some(ToastAction::AllowAsk) | None => {}
        }
    }

    /// Adds a toast, replacing one with the same key.
    pub fn push_toast(&mut self, key: String, title: String, action: Option<ToastAction>) {
        let toasts = &mut self.conversation.toasts;
        toasts.retain(|t| t.key != key);
        toasts.push(AgentToast {
            key,
            title,
            action,
            since: Instant::now(),
            hovered: false,
        });
        if toasts.len() > MAX_TOASTS {
            let oldest = toasts.iter().position(|t| !t.sticky()).unwrap_or(0);
            toasts.remove(oldest);
        }
    }

    /// The pointer entered or left a toast: it stays while hovered and for [`TOAST_FOR`] after.
    pub fn hover_toast(&mut self, key: &str, inside: bool) {
        if let Some(t) = self.conversation.toasts.iter_mut().find(|t| t.key == key) {
            t.hovered = inside;
            t.since = Instant::now();
        }
    }

    /// Drops toasts shown for [`TOAST_FOR`] at `now`; returns whether any went and when the next
    /// one is due.
    pub fn expire_toasts(&mut self, now: Instant) -> (bool, Option<Duration>) {
        let before = self.conversation.toasts.len();
        let live = |t: &AgentToast| t.sticky() || t.hovered || now.saturating_duration_since(t.since) < TOAST_FOR;
        self.conversation.toasts.retain(live);
        let next = self
            .conversation
            .toasts
            .iter()
            .filter(|t| !t.sticky() && !t.hovered)
            .map(|t| TOAST_FOR.saturating_sub(now.saturating_duration_since(t.since)))
            .min();
        (self.conversation.toasts.len() != before, next)
    }

    /// Every ready poll: presence, toasts for what changed, and the remote spotlight's end.
    pub(crate) fn conversation_tick(&mut self, tx: &mut Tx) -> bool {
        let mut changed = false;
        let p = self.shared.presence();
        if p != self.conversation.presence {
            self.conversation.presence = p;
            changed = true;
        }
        if let Some(ago) = seen_ago(&self.conversation.presence, now_ms())
            && ago <= OFFLINE_AFTER_SECS
        {
            tx.wake_after(Duration::from_secs(OFFLINE_AFTER_SECS + 1 - ago));
        }
        let mark = self.keys.as_ptr() as usize;
        if mark != self.conversation.load_mark {
            self.conversation.load_mark = mark;
            changed |= !self.conversation.toasts.is_empty();
            self.conversation.toasts.clear();
            self.conversation.seen_version = u64::MAX;
        }
        if self.conversation.seen_version != self.seen_version {
            self.conversation.seen_version = self.seen_version;
            changed |= self.toast_changes();
        }
        let (expired, next) = self.expire_toasts(Instant::now());
        changed |= expired;
        if let Some(d) = next {
            tx.wake_after(d + Duration::from_millis(20));
        }
        if let Some(at) = self.conversation.spot_cursor
            && self.active().cursor() != Some(at)
        {
            self.conversation.spot_cursor = None;
            self.active_mut().set_spotlight(None);
            changed = true;
        }
        changed
    }

    /// Toasts for agent replies and new agent threads, and one for anchors lost since the last
    /// look. The first look only takes the baseline.
    fn toast_changes(&mut self) -> bool {
        let now: BTreeMap<String, Seen> = self
            .mirror
            .threads
            .iter()
            .map(|t| {
                (
                    t.id.clone(),
                    Seen {
                        messages: t.messages.len(),
                        orphaned: t.placement() == AnchorState::Orphaned,
                    },
                )
            })
            .collect();
        let Some(before) = self.conversation.seen.replace(now.clone()) else {
            return false;
        };
        let mut toasts: Vec<(String, String, Option<ToastAction>)> = Vec::new();
        let (mut added, mut lost) = (0usize, 0usize);
        for t in &self.mirror.threads {
            let seen = now[&t.id];
            match before.get(&t.id) {
                None if by_agent(t) => added += 1,
                Some(b) if seen.messages > b.messages && t.messages.last().is_some_and(|m| is_agent(m.author.as_deref())) => {
                    let what = if t.kind() == ThreadKind::Question {
                        "answered"
                    } else {
                        "replied on"
                    };
                    toasts.push((
                        format!("reply-{}", t.id),
                        format!("Claude {what} {}", short_name(t)),
                        Some(ToastAction::Thread(t.id.clone())),
                    ));
                }
                _ => {}
            }
            if seen.orphaned && !before.get(&t.id).is_some_and(|b| b.orphaned) && before.contains_key(&t.id) {
                lost += 1;
            }
        }
        if added > 0 {
            let title = if added == 1 {
                "Claude added a comment".to_owned()
            } else {
                format!("Claude added {added} comments")
            };
            toasts.push((
                "added".into(),
                title,
                Some(ToastAction::Thread(self.waiting_walk().first().cloned().unwrap_or_default())),
            ));
        }
        if lost > 0 {
            let title = if lost == 1 {
                "1 comment lost its anchor".to_owned()
            } else {
                format!("{lost} comments lost their anchor")
            };
            toasts.push(("orphans".into(), title, Some(ToastAction::Orphaned)));
        }
        let any = !toasts.is_empty();
        for (k, t, a) in toasts {
            self.push_toast(k, t, a);
        }
        any
    }

    /// Spotlights `lines` of global file `g` and remembers the cursor, so moving clears it.
    pub(crate) fn spotlight(
        &mut self,
        g: usize,
        side: rain_core::Side,
        lines: (u32, u32),
        caption: Option<String>,
        step: Option<(u32, u32)>,
    ) {
        let Some(local) = self.local(g) else { return };
        self.active_mut().set_spotlight(Some(Spot {
            file: local,
            side,
            lines,
            caption,
            step,
        }));
        self.conversation.spot_cursor = self.active().cursor();
    }

    /// The presence line: `Claude working on F1 · 4 s ago`.
    pub fn presence_text(&self) -> String {
        let p = &self.conversation.presence;
        let now = now_ms();
        let state = effective_state(p, now);
        let on =
            p.on.as_deref()
                .map(|on| ops::thread(&self.mirror, on).map_or_else(|| "a comment".to_owned(), short_name));
        let what = match (state, on, p.reason.as_deref()) {
            (PresenceState::Working, Some(on), _) => format!("working on {on}"),
            (PresenceState::Blocked | PresenceState::Failed, _, Some(r)) => format!("{}: {r}", state.label()),
            (s, _, _) => s.label().to_owned(),
        };
        match seen_ago(p, now) {
            Some(ago) => format!("Claude {what} · {}", ago_label(ago)),
            None => format!("Claude {what}"),
        }
    }

    pub fn presence_state(&self) -> PresenceState {
        effective_state(&self.conversation.presence, now_ms())
    }

    /// The agent's latest message on any thread, `claude: <first sentence>`; `None` before it
    /// has said anything.
    pub fn latest_agent_message(&self) -> Option<String> {
        let m = self
            .mirror
            .threads
            .iter()
            .flat_map(|t| &t.messages)
            .filter(|m| m.author.as_deref().is_some_and(|a| !a.trim().is_empty()))
            .max_by(|a, b| a.created_at.cmp(&b.created_at))?;
        let who = m.author.as_deref().unwrap_or("agent");
        Some(format!("{who}: {}", rain_comments::label::first_sentence(&m.body, 140)))
    }
}

impl App {
    /// Scene data on top of `seed_threads`: an addressed finding, your open question being
    /// answered, an orphan with its old code, a settled nit, one finished round, and the agent
    /// working; the overlay open.
    pub fn seed_conversation(&mut self) {
        let Some(first) = self.mirror.threads.first().cloned() else {
            return;
        };
        let (repo, path, anchor) = (first.repo.clone(), first.file_path.clone(), first.anchor);
        let now = rain_comments::clock::now_iso();
        let q = self.mutate(|s| {
            if let Some(second) = s.threads.get(1).map(|t| t.id.clone()) {
                ops::set_state(s, &second, ThreadState::Addressed, Some("d4e5f6a"), None);
            }
            let q = ops::add_thread(
                s,
                &repo,
                &path,
                anchor,
                "Why isn't RefreshTotal inside the same unit of work?",
                None,
                None,
            );
            let orphan = ops::add_thread(
                s,
                &repo,
                &path,
                anchor,
                "N2 note: the total is summed here, before vouchers exist.",
                Some("claude"),
                None,
            );
            let nit = ops::add_thread(s, &repo, &path, anchor, "F4 nit: trailing space.", Some("claude"), None);
            ops::dismiss(s, &nit, rain_core::DismissReason::Style);
            for t in s.threads.iter_mut() {
                if t.id == q {
                    t.kind = Some(ThreadKind::Question);
                    t.answer = Some(rain_core::Answer {
                        status: rain_core::AnswerStatus::Answering,
                        by: Some("launcher".into()),
                        started_at: Some(now.clone()),
                        ..rain_core::Answer::default()
                    });
                }
                if t.id == orphan {
                    t.set_anchor_state(AnchorState::Orphaned, None);
                    t.orphan = Some(rain_core::Orphan {
                        reason: rain_core::OrphanReason::ContentRewritten,
                        last_seen_sha: Some("a1b2c3d".into()),
                        snapshot: rain_core::CodeSnapshot {
                            content: "var total = calc.Sum(payments);\nreturn total;".into(),
                            ..rain_core::CodeSnapshot::default()
                        },
                        at: now.clone(),
                    });
                }
            }
            s.rounds.push(rain_core::ReviewRound {
                n: 1,
                at: now.clone(),
                verdict: rain_core::Verdict::RequestChanges,
                summary: "Validate the amount and keep the voucher in the account's unit of work.".into(),
                ..rain_core::ReviewRound::default()
            });
            s.bump_comments();
            q
        });
        self.shared.set_presence(Presence {
            state: PresenceState::Working,
            on: Some(q),
            reason: None,
            seen_at: Some(now),
        });
        self.conversation.presence = self.shared.presence();
        self.open_overlay(Overlay::Conversation);
    }

    /// Scene data: the agent blocked, two toasts, the overlay closed.
    pub fn seed_presence(&mut self) {
        self.shared.set_presence(Presence {
            state: PresenceState::Blocked,
            on: None,
            reason: Some("needs the migration name".into()),
            seen_at: Some(rain_comments::clock::now_iso()),
        });
        self.conversation.presence = self.shared.presence();
        let first = self.mirror.threads.first().map(|t| (t.id.clone(), short_name(t)));
        if let Some((id, name)) = first {
            self.push_toast(
                format!("reply-{id}"),
                format!("Claude replied on {name}"),
                Some(ToastAction::Thread(id)),
            );
        }
        self.push_toast("orphans".into(), "2 comments lost their anchor".into(), Some(ToastAction::Orphaned));
        // The scene's own load would otherwise read as a reload and clear them.
        self.conversation.load_mark = self.keys.as_ptr() as usize;
        self.conversation.seen = Some(BTreeMap::new());
        self.conversation.seen_version = self.seen_version;
    }
}

fn ago_label(s: u64) -> String {
    match s {
        0..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} m ago", s / 60),
        _ => format!("{} h ago", s / 3600),
    }
}

#[cfg(test)]
mod tests {
    use rain_comments::clock::iso_from_millis;
    use rain_core::{Answer, AnswerStatus, CodeSnapshot, Orphan, OrphanReason, Side, UiRequest};

    use super::*;
    use crate::tests::{app, runtime, settle, story};

    /// Five threads, one per section, on the story's first file with added lines.
    fn seeded() -> (App, BTreeMap<Section, String>) {
        let mut a = app(story(), true);
        let g = (0..a.keys.len()).find(|&g| a.diff.files()[g].additions > 3).unwrap();
        let f = a.diff.files()[g].clone();
        let lines: Vec<u32> = f.lines.iter().filter(|l| l.new != 0).map(|l| l.new).collect();
        let (repo, path) = (a.repo_tag(g).to_owned(), f.path.clone());
        let at = |i: usize| Anchor::Line {
            side: Side::New,
            line: lines[i.min(lines.len() - 1)],
        };
        let ids = a.mutate(|s| {
            let add = |s: &mut rain_core::Session, i, body: &str, who| ops::add_thread(s, &repo, &path, at(i), body, who, None);
            let waiting = add(s, 0, "F1 issue (blocking): amount not validated.", Some("claude"));
            let addressed = add(s, 1, "F2 issue: totals skip vouchers.", Some("claude"));
            ops::set_state(s, &addressed, ThreadState::Addressed, Some("abc123"), None);
            let yours = add(s, 2, "Why not inside the unit of work?", None);
            s.threads.iter_mut().find(|t| t.id == yours).unwrap().kind = Some(ThreadKind::Question);
            s.threads.iter_mut().find(|t| t.id == yours).unwrap().answer = Some(Answer {
                status: AnswerStatus::Answering,
                ..Answer::default()
            });
            let orphan = add(s, 3, "N2 note: the sum moved.", Some("claude"));
            let t = s.threads.iter_mut().find(|t| t.id == orphan).unwrap();
            t.set_anchor_state(AnchorState::Orphaned, None);
            t.orphan = Some(Orphan {
                reason: OrphanReason::ContentRewritten,
                last_seen_sha: Some("a1b2c3d".into()),
                snapshot: CodeSnapshot {
                    content: "var total = calc.Sum(payments);".into(),
                    ..CodeSnapshot::default()
                },
                at: String::new(),
            });
            let settled = add(s, 4, "F3 nit: spacing.", Some("claude"));
            ops::resolve(s, &settled);
            BTreeMap::from([
                (Section::Waiting, waiting),
                (Section::Addressed, addressed),
                (Section::Yours, yours),
                (Section::Orphaned, orphan),
                (Section::Settled, settled),
            ])
        });
        (a, ids)
    }

    #[test]
    fn sections_hold_the_right_threads() {
        let (a, ids) = seeded();
        for s in Section::ALL {
            let got: Vec<&str> = a.conversation_section(s).iter().map(|t| t.id.as_str()).collect();
            assert_eq!(got, [ids[&s].as_str()], "{s:?}");
        }
    }

    #[test]
    fn filters_narrow_the_sections() {
        let (mut a, ids) = seeded();
        a.toggle_conversation_filter("mine");
        let kept: Vec<String> = Section::ALL
            .iter()
            .flat_map(|s| a.conversation_section(*s).into_iter().map(|t| t.id.clone()).collect::<Vec<_>>())
            .collect();
        assert_eq!(kept, [ids[&Section::Yours].clone()]);
        a.toggle_conversation_filter("agent");
        assert!(!a.conversation.filter.mine, "agent and mine exclude each other");
        a.conversation.filter = ConvFilter::default();
        a.toggle_conversation_filter("waiting");
        assert_eq!(a.conversation_section(Section::Waiting).len(), 1);
        assert!(a.conversation_section(Section::Settled).is_empty());
        a.conversation.filter = ConvFilter::default();
        for _ in 0..3 {
            a.toggle_conversation_filter("state");
        }
        assert_eq!(a.conversation.filter.state, Some(ThreadState::Resolved));
        assert_eq!(a.conversation_section(Section::Settled).len(), 1);
        assert!(a.conversation_section(Section::Waiting).is_empty());
    }

    #[test]
    fn w_walks_waiting_then_addressed_then_questions_and_says_when_done() {
        let (a, ids) = seeded();
        let mut rt = runtime(a);
        let want = [&ids[&Section::Waiting], &ids[&Section::Addressed], &ids[&Section::Yours]];
        assert_eq!(rt.state.waiting_walk(), want.map(String::clone));
        let w = |rt: &mut rune::Runtime<App>| {
            rt.update(|s, tx| {
                s.run(rain_core::Command::NextWaiting, tx);
            });
            settle(rt);
        };
        for id in want {
            w(&mut rt);
            assert_eq!(rt.state.focused_thread.as_deref(), Some(id.as_str()), "{}", rt.state.status);
        }
        assert!(rt.state.status.starts_with("3/3 · "), "{}", rt.state.status);
        w(&mut rt);
        assert_eq!(rt.state.status, "That was the last of 3 waiting on you · w starts over");
        w(&mut rt);
        assert_eq!(rt.state.focused_thread.as_deref(), Some(want[0].as_str()));

        let mut empty = runtime(app(story(), true));
        w(&mut empty);
        assert_eq!(empty.state.status, "Nothing waiting on you");
    }

    #[test]
    fn presence_follows_the_route_within_one_poll_and_goes_offline() {
        let mut rt = runtime(app(story(), true));
        assert_eq!(rt.state.presence_state(), PresenceState::Offline);
        let poll = |rt: &mut rune::Runtime<App>| {
            rt.update(|s, tx| {
                s.poll(tx);
            })
        };
        rt.state.shared.set_presence(Presence {
            state: PresenceState::Working,
            on: Some("k3x9f2".into()),
            reason: None,
            seen_at: Some(rain_comments::clock::now_iso()),
        });
        rt.state.shared.notify();
        poll(&mut rt);
        assert_eq!(rt.state.presence_state(), PresenceState::Working);
        assert!(
            rt.state.presence_text().starts_with("Claude working on a comment"),
            "{}",
            rt.state.presence_text()
        );

        rt.state.shared.set_presence(Presence {
            state: PresenceState::Blocked,
            reason: Some("needs a migration".into()),
            seen_at: Some(iso_from_millis(now_ms() - 61_000)),
            ..Presence::default()
        });
        poll(&mut rt);
        assert_eq!(rt.state.presence_state(), PresenceState::Offline);
        assert!(rt.state.presence_text().starts_with("Claude offline"));

        let p = Presence {
            state: PresenceState::Blocked,
            reason: Some("needs a migration".into()),
            seen_at: Some(iso_from_millis(now_ms() - 5_000)),
            ..Presence::default()
        };
        assert_eq!(effective_state(&p, now_ms()), PresenceState::Blocked);
        rt.state.shared.set_presence(p);
        poll(&mut rt);
        assert!(
            rt.state.presence_text().starts_with("Claude blocked: needs a migration · 5 s ago"),
            "{}",
            rt.state.presence_text()
        );
    }

    #[test]
    fn a_highlight_request_spotlights_the_range_and_names_it() {
        let mut rt = runtime(app(story(), true));
        let g = (0..rt.state.keys.len()).find(|&g| rt.state.diff.files()[g].additions > 3).unwrap();
        let key = rt.state.keys[g].clone();
        let lines: Vec<u32> = rt.state.diff.files()[g]
            .lines
            .iter()
            .filter(|l| l.new != 0)
            .map(|l| l.new)
            .collect();
        let (a, b) = (lines[0], lines[2]);
        rt.state.shared.push_request(UiRequest::Highlight {
            path: key.clone(),
            lines: (a, b),
            side: Side::New,
            caption: Some("The endpoint".into()),
            step: None,
        });
        settle(&mut rt);
        let spot = rt.state.active().spotlight().cloned().expect("spotlit");
        assert_eq!((spot.file, spot.lines), (rt.state.local(g).unwrap(), (a, b)));
        assert_eq!(rt.state.current(), Some(g));
        assert_eq!(rt.state.status, format!("Claude: look at {key}:{a}-{b} · The endpoint"));
        rt.update(|s, tx| {
            s.run(rain_core::Command::CursorDown, tx);
        });
        settle(&mut rt);
        assert!(rt.state.active().spotlight().is_none(), "moving the cursor ends the spotlight");
    }

    #[test]
    fn toasts_say_what_the_agent_did_dedupe_and_clear_on_reload() {
        let (a, ids) = seeded();
        let mut rt = runtime(a);
        assert!(rt.state.conversation.toasts.is_empty(), "the first look is the baseline");
        let id = ids[&Section::Waiting].clone();
        for body in ["One.", "Two."] {
            rt.state.mutate(|s| ops::reply(s, &id, body, Some("claude")));
            settle(&mut rt);
        }
        let keys: Vec<&str> = rt.state.conversation.toasts.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys, [format!("reply-{id}").as_str()], "two replies, one toast");
        assert_eq!(rt.state.conversation.toasts[0].title, "Claude replied on F1");

        let other = ids[&Section::Addressed].clone();
        rt.state.mutate(|s| {
            for t in s.threads.iter_mut().filter(|t| t.id == id || t.id == other) {
                t.set_anchor_state(AnchorState::Orphaned, None);
            }
            s.bump_comments();
        });
        settle(&mut rt);
        let t = rt
            .state
            .conversation
            .toasts
            .iter()
            .find(|t| t.key == "orphans")
            .expect("orphan toast");
        assert_eq!(t.title, "2 comments lost their anchor");
        rt.update(|s, tx| s.toast_action("orphans", tx));
        assert!(rt.state.layout.is_open(Overlay::Conversation));
        assert_eq!(rt.state.conversation_section(Section::Orphaned).len(), 3);

        rt.state.push_toast("x".into(), "Claude replied on F9".into(), None);
        rt.update(|s, _| s.reload());
        settle(&mut rt);
        assert!(rt.state.conversation.toasts.is_empty(), "a reload clears them");
    }

    #[test]
    fn toasts_leave_after_five_seconds_unless_hovered_or_waiting_on_you_and_stack_at_most_three() {
        let mut rt = runtime(app(story(), true));
        let s = &mut rt.state;
        s.push_toast("added".into(), "Claude added 5 comments".into(), None);
        s.push_toast("ask-allow".into(), "Allow?".into(), Some(ToastAction::AllowAsk));
        s.push_toast("ask-q1".into(), "Claude is answering…".into(), None);
        s.push_toast("reply-a".into(), "Claude replied on F1".into(), None);
        let keys = |s: &App| s.conversation.toasts.iter().map(|t| t.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys(s), ["ask-allow", "ask-q1", "reply-a"], "at most three, the oldest goes");
        s.push_toast("reply-b".into(), "Claude replied on F2".into(), None);
        assert_eq!(keys(s), ["ask-allow", "ask-q1", "reply-b"], "the oldest that waits on nothing goes");
        s.conversation.toasts.retain(|t| t.key != "ask-q1");
        s.push_toast("reply-c".into(), "Claude replied on F3".into(), None);
        s.hover_toast("reply-b", true);
        let (gone, next) = s.expire_toasts(Instant::now() + Duration::from_secs(1));
        assert!(!gone && next.is_some_and(|d| d <= TOAST_FOR), "{next:?}");
        let (gone, _) = s.expire_toasts(Instant::now() + TOAST_FOR + Duration::from_millis(100));
        assert!(gone);
        assert_eq!(keys(s), ["ask-allow", "reply-b"], "the Allow and the hovered one stay");
        s.hover_toast("reply-b", false);
        s.expire_toasts(Instant::now() + TOAST_FOR + Duration::from_millis(100));
        assert_eq!(keys(s), ["ask-allow"], "five seconds after the pointer left");
    }

    #[test]
    fn toasts_and_presence_name_threads_by_place_or_words_never_by_id() {
        let (a, ids) = seeded();
        let mut rt = runtime(a);
        let yours = ids[&Section::Yours].clone();
        let t = ops::thread(&rt.state.mirror, &yours).unwrap().clone();
        let name = t.file_path.rsplit('/').next().unwrap().to_owned();
        assert_eq!(short_name(&t), format!("{name}:{}", t.anchor.lines().0));
        rt.state
            .mutate(|s| ops::reply(s, &yours, "Because the save commits first.", Some("claude")));
        settle(&mut rt);
        let toast = rt
            .state
            .conversation
            .toasts
            .iter()
            .find(|t| t.key == format!("reply-{yours}"))
            .unwrap();
        assert_eq!(toast.title, format!("Claude answered {name}:{}", t.anchor.lines().0));
        assert!(!toast.title.contains(&yours[..6]));

        let mut brief = t.clone();
        brief.file_path = "@brief".into();
        brief.target = Some(rain_core::Target {
            kind: rain_core::TargetKind::Brief,
            section: Some("how.2".into()),
            ..rain_core::Target::default()
        });
        assert_eq!(short_name(&brief), "“Why not inside the unit of work?”");
        rt.state.shared.set_presence(Presence {
            state: PresenceState::Working,
            on: Some(yours.clone()),
            reason: None,
            seen_at: Some(rain_comments::clock::now_iso()),
        });
        rt.update(|s, tx| {
            s.poll(tx);
        });
        assert!(!rt.state.presence_text().contains(&yours), "{}", rt.state.presence_text());
    }

    #[test]
    fn orphans_attach_here_or_close_with_a_reason() {
        let (a, ids) = seeded();
        let mut rt = runtime(a);
        let orphan = ids[&Section::Orphaned].clone();
        rt.state.close_orphan(&orphan, "fixed elsewhere");
        let t = ops::thread(&rt.state.mirror, &orphan).unwrap();
        assert_eq!((t.state, section_of(t)), (ThreadState::Resolved, Section::Settled));
        assert!(t.orphan.is_some(), "the snapshot stays");
        assert_eq!(t.messages.last().unwrap().body, "Closed: fixed elsewhere");

        let (a, ids) = seeded();
        let mut rt = runtime(a);
        let orphan = ids[&Section::Orphaned].clone();
        rt.state.attach_orphan_here(&orphan);
        assert!(rt.state.status.starts_with("Put the cursor"));
        rt.update(|s, tx| {
            s.run(rain_core::Command::CursorDown, tx);
        });
        settle(&mut rt);
        let (local, at) = rt.state.active().cursor().unwrap();
        rt.state.attach_orphan_here(&orphan);
        let t = ops::thread(&rt.state.mirror, &orphan).unwrap();
        assert_eq!(t.placement(), AnchorState::Placed);
        assert_eq!(t.anchor.lines().0, at.line);
        assert_eq!(rt.state.file_of_thread(t), Some(rt.state.global(local)));
        assert_ne!(section_of(t), Section::Orphaned);
    }

    fn scene(name: &str) -> fn(&mut App) {
        crate::spec().scenes.iter().find(|(n, _)| *n == name).map(|(_, f)| *f).unwrap()
    }

    #[test]
    fn the_scenes_draw_the_overlay_presence_and_toasts_in_both_themes() {
        for size in [(1366.0, 768.0), (1920.0, 1080.0)] {
            for dark in [true, false] {
                let mut a = app(story(), true);
                scene("conversation")(&mut a);
                let mut rt = rune::Runtime::new(a, crate::view, size, rune::Env::new(dark, 1.0));
                rt.poll = Some(|s: &mut App, _, tx| s.poll(tx));
                rt.frame(0.0);
                settle(&mut rt);
                let panel = rt
                    .elements()
                    .into_iter()
                    .find(|e| e.key.as_deref() == Some("overlay-conversation"))
                    .expect("overlay");
                assert!((panel.rect[2] - 380.0).abs() < 1.0, "380 pt wide: {:?}", panel.rect);
                assert!(
                    panel.rect[0] + panel.rect[2] >= size.0 - 9.0,
                    "an island at the right edge: {:?}",
                    panel.rect
                );
                for key in [
                    "conv-section-waiting",
                    "conv-section-addressed",
                    "conv-section-yours",
                    "conv-section-orphaned",
                    "conv-round-1",
                ] {
                    assert!(rt.find(key).is_some(), "{key} at {size:?} dark {dark}");
                }
                let orphan = rt.state.conversation_section(Section::Orphaned)[0].id.clone();
                assert!(rt.find(&format!("conv-snapshot-{orphan}")).is_some());
                assert!(rt.find("presence-dot").is_some());
                assert_eq!(rt.state.presence_state(), PresenceState::Working);

                let mut a = app(story(), true);
                scene("presence")(&mut a);
                // Held as if hovered, so a slow run can't time them out: this checks that the
                // scene's load keeps them, not their five-second life.
                for t in &mut a.conversation.toasts {
                    t.hovered = true;
                }
                let mut rt = rune::Runtime::new(a, crate::view, size, rune::Env::new(dark, 1.0));
                rt.poll = Some(|s: &mut App, _, tx| s.poll(tx));
                rt.frame(0.0);
                settle(&mut rt);
                assert!(rt.find("toast-orphans").is_some(), "toasts survive the scene's load");
                assert!(rt.find("overlay-conversation").is_none());
                assert_eq!(rt.state.presence_state(), PresenceState::Blocked);
            }
        }
    }

    #[test]
    fn iso_round_trips() {
        for ms in [0, 951_782_400_000, 1_793_959_200_123] {
            assert_eq!(millis_from_iso(&iso_from_millis(ms)), Some(ms));
        }
    }
}
