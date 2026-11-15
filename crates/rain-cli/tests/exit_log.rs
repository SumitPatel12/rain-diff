//! The panic hook is process-wide, so it gets a test binary of its own.

use std::sync::Mutex;

use rain_cli::exit_log;

static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[test]
fn the_panic_hook_writes_message_location_and_backtrace() {
    exit_log::install_panic_hook(|s| SEEN.lock().unwrap().push(s.to_owned()));
    let caught = std::panic::catch_unwind(|| panic!("boom {}", 7));
    assert!(caught.is_err());
    let seen = SEEN.lock().unwrap();
    assert_eq!(seen.len(), 1);
    let report = &seen[0];
    let first = report.lines().next().unwrap();
    assert!(first.contains(&format!("pid {}] panic in thread '", std::process::id())), "{first}");
    assert!(
        first.contains("tests") && first.contains("exit_log.rs:") && first.ends_with(": boom 7"),
        "{first}"
    );
    assert!(report.contains("\nbacktrace:\n"), "{report}");
}
