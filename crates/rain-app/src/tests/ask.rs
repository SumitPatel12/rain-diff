//! Question mode's answering runtime with the runner stubbed: who answers, when, and what the
//! thread says at each step.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rain_ask::runner::{Job, Outcome, Request};
use rain_comments::clock::{iso_from_millis, now_iso};
use rain_comments::{CardAction, ops};
use rain_core::{Answer, AnswerStatus, EventKind, Presence, PresenceState, Thread, ThreadKind};

use super::*;
use crate::app::ask::{AskItem, NO_ANSWERER};
use crate::settings::AskPrefer;

#[derive(Default)]
struct Ctl {
    reqs: Vec<Request>,
    outcomes: Vec<Option<Outcome>>,
    cancelled: Vec<bool>,
}

struct FakeJob {
    i: usize,
    ctl: Rc<RefCell<Ctl>>,
}

impl Job for FakeJob {
    fn try_wait(&mut self) -> Option<Outcome> {
        self.ctl.borrow().outcomes[self.i].clone()
    }

    fn cancel(&mut self) {
        let mut c = self.ctl.borrow_mut();
        c.cancelled[self.i] = true;
        c.outcomes[self.i] = Some(Outcome {
            cancelled: true,
            ..Outcome::default()
        });
    }

    fn pid(&self) -> Option<u32> {
        None
    }
}

fn stub(a: &mut App) -> Rc<RefCell<Ctl>> {
    let ctl = Rc::new(RefCell::new(Ctl::default()));
    let c = ctl.clone();
    a.ask.set_spawner(Box::new(move |r: Request| {
        let mut x = c.borrow_mut();
        x.reqs.push(r);
        x.outcomes.push(None);
        x.cancelled.push(false);
        Ok(Box::new(FakeJob {
            i: x.reqs.len() - 1,
            ctl: c.clone(),
        }) as Box<dyn Job>)
    }));
    ctl
}

fn answer(ctl: &Rc<RefCell<Ctl>>, i: usize, o: Outcome) {
    ctl.borrow_mut().outcomes[i] = Some(o);
}

fn ok(text: &str) -> Outcome {
    Outcome {
        code: Some(0),
        stdout: text.into(),
        ..Outcome::default()
    }
}

fn setup() -> (App, Rc<RefCell<Ctl>>) {
    let mut a = app(story(), true);
    seed_threads(&mut a);
    a.settings.ask.allowed = true;
    let ctl = stub(&mut a);
    (a, ctl)
}

/// A question on the seeded file's first comment line, asked the way the composer sends one.
fn ask(a: &mut App, body: &str) -> String {
    let first = a.mirror.threads[0].clone();
    let snapshot = a.shared.excerpt(&first.repo, &first.file_path, &first.anchor);
    let extra = ops::ThreadExtra {
        kind: Some(ThreadKind::Question),
        ..ops::ThreadExtra::default()
    };
    let id = a.mutate(|s| ops::add_thread_with(s, &first.repo, &first.file_path, first.anchor, body, None, snapshot, extra));
    a.ask_agent(&id, body);
    id
}

fn thread(a: &App, id: &str) -> Thread {
    ops::thread(&a.mirror, id).cloned().unwrap()
}

fn status(a: &App, id: &str) -> Answer {
    thread(a, id).answer.unwrap()
}

fn presence(a: &App, state: PresenceState, on: Option<&str>) {
    a.shared.set_presence(Presence {
        state,
        on: on.map(str::to_owned),
        reason: None,
        seen_at: Some(now_iso()),
    });
}

#[test]
fn test_builds_never_start_a_runner_by_themselves() {
    let mut a = app(story(), true);
    seed_threads(&mut a);
    let id = ask(&mut a, "Why?");
    assert!(!a.ask_tick_at(Instant::now()));
    assert_eq!(status(&a, &id).status, AnswerStatus::Queued);
    assert!(!a.ask.is_tracking(&id));
}

#[test]
fn a_read_only_window_does_not_resume_the_owners_questions() {
    let (mut a, ctl) = setup();
    a.launch.read_only = true;
    let (queued, running) = (a.mirror.threads[0].id.clone(), a.mirror.threads[1].id.clone());
    a.mutate(|s| {
        s.threads[0].answer = Some(Answer::default());
        s.threads[1].answer = Some(Answer {
            status: AnswerStatus::Answering,
            by: Some("launcher".into()),
            ..Answer::default()
        });
        s.bump_comments();
    });
    let before = (status(&a, &queued), status(&a, &running));
    assert_eq!(before.0.status, AnswerStatus::Queued);
    a.ask_tick_at(Instant::now());
    assert!(ctl.borrow().reqs.is_empty(), "the owner's queued question is not asked from here");
    assert_eq!((status(&a, &queued), status(&a, &running)), before);
    assert!(!a.ask.is_tracking(&queued) && !a.ask.is_tracking(&running));
}

#[test]
fn a_question_asked_in_a_read_only_window_is_answered() {
    let (mut a, ctl) = setup();
    a.launch.read_only = true;
    let now = Instant::now();
    a.ask_tick_at(now);
    let id = ask(&mut a, "Why?");
    assert!(a.ask_tick_at(now + Duration::from_secs(1)));
    assert_eq!(ctl.borrow().reqs.len(), 1);
    answer(&ctl, 0, ok("Because."));
    assert!(a.ask_tick_at(now + Duration::from_secs(2)));
    let t = thread(&a, &id);
    assert_eq!(t.answer.unwrap().status, AnswerStatus::Answered);
    assert_eq!(t.messages.last().unwrap().body, "Because.");
}

#[test]
fn with_no_agent_attached_the_launcher_answers_at_once() {
    let (mut a, ctl) = setup();
    let id = ask(&mut a, "Why is the voucher stored first?");
    let now = Instant::now();
    assert!(a.ask_tick_at(now));
    assert_eq!(ctl.borrow().reqs.len(), 1);
    let ans = status(&a, &id);
    assert_eq!(ans.status, AnswerStatus::Answering);
    assert_eq!(ans.by.as_deref(), Some("launcher"));
    assert_eq!(a.shared.asks().running, 1);
    {
        let c = ctl.borrow();
        let r = &c.reqs[0];
        assert_eq!(r.argv[0], "claude");
        assert!(r.argv.contains(&"--add-dir".to_owned()));
        assert!(
            r.cwd.as_ref().is_some_and(|d| d.ends_with("run")),
            "runs in Rain's own folder, never the repo: {:?}",
            r.cwd
        );
        assert!(r.stdin.contains("Why is the voucher stored first?"));
        assert!(r.stdin.starts_with(rain_ask::prompt::PREAMBLE));
        assert!(r.env.contains(&("RAIN_THREAD".to_owned(), id.clone())));
        assert_eq!(r.timeout, Duration::from_secs(120));
        let log = r.log.as_ref().unwrap().to_string_lossy().into_owned();
        assert!(log.contains("logs") && log.ends_with(&format!("{id}.log")), "{log}");
    }

    answer(&ctl, 0, ok("Because RefreshTotal reads it back.\n"));
    assert!(a.ask_tick_at(now + Duration::from_secs(3)));
    let t = thread(&a, &id);
    let last = t.messages.last().unwrap();
    assert_eq!(last.author.as_deref(), Some("claude"));
    assert_eq!(last.body, "Because RefreshTotal reads it back.");
    let ans = t.answer.unwrap();
    assert_eq!(ans.status, AnswerStatus::Answered);
    assert_eq!(ans.by.as_deref(), Some("launcher"));
    assert_eq!(ans.message_id.as_deref(), Some(last.id.as_str()));
    assert_eq!(a.shared.asks().running, 0);
    let events = a.shared.events_since(0, &[EventKind::Answer]);
    assert_eq!(events.last().unwrap().data["status"], "answered");
    assert!(a.status.starts_with("Answered"), "{}", a.status);
    assert!(
        a.conversation_section(crate::app::conversation::Section::Waiting)
            .iter()
            .any(|t| t.id == id)
    );
}

#[test]
fn the_window_poll_drives_the_runtime() {
    let (a, ctl) = setup();
    let mut rt = runtime(a);
    rt.update(|s, _| {
        ask(s, "Through the poll?");
    });
    settle(&mut rt);
    assert_eq!(ctl.borrow().reqs.len(), 1);
    answer(&ctl, 0, ok("Yes."));
    settle(&mut rt);
    let id = rt
        .state
        .mirror
        .threads
        .iter()
        .find(|t| t.messages[0].body == "Through the poll?")
        .unwrap()
        .id
        .clone();
    assert_eq!(status(&rt.state, &id).status, AnswerStatus::Answered);
}

#[test]
fn an_idle_attached_agent_gets_the_handoff_then_the_launcher_takes_over() {
    let (mut a, ctl) = setup();
    presence(&a, PresenceState::Idle, None);
    let id = ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    assert!(ctl.borrow().reqs.is_empty(), "the attached agent goes first");
    assert_eq!(status(&a, &id).status, AnswerStatus::Queued);
    presence(&a, PresenceState::Idle, None);
    a.ask_tick_at(now + Duration::from_secs(19));
    assert!(ctl.borrow().reqs.is_empty());
    presence(&a, PresenceState::Idle, None);
    a.ask_tick_at(now + Duration::from_secs(21));
    assert_eq!(ctl.borrow().reqs.len(), 1, "after handoffSec the launcher runs");
    assert_eq!(status(&a, &id).by.as_deref(), Some("launcher"));
}

#[test]
fn an_attached_agent_that_takes_the_question_answers_it() {
    let (mut a, ctl) = setup();
    presence(&a, PresenceState::Idle, None);
    let id = ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    presence(&a, PresenceState::Working, Some(&id));
    a.ask_tick_at(now + Duration::from_secs(2));
    let ans = status(&a, &id);
    assert_eq!(ans.status, AnswerStatus::Answering);
    assert_eq!(ans.by.as_deref(), Some("attached"));
    a.ask_tick_at(now + Duration::from_secs(40));
    assert!(ctl.borrow().reqs.is_empty(), "no launcher while the agent works on it");
    let tid = id.clone();
    a.mutate(|s| ops::reply(s, &tid, "It is inside the unit of work.", Some("claude")));
    a.ask_tick_at(now + Duration::from_secs(41));
    assert_eq!(status(&a, &id).status, AnswerStatus::Answered);
    assert!(!a.ask.is_tracking(&id));
    assert!(ctl.borrow().reqs.is_empty());
}

#[test]
fn a_busy_or_blocked_attached_agent_sends_the_question_to_the_launcher_at_once() {
    for (state, on) in [(PresenceState::Working, Some("other")), (PresenceState::Blocked, None)] {
        let (mut a, ctl) = setup();
        presence(&a, state, on);
        ask(&mut a, "Why?");
        a.ask_tick_at(Instant::now());
        assert_eq!(ctl.borrow().reqs.len(), 1, "{state:?}");
    }
    let (mut a, ctl) = setup();
    a.settings.ask.prefer = AskPrefer::Launcher;
    presence(&a, PresenceState::Idle, None);
    ask(&mut a, "Why?");
    a.ask_tick_at(Instant::now());
    assert_eq!(ctl.borrow().reqs.len(), 1, "prefer launcher skips the handoff");
}

#[test]
fn a_stale_heartbeat_is_not_an_attached_agent() {
    let (mut a, ctl) = setup();
    a.shared.set_presence(Presence {
        state: PresenceState::Idle,
        seen_at: Some(iso_from_millis(crate::app::conversation::now_ms() - 30_000)),
        ..Presence::default()
    });
    ask(&mut a, "Why?");
    a.ask_tick_at(Instant::now());
    assert_eq!(ctl.borrow().reqs.len(), 1);
}

#[test]
fn max_concurrent_queues_the_second_question() {
    let (mut a, ctl) = setup();
    let one = ask(&mut a, "First?");
    let two = ask(&mut a, "Second?");
    let now = Instant::now();
    a.ask_tick_at(now);
    assert_eq!(ctl.borrow().reqs.len(), 1);
    let q = status(&a, &two);
    assert_eq!(q.status, AnswerStatus::Queued);
    assert_eq!(q.error.as_deref(), Some("2nd"));
    assert_eq!(
        rain_comments::ui::answer_text(&thread(&a, &two), None).as_deref(),
        Some("queued · 2nd")
    );
    assert_eq!(a.shared.asks().running, 1);
    assert_eq!(a.shared.asks().queued, 1);
    answer(&ctl, 0, ok("One."));
    a.ask_tick_at(now + Duration::from_secs(1));
    assert_eq!(status(&a, &one).status, AnswerStatus::Answered);
    assert_eq!(ctl.borrow().reqs.len(), 2, "the second starts when the first ends");
    assert_eq!(status(&a, &two).status, AnswerStatus::Answering);
    assert!(status(&a, &two).error.is_none());
}

#[test]
fn a_failed_run_shows_the_exit_and_stderr_head_and_retry_asks_again() {
    let (mut a, ctl) = setup();
    let id = ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    answer(
        &ctl,
        0,
        Outcome {
            code: Some(3),
            stderr: format!("not logged in {}", "x".repeat(400)),
            ..Outcome::default()
        },
    );
    a.ask_tick_at(now + Duration::from_secs(1));
    let ans = status(&a, &id);
    assert_eq!(ans.status, AnswerStatus::Failed);
    let err = ans.error.unwrap();
    assert!(err.starts_with("exit 3: not logged in"), "{err}");
    assert!(err.chars().count() <= "exit 3: ".len() + 200);
    assert!(a.status.contains("failed"), "{}", a.status);
    assert_eq!(thread(&a, &id).messages.len(), 1, "no reply posted");

    let mut tx = Tx::default();
    a.card_action(&id, CardAction::Retry, &mut tx);
    a.ask_tick_at(now + Duration::from_secs(2));
    assert_eq!(ctl.borrow().reqs.len(), 2);
    assert!(ctl.borrow().reqs[1].stdin.contains("Why?"));
    assert_eq!(status(&a, &id).status, AnswerStatus::Answering);
}

#[test]
fn a_timeout_reads_timed_out_and_an_empty_answer_fails() {
    let (mut a, ctl) = setup();
    let id = ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    answer(
        &ctl,
        0,
        Outcome {
            timed_out: true,
            ..Outcome::default()
        },
    );
    a.ask_tick_at(now + Duration::from_secs(1));
    assert_eq!(status(&a, &id).status, AnswerStatus::Timeout);
    assert!(a.status.contains("timed out"), "{}", a.status);

    let two = ask(&mut a, "Again?");
    a.ask_tick_at(now + Duration::from_secs(2));
    answer(&ctl, 1, ok("   \n"));
    a.ask_tick_at(now + Duration::from_secs(3));
    let ans = status(&a, &two);
    assert_eq!(ans.status, AnswerStatus::Failed);
    assert_eq!(ans.error.as_deref(), Some("exit 0 with an empty answer"));
}

#[test]
fn cancel_on_the_card_ends_the_run() {
    let (mut a, ctl) = setup();
    let id = ask(&mut a, "Why?");
    a.ask_tick_at(Instant::now());
    let mut tx = Tx::default();
    a.card_action(&id, CardAction::CancelAnswer, &mut tx);
    a.ask_tick_at(Instant::now());
    assert!(ctl.borrow().cancelled[0]);
    assert_eq!(status(&a, &id).status, AnswerStatus::Cancelled);
    assert!(!a.ask.is_tracking(&id));
    assert_eq!(a.shared.asks().running, 0);
}

#[test]
fn at_claude_in_a_reply_asks_and_the_answer_lands_on_that_thread() {
    let (mut a, ctl) = setup();
    let c = a.mirror.threads[0].id.clone();
    let mut tx = Tx::default();
    a.card_action(&c, CardAction::Reply("@claude is zero rejected too?".into()), &mut tx);
    let now = Instant::now();
    a.ask_tick_at(now);
    assert_eq!(ctl.borrow().reqs.len(), 1);
    assert!(ctl.borrow().reqs[0].stdin.contains("@claude is zero rejected too?"));
    assert!(
        ctl.borrow().reqs[0].stdin.contains("The amount is not validated"),
        "the thread so far is in the prompt"
    );
    answer(&ctl, 0, ok("Yes: `<= 0` returns 400."));
    a.ask_tick_at(now + Duration::from_secs(1));
    let t = thread(&a, &c);
    assert_eq!(t.messages.last().unwrap().body, "Yes: `<= 0` returns 400.");
    assert_eq!(t.answer.unwrap().status, AnswerStatus::Answered);
}

#[test]
fn presence_reads_working_while_the_launcher_runs_and_comes_back_after() {
    let (mut a, ctl) = setup();
    let id = ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    let p = a.shared.presence();
    assert_eq!(p.state, PresenceState::Working);
    assert_eq!(p.on.as_deref(), Some(id.as_str()));
    let mut tx = Tx::default();
    a.conversation_tick(&mut tx);
    assert!(a.presence_text().starts_with("Claude working on"), "{}", a.presence_text());
    answer(&ctl, 0, ok("Done."));
    a.ask_tick_at(now + Duration::from_secs(1));
    assert_eq!(a.shared.presence(), Presence::default(), "the dot goes back to what it was");
}

#[test]
fn an_attached_agents_heartbeat_during_a_run_leaves_it_idle_after() {
    let (mut a, ctl) = setup();
    presence(&a, PresenceState::Idle, None);
    a.settings.ask.prefer = AskPrefer::Launcher;
    let id = ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    assert_eq!(a.shared.presence().on.as_deref(), Some(id.as_str()));
    let beat = |a: &App| {
        a.shared
            .update_presence(|p| p.seen_at = Some(iso_from_millis(crate::app::conversation::now_ms() + 1_000)));
    };
    beat(&a);
    a.ask_tick_at(now + Duration::from_secs(1));
    beat(&a);
    answer(&ctl, 0, ok("Done."));
    a.ask_tick_at(now + Duration::from_secs(2));
    let p = a.shared.presence();
    assert_eq!((p.state, p.on), (PresenceState::Idle, None), "the dot goes back to idle");
    a.settings.ask.prefer = AskPrefer::Attached;
    ask(&mut a, "And then?");
    a.ask_tick_at(now + Duration::from_secs(3));
    assert_eq!(ctl.borrow().reqs.len(), 1, "the next question goes to the attached agent first");
}

/// The agent's presence as last heard `secs` ago.
fn presence_ago(a: &App, state: PresenceState, on: Option<&str>, secs: i64) {
    a.shared.set_presence(Presence {
        state,
        on: on.map(str::to_owned),
        reason: None,
        seen_at: Some(iso_from_millis(crate::app::conversation::now_ms() - secs * 1_000)),
    });
}

fn claim(a: &mut App, id: &str, release: bool) {
    let tid = id.to_owned();
    assert_eq!(a.mutate(|s| ops::claim(s, &tid, release)), Some(true));
}

fn reply(a: &mut App, id: &str, body: &str) {
    let tid = id.to_owned();
    a.mutate(|s| ops::reply(s, &tid, body, Some("claude")));
}

#[test]
fn the_agents_answer_stops_a_running_launcher_and_drops_its_late_reply() {
    let (mut a, ctl) = setup();
    let id = ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    reply(&mut a, &id, "Attached says: UoW.");
    answer(&ctl, 0, ok("Launcher says: UoW too."));
    a.ask_tick_at(now + Duration::from_secs(1));
    assert!(ctl.borrow().cancelled[0], "the launcher run is stopped");
    let t = thread(&a, &id);
    assert_eq!(t.messages.len(), 2, "only the agent's answer is posted");
    assert_eq!(t.messages[1].body, "Attached says: UoW.");
    let ans = t.answer.unwrap();
    assert_eq!((ans.status, ans.by.as_deref()), (AnswerStatus::Answered, Some("attached")));
    assert!(!a.ask.is_tracking(&id));
    assert_eq!(a.shared.asks().running, 0);
    assert_eq!(a.shared.presence(), Presence::default(), "the launcher's dot goes");
}

#[test]
fn a_claim_stops_a_running_launcher_and_the_agent_answers() {
    let (mut a, ctl) = setup();
    let id = ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    assert_eq!(status(&a, &id).by.as_deref(), Some("launcher"));
    claim(&mut a, &id, false);
    a.ask_tick_at(now + Duration::from_secs(1));
    assert!(ctl.borrow().cancelled[0]);
    let ans = status(&a, &id);
    assert_eq!((ans.status, ans.by.as_deref()), (AnswerStatus::Answering, Some("attached")));
    a.ask_tick_at(now + Duration::from_secs(60));
    assert_eq!(ctl.borrow().reqs.len(), 1, "the launcher stays off a claimed question");
    reply(&mut a, &id, "UoW.");
    a.ask_tick_at(now + Duration::from_secs(61));
    assert_eq!(thread(&a, &id).messages.len(), 2);
    assert_eq!(status(&a, &id).by.as_deref(), Some("attached"));
}

#[test]
fn an_agent_that_answers_25_s_after_taking_a_question_is_never_raced() {
    let (mut a, ctl) = setup();
    presence(&a, PresenceState::Idle, None);
    let id = ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    presence(&a, PresenceState::Working, Some(&id));
    a.ask_tick_at(now + Duration::from_secs(1));
    presence_ago(&a, PresenceState::Working, Some(&id), 25);
    a.ask_tick_at(now + Duration::from_secs(25));
    assert!(
        ctl.borrow().reqs.is_empty(),
        "a quiet heartbeat does not hand a taken question over"
    );
    reply(&mut a, &id, "Inside the unit of work.");
    a.ask_tick_at(now + Duration::from_secs(26));
    assert!(ctl.borrow().reqs.is_empty(), "no launcher run started");
    let t = thread(&a, &id);
    assert_eq!(t.messages.len(), 2, "a single answer");
    let ans = t.answer.unwrap();
    assert_eq!((ans.status, ans.by.as_deref()), (AnswerStatus::Answered, Some("attached")));
}

#[test]
fn a_delivered_ask_waits_out_handoff_sec_while_the_heartbeat_is_quiet() {
    let (mut a, ctl) = setup();
    presence_ago(&a, PresenceState::Idle, None, 5);
    ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    presence_ago(&a, PresenceState::Idle, None, 24);
    a.ask_tick_at(now + Duration::from_secs(19));
    assert!(ctl.borrow().reqs.is_empty(), "the agent has it until handoffSec passes");
    a.ask_tick_at(now + Duration::from_secs(21));
    assert_eq!(ctl.borrow().reqs.len(), 1, "then the launcher takes it");
}

#[test]
fn an_agent_heard_within_handoff_sec_is_still_attached() {
    let (mut a, ctl) = setup();
    a.settings.ask.handoff_sec = 40;
    presence_ago(&a, PresenceState::Idle, None, 30);
    ask(&mut a, "Why?");
    a.ask_tick_at(Instant::now());
    assert!(ctl.borrow().reqs.is_empty(), "30 s quiet is attached when handoffSec is 40");
    let (mut a, _ctl) = setup();
    a.settings.ask.handoff_sec = 0;
    presence_ago(&a, PresenceState::Idle, None, 10);
    assert!(a.agent_attached(), "never less than the floor");
}

#[test]
fn a_second_question_queues_for_a_working_agent_whose_heartbeat_went_quiet() {
    let (mut a, ctl) = setup();
    presence(&a, PresenceState::Idle, None);
    let one = ask(&mut a, "First?");
    let now = Instant::now();
    a.ask_tick_at(now);
    presence(&a, PresenceState::Working, Some(&one));
    a.ask_tick_at(now + Duration::from_secs(1));
    presence_ago(&a, PresenceState::Working, Some(&one), 25);
    let two = ask(&mut a, "Second?");
    a.ask_tick_at(now + Duration::from_secs(2));
    a.ask_tick_at(now + Duration::from_secs(40));
    assert!(ctl.borrow().reqs.is_empty(), "the second waits for the agent, not the launcher");
    assert_eq!(status(&a, &two).status, AnswerStatus::Queued);
    assert!(a.ask.is_tracking(&two));
    reply(&mut a, &one, "One.");
    presence(&a, PresenceState::Idle, None);
    a.ask_tick_at(now + Duration::from_secs(41));
    claim(&mut a, &two, false);
    a.ask_tick_at(now + Duration::from_secs(50));
    reply(&mut a, &two, "Two.");
    a.ask_tick_at(now + Duration::from_secs(80));
    assert!(ctl.borrow().reqs.is_empty());
    for id in [&one, &two] {
        let ans = status(&a, id);
        assert_eq!((ans.status, ans.by.as_deref()), (AnswerStatus::Answered, Some("attached")));
    }
}

#[test]
fn three_questions_claimed_at_once_and_answered_after_30_s_never_reach_the_launcher() {
    let (mut a, ctl) = setup();
    a.settings.ask.max_concurrent = 3;
    presence(&a, PresenceState::Idle, None);
    let ids = [ask(&mut a, "First?"), ask(&mut a, "Second?"), ask(&mut a, "Third?")];
    let now = Instant::now();
    a.ask_tick_at(now);
    for id in &ids {
        claim(&mut a, id, false);
    }
    a.ask_tick_at(now + Duration::from_secs(1));
    presence_ago(&a, PresenceState::Idle, None, 40);
    a.ask_tick_at(now + Duration::from_secs(30));
    for id in &ids {
        reply(&mut a, id, "Answered by a helper.");
    }
    a.ask_tick_at(now + Duration::from_secs(31));
    assert!(ctl.borrow().reqs.is_empty(), "the launcher never ran");
    for id in &ids {
        let t = thread(&a, id);
        assert_eq!(t.messages.len(), 2);
        let ans = t.answer.unwrap();
        assert_eq!((ans.status, ans.by.as_deref()), (AnswerStatus::Answered, Some("attached")));
    }
}

#[test]
fn a_claim_that_never_gets_a_reply_hands_over_at_the_ceiling() {
    let (mut a, ctl) = setup();
    presence(&a, PresenceState::Idle, None);
    let one = ask(&mut a, "First?");
    let now = Instant::now();
    a.ask_tick_at(now);
    claim(&mut a, &one, false);
    presence(&a, PresenceState::Working, Some(&one));
    a.ask_tick_at(now + Duration::from_secs(1));
    let two = ask(&mut a, "Second?");
    a.ask_tick_at(now + Duration::from_secs(2));
    presence(&a, PresenceState::Working, Some(&one));
    a.ask_tick_at(now + Duration::from_secs(120));
    assert!(ctl.borrow().reqs.is_empty(), "within timeoutSec the claim holds");
    presence(&a, PresenceState::Working, Some(&one));
    a.ask_tick_at(now + Duration::from_secs(121));
    assert_eq!(ctl.borrow().reqs.len(), 1, "past the ceiling the launcher takes it");
    assert_eq!(status(&a, &one).by.as_deref(), Some("launcher"));
    presence(&a, PresenceState::Working, Some(&one));
    a.ask_tick_at(now + Duration::from_secs(122));
    answer(&ctl, 0, ok("One."));
    a.ask_tick_at(now + Duration::from_secs(123));
    assert_eq!(ctl.borrow().reqs.len(), 2, "the queue behind a busy agent hands over too");
    assert_eq!(status(&a, &two).by.as_deref(), Some("launcher"));
}

#[test]
fn releasing_a_claim_hands_the_question_to_the_launcher() {
    let (mut a, ctl) = setup();
    presence(&a, PresenceState::Idle, None);
    let id = ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    claim(&mut a, &id, false);
    a.ask_tick_at(now + Duration::from_secs(1));
    assert!(ctl.borrow().reqs.is_empty());
    claim(&mut a, &id, true);
    a.ask_tick_at(now + Duration::from_secs(2));
    assert_eq!(ctl.borrow().reqs.len(), 1);
}

#[test]
fn with_no_launcher_and_no_agent_the_card_says_so() {
    let (mut a, ctl) = setup();
    a.settings.ask.enabled = false;
    let id = ask(&mut a, "Why?");
    a.ask_tick_at(Instant::now());
    assert!(ctl.borrow().reqs.is_empty());
    let ans = status(&a, &id);
    assert_eq!(ans.status, AnswerStatus::Queued);
    assert_eq!(ans.error.as_deref(), Some(NO_ANSWERER));
    a.apply_ask_item(AskItem::Toggle);
    a.ask_tick_at(Instant::now());
    assert_eq!(ctl.borrow().reqs.len(), 1, "turning the launcher on picks it up");
}

#[test]
fn the_launcher_waits_until_the_user_allows_it_once() {
    let (mut a, ctl) = setup();
    a.settings.ask.allowed = false;
    let id = ask(&mut a, "Why?");
    a.ask_tick_at(Instant::now());
    assert!(ctl.borrow().reqs.is_empty(), "nothing runs before the user allows it");
    assert_eq!(status(&a, &id).error.as_deref(), Some(crate::app::ask::NOT_ALLOWED));
    assert!(a.conversation.toasts.iter().any(|t| t.key == "ask-allow"));
    assert!(
        !a.status.contains("Allow") && !a.status.contains("agent"),
        "the toast says it, the status line does not repeat it: {}",
        a.status
    );
    let mut tx = Tx::default();
    a.toast_action("ask-allow", &mut tx);
    assert!(a.settings.ask.allowed);
    a.ask_tick_at(Instant::now());
    assert_eq!(ctl.borrow().reqs.len(), 1);
    let argv = ctl.borrow().reqs[0].argv.clone();
    assert!(
        argv.contains(&"--disallowedTools".to_owned()) && argv.contains(&"Read(//**/*.env)".to_owned()),
        "{argv:?}"
    );
}

#[test]
fn redacted_paths_are_withheld_from_the_prompt() {
    let (mut a, _ctl) = setup();
    let id = ask(&mut a, "Why?");
    let open = a.ask_prompt_for(&id).unwrap();
    assert!(open.contains("The diff around it"), "{open}");
    a.settings.ask.redact = vec!["**/*".into()];
    let p = a.ask_prompt_for(&id).unwrap();
    assert!(p.contains("is withheld"), "{p}");
    assert!(!p.contains("The diff around it"));
    let snap = thread(&a, &id).code_snapshot.map(|c| c.content).unwrap_or_default();
    if snap.trim().len() > 8 {
        assert!(!p.contains(snap.trim()));
    }
}

#[test]
fn closing_the_window_cancels_running_answers() {
    let (mut a, ctl) = setup();
    let id = ask(&mut a, "Why?");
    a.ask_tick_at(Instant::now());
    a.ask_shutdown();
    assert!(ctl.borrow().cancelled[0]);
    assert_eq!(status(&a, &id).status, AnswerStatus::Cancelled);
    assert_eq!(a.shared.presence(), Presence::default());
}

#[test]
fn a_run_left_by_a_closed_window_reads_failed_on_open() {
    let (mut a, _ctl) = setup();
    let first = a.mirror.threads[0].id.clone();
    a.mutate(|s| {
        s.threads[0].answer = Some(Answer {
            status: AnswerStatus::Answering,
            by: Some("launcher".into()),
            ..Answer::default()
        });
        s.bump_comments();
    });
    a.ask_tick_at(Instant::now());
    let ans = status(&a, &first);
    assert_eq!(ans.status, AnswerStatus::Failed);
    assert!(ans.error.unwrap().contains("closed"));
}

#[test]
fn a_question_left_queued_by_a_closed_window_is_asked_again_on_open() {
    let (mut a, ctl) = setup();
    let first = a.mirror.threads[0].id.clone();
    a.mutate(|s| {
        s.threads[0].answer = Some(Answer::default());
        s.bump_comments();
    });
    a.ask_tick_at(Instant::now());
    assert_eq!(ctl.borrow().reqs.len(), 1);
    assert_eq!(status(&a, &first).status, AnswerStatus::Answering);
}

#[test]
fn an_ask_imported_before_the_mirror_caught_up_is_still_answered() {
    let (mut a, ctl) = setup();
    let first = a.mirror.threads[0].clone();
    let extra = ops::ThreadExtra {
        kind: Some(ThreadKind::Question),
        ..ops::ThreadExtra::default()
    };
    let id = {
        let mut s = a.shared.lock();
        ops::add_thread_with(&mut s, &first.repo, &first.file_path, first.anchor, "Imported?", None, None, extra)
    };
    a.shared.push_event(rain_core::Event {
        kind: EventKind::Ask,
        thread: Some(id.clone()),
        data: serde_json::json!({ "question": "Imported?" }),
        ..rain_core::Event::default()
    });
    assert!(ops::thread(&a.mirror, &id).is_none(), "the window has not seen the import yet");
    a.ask_tick_at(Instant::now());
    assert_eq!(ctl.borrow().reqs.len(), 1, "the ask is not used up before its thread is known");
    assert_eq!(status(&a, &id).status, AnswerStatus::Answering);
}

#[test]
fn undoing_the_delete_of_a_question_being_answered_asks_it_again() {
    let (mut a, ctl) = setup();
    let id = ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    let mut tx = Tx::default();
    a.card_action(&id, CardAction::Delete, &mut tx);
    a.ask_tick_at(now + Duration::from_secs(1));
    assert!(ctl.borrow().cancelled[0], "the delete ends the run");
    a.undo_latest(&mut tx);
    assert!(ops::thread(&a.mirror, &id).is_some());
    a.ask_tick_at(now + Duration::from_secs(2));
    assert_eq!(ctl.borrow().reqs.len(), 2, "restored, it is asked again");
    assert_eq!(status(&a, &id).status, AnswerStatus::Answering);
}

#[test]
fn a_follow_up_sent_while_the_answer_runs_is_asked_after_it() {
    let (mut a, ctl) = setup();
    let id = ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    let mut tx = Tx::default();
    a.card_action(&id, CardAction::Reply("And the second case?".into()), &mut tx);
    assert_eq!(
        status(&a, &id).status,
        AnswerStatus::Answering,
        "the running answer keeps its status"
    );
    a.ask_tick_at(now + Duration::from_secs(1));
    assert_eq!(ctl.borrow().reqs.len(), 1, "one run at a time per thread");
    answer(&ctl, 0, ok("Because."));
    a.ask_tick_at(now + Duration::from_secs(2));
    assert_eq!(ctl.borrow().reqs.len(), 2, "the follow-up runs next");
    assert!(ctl.borrow().reqs[1].stdin.contains("And the second case?"));
    assert_eq!(status(&a, &id).status, AnswerStatus::Answering);
    answer(&ctl, 1, ok("Also because."));
    a.ask_tick_at(now + Duration::from_secs(3));
    let t = thread(&a, &id);
    assert_eq!(t.messages.last().unwrap().body, "Also because.");
    assert_eq!(t.answer.unwrap().status, AnswerStatus::Answered);
}

#[test]
fn an_agent_reply_to_an_at_claude_reply_on_a_comment_ends_the_wait() {
    let (mut a, ctl) = setup();
    presence(&a, PresenceState::Idle, None);
    let c = a.mirror.threads[0].id.clone();
    assert_ne!(thread(&a, &c).kind(), ThreadKind::Question);
    let mut tx = Tx::default();
    a.card_action(&c, CardAction::Reply("@Claude I'm not getting this one.".into()), &mut tx);
    let now = Instant::now();
    a.ask_tick_at(now);
    presence(&a, PresenceState::Working, Some(&c));
    a.ask_tick_at(now + Duration::from_secs(1));
    assert_eq!(status(&a, &c).status, AnswerStatus::Answering);
    let tid = c.clone();
    a.mutate(|s| ops::reply(s, &tid, "Fair, that one was badly worded.", Some("Skippy")));
    presence(&a, PresenceState::Idle, None);
    a.ask_tick_at(now + Duration::from_secs(2));
    let ans = status(&a, &c);
    assert_eq!((ans.status, ans.by.as_deref()), (AnswerStatus::Answered, Some("attached")));
    assert!(!a.ask.is_tracking(&c));
    presence(&a, PresenceState::Idle, None);
    a.ask_tick_at(now + Duration::from_secs(60));
    assert!(ctl.borrow().reqs.is_empty(), "the launcher never answers it again");
    let next = ask(&mut a, "And the next one?");
    a.ask_tick_at(now + Duration::from_secs(61));
    assert!(ctl.borrow().reqs.is_empty(), "the next question goes to the agent first");
    assert!(a.ask.is_tracking(&next));
}

#[test]
fn a_question_asked_while_the_agent_answers_another_queues_for_the_agent() {
    let (mut a, ctl) = setup();
    presence(&a, PresenceState::Idle, None);
    let one = ask(&mut a, "First?");
    let now = Instant::now();
    a.ask_tick_at(now);
    presence(&a, PresenceState::Working, Some(&one));
    a.ask_tick_at(now + Duration::from_secs(1));
    assert_eq!(status(&a, &one).by.as_deref(), Some("attached"));
    let two = ask(&mut a, "Second?");
    a.ask_tick_at(now + Duration::from_secs(2));
    presence(&a, PresenceState::Working, Some(&one));
    a.ask_tick_at(now + Duration::from_secs(40));
    assert!(ctl.borrow().reqs.is_empty(), "no launcher while the agent is on the first");
    assert_eq!(status(&a, &two).status, AnswerStatus::Queued);
    let tid = one.clone();
    a.mutate(|s| ops::reply(s, &tid, "One.", Some("claude")));
    a.ask_tick_at(now + Duration::from_secs(41));
    assert!(ctl.borrow().reqs.is_empty(), "still working, the agent keeps the second");
    presence(&a, PresenceState::Working, Some(&two));
    a.ask_tick_at(now + Duration::from_secs(45));
    let ans = status(&a, &two);
    assert_eq!((ans.status, ans.by.as_deref()), (AnswerStatus::Answering, Some("attached")));
    assert!(ctl.borrow().reqs.is_empty());
}

#[test]
fn an_agent_answering_several_questions_in_parallel_keeps_them_all() {
    let (mut a, ctl) = setup();
    a.settings.ask.max_concurrent = 3;
    presence(&a, PresenceState::Idle, None);
    let one = ask(&mut a, "First?");
    let now = Instant::now();
    a.ask_tick_at(now);
    presence(&a, PresenceState::Working, Some(&one));
    let two = ask(&mut a, "Second?");
    let three = ask(&mut a, "Third?");
    a.ask_tick_at(now + Duration::from_secs(1));
    for (i, id) in [&three, &one, &two].into_iter().enumerate() {
        let tid = id.clone();
        a.mutate(|s| ops::reply(s, &tid, "Answered by a helper.", Some("claude")));
        presence(&a, PresenceState::Working, Some(&one));
        a.ask_tick_at(now + Duration::from_secs(30 * (i as u64 + 1)));
    }
    presence(&a, PresenceState::Idle, None);
    a.ask_tick_at(now + Duration::from_secs(200));
    assert!(ctl.borrow().reqs.is_empty(), "the launcher never ran");
    for id in [&one, &two, &three] {
        let ans = status(&a, id);
        assert_eq!((ans.status, ans.by.as_deref()), (AnswerStatus::Answered, Some("attached")));
    }
}

#[test]
fn a_question_behind_one_the_agent_has_not_taken_waits_its_turn() {
    let (mut a, ctl) = setup();
    presence(&a, PresenceState::Idle, None);
    let one = ask(&mut a, "First?");
    let now = Instant::now();
    a.ask_tick_at(now);
    let two = ask(&mut a, "Second?");
    a.ask_tick_at(now + Duration::from_secs(2));
    presence(&a, PresenceState::Idle, None);
    let tid = one.clone();
    a.mutate(|s| ops::reply(s, &tid, "One.", Some("claude")));
    a.ask_tick_at(now + Duration::from_secs(19));
    presence(&a, PresenceState::Idle, None);
    a.ask_tick_at(now + Duration::from_secs(30));
    assert!(
        ctl.borrow().reqs.is_empty(),
        "the second's handoffSec started when the first was answered, not when it was asked"
    );
    assert!(a.ask.is_tracking(&two));
}

#[test]
fn an_agent_heard_during_a_launcher_run_still_gets_the_next_question() {
    let (mut a, ctl) = setup();
    a.settings.ask.max_concurrent = 2;
    presence(&a, PresenceState::Idle, None);
    a.settings.ask.prefer = AskPrefer::Launcher;
    ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    assert_eq!(ctl.borrow().reqs.len(), 1);
    a.settings.ask.prefer = AskPrefer::Attached;
    a.shared
        .update_presence(|p| p.seen_at = Some(iso_from_millis(crate::app::conversation::now_ms() + 1_000)));
    let two = ask(&mut a, "And this?");
    a.ask_tick_at(now + Duration::from_secs(1));
    assert_eq!(ctl.borrow().reqs.len(), 1, "the launcher's own dot does not hide the agent");
    assert!(a.ask.is_tracking(&two));
}

#[test]
fn the_allow_prompt_says_the_launcher_is_the_fallback() {
    let (mut a, _ctl) = setup();
    a.settings.ask.allowed = false;
    presence(&a, PresenceState::Idle, None);
    ask(&mut a, "Why?");
    let now = Instant::now();
    a.ask_tick_at(now);
    presence(&a, PresenceState::Idle, None);
    a.ask_tick_at(now + Duration::from_secs(21));
    let toast = a.conversation.toasts.iter().find(|t| t.key == "ask-allow").unwrap();
    assert!(
        toast.title.starts_with("No attached agent picked up the question") && toast.title.contains("fallback"),
        "{}",
        toast.title
    );
}

#[test]
fn palette_rows_toggle_prefer_time_out_and_set_the_command() {
    let mut a = app(story(), true);
    let rows = crate::palette::filtered(&a, "ask command: my-agent --ask {repos}");
    let set = rows
        .iter()
        .find(|e| matches!(e.item, crate::palette::Item::Ask(AskItem::SetCommand(_))))
        .expect("typing `ask command: …` offers to set it");
    let crate::palette::Item::Ask(item) = set.item.clone() else {
        unreachable!()
    };
    a.apply_ask_item(item);
    assert_eq!(a.settings.ask.command, "my-agent --ask {repos}");
    a.apply_ask_item(AskItem::Prefer);
    assert_eq!(a.settings.ask.prefer, AskPrefer::Launcher);
    let t = crate::palette::filtered(&a, "ask time out")
        .into_iter()
        .find_map(|e| match e.item {
            crate::palette::Item::Ask(AskItem::Timeout(t)) => Some(t),
            _ => None,
        })
        .unwrap();
    assert_eq!(t, 300);
    a.apply_ask_item(AskItem::Timeout(t));
    assert_eq!(a.settings.ask.timeout_sec, 300);
    a.apply_ask_item(AskItem::EditCommand);
    assert!(a.palette.open);
    assert_eq!(a.palette.edit.text, "ask command: my-agent --ask {repos}");
    let saved = crate::settings::Settings::load(a.settings_path.as_ref().unwrap());
    assert_eq!(saved.ask.timeout_sec, 300, "ask settings persist");
}

#[test]
fn ask_scenes_draw_in_both_themes() {
    for (name, want) in [
        ("question-answering", AnswerStatus::Answering),
        ("question-answered", AnswerStatus::Answered),
    ] {
        for dark in [true, false] {
            let mut a = app(story(), true);
            shell_scene(name)(&mut a);
            let q = a
                .mirror
                .threads
                .iter()
                .find(|t| t.kind() == ThreadKind::Question && t.answer.is_some())
                .cloned()
                .unwrap();
            assert_eq!(q.answer.unwrap().status, want, "{name}");
            let mut rt = Runtime::new(
                a,
                view,
                (1366.0, 768.0),
                Env {
                    system_dark: dark,
                    scale: 1.0,
                },
            );
            rt.poll = Some(|s: &mut App, _, tx| s.poll(tx));
            rt.frame(0.0);
            settle(&mut rt);
            assert!(
                rt.elements().iter().any(|e| !e.exiting && e.key.as_deref() == Some("answer-text")),
                "{name} shows the answer chip"
            );
        }
    }
}
