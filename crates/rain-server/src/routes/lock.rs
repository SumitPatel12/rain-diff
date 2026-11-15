//! `sessions/<id>.lock`: which port and process serve a session, so a second `rain open` or a CLI
//! call with `--session` finds the running window instead of starting another.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{Ctx, Reply, Req, error, value};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lock {
    pub port: u16,
    pub pid: u32,
    pub started_at: String,
}

impl Lock {
    /// The lock names a dead window: nothing answers `GET /api/health` on its port, or what
    /// answers is another process.
    pub fn is_stale(&self) -> bool {
        health_pid(self.port) != Some(self.pid)
    }
}

/// The lock file beside a session file: `sessions/<id>.json` → `sessions/<id>.lock`.
pub fn lock_path(session_file: &Path) -> PathBuf {
    session_file.with_extension("lock")
}

/// The lock at `path`, if the file exists and parses. Says nothing about whether it is live:
/// see [`Lock::is_stale`].
pub fn read_lock(path: &Path) -> Option<Lock> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub(crate) fn write(path: &Path, port: u16) {
    let lock = Lock {
        port,
        pid: std::process::id(),
        started_at: rain_comments::clock::now_iso(),
    };
    let text = serde_json::to_string(&lock).unwrap_or_default();
    if let Err(e) = std::fs::write(path, text) {
        eprintln!("Rain: writing {} failed: {e}", path.display());
    }
}

/// Removes the lock only while it still names this process and port, so a window that took the
/// session over keeps its lock.
pub(crate) fn remove(path: &Path, port: u16) {
    if read_lock(path).is_some_and(|l| l.pid == std::process::id() && l.port == port) {
        let _ = std::fs::remove_file(path);
    }
}

/// The `pid` field of the health reply on `port`, read with a bare HTTP/1.0 request.
fn health_pid(port: u16) -> Option<u32> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(500)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let req = format!("GET /api/health HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    s.write_all(req.as_bytes()).ok()?;
    let mut text = String::new();
    s.take(64 * 1024).read_to_string(&mut text).ok()?;
    let body = text.split_once("\r\n\r\n")?.1;
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    (v["app"] == "rain").then_some(())?;
    v["pid"].as_u64().and_then(|p| u32::try_from(p).ok())
}

pub(crate) fn route(ctx: &Ctx, r: &Req) -> Option<Reply> {
    if !(r.method == &tiny_http::Method::Get && r.path == "/api/lock") {
        return None;
    }
    Some(match &ctx.lock {
        Some(path) => value(200, &json!({ "path": path, "lock": read_lock(path) })),
        None => error(404, "This window has no session file, so no lock"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_sits_beside_the_session_file() {
        assert_eq!(
            lock_path(Path::new("C:/x/sessions/ACME-1.json")),
            Path::new("C:/x/sessions/ACME-1.lock")
        );
    }

    #[test]
    fn a_lock_on_a_closed_port_is_stale() {
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let lock = Lock {
            port,
            pid: std::process::id(),
            started_at: String::new(),
        };
        assert!(lock.is_stale());
    }
}
