//! Why a window's process ended, one timestamped line each on stderr: the background child's log
//! (`%LOCALAPPDATA%\Rain\logs\<session>-<pid>.log`) after the handshake, the terminal otherwise.
//! A log that stops without an `exit` or `panic` line means the process was killed outright.

use std::backtrace::{Backtrace, BacktraceStatus};
use std::io::Write;
use std::panic::PanicHookInfo;
use std::time::{SystemTime, UNIX_EPOCH};

/// `[2026-11-07T10:15:02Z pid 1234] <reason>`.
pub fn line(reason: &str) -> String {
    format!("[{} pid {}] {reason}", utc_now(), std::process::id())
}

/// Writes `text` to stderr, ignoring failures (`eprintln!` would panic, inside a panic hook too).
pub fn write(text: &str) {
    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "{text}");
    let _ = err.flush();
}

pub fn record(reason: &str) {
    write(&line(reason));
}

/// The panic's thread, location and message, then the backtrace when one could be captured.
pub fn panic_report(info: &PanicHookInfo, backtrace: &Backtrace) -> String {
    let thread = std::thread::current();
    let at = info.location().map_or_else(
        || "an unknown location".into(),
        |l| format!("{}:{}:{}", l.file(), l.line(), l.column()),
    );
    let msg = info.payload_as_str().unwrap_or("(no message)");
    let mut s = line(&format!("panic in thread '{}' at {at}: {msg}", thread.name().unwrap_or("unnamed")));
    if backtrace.status() == BacktraceStatus::Captured {
        s.push_str(&format!("\nbacktrace:\n{backtrace}"));
    }
    s
}

/// Hands every panic's report to `sink`, then runs the hook that was installed before.
pub fn install_panic_hook(sink: fn(&str)) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        sink(&panic_report(info, &Backtrace::force_capture()));
        previous(info);
    }));
}

/// Logs Ctrl-C, Ctrl-Break and the console closing; Windows then ends the process as before.
#[cfg(windows)]
pub fn log_console_events() {
    unsafe extern "system" {
        fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> i32>, add: i32) -> i32;
    }
    unsafe extern "system" fn handler(event: u32) -> i32 {
        record(&format!("exit: console control event {}", console_event_name(event)));
        0
    }
    // SAFETY: registers a handler that lives for the whole process.
    unsafe {
        SetConsoleCtrlHandler(Some(handler), 1);
    }
}

#[cfg(not(windows))]
pub fn log_console_events() {}

pub fn console_event_name(event: u32) -> String {
    match event {
        0 => "CTRL_C_EVENT".into(),
        1 => "CTRL_BREAK_EVENT".into(),
        2 => "CTRL_CLOSE_EVENT".into(),
        5 => "CTRL_LOGOFF_EVENT".into(),
        6 => "CTRL_SHUTDOWN_EVENT".into(),
        n => format!("{n}"),
    }
}

fn utc_now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (y, m, d) = civil_from_days(secs / 86_400);
    let t = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", t / 3600, t % 3600 / 60, t % 60)
}

/// Days since 1970-01-01 to (year, month, day): Howard Hinnant's `civil_from_days`.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (era * 400 + yoe + u64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_are_civil() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_764), (2026, 11, 7));
    }

    #[test]
    fn lines_carry_time_and_pid() {
        let l = line("exit 0");
        assert!(
            l.starts_with('[') && l.ends_with(&format!("Z pid {}] exit 0", std::process::id())),
            "{l}"
        );
        assert_eq!(console_event_name(2), "CTRL_CLOSE_EVENT");
    }
}
