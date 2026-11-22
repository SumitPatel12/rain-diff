//! The runner against small stand-in commands (never an agent).

use std::time::{Duration, Instant};

use rain_ask::runner::{self, Job, Request};

fn shell(script: &str) -> Vec<String> {
    if cfg!(windows) {
        vec!["cmd".into(), "/d".into(), "/c".into(), script.into()]
    } else {
        vec!["sh".into(), "-c".into(), script.into()]
    }
}

fn sleeper() -> Vec<String> {
    if cfg!(windows) {
        shell("ping -n 30 127.0.0.1")
    } else {
        shell("sleep 30")
    }
}

fn run(argv: Vec<String>, stdin: &str, timeout: Duration) -> rain_ask::Outcome {
    runner::spawn(Request {
        argv,
        stdin: stdin.into(),
        timeout,
        env: vec![("RAIN_THREAD".into(), "k3x9f2".into())],
        ..Request::default()
    })
    .unwrap()
    .wait()
}

#[test]
fn stdout_and_exit_code_come_back_and_stdin_is_the_prompt() {
    let echo = if cfg!(windows) { shell("more") } else { shell("cat") };
    let o = run(echo, "the prompt\n", Duration::from_secs(20));
    assert_eq!(o.code, Some(0));
    assert_eq!(o.stdout.trim(), "the prompt");
    assert!(o.answered());

    let env = if cfg!(windows) {
        shell("echo %RAIN_THREAD%")
    } else {
        shell("echo $RAIN_THREAD")
    };
    assert_eq!(run(env, "", Duration::from_secs(20)).stdout.trim(), "k3x9f2");

    // `&` sequences commands in cmd, but backgrounds the echo in a POSIX shell.
    let error = if cfg!(windows) {
        "echo bad 1>&2 & exit 4"
    } else {
        "echo bad 1>&2; exit 4"
    };
    let o = run(shell(error), "", Duration::from_secs(20));
    assert_eq!(o.code, Some(4));
    assert!(o.error().starts_with("exit 4: bad"), "{}", o.error());
}

#[test]
fn stderr_lands_in_the_log_file() {
    let dir = std::env::temp_dir().join(format!("rain-ask-log-{}", std::process::id()));
    let log = dir.join("logs").join("ask-s-t.log");
    let h = runner::spawn(Request {
        argv: shell("echo oops 1>&2"),
        timeout: Duration::from_secs(20),
        log: Some(log.clone()),
        ..Request::default()
    })
    .unwrap();
    let _ = h.wait();
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("oops"), "{text}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_run_past_its_timeout_is_killed() {
    let t = Instant::now();
    let o = run(sleeper(), "", Duration::from_secs(1));
    assert!(o.timed_out);
    assert!(!o.answered());
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
}

#[test]
fn a_timeout_ends_the_children_the_command_started_too() {
    // The grandchild holds stdout: were it left running, reading the output would wait it out.
    let script = if cfg!(windows) {
        "start /b ping -n 30 127.0.0.1 & ping -n 30 127.0.0.1"
    } else {
        "sleep 30 & sleep 30"
    };
    let t = Instant::now();
    let o = run(shell(script), "", Duration::from_secs(1));
    assert!(o.timed_out);
    assert!(t.elapsed() < Duration::from_millis(2500), "{:?}", t.elapsed());
}

#[test]
fn cancel_ends_the_run_within_half_a_second() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut h = runner::spawn(Request {
        argv: sleeper(),
        timeout: Duration::from_secs(60),
        on_done: Some(std::sync::Arc::new(move || {
            let _ = tx.send(());
        })),
        ..Request::default()
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(h.try_wait().is_none(), "still running");
    let t = Instant::now();
    h.cancel();
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(t.elapsed() < Duration::from_millis(500), "{:?}", t.elapsed());
    let o = h.try_wait().unwrap();
    assert!(o.cancelled && !o.timed_out);
}

#[cfg(unix)]
#[test]
fn a_successful_command_does_not_leave_children_holding_its_output() {
    let o = run(shell("sleep 6 & printf answer"), "", Duration::from_secs(20));
    assert_eq!(o.code, Some(0));
    assert_eq!(o.stdout, "answer", "a descendant must not keep the output pipe open");
    assert!(o.answered());
}
