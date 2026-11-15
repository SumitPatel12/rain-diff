//! End to end against the real binary and difit's Node CLI. Needs `cargo build -p rain` and
//! `cargo xtask fixtures --tier s` first; run with `cargo test -p rain-cli -- --ignored`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Instant;

use rain_cli::background::parse_handshake;

/// difit's Node CLI (`dist/cli/index.js`) from `DIFIT_CLI`; the cross-check is skipped without it.
fn difit() -> Option<PathBuf> {
    std::env::var_os("DIFIT_CLI").map(PathBuf::from).filter(|p| p.exists())
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `Rain.exe` next to this test's `deps` directory.
fn exe() -> PathBuf {
    let me = std::env::current_exe().unwrap();
    me.parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(format!("Rain{}", std::env::consts::EXE_SUFFIX))
}

fn run(cmd: &mut Command) -> Output {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "{cmd:?} failed: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).trim_end().to_owned()
}

struct Kill(u32);

impl Drop for Kill {
    fn drop(&mut self) {
        let _ = Command::new("pwsh")
            .args(["-NoProfile", "-Command", &format!("Stop-Process -Id {} -Force", self.0)])
            .status();
    }
}

fn json(o: &Output) -> serde_json::Value {
    serde_json::from_str(&text(o)).unwrap_or_else(|e| panic!("{e}: {}", text(o)))
}

#[test]
#[ignore = "launches the app window"]
fn agent_drives_a_running_window() {
    let repo = root().join("fixtures/gen/s/repo");
    if !repo.exists() || !exe().exists() {
        eprintln!("skipped: run `cargo xtask fixtures --tier s` and `cargo build -p rain` first");
        return;
    }
    let started = Instant::now();
    let out =
        run(Command::new(exe())
            .current_dir(&repo)
            .args(["HEAD~1", "HEAD", "--background", "--session", "agent-e2e-test", "--clean"]));
    let (port, _, pid) = parse_handshake(&text(&out)).unwrap();
    let kill = Kill(pid);
    eprintln!("handshake {} ms", started.elapsed().as_millis());
    let p = port.to_string();
    let changed = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["diff", "--name-only", "HEAD~1", "HEAD"])
        .output()
        .unwrap();
    let file = String::from_utf8_lossy(&changed.stdout).lines().next().unwrap().to_owned();
    let rain = |args: &[&str]| {
        let t = Instant::now();
        let o = run(Command::new(exe()).args(args).args(["--port", &p]));
        eprintln!("{:>5} ms  rain {}", t.elapsed().as_millis(), args[..args.len().min(2)].join(" "));
        o
    };

    let story = json(&rain(&[
        "note",
        "add",
        "--story",
        "--title",
        "E2E",
        "--body",
        "The **whole** change.",
    ]));
    assert_eq!(story["changed"], true);
    let file_note = json(&rain(&["note", "add", "--file", &file, "--body", "Why this file changed."]));
    let file_note_id = file_note["ids"][0].as_str().unwrap().to_owned();
    let batch = format!("@{}", root().join("fixtures/notes/notes.json").display());
    assert_eq!(json(&rain(&["note", "add", "--json", &batch]))["count"], 5);

    let import = std::fs::read_to_string(root().join("fixtures/difit/import.json")).unwrap();
    rain(&["comment", "add", &import]);
    let threads = json(&rain(&["comment", "get", "--format", "json"]));
    let first = threads["threads"][0]["id"].as_str().unwrap().to_owned();
    let replied = json(&rain(&["comment", "reply", "--thread", &first, "--body", "On it."]));
    assert_eq!(replied["changed"], true);
    let resolved = json(&rain(&[
        "comment",
        "resolve",
        "--file",
        "src/legacy.cs",
        "--line",
        "7",
        "--side",
        "old",
    ]));
    assert_eq!(resolved["resolved"], serde_json::json!(["t-old"]));
    let goto = json(&rain(&["goto", &format!("{file}:1")]));
    assert_eq!(goto["success"], true);
    rain(&["goto", "--thread", &first]);

    let notes = json(&rain(&["note", "list", "--format", "json"]));
    assert_eq!(notes["notes"].as_array().unwrap().len(), 7);
    let comments = text(&rain(&["comment", "get", "--format", "json"]));
    assert!(!comments.contains("Why this file changed") && !comments.contains("The **whole** change"));
    assert!(comments.contains("On it."));
    let health = json(&rain(&["health"]));
    assert_eq!(
        (health["threads"].clone(), health["notes"].clone()),
        (serde_json::json!(3), serde_json::json!(7)),
        "a resolved thread stays in the session (state resolved)"
    );
    let removed = json(&rain(&["note", "remove", &file_note_id]));
    assert_eq!(removed["removed"], serde_json::json!([file_note_id]));
    eprintln!("total {} ms", started.elapsed().as_millis());

    drop(kill);
    assert!(port_frees(port), "port {port} is free within 5 s of the stop");
}

/// One `rain` call in a private data dir: `(exit code, stdout)`.
fn rain_in(data: &Path, args: &[&str]) -> (i32, String) {
    let t = Instant::now();
    let out = Command::new(exe()).env("LOCALAPPDATA", data).args(args).output().unwrap();
    eprintln!(
        "{:>5} ms  rain {} -> {:?}{}",
        t.elapsed().as_millis(),
        args[..args.len().min(3)].join(" "),
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).trim_end()
    );
    (out.status.code().unwrap_or(-1), text(&out))
}

fn ok_json(data: &Path, args: &[&str]) -> serde_json::Value {
    let (code, out) = rain_in(data, args);
    assert_eq!(code, 0, "rain {args:?}: {out}");
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"))
}

fn until(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while t.elapsed().as_secs() < secs {
        if f() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("{what} within {secs} s");
}

#[test]
#[ignore = "launches the app window"]
fn round3_session_loop() {
    if !exe().exists() {
        eprintln!("skipped: run `cargo build -p rain` first");
        return;
    }
    let data = std::env::temp_dir().join(format!("rain-e2e-r3-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&data);
    std::fs::create_dir_all(&data).unwrap();
    let patch = root().join("fixtures/m.patch");
    let patch = patch.to_str().unwrap();
    let s = ["--session", "e2e-r3"];
    let lock = data.join("Rain/sessions/e2e-r3.lock");

    let (code, line) = rain_in(&data, &["--fixture", patch, "--session", "e2e-r3", "--background", "--clean"]);
    assert_eq!(code, 0, "{line}");
    let (port, _, pid) = parse_handshake(&line).unwrap_or_else(|| panic!("no handshake in {line:?}"));
    let kill = Kill(pid);
    assert!(lock.exists(), "the window writes {}", lock.display());

    let health = ok_json(&data, &["health", "--session", "e2e-r3"]);
    assert_eq!(
        (health["pid"].as_u64(), health["session"].as_str()),
        (Some(u64::from(pid)), Some("e2e-r3"))
    );
    assert!(health.get("targetSha").is_some() && health.get("agent").is_some() && health.get("asks").is_some());

    let (code, again) = rain_in(&data, &["--fixture", patch, "--session", "e2e-r3", "--background"]);
    assert_eq!(code, 0, "{again}");
    let again: serde_json::Value = serde_json::from_str(&again).unwrap();
    assert_eq!(
        (again["reused"].as_bool(), again["port"].as_u64()),
        (Some(true), Some(u64::from(port)))
    );

    let import = data.join("import.json");
    std::fs::write(
        &import,
        r#"{"type":"thread","filePath":"@pr","position":{"side":"new","line":1},"body":"Does this need a migration?","kind":"question","author":"claude"}"#,
    )
    .unwrap();
    let at = format!("@{}", import.display());
    let added = ok_json(&data, &[&["comment", "add", &at][..], &s].concat());
    assert_eq!((added["success"].as_bool(), added["count"].as_u64()), (Some(true), Some(1)));
    let pr = ok_json(
        &data,
        &[
            &["comment", "add", "--pr-level", "--body", "Ship after QA.", "--kind", "praise"][..],
            &s,
        ]
        .concat(),
    );
    assert_eq!(pr["success"], true);
    let threads = ok_json(&data, &[&["comment", "get", "--format", "json"][..], &s].concat());
    let threads = threads["threads"].as_array().unwrap().clone();
    assert_eq!(threads.len(), 2);
    let id = threads[0]["id"].as_str().unwrap().to_owned();
    assert_eq!(threads[0]["kind"], "question");

    let set = ok_json(
        &data,
        &[&["comment", "set-state", &id, "addressed", "--sha", "abc1234"][..], &s].concat(),
    );
    assert_eq!(set["success"], true);
    let open = ok_json(&data, &[&["comment", "get", "--format", "json"][..], &s].concat());
    assert!(!open.to_string().contains(&id), "an addressed thread leaves the default readback");
    let all = ok_json(&data, &[&["comment", "get", "--format", "json", "--all"][..], &s].concat());
    let t = all["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == id.as_str())
        .unwrap()
        .clone();
    assert_eq!((t["state"].as_str(), t["stateSha"].as_str()), (Some("addressed"), Some("abc1234")));

    let (code, idle) = rain_in(
        &data,
        &[&["watch", "--kinds", "reply", "--for", "agent", "--timeout", "2"][..], &s].concat(),
    );
    assert_eq!(code, 2, "no reply yet: {idle}");
    let seq = serde_json::from_str::<serde_json::Value>(&idle).unwrap()["seq"]
        .as_u64()
        .unwrap()
        .to_string();
    let other = threads[1]["id"].as_str().unwrap();
    ok_json(
        &data,
        &[
            &["comment", "reply", "--thread", other, "--body", "Mine.", "--author", "claude"][..],
            &s,
        ]
        .concat(),
    );
    let (code, own) = rain_in(
        &data,
        &[
            &["watch", "--kinds", "reply", "--for", "agent", "--since", &seq, "--timeout", "2"][..],
            &s,
        ]
        .concat(),
    );
    assert_eq!(code, 2, "the agent's own reply does not wake it: {own}");
    ok_json(
        &data,
        &[
            &["comment", "reply", "--thread", other, "--body", "Agreed.", "--author", "you"][..],
            &s,
        ]
        .concat(),
    );
    let (code, woke) = rain_in(
        &data,
        &[
            &["watch", "--kinds", "reply", "--for", "agent", "--since", &seq, "--timeout", "5"][..],
            &s,
        ]
        .concat(),
    );
    assert_eq!(code, 0, "{woke}");
    let woke: serde_json::Value = serde_json::from_str(&woke).unwrap();
    assert_eq!(
        (woke["events"][0]["kind"].as_str(), woke["events"][0]["author"].as_str()),
        (Some("reply"), Some("you"))
    );
    let presence = ok_json(&data, &[&["presence", "working", "--on", other][..], &s].concat());
    assert_eq!(presence["success"], true);
    assert_eq!(ok_json(&data, &[&["health"][..], &s].concat())["agent"]["state"], "working");

    until("the diff loads", 30, || {
        ok_json(&data, &[&["health"][..], &s].concat())["diffLoaded"] == true
    });
    let brief = format!("@{}", root().join("fixtures/brief/sample.json").display());
    let set = ok_json(&data, &[&["brief", "set", &brief][..], &s].concat());
    assert_eq!(set["success"], true);
    let (code, check) = rain_in(&data, &[&["brief", "check"][..], &s].concat());
    assert_eq!(code, 1, "the sample has issues against m.patch: {check}");
    assert!(check.contains("phantom path: server/src/Services/VoucherService.cs"), "{check}");
    assert!(
        check.contains("not cited: server/src/Acme.Services/Reader2/Reader2Service.cs"),
        "{check}"
    );

    assert_eq!(ok_json(&data, &[&["review", "get"][..], &s].concat())["round"], 1);
    let state = ok_json(&data, &[&["state"][..], &s].concat());
    assert!(state.get("viewed").is_some() && state.get("round").is_some());
    let lit = ok_json(
        &data,
        &[
            &[
                "highlight",
                "server/src/Acme.Api/Controllers/Hold0Controller.cs:1-3",
                "--step",
                "1/3",
            ][..],
            &s,
        ]
        .concat(),
    );
    assert_eq!(lit["success"], true);
    assert_eq!(ok_json(&data, &[&["reload"][..], &s].concat())["success"], true);

    let closed = ok_json(&data, &[&["close"][..], &s].concat());
    assert_eq!(closed["pid"].as_u64(), Some(u64::from(pid)));
    until("the lock goes with the window", 10, || !lock.exists());
    std::mem::forget(kill);
    assert!(port_frees(port));

    let older = data.join("Rain/sessions/older.json");
    std::fs::write(&older, "{}").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&older)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
    let (code, line) = rain_in(&data, &["open", "--last", "--background"]);
    assert_eq!(code, 0, "{line}");
    let (_, _, pid) = parse_handshake(&line).unwrap();
    let kill = Kill(pid);
    assert_eq!(ok_json(&data, &[&["health"][..], &s].concat())["session"], "e2e-r3");
    assert_eq!(ok_json(&data, &[&["close"][..], &s].concat())["success"], true);
    std::mem::forget(kill);
    let _ = std::fs::remove_dir_all(&data);
}

/// Nothing listens on `port` any more. A killed window's teardown took 2–12 s on a loaded machine.
fn port_frees(port: u16) -> bool {
    let t = Instant::now();
    let free = (0..300).any(|_| {
        std::thread::sleep(std::time::Duration::from_millis(100));
        std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), std::time::Duration::from_millis(200)).is_err()
    });
    eprintln!("port {port} free after {} ms", t.elapsed().as_millis());
    free
}

#[test]
#[ignore = "launches the app window and difit's Node CLI"]
fn background_launch_and_difit_client_compat() {
    let repo = root().join("fixtures/gen/s/repo");
    if !repo.exists() || !exe().exists() {
        eprintln!("skipped: run `cargo xtask fixtures --tier s` and `cargo build -p rain` first");
        return;
    }
    let t = Instant::now();
    let out = run(Command::new(exe())
        .current_dir(&repo)
        .args(["HEAD~1", "HEAD", "--background", "--session", "cli-e2e-test", "--clean"]));
    let line = text(&out);
    let (port, url, pid) = parse_handshake(&line).unwrap_or_else(|| panic!("no handshake in {line:?}"));
    let kill = Kill(pid);
    eprintln!("handshake in {} ms: {line}", t.elapsed().as_millis());
    assert_eq!(line.lines().count(), 1);
    assert_eq!(url, format!("http://localhost:{port}"));
    let p = port.to_string();

    let import = std::fs::read_to_string(root().join("fixtures/difit/import.json")).unwrap();
    let added = text(&run(Command::new(exe()).args(["comment", "add", &import, "--port", &p])));
    assert!(added.starts_with(r#"{"success":true,"importId":"#), "{added}");

    let mine = text(&run(Command::new(exe()).args(["comment", "get", "--port", &p, "--format", "json"])));
    let golden = std::fs::read_to_string(root().join("fixtures/difit/comments-json.golden.json")).unwrap();
    assert_eq!(mine, golden.trim_end());
    if let Some(difit) = difit() {
        let node = text(&run(Command::new("node")
            .arg(difit)
            .args(["comment", "get", "--port", &p, "--format", "json"])));
        // The window may mark threads outdated between the two reads (none of their files are in this diff).
        let now = text(&run(Command::new(exe()).args(["comment", "get", "--port", &p, "--format", "json"])));
        assert_eq!(node, now, "difit's Node client reads the same bytes");
    }

    let overview = root().join("fixtures/overview/sample-overview-schema.json");
    let set = text(&run(Command::new(exe())
        .arg("overview")
        .arg("set")
        .arg(&overview)
        .args(["--port", &p, "--self", "server"])));
    let set: serde_json::Value = serde_json::from_str(&set).unwrap();
    assert_eq!(set["version"], 1);
    assert_eq!(set["selfRepo"], "server");

    drop(kill);
    assert!(port_frees(port), "port {port} is free within 5 s of the stop");
    let down = Command::new(exe()).args(["comment", "get", "--port", &p]).output().unwrap();
    assert!(!down.status.success());
    assert!(String::from_utf8_lossy(&down.stderr).contains(&format!("Cannot connect to Rain server on port {port}")));
}
