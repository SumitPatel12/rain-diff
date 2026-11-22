//! Command line: launch, background handshake, session reuse and the clients of a running Rain.

pub mod args;
pub mod background;
pub mod client;
pub mod exit_log;
pub mod launch;
pub mod passthrough;
pub mod sessions;

use clap::{CommandFactory, Parser};

pub use args::{Cli, CommentCmd, Conn, Format, NoteAdd, NoteCmd, OpenArgs, OverviewCmd, SideArg, Sub};

pub fn main() {
    let argv: Vec<String> = std::env::args_os().map(|a| a.to_string_lossy().into_owned()).collect();
    std::process::exit(run(argv));
}

/// Splits off Rune's flags for the window form only; subcommands keep every argument, so
/// `overview render --out --size` stay theirs.
pub fn split_args(args: &[String]) -> (Vec<String>, passthrough::RuneArgs) {
    match args.first().map(String::as_str) {
        Some("open") => {
            let (ours, rune) = passthrough::split(&args[1..]);
            (std::iter::once("open".to_owned()).chain(ours).collect(), rune)
        }
        Some(a) if Cli::command().find_subcommand(a).is_some() => (args.to_vec(), passthrough::RuneArgs::default()),
        _ => passthrough::split(args),
    }
}

/// Runs one invocation (`argv[0]` is the program) and returns the exit code.
pub fn run(argv: Vec<String>) -> i32 {
    let args = argv.get(1..).unwrap_or_default();
    let (ours, rune) = split_args(args);
    let program = argv.first().cloned().unwrap_or_else(|| "rain".into());
    let cli = Cli::parse_from(std::iter::once(program).chain(ours));
    match cli.command {
        Some(Sub::Open(o)) => launch::open_or_reuse(o, &rune, args),
        Some(sub) => client::run(sub),
        None => launch::open_or_reuse(cli.open, &rune, args),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn subcommands_keep_flags_rune_would_take() {
        let render = s(&["overview", "render", "o.json", "--out", "d", "--size", "800x600"]);
        let (ours, rune) = split_args(&render);
        assert_eq!((ours, rune.args.is_empty()), (render, true));
        let (ours, rune) = split_args(&s(&["HEAD", "--snap", "x.png"]));
        assert_eq!((ours, rune.args), (s(&["HEAD"]), s(&["--snap", "x.png"])));
        let (ours, rune) = split_args(&s(&["open", "--last", "--snap", "x.png"]));
        assert_eq!((ours, rune.args), (s(&["open", "--last"]), s(&["--snap", "x.png"])));
        let cli = Cli::try_parse_from(["rain", "open", "HEAD", "--session", "x"]).unwrap();
        assert!(matches!(cli.command, Some(Sub::Open(ref o)) if o.session.as_deref() == Some("x")));
    }
}
