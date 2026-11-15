//! The subcommands that talk to a running Rain: thin HTTP clients, with difit's outputs and
//! error texts where difit has the command, so scripts parse either.

use std::io::{IsTerminal, Read};
use std::path::Path;
use std::time::{Duration, Instant};

use rain_core::{Note, NoteKind, Side, Thread};
use serde_json::{Value, json};
use ureq::Agent;

use crate::args::{
    Audience, BriefCmd, CommentAdd, CommentCmd, Conn, Format, KindArg, NoteAdd, NoteCmd, OverviewCmd, PresenceArg, ReviewCmd, SideArg, Sub,
    WatchArgs, wire,
};
use crate::launch::format_issues;
use crate::sessions;

enum Fail {
    Connect,
    Other(String),
}

struct Http {
    port: u16,
    agent: Agent,
}

impl Http {
    fn new(port: u16) -> Http {
        Http::with_timeout(port, 60)
    }

    fn with_timeout(port: u16, secs: u64) -> Http {
        let agent = Agent::config_builder()
            .http_status_as_error(false)
            .proxy(None)
            .timeout_global(Some(Duration::from_secs(secs)))
            .build()
            .into();
        Http { port, agent }
    }

    fn call(&self, method: &str, path: &str, body: Option<String>) -> Result<(u16, String), Fail> {
        self.call_as(method, path, body, "application/json")
    }

    fn call_as(&self, method: &str, path: &str, body: Option<String>, media: &str) -> Result<(u16, String), Fail> {
        let url = format!("http://localhost:{}{path}", self.port);
        let sent = match (method, body) {
            ("POST", Some(b)) => self.agent.post(&url).header("Content-Type", media).send(b),
            ("PUT", Some(b)) => self.agent.put(&url).header("Content-Type", media).send(b),
            ("DELETE", _) => self.agent.delete(&url).call(),
            _ => self.agent.get(&url).call(),
        };
        let mut res = sent.map_err(|e| match e {
            ureq::Error::Io(_) | ureq::Error::ConnectionFailed | ureq::Error::HostNotFound => Fail::Connect,
            e => Fail::Other(e.to_string()),
        })?;
        let status = res.status().as_u16();
        let text = res
            .body_mut()
            .with_config()
            .limit(u64::MAX)
            .read_to_string()
            .map_err(|e| Fail::Other(e.to_string()))?;
        Ok((status, text))
    }

    /// Prints a failure as difit does and returns exit code 1.
    fn fail(&self, f: Fail) -> i32 {
        match f {
            Fail::Connect => eprintln!("Error: Cannot connect to Rain server on port {}. Is the server running?", self.port),
            Fail::Other(m) => eprintln!("Error: {m}"),
        }
        1
    }
}

fn error_of(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body).ok()?.get("error")?.as_str().map(str::to_owned)
}

fn repo_query(repo: Option<&str>) -> String {
    repo.map_or_else(String::new, |r| format!("?repo={}", encode(r)))
}

/// `encodeURIComponent`.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn read_stdin_for(what: &str) -> Result<String, String> {
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(format!("Provide {what} as an argument or via stdin"));
    }
    let mut s = String::new();
    stdin.read_to_string(&mut s).map_err(|e| e.to_string())?;
    if s.trim().is_empty() {
        return Err(format!("No {what} received from stdin"));
    }
    Ok(s)
}

/// `TEXT`, `@file` (its contents) or, when absent, stdin.
fn text_arg(v: Option<String>, what: &str) -> Result<String, String> {
    match v {
        Some(v) => match v.strip_prefix('@') {
            Some(path) => std::fs::read_to_string(path).map_err(|_| format!("Cannot read {path}")),
            None => Ok(v),
        },
        None => read_stdin_for(what),
    }
}

fn query(pairs: &[(&str, Option<&str>)]) -> String {
    let q: Vec<String> = pairs
        .iter()
        .filter_map(|(k, v)| v.filter(|v| !v.is_empty()).map(|v| format!("{k}={}", encode(v))))
        .collect();
    if q.is_empty() { String::new() } else { format!("?{}", q.join("&")) }
}

fn report(r: Result<String, String>) -> i32 {
    match r {
        Ok(out) => {
            println!("{out}");
            0
        }
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

impl Http {
    /// A JSON request whose non-2xx reply becomes its `error` text (or `fallback`).
    fn json(&self, method: &str, path: &str, body: Option<String>, fallback: &str) -> Result<String, String> {
        self.send(method, path, body, "application/json", fallback)
    }

    /// [`Http::json`] with a body of another media type; the reply is still JSON.
    fn send(&self, method: &str, path: &str, body: Option<String>, media: &str, fallback: &str) -> Result<String, String> {
        match self.call_as(method, path, body, media) {
            Ok((200..=299, text)) => Ok(text.trim_end().to_owned()),
            Ok((_, text)) => Err(error_of(&text).unwrap_or_else(|| fallback.to_owned())),
            Err(Fail::Connect) => Err(format!(
                "Cannot connect to Rain server on port {}. Is the server running?",
                self.port
            )),
            Err(Fail::Other(m)) => Err(m),
        }
    }
}

/// Runs `f` with the port `conn` names; exit 1 with the reason when it names none that is live.
fn on(conn: &Conn, f: impl FnOnce(u16) -> i32) -> i32 {
    match sessions::resolve(conn) {
        Ok(port) => f(port),
        Err(e) => report(Err(e)),
    }
}

/// Every subcommand but `open`; `version` and `overview validate|render` need no window.
pub fn run(sub: Sub) -> i32 {
    match sub {
        Sub::Open(_) => report(Err("open is not a client command".into())),
        Sub::Comment { cmd } => comment(cmd),
        Sub::Overview { cmd } => overview(cmd),
        Sub::Note { cmd } => note(cmd),
        Sub::Brief { cmd } => brief(cmd),
        Sub::Review {
            cmd: ReviewCmd::Get { round, format, conn },
        } => on(&conn, |p| review_get(p, round, format)),
        Sub::Goto { spec, thread, side, conn } => on(&conn, |p| goto(spec.as_deref(), thread.as_deref(), side, p)),
        Sub::Highlight {
            spec,
            side,
            caption,
            step,
            conn,
        } => on(&conn, |p| {
            let body = highlight_body(&spec.0, spec.1, side, caption.as_deref(), step);
            report(Http::new(p).json("POST", "/api/highlight", Some(body.to_string()), "Failed to highlight"))
        }),
        Sub::Watch(w) => on(&w.conn.clone(), |p| watch(p, &w)),
        Sub::Triage { format, conn } => on(&conn, |p| triage(p, format)),
        Sub::State { conn } => on(&conn, |p| {
            report(Http::new(p).json("GET", "/api/state", None, "Failed to read the state"))
        }),
        Sub::Presence {
            state,
            on: subject,
            reason,
            conn,
        } => on(&conn, |p| {
            let body = presence_body(state, subject.as_deref(), reason.as_deref());
            report(Http::new(p).json("POST", "/api/presence", Some(body.to_string()), "Failed to set presence"))
        }),
        Sub::Reload { conn } => on(&conn, |p| {
            let body = json!({ "author": "claude" }).to_string();
            report(Http::new(p).json("POST", "/api/reload", Some(body), "Failed to reload"))
        }),
        Sub::Close { timeout, conn } => on(&conn, |p| close(p, Duration::from_secs(timeout))),
        Sub::Health { conn } => on(&conn, health),
        Sub::Version => {
            println!("Rain {}", env!("CARGO_PKG_VERSION"));
            0
        }
    }
}

pub fn comment(cmd: CommentCmd) -> i32 {
    match cmd {
        CommentCmd::Add(a) => on(&a.conn.clone(), |p| comment_add(&a, p)),
        CommentCmd::Get { format, repo, all, conn } => on(&conn, |p| comment_get(p, format, repo.as_deref(), all)),
        CommentCmd::Reply {
            thread,
            file,
            line,
            lines,
            side,
            body,
            author,
            repo,
            conn,
        } => on(&conn, |port| {
            comment_reply(
                port,
                thread,
                file.zip(lines.map(|(a, b)| Lines::Range(a, b)).or(line.map(Lines::One)))
                    .map(|(f, l)| (f, l, side)),
                body,
                &author,
                repo.as_deref(),
            )
        }),
        CommentCmd::SetState {
            id,
            state,
            sha,
            reason,
            author,
            conn,
        } => on(&conn, |p| {
            let mut body = json!({ "state": wire(state), "author": author });
            if let Some(sha) = sha {
                body["sha"] = json!(sha);
            }
            if let Some(r) = reason {
                body["reason"] = json!(wire(r));
            }
            let path = format!("/api/comments/{}/state", encode(&id));
            report(Http::new(p).json("POST", &path, Some(body.to_string()), "Failed to set the state"))
        }),
        CommentCmd::Wait {
            since,
            timeout,
            format,
            conn,
        } => on(&conn, |p| comment_wait(p, since, timeout, format)),
        CommentCmd::Resolve {
            ids,
            file,
            line,
            side,
            repo,
            conn,
        } => on(&conn, |port| match file.zip(line) {
            Some((f, l)) => comment_resolve_at(&f, l, side.unwrap_or(SideArg::New), repo.as_deref(), port),
            None => comment_resolve(&ids, port),
        }),
    }
}

/// The lines a thread hangs on: `--line N` or `--lines A-B`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lines {
    One(u32),
    Range(u32, u32),
}

/// The reply import: by thread id, or by the exact file line or range it hangs on.
pub fn reply_import(thread: Option<&str>, at: Option<(&str, Lines, SideArg)>, body: &str, author: &str) -> Value {
    let mut v = json!({ "type": "reply", "body": body, "author": author });
    if let Some(id) = thread {
        v["threadId"] = json!(id);
    } else if let Some((file, lines, side)) = at {
        v["filePath"] = json!(file);
        v["position"] = match lines {
            Lines::One(l) => json!({ "side": side.as_str(), "line": l }),
            Lines::Range(s, e) => json!({ "side": side.as_str(), "line": { "start": s, "end": e } }),
        };
    }
    v
}

fn comment_reply(
    port: u16,
    thread: Option<String>,
    at: Option<(String, Lines, Option<SideArg>)>,
    body: Option<String>,
    author: &str,
    repo: Option<&str>,
) -> i32 {
    let body = match text_arg(body, "the reply text") {
        Ok(b) if !b.trim().is_empty() => b,
        Ok(_) => return report(Err("The reply text is empty".into())),
        Err(e) => return report(Err(e)),
    };
    let at = at.as_ref().map(|(f, l, s)| (f.as_str(), *l, s.unwrap_or(SideArg::New)));
    let import = reply_import(thread.as_deref(), at, body.trim(), author);
    let http = Http::new(port);
    let path = format!("/api/comment-imports{}", repo_query(repo));
    let text = match http.json("POST", &path, Some(import.to_string()), "Failed to add the reply") {
        Ok(t) => t,
        Err(e) => return report(Err(e)),
    };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let warnings: Vec<String> = v
        .get("warnings")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|w| w.as_str().map(str::to_owned))
        .collect();
    println!("{text}");
    for w in &warnings {
        eprintln!("Error: {w}");
    }
    i32::from(!warnings.is_empty())
}

/// `GET /api/events`: prints the change and exits 0, or exits 2 when the wait timed out.
fn comment_wait(port: u16, since: Option<u64>, timeout: u64, format: Format) -> i32 {
    let since = since.map(|s| s.to_string());
    let timeout_s = timeout.to_string();
    let path = format!(
        "/api/events{}",
        query(&[("since", since.as_deref()), ("timeout", Some(&timeout_s))])
    );
    let http = Http::with_timeout(port, timeout.min(60) + 30);
    let text = match http.json("GET", &path, None, "Failed to wait for changes") {
        Ok(t) => t,
        Err(e) => return report(Err(e)),
    };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    let changed = v["changed"].as_bool().unwrap_or(false);
    match format {
        Format::Json => println!("{text}"),
        Format::Text => println!("{}", events_text(&v)),
    }
    if changed { 0 } else { 2 }
}

/// `version N: changed` (or `no change`), then the threads as `comment get` prints them.
pub fn events_text(v: &Value) -> String {
    let version = v["version"].as_u64().unwrap_or(0);
    if !v["changed"].as_bool().unwrap_or(false) {
        return format!("version {version}: no change");
    }
    let threads: Vec<Thread> = serde_json::from_value(v["threads"].clone()).unwrap_or_default();
    let session = rain_core::Session {
        threads,
        ..rain_core::Session::default()
    };
    let out = rain_comments::comments_output(&session, None);
    format!("version {version}: changed\n\n{}", out.trim_end())
}

/// Ids of the threads anchored on `file` (`<repo>/<path>`, or a path within `repo`) whose
/// anchor covers `line` on `side`. An exact `<repo>/<path>` match wins over a bare path match.
pub fn threads_covering(comments_json: &str, file: &str, line: u32, side: SideArg, repo: Option<&str>) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct All {
        threads: Vec<Thread>,
    }
    let Ok(all) = serde_json::from_str::<All>(comments_json) else {
        return Vec::new();
    };
    let side = match side {
        SideArg::Old => Side::Old,
        SideArg::New => Side::New,
    };
    let file = rain_core::paths::normalize(file);
    let covers = |t: &&Thread| {
        let (a, b) = t.anchor.lines();
        t.anchor.side() == side && a <= line && line <= b
    };
    let pick =
        |m: &dyn Fn(&Thread) -> bool| -> Vec<String> { all.threads.iter().filter(covers).filter(|t| m(t)).map(|t| t.id.clone()).collect() };
    if let Some(r) = repo {
        return pick(&|t| t.repo == r && t.file_path == file);
    }
    let exact = pick(&|t| rain_core::paths::join(&t.repo, &t.file_path) == file);
    if exact.is_empty() { pick(&|t| t.file_path == file) } else { exact }
}

fn comment_resolve_at(file: &str, line: u32, side: SideArg, repo: Option<&str>, port: u16) -> i32 {
    let http = Http::new(port);
    let all = match http.json(
        "GET",
        &format!("/api/comments-json{}", repo_query(repo)),
        None,
        "Failed to retrieve comments",
    ) {
        Ok(t) => t,
        Err(e) => return report(Err(e)),
    };
    let ids = threads_covering(&all, file, line, side, repo);
    if ids.is_empty() {
        println!(
            r#"{{"success":false,"resolved":[],"notFound":{},"errors":[]}}"#,
            json!([format!("{file}:{}:{line}", side.as_str())])
        );
        eprintln!("Error: No thread covers {file}:{line} ({} side)", side.as_str());
        return 1;
    }
    comment_resolve(&ids, port)
}

/// The one thread `--body` describes: at `--file` and `--line`/`--lines`, about the whole file
/// (`--file-level`) or the whole change (`--pr-level`).
fn flag_thread(a: &CommentAdd, body: &str) -> Result<Value, String> {
    let mut v = json!({ "type": "thread", "body": body, "author": a.author });
    if a.pr_level {
        return Ok(v);
    }
    let file = a.file.as_deref().ok_or("Give --file <repo>/<path> or --pr-level with --body")?;
    v["filePath"] = json!(file);
    let side = a.side.unwrap_or(SideArg::New).as_str();
    v["position"] = match (a.line, a.lines) {
        (Some(l), _) => json!({ "side": side, "line": l }),
        (None, Some((s, e))) => json!({ "side": side, "line": { "start": s, "end": e } }),
        (None, None) if a.file_level => Value::Null,
        (None, None) => return Err("Give --line, --lines or --file-level with --file".into()),
    };
    Ok(v)
}

/// `--kind`, `--file-level` and `--pr-level` applied to every thread import of `v`: the kind
/// where none is named; a file or whole-change target (at line 1, `@pr` for the change).
pub fn shape_imports(mut v: Value, kind: Option<KindArg>, file_level: bool, pr_level: bool) -> Value {
    let items: Vec<&mut Value> = match &mut v {
        Value::Array(items) => items.iter_mut().collect(),
        one => vec![one],
    };
    for item in items {
        let Value::Object(o) = item else { continue };
        if o.get("type").and_then(Value::as_str) != Some("thread") {
            continue;
        }
        if let Some(k) = kind
            && o.get("kind").is_none_or(Value::is_null)
        {
            o.insert("kind".into(), json!(wire(k)));
        }
        if pr_level {
            o.insert("filePath".into(), json!(rain_core::comments::PR_PATH));
        }
        if file_level || pr_level {
            o.insert("target".into(), json!({ "kind": if pr_level { "pr" } else { "file" } }));
            if o.get("position").is_none_or(Value::is_null) {
                o.insert("position".into(), json!({ "side": "new", "line": 1 }));
            }
        }
    }
    v
}

/// The imports `comment add` posts: from `--body` and its flags, else JSON (`@file`, or stdin).
pub fn add_imports(a: &CommentAdd) -> Result<Vec<rain_core::CommentImport>, String> {
    let v = match &a.body {
        Some(b) => {
            let body = text_arg(Some(b.clone()), "the comment text")?;
            if body.trim().is_empty() {
                return Err("The comment text is empty".into());
            }
            flag_thread(a, body.trim())?
        }
        None => {
            let text = text_arg(a.json.clone(), "comment JSON")?;
            let v = serde_json::from_str(&text).map_err(|_| "Invalid --comment JSON".to_owned())?;
            // An empty author is the reviewer, whose questions the launcher answers.
            with_default_author(v, &a.author)
        }
    };
    rain_comments::import::from_value(shape_imports(v, a.kind, a.file_level, a.pr_level))
}

fn comment_add(a: &CommentAdd, port: u16) -> i32 {
    let imports = match add_imports(a) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    let http = Http::new(port);
    let body = serde_json::to_string(&imports).unwrap_or_default();
    match http.call(
        "POST",
        &format!("/api/comment-imports{}", repo_query(a.repo.as_deref())),
        Some(body),
    ) {
        Ok((200..=299, text)) => {
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let out = format!(
                r#"{{"success":{},"importId":{},"count":{},"warnings":{}}}"#,
                v.get("success").cloned().unwrap_or(Value::Bool(true)),
                v.get("importId").cloned().unwrap_or(Value::Null),
                v.get("count").cloned().unwrap_or(json!(imports.len())),
                v.get("warnings").cloned().unwrap_or(json!([])),
            );
            println!("{out}");
            0
        }
        Ok((_, text)) => {
            eprintln!("Error: {}", error_of(&text).unwrap_or_else(|| "Failed to add comments".into()));
            1
        }
        Err(f) => http.fail(f),
    }
}

fn comment_get(port: u16, format: Format, repo: Option<&str>, all: bool) -> i32 {
    let http = Http::new(port);
    let route = match format {
        Format::Json => "/api/comments-json",
        Format::Text => "/api/comments-output",
    };
    let q = query(&[("repo", repo), ("all", all.then_some("1"))]);
    match http.call("GET", &format!("{route}{q}"), None) {
        Ok((200..=299, text)) => {
            match format {
                Format::Json => println!("{}", text.trim_end()),
                Format::Text if !text.trim().is_empty() => println!("{text}"),
                Format::Text => {}
            }
            0
        }
        Ok(_) => {
            eprintln!("Error: Failed to retrieve comments");
            1
        }
        Err(f) => http.fail(f),
    }
}

fn comment_resolve(ids: &[String], port: u16) -> i32 {
    let http = Http::new(port);
    let (mut resolved, mut not_found, mut errors) = (Vec::new(), Vec::new(), Vec::new());
    for id in ids {
        match http.call("DELETE", &format!("/api/comments/{}", encode(id)), None) {
            Ok((200..=299, _)) => resolved.push(id.clone()),
            Ok((404, _)) => not_found.push(id.clone()),
            Ok((_, text)) => errors.push(json!({
                "threadId": id,
                "error": error_of(&text).unwrap_or_else(|| format!("Failed to resolve thread {id}")),
            })),
            Err(f) => return http.fail(f),
        }
    }
    let ok = not_found.is_empty() && errors.is_empty();
    println!(
        r#"{{"success":{ok},"resolved":{},"notFound":{},"errors":{}}}"#,
        json!(resolved),
        json!(not_found),
        Value::Array(errors)
    );
    i32::from(!ok)
}

fn read_overview(file: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(file).map_err(|_| format!("Cannot read {}", file.display()))?;
    serde_json::from_str(&text).map_err(|_| format!("Invalid JSON in {}", file.display()))
}

pub fn overview(cmd: OverviewCmd) -> i32 {
    match cmd {
        OverviewCmd::Set {
            file,
            self_repo,
            links,
            conn,
        } => on(&conn, |port| overview_set(&file, port, self_repo, links)),
        OverviewCmd::Get { conn } => on(&conn, |port| {
            let http = Http::new(port);
            match http.call("GET", "/api/overview", None) {
                Ok((200..=299, text)) => {
                    println!("{text}");
                    0
                }
                Ok(_) => {
                    eprintln!("Error: Failed to retrieve overview");
                    1
                }
                Err(f) => http.fail(f),
            }
        }),
        OverviewCmd::Validate { file } => overview_validate(&file),
        OverviewCmd::Render { file, out, size } => overview_render(&file, &out, size),
    }
}

/// Prints one PNG path per line; exit 1 when the file is invalid or nothing could be drawn.
fn overview_render(file: &Path, out: &Path, size: (u32, u32)) -> i32 {
    let loaded = match read_overview(file).and_then(|v| rain_overview::load_value(v).map_err(|i| format_issues(&i))) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    match rain_app::render_overview(&loaded, out, size) {
        Ok(paths) if !paths.is_empty() => {
            for p in paths {
                println!("{}", p.display());
            }
            0
        }
        Ok(_) => {
            eprintln!("Error: the overview has no view to draw");
            1
        }
        Err(e) => {
            eprintln!("Error: {e}");
            1
        }
    }
}

fn overview_set(file: &Path, port: u16, self_repo: Option<String>, links: Vec<(String, String)>) -> i32 {
    let doc = match read_overview(file) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    let links: serde_json::Map<String, Value> = links.into_iter().map(|(k, v)| (k, Value::String(v))).collect();
    let mut body = json!({ "overview": doc, "links": links });
    if let Some(s) = self_repo {
        body["self"] = Value::String(s);
    }
    let http = Http::new(port);
    match http.call("PUT", "/api/overview", Some(body.to_string())) {
        Ok((200..=299, text)) => {
            println!("{}", text.trim_end());
            0
        }
        Ok((status, text)) => {
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let msg = v
                .get("error")
                .and_then(Value::as_str)
                .map_or_else(|| format!("Request failed ({status})"), str::to_owned);
            eprintln!("Error: {msg}");
            for i in v.get("issues").and_then(Value::as_array).into_iter().flatten() {
                let f = |k: &str| i.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
                eprintln!("  {}: {} [{}]", f("path"), f("message"), f("code"));
            }
            1
        }
        Err(f) => http.fail(f),
    }
}

/// difit's `valid: …` summary on success; on failure the issues as JSON on stdout (for agents)
/// and difit's issue list on stderr, exit 1.
fn overview_validate(file: &Path) -> i32 {
    let value = match read_overview(file) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };
    match rain_overview::load_value(value) {
        Ok(l) => {
            let d = &l.document;
            let reviews: Vec<_> = d.reading_order.iter().flat_map(|o| o.review.iter()).collect();
            let rated = reviews.iter().filter(|e| e.risk.is_some()).count();
            println!(
                "valid: {} \u{2014} {} repo(s), {} nodes, {} edges, {} flows, {} picture(s), {} notes, {} of {} review files rated",
                d.title,
                d.repos.len(),
                d.graph.nodes.len(),
                d.graph.edges.len(),
                d.graph.flows.len(),
                l.targets.len(),
                d.notes.len(),
                rated,
                reviews.len()
            );
            for w in &l.warnings {
                println!("warning: {w}");
            }
            0
        }
        Err(issues) => {
            println!("{}", json!({ "valid": false, "issues": issues }));
            eprintln!("Error: {}", format_issues(&issues));
            1
        }
    }
}

pub fn note(cmd: NoteCmd) -> i32 {
    match cmd {
        NoteCmd::Add(a) => on(&a.conn.clone(), |p| report(note_add(a, p))),
        NoteCmd::List {
            format,
            file,
            story,
            repo,
            conn,
        } => on(&conn, |p| report(note_list(p, format, file.as_deref(), story, repo.as_deref()))),
        NoteCmd::Remove { ids, conn } => on(&conn, |p| note_remove(&ids, p)),
    }
}

/// The `POST /api/notes` body for the flag form of `note add` (the body text is passed in).
pub fn note_body(a: &NoteAdd, body: &str) -> Value {
    let kind = match (a.story, a.line.is_some() || a.lines.is_some()) {
        (true, _) => "story",
        (false, true) => "line",
        (false, false) => "file",
    };
    let mut v = json!({ "kind": kind, "body": body, "author": a.author.as_deref().unwrap_or("claude") });
    if let Some(f) = a.file.as_deref().filter(|_| !a.story) {
        v["filePath"] = json!(f);
    }
    let side = a.side.unwrap_or(SideArg::New).as_str();
    if let Some(l) = a.line {
        v["position"] = json!({ "side": side, "line": l });
    } else if let Some((s, e)) = a.lines {
        v["position"] = json!({ "side": side, "line": { "start": s, "end": e } });
    }
    if let Some(t) = &a.title {
        v["title"] = json!(t);
    }
    if let Some(id) = &a.id {
        v["id"] = json!(id);
    }
    v
}

/// Gives every note object in `v` (one note or an array) `author` unless it names one.
pub fn with_default_author(mut v: Value, author: &str) -> Value {
    let notes: Vec<&mut Value> = match &mut v {
        Value::Array(items) => items.iter_mut().collect(),
        one => vec![one],
    };
    for n in notes {
        if let Value::Object(o) = n
            && o.get("author").is_none_or(Value::is_null)
        {
            o.insert("author".into(), json!(author));
        }
    }
    v
}

fn note_add(a: NoteAdd, port: u16) -> Result<String, String> {
    let payload = match &a.json {
        Some(j) => {
            let text = text_arg(Some(j.clone()), "note JSON")?;
            let v: Value = serde_json::from_str(&text).map_err(|_| "Invalid note JSON".to_owned())?;
            rain_comments::notes::from_value(v.clone())?;
            with_default_author(v, a.author.as_deref().unwrap_or("claude"))
        }
        None => {
            let body = text_arg(a.body.clone(), "the note text")?;
            if body.trim().is_empty() {
                return Err("The note text is empty".into());
            }
            note_body(&a, body.trim())
        }
    };
    let replace = a.replace.then_some("1");
    let path = format!("/api/notes{}", query(&[("repo", a.repo.as_deref()), ("replace", replace)]));
    Http::new(port).json("POST", &path, Some(payload.to_string()), "Failed to add notes")
}

fn note_list(port: u16, format: Format, file: Option<&str>, story: bool, repo: Option<&str>) -> Result<String, String> {
    let path = format!("/api/notes{}", query(&[("repo", repo), ("filePath", file)]));
    let text = Http::new(port).json("GET", &path, None, "Failed to retrieve notes")?;
    let mut v: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if story && let Some(Value::Array(notes)) = v.get_mut("notes") {
        notes.retain(|n| n["kind"] == "story");
    }
    Ok(match format {
        Format::Json => v.to_string(),
        Format::Text => notes_text(&v),
    })
}

/// One block per note: kind, location, id, author, optional title, then the Markdown body.
pub fn notes_text(v: &Value) -> String {
    let notes: Vec<Note> = v
        .get("notes")
        .cloned()
        .and_then(|n| serde_json::from_value(n).ok())
        .unwrap_or_default();
    notes
        .iter()
        .map(|n| {
            let place = match (&n.kind, &n.anchor) {
                (NoteKind::Story, _) => "the whole change".to_owned(),
                (_, Some(a)) => rain_comments::anchor_label(&n.key(), a),
                (_, None) => n.key(),
            };
            let by = n.author.as_deref().map(|a| format!(" by {a}")).unwrap_or_default();
            let title = n.title.as_deref().map(|t| format!("{t}\n")).unwrap_or_default();
            format!("[{} note] {place} ({}){by}\n{title}{}", n.kind.label(), n.id, n.body)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn note_remove(ids: &[String], port: u16) -> i32 {
    let http = Http::new(port);
    let (mut removed, mut not_found, mut errors) = (Vec::new(), Vec::new(), Vec::new());
    for id in ids {
        match http.call("DELETE", &format!("/api/notes/{}", encode(id)), None) {
            Ok((200..=299, _)) => removed.push(id.clone()),
            Ok((404, _)) => not_found.push(id.clone()),
            Ok((_, text)) => errors.push(json!({
                "id": id,
                "error": error_of(&text).unwrap_or_else(|| format!("Failed to remove note {id}")),
            })),
            Err(f) => return http.fail(f),
        }
    }
    let ok = not_found.is_empty() && errors.is_empty();
    println!(
        r#"{{"success":{ok},"removed":{},"notFound":{},"errors":{}}}"#,
        json!(removed),
        json!(not_found),
        Value::Array(errors)
    );
    i32::from(!ok)
}

/// The `POST /api/goto` body: `{"threadId"}`, or `{"path","line"?,"side"?}` from `path[:line]`.
pub fn goto_body(spec: Option<&str>, thread: Option<&str>, side: Option<SideArg>) -> Value {
    if let Some(id) = thread {
        return json!({ "threadId": id });
    }
    let spec = spec.unwrap_or_default();
    let mut v = match spec.rsplit_once(':') {
        Some((p, l)) if !l.is_empty() && l.chars().all(|c| c.is_ascii_digit()) => {
            json!({ "path": p, "line": l.parse::<u64>().unwrap_or(0) })
        }
        _ => json!({ "path": spec }),
    };
    if let Some(s) = side {
        v["side"] = json!(s.as_str());
    }
    v
}

pub fn goto(spec: Option<&str>, thread: Option<&str>, side: Option<SideArg>, port: u16) -> i32 {
    let body = goto_body(spec, thread, side).to_string();
    report(Http::new(port).json("POST", "/api/goto", Some(body), "Failed to move the window"))
}

pub fn health(port: u16) -> i32 {
    report(Http::new(port).json("GET", "/api/health", None, "Failed to read health"))
}

/// The `POST /api/highlight` body.
pub fn highlight_body(path: &str, lines: (u32, u32), side: Option<SideArg>, caption: Option<&str>, step: Option<(u32, u32)>) -> Value {
    let mut v = json!({ "path": path, "lines": [lines.0, lines.1] });
    if let Some(s) = side {
        v["side"] = json!(s.as_str());
    }
    if let Some(c) = caption {
        v["caption"] = json!(c);
    }
    if let Some((i, n)) = step {
        v["step"] = json!([i, n]);
    }
    v
}

pub fn presence_body(state: PresenceArg, on: Option<&str>, reason: Option<&str>) -> Value {
    let mut v = json!({ "state": wire(state) });
    if let Some(on) = on {
        v["on"] = json!(on);
    }
    if let Some(r) = reason {
        v["reason"] = json!(r);
    }
    v
}

/// The typed-events query: `kinds` is always there (empty = every kind), so the server answers
/// with events rather than the round-2 dump.
pub fn watch_path(w: &WatchArgs) -> String {
    let kinds: Vec<String> = w.kinds.iter().map(|k| wire(*k)).collect();
    let since = w.since.map(|s| s.to_string());
    let timeout = w.timeout.to_string();
    let rest = query(&[
        ("since", since.as_deref()),
        ("timeout", Some(&timeout)),
        ("for", w.for_who.map(|Audience::Agent| "agent")),
        ("author", w.author.as_deref()),
    ]);
    let rest = rest.strip_prefix('?').map(|r| format!("&{r}")).unwrap_or_default();
    format!("/api/events?kinds={}{rest}", encode(&kinds.join(",")))
}

/// Exit 0 with events, 2 when the wait ended with none.
fn watch(port: u16, w: &WatchArgs) -> i32 {
    let http = Http::with_timeout(port, w.timeout.min(60) + 30);
    let text = match http.json("GET", &watch_path(w), None, "Failed to wait for events") {
        Ok(t) => t,
        Err(e) => return report(Err(e)),
    };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    match w.format {
        Format::Json => println!("{text}"),
        Format::Text => println!("{}", watch_text(&v)),
    }
    if v["changed"].as_bool().unwrap_or(false) { 0 } else { 2 }
}

/// `seq N: K event(s)`, then `#seq kind thread by author data` per event.
pub fn watch_text(v: &Value) -> String {
    let seq = v["seq"].as_u64().unwrap_or(0);
    let events = v["events"].as_array().cloned().unwrap_or_default();
    if events.is_empty() {
        return format!("seq {seq}: no events");
    }
    let mut out = format!("seq {seq}: {} event(s)", events.len());
    for e in &events {
        let mut line = format!("\n#{} {}", e["seq"].as_u64().unwrap_or(0), e["kind"].as_str().unwrap_or("?"));
        if let Some(t) = e["thread"].as_str() {
            line.push_str(&format!(" {t}"));
        }
        if let Some(a) = e["author"].as_str() {
            line.push_str(&format!(" by {a}"));
        }
        if !e["data"].is_null() {
            line.push_str(&format!(" {}", e["data"]));
        }
        out.push_str(&line);
    }
    out
}

fn triage(port: u16, format: Format) -> i32 {
    let text = match Http::new(port).json("GET", "/api/triage", None, "Failed to read the triage") {
        Ok(t) => t,
        Err(e) => return report(Err(e)),
    };
    match format {
        Format::Json => report(Ok(text)),
        Format::Text => report(Ok(triage_text(&serde_json::from_str(&text).unwrap_or(Value::Null)))),
    }
}

fn str_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Shape, one line per group, the facts, then the items in reading order with their reasons.
pub fn triage_text(v: &Value) -> String {
    if v.get("shape").is_none_or(Value::is_null) && v["items"].as_array().is_none_or(Vec::is_empty) {
        return "The diff is not loaded yet (or the window publishes no triage); try again in a moment.".into();
    }
    let mut out = vec![format!("shape: {}", str_of(&v["shape"]))];
    for g in v["groups"].as_array().into_iter().flatten() {
        let trust = if g["trusted"].as_bool() == Some(false) {
            " (heuristic)"
        } else {
            ""
        };
        out.push(format!(
            "group {}: {} file(s), {} line(s){trust} · {}",
            str_of(&g["group"]),
            g["count"].as_u64().unwrap_or(0),
            g["lines"].as_u64().unwrap_or(0),
            str_of(&g["reason"])
        ));
    }
    if let Some(facts) = v["facts"].as_object() {
        let facts: Vec<String> = facts
            .iter()
            .filter(|(_, f)| !f.is_null() && f.as_array().is_none_or(|a| !a.is_empty()))
            .map(|(k, f)| format!("{k} {}", str_of(f)))
            .collect();
        out.push(format!("facts: {}", facts.join(" · ")));
    }
    for i in v["items"].as_array().into_iter().flatten() {
        let mut line = format!("{:<15} {}", str_of(&i["group"]), str_of(&i["key"]));
        let reason = str_of(&i["reason"]["text"]);
        if !reason.is_empty() {
            line.push_str(&format!(" · {reason}"));
        }
        if let Some(why) = i["why"].as_str() {
            line.push_str(&format!(" · why: {why}"));
        }
        out.push(line);
    }
    out.join("\n")
}

pub fn brief(cmd: BriefCmd) -> i32 {
    match cmd {
        BriefCmd::Set { brief, conn } => on(&conn, |p| {
            let doc = brief_body(brief);
            report(doc.and_then(|(d, media)| Http::new(p).send("PUT", "/api/brief", Some(d), media, "Failed to set the brief")))
        }),
        BriefCmd::Get { conn } => on(&conn, |p| {
            report(Http::new(p).json("GET", "/api/brief", None, "Failed to read the brief"))
        }),
        BriefCmd::Check { format, conn } => on(&conn, |p| brief_check(p, format)),
    }
}

/// The brief to send and its media type: a `.md` file (with or without `@`) or text starting with
/// `#` goes as Markdown, anything else must be JSON.
fn brief_body(arg: Option<String>) -> Result<(String, &'static str), String> {
    const MD: &str = "text/markdown; charset=utf-8";
    if let Some(a) = &arg {
        let path = a.strip_prefix('@').unwrap_or(a);
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".md") || lower.ends_with(".markdown") {
            return std::fs::read_to_string(path)
                .map(|t| (t, MD))
                .map_err(|_| format!("Cannot read {path}"));
        }
    }
    let t = text_arg(arg, "the brief JSON or Markdown")?;
    if t.trim_start().starts_with('#') {
        return Ok((t, MD));
    }
    serde_json::from_str::<Value>(&t)
        .map(|v| (v.to_string(), "application/json"))
        .map_err(|_| "Invalid brief JSON".to_owned())
}

/// Exit 0 when the check finds nothing, 1 when it finds anything or there is no brief.
fn brief_check(port: u16, format: Format) -> i32 {
    let text = match Http::new(port).json("GET", "/api/brief", None, "Failed to read the brief") {
        Ok(t) => t,
        Err(e) => return report(Err(e)),
    };
    let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
    if v["brief"].is_null() {
        return report(Err("No brief yet: rain brief set '@brief.json'".into()));
    }
    let (out, issues) = check_text(&v["check"], v["stale"].as_bool().unwrap_or(false));
    match format {
        Format::Json => println!("{}", json!({ "stale": v["stale"], "check": v["check"] })),
        Format::Text => println!("{out}"),
    }
    i32::from(issues > 0)
}

/// One line per problem the check reports, and how many there are.
pub fn check_text(check: &Value, stale: bool) -> (String, usize) {
    let list = |k: &str| check[k].as_array().cloned().unwrap_or_default();
    let mut lines = Vec::new();
    for p in list("phantomPaths") {
        lines.push(format!("phantom path: {} (not in the diff)", str_of(&p)));
    }
    for c in list("citesOutsideDiff") {
        lines.push(format!(
            "cite outside the diff: {} cites {}:{}-{}",
            str_of(&c["section"]),
            str_of(&c["path"]),
            c["lines"][0].as_u64().unwrap_or(0),
            c["lines"][1].as_u64().unwrap_or(0)
        ));
    }
    for f in list("filesNotCited") {
        lines.push(format!("not cited: {}", str_of(&f)));
    }
    for s in list("stale") {
        lines.push(format!(
            "stale: {} written against {}, loaded {}",
            str_of(&s["repo"]),
            str_of(&s["written"]),
            str_of(&s["loaded"])
        ));
    }
    if stale && list("stale").is_empty() {
        lines.push("stale: written against another sha than the one loaded".into());
    }
    let n = lines.len();
    if n == 0 {
        return ("brief check: nothing to fix".into(), 0);
    }
    lines.insert(0, format!("brief check: {n} issue(s)"));
    (lines.join("\n"), n)
}

fn review_get(port: u16, round: Option<u32>, format: Format) -> i32 {
    let path = round.map_or_else(|| "/api/review".to_owned(), |n| format!("/api/review/{n}"));
    let text = match Http::new(port).json("GET", &path, None, "Failed to read the review") {
        Ok(t) => t,
        Err(e) => return report(Err(e)),
    };
    match format {
        Format::Json => report(Ok(text)),
        Format::Text => {
            let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            let rounds = match round {
                Some(_) => vec![v.clone()],
                None => v["rounds"].as_array().cloned().unwrap_or_default(),
            };
            let mut out = Vec::new();
            if round.is_none() {
                out.push(format!("in round {} · {} finished", v["round"].as_u64().unwrap_or(1), rounds.len()));
            }
            out.extend(rounds.iter().map(round_text));
            report(Ok(out.join("\n\n")))
        }
    }
}

/// `Round N · verdict · at`, the summary, then what it decided.
pub fn round_text(r: &Value) -> String {
    let count = |k: &str| r[k].as_array().map_or(0, Vec::len);
    let questions: Vec<String> = r["questionsOpen"].as_array().into_iter().flatten().map(str_of).collect();
    let mut s = format!(
        "Round {} · {} · {}\n{}\n{} disposition(s) · {} file(s) viewed · {} group(s) accepted · {} draft(s) unsent",
        r["n"].as_u64().unwrap_or(0),
        str_of(&r["verdict"]),
        str_of(&r["at"]),
        str_of(&r["summary"]),
        count("dispositions"),
        count("viewed"),
        count("accepted"),
        r["draftsUnsent"].as_u64().unwrap_or(0)
    );
    if !questions.is_empty() {
        s.push_str(&format!("\nquestions open: {}", questions.join(", ")));
    }
    s
}

/// `POST /api/close`, then waits for the window's process to end. The window gets a `WM_CLOSE`
/// too when it is still up after a moment, the same path as its close button.
fn close(port: u16, timeout: Duration) -> i32 {
    let http = Http::new(port);
    let health = match http.json("GET", "/api/health", None, "Failed to read health") {
        Ok(t) => serde_json::from_str::<Value>(&t).unwrap_or(Value::Null),
        Err(e) => return report(Err(e)),
    };
    let Some(pid) = health["pid"].as_u64().and_then(|p| u32::try_from(p).ok()) else {
        return report(Err(format!("Port {port} does not answer as Rain")));
    };
    if let Err(e) = http.json("POST", "/api/close", Some("{}".into()), "Failed to close") {
        return report(Err(e));
    }
    let gone = || {
        rain_server::Lock {
            port,
            pid,
            started_at: String::new(),
        }
        .is_stale()
    };
    let started = Instant::now();
    let mut nudged = false;
    while started.elapsed() < timeout {
        if gone() {
            return report(Ok(json!({ "success": true, "pid": pid, "port": port }).to_string()));
        }
        if !nudged && started.elapsed() >= Duration::from_secs(2) {
            nudged = true;
            post_wm_close(pid);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    report(Err(format!(
        "Rain (pid {pid}) is still running {} s after the close request",
        timeout.as_secs()
    )))
}

#[cfg(windows)]
fn post_wm_close(pid: u32) {
    type Hwnd = *mut core::ffi::c_void;
    const WM_CLOSE: u32 = 0x0010;
    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(cb: unsafe extern "system" fn(Hwnd, isize) -> i32, lparam: isize) -> i32;
        fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
        fn IsWindowVisible(hwnd: Hwnd) -> i32;
        fn PostMessageW(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> i32;
    }
    unsafe extern "system" fn each(hwnd: Hwnd, lparam: isize) -> i32 {
        let mut owner = 0u32;
        // SAFETY: `hwnd` comes from EnumWindows; `owner` is a live local.
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut owner);
            if owner as isize == lparam && IsWindowVisible(hwnd) != 0 {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
        }
        1
    }
    // SAFETY: plain Win32 enumeration; the callback touches only the window it is given.
    unsafe {
        EnumWindows(each, pid as isize);
    }
}

#[cfg(not(windows))]
fn post_wm_close(_pid: u32) {}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    use crate::args::{Cli, Sub};

    fn note_add_args(args: &[&str]) -> NoteAdd {
        let cli = Cli::try_parse_from(["rain", "note", "add"].into_iter().chain(args.iter().copied())).unwrap();
        let Some(Sub::Note { cmd: NoteCmd::Add(a) }) = cli.command else {
            panic!()
        };
        a
    }

    #[test]
    fn note_bodies_for_each_kind() {
        let a = note_add_args(&["--port", "1", "--story", "--title", "T"]);
        assert_eq!(
            note_body(&a, "sum"),
            json!({ "kind": "story", "body": "sum", "author": "claude", "title": "T" })
        );
        let a = note_add_args(&["--port", "1", "--file", "server/a.cs", "--author", "me", "--id", "n1"]);
        assert_eq!(
            note_body(&a, "why"),
            json!({ "kind": "file", "filePath": "server/a.cs", "body": "why", "author": "me", "id": "n1" })
        );
        let a = note_add_args(&["--port", "1", "--file", "a.cs", "--line", "12"]);
        assert_eq!(note_body(&a, "b")["position"], json!({ "side": "new", "line": 12 }));
        let a = note_add_args(&["--port", "1", "--file", "a.cs", "--lines", "3-7", "--side", "old"]);
        assert_eq!(
            note_body(&a, "b")["position"],
            json!({ "side": "old", "line": { "start": 3, "end": 7 } })
        );
        assert_eq!(note_body(&a, "b")["kind"], "line");
        rain_comments::notes::from_value(note_body(&a, "b")).unwrap();
    }

    #[test]
    fn json_notes_get_the_default_author_unless_they_name_one() {
        let batch = json!([
            { "kind": "story", "body": "a" },
            { "kind": "story", "body": "b", "author": "me" },
            { "kind": "story", "body": "c", "author": null }
        ]);
        let v = with_default_author(batch, "claude");
        let authors: Vec<&str> = v.as_array().unwrap().iter().map(|n| n["author"].as_str().unwrap()).collect();
        assert_eq!(authors, ["claude", "me", "claude"]);
        let one = with_default_author(json!({ "kind": "story", "body": "a" }), "bot");
        assert_eq!(one["author"], "bot");
    }

    #[test]
    fn body_from_a_file() {
        let f = std::env::temp_dir().join(format!("rain-cli-body-{}.md", std::process::id()));
        std::fs::write(&f, "**from a file**\n").unwrap();
        assert_eq!(text_arg(Some(format!("@{}", f.display())), "x").unwrap(), "**from a file**\n");
        assert_eq!(text_arg(Some("plain".into()), "x").unwrap(), "plain");
        assert_eq!(
            text_arg(Some("@nope/missing.md".into()), "x").unwrap_err(),
            "Cannot read nope/missing.md"
        );
        let _ = std::fs::remove_file(f);
    }

    #[test]
    fn reply_imports() {
        assert_eq!(
            reply_import(Some("t1"), None, "ok", "claude"),
            json!({ "type": "reply", "threadId": "t1", "body": "ok", "author": "claude" })
        );
        let by_line = reply_import(None, Some(("server/a.cs", Lines::One(9), SideArg::Old)), "ok", "claude");
        assert_eq!(by_line["filePath"], "server/a.cs");
        assert_eq!(by_line["position"], json!({ "side": "old", "line": 9 }));
        rain_comments::import::from_value(by_line).unwrap();
        let by_range = reply_import(None, Some(("src/app.ts", Lines::Range(4, 6), SideArg::New)), "ok", "claude");
        assert_eq!(by_range["position"], json!({ "side": "new", "line": { "start": 4, "end": 6 } }));
        rain_comments::import::from_value(by_range).unwrap();
    }

    #[test]
    fn a_range_reply_reaches_the_range_thread() {
        let golden = include_str!("../../../fixtures/difit/comments-json.golden.json");
        #[derive(serde::Deserialize)]
        struct All {
            threads: Vec<Thread>,
        }
        let all: All = serde_json::from_str(golden).unwrap();
        let range = all.threads.iter().find(|t| t.id == "t-range").unwrap();
        let (a, b) = range.anchor.lines();
        assert!(a < b, "t-range is a range thread");
        let mut s = rain_core::Session {
            threads: all.threads.clone(),
            ..rain_core::Session::default()
        };
        let import = reply_import(None, Some(("src/app.ts", Lines::Range(a, b), SideArg::New)), "Done.", "claude");
        let r = rain_comments::import::apply(&mut s, rain_comments::import::from_value(import).unwrap(), "");
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        let t = s.threads.iter().find(|t| t.id == "t-range").unwrap();
        assert_eq!(t.messages.last().unwrap().body, "Done.");
    }

    #[test]
    fn events_as_text() {
        assert_eq!(
            events_text(&json!({ "version": 4, "changed": false, "threads": [], "notes": [] })),
            "version 4: no change"
        );
        let golden: Value = serde_json::from_str(include_str!("../../../fixtures/difit/comments-json.golden.json")).unwrap();
        let v = json!({ "version": 5, "changed": true, "threads": golden["threads"], "notes": [] });
        let text = events_text(&v);
        assert!(text.starts_with("version 5: changed\n\n"), "{text}");
        assert!(text.contains("src/app.ts"), "{text}");
    }

    #[test]
    fn resolve_picks_threads_covering_the_line() {
        let golden = include_str!("../../../fixtures/difit/comments-json.golden.json");
        assert_eq!(threads_covering(golden, "src/app.ts", 5, SideArg::New, None), ["t-range"]);
        assert_eq!(threads_covering(golden, "src/app.ts", 2, SideArg::New, None), ["t-line"]);
        assert!(threads_covering(golden, "src/app.ts", 2, SideArg::Old, None).is_empty());
        assert_eq!(threads_covering(golden, "src/legacy.cs", 7, SideArg::Old, None), ["t-old"]);
        assert!(threads_covering(golden, "src/legacy.cs", 7, SideArg::Old, Some("server")).is_empty());
        let multi = r#"{"version":1,"threads":[
            {"id":"a","repo":"server","filePath":"x.cs","createdAt":"t","updatedAt":"t","position":{"side":"new","line":3},"messages":[]},
            {"id":"b","repo":"app","filePath":"x.cs","createdAt":"t","updatedAt":"t","position":{"side":"new","line":3},"messages":[]}]}"#;
        assert_eq!(threads_covering(multi, "server/x.cs", 3, SideArg::New, None), ["a"]);
        assert_eq!(threads_covering(multi, "x.cs", 3, SideArg::New, Some("app")), ["b"]);
    }

    #[test]
    fn goto_bodies() {
        assert_eq!(
            goto_body(Some("server/a.cs:12"), None, None),
            json!({ "path": "server/a.cs", "line": 12 })
        );
        assert_eq!(
            goto_body(Some("a.cs"), None, Some(SideArg::Old)),
            json!({ "path": "a.cs", "side": "old" })
        );
        assert_eq!(goto_body(None, Some("t1"), None), json!({ "threadId": "t1" }));
    }

    #[test]
    fn note_text_blocks() {
        let v = json!({ "notesVersion": 1, "notes": [
            { "id": "s", "kind": "story", "title": "Vouchers", "body": "Adds vouchers.", "author": "claude", "createdAt": "t", "updatedAt": "t" },
            { "id": "l", "kind": "line", "repo": "server", "filePath": "a.cs", "position": { "side": "old", "line": { "start": 3, "end": 4 } }, "body": "Old total.", "createdAt": "t", "updatedAt": "t" }
        ]});
        assert_eq!(
            notes_text(&v),
            "[story note] the whole change (s) by claude\nVouchers\nAdds vouchers.\n\n[line note] server/a.cs:L3-L4 (old) (l)\nOld total."
        );
    }

    fn add_args(args: &[&str]) -> CommentAdd {
        let cli = Cli::try_parse_from(["rain", "comment", "add"].into_iter().chain(args.iter().copied())).unwrap();
        let Some(Sub::Comment { cmd: CommentCmd::Add(a) }) = cli.command else {
            panic!()
        };
        a
    }

    #[test]
    fn comment_add_reads_at_file_and_applies_kind_and_level() {
        let f = std::env::temp_dir().join(format!("rain-cli-add-{}.json", std::process::id()));
        std::fs::write(
            &f,
            r#"[{"type":"thread","filePath":"server/a.cs","position":{"side":"new","line":3},"body":"Why?"},
                {"type":"thread","filePath":"server/b.cs","position":{"side":"new","line":4},"body":"Fix","kind":"issue"}]"#,
        )
        .unwrap();
        let at = format!("@{}", f.display());
        let imports = add_imports(&add_args(&[&at, "--port", "1", "--kind", "question"])).unwrap();
        let v = serde_json::to_value(&imports).unwrap();
        assert_eq!((v[0]["kind"].as_str(), v[1]["kind"].as_str()), (Some("question"), Some("issue")));
        assert_eq!(v[0]["author"], "claude", "an agent's JSON question is not the reviewer's");
        let file_level = serde_json::to_value(add_imports(&add_args(&[&at, "--port", "1", "--file-level"])).unwrap()).unwrap();
        assert_eq!(file_level[0]["target"]["kind"], "file");
        let _ = std::fs::remove_file(&f);
        assert_eq!(
            add_imports(&add_args(&["@nope/missing.json", "--port", "1"])).unwrap_err(),
            "Cannot read nope/missing.json"
        );

        let pr = serde_json::to_value(add_imports(&add_args(&["--pr-level", "--body", "Ship it?", "--port", "1"])).unwrap()).unwrap();
        assert_eq!(
            (pr[0]["filePath"].as_str(), pr[0]["target"]["kind"].as_str()),
            (Some("@pr"), Some("pr"))
        );
        assert_eq!(pr[0]["author"], "claude");
        let whole = add_imports(&add_args(&["--file-level", "--file", "server/a.cs", "--body", "x", "--port", "1"])).unwrap();
        let whole = serde_json::to_value(whole).unwrap();
        assert_eq!(
            (whole[0]["filePath"].as_str(), whole[0]["target"]["kind"].as_str()),
            (Some("server/a.cs"), Some("file"))
        );
        let line = serde_json::to_value(
            add_imports(&add_args(&[
                "--file", "a.cs", "--lines", "3-5", "--body", "x", "--kind", "nit", "--port", "1",
            ]))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(line[0]["position"], json!({ "side": "new", "line": { "start": 3, "end": 5 } }));
        assert_eq!(line[0]["kind"], "nit");
        assert!(line[0].get("target").is_none());
        assert!(
            add_imports(&add_args(&["--file", "a.cs", "--body", "x", "--port", "1"]))
                .unwrap_err()
                .contains("--file-level")
        );
    }

    #[test]
    fn watch_queries_and_text() {
        let w = |args: &[&str]| {
            let cli = Cli::try_parse_from(["rain", "watch"].into_iter().chain(args.iter().copied())).unwrap();
            let Some(Sub::Watch(w)) = cli.command else { panic!() };
            w
        };
        assert_eq!(watch_path(&w(&[])), "/api/events?kinds=&timeout=25");
        assert_eq!(
            watch_path(&w(&["--for", "agent", "--kinds", "reply,ask", "--since", "9", "--timeout", "2"])),
            "/api/events?kinds=reply%2Cask&since=9&timeout=2&for=agent"
        );
        assert_eq!(watch_text(&json!({ "seq": 4, "changed": false, "events": [] })), "seq 4: no events");
        let v = json!({ "seq": 6, "changed": true, "events": [
            { "seq": 5, "kind": "reply", "thread": "k3", "author": "you", "data": { "messageId": "m1" } },
            { "seq": 6, "kind": "reload" }
        ]});
        assert_eq!(
            watch_text(&v),
            "seq 6: 2 event(s)\n#5 reply k3 by you {\"messageId\":\"m1\"}\n#6 reload"
        );
    }

    #[test]
    fn brief_check_lines() {
        let (text, n) = check_text(&json!({}), false);
        assert_eq!((text.as_str(), n), ("brief check: nothing to fix", 0));
        let check = json!({
            "phantomPaths": ["server/Nope.cs"],
            "citesOutsideDiff": [{ "section": "how.2", "path": "server/A.cs", "lines": [40, 44] }],
            "filesNotCited": ["server/B.cs", "server/C.cs"],
            "stale": [{ "repo": "server", "written": "a1b2c3d", "loaded": "d4e5f6a" }]
        });
        let (text, n) = check_text(&check, true);
        assert_eq!(n, 5);
        assert_eq!(
            text,
            "brief check: 5 issue(s)\nphantom path: server/Nope.cs (not in the diff)\ncite outside the diff: how.2 cites server/A.cs:40-44\nnot cited: server/B.cs\nnot cited: server/C.cs\nstale: server written against a1b2c3d, loaded d4e5f6a"
        );
    }

    #[test]
    fn brief_set_sends_markdown_files_and_text_as_markdown() {
        let md = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/brief/sample.md");
        let json = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/brief/sample.json");
        let media = |a: String| brief_body(Some(a)).map(|(_, m)| m);
        assert_eq!(media(md.display().to_string()), Ok("text/markdown; charset=utf-8"));
        assert_eq!(media(format!("@{}", md.display())), Ok("text/markdown; charset=utf-8"));
        assert_eq!(media("# T\n## Intent\nx\n".into()), Ok("text/markdown; charset=utf-8"));
        assert_eq!(media(format!("@{}", json.display())), Ok("application/json"));
        assert_eq!(media("{\"title\":\"t\"}".into()), Ok("application/json"));
        assert_eq!(media("not json".into()), Err("Invalid brief JSON".into()));
        assert_eq!(media("missing.md".into()), Err("Cannot read missing.md".into()));
    }

    #[test]
    fn triage_and_round_text() {
        assert!(triage_text(&json!({ "diffLoaded": false, "groups": [], "items": [] })).starts_with("The diff is not loaded"));
        let v = json!({
            "shape": "dense",
            "groups": [{ "group": "formatting-only", "count": 5, "lines": 120, "trusted": true, "reason": "-w empty" }],
            "items": [
                { "key": "server/Vouchers.cs", "group": "must", "reason": { "text": "money path" }, "why": "payments entry point" },
                { "key": "server/Fmt.cs", "group": "formatting-only", "reason": { "text": "formatting-only (-w empty)" } }
            ],
            "facts": { "files": 2, "testsOff": [], "uncited": null }
        });
        assert_eq!(
            triage_text(&v),
            "shape: dense\ngroup formatting-only: 5 file(s), 120 line(s) · -w empty\nfacts: files 2\nmust            server/Vouchers.cs · money path · why: payments entry point\nformatting-only server/Fmt.cs · formatting-only (-w empty)"
        );
        let r = json!({ "n": 1, "verdict": "request-changes", "at": "t", "summary": "Two blockers.",
            "dispositions": [{}, {}], "viewed": [{}], "accepted": [], "draftsUnsent": 1, "questionsOpen": ["q1"] });
        assert_eq!(
            round_text(&r),
            "Round 1 · request-changes · t\nTwo blockers.\n2 disposition(s) · 1 file(s) viewed · 0 group(s) accepted · 1 draft(s) unsent\nquestions open: q1"
        );
    }

    #[test]
    fn highlight_and_presence_bodies() {
        assert_eq!(
            highlight_body("server/a.cs", (20, 27), None, Some("The endpoint"), Some((3, 9))),
            json!({ "path": "server/a.cs", "lines": [20, 27], "caption": "The endpoint", "step": [3, 9] })
        );
        assert_eq!(
            highlight_body("a.cs", (7, 7), Some(SideArg::Old), None, None),
            json!({ "path": "a.cs", "lines": [7, 7], "side": "old" })
        );
        assert_eq!(
            presence_body(PresenceArg::Working, Some("k3"), None),
            json!({ "state": "working", "on": "k3" })
        );
        assert_eq!(
            presence_body(PresenceArg::Blocked, None, Some("needs a DB")),
            json!({ "state": "blocked", "reason": "needs a DB" })
        );
    }

    #[test]
    fn query_strings() {
        assert_eq!(query(&[("repo", Some("server")), ("replace", None)]), "?repo=server");
        assert_eq!(query(&[("repo", None), ("filePath", Some("a b/c"))]), "?filePath=a%20b%2Fc");
        assert_eq!(query(&[("repo", Some(""))]), "");
    }

    #[test]
    fn encodes_like_encode_uri_component() {
        assert_eq!(encode("a b/c?d"), "a%20b%2Fc%3Fd");
        assert_eq!(encode("t-1_x.y~"), "t-1_x.y~");
        assert_eq!(repo_query(Some("server")), "?repo=server");
    }
}
