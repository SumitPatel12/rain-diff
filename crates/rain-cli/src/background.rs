//! `--background`: start a detached copy of this process and relay its one-line handshake, as
//! difit's `cli/background.ts` does over IPC.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const CHILD_ENV: &str = "RAIN_BACKGROUND_CHILD";
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
pub const READ_ONLY_WARNING: &str = "Warning: another window holds this session; this one is read-only and refuses changes (423). Use --session to reach the window that holds it.";

pub fn is_child() -> bool {
    std::env::var_os(CHILD_ENV).is_some_and(|v| v == "1")
}

/// `{"port":N,"url":"…","pid":N}`, the line difit prints and its launch scripts parse.
pub fn handshake_line(port: u16, url: &str, pid: u32) -> String {
    let url = serde_json::Value::from(url);
    format!(r#"{{"port":{port},"url":{url},"pid":{pid}}}"#)
}

/// The handshake of a window that already served the session: `{…,"reused":true}`.
pub fn reused_line(port: u16, pid: u32) -> String {
    let line = handshake_line(port, &format!("http://localhost:{port}"), pid);
    format!("{},\"reused\":true}}", &line[..line.len() - 1])
}

/// `line` with Rain's keys after difit's: `"token"` (the window's, see `rain_server::TOKEN_HEADER`)
/// and `"readOnly":true` when another window holds the session.
pub fn with_extras(line: &str, token: Option<&str>, read_only: bool) -> String {
    let mut extra = String::new();
    if let Some(t) = token {
        extra.push_str(&format!(",\"token\":{}", serde_json::Value::from(t)));
    }
    if read_only {
        extra.push_str(",\"readOnly\":true");
    }
    format!("{}{extra}}}", &line[..line.len() - 1])
}

/// Whether the handshake `line` says the window is read-only.
pub fn is_read_only(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line.trim()).is_ok_and(|v| v["readOnly"] == true)
}

/// The parsed handshake, when `line` is one.
pub fn parse_handshake(line: &str) -> Option<(u16, String, u32)> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    Some((
        u16::try_from(v.get("port")?.as_u64()?).ok()?,
        v.get("url")?.as_str()?.to_owned(),
        u32::try_from(v.get("pid")?.as_u64()?).ok()?,
    ))
}

#[cfg(windows)]
fn spawn_detached(cmd: &mut Command) -> std::io::Result<Child> {
    use std::os::windows::process::CommandExt;
    // A hidden console of its own (not DETACHED_PROCESS) so every `git` the app starts inherits it
    // instead of flashing a new console window; the Rune window is unaffected.
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let base = CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW;
    cmd.creation_flags(base | CREATE_BREAKAWAY_FROM_JOB);
    cmd.spawn().or_else(|_| {
        cmd.creation_flags(base);
        cmd.spawn()
    })
}

#[cfg(not(windows))]
fn spawn_detached(cmd: &mut Command) -> std::io::Result<Child> {
    cmd.spawn()
}

/// Starts this executable again with `args` minus `--background`, waits up to 10 s for its
/// handshake on stdout, prints it and returns 0; prints the child's stderr and returns 1 when it
/// exits or times out first.
pub fn spawn(args: &[String], hint: impl FnOnce(u16) -> Option<String>) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Error: cannot find the Rain executable: {e}");
            return 1;
        }
    };
    let child_args: Vec<&String> = args.iter().filter(|a| *a != "--background").collect();
    match start(&exe, &child_args, HANDSHAKE_TIMEOUT) {
        Ok(line) => {
            let hint = parse_handshake(&line).and_then(|(port, ..)| hint(port));
            relay(&line, hint.as_deref(), &mut std::io::stdout(), &mut std::io::stderr());
            0
        }
        Err(msg) => {
            eprintln!("Error: {msg}");
            1
        }
    }
}

/// The handshake `line` alone on `out`; the read-only warning, then `hint`, on `err`.
pub fn relay(line: &str, hint: Option<&str>, out: &mut impl Write, err: &mut impl Write) {
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
    if is_read_only(line) {
        let _ = writeln!(err, "{READ_ONLY_WARNING}");
    }
    if let Some(h) = hint {
        let _ = writeln!(err, "{h}");
    }
}

/// The next step for an agent whose window opened with no brief.
pub fn brief_hint(session: &str) -> String {
    format!("next: send a brief (intent, how, tour) with: rain brief set <file.md> --session {session}; see docs/agent.md §4")
}

/// Runs `exe args` as a background child and returns its handshake line.
pub fn start(exe: &Path, args: &[&String], timeout: Duration) -> Result<String, String> {
    imp::keep_std_private();
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .env(CHILD_ENV, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = spawn_detached(&mut cmd).map_err(|e| format!("cannot start background Rain: {e}"))?;
    let (Some(stdout), Some(mut stderr)) = (child.stdout.take(), child.stderr.take()) else {
        return Err("no pipes to the background Rain".into());
    };
    let (tx, rx) = mpsc::channel::<Option<String>>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(l) if parse_handshake(&l).is_some() => {
                    let _ = tx.send(Some(l.trim().to_owned()));
                    return;
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let _ = tx.send(None);
    });
    let (etx, erx) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        let _ = etx.send(String::from_utf8_lossy(&buf).into_owned());
    });
    let started = Instant::now();
    match rx.recv_timeout(timeout) {
        Ok(Some(line)) => Ok(line),
        Ok(None) | Err(mpsc::RecvTimeoutError::Disconnected) => {
            let code = wait_briefly(&mut child);
            let err = erx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
            let err = err.trim();
            Err(if err.is_empty() {
                format!("Background Rain exited early (code {code})")
            } else {
                err.to_owned()
            })
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = child.kill();
            let _ = child.wait();
            let err = erx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
            Err(format!(
                "Timed out after {:.1} s while starting background Rain{}",
                started.elapsed().as_secs_f64(),
                if err.trim().is_empty() {
                    String::new()
                } else {
                    format!(": {}", err.trim())
                }
            ))
        }
    }
}

fn wait_briefly(child: &mut Child) -> String {
    let until = Instant::now() + Duration::from_secs(2);
    while Instant::now() < until {
        if let Ok(Some(status)) = child.try_wait() {
            return status.code().map_or_else(|| "unknown".into(), |c| c.to_string());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    "unknown".into()
}

/// In the background child after the handshake: the parent closes its pipes when it exits, so
/// stdout and stderr move to `log` and later writes cannot fail.
pub fn redirect_stdio(log: &Path) {
    use std::io::Write;
    let _ = std::io::stdout().flush();
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(log) else {
        return;
    };
    imp::set_std(file);
}

#[cfg(windows)]
mod imp {
    use std::os::windows::io::IntoRawHandle;

    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;

    const STD_INPUT_HANDLE: u32 = -10i32 as u32;
    const HANDLE_FLAG_INHERIT: u32 = 1;

    unsafe extern "system" {
        fn SetStdHandle(std_handle: u32, handle: *mut core::ffi::c_void) -> i32;
        fn GetStdHandle(std_handle: u32) -> *mut core::ffi::c_void;
        fn SetHandleInformation(handle: *mut core::ffi::c_void, mask: u32, flags: u32) -> i32;
    }

    /// Rust spawns with handle inheritance on, so without this the background child would inherit
    /// the caller's stdout pipe and a shell capturing `rain --background` would wait for the
    /// app to exit.
    pub fn keep_std_private() {
        for n in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            // SAFETY: plain Win32 calls on this process's own standard handles.
            unsafe {
                let h = GetStdHandle(n);
                if !h.is_null() && h as isize != -1 {
                    SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
                }
            }
        }
    }

    pub fn set_std(file: std::fs::File) {
        let Ok(out) = file.try_clone() else { return };
        // SAFETY: both handles are valid, owned file handles leaked for the life of the process.
        unsafe {
            SetStdHandle(STD_OUTPUT_HANDLE, out.into_raw_handle());
            SetStdHandle(STD_ERROR_HANDLE, file.into_raw_handle());
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn set_std(_file: std::fs::File) {}
    pub fn keep_std_private() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_round_trips_in_difit_key_order() {
        let line = handshake_line(4967, "http://localhost:4967", 1234);
        assert_eq!(line, r#"{"port":4967,"url":"http://localhost:4967","pid":1234}"#);
        assert_eq!(parse_handshake(&line), Some((4967, "http://localhost:4967".into(), 1234)));
        assert_eq!(parse_handshake("Listening..."), None);
        assert_eq!(parse_handshake(r#"{"port":1}"#), None);
        let reused = reused_line(4967, 1234);
        assert_eq!(reused, r#"{"port":4967,"url":"http://localhost:4967","pid":1234,"reused":true}"#);
        assert_eq!(parse_handshake(&reused), Some((4967, "http://localhost:4967".into(), 1234)));
    }

    #[test]
    fn rain_keys_follow_difits_and_say_read_only() {
        let line = with_extras(&handshake_line(4967, "http://localhost:4967", 1234), Some("ab12"), true);
        assert_eq!(
            line,
            r#"{"port":4967,"url":"http://localhost:4967","pid":1234,"token":"ab12","readOnly":true}"#
        );
        assert_eq!(parse_handshake(&line), Some((4967, "http://localhost:4967".into(), 1234)));
        assert!(is_read_only(&line));
        let normal = with_extras(&reused_line(4967, 1234), Some("ab12"), false);
        assert_eq!(
            normal,
            r#"{"port":4967,"url":"http://localhost:4967","pid":1234,"reused":true,"token":"ab12"}"#
        );
        assert!(!is_read_only(&normal));
    }
}
