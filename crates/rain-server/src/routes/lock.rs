//! `sessions/<id>.lock`: which port and process serve a session, so a second `rain open` or a CLI
//! call with `--session` finds the running window instead of starting another.

use std::io::{self, Read, Write};
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
    /// The lock names a dead window: nothing listens on its port, what answers is another
    /// process, or the port is silent and the lock's process has exited. A slow reply alone
    /// (a busy window) doesn't make it stale.
    pub fn is_stale(&self) -> bool {
        match health(self.port) {
            Health::Pid(pid) => pid != self.pid,
            Health::Closed | Health::Other => true,
            Health::Silent => !pid_alive(self.pid),
        }
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

/// Takes the lock for this process, or returns the live window that holds it. The file appears
/// whole and only if absent (a hard link of a finished temp file), so two windows starting at
/// once can't both take it. When it can't be created that way, it is written over instead.
pub(crate) fn acquire(path: &Path, port: u16) -> Result<(), Lock> {
    let lock = Lock {
        port,
        pid: std::process::id(),
        started_at: rain_comments::clock::now_iso(),
    };
    let text = serde_json::to_string(&lock).unwrap_or_default();
    for _ in 0..3 {
        match create_new(path, text.as_bytes()) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => match read_lock(path) {
                Some(l) if l.pid == lock.pid => break,
                Some(l) if !l.is_stale() => return Err(l),
                _ => {
                    let _ = std::fs::remove_file(path);
                }
            },
            Err(e) => {
                eprintln!("Rain: creating {} failed: {e}", path.display());
                break;
            }
        }
    }
    if let Some(l) = read_lock(path).filter(|l| l.pid != lock.pid && !l.is_stale()) {
        return Err(l);
    }
    if let Err(e) = rain_comments::store::write_atomic(path, text.as_bytes()) {
        eprintln!("Rain: writing {} failed: {e}", path.display());
    }
    Ok(())
}

fn create_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", std::process::id()));
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    let linked = std::fs::hard_link(&tmp, path);
    let _ = std::fs::remove_file(&tmp);
    linked
}

/// Removes the lock only while it still names this process and port, so a window that took the
/// session over keeps its lock.
pub(crate) fn remove(path: &Path, port: u16) {
    if read_lock(path).is_some_and(|l| l.pid == std::process::id() && l.port == port) {
        let _ = std::fs::remove_file(path);
    }
}

enum Health {
    /// Rain answered with this pid.
    Pid(u32),
    /// Nothing accepts a connection on the port.
    Closed,
    /// Something answered, but not as Rain.
    Other,
    /// Connected, but no full reply in time: a busy window, or a stuck listener.
    Silent,
}

/// `GET /api/health` on `port` with a bare HTTP/1.0 request.
fn health(port: u16) -> Health {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_millis(500)) else {
        return Health::Closed;
    };
    let req = format!("GET /api/health HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    let mut buf = Vec::new();
    let read = s
        .set_read_timeout(Some(Duration::from_secs(2)))
        .and_then(|()| s.write_all(req.as_bytes()))
        .and_then(|()| (&mut s).take(64 * 1024).read_to_end(&mut buf));
    if read.is_err() || buf.is_empty() {
        return Health::Silent;
    }
    let pid = String::from_utf8_lossy(&buf)
        .split_once("\r\n\r\n")
        .and_then(|(_, body)| serde_json::from_str::<serde_json::Value>(body).ok())
        .filter(|v| v["app"] == "rain")
        .and_then(|v| v["pid"].as_u64())
        .and_then(|p| u32::try_from(p).ok());
    pid.map_or(Health::Other, Health::Pid)
}

/// The process `pid` is still running.
pub fn pid_alive(pid: u32) -> bool {
    imp::pid_alive(pid)
}

#[cfg(windows)]
mod imp {
    type Handle = *mut core::ffi::c_void;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn GetExitCodeProcess(process: Handle, code: *mut u32) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    pub fn pid_alive(pid: u32) -> bool {
        // SAFETY: the handle is checked for null, used only here, and closed.
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code) != 0;
            CloseHandle(h);
            ok && code == STILL_ACTIVE
        }
    }
}

#[cfg(unix)]
mod imp {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }

    pub fn pid_alive(pid: u32) -> bool {
        let Ok(pid) = i32::try_from(pid) else { return false };
        // SAFETY: signal 0 only checks that the process exists.
        let r = unsafe { kill(pid, 0) };
        r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(1)
    }
}

#[cfg(not(any(windows, unix)))]
mod imp {
    pub fn pid_alive(_pid: u32) -> bool {
        false
    }
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

    #[test]
    fn pid_alive_tells_running_from_exited() {
        assert!(pid_alive(std::process::id()));
        let mut c = std::process::Command::new("git").arg("--version").spawn().unwrap();
        let pid = c.id();
        c.wait().unwrap();
        assert!(!pid_alive(pid));
    }
}
