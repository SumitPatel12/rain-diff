//! What the window's loaded diff gives the routes at post time: snapshots for imported threads
//! and notes, the commit they were taken at, an `ask` for a user's question, and artifact
//! threads that follow an overview update.

use std::sync::Arc;
use std::time::Duration;

use rain_core::{Anchor, AnchorHow, AnchorState, CodeSnapshot, Session, SessionRepo, Shared};
use rain_server::{ServerHandle, ServerOptions, start_with};
use serde_json::{Value, json};
use ureq::Agent;

const TARGET: &str = "bfaf6647a75736caba1e8a5a4728495cf773f358";
const BASE: &str = "b36e39c54c4184d04456d575e494cd26a5e488e5";

struct T {
    shared: Arc<Shared>,
    server: ServerHandle,
    agent: Agent,
}

fn serve(with_diff: bool) -> T {
    let session = Session {
        repos: vec![SessionRepo {
            id: String::new(),
            repository_id: String::new(),
            base: BASE.into(),
            target: TARGET.into(),
        }],
        ..Session::default()
    };
    let shared = Arc::new(Shared::new(session));
    if with_diff {
        shared.set_excerpt_source(Box::new(|_, path, anchor| {
            let (a, _) = anchor.lines();
            (path == "src/a.rs" && (1..=10).contains(&a)).then(|| CodeSnapshot {
                content: format!("line {a}"),
                ..CodeSnapshot::default()
            })
        }));
    }
    let server = start_with(
        shared.clone(),
        ServerOptions {
            preferred: Some(0),
            ..ServerOptions::default()
        },
    )
    .unwrap();
    let agent = Agent::config_builder()
        .http_status_as_error(false)
        .proxy(None)
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();
    T { shared, server, agent }
}

impl T {
    fn get(&self, path: &str) -> Value {
        let mut r = self.agent.get(&format!("{}{path}", self.server.url)).call().unwrap();
        serde_json::from_str(&r.body_mut().read_to_string().unwrap()).unwrap()
    }

    fn send(&self, method: &str, path: &str, body: &str) -> Value {
        let url = format!("{}{path}", self.server.url);
        let mut r = match method {
            "PUT" => self.agent.put(&url),
            _ => self.agent.post(&url),
        }
        .header("Content-Type", "application/json")
        .send(body)
        .unwrap();
        serde_json::from_str(&r.body_mut().read_to_string().unwrap()).unwrap()
    }
}

fn thread(path: &str, line: u32, body: &str) -> Value {
    json!({"type": "thread", "filePath": path, "position": {"side": "new", "line": line}, "body": body, "author": "claude"})
}

#[test]
fn imported_threads_snapshot_the_loaded_code_and_its_commit_at_post_time() {
    let t = serve(true);
    let r = t.send(
        "POST",
        "/api/comment-imports",
        &json!([thread("src/a.rs", 5, "in"), thread("src/a.rs", 99, "out")]).to_string(),
    );
    assert_eq!(r["warnings"].as_array().unwrap().len(), 1, "{r}");
    let s = t.shared.lock();
    let inside = s.threads.iter().find(|t| t.messages[0].body == "in").unwrap();
    assert_eq!(inside.code_snapshot.as_ref().unwrap().content, "line 5");
    assert_eq!(inside.commit.as_deref(), Some(TARGET));
    let outside = s.threads.iter().find(|t| t.messages[0].body == "out").unwrap();
    assert_eq!(outside.target.as_ref().map(|t| t.kind), Some(rain_core::TargetKind::File));
}

#[test]
fn without_a_loaded_diff_imports_are_stored_as_given() {
    let t = serve(false);
    t.send("POST", "/api/comment-imports", &json!([thread("src/a.rs", 99, "out")]).to_string());
    let s = t.shared.lock();
    assert!(s.threads[0].code_snapshot.is_none());
    assert!(matches!(s.threads[0].anchor, Anchor::Line { line: 99, .. }));
}

#[test]
fn line_notes_snapshot_at_post_time() {
    let t = serve(true);
    t.send(
        "POST",
        "/api/notes",
        r#"{"kind":"line","filePath":"src/a.rs","position":{"side":"new","line":3},"body":"n"}"#,
    );
    assert_eq!(t.shared.lock().notes[0].code_snapshot.as_ref().unwrap().content, "line 3");
}

#[test]
fn a_users_question_import_asks_the_window_and_an_agents_does_not() {
    let t = serve(true);
    let q = json!([
        {"type": "thread", "filePath": "src/a.rs", "position": {"side": "new", "line": 2}, "body": "why?", "kind": "question"},
        {"type": "thread", "filePath": "src/a.rs", "position": {"side": "new", "line": 3}, "body": "agent asks", "kind": "question", "author": "claude"}
    ]);
    t.send("POST", "/api/comment-imports", &q.to_string());
    let v = t.get("/api/events?kinds=ask&since=0&timeout=1");
    let asks = v["events"].as_array().unwrap();
    assert_eq!(asks.len(), 1, "{v}");
    assert_eq!(asks[0]["data"]["question"], "why?");
}

#[test]
fn an_overview_update_moves_node_threads_by_label() {
    let t = serve(true);
    let doc =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/overview/sample-overview-r1.json"))
            .unwrap();
    let put = |text: &str| {
        t.send(
            "PUT",
            "/api/overview",
            &json!({"overview": serde_json::from_str::<Value>(text).unwrap()}).to_string(),
        )
    };
    assert_eq!(put(&doc)["success"], true);
    let node = json!([{"type": "thread", "filePath": "@map", "position": {"side": "new", "line": 1}, "body": "x",
        "target": {"kind": "node", "node": "voucher-service"}}]);
    t.send("POST", "/api/comment-imports", &node.to_string());
    assert_eq!(put(&doc.replace("\"voucher-service\"", "\"voucher-svc\""))["success"], true);
    let moved = |t: &T| {
        let s = t.shared.lock();
        (s.threads[0].anchor_state, s.threads[0].anchor_how)
    };
    assert_eq!(moved(&t), (AnchorState::Moved, Some(AnchorHow::Label)));
    t.send("POST", "/api/notes", r#"{"kind":"story","body":"later"}"#);
    assert_eq!(
        moved(&t),
        (AnchorState::Moved, Some(AnchorHow::Label)),
        "a later pass keeps the move"
    );
}
