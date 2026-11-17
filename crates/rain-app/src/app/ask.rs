//! Question mode's answering runtime. Every `ask` event becomes a pending answer: an idle
//! attached agent gets `handoffSec` to take it, otherwise (or after that) the local launcher
//! runs `ask.command` with the prompt on stdin and posts stdout as Claude's reply. Every step is
//! written to the thread's `answer`, so the card, the Conversation and the API all show it.

use std::io;
use std::time::{Duration, Instant};

use rain_ask::runner::{self, Job, Request, Subs};
use rain_ask::{Excerpt, Msg, PromptCtx, RepoShas};
use rain_comments::clock::now_iso;
use rain_comments::ops;
use rain_core::{Answer, AnswerStatus, AskCounts, EventKind, Presence, PresenceState, Side, TargetKind, Thread, ThreadKind, ThreadState};
use rune::Tx;
use serde_json::json;

use super::App;
use super::conversation::{now_ms, seen_ago, short_name};
use crate::settings::{AskPrefer, DEFAULT_ASK_COMMAND};

/// Starts a run of the answering command; tests swap in a fake.
pub type Spawner = Box<dyn FnMut(Request) -> io::Result<Box<dyn Job>>>;

/// A heartbeat this fresh means an agent is attached.
pub const ATTACHED_SECS: u64 = 15;
/// Diff lines quoted above and below the anchor.
const EXCERPT_CONTEXT: u32 = 12;
pub const NO_ANSWERER: &str = "no agent attached · set ask.command in the palette";
pub const NOT_ALLOWED: &str = "waiting for Allow (Ctrl+K › Ask: allow)";

enum Phase {
    /// An idle attached agent has until then to start on it.
    Handoff {
        until: Instant,
    },
    /// The attached agent said it is working on it.
    Attached,
    /// Waiting for a launcher slot.
    Waiting,
    /// No launcher configured and nobody attached: waits for an agent to reply.
    NoAnswerer,
    Running {
        job: Box<dyn Job>,
    },
}

struct Pending {
    thread: String,
    question: String,
    phase: Phase,
    /// A follow-up asked while this one ran: asked once it ends.
    again: Option<String>,
}

/// The launcher's own presence while it runs, and what was there before it.
struct OwnPresence {
    mine: Presence,
    before: Presence,
}

pub struct AskState {
    spawner: Option<Spawner>,
    pending: Vec<Pending>,
    seq: u64,
    started: bool,
    own: Option<OwnPresence>,
    counts: AskCounts,
}

impl Default for AskState {
    fn default() -> AskState {
        AskState {
            spawner: default_spawner(),
            pending: Vec::new(),
            seq: 0,
            started: false,
            own: None,
            counts: AskCounts::default(),
        }
    }
}

/// The real runner; test builds start with none, so no test can launch an agent by accident.
fn default_spawner() -> Option<Spawner> {
    if cfg!(test) {
        return None;
    }
    Some(Box::new(|r: Request| runner::spawn(r).map(|h| Box::new(h) as Box<dyn Job>)))
}

impl AskState {
    pub fn set_spawner(&mut self, s: Spawner) {
        self.spawner = Some(s);
    }

    pub fn counts(&self) -> AskCounts {
        self.counts
    }

    /// The queue position of `id` among questions waiting for a launcher slot (1-based).
    pub fn queue_position(&self, id: &str) -> Option<usize> {
        self.pending
            .iter()
            .filter(|p| matches!(p.phase, Phase::Waiting))
            .position(|p| p.thread == id)
            .map(|i| i + 1)
    }

    pub fn running(&self) -> Vec<&str> {
        self.pending
            .iter()
            .filter(|p| matches!(p.phase, Phase::Running { .. }))
            .map(|p| p.thread.as_str())
            .collect()
    }

    pub fn is_tracking(&self, id: &str) -> bool {
        self.pending.iter().any(|p| p.thread == id)
    }
}

impl Drop for AskState {
    fn drop(&mut self) {
        for p in &mut self.pending {
            if let Phase::Running { job } = &mut p.phase {
                job.cancel();
            }
        }
    }
}

/// Equal apart from `seen_at`, which the attached agent's heartbeat refreshes.
fn same_presence(a: &Presence, b: &Presence) -> bool {
    (a.state, &a.on, &a.reason) == (b.state, &b.on, &b.reason)
}

fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, x) if x != 11 => "st",
        (2, x) if x != 12 => "nd",
        (3, x) if x != 13 => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// The question a thread asks: its last message from the reviewer.
pub(crate) fn last_question(t: &Thread) -> String {
    t.messages
        .iter()
        .rev()
        .find(|m| m.author.as_deref().is_none_or(|a| a.trim().is_empty()))
        .map(|m| m.body.clone())
        .unwrap_or_default()
}

impl App {
    /// `?`: on a focused thread, the Ask disposition; elsewhere the composer in Question mode.
    pub fn ask_question(&mut self, tx: &mut Tx) {
        if self.target_thread().is_some() {
            self.dispose(rain_core::Disposition::Ask);
        } else {
            self.open_question(tx);
        }
    }

    /// Every ready poll: new asks, handoffs that ran out, finished runs.
    pub(crate) fn ask_poll(&mut self, tx: &mut Tx) -> bool {
        let changed = self.ask_tick_at(Instant::now());
        if let Some(d) = self.ask_next_wake(Instant::now()) {
            tx.wake_after(d);
        }
        changed
    }

    fn ask_next_wake(&self, now: Instant) -> Option<Duration> {
        let mut next: Option<Duration> = None;
        for p in &self.ask.pending {
            let d = match p.phase {
                Phase::Handoff { until } => until.saturating_duration_since(now).max(Duration::from_millis(50)),
                // The card's seconds counter, and runs whose end didn't wake the window.
                Phase::Running { .. } | Phase::Attached => Duration::from_secs(1),
                Phase::Waiting | Phase::NoAnswerer => continue,
            };
            next = Some(next.map_or(d, |n| n.min(d)));
        }
        next
    }

    /// One step of the state machine at `now`; true when anything visible changed.
    pub(crate) fn ask_tick_at(&mut self, now: Instant) -> bool {
        if self.ask.spawner.is_none() {
            return false;
        }
        let mut changed = false;
        if !self.ask.started {
            self.ask.started = true;
            changed |= self.fail_orphaned_runs();
            changed |= self.resume_waiting(now);
        }
        // Head first, then the events up to it, then the session: imports change the session
        // before they push their events, so every thread an event names is in the mirror.
        let head = self.shared.event_seq();
        let asks: Vec<_> = self
            .shared
            .events_since(self.ask.seq, &[EventKind::Ask])
            .into_iter()
            .filter(|e| e.seq <= head)
            .collect();
        self.ask.seq = self.ask.seq.max(head);
        changed |= self.sync_session();
        for e in asks {
            let Some(id) = e.thread.clone() else { continue };
            let question = e.data.get("question").and_then(|q| q.as_str()).unwrap_or_default().to_owned();
            changed |= self.take_ask(&id, question, now);
        }
        let mut i = 0;
        while i < self.ask.pending.len() {
            match self.step(i, now) {
                Some(c) => {
                    changed |= c;
                    i += 1;
                }
                None => {
                    self.ask.pending.remove(i);
                    changed = true;
                }
            }
        }
        changed |= self.launch_waiting();
        self.keep_presence();
        let counts = AskCounts {
            running: self.ask.running().len(),
            queued: self.ask.pending.len() - self.ask.running().len(),
        };
        if counts != self.ask.counts {
            self.ask.counts = counts;
            self.shared.set_asks(counts);
            changed = true;
        }
        changed
    }

    /// Answers a previous window left running can't finish now.
    fn fail_orphaned_runs(&mut self) -> bool {
        let stale: Vec<String> = self
            .mirror
            .threads
            .iter()
            .filter(|t| {
                t.answer
                    .as_ref()
                    .is_some_and(|a| a.status == AnswerStatus::Answering && a.by.as_deref() == Some("launcher"))
            })
            .map(|t| t.id.clone())
            .collect();
        for id in &stale {
            self.set_answer(id, |a| {
                a.status = AnswerStatus::Failed;
                a.error = Some("the window closed while answering".into());
                a.ended_at = Some(now_iso());
            });
        }
        !stale.is_empty()
    }

    /// Questions a previous window left queued or with the attached agent: asked again.
    fn resume_waiting(&mut self, now: Instant) -> bool {
        let waiting: Vec<String> = self
            .mirror
            .threads
            .iter()
            .filter(|t| matches!(t.state, ThreadState::Open | ThreadState::Addressed))
            .filter(|t| {
                t.answer.as_ref().is_some_and(|a| {
                    a.status == AnswerStatus::Queued || (a.status == AnswerStatus::Answering && a.by.as_deref() == Some("attached"))
                })
            })
            .map(|t| t.id.clone())
            .collect();
        let mut changed = false;
        for id in waiting {
            changed |= self.take_ask(&id, String::new(), now);
        }
        changed
    }

    fn set_answer(&mut self, id: &str, f: impl FnOnce(&mut Answer)) {
        let tid = id.to_owned();
        self.mutate(|s| {
            if let Some(t) = s.threads.iter_mut().find(|t| t.id == tid) {
                f(t.answer.get_or_insert_with(Answer::default));
                s.bump_comments();
            }
        });
    }

    fn answer_event(&self, id: &str, data: serde_json::Value) {
        self.push_ui_event(EventKind::Answer, id, data);
    }

    /// The attached agent's state when one is attached (fresh heartbeat, not the launcher's own).
    fn attached(&self) -> Option<Presence> {
        let p = self.shared.presence();
        if self.ask.own.as_ref().is_some_and(|o| same_presence(&o.mine, &p)) {
            return None;
        }
        let ago = seen_ago(&p, now_ms())?;
        (ago <= ATTACHED_SECS && p.state != PresenceState::Offline).then_some(p)
    }

    fn take_ask(&mut self, id: &str, question: String, now: Instant) -> bool {
        let Some(t) = ops::thread(&self.mirror, id).cloned() else {
            return false;
        };
        if let Some(p) = self.ask.pending.iter_mut().find(|p| p.thread == id) {
            if matches!(p.phase, Phase::Running { .. }) {
                p.again = Some(question);
                return true;
            }
            self.ask.pending.retain(|p| p.thread != id);
        }
        if t.answer.as_ref().is_none_or(|a| !a.status.pending()) {
            self.set_answer(id, |a| *a = Answer::default());
        }
        let question = if question.trim().is_empty() { last_question(&t) } else { question };
        let phase = match (self.settings.ask.prefer, self.attached()) {
            (AskPrefer::Attached, Some(p)) if p.state == PresenceState::Working && p.on.as_deref() == Some(id) => {
                self.mark_attached(id);
                Phase::Attached
            }
            (AskPrefer::Attached, Some(p)) if matches!(p.state, PresenceState::Idle | PresenceState::Done) => {
                self.status = format!(
                    "Asked the attached agent; the launcher takes over in {} s",
                    self.settings.ask.handoff_sec
                );
                Phase::Handoff {
                    until: now + Duration::from_secs(self.settings.ask.handoff_sec),
                }
            }
            _ => Phase::Waiting,
        };
        self.ask.pending.push(Pending {
            thread: id.to_owned(),
            question,
            phase,
            again: None,
        });
        true
    }

    fn mark_attached(&mut self, id: &str) {
        self.set_answer(id, |a| {
            a.status = AnswerStatus::Answering;
            a.by = Some("attached".into());
            a.started_at.get_or_insert_with(now_iso);
            a.error = None;
        });
        self.answer_event(id, json!({"status": "answering", "by": "attached"}));
    }

    /// Moves pending `i` along; `None` drops it.
    fn step(&mut self, i: usize, now: Instant) -> Option<bool> {
        let id = self.ask.pending[i].thread.clone();
        let t = ops::thread(&self.mirror, &id).cloned();
        let gone = t
            .as_ref()
            .is_none_or(|t| matches!(t.state, ThreadState::Resolved | ThreadState::Dismissed));
        let status = t.as_ref().and_then(|t| t.answer.as_ref()).map(|a| a.status);
        let running = matches!(self.ask.pending[i].phase, Phase::Running { .. });
        if gone || status == Some(AnswerStatus::Cancelled) {
            if let Phase::Running { job } = &mut self.ask.pending[i].phase {
                job.cancel();
            }
            self.dismiss_toast(&format!("ask-{id}"));
            return None;
        }
        if running {
            let Phase::Running { job } = &mut self.ask.pending[i].phase else {
                unreachable!()
            };
            let Some(out) = job.try_wait() else { return Some(false) };
            self.finish(&id, out);
            let q = self.ask.pending[i].again.take()?;
            let t = ops::thread(&self.mirror, &id)?.clone();
            self.ask.pending[i].question = if q.trim().is_empty() { last_question(&t) } else { q };
            self.ask.pending[i].phase = Phase::Waiting;
            self.set_answer(&id, |a| *a = Answer::default());
            return Some(true);
        }
        if status == Some(AnswerStatus::Answered) {
            return None;
        }
        let attached = self.attached();
        let working_on_it = attached
            .as_ref()
            .is_some_and(|p| p.state == PresenceState::Working && p.on.as_deref() == Some(id.as_str()));
        match self.ask.pending[i].phase {
            Phase::Handoff { until } => {
                if working_on_it {
                    self.mark_attached(&id);
                    self.ask.pending[i].phase = Phase::Attached;
                    return Some(true);
                }
                let still_idle = attached
                    .as_ref()
                    .is_some_and(|p| matches!(p.state, PresenceState::Idle | PresenceState::Done));
                if now >= until || !still_idle {
                    self.ask.pending[i].phase = Phase::Waiting;
                    return Some(true);
                }
                Some(false)
            }
            Phase::Attached => {
                if attached.is_none() {
                    self.ask.pending[i].phase = Phase::Waiting;
                    return Some(true);
                }
                Some(false)
            }
            Phase::NoAnswerer if self.launcher_ready() => {
                self.ask.pending[i].phase = Phase::Waiting;
                Some(true)
            }
            _ => Some(false),
        }
    }

    fn launcher_ready(&self) -> bool {
        let a = &self.settings.ask;
        a.enabled && a.allowed && !a.command.trim().is_empty()
    }

    /// Starts waiting questions while slots are free; the rest read their place in the queue.
    fn launch_waiting(&mut self) -> bool {
        let mut changed = false;
        let max = self.settings.ask.max_concurrent.max(1);
        let waiting: Vec<String> = self
            .ask
            .pending
            .iter()
            .filter(|p| matches!(p.phase, Phase::Waiting))
            .map(|p| p.thread.clone())
            .collect();
        for id in waiting {
            if !self.launcher_ready() {
                if let Some(p) = self.ask.pending.iter_mut().find(|p| p.thread == id) {
                    p.phase = Phase::NoAnswerer;
                }
                let a = &self.settings.ask;
                let why = if a.enabled && !a.command.trim().is_empty() && !a.allowed {
                    self.push_toast(
                        "ask-allow".into(),
                        "Questions run the local command (claude -p), which sends the excerpt to your Claude account".into(),
                        Some(super::conversation::ToastAction::AllowAsk),
                    );
                    NOT_ALLOWED
                } else {
                    self.status = format!("Waiting for an agent: {NO_ANSWERER}");
                    NO_ANSWERER
                };
                self.set_answer(&id, |a| {
                    a.status = AnswerStatus::Queued;
                    a.error = Some(why.into());
                });
                changed = true;
                continue;
            }
            if self.ask.running().len() < max {
                self.start(&id);
                changed = true;
                continue;
            }
            let note = self.ask.queue_position(&id).map(|n| ordinal(n + self.ask.running().len()));
            let had = ops::thread(&self.mirror, &id)
                .and_then(|t| t.answer.as_ref())
                .and_then(|a| a.error.clone());
            if had != note {
                self.set_answer(&id, |a| {
                    a.status = AnswerStatus::Queued;
                    a.error = note;
                });
                changed = true;
            }
        }
        changed
    }

    fn start(&mut self, id: &str) {
        let Some(t) = ops::thread(&self.mirror, id).cloned() else { return };
        let Some(i) = self.ask.pending.iter().position(|p| p.thread == id) else {
            return;
        };
        let question = self.ask.pending[i].question.clone();
        let prompt = self.ask_prompt(&t, &question);
        let session = if self.launch.session.is_empty() {
            self.mirror.id.clone()
        } else {
            self.launch.session.clone()
        };
        let subs = Subs {
            repos: self
                .repos
                .iter()
                .filter(|r| !r.dir.as_os_str().is_empty())
                .map(|r| r.dir.to_string_lossy().into_owned())
                .collect(),
            port: self.launch.port,
            session: session.clone(),
        };
        let template = if self.settings.ask.command.trim().is_empty() {
            DEFAULT_ASK_COMMAND.to_owned()
        } else {
            self.settings.ask.command.clone()
        };
        let mut env = vec![
            ("RAIN_SESSION".to_owned(), session.clone()),
            ("RAIN_THREAD".to_owned(), id.to_owned()),
        ];
        if let Some(p) = self.launch.port {
            env.push(("RAIN_PORT".into(), p.to_string()));
        }
        let safe = |s: &str| {
            s.chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
                .collect::<String>()
        };
        let req = Request {
            argv: rain_ask::redact::deny_reads(runner::argv(&template, &subs), &self.settings.ask.redact),
            cwd: self.repos.first().map(|r| r.dir.clone()).filter(|d| d.is_dir()),
            env,
            stdin: prompt,
            timeout: Duration::from_secs(self.settings.ask.timeout_sec.max(1)),
            log: Some(
                rain_comments::store::data_dir()
                    .join("logs")
                    .join(format!("ask-{}-{}.log", safe(&session), safe(id))),
            ),
            on_done: self.wake.clone(),
        };
        let spawned = match self.ask.spawner.as_mut() {
            Some(s) => s(req),
            None => return,
        };
        let name = short_name(&t);
        match spawned {
            Ok(job) => {
                self.ask.pending[i].phase = Phase::Running { job };
                self.set_answer(id, |a| {
                    a.status = AnswerStatus::Answering;
                    a.by = Some("launcher".into());
                    a.started_at = Some(now_iso());
                    a.ended_at = None;
                    a.error = None;
                });
                self.answer_event(id, json!({"status": "answering", "by": "launcher"}));
                self.push_toast(
                    format!("ask-{id}"),
                    format!("Claude is answering {name}…"),
                    Some(super::conversation::ToastAction::Thread(id.to_owned())),
                );
                self.status = format!("Claude is answering {name}…");
            }
            Err(e) => {
                self.ask.pending.remove(i);
                let msg = format!("could not start the ask command: {e}");
                self.set_answer(id, |a| {
                    a.status = AnswerStatus::Failed;
                    a.by = Some("launcher".into());
                    a.ended_at = Some(now_iso());
                    a.error = Some(msg.clone());
                });
                self.answer_event(id, json!({"status": "failed", "error": msg}));
                self.status = format!("Question {name}: {msg}");
            }
        }
    }

    fn finish(&mut self, id: &str, out: rain_ask::Outcome) {
        self.dismiss_toast(&format!("ask-{id}"));
        let name = ops::thread(&self.mirror, id).map(short_name).unwrap_or_default();
        if out.cancelled {
            return;
        }
        let end = now_iso();
        if out.answered() {
            let body = out.stdout.trim().to_owned();
            let tid = id.to_owned();
            let msg = self.mutate(|s| ops::reply(s, &tid, &body, Some("claude")));
            let first = ops::thread(&self.mirror, id)
                .and_then(|t| t.answer.as_ref())
                .is_none_or(|a| a.status != AnswerStatus::Answered || a.message_id == msg);
            if first {
                self.set_answer(id, |a| {
                    a.status = AnswerStatus::Answered;
                    a.by = Some("launcher".into());
                    a.ended_at = Some(end.clone());
                    a.error = None;
                    a.message_id.clone_from(&msg);
                });
            }
            self.answer_event(id, json!({"status": "answered", "by": "launcher", "messageId": msg}));
            self.status = format!("Answered {name}");
            return;
        }
        let (status, error) = if out.timed_out {
            (AnswerStatus::Timeout, format!("no answer in {} s", self.settings.ask.timeout_sec))
        } else {
            (AnswerStatus::Failed, out.error())
        };
        self.set_answer(id, |a| {
            a.status = status;
            a.ended_at = Some(end);
            a.error = Some(error.clone());
        });
        self.answer_event(id, json!({"status": status.label(), "error": error}));
        self.status = match status {
            AnswerStatus::Timeout => format!("Question {name} timed out · Retry on its card"),
            _ => format!("Question {name} failed: {error}"),
        };
    }

    /// Shows the launcher as the agent working on the running question; gives the dot back to
    /// what was there once nothing runs, unless an attached agent has spoken since.
    fn keep_presence(&mut self) {
        let running = self.ask.running().first().map(|s| (*s).to_owned());
        let own = self.ask.own.take();
        self.ask.own = self.shared.update_presence(|cur| match (running, own) {
            (Some(on), own) => {
                let before = match own {
                    Some(o) if same_presence(&o.mine, cur) => o.before,
                    // The attached agent posted its own state: leave the dot to it for this run.
                    Some(o) => return Some(o),
                    None => cur.clone(),
                };
                let mine = Presence {
                    state: PresenceState::Working,
                    on: Some(on),
                    reason: Some("answering".into()),
                    seen_at: Some(now_iso()),
                };
                *cur = mine.clone();
                Some(OwnPresence { mine, before })
            }
            (None, Some(o)) => {
                if same_presence(&o.mine, cur) {
                    let heard = cur.seen_at != o.mine.seen_at;
                    let seen_at = if heard { cur.seen_at.clone() } else { o.before.seen_at.clone() };
                    *cur = Presence { seen_at, ..o.before };
                }
                None
            }
            (None, None) => None,
        });
    }

    /// The prompt for `t`'s `question`, from what the window has loaded.
    fn ask_prompt(&self, t: &Thread, question: &str) -> String {
        let target = t.target.as_ref().map(|x| x.kind);
        let joined = rain_core::paths::join(&t.repo, &t.file_path);
        let artifact = matches!(
            target,
            Some(TargetKind::Brief | TargetKind::Node | TargetKind::Note | TargetKind::Pr)
        );
        let location = if artifact {
            App::thread_title(t)
        } else {
            rain_comments::anchor_label(&joined, &t.anchor)
        };
        let excerpt = if artifact {
            Excerpt::Artifact {
                title: App::thread_title(t),
                text: t.code_snapshot.as_ref().map(|c| c.content.clone()).unwrap_or_default(),
            }
        } else if rain_ask::redact::matches(&joined, &self.settings.ask.redact) {
            Excerpt::Withheld { path: joined.clone() }
        } else {
            let diff = self.file_of_thread(t).and_then(|g| {
                let f = self.diff.files().get(g)?;
                let side = if f.status == rain_core::FileStatus::Deleted {
                    Side::Old
                } else {
                    t.anchor.side()
                };
                rain_ask::excerpt::around(f, side, t.anchor.lines(), EXCERPT_CONTEXT)
            });
            Excerpt::Code {
                path: joined.clone(),
                snapshot: t.code_snapshot.as_ref().map(|c| c.content.clone()),
                diff,
            }
        };
        let mut earlier: Vec<Msg> = t
            .messages
            .iter()
            .map(|m| Msg {
                author: m.author.clone().unwrap_or_default(),
                body: m.body.clone(),
            })
            .collect();
        if let Some(i) = earlier
            .iter()
            .rposition(|m| m.author.trim().is_empty() && m.body.trim() == question.trim())
        {
            earlier.remove(i);
        }
        let brief = self.mirror.brief.as_ref();
        let title = brief
            .map(|b| b.title.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| self.launch.session.clone());
        let section = (target == Some(TargetKind::Brief))
            .then(|| t.target.as_ref().and_then(|x| x.section.clone()))
            .flatten();
        rain_ask::prompt::build(&PromptCtx {
            title,
            shas: self
                .repos
                .iter()
                .map(|r| RepoShas {
                    repo: r.id.clone(),
                    base: r.resolved_base.clone(),
                    target: r.resolved_target.clone(),
                })
                .collect(),
            brief,
            file: (!artifact).then_some(joined),
            section,
            location,
            excerpt,
            earlier,
            question: question.to_owned(),
        })
    }

    /// The window is closing: ends every run and marks its answer cancelled.
    pub fn ask_shutdown(&mut self) {
        let ids: Vec<String> = self.ask.running().iter().map(|s| (*s).to_owned()).collect();
        for p in &mut self.ask.pending {
            if let Phase::Running { job } = &mut p.phase {
                job.cancel();
            }
        }
        for id in ids {
            self.set_answer(&id, |a| {
                a.status = AnswerStatus::Cancelled;
                a.ended_at = Some(now_iso());
                a.error = Some("the window closed".into());
            });
        }
        self.ask.pending.clear();
        self.keep_presence();
    }

    /// The prompt Rain would send for thread `id` now (palette preview and tests).
    pub fn ask_prompt_for(&self, id: &str) -> Option<String> {
        let t = ops::thread(&self.mirror, id)?;
        Some(self.ask_prompt(t, &last_question(t)))
    }

    /// Scenes: a question the launcher is answering (12 s in) with a second one queued behind
    /// it, or (`answered`) the same question with Claude's reply.
    pub fn ask_scene(&mut self, answered: bool) {
        self.ask.started = true;
        crate::seed_threads(self);
        let Some(first) = self.mirror.threads.first().cloned() else {
            return;
        };
        let (repo, path, anchor) = (first.repo.clone(), first.file_path.clone(), first.anchor);
        let started = rain_comments::clock::iso_from_millis(rain_comments::ui::iso_millis(&now_iso()).unwrap_or(0) - 12_000);
        let question = ops::ThreadExtra {
            kind: Some(ThreadKind::Question),
            ..ops::ThreadExtra::default()
        };
        let snapshot = self.shared.excerpt(&repo, &path, &anchor);
        let q = self.mutate(|s| {
            let q = ops::add_thread_with(
                s,
                &repo,
                &path,
                anchor,
                "Why is the voucher stored before the account total is refreshed?",
                None,
                snapshot.clone(),
                question.clone(),
            );
            let second = (!answered).then(|| {
                ops::add_thread_with(
                    s,
                    &repo,
                    &path,
                    anchor,
                    "Is a negative voucher rejected anywhere else?",
                    None,
                    snapshot.clone(),
                    question.clone(),
                )
            });
            if answered {
                ops::reply(
                    s,
                    &q,
                    "The voucher is saved first so `RefreshTotal` reads it back inside the same unit of work \
                     (VoucherService.cs:27-31); refreshing first would total without the new voucher. Both writes \
                     commit together in `SaveAsync`, so a failure leaves neither.",
                    Some("claude"),
                );
            }
            for t in s.threads.iter_mut() {
                if t.id == q {
                    let a = t.answer.get_or_insert_with(Answer::default);
                    a.by = Some("launcher".into());
                    a.started_at = Some(started.clone());
                    if !answered {
                        a.status = AnswerStatus::Answering;
                    }
                } else if Some(&t.id) == second.as_ref() {
                    t.answer = Some(Answer {
                        status: AnswerStatus::Queued,
                        error: Some(ordinal(2)),
                        ..Answer::default()
                    });
                }
            }
            s.bump_comments();
            q
        });
        if !answered {
            let mine = Presence {
                state: PresenceState::Working,
                on: Some(q.clone()),
                reason: Some("answering".into()),
                seen_at: Some(now_iso()),
            };
            self.shared.set_presence(mine);
            self.conversation.presence = self.shared.presence();
            self.push_toast(
                format!("ask-{q}"),
                format!(
                    "Claude is answering {}…",
                    ops::thread(&self.mirror, &q).map(short_name).unwrap_or_default()
                ),
                None,
            );
        }
        self.expanded.insert(q.clone());
        self.focused_thread = Some(q.clone());
        self.rebuild_annotations();
        self.goto_later(format!("thread:{q}"));
    }

    /// Whether `id` is a question still waiting for its first answer.
    pub fn ask_open(&self, id: &str) -> bool {
        ops::thread(&self.mirror, id)
            .is_some_and(|t| t.kind() == ThreadKind::Question && t.answer.as_ref().is_some_and(|a| a.status.pending()))
    }
}

/// Palette rows for the ask settings; `query` lets `ask command: …` set the command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AskItem {
    Allow,
    Toggle,
    Prefer,
    Timeout(u64),
    EditCommand,
    SetCommand(String),
}

const TIMEOUTS: [u64; 4] = [60, 120, 300, 600];

pub(crate) fn palette_rows(s: &App, query: &str) -> Vec<(AskItem, String, String)> {
    let a = &s.settings.ask;
    let next_timeout = TIMEOUTS.iter().copied().find(|t| *t > a.timeout_sec).unwrap_or(TIMEOUTS[0]);
    let mut rows = vec![
        (
            AskItem::Allow,
            if a.allowed {
                "Ask: stop allowing the local command".to_owned()
            } else {
                "Ask: allow the local command to answer (sends the excerpt to your Claude account)".to_owned()
            },
            if a.allowed { "allowed".into() } else { "not allowed".into() },
        ),
        (
            AskItem::Toggle,
            "Ask: answer questions with the local command".to_owned(),
            if a.enabled { "on".into() } else { "off".into() },
        ),
        (
            AskItem::Prefer,
            match a.prefer {
                AskPrefer::Attached => "Ask: prefer the launcher over the attached agent".to_owned(),
                AskPrefer::Launcher => "Ask: prefer the attached agent over the launcher".to_owned(),
            },
            match a.prefer {
                AskPrefer::Attached => "now: attached first".into(),
                AskPrefer::Launcher => "now: launcher first".into(),
            },
        ),
        (
            AskItem::Timeout(next_timeout),
            format!("Ask: time out after {next_timeout} s"),
            format!("now {} s", a.timeout_sec),
        ),
        (
            AskItem::EditCommand,
            "Ask: edit the command".to_owned(),
            rain_comments::label::first_sentence(&a.command, 60),
        ),
    ];
    let q = query.trim();
    if let Some(rest) = q
        .get(..12)
        .filter(|p| p.eq_ignore_ascii_case("ask command:"))
        .map(|_| q[12..].trim())
        .filter(|r| !r.is_empty())
    {
        rows.push((
            AskItem::SetCommand(rest.to_owned()),
            format!("ask command: {rest}"),
            "Enter sets it".into(),
        ));
    }
    rows
}

impl App {
    pub(crate) fn apply_ask_item(&mut self, item: AskItem) {
        let a = &mut self.settings.ask;
        self.status = match item {
            AskItem::Allow => {
                a.allowed = !a.allowed;
                if a.allowed {
                    self.dismiss_toast("ask-allow");
                    "The local command may answer questions".into()
                } else {
                    "The local command no longer answers questions".into()
                }
            }
            AskItem::Toggle => {
                a.enabled = !a.enabled;
                format!("Answering with the local command: {}", if a.enabled { "on" } else { "off" })
            }
            AskItem::Prefer => {
                a.prefer = match a.prefer {
                    AskPrefer::Attached => AskPrefer::Launcher,
                    AskPrefer::Launcher => AskPrefer::Attached,
                };
                match a.prefer {
                    AskPrefer::Attached => "Questions go to the attached agent first".into(),
                    AskPrefer::Launcher => "Questions go to the local command at once".into(),
                }
            }
            AskItem::Timeout(t) => {
                a.timeout_sec = t;
                format!("Questions time out after {t} s")
            }
            AskItem::EditCommand => {
                let text = format!("ask command: {}", a.command);
                self.open_palette();
                let _ = self.palette.edit.paste(&text);
                "Edit the command, then Enter".into()
            }
            AskItem::SetCommand(c) => {
                a.command = c;
                format!("Ask command set: {}", a.command)
            }
        };
        self.save_settings();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinals_read_naturally() {
        let got: Vec<String> = [1, 2, 3, 4, 11, 12, 13, 21, 22].iter().map(|n| ordinal(*n)).collect();
        assert_eq!(got, ["1st", "2nd", "3rd", "4th", "11th", "12th", "13th", "21st", "22nd"]);
    }
}
