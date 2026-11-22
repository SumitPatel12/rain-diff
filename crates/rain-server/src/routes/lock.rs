//! `sessions/<id>.lock`: which port and process serve a session, so a second `rain open` or a CLI
//! call with `--session` finds the running window instead of starting another.

use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
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
    /// The window's token (see `ServerHandle::token`); never served by `GET /api/lock`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
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

/// The token of the window serving `port`, from the newest lock in the sessions folder `dir`
/// that names that port.
pub fn token_for_port(dir: &Path, port: u16) -> Option<String> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "lock"))
        .filter_map(|e| read_lock(&e.path()))
        .filter(|l| l.port == port && l.token.is_some())
        .max_by(|a, b| a.started_at.cmp(&b.started_at))?
        .token
}

/// Takes the lock for this window, or returns the live window that holds it. The file appears
/// whole and only if absent (a hard link of a finished temp file), so two windows starting at
/// once can't both take it. An I/O error never grants ownership.
pub(crate) fn acquire(path: &Path, port: u16, token: &str) -> io::Result<Option<Lock>> {
    acquire_as(
        path,
        &Lock {
            port,
            pid: std::process::id(),
            started_at: rain_comments::clock::now_iso(),
            token: Some(token.to_owned()),
        },
    )
}

fn acquire_as(path: &Path, lock: &Lock) -> io::Result<Option<Lock>> {
    let text = serde_json::to_vec(lock).map_err(io::Error::other)?;
    for attempt in 0..5 {
        match settled(|| create_new(path, &text)) {
            Ok(()) => return Ok(None),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                // Gone between the two calls: another window just let go, or took it over.
                let seen = match settled(|| std::fs::read(path)) {
                    Ok(seen) => seen,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e),
                };
                match serde_json::from_slice::<Lock>(&seen).ok() {
                    // This window holds that port now, so whoever wrote the lock is gone.
                    Some(l) if l.port != lock.port && !l.is_stale() => return Ok(Some(l)),
                    _ => take_stale(path, &seen),
                }
            }
            Err(e) => return Err(e),
        }
        std::thread::sleep(Duration::from_millis(10 * attempt));
    }
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        format!("could not acquire {}", path.display()),
    ))
}

/// `op`, tried again while Windows briefly denies it: a just-written file (antivirus, indexer) or
/// a lock another window is replacing.
fn settled<T>(mut op: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut tries = 0;
    loop {
        match op() {
            Err(e) if tries < 20 && matches!(e.raw_os_error(), Some(5 | 32)) => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(10 * tries));
            }
            done => return done,
        }
    }
}

/// Removes the lock at `path` only while it still holds `seen`, the lock judged stale, and only
/// while holding `<lock>.takeover`: of two windows that judged it stale, the second finds it
/// gone or replaced by the first one's fresh lock, which it leaves alone.
fn take_stale(path: &Path, seen: &[u8]) {
    let mut takeover = path.as_os_str().to_owned();
    takeover.push(".takeover");
    let takeover = PathBuf::from(takeover);
    if create_new(&takeover, b"").is_err() {
        // Left by a window that crashed while taking over; the caller tries again.
        let abandoned = std::fs::metadata(&takeover)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(10));
        if abandoned {
            let _ = std::fs::remove_file(&takeover);
        }
        return;
    }
    if std::fs::read(path).ok().as_deref() == Some(seen) {
        let _ = std::fs::remove_file(path);
    }
    let _ = std::fs::remove_file(&takeover);
}

static TMP: AtomicU64 = AtomicU64::new(0);

/// Creates `path` holding `bytes` only if it is absent; `AlreadyExists` means `path` itself exists.
fn create_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        // A file where the folder should be is not the lock existing.
        std::fs::create_dir_all(dir).map_err(|e| match e.kind() {
            io::ErrorKind::AlreadyExists => io::Error::other(format!("{} is not a folder: {e}", dir.display())),
            _ => e,
        })?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    // The token must be private from creation, including while the temp file is written.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let (tmp, mut file) = loop {
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(format!(".{}.{}.tmp", std::process::id(), TMP.fetch_add(1, Ordering::Relaxed)));
        let tmp = PathBuf::from(tmp);
        match options.open(&tmp) {
            Ok(file) => break (tmp, file),
            // Left by an earlier process with this pid; the next name is free.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    };
    let written = file.write_all(bytes);
    drop(file);
    let linked = written.and_then(|()| std::fs::hard_link(&tmp, path));
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
    let text = String::from_utf8_lossy(&buf);
    // Too busy to answer is not gone.
    if text.split(' ').nth(1) == Some("503") {
        return Health::Silent;
    }
    let pid = text
        .split_once("\r\n\r\n")
        .and_then(|(_, body)| serde_json::from_str::<serde_json::Value>(body).ok())
        .filter(|v| v["app"] == "rain")
        .and_then(|v| v["pid"].as_u64())
        .and_then(|p| u32::try_from(p).ok());
    pid.map_or(Health::Other, Health::Pid)
}

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
        Some(path) => {
            let lock = read_lock(path).map(|l| Lock { token: None, ..l });
            value(200, &json!({ "path": path, "lock": lock }))
        }
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
            token: None,
        };
        assert!(lock.is_stale());
    }

    /// A listener that answers every request with `reply` until the test process ends.
    fn answering(reply: String) -> u16 {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut s in l.incoming().flatten() {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                let _ = s.write_all(reply.as_bytes());
            }
        });
        port
    }

    #[test]
    fn a_window_too_busy_to_answer_is_not_stale() {
        let port = answering("HTTP/1.1 503 Service Unavailable\r\n\r\n{\"error\":\"Too many requests at once\"}".into());
        let lock = Lock {
            port,
            pid: std::process::id(),
            started_at: String::new(),
            token: None,
        };
        assert!(!lock.is_stale());
    }

    #[test]
    fn windows_racing_for_a_stale_lock_leave_exactly_one_owner() {
        let dir = std::env::temp_dir().join(format!("rain-lock-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        // Fake windows: each port answers health as Rain with its own (made-up) pid.
        let contenders: Vec<Lock> = (0..6u32)
            .map(|i| {
                let pid = 4_000_000 + i;
                Lock {
                    port: answering(format!("HTTP/1.0 200 OK\r\n\r\n{{\"app\":\"rain\",\"pid\":{pid}}}")),
                    pid,
                    started_at: format!("t{i}"),
                    token: None,
                }
            })
            .collect();
        for round in 0..4 {
            let path = dir.join(format!("S{round}.lock"));
            let dead = format!(r#"{{"port":{closed},"pid":4999999,"startedAt":"t"}}"#);
            std::fs::write(&path, dead).unwrap();
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(contenders.len()));
            let handles: Vec<_> = contenders
                .iter()
                .cloned()
                .map(|l| {
                    let (path, barrier) = (path.clone(), barrier.clone());
                    std::thread::spawn(move || {
                        barrier.wait();
                        acquire_as(&path, &l).unwrap().is_none()
                    })
                })
                .collect();
            let owners = handles.into_iter().map(|h| h.join().unwrap()).filter(|&ok| ok).count();
            assert_eq!(owners, 1, "round {round}");
            let held = read_lock(&path).unwrap();
            assert!(contenders.contains(&held), "the lock names the owner");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rain-lock-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_lock_naming_the_port_this_window_holds_is_stale() {
        let dir = temp("own-port");
        let path = dir.join("S.lock");
        // The crashed window's port, bound again by this one and not serving yet.
        let bound = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let left = Lock {
            port: bound.local_addr().unwrap().port(),
            pid: std::process::id(),
            started_at: "then".into(),
            token: None,
        };
        std::fs::write(&path, serde_json::to_vec(&left).unwrap()).unwrap();
        let mine = Lock {
            started_at: "now".into(),
            ..left.clone()
        };
        assert_eq!(acquire_as(&path, &mine).unwrap(), None, "a window never locks itself out");
        assert_eq!(read_lock(&path), Some(mine));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_file_where_the_folder_should_be_is_not_taken_for_the_lock() {
        let dir = temp("no-folder");
        let blocked = dir.join("not-a-folder");
        std::fs::write(&blocked, "keep").unwrap();
        let lock = Lock {
            port: 1,
            pid: std::process::id(),
            started_at: String::new(),
            token: None,
        };
        let e = acquire_as(&blocked.join("S.lock"), &lock).unwrap_err();
        assert!(
            !matches!(e.kind(), io::ErrorKind::AlreadyExists | io::ErrorKind::WouldBlock),
            "{e:?}"
        );
        assert_eq!(std::fs::read_to_string(&blocked).unwrap(), "keep");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_leftover_temp_file_does_not_stand_in_for_the_lock() {
        let dir = temp("leftover");
        let path = dir.join("S.lock");
        let next = TMP.load(Ordering::Relaxed);
        for n in next..next + 64 {
            let mut tmp = path.as_os_str().to_owned();
            tmp.push(format!(".{}.{n}.tmp", std::process::id()));
            std::fs::write(tmp, "left").unwrap();
        }
        create_new(&path, b"lock").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"lock");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_lock_file_another_process_holds_open_for_a_moment_is_waited_for() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = temp("sharing");
        let path = dir.join("S.lock");
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        std::fs::write(&path, format!(r#"{{"port":{closed},"pid":4999999,"startedAt":"t"}}"#)).unwrap();
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            drop(held);
        });
        let mine = Lock {
            port: 1,
            pid: std::process::id(),
            started_at: "now".into(),
            token: None,
        };
        assert_eq!(acquire_as(&path, &mine).unwrap(), None);
        assert_eq!(read_lock(&path), Some(mine));
        release.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_stale_lock_replaced_meanwhile_is_left_alone() {
        let dir = std::env::temp_dir().join(format!("rain-lock-takeover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("S.lock");
        std::fs::write(&path, "fresh").unwrap();
        take_stale(&path, b"stale");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "fresh");
        take_stale(&path, b"fresh");
        assert!(!path.exists());
        let leftovers = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(leftovers, 0, "no takeover or temp file is left behind");
        let _ = std::fs::remove_dir_all(dir);
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
