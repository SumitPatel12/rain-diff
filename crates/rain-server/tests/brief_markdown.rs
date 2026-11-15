//! `PUT /api/brief` with `Content-Type: text/markdown`: the same Brief as the JSON, the session's
//! shas as `writtenAgainst`, errors with a line, and no other route taking Markdown.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rain_core::{Session, SessionRepo, Shared};
use rain_server::{ServerHandle, ServerOptions, start_with};
use serde_json::Value;
use ureq::Agent;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/brief").join(name)).unwrap()
}

fn server() -> (Arc<Shared>, ServerHandle, Agent) {
    let shared = Arc::new(Shared::new(Session {
        repos: vec![
            SessionRepo {
                id: "server".into(),
                target: "a1b2c3d".into(),
                ..SessionRepo::default()
            },
            SessionRepo {
                id: "app".into(),
                target: "e4f5a6b".into(),
                ..SessionRepo::default()
            },
        ],
        ..Session::default()
    }));
    let server = start_with(
        shared.clone(),
        ServerOptions {
            preferred: Some(0),
            ..ServerOptions::default()
        },
    )
    .unwrap();
    let agent: Agent = Agent::config_builder()
        .http_status_as_error(false)
        .proxy(None)
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into();
    (shared, server, agent)
}

fn put(agent: &Agent, url: &str, content_type: &str, body: &str) -> (u16, Value) {
    let mut r = agent.put(url).header("Content-Type", content_type).send(body).unwrap();
    let text = r.body_mut().read_to_string().unwrap();
    (r.status().as_u16(), serde_json::from_str(&text).unwrap())
}

#[test]
fn a_markdown_brief_lands_as_the_same_brief() {
    let (shared, server, agent) = server();
    let url = format!("{}/api/brief", server.url);
    let (code, got) = put(&agent, &url, "text/markdown; charset=utf-8", &fixture("sample.md"));
    assert_eq!(code, 200, "{got}");
    assert_eq!(
        (got["success"].clone(), got["stale"].clone()),
        (Value::Bool(true), Value::Bool(false))
    );
    let md = shared.lock().brief.clone().unwrap();

    let (code, _) = put(&agent, &url, "application/json", &fixture("sample.json"));
    assert_eq!(code, 200);
    let json = shared.lock().brief.clone().unwrap();
    assert_eq!(md.written_against, json.written_against, "the session's shas");
    let ids = |d: &rain_core::BriefDoc| d.sections().iter().map(|s| s.id.to_owned()).collect::<Vec<_>>();
    assert_eq!(ids(&md), ids(&json));
    assert_eq!(
        (&md.invariants, &md.not_covered, &md.verify),
        (&json.invariants, &json.not_covered, &json.verify)
    );
}

#[test]
fn a_bad_markdown_brief_is_refused_with_its_line() {
    let (shared, server, agent) = server();
    let url = format!("{}/api/brief", server.url);
    let (code, got) = put(&agent, &url, "text/markdown", "# T\n## Intent\nx\n## Risks\ny\n");
    assert_eq!(code, 400);
    assert!(
        got["error"]
            .as_str()
            .unwrap()
            .starts_with("Invalid brief: line 4: unknown section `## Risks`"),
        "{got}"
    );
    assert!(shared.lock().brief.is_none());
}

#[test]
fn markdown_is_taken_only_by_the_brief_put() {
    let (_, server, agent) = server();
    let (code, _) = put(&agent, &format!("{}/api/notes", server.url), "text/markdown", "# T\n");
    assert_eq!(code, 415);
    let mut r = agent
        .post(format!("{}/api/brief", server.url))
        .header("Content-Type", "text/markdown")
        .send("# T\n")
        .unwrap();
    assert_eq!(r.status().as_u16(), 415, "{}", r.body_mut().read_to_string().unwrap());
}
