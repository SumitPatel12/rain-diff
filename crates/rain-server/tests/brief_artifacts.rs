//! `/api/brief`: the check against the window's files, and threads on Brief sections surviving a
//! rewrite.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rain_core::{AnchorState, OrphanReason, RepoFiles, Session, SessionRepo, Shared, Target, TargetKind};
use rain_server::{ServerOptions, start_with};
use serde_json::Value;
use ureq::Agent;

fn sample() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/brief/sample.json")).unwrap()
}

fn m_files() -> Vec<RepoFiles> {
    let keys = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/brief/m-keys.txt")).unwrap();
    ["app", "server"]
        .iter()
        .map(|id| RepoFiles {
            id: (*id).into(),
            files: keys
                .lines()
                .filter_map(|k| k.strip_prefix(&format!("{id}/")))
                .map(str::to_owned)
                .collect(),
            generated: keys
                .lines()
                .filter(|k| k.contains("/i18n/"))
                .filter_map(|k| k.strip_prefix(&format!("{id}/")))
                .map(str::to_owned)
                .collect(),
            formatting_only: Vec::new(),
        })
        .collect()
}

fn call(agent: &Agent, method: &str, url: &str, body: &str) -> (u16, Value) {
    let mut r = match method {
        "PUT" => agent.put(url).header("Content-Type", "application/json").send(body),
        _ => agent.get(url).call(),
    }
    .unwrap();
    let text = r.body_mut().read_to_string().unwrap();
    (r.status().as_u16(), serde_json::from_str(&text).unwrap())
}

#[test]
fn check_reads_the_published_files_and_removed_sections_orphan_their_threads() {
    let shared = Arc::new(Shared::new(Session {
        repos: vec![
            SessionRepo {
                id: "server".into(),
                target: "a1b2c3d".into(),
                ..SessionRepo::default()
            },
            SessionRepo {
                id: "app".into(),
                target: "ffff000".into(),
                ..SessionRepo::default()
            },
        ],
        ..Session::default()
    }));
    shared.set_diff_files(m_files());
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
    let url = format!("{}/api/brief", server.url);

    let (code, _) = call(&agent, "PUT", &url, &sample());
    assert_eq!(code, 200);
    let (_, got) = call(&agent, "GET", &url, "");
    assert_eq!(got["stale"], true);
    assert_eq!(got["check"]["phantomPaths"][0], "server/src/Services/VoucherService.cs");
    assert_eq!(got["check"]["filesNotCited"].as_array().unwrap().len(), 24);
    assert_eq!(got["check"]["stale"][0]["repo"], "app");

    let id = {
        let mut s = shared.lock();
        let snap = rain_comments::ops::brief_snapshot(s.brief.as_ref().unwrap(), "how.2");
        rain_comments::ops::add_thread_with(
            &mut s,
            "",
            rain_core::comments::BRIEF_PATH,
            rain_core::Anchor::Line {
                side: rain_core::Side::New,
                line: 1,
            },
            "Why the same unit of work?",
            None,
            snap,
            rain_comments::ops::ThreadExtra {
                target: Some(Target {
                    kind: TargetKind::Brief,
                    section: Some("how.2".into()),
                    ..Target::default()
                }),
                ..Default::default()
            },
        )
    };
    let mut doc: Value = serde_json::from_str(&sample()).unwrap();
    doc["how"].as_array_mut().unwrap().remove(1);
    let (code, put) = call(&agent, "PUT", &url, &doc.to_string());
    assert_eq!(code, 200);
    assert_eq!(put["threadsMoved"], 1);
    let s = shared.lock();
    let t = s.threads.iter().find(|t| t.id == id).unwrap();
    assert_eq!(t.anchor_state, AnchorState::Orphaned);
    let o = t.orphan.as_ref().unwrap();
    assert_eq!(o.reason, OrphanReason::SectionRemoved);
    assert!(o.snapshot.content.contains("One Voucher row in the same unit of work"));
    assert!(o.snapshot.content.contains("so a failed payment never leaves a voucher behind"));
}
