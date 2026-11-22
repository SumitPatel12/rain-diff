//! "Nothing leaves the machine", checked: the resolved dependencies hold no crate that calls out,
//! every crate that could open a socket is listed here with why it is there, first-party code
//! touches sockets only in the listed files, and the server is reachable on loopback alone.

use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use rain_core::Shared;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Package name to the names it depends on, from `Cargo.lock` (every platform and feature the
/// workspace resolves, dev-dependencies included).
fn lock() -> BTreeMap<String, Vec<String>> {
    let text = std::fs::read_to_string(root().join("Cargo.lock")).unwrap();
    let mut packages: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut name = String::new();
    let mut in_deps = false;
    for l in text.lines() {
        if let Some(n) = l.strip_prefix("name = \"") {
            name = n.trim_end_matches('"').to_owned();
            packages.entry(name.clone()).or_default();
        } else if l == "dependencies = [" {
            in_deps = true;
        } else if l == "]" {
            in_deps = false;
        } else if in_deps && let Some(dep) = l.trim().strip_prefix('"') {
            let dep = dep.split([' ', '"']).next().unwrap_or_default().to_owned();
            packages.entry(name.clone()).or_default().push(dep);
        }
    }
    assert!(
        packages.len() > 50 && packages.contains_key("rain-server"),
        "Cargo.lock did not parse"
    );
    packages
}

/// HTTP and other network clients, TLS stacks, async network runtimes, resolvers, telemetry and
/// updaters. None may be resolved at all; a name ending in `*` is a prefix.
const DENIED: &[&str] = &[
    "reqwest*",
    "hyper*",
    "curl*",
    "isahc",
    "surf",
    "attohttpc",
    "minreq",
    "ehttp",
    "http-client",
    "async-h1",
    "native-tls",
    "rustls*",
    "openssl*",
    "schannel",
    "security-framework*",
    "webpki*",
    "tokio-rustls",
    "tokio-native-tls",
    "tokio-tungstenite",
    "tungstenite",
    "websocket*",
    "h2",
    "h3",
    "quinn*",
    "quiche",
    "s2n-quic*",
    "socket2",
    "async-std",
    "trust-dns*",
    "hickory*",
    "dns-lookup",
    "sentry*",
    "opentelemetry*",
    "self_update",
    "lettre",
    "tonic*",
    "git2",
    "libgit2-sys",
    "ssh2",
    "libssh2-sys",
];

/// A resolved package whose name holds one of these needs an [`ALLOWED`] entry.
const SUSPECT: &[&str] = &[
    "http",
    "tls",
    "ssl",
    "socket",
    "curl",
    "dns",
    "smtp",
    "grpc",
    "telemetry",
    "sentry",
    "analytics",
    "tcp",
    "udp",
    "uds",
    "url",
    "hyper",
    "oauth",
    "cookie",
    "webpki",
    "ureq",
    "reqwest",
    "tokio",
    "async-io",
];

/// Whole names that need an [`ALLOWED`] entry: event loops and buses that can hold a socket.
const SUSPECT_NAMES: &[&str] = &["mio", "polling", "zbus", "smol"];

/// Every suspect package that is resolved: who may depend on it, and why it is no way out.
const ALLOWED: &[(&str, &[&str], &str)] = &[
    (
        "tiny_http",
        &["rain-server"],
        "the comment server; it listens on 127.0.0.1 and ::1 only",
    ),
    ("httpdate", &["tiny_http"], "tiny_http's Date header"),
    (
        "ureq",
        &["rain-cli", "rain-server"],
        "the CLI's client of a running window on localhost, without TLS and with proxies off; a dev-dependency of rain-server's tests",
    ),
    ("ureq-proto", &["ureq"], "ureq's HTTP/1.1 state machine"),
    ("http", &["ureq-proto"], "request and response types, no I/O"),
    ("httparse", &["ureq-proto"], "a header parser, no I/O"),
    ("mio", &["notify"], "the file watcher's poller on Linux and BSD"),
    (
        "tokio",
        &["wasm-bindgen-futures"],
        "resolved for wasm only, and checked below to have no `net` feature",
    ),
    (
        "polling",
        &["async-io", "calloop"],
        "the Linux window and accessibility event loops",
    ),
    ("async-io", &["async-process", "async-signal", "zbus"], "zbus's reactor on Linux"),
    (
        "zbus",
        &["accesskit_unix", "atspi-common", "atspi-proxies"],
        "Linux accessibility over the session D-Bus, a local socket",
    ),
    ("uds_windows", &["zbus"], "unix-domain sockets, local by nature"),
    ("data-url", &["usvg"], "parses `data:` URLs inside an SVG; opens nothing"),
    ("scoped-tls", &["wayland-backend"], "thread-local storage, not TLS"),
];

fn denied(name: &str) -> bool {
    DENIED.iter().any(|d| match d.strip_suffix('*') {
        Some(prefix) => name.starts_with(prefix),
        None => name == *d,
    })
}

fn suspect(name: &str) -> bool {
    SUSPECT.iter().any(|s| name.contains(s)) || SUSPECT_NAMES.contains(&name)
}

#[test]
fn no_resolved_dependency_can_call_out() {
    let packages = lock();
    let hits: Vec<&String> = packages.keys().filter(|n| denied(n)).collect();
    assert!(hits.is_empty(), "networking crates in Cargo.lock: {hits:?}");

    for name in packages.keys().filter(|n| suspect(n)) {
        let Some((_, dependents, _)) = ALLOWED.iter().find(|(n, ..)| n == name) else {
            panic!("{name} looks like networking and has no ALLOWED entry saying why it is safe");
        };
        let actual: Vec<&str> = packages
            .iter()
            .filter(|(_, deps)| deps.contains(name))
            .map(|(n, _)| n.as_str())
            .collect();
        assert_eq!(&actual, dependents, "who depends on {name} changed; re-check it and update ALLOWED");
    }
    for (name, _, reason) in ALLOWED {
        assert!(packages.contains_key(*name), "{name} is gone; drop its ALLOWED entry");
        assert!(!reason.is_empty());
    }

    let no_net = |name: &str| !packages[name].iter().any(|d| d == "mio" || d == "socket2");
    assert!(no_net("tokio"), "tokio gained its `net` feature");
    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).unwrap();
    assert!(
        manifest
            .lines()
            .any(|l| l == "ureq = { version = \"3\", default-features = false }"),
        "ureq must stay without default features (they bring TLS)"
    );
}

/// The only first-party files that may name a socket API or the HTTP client.
const SOCKET_FILES: &[(&str, &str)] = &[
    ("crates/rain-server/src/lib.rs", "binds the loopback listeners"),
    (
        "crates/rain-server/src/routes/lock.rs",
        "asks 127.0.0.1 whether the window in a lock file still answers",
    ),
    ("crates/rain-cli/src/client.rs", "calls a running window on localhost"),
    ("crates/rain-cli/src/sessions.rs", "its tests reserve a loopback port"),
];

const SOCKET_WORDS: &[&str] = &[
    "std::net",
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "ToSocketAddrs",
    "ureq",
    "Server::http",
    "ws2_32",
    "winhttp",
    "wininet",
];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn only_the_listed_files_touch_sockets_and_only_on_loopback() {
    let root = root();
    let mut files = Vec::new();
    for entry in std::fs::read_dir(root.join("crates")).unwrap() {
        let src = entry.unwrap().path().join("src");
        if src.is_dir() {
            sources(&src, &mut files);
        }
    }
    assert!(files.len() > 50, "the source walk found {} files", files.len());
    let mut seen = Vec::new();
    for path in files {
        let text = std::fs::read_to_string(&path).unwrap();
        let lower = text.to_ascii_lowercase();
        if !SOCKET_WORDS.iter().any(|w| lower.contains(&w.to_ascii_lowercase())) {
            continue;
        }
        let rel = path.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
        assert!(
            SOCKET_FILES.iter().any(|(f, _)| rel == *f),
            "{rel} names a socket API or the HTTP client; if it stays on loopback, list it in SOCKET_FILES"
        );
        seen.push(rel);
    }
    assert_eq!(seen.len(), SOCKET_FILES.len(), "a SOCKET_FILES entry no longer applies: {seen:?}");

    let client = std::fs::read_to_string(root.join("crates/rain-cli/src/client.rs")).unwrap();
    assert!(client.contains(".proxy(None)"), "the client must not use a proxy");
    assert!(!client.contains("https://"), "the client has no TLS");
    for (at, _) in client.match_indices("http://") {
        assert!(
            client[at..].starts_with("http://localhost:"),
            "the client names a URL that is not localhost"
        );
    }
    let lock = std::fs::read_to_string(root.join("crates/rain-server/src/routes/lock.rs")).unwrap();
    let connects = lock.matches("TcpStream::connect").count();
    assert_eq!(connects, 1, "one connect, checked below");
    assert!(lock.contains("SocketAddr::from(([127, 0, 0, 1], port))"));
}

#[test]
fn the_server_answers_on_loopback_and_not_on_the_machines_own_address() {
    let server = rain_server::start(Arc::new(Shared::default()), Some(0)).unwrap();
    let wait = Duration::from_millis(500);
    let loopback = SocketAddr::from(([127, 0, 0, 1], server.port));
    assert!(TcpStream::connect_timeout(&loopback, wait).is_ok());

    // A UDP connect sends nothing: it only asks the OS which local address would be used.
    let own = UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| s.connect("192.0.2.1:9").and_then(|()| s.local_addr()))
        .map(|a| a.ip())
        .ok()
        .filter(|ip| !ip.is_loopback() && !ip.is_unspecified());
    let Some(own) = own else {
        eprintln!("skipped: this machine has no address besides loopback");
        return;
    };
    let outside = SocketAddr::new(own, server.port);
    assert!(
        TcpStream::connect_timeout(&outside, wait).is_err(),
        "the server accepted a connection on {outside}"
    );
}
