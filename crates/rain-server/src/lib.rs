//! difit-compatible HTTP routes over the shared session, so `difit comment get --port N` and the
//! story script keep working against a running Rain, plus Rain's own routes (`routes/*`).

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;

use rain_core::{Session, Shared};
use serde::Serialize;
use serde_json::json;
use tiny_http::{Header, Method, Request, Response, Server};

mod routes;

pub use routes::lock::{Lock, lock_path, read_lock};
pub use routes::overview::{overview_state, parse_links, set_overview};

/// difit's default port; taken ports are skipped upwards.
pub const DEFAULT_PORT: u16 = 4966;
const PROBE_PORTS: u16 = 100;
/// difit's `express.json({ limit: '50mb' })`.
pub const BODY_LIMIT: u64 = 50 * 1024 * 1024;
/// `GET /api/events` waits this many seconds by default, and at most the max.
pub const EVENTS_TIMEOUT: u64 = 25;
pub const EVENTS_MAX_TIMEOUT: u64 = 60;

#[derive(Clone, Debug, Default)]
pub struct ServerOptions {
    /// First port to try; `Some(0)` lets the OS pick one.
    pub preferred: Option<u16>,
    /// Where every mutation is saved; `None` keeps changes in memory only. Its `.lock` sibling
    /// names this server while it runs.
    pub session_file: Option<PathBuf>,
    /// Checkout folder name matched against the overview's `repos[].dir` when `PUT /api/overview`
    /// names no `self`.
    pub repo_folder: Option<String>,
}

/// A running server; dropping it stops accepting requests and removes its lock file.
pub struct ServerHandle {
    pub port: u16,
    pub url: String,
    /// `sessions/<id>.lock` while the server runs; `None` for an in-memory session.
    pub lock: Option<PathBuf>,
    servers: Vec<Arc<Server>>,
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        for s in &self.servers {
            s.unblock();
        }
        if let Some(path) = &self.lock {
            routes::lock::remove(path, self.port);
        }
    }
}

/// Binds 127.0.0.1 (and ::1 when the machine has it, so `localhost` works either way) from
/// `preferred.unwrap_or(4966)` upwards and serves on background threads.
pub fn start(shared: Arc<Shared>, preferred: Option<u16>) -> io::Result<ServerHandle> {
    start_with(
        shared,
        ServerOptions {
            preferred,
            ..ServerOptions::default()
        },
    )
}

pub fn start_with(shared: Arc<Shared>, opts: ServerOptions) -> io::Result<ServerHandle> {
    let (servers, port) = bind(opts.preferred)?;
    let mut lock = opts.session_file.as_deref().map(lock_path);
    if let Some(path) = &lock {
        match routes::lock::read_lock(path).filter(|l| l.pid != std::process::id() && !l.is_stale()) {
            Some(live) => {
                eprintln!(
                    "Rain: another window (pid {}, port {}) holds {}; this one leaves its lock alone",
                    live.pid,
                    live.port,
                    path.display()
                );
                lock = None;
            }
            None => routes::lock::write(path, port),
        }
    }
    let ctx = Arc::new(Ctx {
        port,
        shared,
        session_file: opts.session_file,
        repo_folder: opts.repo_folder,
        lock: lock.clone(),
    });
    for server in &servers {
        let server = server.clone();
        let ctx = ctx.clone();
        std::thread::Builder::new().name(format!("rain-http-{port}")).spawn(move || {
            for req in server.incoming_requests() {
                let ctx = ctx.clone();
                let _ = std::thread::Builder::new()
                    .name("rain-http-req".into())
                    .spawn(move || handle(&ctx, req));
            }
        })?;
    }
    Ok(ServerHandle {
        port,
        url: format!("http://localhost:{port}"),
        lock,
        servers,
    })
}

fn boxed(e: Box<dyn std::error::Error + Send + Sync>) -> io::Error {
    io::Error::other(e.to_string())
}

/// Another process listens on the wildcard address. Checked first because Windows lets a
/// loopback bind succeed next to someone else's wildcard listener (Node binds `::`).
fn wildcard_taken(port: u16, v6: bool) -> bool {
    let taken = |addr: &str| TcpListener::bind(addr).is_err_and(|e| e.kind() == io::ErrorKind::AddrInUse);
    taken(&format!("0.0.0.0:{port}")) || (v6 && taken(&format!("[::]:{port}")))
}

fn bind(preferred: Option<u16>) -> io::Result<(Vec<Arc<Server>>, u16)> {
    let first = preferred.unwrap_or(DEFAULT_PORT);
    let v6 = TcpListener::bind("[::1]:0").is_ok();
    if first == 0 {
        for _ in 0..10 {
            let v4 = Server::http("127.0.0.1:0").map_err(boxed)?;
            let port = v4.server_addr().to_ip().map_or(0, |a| a.port());
            if !v6 {
                return Ok((vec![Arc::new(v4)], port));
            }
            if let Ok(s6) = Server::http(format!("[::1]:{port}")) {
                return Ok((vec![Arc::new(v4), Arc::new(s6)], port));
            }
        }
        return Err(io::Error::new(io::ErrorKind::AddrInUse, "no free port pair on loopback"));
    }
    let last = first.saturating_add(PROBE_PORTS);
    for port in first..=last {
        if wildcard_taken(port, v6) {
            continue;
        }
        let Ok(v4) = Server::http(("127.0.0.1", port)) else { continue };
        if !v6 {
            return Ok((vec![Arc::new(v4)], port));
        }
        if let Ok(s6) = Server::http(format!("[::1]:{port}")) {
            return Ok((vec![Arc::new(v4), Arc::new(s6)], port));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AddrInUse,
        format!("no free port in {first}..={last}"),
    ))
}

pub(crate) struct Ctx {
    pub(crate) port: u16,
    pub(crate) shared: Arc<Shared>,
    session_file: Option<PathBuf>,
    pub(crate) repo_folder: Option<String>,
    pub(crate) lock: Option<PathBuf>,
}

impl Ctx {
    pub(crate) fn save(&self, s: &Session) {
        if let Some(path) = &self.session_file
            && let Err(e) = rain_comments::SessionFile::save(path, s)
        {
            eprintln!("Rain: saving {} failed: {e}", path.display());
        }
    }
}

/// One parsed request, as every route module sees it.
pub(crate) struct Req<'a> {
    pub(crate) method: &'a Method,
    pub(crate) path: &'a str,
    pub(crate) query: &'a BTreeMap<String, String>,
    pub(crate) body: &'a str,
    /// The body was sent as `text/markdown` (only `PUT /api/brief` takes it).
    pub(crate) markdown: bool,
}

impl Req<'_> {
    pub(crate) fn repo(&self) -> Option<&str> {
        self.query.get("repo").map(String::as_str).filter(|r| !r.is_empty())
    }

    /// `?<k>=1` or `?<k>=true`.
    pub(crate) fn flag(&self, k: &str) -> bool {
        self.query.get(k).is_some_and(|v| matches!(v.as_str(), "1" | "true"))
    }
}

pub(crate) struct Reply {
    pub(crate) status: u16,
    pub(crate) content_type: &'static str,
    pub(crate) body: String,
}

const JSON: &str = "application/json; charset=utf-8";
pub(crate) const TEXT: &str = "text/plain; charset=utf-8";

pub(crate) fn json_reply(status: u16, body: String) -> Reply {
    Reply {
        status,
        content_type: JSON,
        body,
    }
}

pub(crate) fn value(status: u16, v: &impl Serialize) -> Reply {
    json_reply(status, serde_json::to_string(v).unwrap_or_else(|_| "{}".into()))
}

pub(crate) fn error(status: u16, msg: &str) -> Reply {
    value(status, &json!({ "error": msg }))
}

fn header<'a>(req: &'a Request, name: &'static str) -> Option<&'a str> {
    req.headers().iter().find(|h| h.field.equiv(name)).map(|h| h.value.as_str())
}

/// Only local non-browser clients may talk to the session: comments read back from here become an
/// agent's instructions, so a web page (cross-origin form post, DNS rebinding) must not reach it.
fn refuse_foreign(req: &Request, port: u16) -> Option<Reply> {
    if header(req, "Origin").is_some() {
        return Some(error(403, "Browser requests are not accepted"));
    }
    let host_ok = header(req, "Host").is_some_and(|h| {
        ["localhost", "127.0.0.1", "[::1]"]
            .iter()
            .any(|name| h.eq_ignore_ascii_case(&format!("{name}:{port}")))
    });
    if !host_ok {
        return Some(error(403, "Unexpected Host header"));
    }
    let json = is_media(req, "application/json");
    if matches!(req.method(), Method::Post | Method::Put) && !json && !markdown_brief(req) {
        return Some(error(415, "Content-Type must be application/json"));
    }
    None
}

fn is_media(req: &Request, media: &str) -> bool {
    header(req, "Content-Type").is_some_and(|c| c.split(';').next().is_some_and(|m| m.trim().eq_ignore_ascii_case(media)))
}

/// `PUT /api/brief` may send the Brief as Markdown; a page cannot send that type without a preflight.
fn markdown_brief(req: &Request) -> bool {
    let path = req.url().split('?').next().unwrap_or_default();
    *req.method() == Method::Put && path == "/api/brief" && is_media(req, "text/markdown")
}

fn handle(ctx: &Ctx, mut req: Request) {
    if let Some(r) = refuse_foreign(&req, ctx.port) {
        return respond(req, r);
    }
    let url = req.url().to_owned();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let query = parse_query(query);
    let markdown = markdown_brief(&req);
    let body = match req.method() {
        Method::Post | Method::Put => match read_body(&mut req) {
            Ok(b) => Some(b),
            Err(r) => return respond(req, r),
        },
        _ => None,
    };
    let reply = routes::route(
        ctx,
        &Req {
            method: req.method(),
            path,
            query: &query,
            body: body.as_deref().unwrap_or_default(),
            markdown,
        },
    );
    respond(req, reply);
}

fn respond(req: Request, r: Reply) {
    let mut res = Response::from_string(r.body).with_status_code(r.status);
    if let Ok(h) = Header::from_bytes(&b"Content-Type"[..], r.content_type.as_bytes()) {
        res.add_header(h);
    }
    let _ = req.respond(res);
}

fn read_body(req: &mut Request) -> Result<String, Reply> {
    if req.body_length().is_some_and(|n| n as u64 > BODY_LIMIT) {
        return Err(error(413, "Payload too large"));
    }
    let mut buf = Vec::new();
    req.as_reader()
        .take(BODY_LIMIT + 1)
        .read_to_end(&mut buf)
        .map_err(|_| error(400, "Could not read the request body"))?;
    if buf.len() as u64 > BODY_LIMIT {
        return Err(error(413, "Payload too large"));
    }
    String::from_utf8(buf).map_err(|_| error(400, "The request body is not UTF-8"))
}

fn parse_query(q: &str) -> BTreeMap<String, String> {
    q.split('&')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=').unwrap_or((p, ""));
            (percent_decode(&k.replace('+', " ")), percent_decode(&v.replace('+', " ")))
        })
        .collect()
}

/// `%XX` escapes to bytes, then lossy UTF-8.
pub fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| (c as char).to_digit(16);
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2]))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%2Fb%20c"), "a/b c");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%e2%9c%93"), "\u{2713}");
        assert_eq!(parse_query("repo=server&x=a+b")["x"], "a b");
    }
}
