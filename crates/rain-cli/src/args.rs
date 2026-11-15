//! clap definitions. Positional and flag names follow difit's so difit tooling can swap its entry
//! for this binary.

use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "rain",
    version,
    about = "Native code-review diff viewer (difit-compatible comment routes)",
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Sub>,
    #[command(flatten)]
    pub open: OpenArgs,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Args)]
pub struct OpenArgs {
    /// Revision to review: a commit-ish, or `.` (all uncommitted), `working` (unstaged), `staged`.
    pub target: Option<String>,
    /// Revision to compare with; defaults to the target's parent (`HEAD` for `.`/`staged`).
    pub base: Option<String>,
    /// Diff against `git merge-base <target> <base>`.
    #[arg(long)]
    pub merge_base: bool,
    /// A repository to show (repeat for one window over several repos); default: the current directory.
    #[arg(long = "repo", value_name = "DIR")]
    pub repos: Vec<PathBuf>,
    /// Session name; its threads and viewed state persist in `%LOCALAPPDATA%\Rain\sessions\<name>.json`.
    /// Default: `<repo folder>-<branch or target>`. A window already serving it is reused.
    #[arg(long)]
    pub session: Option<String>,
    /// Reopen the most recently used session with the repos and revisions it was opened with.
    #[arg(long, conflicts_with_all = ["target", "base", "repos", "session", "fixture", "merge_base", "include_untracked"])]
    pub last: bool,
    /// Preferred port for the comment routes (the next free one is used when taken).
    #[arg(long)]
    pub port: Option<u16>,
    /// Start in the background, print `{"port","url","pid"}` and exit.
    #[arg(long)]
    pub background: bool,
    /// Comment import JSON (object or array), or `@file` holding it. Repeatable.
    #[arg(long = "comment", value_name = "JSON|@FILE")]
    pub comments: Vec<String>,
    /// Overview document (difit-overview/v1) to load at start.
    #[arg(long, value_name = "FILE")]
    pub overview: Option<PathBuf>,
    /// Show untracked files as added.
    #[arg(long)]
    pub include_untracked: bool,
    /// Hide whitespace-only changes; files left with none are tagged formatting-only.
    #[arg(long, overrides_with = "no_ignore_whitespace")]
    pub ignore_whitespace: bool,
    /// Show whitespace-only changes (default, as difit).
    #[arg(long, overrides_with = "ignore_whitespace")]
    pub no_ignore_whitespace: bool,
    /// Context lines around each change.
    #[arg(long, default_value_t = 3)]
    pub context: u32,
    /// Start with an empty session instead of the saved one.
    #[arg(long)]
    pub clean: bool,
    /// Open at `<repo>/<path>:<line>`.
    #[arg(long, value_name = "REPO/PATH:LINE")]
    pub goto: Option<String>,
    /// Show this `.patch` instead of running git.
    #[arg(long, value_name = "PATCH")]
    pub fixture: Option<PathBuf>,
    /// Accepted for difit compatibility; Rain always keeps running until its window closes.
    #[arg(long, hide = true)]
    pub keep_alive: bool,
    /// Accepted for difit compatibility; there is no browser to open.
    #[arg(long = "no-open", hide = true)]
    pub no_open: bool,
}

impl OpenArgs {
    pub fn ignores_whitespace(&self) -> bool {
        self.ignore_whitespace && !self.no_ignore_whitespace
    }
}

/// Which running Rain a subcommand talks to.
#[derive(Debug, Default, Clone, PartialEq, Eq, Args)]
pub struct Conn {
    /// Port of the running Rain; wins over `--session`. Default: `RAIN_PORT`.
    #[arg(long)]
    pub port: Option<u16>,
    /// Session of the running Rain; its port comes from `sessions/<id>.lock`. Default: `RAIN_SESSION`.
    #[arg(long, value_name = "ID")]
    pub session: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum Sub {
    /// Open a window, as the bare form does (`rain open --last`, `rain open HEAD --session X`).
    Open(OpenArgs),
    /// Add, read, answer or settle comments on a running Rain.
    Comment {
        #[command(subcommand)]
        cmd: CommentCmd,
    },
    /// Push, read or check an overview document.
    Overview {
        #[command(subcommand)]
        cmd: OverviewCmd,
    },
    /// Add, list or remove agent notes (story, file and line notes; never part of comment readback).
    Note {
        #[command(subcommand)]
        cmd: NoteCmd,
    },
    /// Push, read or check the Brief (intent, how, invariants, tour).
    Brief {
        #[command(subcommand)]
        cmd: BriefCmd,
    },
    /// Read finished review rounds.
    Review {
        #[command(subcommand)]
        cmd: ReviewCmd,
    },
    /// Scroll the open window to a file, line or thread.
    Goto {
        /// `<repo>/<path>[:<line>]` (repo prefix in multi-repo windows).
        #[arg(value_name = "REPO/PATH[:LINE]", required_unless_present = "thread", conflicts_with = "thread")]
        spec: Option<String>,
        #[arg(long, value_name = "THREAD_ID")]
        thread: Option<String>,
        /// Side of the line in `spec`.
        #[arg(long, value_enum)]
        side: Option<SideArg>,
        #[command(flatten)]
        conn: Conn,
    },
    /// Spotlight a range in the reader (the rest dims), optionally as step i of n of a walkthrough.
    Highlight {
        /// `<repo>/<path>:<a>-<b>` or `<repo>/<path>:<line>`.
        #[arg(value_name = "REPO/PATH:A-B", value_parser = parse_range_spec)]
        spec: (String, (u32, u32)),
        #[arg(long, value_enum)]
        side: Option<SideArg>,
        /// One line shown with the spotlight.
        #[arg(long)]
        caption: Option<String>,
        /// `i/n`: this range is step i of a walkthrough of n.
        #[arg(long, value_name = "I/N", value_parser = parse_step)]
        step: Option<(u32, u32)>,
        #[command(flatten)]
        conn: Conn,
    },
    /// Wait for typed session events (replies, questions to answer, state changes…); exit 2 when
    /// the timeout passes with none.
    Watch(WatchArgs),
    /// Print the triage: groups, reasons, facts and the one reading order.
    Triage {
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        #[command(flatten)]
        conn: Conn,
    },
    /// Print where the reviewer is (file, line, selection, mode, viewed files, round, place).
    State {
        #[command(flatten)]
        conn: Conn,
    },
    /// Say what the agent is doing; the window shows it in the presence dot.
    Presence {
        #[arg(value_enum)]
        state: PresenceArg,
        /// The thread or file it is working on.
        #[arg(long, value_name = "ID")]
        on: Option<String>,
        #[arg(long)]
        reason: Option<String>,
        #[command(flatten)]
        conn: Conn,
    },
    /// Reload the diff in the open window, keeping the reader's place.
    Reload {
        #[command(flatten)]
        conn: Conn,
    },
    /// Save and close the open window; waits until its process is gone.
    Close {
        /// Seconds to wait for the process to end.
        #[arg(long, default_value_t = 15)]
        timeout: u64,
        #[command(flatten)]
        conn: Conn,
    },
    /// Print the running instance's health JSON (session, repos, counts, shas, agent, asks, lock).
    Health {
        #[command(flatten)]
        conn: Conn,
    },
    /// Print the version.
    Version,
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct WatchArgs {
    /// `agent`: the caller is the attached agent (keeps its presence alive) and its own events are left out.
    #[arg(long = "for", value_enum, value_name = "WHO")]
    pub for_who: Option<Audience>,
    /// Comma-separated event kinds; default: every kind.
    #[arg(long, value_name = "KIND,…", value_delimiter = ',', value_enum)]
    pub kinds: Vec<EventArg>,
    /// The `seq` last seen; default: the newest now, so the next event returns.
    #[arg(long)]
    pub since: Option<u64>,
    /// Leave out events by this author (with `--for agent` the default is `claude`).
    #[arg(long)]
    pub author: Option<String>,
    /// Seconds to wait (the server caps it at 60).
    #[arg(long, default_value_t = 25)]
    pub timeout: u64,
    #[arg(long, value_enum, default_value_t = Format::Json)]
    pub format: Format,
    #[command(flatten)]
    pub conn: Conn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Audience {
    Agent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EventArg {
    Thread,
    Reply,
    Disposition,
    State,
    Viewed,
    Ask,
    Answer,
    Submitted,
    Reload,
    Presence,
    Brief,
    Notes,
    Overview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PresenceArg {
    Idle,
    Working,
    Blocked,
    Done,
    Failed,
    Offline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Format {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SideArg {
    Old,
    New,
}

impl SideArg {
    pub fn as_str(self) -> &'static str {
        match self {
            SideArg::Old => "old",
            SideArg::New => "new",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StateArg {
    Open,
    Addressed,
    Resolved,
    Dismissed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReasonArg {
    FalsePositive,
    OutOfScope,
    Style,
    Duplicate,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum KindArg {
    Comment,
    Issue,
    Question,
    Nit,
    Suggestion,
    Praise,
}

/// The wire name of a value-enum member (`out-of-scope`, `addressed`, …).
pub fn wire<T: ValueEnum>(v: T) -> String {
    v.to_possible_value().map(|p| p.get_name().to_owned()).unwrap_or_default()
}

#[derive(Debug, Subcommand)]
pub enum CommentCmd {
    /// Add threads: comment import JSON (object or array, as text, `@file` or stdin), or one thread
    /// from `--body` with `--file`/`--line`, `--file-level` or `--pr-level`.
    Add(CommentAdd),
    /// Print the open threads (text for a prompt, or JSON); `--all` adds settled ones.
    Get {
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        /// Only threads of this repo id.
        #[arg(long)]
        repo: Option<String>,
        /// Also resolved, dismissed and addressed threads.
        #[arg(long)]
        all: bool,
        #[command(flatten)]
        conn: Conn,
    },
    /// Reply to a thread, by id or by the file and line it is anchored on.
    Reply {
        #[arg(long, value_name = "THREAD_ID", required_unless_present = "file", conflicts_with = "file")]
        thread: Option<String>,
        /// `<repo>/<path>` of the thread (joins the newest thread anchored at exactly that line or range).
        #[arg(long, value_name = "REPO/PATH", requires = "at")]
        file: Option<String>,
        #[arg(long, requires = "file", group = "at")]
        line: Option<u32>,
        #[arg(long, value_name = "A-B", requires = "file", group = "at", value_parser = parse_lines)]
        lines: Option<(u32, u32)>,
        #[arg(long, value_enum, requires = "at")]
        side: Option<SideArg>,
        /// Reply text, `@file` holding it, or nothing to read stdin.
        #[arg(long, value_name = "TEXT|@FILE")]
        body: Option<String>,
        #[arg(long, default_value = "claude")]
        author: String,
        /// Repo id `--file` is relative to (instead of a `<repo>/` prefix).
        #[arg(long)]
        repo: Option<String>,
        #[command(flatten)]
        conn: Conn,
    },
    /// Set a thread's state: `addressed` (with the fixing `--sha`), `resolved`, `dismissed`
    /// (with a `--reason`) or `open` again.
    SetState {
        #[arg(value_name = "THREAD_ID")]
        id: String,
        #[arg(value_enum)]
        state: StateArg,
        /// The commit that addresses it.
        #[arg(long)]
        sha: Option<String>,
        /// Why it is dismissed (required for `dismissed`).
        #[arg(long, value_enum, required_if_eq("state", "dismissed"))]
        reason: Option<ReasonArg>,
        #[arg(long, default_value = "claude")]
        author: String,
        #[command(flatten)]
        conn: Conn,
    },
    /// Resolve threads by id, or every thread whose anchor covers a file line (kept, with
    /// `state: resolved`; `comment get --all` still shows them).
    #[command(alias = "remove")]
    Resolve {
        #[arg(value_name = "THREAD_ID", required_unless_present = "file", conflicts_with = "file")]
        ids: Vec<String>,
        #[arg(long, value_name = "REPO/PATH", requires = "line")]
        file: Option<String>,
        #[arg(long, requires = "file")]
        line: Option<u32>,
        #[arg(long, value_enum, requires = "line")]
        side: Option<SideArg>,
        /// Repo id `--file` is relative to.
        #[arg(long)]
        repo: Option<String>,
        #[command(flatten)]
        conn: Conn,
    },
    /// Wait until the session changes, then print every open thread and note (the round-2 shape;
    /// `rain watch` prints typed events instead); exit 2 when the timeout passes with no change.
    Wait {
        /// The `version` last seen; default: the version now, so the next change returns.
        #[arg(long)]
        since: Option<u64>,
        /// Seconds to wait (the server caps it at 60).
        #[arg(long, default_value_t = 25)]
        timeout: u64,
        #[arg(long, value_enum, default_value_t = Format::Json)]
        format: Format,
        #[command(flatten)]
        conn: Conn,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
#[command(group(ArgGroup::new("at").args(["line", "lines"])))]
#[command(group(ArgGroup::new("level").args(["file_level", "pr_level"])))]
pub struct CommentAdd {
    /// Import JSON (object or array) or `@file` holding it; left out with no `--body`: stdin.
    #[arg(value_name = "JSON|@FILE", conflicts_with = "body")]
    pub json: Option<String>,
    /// Repo id the threads belong to (multi-repo sessions).
    #[arg(long)]
    pub repo: Option<String>,
    /// The kind of every thread that names none.
    #[arg(long, value_enum)]
    pub kind: Option<KindArg>,
    /// About the whole file, not a line (shown under the file header).
    #[arg(long)]
    pub file_level: bool,
    /// About the whole change (top of the Conversation).
    #[arg(long, conflicts_with_all = ["file", "line", "lines", "side"])]
    pub pr_level: bool,
    /// One thread's text, or `@file` holding it.
    #[arg(long, value_name = "TEXT|@FILE")]
    pub body: Option<String>,
    /// `<repo>/<path>` of that thread.
    #[arg(long, value_name = "REPO/PATH", requires = "body")]
    pub file: Option<String>,
    #[arg(long, requires = "file", conflicts_with = "file_level")]
    pub line: Option<u32>,
    #[arg(long, value_name = "A-B", requires = "file", conflicts_with = "file_level", value_parser = parse_lines)]
    pub lines: Option<(u32, u32)>,
    #[arg(long, value_enum, requires = "at")]
    pub side: Option<SideArg>,
    /// Author of every thread that names none; an empty author is the reviewer.
    #[arg(long, default_value = "claude")]
    pub author: String,
    #[command(flatten)]
    pub conn: Conn,
}

#[derive(Debug, Subcommand)]
pub enum NoteCmd {
    /// Add a story, file or line note (or a JSON batch); the window shows it at once.
    Add(NoteAdd),
    /// Print the notes (stored ones, then the overview's).
    List {
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        /// Only the file and line notes of `<repo>/<path>`.
        #[arg(long, value_name = "REPO/PATH", conflicts_with = "story")]
        file: Option<String>,
        /// Only story notes.
        #[arg(long)]
        story: bool,
        #[arg(long)]
        repo: Option<String>,
        #[command(flatten)]
        conn: Conn,
    },
    /// Remove notes by id.
    Remove {
        #[arg(required = true, value_name = "NOTE_ID")]
        ids: Vec<String>,
        #[command(flatten)]
        conn: Conn,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
#[command(group(ArgGroup::new("target").required(true).args(["story", "file", "json"])))]
#[command(group(ArgGroup::new("at").args(["line", "lines"])))]
pub struct NoteAdd {
    #[command(flatten)]
    pub conn: Conn,
    /// A note about the whole change.
    #[arg(long)]
    pub story: bool,
    /// A note about `<repo>/<path>` (with `--line`/`--lines`: about those lines).
    #[arg(long, value_name = "REPO/PATH")]
    pub file: Option<String>,
    #[arg(long, requires = "file", conflicts_with = "story")]
    pub line: Option<u32>,
    #[arg(long, value_name = "A-B", requires = "file", conflicts_with = "story", value_parser = parse_lines)]
    pub lines: Option<(u32, u32)>,
    #[arg(long, value_enum, requires = "at")]
    pub side: Option<SideArg>,
    /// Markdown text, `@file` holding it, or nothing to read stdin.
    #[arg(long, value_name = "TEXT|@FILE", conflicts_with = "json")]
    pub body: Option<String>,
    #[arg(long, conflicts_with = "json")]
    pub title: Option<String>,
    /// Defaults to `claude`.
    #[arg(long, conflicts_with = "json")]
    pub author: Option<String>,
    /// Update the note with this id instead of adding one.
    #[arg(long, conflicts_with = "json")]
    pub id: Option<String>,
    /// Note JSON (object or array) or `@file` holding it.
    #[arg(long, value_name = "JSON|@FILE")]
    pub json: Option<String>,
    /// Drop existing notes of the same kind, file and author first.
    #[arg(long)]
    pub replace: bool,
    /// Repo id the paths are relative to (instead of a `<repo>/` prefix).
    #[arg(long)]
    pub repo: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum BriefCmd {
    /// Load a Brief (`rain-brief/v1` JSON, a Markdown Brief, a `.md` file, `@file` holding either,
    /// or stdin) into a running Rain.
    Set {
        #[arg(value_name = "JSON|FILE.md|@FILE")]
        brief: Option<String>,
        #[command(flatten)]
        conn: Conn,
    },
    /// Print the Brief, its version, whether it is stale and its check.
    Get {
        #[command(flatten)]
        conn: Conn,
    },
    /// Print what the Brief gets wrong against the loaded diff; exit 1 when anything is.
    Check {
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        #[command(flatten)]
        conn: Conn,
    },
}

#[derive(Debug, Subcommand)]
pub enum ReviewCmd {
    /// Print the finished rounds (verdict, summary, shas, dispositions, open questions), or one.
    Get {
        #[arg(long, value_name = "N")]
        round: Option<u32>,
        #[arg(long, value_enum, default_value_t = Format::Json)]
        format: Format,
        #[command(flatten)]
        conn: Conn,
    },
}

fn parse_lines(s: &str) -> Result<(u32, u32), String> {
    let bad = || "Expected <start>-<end>, e.g. 12-18.".to_owned();
    let (a, b) = s.split_once('-').ok_or_else(bad)?;
    let (a, b) = (
        a.trim().parse::<u32>().map_err(|_| bad())?,
        b.trim().parse::<u32>().map_err(|_| bad())?,
    );
    if a == 0 || b < a {
        return Err(bad());
    }
    Ok((a, b))
}

/// `path:a-b` or `path:n` → `(path, (a, b))`.
fn parse_range_spec(s: &str) -> Result<(String, (u32, u32)), String> {
    let bad = || "Expected <repo>/<path>:<start>-<end>, e.g. server/src/Voucher.cs:20-27.".to_owned();
    let (path, at) = s.rsplit_once(':').ok_or_else(bad)?;
    if path.trim().is_empty() {
        return Err(bad());
    }
    let lines = if at.contains('-') {
        parse_lines(at).map_err(|_| bad())?
    } else {
        let n = at.trim().parse::<u32>().ok().filter(|n| *n > 0).ok_or_else(bad)?;
        (n, n)
    };
    Ok((path.to_owned(), lines))
}

fn parse_step(s: &str) -> Result<(u32, u32), String> {
    let bad = || "Expected <i>/<n>, e.g. 3/9.".to_owned();
    let (i, n) = s.split_once('/').ok_or_else(bad)?;
    match (i.trim().parse::<u32>(), n.trim().parse::<u32>()) {
        (Ok(i), Ok(n)) if i >= 1 && i <= n => Ok((i, n)),
        _ => Err(bad()),
    }
}

#[derive(Debug, Subcommand)]
pub enum OverviewCmd {
    /// Load an overview into a running Rain.
    Set {
        file: PathBuf,
        /// Repo id this instance serves (overrides the folder match).
        #[arg(long = "self", value_name = "REPO_ID")]
        self_repo: Option<String>,
        /// Base URL of the instance serving another repo (repeatable).
        #[arg(long = "link", value_name = "REPO_ID=URL", value_parser = parse_link)]
        links: Vec<(String, String)>,
        #[command(flatten)]
        conn: Conn,
    },
    /// Print the overview state of a running Rain.
    Get {
        #[command(flatten)]
        conn: Conn,
    },
    /// Check an overview file without a running Rain.
    Validate { file: PathBuf },
    /// Draw every view of an overview file to PNGs, dark and light, without a window.
    Render {
        file: PathBuf,
        /// Folder for the PNGs (created when missing).
        #[arg(long)]
        out: PathBuf,
        /// Image size in points, `WIDTHxHEIGHT`.
        #[arg(long, default_value = "1600x1000", value_parser = parse_size)]
        size: (u32, u32),
    },
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let bad = || "Expected <width>x<height>, e.g. 1600x1000.".to_owned();
    let (w, h) = s.split_once(['x', 'X']).ok_or_else(bad)?;
    match (w.trim().parse::<u32>(), h.trim().parse::<u32>()) {
        (Ok(w), Ok(h)) if (200..=8000).contains(&w) && (200..=8000).contains(&h) => Ok((w, h)),
        _ => Err(bad()),
    }
}

fn parse_link(s: &str) -> Result<(String, String), String> {
    match s.split_once('=') {
        Some((id, url)) if !id.is_empty() => Ok((id.to_owned(), url.to_owned())),
        _ => Err("Expected <repoId>=<url>.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("rain").chain(args.iter().copied())).unwrap()
    }

    fn port(p: u16) -> Conn {
        Conn {
            port: Some(p),
            session: None,
        }
    }

    #[test]
    fn parses_the_difit_shaped_launch() {
        let cli = parse(&["HEAD", "origin/development", "--merge-base", "--background"]);
        assert!(cli.command.is_none());
        let o = cli.open;
        assert_eq!(o.target.as_deref(), Some("HEAD"));
        assert_eq!(o.base.as_deref(), Some("origin/development"));
        assert!(o.merge_base && o.background);
        assert!(!o.ignores_whitespace(), "whitespace shows by default, as in difit");
        assert_eq!(o.context, 3);
    }

    #[test]
    fn parses_the_story_form() {
        let o = parse(&["--repo", "a", "--repo", "b", "--session", "review-ACME-1"]).open;
        assert_eq!(o.repos, [PathBuf::from("a"), PathBuf::from("b")]);
        assert_eq!(o.session.as_deref(), Some("review-ACME-1"));
        assert!(o.target.is_none());
    }

    #[test]
    fn last_reopens_without_revisions() {
        let o = parse(&["--last", "--background"]).open;
        assert!(o.last && o.background);
        assert!(fails(&["--last", "HEAD"]));
        assert!(fails(&["--last", "--session", "x"]));
        assert!(fails(&["--last", "--repo", "a"]));
    }

    #[test]
    fn comment_and_whitespace_flags() {
        let o = parse(&[
            ".",
            "--include-untracked",
            "--comment",
            r#"[{"type":"thread"}]"#,
            "--comment",
            "@c.json",
            "--no-ignore-whitespace",
        ])
        .open;
        assert_eq!(o.comments.len(), 2);
        assert!(!o.ignores_whitespace());
        assert!(o.include_untracked);
        let o = parse(&["--no-ignore-whitespace", "--ignore-whitespace", "--keep-alive", "--no-open"]).open;
        assert!(o.ignores_whitespace());
        assert!(!parse(&["--ignore-whitespace", "--no-ignore-whitespace"]).open.ignores_whitespace());
    }

    #[test]
    fn subcommands() {
        let cli = parse(&["comment", "get", "--port", "4966", "--format", "json"]);
        assert!(matches!(
            cli.command,
            Some(Sub::Comment {
                cmd: CommentCmd::Get {
                    format: Format::Json,
                    all: false,
                    ref conn,
                    ..
                }
            }) if *conn == port(4966)
        ));
        let cli = parse(&["comment", "remove", "a", "b", "--port", "1"]);
        assert!(matches!(cli.command, Some(Sub::Comment { cmd: CommentCmd::Resolve { ref ids, .. } }) if ids.len() == 2));
        let cli = parse(&[
            "overview",
            "set",
            "o.json",
            "--port",
            "2",
            "--self",
            "server",
            "--link",
            "app=http://localhost:4967",
        ]);
        let Some(Sub::Overview {
            cmd: OverviewCmd::Set { self_repo, links, .. },
        }) = cli.command
        else {
            panic!()
        };
        assert_eq!(self_repo.as_deref(), Some("server"));
        assert_eq!(links, [("app".to_owned(), "http://localhost:4967".to_owned())]);
        assert!(Cli::try_parse_from(["rain", "overview", "set", "o.json", "--port", "2", "--link", "nope"]).is_err());
        assert!(matches!(
            parse(&["overview", "render", "o.json", "--out", "d"]).command,
            Some(Sub::Overview {
                cmd: OverviewCmd::Render { size: (1600, 1000), .. }
            })
        ));
        assert!(matches!(
            parse(&["overview", "render", "o.json", "--out", "d", "--size", "800x600"]).command,
            Some(Sub::Overview {
                cmd: OverviewCmd::Render { size: (800, 600), .. }
            })
        ));
        assert!(fails(&["overview", "render", "o.json", "--out", "d", "--size", "big"]));
        assert!(matches!(parse(&["version"]).command, Some(Sub::Version)));
    }

    #[test]
    fn every_client_takes_a_port_or_a_session() {
        let cli = parse(&["comment", "get", "--session", "review-ACME-1", "--all"]);
        assert!(matches!(
            cli.command,
            Some(Sub::Comment { cmd: CommentCmd::Get { all: true, ref conn, .. } })
                if conn.session.as_deref() == Some("review-ACME-1") && conn.port.is_none()
        ));
        let cli = parse(&["health", "--session", "s", "--port", "9"]);
        assert!(matches!(cli.command, Some(Sub::Health { ref conn }) if conn.port == Some(9) && conn.session.is_some()));
        assert!(matches!(parse(&["state"]).command, Some(Sub::State { ref conn }) if *conn == Conn::default()));
        assert!(matches!(parse(&["reload", "--port", "3"]).command, Some(Sub::Reload { .. })));
        assert!(matches!(
            parse(&["close", "--session", "s"]).command,
            Some(Sub::Close { timeout: 15, .. })
        ));
    }

    #[test]
    fn set_state_needs_a_reason_to_dismiss() {
        let cli = parse(&["comment", "set-state", "k3", "addressed", "--sha", "abc123", "--port", "1"]);
        assert!(matches!(
            cli.command,
            Some(Sub::Comment { cmd: CommentCmd::SetState { ref id, state: StateArg::Addressed, sha: Some(ref s), reason: None, .. } })
                if id == "k3" && s == "abc123"
        ));
        assert!(fails(&["comment", "set-state", "k3", "dismissed", "--port", "1"]));
        let cli = parse(&["comment", "set-state", "k3", "dismissed", "--reason", "out-of-scope", "--port", "1"]);
        assert!(matches!(
            cli.command,
            Some(Sub::Comment {
                cmd: CommentCmd::SetState {
                    reason: Some(ReasonArg::OutOfScope),
                    ..
                }
            })
        ));
        assert_eq!(wire(ReasonArg::FalsePositive), "false-positive");
        assert_eq!(wire(StateArg::Resolved), "resolved");
        assert!(fails(&["comment", "set-state", "k3", "gone", "--port", "1"]));
    }

    fn comment_add(args: &[&str]) -> CommentAdd {
        match parse(&[&["comment", "add"], args].concat()).command {
            Some(Sub::Comment { cmd: CommentCmd::Add(a) }) => a,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn comment_add_forms() {
        let a = comment_add(&["@c.json", "--port", "1", "--kind", "issue"]);
        assert_eq!((a.json.as_deref(), a.kind), (Some("@c.json"), Some(KindArg::Issue)));
        let a = comment_add(&["--pr-level", "--body", "Ship it?", "--kind", "question", "--port", "1"]);
        assert!(a.pr_level && a.file.is_none());
        let a = comment_add(&["--file-level", "--file", "server/a.cs", "--body", "x", "--port", "1"]);
        assert!(a.file_level && a.author == "claude");
        let a = comment_add(&["--file", "a.cs", "--lines", "3-5", "--side", "old", "--body", "x", "--port", "1"]);
        assert_eq!((a.lines, a.side), (Some((3, 5)), Some(SideArg::Old)));
        assert!(fails(&["comment", "add", "--pr-level", "--file-level", "--port", "1"]));
        assert!(fails(&[
            "comment",
            "add",
            "--pr-level",
            "--file",
            "a",
            "--body",
            "x",
            "--port",
            "1"
        ]));
        assert!(fails(&[
            "comment",
            "add",
            "--file-level",
            "--file",
            "a",
            "--line",
            "3",
            "--body",
            "x"
        ]));
        assert!(fails(&["comment", "add", "{}", "--body", "x"]));
        assert!(fails(&["comment", "add", "--file", "a.cs"]), "--file needs --body");
    }

    #[test]
    fn watch_and_highlight() {
        let Some(Sub::Watch(w)) = parse(&["watch", "--for", "agent", "--kinds", "reply,ask", "--since", "4", "--session", "s"]).command
        else {
            panic!()
        };
        assert_eq!(
            (w.for_who, w.kinds.clone(), w.since, w.timeout),
            (Some(Audience::Agent), vec![EventArg::Reply, EventArg::Ask], Some(4), 25)
        );
        assert_eq!(wire(EventArg::Submitted), "submitted");
        assert!(fails(&["watch", "--kinds", "nope"]));
        let cli = parse(&["highlight", "server/a.cs:20-27", "--step", "3/9", "--caption", "The endpoint"]);
        let Some(Sub::Highlight { spec, step, caption, .. }) = cli.command else {
            panic!()
        };
        assert_eq!((spec, step), (("server/a.cs".to_owned(), (20, 27)), Some((3, 9))));
        assert_eq!(caption.as_deref(), Some("The endpoint"));
        assert!(matches!(
            parse(&["highlight", "a.cs:7"]).command,
            Some(Sub::Highlight { spec: (_, (7, 7)), .. })
        ));
        assert!(fails(&["highlight", "a.cs"]));
        assert!(fails(&["highlight", "a.cs:9-3"]));
        assert!(fails(&["highlight", "a.cs:1", "--step", "4/3"]));
    }

    #[test]
    fn brief_review_presence_triage() {
        assert!(matches!(
            parse(&["brief", "set", "@b.json", "--port", "1"]).command,
            Some(Sub::Brief { cmd: BriefCmd::Set { brief: Some(ref b), .. } }) if b == "@b.json"
        ));
        assert!(matches!(
            parse(&["brief", "check", "--port", "1"]).command,
            Some(Sub::Brief {
                cmd: BriefCmd::Check { format: Format::Text, .. }
            })
        ));
        assert!(matches!(
            parse(&["review", "get", "--round", "2", "--port", "1"]).command,
            Some(Sub::Review {
                cmd: ReviewCmd::Get { round: Some(2), .. }
            })
        ));
        assert!(matches!(
            parse(&["presence", "working", "--on", "k3", "--port", "1"]).command,
            Some(Sub::Presence {
                state: PresenceArg::Working,
                on: Some(_),
                ..
            })
        ));
        assert!(fails(&["presence", "dancing", "--port", "1"]));
        assert!(matches!(
            parse(&["triage", "--format", "json"]).command,
            Some(Sub::Triage { format: Format::Json, .. })
        ));
    }

    fn fails(args: &[&str]) -> bool {
        Cli::try_parse_from(std::iter::once("rain").chain(args.iter().copied())).is_err()
    }

    fn note_add(args: &[&str]) -> NoteAdd {
        match parse(&[&["note", "add"], args].concat()).command {
            Some(Sub::Note { cmd: NoteCmd::Add(a) }) => a,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn note_add_forms() {
        let a = note_add(&["--port", "1", "--story", "--body", "Summary"]);
        assert!(a.story && a.body.as_deref() == Some("Summary") && a.file.is_none());
        let a = note_add(&["--port", "1", "--file", "server/a.cs", "--line", "12", "--body", "@n.md"]);
        assert_eq!(
            (a.file.as_deref(), a.line, a.body.as_deref()),
            (Some("server/a.cs"), Some(12), Some("@n.md"))
        );
        let a = note_add(&["--port", "1", "--file", "a.cs", "--lines", "3-7", "--side", "old", "--replace"]);
        assert_eq!((a.lines, a.side, a.replace), (Some((3, 7)), Some(SideArg::Old), true));
        let a = note_add(&["--json", "@fixtures/notes/notes.json", "--port", "1", "--repo", "server"]);
        assert_eq!(a.json.as_deref(), Some("@fixtures/notes/notes.json"));
        let a = note_add(&["--port", "1", "--file", "a.cs", "--title", "T", "--author", "me", "--id", "n1"]);
        assert_eq!(
            (a.title.as_deref(), a.author.as_deref(), a.id.as_deref()),
            (Some("T"), Some("me"), Some("n1"))
        );
        let a = note_add(&["--session", "s", "--story", "--body", "x"]);
        assert_eq!(a.conn.session.as_deref(), Some("s"));

        assert!(
            fails(&["note", "add", "--port", "1", "--body", "x"]),
            "needs --story, --file or --json"
        );
        assert!(fails(&["note", "add", "--port", "1", "--story", "--file", "a"]));
        assert!(
            fails(&["note", "add", "--port", "1", "--story", "--line", "3"]),
            "--line needs --file"
        );
        assert!(fails(&[
            "note", "add", "--port", "1", "--file", "a", "--line", "3", "--lines", "3-4"
        ]));
        assert!(
            fails(&["note", "add", "--port", "1", "--file", "a", "--side", "old"]),
            "--side needs a line"
        );
        assert!(fails(&["note", "add", "--port", "1", "--file", "a", "--lines", "7-3"]));
        assert!(fails(&["note", "add", "--port", "1", "--file", "a", "--lines", "x"]));
        assert!(fails(&["note", "add", "--port", "1", "--json", "[]", "--body", "x"]));
    }

    #[test]
    fn note_list_and_remove() {
        let cli = parse(&["note", "list", "--port", "3", "--format", "json", "--file", "server/a.cs"]);
        assert!(matches!(
            cli.command,
            Some(Sub::Note { cmd: NoteCmd::List { format: Format::Json, file: Some(ref f), story: false, ref conn, .. } }) if f == "server/a.cs" && *conn == port(3)
        ));
        assert!(matches!(
            parse(&["note", "list", "--port", "3", "--story"]).command,
            Some(Sub::Note {
                cmd: NoteCmd::List {
                    story: true,
                    format: Format::Text,
                    ..
                }
            })
        ));
        assert!(fails(&["note", "list", "--port", "3", "--story", "--file", "a"]));
        assert!(matches!(
            parse(&["note", "remove", "a", "b", "--port", "3"]).command,
            Some(Sub::Note { cmd: NoteCmd::Remove { ref ids, .. } }) if ids.len() == 2
        ));
        assert!(fails(&["note", "remove", "--port", "3"]));
    }

    #[test]
    fn comment_reply_and_resolve_by_line() {
        let cli = parse(&["comment", "reply", "--port", "1", "--thread", "t1", "--body", "ok"]);
        let Some(Sub::Comment {
            cmd:
                CommentCmd::Reply {
                    thread,
                    author,
                    body,
                    file,
                    ..
                },
        }) = cli.command
        else {
            panic!()
        };
        assert_eq!(
            (thread.as_deref(), author.as_str(), body.as_deref(), file),
            (Some("t1"), "claude", Some("ok"), None)
        );
        let cli = parse(&[
            "comment",
            "reply",
            "--port",
            "1",
            "--file",
            "server/a.cs",
            "--line",
            "9",
            "--side",
            "old",
        ]);
        assert!(matches!(
            cli.command,
            Some(Sub::Comment {
                cmd: CommentCmd::Reply {
                    line: Some(9),
                    side: Some(SideArg::Old),
                    ..
                }
            })
        ));
        assert!(
            fails(&["comment", "reply", "--port", "1", "--body", "x"]),
            "needs --thread or --file"
        );
        assert!(fails(&["comment", "reply", "--port", "1", "--file", "a"]), "--file needs --line");
        assert!(fails(&[
            "comment", "reply", "--port", "1", "--thread", "t", "--file", "a", "--line", "1"
        ]));

        let cli = parse(&["comment", "resolve", "--file", "server/a.cs", "--line", "4", "--port", "1"]);
        assert!(matches!(
            cli.command,
            Some(Sub::Comment { cmd: CommentCmd::Resolve { ref ids, line: Some(4), .. } }) if ids.is_empty()
        ));
        assert!(fails(&["comment", "resolve", "--port", "1"]), "needs ids or --file");
        assert!(fails(&["comment", "resolve", "t1", "--file", "a", "--line", "1", "--port", "1"]));
        assert!(fails(&["comment", "resolve", "--file", "a", "--port", "1"]));
    }

    #[test]
    fn comment_reply_by_range() {
        let cli = parse(&[
            "comment", "reply", "--port", "1", "--file", "a.cs", "--lines", "4-6", "--body", "ok",
        ]);
        assert!(matches!(
            cli.command,
            Some(Sub::Comment {
                cmd: CommentCmd::Reply {
                    lines: Some((4, 6)),
                    line: None,
                    ..
                }
            })
        ));
        let cli = parse(&[
            "comment", "reply", "--port", "1", "--file", "a.cs", "--lines", "4-6", "--side", "old",
        ]);
        assert!(matches!(
            cli.command,
            Some(Sub::Comment {
                cmd: CommentCmd::Reply {
                    side: Some(SideArg::Old),
                    ..
                }
            })
        ));
        assert!(fails(&[
            "comment", "reply", "--port", "1", "--file", "a", "--line", "3", "--lines", "3-4"
        ]));
        assert!(
            fails(&["comment", "reply", "--port", "1", "--lines", "3-4"]),
            "--lines needs --file"
        );
        assert!(fails(&["comment", "reply", "--port", "1", "--file", "a", "--lines", "6-4"]));
        assert!(
            fails(&["comment", "reply", "--port", "1", "--thread", "t", "--side", "old"]),
            "--side needs a line"
        );
    }

    #[test]
    fn comment_wait() {
        assert!(matches!(
            parse(&["comment", "wait", "--port", "7"]).command,
            Some(Sub::Comment {
                cmd: CommentCmd::Wait {
                    since: None,
                    timeout: 25,
                    format: Format::Json,
                    ref conn,
                }
            }) if *conn == port(7)
        ));
        assert!(matches!(
            parse(&[
                "comment",
                "wait",
                "--port",
                "7",
                "--since",
                "12",
                "--timeout",
                "60",
                "--format",
                "text"
            ])
            .command,
            Some(Sub::Comment {
                cmd: CommentCmd::Wait {
                    since: Some(12),
                    timeout: 60,
                    format: Format::Text,
                    ..
                }
            })
        ));
        assert!(fails(&["comment", "wait", "--port", "7", "--since", "-1"]));
    }

    #[test]
    fn goto_and_health() {
        assert!(matches!(
            parse(&["goto", "server/a.cs:12", "--port", "5"]).command,
            Some(Sub::Goto { spec: Some(ref s), thread: None, ref conn, .. }) if s == "server/a.cs:12" && *conn == port(5)
        ));
        assert!(matches!(
            parse(&["goto", "--thread", "t1", "--port", "5"]).command,
            Some(Sub::Goto {
                spec: None,
                thread: Some(_),
                ..
            })
        ));
        assert!(fails(&["goto", "--port", "5"]));
        assert!(fails(&["goto", "a.cs", "--thread", "t", "--port", "5"]));
        assert!(matches!(parse(&["health", "--port", "5"]).command, Some(Sub::Health { ref conn }) if *conn == port(5)));
    }
}
