use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rain_core::{Session, SessionRepo, Shared, Side, ThreadState, UiRequest, UiState};
use rain_server::{ServerHandle, ServerOptions, read_lock, start_with};
use serde_json::{Value, json};
use ureq::Agent;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures").join(rel)
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(fixture(rel)).unwrap()
}

struct T {
    shared: Arc<Shared>,
    server: ServerHandle,
    agent: Agent,
}

fn serve(opts: ServerOptions) -> T {
    let shared = Arc::new(Shared::new(Session::default()));
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
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.server.url)
    }

    fn get(&self, path: &str) -> (u16, Value) {
        let mut r = self.agent.get(&self.url(path)).call().unwrap();
        let text = r.body_mut().read_to_string().unwrap();
        (r.status().as_u16(), serde_json::from_str(&text).unwrap_or(Value::String(text)))
    }

    fn send(&self, method: &str, path: &str, body: &str) -> (u16, Value) {
        let url = self.url(path);
        let json = "application/json";
        let mut r = match method {
            "POST" => self.agent.post(&url).header("Content-Type", json).send(body),
            "PUT" => self.agent.put(&url).header("Content-Type", json).send(body),
            _ => self.agent.delete(&url).call(),
        }
        .unwrap();
        let text = r.body_mut().read_to_string().unwrap();
        (r.status().as_u16(), serde_json::from_str(&text).unwrap())
    }

    fn import(&self) {
        self.send("POST", "/api/comment-imports", &read("difit/import.json"));
    }
}

fn ids(v: &Value) -> Vec<String> {
    v["threads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn delete_resolves_keeps_history_and_hides_unless_all() {
    let t = serve(ServerOptions::default());
    t.import();
    let (status, r) = t.send("DELETE", "/api/comments/t-old", "");
    assert_eq!(status, 200);
    assert_eq!(r, json!({ "success": true, "threadId": "t-old", "version": 2 }));
    let s = t.shared.lock();
    let old = s.threads.iter().find(|t| t.id == "t-old").unwrap();
    assert_eq!(old.state, ThreadState::Resolved);
    assert!(old.state_at.is_some());
    drop(s);

    let (_, open) = t.get("/api/comments-json");
    assert!(!ids(&open).contains(&"t-old".to_owned()));
    let (_, all) = t.get("/api/comments-json?all=1");
    assert!(ids(&all).contains(&"t-old".to_owned()));
    assert_eq!(all["threads"].as_array().unwrap().len(), 3);
    assert_eq!(open["version"], all["version"]);

    let (_, text) = t.get("/api/comments-output");
    let (_, all_text) = t.get("/api/comments-output?all=1");
    assert!(text.as_str().unwrap().contains("Total comments: 2"), "{text}");
    assert!(all_text.as_str().unwrap().contains("Total comments: 3"));

    let (_, again) = t.send("DELETE", "/api/comments/t-old", "");
    assert_eq!(again["version"], 2, "resolving twice changes nothing");
}

#[test]
fn state_route_sets_addressed_dismissed_and_reopens() {
    let t = serve(ServerOptions::default());
    t.import();
    let (status, r) = t.send("POST", "/api/comments/t-line/state", r#"{"state":"addressed","sha":"abc123"}"#);
    assert_eq!(status, 200, "{r}");
    assert_eq!((r["changed"].clone(), r["threadId"].clone()), (json!(true), json!("t-line")));
    let (_, all) = t.get("/api/comments-json?all=1");
    let line = all["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == "t-line")
        .unwrap()
        .clone();
    assert_eq!(
        (line["state"].clone(), line["stateSha"].clone()),
        (json!("addressed"), json!("abc123"))
    );

    let (status, r) = t.send("POST", "/api/comments/t-line/state", r#"{"state":"dismissed"}"#);
    assert_eq!((status, r["error"].clone()), (400, json!("Dismissing needs a reason")));
    let (status, _) = t.send("POST", "/api/comments/t-line/state", r#"{"state":"dismissed","reason":"style"}"#);
    assert_eq!(status, 200);
    let s = t.shared.lock();
    let th = s.threads.iter().find(|t| t.id == "t-line").unwrap();
    assert_eq!(th.state, ThreadState::Dismissed);
    assert_eq!(th.dismiss_reason, Some(rain_core::DismissReason::Style));
    assert_eq!(th.state_sha, None);
    drop(s);

    let (_, r) = t.send("POST", "/api/comments/t-line/state", r#"{"state":"open"}"#);
    assert_eq!(r["changed"], true);
    let (_, open) = t.get("/api/comments-json");
    assert!(ids(&open).contains(&"t-line".to_owned()));

    let (status, _) = t.send("POST", "/api/comments/nope/state", r#"{"state":"resolved"}"#);
    assert_eq!(status, 404);
    let (status, _) = t.send("POST", "/api/comments/t-line/state", r#"{"state":"gone"}"#);
    assert_eq!(status, 400);
}

#[test]
fn claim_route_takes_and_gives_back_a_pending_question() {
    let t = serve(ServerOptions::default());
    t.import();
    let (_, r) = t.send("POST", "/api/comments/t-line/claim", "");
    assert_eq!(
        (r["claimed"].clone(), r["changed"].clone()),
        (json!(false), json!(false)),
        "nothing pending"
    );
    t.shared.lock().threads.iter_mut().find(|t| t.id == "t-line").unwrap().answer = Some(rain_core::Answer::default());
    let (status, r) = t.send("POST", "/api/comments/t-line/claim", "");
    assert_eq!(status, 200, "{r}");
    assert_eq!((r["claimed"].clone(), r["changed"].clone()), (json!(true), json!(true)));
    let a = t
        .shared
        .lock()
        .threads
        .iter()
        .find(|t| t.id == "t-line")
        .unwrap()
        .answer
        .clone()
        .unwrap();
    assert_eq!((a.status, a.by.as_deref()), (rain_core::AnswerStatus::Answering, Some("attached")));
    assert_eq!(
        t.send("POST", "/api/comments/t-line/claim", "{}").1["changed"],
        false,
        "claiming twice changes nothing"
    );
    let (_, r) = t.send("POST", "/api/comments/t-line/claim", r#"{"release":true}"#);
    assert_eq!((r["claimed"].clone(), r["changed"].clone()), (json!(false), json!(true)));
    let a = t
        .shared
        .lock()
        .threads
        .iter()
        .find(|t| t.id == "t-line")
        .unwrap()
        .answer
        .clone()
        .unwrap();
    assert_eq!(a.status, rain_core::AnswerStatus::Queued);
    assert_eq!(t.send("POST", "/api/comments/nope/claim", "").0, 404);
    assert_eq!(t.send("POST", "/api/comments/t-line/claim", "[1]").0, 400);
}

#[test]
fn legacy_events_shape_is_untouched_and_hides_resolved() {
    let t = serve(ServerOptions::default());
    t.import();
    t.send("DELETE", "/api/comments/t-old", "");
    let (_, v) = t.get("/api/events?since=0&timeout=1");
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["changed", "notes", "threads", "version"]);
    assert_eq!(v["threads"].as_array().unwrap().len(), 2);
}

#[test]
fn typed_events_wake_on_a_user_reply_but_not_on_claudes_own() {
    let t = Arc::new(serve(ServerOptions::default()));
    t.import();
    let (_, now) = t.get("/api/events?kinds=&timeout=0");
    let since = now["seq"].as_u64().unwrap();
    assert!(since >= 3, "the import made one thread event per thread: {now}");

    let waiter = {
        let t = t.clone();
        std::thread::spawn(move || {
            let at = Instant::now();
            let r = t.get(&format!("/api/events?kinds=reply&for=agent&author=claude&since={since}&timeout=10"));
            (r, at.elapsed())
        })
    };
    std::thread::sleep(Duration::from_millis(300));
    let own = r#"{"type":"reply","threadId":"t-old","body":"Mine.","author":"claude"}"#;
    t.send("POST", "/api/comment-imports", own);
    std::thread::sleep(Duration::from_millis(500));
    assert!(!waiter.is_finished(), "claude's own reply does not wake claude");

    let theirs = r#"{"type":"reply","threadId":"t-old","body":"Why?","author":"user"}"#;
    t.send("POST", "/api/comment-imports", theirs);
    let ((status, v), took) = waiter.join().unwrap();
    assert_eq!(status, 200);
    assert!(took < Duration::from_secs(5), "{took:?}");
    assert_eq!(v["changed"], true);
    let events = v["events"].as_array().unwrap();
    assert_eq!(events.len(), 1, "{v}");
    assert_eq!(
        (events[0]["kind"].clone(), events[0]["author"].clone(), events[0]["thread"].clone()),
        (json!("reply"), json!("user"), json!("t-old"))
    );
    assert_eq!(v["seq"], events[0]["seq"]);
    assert_eq!(v["presence"]["state"], "idle", "for=agent is a heartbeat");
    for k in ["version", "seq", "changed", "events", "presence"] {
        assert!(v.get(k).is_some(), "{k}");
    }

    let (_, later) = t.get(&format!("/api/events?kinds=reply&since={}&timeout=0", v["seq"]));
    assert_eq!(later["events"], json!([]), "since returns only newer events");
    assert_eq!(later["changed"], false);
    let (_, both) = t.get(&format!("/api/events?kinds=reply,thread&since={since}&timeout=0"));
    assert_eq!(both["events"].as_array().unwrap().len(), 2, "both replies, no author filter");

    let (status, bad) = t.get("/api/events?kinds=nope");
    assert_eq!(status, 400, "{bad}");
}

#[test]
fn typed_events_report_a_gap_once_the_ring_dropped_some() {
    let t = serve(ServerOptions::default());
    t.import();
    let (_, now) = t.get("/api/events?kinds=&since=0&timeout=0");
    assert_eq!(now["gap"], false, "{now}");
    for _ in 0..rain_core::session::EVENT_RING_CAP {
        t.shared.push_event(rain_core::Event::default());
    }
    let (_, lost) = t.get("/api/events?kinds=&since=0&timeout=0");
    assert_eq!(lost["gap"], true);
    let (_, kept) = t.get(&format!("/api/events?kinds=&since={}&timeout=0", lost["seq"]));
    assert_eq!(kept["gap"], false);
}

#[test]
fn presence_round_trips_into_health() {
    let t = serve(ServerOptions::default());
    let (_, h) = t.get("/api/health");
    assert_eq!(h["agent"]["state"], "offline");
    let (status, r) = t.send("POST", "/api/presence", r#"{"state":"working","on":"t-line"}"#);
    assert_eq!((status, r["success"].clone()), (200, json!(true)));
    let (_, h) = t.get("/api/health");
    assert_eq!(h["agent"]["state"], "working");
    assert_eq!(h["agent"]["seenAgo"], 0);
    let (_, p) = t.get("/api/presence");
    assert_eq!(p["on"], "t-line");
    let (_, ev) = t.get("/api/events?kinds=presence&since=0&timeout=0");
    assert_eq!(ev["events"].as_array().unwrap().len(), 1);
    let (status, _) = t.send("POST", "/api/presence", r#"{"state":"dancing"}"#);
    assert_eq!(status, 400);
}

#[test]
fn health_carries_round_three_fields() {
    let t = serve(ServerOptions::default());
    t.shared.lock().repos.push(SessionRepo {
        id: "server".into(),
        base: "b1".into(),
        target: "a1b2c3d4".into(),
        ..Default::default()
    });
    let (_, h) = t.get("/api/health");
    assert_eq!(h["targetSha"], json!([{ "repo": "server", "base": "b1", "target": "a1b2c3d4" }]));
    assert_eq!(h["diffLoaded"], false);
    assert_eq!(h["round"], 1);
    assert_eq!(h["asks"], json!({ "running": 0, "queued": 0 }));
    assert_eq!(h["lock"], Value::Null);
    t.shared.set_diff_files(Vec::new());
    t.shared.set_asks(rain_core::AskCounts { running: 1, queued: 2 });
    assert_eq!(h["stale"], false);
    t.shared.set_stale(true);
    let (_, h) = t.get("/api/health");
    assert_eq!(h["stale"], true, "HEAD moved since the load");
    assert_eq!(h["diffLoaded"], true);
    assert_eq!(h["asks"], json!({ "running": 1, "queued": 2 }));
}

#[test]
fn state_reflects_the_published_ui_state() {
    let t = serve(ServerOptions::default());
    t.shared.set_ui_state(UiState {
        file: Some("server/a.cs".into()),
        line: Some(12),
        zoomed: true,
        ..UiState::default()
    });
    rain_comments::ops::set_viewed(&mut t.shared.lock(), "server", "a.cs", "h1");
    let (status, s) = t.get("/api/state");
    assert_eq!(status, 200);
    assert_eq!(
        (s["file"].clone(), s["line"].clone(), s["zoomed"].clone()),
        (json!("server/a.cs"), json!(12), json!(true))
    );
    assert_eq!(s["viewed"][0]["key"], "server/a.cs");
    assert_eq!(s["viewed"][0]["hash"], "h1");
    assert_eq!(s["round"], 1);
    assert!(s.get("mode").is_some() && s.get("reader").is_some());
}

#[test]
fn reload_close_and_highlight_queue_requests() {
    let t = serve(ServerOptions::default());
    let (status, r) = t.send("POST", "/api/reload", "{}");
    assert_eq!((status, r["success"].clone()), (200, json!(true)));
    let (status, _) = t.send("POST", "/api/close", "{}");
    assert_eq!(status, 200);
    let body = r#"{"path":"server\\a.cs","lines":[3,9],"side":"old","caption":"The check","step":[2,5]}"#;
    let (status, r) = t.send("POST", "/api/highlight", body);
    assert_eq!(status, 200, "{r}");
    assert_eq!(
        t.shared.take_requests(),
        vec![
            UiRequest::Reload,
            UiRequest::Close,
            UiRequest::Highlight {
                path: "server/a.cs".into(),
                lines: (3, 9),
                side: Side::Old,
                caption: Some("The check".into()),
                step: Some((2, 5)),
            },
        ]
    );
    let (_, ev) = t.get("/api/events?kinds=reload&since=0&timeout=0");
    assert_eq!(ev["events"].as_array().unwrap().len(), 1);

    for (bad, field) in [
        (r#"{"lines":[1,2]}"#, "path"),
        (r#"{"path":"a","lines":[3,1]}"#, "lines"),
        (r#"{"path":"a","lines":[0,1]}"#, "lines"),
        (r#"{"path":"a","lines":[1,2],"side":"left"}"#, "side"),
        (r#"{"path":"a","lines":[1,2],"step":[1]}"#, "step"),
    ] {
        let (status, r) = t.send("POST", "/api/highlight", bad);
        assert_eq!(
            (status, r["error"].clone()),
            (400, json!(format!("Invalid highlight field: {field}"))),
            "{bad}"
        );
    }
    assert!(t.shared.take_requests().is_empty());
}

#[test]
fn lock_file_names_the_running_server_and_goes_with_it() {
    let dir = std::env::temp_dir().join(format!("rain-server-lock-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let session = dir.join("ACME-1.json");
    let t = serve(ServerOptions {
        session_file: Some(session.clone()),
        ..ServerOptions::default()
    });
    let path = dir.join("ACME-1.lock");
    assert_eq!(t.server.lock.as_deref(), Some(path.as_path()));
    let lock = read_lock(&path).unwrap();
    assert_eq!((lock.pid, lock.port), (std::process::id(), t.server.port));
    assert_eq!(lock.started_at.len(), 24);
    assert!(!lock.is_stale());
    let (_, h) = t.get("/api/health");
    assert_eq!(h["lock"]["path"], path.to_string_lossy().as_ref());
    let (status, l) = t.get("/api/lock");
    assert_eq!((status, l["lock"]["pid"].clone()), (200, json!(std::process::id())));

    drop(t);
    assert!(!path.exists(), "dropping the server removes its lock");
    // The acceptors release their sockets asynchronously after being unblocked.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !lock.is_stale() {
        assert!(Instant::now() < deadline, "the stopped server still answers on its port");
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn two_servers_in_one_process_cannot_both_own_a_session() {
    let dir = std::env::temp_dir().join(format!("rain-server-same-pid-{}", std::process::id()));
    let opts = ServerOptions {
        session_file: Some(dir.join("S.json")),
        ..ServerOptions::default()
    };
    let first = serve(opts.clone());
    let second = serve(opts);
    let read_only = second.server.read_only;
    drop(second);
    let owner = read_lock(first.server.lock.as_ref().unwrap()).map(|l| l.port);
    let first_port = first.server.port;
    drop(first);
    let _ = std::fs::remove_dir_all(dir);
    assert!(read_only, "the port, not just the process, identifies a window");
    assert_eq!(owner, Some(first_port), "dropping the second server must preserve the first lock");
}

#[test]
fn a_server_that_cannot_take_its_lock_opens_read_only_and_says_why() {
    let dir = std::env::temp_dir().join(format!("rain-server-lock-error-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let blocked = dir.join("not-a-directory");
    std::fs::write(&blocked, "keep").unwrap();
    let t = serve(ServerOptions {
        session_file: Some(blocked.join("S.json")),
        ..ServerOptions::default()
    });
    assert!(
        t.server.read_only && t.server.lock.is_none(),
        "a failed lock write must not grant write access"
    );
    let cause = t.server.lock_error.clone().unwrap();
    let one = r#"{"type":"thread","filePath":"a.cs","position":{"side":"new","line":1},"body":"x"}"#;
    let (status, refused) = t.send("POST", "/api/comment-imports", one);
    assert_eq!(status, 423);
    let why = refused["error"].as_str().unwrap();
    assert!(why.contains("could not be taken") && why.contains(&cause), "{why}");
    assert!(t.shared.lock().threads.is_empty());
    assert_eq!(t.get("/api/comments-json").0, 200, "reads still work");
    drop(t);
    assert_eq!(std::fs::read_to_string(&blocked).unwrap(), "keep", "nothing was saved");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn shutdown_refuses_a_mutation_whose_body_arrives_after_the_final_save() {
    use std::io::{Read, Write};
    let t = serve(ServerOptions::default());
    let body = r#"{"type":"thread","filePath":"a.cs","position":{"side":"new","line":1},"body":"late"}"#;
    let mut socket = std::net::TcpStream::connect(("127.0.0.1", t.server.port)).unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(
        socket,
        "POST /api/comment-imports HTTP/1.1\r\nHost: localhost:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n",
        t.server.port,
        body.len()
    )
    .unwrap();
    let mut interim = Vec::new();
    while !interim.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        socket.read_exact(&mut byte).unwrap();
        interim.push(byte[0]);
        assert!(interim.len() < 4096);
    }
    assert!(interim.starts_with(b"HTTP/1.1 100 Continue\r\n"));
    t.server.stop_then(|| assert!(t.shared.lock().threads.is_empty()));
    socket.write_all(body.as_bytes()).unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 503"), "{response}");
    assert!(t.shared.lock().threads.is_empty(), "no mutation after shutdown returns");
}

/// A live process that isn't this one.
fn sleeper() -> std::process::Child {
    let mut c = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/c", "pause"]);
        c
    } else {
        std::process::Command::new("cat")
    };
    c.stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
fn a_window_that_finds_a_live_lock_serves_but_never_saves() {
    let dir = std::env::temp_dir().join(format!("rain-server-held-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let session = dir.join("fresh").join("HELD.json");
    let lock_file = session.with_extension("lock");
    let one = r#"{"type":"thread","filePath":"a.cs","position":{"side":"new","line":1},"body":"x"}"#;

    let first = serve(ServerOptions {
        session_file: Some(session.clone()),
        ..ServerOptions::default()
    });
    assert!(!first.server.read_only);
    assert_eq!(read_lock(&lock_file).map(|l| l.pid), Some(std::process::id()), "made its folder");
    drop(first);

    // A busy window: its port takes connections but doesn't answer in time.
    let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut other = sleeper();
    let held = json!({ "port": busy.local_addr().unwrap().port(), "pid": other.id(), "startedAt": "t" });
    std::fs::write(&lock_file, held.to_string()).unwrap();
    let t = serve(ServerOptions {
        session_file: Some(session.clone()),
        ..ServerOptions::default()
    });
    assert!(t.server.read_only && t.server.lock.is_none());
    let (status, refused) = t.send("POST", "/api/comment-imports", one);
    assert_eq!(status, 423, "a write that would be lost is refused, not answered with success");
    let why = refused["error"].as_str().unwrap();
    assert!(
        why.contains("read-only") && why.contains(&format!("port {}", held["port"])),
        "{why}"
    );
    assert!(t.shared.lock().threads.is_empty());
    for (method, path, body) in [
        ("POST", "/api/notes", r#"{"kind":"story","body":"n"}"#),
        ("PUT", "/api/brief", r#"{"format":"rain-brief/v1"}"#),
        ("DELETE", "/api/comments/x", ""),
        ("POST", "/api/comments/x/state", r#"{"state":"resolved"}"#),
    ] {
        assert_eq!(t.send(method, path, body).0, 423, "{method} {path}");
    }
    assert_eq!(
        t.send("POST", "/api/goto", r#"{"path":"a.cs","line":1}"#).0,
        200,
        "steering still works"
    );
    assert_eq!(t.get("/api/comments-json").0, 200, "reads still work");
    assert!(!session.exists(), "never saved over the other window's session");
    drop(t);
    assert_eq!(read_lock(&lock_file).map(|l| l.pid), Some(other.id()), "its lock is left alone");

    other.kill().unwrap();
    other.wait().unwrap();
    let t = serve(ServerOptions {
        session_file: Some(session.clone()),
        ..ServerOptions::default()
    });
    assert!(!t.server.read_only, "a silent port whose process has exited is stale");
    assert_eq!(read_lock(&lock_file).map(|l| l.pid), Some(std::process::id()));
    t.send("POST", "/api/comment-imports", one);
    assert!(session.exists());
    drop(t);
    drop(busy);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_state_post_without_an_id_is_a_404_not_a_panic() {
    let t = serve(ServerOptions::default());
    let (status, _) = t.send("POST", "/api/comments/state", r#"{"state":"resolved"}"#);
    assert_eq!(status, 404);
    assert_eq!(t.get("/api/health").0, 200, "the server is unharmed");
}

#[test]
fn health_answers_while_long_polls_fill_every_request_slot() {
    use std::io::Write;
    let t = serve(ServerOptions::default());
    let port = t.server.port;
    let mut polls = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let req = format!("GET /api/events?timeout=20 HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
        s.write_all(req.as_bytes()).unwrap();
        polls.push(s);
        // The probe itself can take the last slot before a poll does. Keep adding polls
        // until admission actually refuses the probe, rather than assuming all were admitted.
        if t.get("/api/comments-json").0 == 503 {
            break;
        }
        assert!(Instant::now() < deadline, "the long polls never filled the slots");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Once the slots are full, health still answers as Rain.
    let (status, h) = t.get("/api/health");
    assert_eq!((status, h["app"].clone()), (200, json!("rain")));
    drop(polls);
}

#[test]
fn only_a_caller_with_the_windows_token_starts_its_answerer() {
    let t = serve(ServerOptions::default());
    assert_eq!(t.server.token.len(), 32);
    let question = |line: u32| {
        json!([{"type": "thread", "filePath": "a.cs", "position": {"side": "new", "line": line}, "body": format!("why {line}?"), "kind": "question"}])
            .to_string()
    };
    let asks = |t: &T| {
        let (_, v) = t.get("/api/events?kinds=ask&since=0&timeout=1");
        v["events"].as_array().unwrap().len()
    };
    let answer = |t: &T, line: u32| {
        let s = t.shared.lock();
        let th = s.threads.iter().find(|x| x.messages[0].body == format!("why {line}?")).unwrap();
        th.answer.clone()
    };

    t.send("POST", "/api/comment-imports", &question(1));
    assert_eq!(asks(&t), 0, "no token: the question is kept, but nothing runs");
    assert_eq!(answer(&t, 1), None);

    let queued = json!([{"type": "thread", "filePath": "a.cs", "position": {"side": "new", "line": 2}, "body": "why 2?",
        "kind": "question", "author": "claude", "answer": {"status": "queued"}}]);
    t.send("POST", "/api/comment-imports", &queued.to_string());
    assert_eq!(answer(&t, 2), None, "nor can it queue an answer directly");

    let mut r = t
        .agent
        .post(&t.url("/api/comment-imports"))
        .header("Content-Type", "application/json")
        .header(rain_server::TOKEN_HEADER, &t.server.token)
        .send(question(3))
        .unwrap();
    assert_eq!(r.status().as_u16(), 200, "{}", r.body_mut().read_to_string().unwrap());
    assert_eq!(asks(&t), 1, "the window's own CLI carries the token");
    assert_eq!(answer(&t, 3).map(|a| a.status), Some(rain_core::AnswerStatus::Queued));

    let mut r = t
        .agent
        .post(&t.url("/api/comment-imports"))
        .header("Content-Type", "application/json")
        .header(rain_server::TOKEN_HEADER, "0".repeat(32))
        .send(question(4))
        .unwrap();
    assert_eq!(r.status().as_u16(), 200, "{}", r.body_mut().read_to_string().unwrap());
    assert_eq!(answer(&t, 4), None, "a wrong token is no token");
}

#[test]
fn the_lock_file_keeps_the_token_and_the_lock_route_does_not_serve_it() {
    let dir = std::env::temp_dir().join(format!("rain-server-token-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let session = dir.join("TOK.json");
    let t = serve(ServerOptions {
        session_file: Some(session.clone()),
        ..ServerOptions::default()
    });
    let on_disk = read_lock(&session.with_extension("lock")).unwrap();
    assert_eq!(on_disk.token.as_deref(), Some(t.server.token.as_str()));
    assert_eq!(
        rain_server::token_for_port(&dir, t.server.port).as_deref(),
        Some(t.server.token.as_str())
    );
    let (_, l) = t.get("/api/lock");
    assert!(l["lock"]["pid"].is_number() && l["lock"].get("token").is_none(), "{l}");
    let (_, h) = t.get("/api/health");
    assert!(!h.to_string().contains(&t.server.token));
    drop(t);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn brief_put_validates_and_get_reports_stale() {
    let t = serve(ServerOptions::default());
    let (_, empty) = t.get("/api/brief");
    assert_eq!((empty["brief"].clone(), empty["version"].clone()), (Value::Null, json!(0)));
    for bad in ["[1]", "\"x\"", "nope", r#"{"format":"rain-brief/v9"}"#, r#"{"how":"x"}"#] {
        let (status, _) = t.send("PUT", "/api/brief", bad);
        assert_eq!(status, 400, "{bad}");
    }

    t.shared.lock().repos.push(SessionRepo {
        id: "server".into(),
        target: "a1b2c3d4e5".into(),
        ..Default::default()
    });
    let mut doc: Value = serde_json::from_str(&read("brief/sample.json")).unwrap();
    doc["writtenAgainst"] = json!([{ "repo": "server", "sha": "a1b2c3d" }]);
    let (status, r) = t.send("PUT", "/api/brief", &doc.to_string());
    assert_eq!(status, 200, "{r}");
    assert_eq!((r["version"].clone(), r["stale"].clone()), (json!(1), json!(false)));
    let (_, got) = t.get("/api/brief");
    assert_eq!(got["stale"], false);
    assert_eq!(got["brief"]["title"], doc["title"]);
    assert!(got["check"].is_object());
    {
        let mut s = t.shared.lock();
        s.add_verify("Mine").unwrap();
        s.verified.push("Mine".into());
    }
    let (_, got) = t.get("/api/brief");
    let items = got["verify"]["items"].as_array().unwrap();
    assert_eq!(items.last().unwrap(), &json!({"text": "Mine", "done": true, "source": "added"}));

    doc["writtenAgainst"] = json!([{ "repo": "server", "sha": "fff0000" }]);
    let (_, r) = t.send("PUT", "/api/brief", &doc.to_string());
    assert_eq!((r["version"].clone(), r["stale"].clone()), (json!(2), json!(true)));
    let (_, got) = t.get("/api/brief");
    assert_eq!(got["stale"], true);
    let (_, ev) = t.get("/api/events?kinds=brief&since=0&timeout=0");
    assert_eq!(ev["events"].as_array().unwrap().len(), 2);
}

#[test]
fn triage_reads_what_the_window_published() {
    let t = serve(ServerOptions::default());
    let (_, before) = t.get("/api/triage");
    assert_eq!(before["diffLoaded"], false);
    t.shared
        .set_triage(json!({ "shape": "tiny", "groups": [], "items": [{ "key": "a.cs" }] }));
    let (_, after) = t.get("/api/triage");
    assert_eq!(after["items"][0]["key"], "a.cs");
}

#[test]
fn review_rounds_post_list_and_get() {
    let t = serve(ServerOptions::default());
    t.import();
    t.send(
        "POST",
        "/api/comments/t-line/state",
        r#"{"state":"dismissed","reason":"duplicate"}"#,
    );
    let (status, _) = t.send("POST", "/api/review", r#"{"summary":"x"}"#);
    assert_eq!(status, 400);
    let (status, r) = t.send("POST", "/api/review", r#"{"verdict":"request-changes","summary":"Two blockers."}"#);
    assert_eq!((status, r["round"].clone()), (200, json!(1)));
    let (_, all) = t.get("/api/review");
    assert_eq!(all["round"], 2);
    assert_eq!(all["rounds"][0]["verdict"], "request-changes");
    let (status, one) = t.get("/api/review/1");
    assert_eq!(status, 200);
    assert_eq!(one["summary"], "Two blockers.");
    let outcome = one["dispositions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["thread"] == "t-line")
        .unwrap();
    assert_eq!(outcome["dismissReason"], "duplicate");
    let (status, _) = t.get("/api/review/7");
    assert_eq!(status, 404);
    let (_, ev) = t.get("/api/events?kinds=submitted&since=0&timeout=0");
    assert_eq!(ev["events"][0]["data"]["round"], 1);
}

#[test]
fn new_routes_keep_the_browser_defences() {
    let t = serve(ServerOptions::default());
    for path in ["/api/presence", "/api/reload", "/api/close", "/api/highlight", "/api/review"] {
        let url = t.url(path);
        let plain = t.agent.post(&url).header("Content-Type", "text/plain").send("{}").unwrap();
        assert_eq!(plain.status().as_u16(), 415, "{path}");
        let page = t
            .agent
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Origin", "https://evil.example")
            .send("{}")
            .unwrap();
        assert_eq!(page.status().as_u16(), 403, "{path}");
    }
    let put = t
        .agent
        .put(t.url("/api/brief"))
        .header("Content-Type", "text/plain")
        .send("{}")
        .unwrap();
    assert_eq!(put.status().as_u16(), 415);
    assert!(t.shared.take_requests().is_empty());
}
