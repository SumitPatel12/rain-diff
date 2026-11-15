use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use rain_core::{Session, Shared};
use rain_server::{ServerHandle, ServerOptions, start, start_with};
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
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();
    T { shared, server, agent }
}

impl T {
    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.server.url)
    }

    fn get(&self, path: &str) -> (u16, String) {
        let mut r = self.agent.get(&self.url(path)).call().unwrap();
        (r.status().as_u16(), r.body_mut().read_to_string().unwrap())
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
}

#[test]
fn browser_shaped_requests_are_refused() {
    let t = serve(ServerOptions::default());
    let one = r#"[{"type":"thread","filePath":"a.cs","position":{"side":"new","line":1},"body":"x"}]"#;
    let url = t.url("/api/comment-imports");
    let status = |r: Result<ureq::http::Response<ureq::Body>, ureq::Error>| r.unwrap().status().as_u16();

    assert_eq!(status(t.agent.post(&url).header("Content-Type", "text/plain").send(one)), 415);
    assert_eq!(status(t.agent.post(&url).send(one)), 415);
    let from_page = t
        .agent
        .post(&url)
        .header("Content-Type", "application/json")
        .header("Origin", "https://evil.example")
        .send(one);
    assert_eq!(status(from_page), 403);
    let rebound = t
        .agent
        .get(t.url("/api/comments-json"))
        .header("Host", format!("evil.example:{}", t.server.port))
        .call();
    assert_eq!(status(rebound), 403);
    assert!(t.shared.lock().threads.is_empty());

    for (path, body) in [
        ("/api/notes", r#"{"kind":"story","body":"x"}"#),
        ("/api/goto", r#"{"path":"a.cs"}"#),
    ] {
        let url = t.url(path);
        assert_eq!(
            status(t.agent.post(&url).header("Content-Type", "text/plain").send(body)),
            415,
            "{path}"
        );
        let from_page = t
            .agent
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Origin", "https://evil.example")
            .send(body);
        assert_eq!(status(from_page), 403, "{path}");
    }
    let delete_from_page = t
        .agent
        .delete(t.url("/api/notes/x"))
        .header("Origin", "https://evil.example")
        .call();
    assert_eq!(status(delete_from_page), 403);
    assert!(t.shared.lock().notes.is_empty());
    assert!(t.shared.take_requests().is_empty());

    let ok = t
        .agent
        .post(&url)
        .header("Content-Type", "application/json; charset=utf-8")
        .send(one);
    assert_eq!(status(ok), 200);
    let via_ip = format!("http://127.0.0.1:{}/api/health", t.server.port);
    assert_eq!(status(t.agent.get(&via_ip).call()), 200);
}

#[test]
fn imports_read_back_byte_equal_to_difit() {
    let t = serve(ServerOptions::default());
    let (status, r) = t.send("POST", "/api/comment-imports", &read("difit/import.json"));
    assert_eq!(status, 200);
    assert_eq!(r["success"], true);
    assert_eq!(r["changed"], true);
    assert_eq!(r["count"], 5);
    assert_eq!(r["warnings"], json!([]));
    assert_eq!(r["importId"].as_str().unwrap().len(), 64);

    let (status, body) = t.get("/api/comments-json");
    assert_eq!(status, 200);
    assert_eq!(body, read("difit/comments-json.golden.json").trim_end());
    let (_, text) = t.get("/api/comments-output");
    assert_eq!(text, read("difit/comments-output.golden.txt"));

    let (_, again) = t.send("POST", "/api/comment-imports", &read("difit/import.json"));
    assert_eq!(again["changed"], false);
    assert_eq!(t.shared.lock().threads.len(), 3);
    assert_eq!(t.shared.lock().version, 1);
}

#[test]
fn import_warnings_and_bad_payloads() {
    let t = serve(ServerOptions::default());
    let reply = r#"{"type":"reply","filePath":"x.ts","position":{"side":"new","line":9},"body":"hi"}"#;
    let (status, r) = t.send("POST", "/api/comment-imports", reply);
    assert_eq!(status, 200);
    assert_eq!(r["changed"], false);
    assert_eq!(
        r["warnings"],
        json!(["Skipped reply import for x.ts:new:9 because no matching thread was found."])
    );
    let (status, r) = t.send("POST", "/api/comment-imports", r#"{"type":"nope"}"#);
    assert_eq!(status, 400);
    assert_eq!(r["error"], "Invalid comment import data");
    let (status, _) = t.send("POST", "/api/comment-imports", "not json");
    assert_eq!(status, 400);
}

#[test]
fn repo_query_scopes_imports_and_reads() {
    let t = serve(ServerOptions::default());
    let one = r#"{"type":"thread","filePath":"a.cs","position":{"side":"new","line":1},"body":"in server"}"#;
    t.send("POST", "/api/comment-imports?repo=server", one);
    let two = r#"{"type":"thread","filePath":"b.ts","position":{"side":"new","line":2},"body":"in app"}"#;
    t.send("POST", "/api/comment-imports?repo=app", two);
    let (_, server) = t.get("/api/comments-json?repo=server");
    let server: Value = serde_json::from_str(&server).unwrap();
    assert_eq!(server["threads"].as_array().unwrap().len(), 1);
    assert_eq!(server["threads"][0]["repo"], "server");
    let (_, all) = t.get("/api/comments-output");
    assert!(all.contains("server/a.cs:L1") && all.contains("app/b.ts:L2"), "{all}");
}

#[test]
fn delete_resolves_or_404s() {
    let t = serve(ServerOptions::default());
    t.send("POST", "/api/comment-imports", &read("difit/import.json"));
    let (status, r) = t.send("DELETE", "/api/comments/missing", "");
    assert_eq!(status, 404);
    assert_eq!(r["error"], "Thread not found: missing");
    let (status, r) = t.send("DELETE", "/api/comments/t-old", "");
    assert_eq!(status, 200);
    assert_eq!(r["threadId"], "t-old");
    assert_eq!(r["version"], 2);
    assert_eq!(t.shared.lock().threads.iter().filter(|t| t.is_open()).count(), 2);
}

#[test]
fn post_comments_merges_on_a_stale_base_version() {
    let t = serve(ServerOptions::default());
    t.send("POST", "/api/comment-imports", &read("difit/import.json"));
    let (_, now) = t.get("/api/comments-json");
    let now: Value = serde_json::from_str(&now).unwrap();
    let mut mine = now["threads"].as_array().unwrap()[..1].to_vec();
    mine.push(json!({
        "id": "t-new", "filePath": "src/new.ts", "createdAt": "2026-11-06T11:00:00.000Z", "updatedAt": "2026-11-06T11:00:00.000Z",
        "position": { "side": "new", "line": 1 },
        "messages": [{ "id": "t-new", "body": "fresh", "createdAt": "2026-11-06T11:00:00.000Z", "updatedAt": "2026-11-06T11:00:00.000Z" }]
    }));

    let stale = json!({ "threads": mine, "baseVersion": 0 }).to_string();
    let (status, r) = t.send("POST", "/api/comments", &stale);
    assert_eq!(status, 200);
    assert_eq!(r["merged"], true);
    assert_eq!(r["threads"].as_array().unwrap().len(), 4, "union keeps the agent's threads");
    let version = r["version"].as_u64().unwrap();
    assert_eq!(version, 2);

    let fresh = json!({ "threads": mine, "baseVersion": version }).to_string();
    let (_, r) = t.send("POST", "/api/comments", &fresh);
    assert_eq!(r["merged"], false);
    assert_eq!(r["threads"].as_array().unwrap().len(), 2, "a current base replaces");
    assert_eq!(r["version"], 3);

    let (status, _) = t.send("POST", "/api/comments", r#"{"threads":[{"bad":1}]}"#);
    assert_eq!(status, 400);
}

#[test]
fn a_viewed_mark_does_not_make_a_post_stale() {
    let t = serve(ServerOptions::default());
    t.send("POST", "/api/comment-imports", &read("difit/import.json"));
    let (_, now) = t.get("/api/comments-json");
    let now: Value = serde_json::from_str(&now).unwrap();
    rain_comments::ops::set_viewed(&mut t.shared.lock(), "", "src/a.ts", "h");
    let one = now["threads"].as_array().unwrap()[..1].to_vec();
    let body = json!({ "threads": one, "baseVersion": now["version"] }).to_string();
    let (_, r) = t.send("POST", "/api/comments", &body);
    assert_eq!(r["merged"], false, "only thread changes count against baseVersion");
    assert_eq!(r["threads"].as_array().unwrap().len(), 1, "a deleted thread stays deleted");
}

#[test]
fn overview_put_validates_and_get_echoes_self_and_links() {
    let file = std::env::temp_dir().join(format!("rain-server-test-{}.json", std::process::id()));
    let t = serve(ServerOptions {
        session_file: Some(file.clone()),
        repo_folder: Some("acme-app-wt-7".into()),
        ..ServerOptions::default()
    });
    let (_, empty) = t.get("/api/overview");
    assert_eq!(
        serde_json::from_str::<Value>(&empty).unwrap(),
        json!({ "version": 0, "overview": null })
    );

    let broken = json!({ "overview": { "format": "difit-overview/v1", "title": "x", "repos": [], "graph": {} } }).to_string();
    let (status, r) = t.send("PUT", "/api/overview", &broken);
    assert_eq!(status, 400);
    assert_eq!(r["error"], "Invalid overview");
    assert!(!r["issues"].as_array().unwrap().is_empty());

    let doc: Value = serde_json::from_str(&read("overview/sample-overview-schema.json")).unwrap();
    let (status, r) = t.send("PUT", "/api/overview", &json!({ "overview": doc, "self": "nope" }).to_string());
    assert_eq!(status, 400);
    assert_eq!(r["issues"][0]["code"], "UNKNOWN_REPO");

    let body = json!({ "overview": doc, "self": "server", "links": { "app": "http://localhost:4967/" } }).to_string();
    let (status, r) = t.send("PUT", "/api/overview", &body);
    assert_eq!(status, 200, "{r}");
    assert_eq!(r["success"], true);
    assert_eq!(r["version"], 1);
    assert_eq!(r["selfRepo"], "server");

    let (_, state) = t.get("/api/overview");
    let state: Value = serde_json::from_str(&state).unwrap();
    assert_eq!(state["version"], 1);
    assert_eq!(state["selfRepo"], "server");
    assert_eq!(state["links"], json!({ "app": "http://localhost:4967" }));
    assert_eq!(state["overview"]["title"], doc["title"]);
    assert!(!state["targets"].as_array().unwrap().is_empty());
    assert!(state["defaultTarget"].is_object());
    assert_eq!(state["coverage"]["repo"], "server");
    assert_eq!(state["diffLoaded"], false);
    assert_eq!(state["coverageByRepo"], json!([]));

    let files = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    t.shared.set_diff_files(vec![
        rain_core::RepoFiles {
            id: "server".into(),
            files: files(&["src/Api/VouchersController.cs", "src/Migrations/X.Designer.cs", "src/Unlisted.cs"]),
            ..Default::default()
        },
        rain_core::RepoFiles {
            id: "app".into(),
            files: files(&["src/app/vouchers/voucher.service.ts", "src/app/generated/voucher.ts"]),
            ..Default::default()
        },
        rain_core::RepoFiles {
            id: "docs".into(),
            files: files(&["README.md"]),
            ..Default::default()
        },
    ]);
    let (_, state) = t.get("/api/overview");
    let state: Value = serde_json::from_str(&state).unwrap();
    assert_eq!(state["diffLoaded"], true);
    assert_eq!(state["coverage"]["review"], json!(["src/Api/VouchersController.cs"]));
    assert_eq!(state["coverage"]["uncovered"], json!(["src/Unlisted.cs"]));
    assert_eq!(state["coverage"]["ignored"], json!(["src/Migrations/X.Designer.cs"]));
    let by_repo = state["coverageByRepo"].as_array().unwrap();
    assert_eq!(by_repo.len(), 2, "repos the overview does not name are left out");
    assert_eq!(by_repo[1]["repo"], "app");
    assert_eq!(by_repo[1]["ignored"], json!(["src/app/generated/voucher.ts"]));
    assert_eq!(by_repo[1]["missingReview"], json!(["src/app/vouchers/voucher-form.component.ts"]));

    let (_, r) = t.send("PUT", "/api/overview", &doc.to_string());
    assert_eq!(r["version"], 2);
    assert_eq!(r["selfRepo"], "app", "folder match when no self is named");

    let saved: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved["overviewVersion"], 2);
    let _ = std::fs::remove_file(&file);
}

#[test]
fn mutations_save_the_session_and_wake_the_ui() {
    let file = std::env::temp_dir().join(format!("rain-server-wake-{}.json", std::process::id()));
    let t = serve(ServerOptions {
        session_file: Some(file.clone()),
        ..ServerOptions::default()
    });
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    t.shared.set_wake(move || {
        h.fetch_add(1, Ordering::SeqCst);
    });
    t.send("POST", "/api/comment-imports", &read("difit/import.json"));
    t.send("DELETE", "/api/comments/t-line", "");
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    let saved: Session = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved.threads.iter().filter(|t| t.is_open()).count(), 2);
    assert_eq!(saved.version, 2);
    let _ = std::fs::remove_file(&file);
}

#[test]
fn health_and_unknown_routes() {
    let t = serve(ServerOptions::default());
    let (status, body) = t.get("/api/health");
    assert_eq!(status, 200);
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["pid"], std::process::id());
    let (status, _) = t.get("/api/nope");
    assert_eq!(status, 404);
}

fn two_repos(t: &T) {
    let mut s = t.shared.lock();
    for id in ["server", "app"] {
        s.repos.push(rain_core::SessionRepo {
            id: id.into(),
            ..Default::default()
        });
    }
}

#[test]
fn notes_add_list_dedupe_replace_and_remove() {
    let file = std::env::temp_dir().join(format!("rain-server-notes-{}.json", std::process::id()));
    let t = serve(ServerOptions {
        session_file: Some(file.clone()),
        ..ServerOptions::default()
    });
    two_repos(&t);
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    t.shared.set_wake(move || {
        h.fetch_add(1, Ordering::SeqCst);
    });

    let (status, r) = t.send("POST", "/api/notes", &read("notes/notes.json"));
    assert_eq!(status, 200, "{r}");
    assert_eq!(
        (r["success"].clone(), r["changed"].clone(), r["count"].clone()),
        (json!(true), json!(true), json!(5))
    );
    let ids: Vec<String> = serde_json::from_value(r["ids"].clone()).unwrap();
    assert_eq!(ids.len(), 5);
    assert_eq!(
        (r["notesVersion"].clone(), r.get("version")),
        (json!(1), None),
        "the notes counter, not the session version"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    let comments_version = t.shared.lock().comments_version;

    let (_, again) = t.send("POST", "/api/notes", &read("notes/notes.json"));
    assert_eq!(again["changed"], false);
    assert_eq!(again["ids"], r["ids"], "duplicates report the stored ids");
    assert_eq!(hits.load(Ordering::SeqCst), 1, "no change, no wake");

    let (status, list) = t.get("/api/notes");
    assert_eq!(status, 200);
    let list: Value = serde_json::from_str(&list).unwrap();
    assert_eq!(list["notesVersion"], 1);
    assert_eq!(list["notes"].as_array().unwrap().len(), 5);

    let first_file = list["notes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "file")
        .unwrap()
        .clone();
    let key = format!(
        "{}/{}",
        first_file["repo"].as_str().unwrap(),
        first_file["filePath"].as_str().unwrap()
    );
    let (_, one) = t.get(&format!("/api/notes?filePath={key}"));
    let one: Value = serde_json::from_str(&one).unwrap();
    assert!(!one["notes"].as_array().unwrap().is_empty());
    assert!(one["notes"].as_array().unwrap().iter().all(|n| n["kind"] != "story"));
    let (_, scoped) = t.get(&format!(
        "/api/notes?repo={}&filePath={}",
        first_file["repo"].as_str().unwrap(),
        first_file["filePath"].as_str().unwrap()
    ));
    assert_eq!(serde_json::from_str::<Value>(&scoped).unwrap()["notes"], one["notes"]);

    let story = r#"{"kind":"story","body":"Second take on the summary.","author":"claude"}"#;
    let (_, replaced) = t.send("POST", "/api/notes?replace=1", story);
    assert_eq!(replaced["changed"], true);
    let s = t.shared.lock();
    let stories: Vec<_> = s.notes.iter().filter(|n| n.kind == rain_core::NoteKind::Story).collect();
    assert_eq!(stories.len(), 1);
    assert_eq!(stories[0].body, "Second take on the summary.");
    assert_eq!(s.comments_version, comments_version, "notes never bump the comment version");
    drop(s);

    let (status, bad) = t.send("POST", "/api/notes", r#"{"kind":"file","body":"x"}"#);
    assert_eq!(status, 400);
    assert_eq!(bad["error"], "Invalid note field: filePath");
    let (status, bad) = t.send("POST", "/api/notes", "nope");
    assert_eq!(status, 400);
    assert_eq!(bad["error"], "Invalid note JSON");

    let (status, gone) = t.send("DELETE", &format!("/api/notes/{}", ids[1]), "");
    assert_eq!(status, 200);
    assert_eq!(gone["success"], true);
    assert_eq!(gone["id"], ids[1].as_str());
    let (status, missing) = t.send("DELETE", &format!("/api/notes/{}", ids[1]), "");
    assert_eq!(status, 404);
    assert_eq!(missing["error"], format!("Note not found: {}", ids[1]));
    let (status, owned) = t.send("DELETE", "/api/notes/overview:n1", "");
    assert_eq!(status, 409);
    assert_eq!(
        owned["error"],
        "Note overview:n1 comes from the overview; push a new overview to change it"
    );

    let saved: Session = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(saved.notes.len(), t.shared.lock().notes.len());
    let _ = std::fs::remove_file(&file);
}

#[test]
fn overview_notes_list_but_cannot_be_removed() {
    let t = serve(ServerOptions::default());
    let doc: Value = serde_json::from_str(&read("overview/sample-overview-schema.json")).unwrap();
    let (status, _) = t.send("PUT", "/api/overview", &json!({ "overview": doc, "self": "server" }).to_string());
    assert_eq!(status, 200);
    let (_, list) = t.get("/api/notes");
    let list: Value = serde_json::from_str(&list).unwrap();
    let notes = list["notes"].as_array().unwrap();
    assert_eq!(notes.len(), doc["notes"].as_array().map_or(0, Vec::len));
    let Some(first) = notes.first() else { return };
    let id = first["id"].as_str().unwrap();
    assert!(id.starts_with("overview:"));
    let (status, _) = t.send("DELETE", &format!("/api/notes/{id}"), "");
    assert_eq!(status, 409);
}

#[test]
fn comments_json_ignores_notes() {
    let t = serve(ServerOptions::default());
    t.send("POST", "/api/comment-imports", &read("difit/import.json"));
    let (_, before_output) = t.get("/api/comments-output");
    t.send("POST", "/api/notes", &read("notes/notes.json"));
    assert_eq!(t.shared.lock().notes.len(), 5);
    let (_, body) = t.get("/api/comments-json");
    assert_eq!(body, read("difit/comments-json.golden.json").trim_end());
    let (_, output) = t.get("/api/comments-output");
    assert_eq!(output, before_output);
    assert_eq!(output, read("difit/comments-output.golden.txt"));
}

#[test]
fn reply_by_thread_id_and_repo_prefixed_paths() {
    let t = serve(ServerOptions::default());
    t.send("POST", "/api/comment-imports", &read("difit/import.json"));
    let reply = r#"{"type":"reply","threadId":"t-old","body":"Because it moved.","author":"claude"}"#;
    let (status, r) = t.send("POST", "/api/comment-imports", reply);
    assert_eq!(status, 200);
    assert_eq!(r["changed"], true);
    assert_eq!(r["warnings"], json!([]));
    let s = t.shared.lock();
    let thread = s.threads.iter().find(|t| t.id == "t-old").unwrap();
    assert_eq!(thread.messages.last().unwrap().body, "Because it moved.");
    drop(s);

    let (_, r) = t.send("POST", "/api/comment-imports", r#"{"type":"reply","threadId":"gone","body":"x"}"#);
    assert_eq!(r["changed"], false);
    assert_eq!(r["warnings"], json!(["No thread with id gone."]));

    two_repos(&t);
    let one = r#"{"type":"thread","filePath":"server/src/x.cs","position":{"side":"new","line":3},"body":"prefixed"}"#;
    let (_, r) = t.send("POST", "/api/comment-imports", one);
    assert_eq!(r["changed"], true);
    let s = t.shared.lock();
    let t2 = s.threads.iter().find(|t| t.messages[0].body == "prefixed").unwrap();
    assert_eq!((t2.repo.as_str(), t2.file_path.as_str()), ("server", "src/x.cs"));
}

#[test]
fn goto_queues_a_request_and_wakes_the_ui() {
    let t = serve(ServerOptions::default());
    t.send("POST", "/api/comment-imports", &read("difit/import.json"));
    let hits = Arc::new(AtomicUsize::new(0));
    let h = hits.clone();
    t.shared.set_wake(move || {
        h.fetch_add(1, Ordering::SeqCst);
    });
    let (status, r) = t.send("POST", "/api/goto", r#"{"path":"src/app.ts","line":4,"side":"old"}"#);
    assert_eq!(status, 200, "{r}");
    assert_eq!(r["success"], true);
    let (status, _) = t.send("POST", "/api/goto", r#"{"path":"src/app.ts"}"#);
    assert_eq!(status, 200);
    let (status, _) = t.send("POST", "/api/goto", r#"{"threadId":"t-range"}"#);
    assert_eq!(status, 200);
    assert_eq!(hits.load(Ordering::SeqCst), 3);
    assert_eq!(
        t.shared.take_requests(),
        vec![
            rain_core::UiRequest::Goto {
                path: "src/app.ts".into(),
                line: Some(4),
                side: rain_core::Side::Old
            },
            rain_core::UiRequest::Goto {
                path: "src/app.ts".into(),
                line: None,
                side: rain_core::Side::New
            },
            rain_core::UiRequest::GotoThread("t-range".into()),
        ]
    );

    let (status, r) = t.send("POST", "/api/goto", r#"{"threadId":"nope"}"#);
    assert_eq!((status, r["error"].clone()), (404, json!("Thread not found: nope")));
    let (status, r) = t.send("POST", "/api/goto", r#"{"path":"a","line":0}"#);
    assert_eq!((status, r["error"].clone()), (400, json!("Invalid goto field: line")));
    let (status, r) = t.send("POST", "/api/goto", r#"{"path":"a","side":"left"}"#);
    assert_eq!((status, r["error"].clone()), (400, json!("Invalid goto field: side")));
    let (status, _) = t.send("POST", "/api/goto", "{}");
    assert_eq!(status, 400);
    assert!(t.shared.take_requests().is_empty());
}

#[test]
fn health_counts_threads_notes_and_lists_repos() {
    let t = serve(ServerOptions::default());
    two_repos(&t);
    t.send("POST", "/api/comment-imports?repo=server", &read("difit/import.json"));
    t.send("POST", "/api/notes", r#"{"kind":"story","body":"s"}"#);
    let (_, body) = t.get("/api/health");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["repos"], json!(["server", "app"]));
    assert_eq!(v["threads"], 3);
    assert_eq!(v["notes"], 1);
}

#[test]
fn a_taken_port_is_skipped() {
    let held = TcpListener::bind("127.0.0.1:0").unwrap();
    let taken = held.local_addr().unwrap().port();
    let server = start(Arc::new(Shared::default()), Some(taken)).unwrap();
    assert!(server.port > taken, "{} should be above {taken}", server.port);
    assert_eq!(server.url, format!("http://localhost:{}", server.port));

    let wildcard = TcpListener::bind("0.0.0.0:0").unwrap();
    let taken = wildcard.local_addr().unwrap().port();
    let server = start(Arc::new(Shared::default()), Some(taken)).unwrap();
    assert!(server.port > taken, "a wildcard listener (Node difit) also counts as taken");
}

fn events(t: &T, query: &str) -> (u16, Value) {
    let (status, body) = t.get(&format!("/api/events{query}"));
    (status, serde_json::from_str(&body).unwrap())
}

#[test]
fn events_answer_at_once_when_the_session_moved_and_time_out_when_not() {
    let t = serve(ServerOptions::default());
    t.send("POST", "/api/comment-imports", &read("difit/import.json"));
    t.send("POST", "/api/notes", r#"{"kind":"story","body":"s"}"#);
    let at = Instant::now();
    let (status, v) = events(&t, "?since=0&timeout=30");
    assert_eq!(status, 200);
    assert!(at.elapsed() < Duration::from_secs(5), "answered at once");
    assert_eq!((v["version"].clone(), v["changed"].clone()), (json!(2), json!(true)));
    let (_, cj) = t.get("/api/comments-json");
    let cj: Value = serde_json::from_str(&cj).unwrap();
    assert_eq!(v["threads"], cj["threads"], "same shape as comments-json");
    assert_eq!(v["notes"][0]["body"], "s");

    let at = Instant::now();
    let (status, v) = events(&t, "?since=2&timeout=1");
    assert_eq!(status, 200);
    assert!(at.elapsed() >= Duration::from_millis(950), "waited the timeout");
    assert_eq!((v["version"].clone(), v["changed"].clone()), (json!(2), json!(false)));
    assert_eq!(v["threads"].as_array().unwrap().len(), 3);

    let (status, v) = events(&t, "?since=x");
    assert_eq!((status, v["error"].clone()), (400, json!("Invalid events field: since")));
    let (status, _) = events(&t, "?timeout=-1");
    assert_eq!(status, 400);
    let from_page = t
        .agent
        .get(t.url("/api/events?since=0"))
        .header("Origin", "https://evil.example")
        .call();
    assert_eq!(from_page.unwrap().status().as_u16(), 403);
}

#[test]
fn a_waiting_events_request_wakes_on_a_change_without_blocking_other_requests() {
    let t = Arc::new(serve(ServerOptions::default()));
    t.send("POST", "/api/comment-imports", &read("difit/import.json"));
    let waiter = {
        let t = t.clone();
        std::thread::spawn(move || {
            let at = Instant::now();
            let r = events(&t, "?timeout=30");
            (r, at.elapsed())
        })
    };
    std::thread::sleep(Duration::from_millis(300));
    let at = Instant::now();
    let (status, _) = t.get("/api/health");
    assert_eq!(status, 200);
    assert!(at.elapsed() < Duration::from_secs(2), "other requests are served meanwhile");
    assert!(!waiter.is_finished(), "still waiting with no change");

    let reply = r#"{"type":"reply","threadId":"t-old","body":"Fixed.","author":"user"}"#;
    t.send("POST", "/api/comment-imports", reply);
    let ((status, v), took) = waiter.join().unwrap();
    assert_eq!(status, 200);
    assert!(took < Duration::from_secs(10), "woke on the change, not the timeout: {took:?}");
    assert_eq!((v["version"].clone(), v["changed"].clone()), (json!(2), json!(true)));
    let old = v["threads"].as_array().unwrap().iter().find(|t| t["id"] == "t-old").unwrap();
    assert_eq!(old["messages"].as_array().unwrap().last().unwrap()["body"], "Fixed.");

    let waiter = {
        let t = t.clone();
        std::thread::spawn(move || events(&t, "?since=2&timeout=30"))
    };
    std::thread::sleep(Duration::from_millis(200));
    t.shared.lock().version += 1;
    t.shared.notify_waiters();
    let (_, v) = waiter.join().unwrap();
    assert_eq!(
        (v["version"].clone(), v["changed"].clone()),
        (json!(3), json!(true)),
        "a change made by the window"
    );
}
