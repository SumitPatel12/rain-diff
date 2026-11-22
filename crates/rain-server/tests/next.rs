//! `GET /api/next`: what waits on the agent, derived from the stored threads, rounds and brief.

use std::sync::Arc;
use std::time::Duration;

use rain_core::{
    Anchor, Answer, AnswerStatus, BriefDoc, Message, ReviewRound, Session, SessionRepo, Shared, Side, Target, TargetKind, Thread,
    ThreadKind, ThreadState, Verdict,
};
use rain_server::{ServerHandle, ServerOptions, start_with};
use serde_json::{Value, json};
use ureq::Agent;

struct T {
    shared: Arc<Shared>,
    server: ServerHandle,
    agent: Agent,
}

fn serve(session: Session, opts: ServerOptions) -> T {
    let shared = Arc::new(Shared::new(session));
    let server = start_with(
        shared.clone(),
        ServerOptions {
            preferred: Some(0),
            ..opts
        },
    )
    .unwrap();
    let agent = Agent::config_builder()
        .http_status_as_error(false)
        .proxy(None)
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    T { shared, server, agent }
}

impl T {
    fn next(&self) -> Value {
        let mut r = self.agent.get(&format!("{}/api/next", self.server.url)).call().unwrap();
        let text = r.body_mut().read_to_string().unwrap();
        assert_eq!(r.status().as_u16(), 200, "{text}");
        serde_json::from_str(&text).unwrap()
    }

    fn post(&self, path: &str, body: &str) -> u16 {
        let url = format!("{}{path}", self.server.url);
        let r = self.agent.post(&url).header("Content-Type", "application/json").send(body).unwrap();
        r.status().as_u16()
    }
}

/// `2001-01-01T10:<mm>:00.000Z`: long before any time the routes stamp.
fn at(minute: u32) -> String {
    format!("2001-01-01T10:{minute:02}:00.000Z")
}

fn said(author: Option<&str>, minute: u32, body: &str) -> Message {
    Message {
        id: format!("m{minute}"),
        body: body.into(),
        author: author.map(str::to_owned),
        created_at: at(minute),
        updated_at: at(minute),
    }
}

fn thread(id: &str, line: u32, messages: Vec<Message>) -> Thread {
    Thread {
        id: id.into(),
        file_path: "src/Api/VouchersController.cs".into(),
        anchor: Anchor::Line { side: Side::New, line },
        messages,
        ..Thread::default()
    }
}

fn question(id: &str, minute: u32, status: AnswerStatus, by: Option<&str>) -> Thread {
    Thread {
        kind: Some(ThreadKind::Question),
        answer: Some(Answer {
            status,
            by: by.map(str::to_owned),
            ..Answer::default()
        }),
        ..thread(id, minute, vec![said(None, minute, &format!("why {id}?"))])
    }
}

fn kinds(v: &Value) -> Vec<(String, String)> {
    v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            let id = i["threadId"].as_str().or(i["reason"].as_str()).unwrap_or_default();
            (i["kind"].as_str().unwrap().to_owned(), id.to_owned())
        })
        .collect()
}

fn pair(kind: &str, id: &str) -> (String, String) {
    (kind.to_owned(), id.to_owned())
}

fn with_brief(threads: Vec<Thread>) -> Session {
    Session {
        threads,
        brief: Some(BriefDoc::default()),
        ..Session::default()
    }
}

#[test]
fn nothing_waits_when_the_agent_had_the_last_word() {
    let resolved = Thread {
        state: ThreadState::Resolved,
        ..thread("t-resolved", 3, vec![said(None, 1, "drop this")])
    };
    let dismissed = Thread {
        state: ThreadState::Dismissed,
        ..question("q-dismissed", 2, AnswerStatus::Queued, None)
    };
    let answered = Thread {
        messages: vec![said(None, 3, "why?"), said(Some("claude"), 4, "Because.")],
        ..question("q-answered", 3, AnswerStatus::Answered, Some("attached"))
    };
    let t = serve(
        with_brief(vec![
            thread("t-draft", 1, vec![said(Some("claude"), 1, "F1 issue (blocking): x")]),
            thread("t-replied", 2, vec![said(None, 1, "fix"), said(Some("claude"), 2, "Done.")]),
            resolved,
            dismissed,
            answered,
            question("q-launcher", 5, AnswerStatus::Answering, Some("launcher")),
        ]),
        ServerOptions::default(),
    );
    assert_eq!(t.next(), json!({ "waiting": 0, "round": 1, "seq": 0, "items": [] }));
}

#[test]
fn each_kind_is_listed_in_the_order_to_take_it_up() {
    let addressed = |id: &str, said_at: u32| Thread {
        state: ThreadState::Addressed,
        state_sha: Some("d4e5f6a".into()),
        state_at: Some(at(30)),
        ..thread(id, 9, vec![said(None, said_at, "fix this")])
    };
    let failed = Thread {
        answer: Some(Answer {
            status: AnswerStatus::Failed,
            by: Some("launcher".into()),
            ..Answer::default()
        }),
        ..thread(
            "q-failed",
            6,
            vec![said(Some("claude"), 1, "F2 nit: x"), said(None, 12, "@claude why?")],
        )
    };
    let s = Session {
        threads: vec![
            thread("r-new", 4, vec![said(None, 20, "This still double-counts.")]),
            question("q-late", 15, AnswerStatus::Queued, None),
            question("c-claimed", 5, AnswerStatus::Answering, Some("attached")),
            question("q-early", 10, AnswerStatus::Queued, None),
            failed,
            addressed("a-before", 25),
            addressed("r-after", 35),
        ],
        rounds: vec![ReviewRound {
            n: 1,
            at: at(40),
            verdict: Verdict::RequestChanges,
            summary: "  Two blockers.  ".into(),
            ..ReviewRound::default()
        }],
        ..Session::default()
    };
    let t = serve(s, ServerOptions::default());
    let v = t.next();
    assert_eq!(
        kinds(&v),
        [
            pair("question", "q-early"),
            pair("question", "q-failed"),
            pair("question", "q-late"),
            pair("reply", "r-new"),
            pair("reply", "r-after"),
            pair("claimed", "c-claimed"),
            pair("verdict", ""),
            pair("brief", "missing"),
        ],
        "{v}"
    );
    assert_eq!((v["waiting"].clone(), v["round"].clone()), (json!(8), json!(2)));
    let items = v["items"].as_array().unwrap();
    assert_eq!(
        items[6],
        json!({ "kind": "verdict", "round": 1, "verdict": "request-changes", "text": "Two blockers.", "at": at(40) })
    );
    assert_eq!(items[7], json!({ "kind": "brief", "reason": "missing" }));
    assert_eq!(items[5]["answer"], json!({ "status": "answering", "by": "attached" }));
}

#[test]
fn a_thread_item_carries_what_the_existing_commands_take() {
    let long = "x".repeat(900);
    let on_pr = Thread {
        file_path: "@pr".into(),
        target: Some(Target {
            kind: TargetKind::Pr,
            ..Target::default()
        }),
        ..thread("t-pr", 1, vec![said(None, 2, &long)])
    };
    let ranged = Thread {
        repo: "server".into(),
        anchor: Anchor::Range {
            side: Side::Old,
            start: 27,
            end: 31,
        },
        kind: Some(ThreadKind::Issue),
        blocking: Some(true),
        ..thread(
            "k3x9f2",
            1,
            vec![said(Some("claude"), 1, "Fixed."), said(None, 1, "\n Still double-counts.\n")],
        )
    };
    let t = serve(with_brief(vec![ranged, on_pr]), ServerOptions::default());
    let v = t.next();
    assert_eq!(
        v["items"][0],
        json!({
            "kind": "reply",
            "threadId": "k3x9f2",
            "repo": "server",
            "filePath": "src/Api/VouchersController.cs",
            "position": { "side": "old", "line": { "start": 27, "end": 31 } },
            "where": "server/src/Api/VouchersController.cs:L27-L31 (old)",
            "threadKind": "issue",
            "blocking": true,
            "state": "open",
            "text": "Still double-counts.",
            "at": at(1)
        })
    );
    let pr = &v["items"][1];
    assert_eq!(
        (pr["where"].clone(), pr["target"].clone()),
        (json!("Review (whole change)"), json!({ "kind": "pr" }))
    );
    let text = pr["text"].as_str().unwrap();
    assert_eq!(text.chars().count(), 400, "the reviewer's text is cut");
    assert!(text.ends_with('…'));
}

#[test]
fn a_claim_and_a_reply_move_a_question_off_the_list() {
    let t = serve(
        with_brief(vec![question("q1", 1, AnswerStatus::Queued, None)]),
        ServerOptions::default(),
    );
    assert_eq!(kinds(&t.next()), [pair("question", "q1")]);
    assert_eq!(t.post("/api/comments/q1/claim", "{}"), 200);
    let claimed = t.next();
    assert_eq!(kinds(&claimed), [pair("claimed", "q1")]);
    assert!(claimed["seq"].as_u64().unwrap() >= 1, "the seq to resume `watch --since` from");
    let reply = r#"{"type":"reply","threadId":"q1","body":"Because.","author":"claude"}"#;
    assert_eq!(t.post("/api/comment-imports", reply), 200);
    assert_eq!(t.next()["waiting"], 0);
}

#[test]
fn the_verdict_waits_until_the_agent_answers_the_round() {
    let session = || Session {
        repos: vec![SessionRepo {
            id: "server".into(),
            target: "a1b2c3d".into(),
            ..SessionRepo::default()
        }],
        ..with_brief(vec![thread("t1", 1, vec![said(Some("claude"), 1, "F1 issue: x")])])
    };
    let submit = |t: &T| assert_eq!(t.post("/api/review", r#"{"verdict":"approve","summary":"Ship it."}"#), 200);

    let t = serve(session(), ServerOptions::default());
    submit(&t);
    let v = t.next();
    assert_eq!(kinds(&v), [pair("verdict", "")]);
    assert_eq!((v["items"][0]["verdict"].clone(), v["round"].clone()), (json!("approve"), json!(2)));
    let reply = r#"{"type":"reply","threadId":"t1","body":"Thanks.","author":"claude"}"#;
    assert_eq!(t.post("/api/comment-imports", reply), 200);
    assert_eq!(t.next()["waiting"], 0, "a reply after the round answers it");

    let t = serve(session(), ServerOptions::default());
    submit(&t);
    assert_eq!(t.post("/api/comments/t1/state", r#"{"state":"addressed","sha":"d4e5f6a"}"#), 200);
    assert_eq!(t.next()["waiting"], 0, "so does marking a thread addressed");

    let t = serve(session(), ServerOptions::default());
    submit(&t);
    t.shared.lock().repos[0].target = "d4e5f6a".into();
    assert_eq!(t.next()["waiting"], 0, "and loading another sha");
}

#[test]
fn a_stale_brief_is_owed() {
    let mut s = with_brief(Vec::new());
    s.repos.push(SessionRepo {
        id: "server".into(),
        target: "a1b2c3d4e5".into(),
        ..SessionRepo::default()
    });
    let t = serve(s, ServerOptions::default());
    let doc =
        |sha: &str| json!({ "format": "rain-brief/v1", "title": "T", "writtenAgainst": [{ "repo": "server", "sha": sha }] }).to_string();
    let put = |body: String| {
        let url = format!("{}/api/brief", t.server.url);
        let r = t.agent.put(&url).header("Content-Type", "application/json").send(body).unwrap();
        assert_eq!(r.status().as_u16(), 200);
    };
    put(doc("fff0000"));
    assert_eq!(t.next()["items"], json!([{ "kind": "brief", "reason": "stale" }]));
    put(doc("a1b2c3d"));
    assert_eq!(t.next()["waiting"], 0);
}

#[test]
fn reading_changes_nothing_and_works_in_a_read_only_window() {
    let dir = std::env::temp_dir().join(format!("rain-server-next-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let blocked = dir.join("not-a-directory");
    std::fs::write(&blocked, "keep").unwrap();
    let t = serve(
        with_brief(vec![question("q1", 1, AnswerStatus::Queued, None)]),
        ServerOptions {
            session_file: Some(blocked.join("S.json")),
            ..ServerOptions::default()
        },
    );
    assert!(t.server.read_only);
    assert_eq!(t.post("/api/comments/q1/claim", "{}"), 423, "this window takes no change");
    let before = t.shared.lock().clone();
    let v = t.next();
    assert_eq!(kinds(&v), [pair("question", "q1")]);
    assert_eq!(v["items"][0]["answer"], json!({ "status": "queued" }));
    assert_eq!(t.next(), v, "asking twice gives the same answer");
    assert_eq!(*t.shared.lock(), before, "nothing is claimed, marked or bumped");
    assert_eq!(t.shared.event_seq(), 0, "no event");
    assert!(t.shared.presence().seen_at.is_none(), "and it is no heartbeat");
    drop(t);
    assert_eq!(std::fs::read_to_string(&blocked).unwrap(), "keep");
    let _ = std::fs::remove_dir_all(dir);
}
