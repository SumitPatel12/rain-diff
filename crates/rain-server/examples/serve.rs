//! The comment and overview routes over an in-memory session, without a window: for checking
//! HTTP clients (difit's Node CLI, `rain comment`) against Rain on a headless machine.
//! `cargo run -p rain-server --example serve -- [PORT]` prints `{"port","url","pid"}` and
//! serves until killed.

use std::sync::Arc;

use rain_core::Shared;

fn main() -> std::io::Result<()> {
    let port = std::env::args().nth(1).and_then(|p| p.parse().ok());
    let server = rain_server::start(Arc::new(Shared::default()), port)?;
    println!(
        "{}",
        serde_json::json!({ "port": server.port, "url": server.url, "pid": std::process::id() })
    );
    loop {
        std::thread::park();
    }
}
