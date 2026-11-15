//! `docs/agent.md` names every subcommand and every agent route, so it cannot drift from the CLI.

use clap::CommandFactory;
use rain_cli::Cli;

fn doc() -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/agent.md");
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn agent_doc_names_every_subcommand_and_its_flags() {
    let doc = doc();
    let mut missing = Vec::new();
    for sub in Cli::command().get_subcommands() {
        let leaves: Vec<_> = sub.get_subcommands().collect();
        let commands: Vec<(String, &clap::Command)> = if leaves.is_empty() {
            vec![(format!("rain {}", sub.get_name()), sub)]
        } else {
            leaves
                .into_iter()
                .map(|l| (format!("rain {} {}", sub.get_name(), l.get_name()), l))
                .collect()
        };
        for (name, cmd) in commands {
            if !doc.contains(&name) {
                missing.push(name.clone());
            }
            for flag in cmd.get_arguments().filter_map(|a| a.get_long()).filter(|f| *f != "help") {
                if !doc.contains(&format!("--{flag}")) {
                    missing.push(format!("{name} --{flag}"));
                }
            }
        }
    }
    assert!(missing.is_empty(), "docs/agent.md does not mention: {missing:?}");
}

#[test]
fn agent_doc_lists_the_overview_kinds_the_validator_accepts() {
    let doc = doc();
    let list = |kinds: &[&str]| kinds.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ");
    for (what, kinds) in [("Node kinds", rain_overview::NODE_KINDS), ("Edge kinds", rain_overview::EDGE_KINDS)] {
        let line = format!("{what}: {}.", list(kinds));
        assert!(doc.contains(&line), "docs/agent.md needs the line `{line}`");
    }
}

#[test]
fn agent_doc_names_every_route() {
    let doc = doc();
    for route in [
        "GET /api/health",
        "POST /api/comment-imports",
        "GET /api/comments-json",
        "GET /api/comments-output",
        "DELETE /api/comments/",
        "POST /api/comments",
        "POST /api/notes",
        "GET /api/notes",
        "DELETE /api/notes/",
        "POST /api/goto",
        "GET /api/events",
        "PUT /api/overview",
        "GET /api/overview",
        "POST /api/comments/<id>/state",
        "GET /api/state",
        "POST /api/highlight",
        "POST /api/reload",
        "POST /api/close",
        "POST /api/presence",
        "GET /api/presence",
        "GET /api/brief",
        "PUT /api/brief",
        "GET /api/triage",
        "GET /api/review",
        "POST /api/review",
        "GET /api/lock",
    ] {
        assert!(doc.contains(route), "docs/agent.md does not mention {route}");
    }
}
