//! Finding a running window by session (its lock file), reusing it on `open`, and `open --last`.

use std::path::{Path, PathBuf};

use rain_comments::SessionFile;
use rain_comments::store::sanitize;
use rain_core::Launch;
use rain_server::{Lock, lock_path, read_lock};
use serde::{Deserialize, Serialize};

use crate::args::{Conn, OpenArgs};

/// The port a subcommand talks to: `--port`, else the live lock of `--session`, else
/// `RAIN_PORT`, else `RAIN_SESSION`'s lock.
pub fn resolve(conn: &Conn) -> Result<u16, String> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    resolve_in(conn, env("RAIN_PORT"), env("RAIN_SESSION"), &SessionFile::dir())
}

pub fn resolve_in(conn: &Conn, env_port: Option<String>, env_session: Option<String>, dir: &Path) -> Result<u16, String> {
    if let Some(p) = conn.port {
        return Ok(p);
    }
    if let Some(s) = conn.session.as_deref() {
        return live_lock(dir, s).map(|l| l.port);
    }
    if let Some(p) = env_port {
        return p.trim().parse::<u16>().map_err(|_| format!("RAIN_PORT is not a port: {p}"));
    }
    if let Some(s) = env_session {
        return live_lock(dir, s.trim()).map(|l| l.port);
    }
    Err("Give --port <port> or --session <id> (or set RAIN_PORT or RAIN_SESSION)".into())
}

fn lock_file(dir: &Path, session: &str) -> PathBuf {
    lock_path(&SessionFile::path_in(dir, session))
}

/// The lock of the window serving `session`, when one is running.
pub fn live_lock(dir: &Path, session: &str) -> Result<Lock, String> {
    let path = lock_file(dir, session);
    let lock = read_lock(&path).ok_or_else(|| format!("No Rain is open for session {session} (no {})", path.display()))?;
    if lock.is_stale() {
        return Err(format!(
            "Rain for session {session} is not running (stale lock {}: port {}, pid {})",
            path.display(),
            lock.port,
            lock.pid
        ));
    }
    Ok(lock)
}

/// How a session was opened, so `open --last` can open it the same way.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LaunchRecord {
    pub session: String,
    pub repos: Vec<PathBuf>,
    pub target: String,
    pub base: String,
    pub merge_base: bool,
    pub include_untracked: bool,
    pub ignore_whitespace: bool,
    pub context: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixture: Option<PathBuf>,
}

impl LaunchRecord {
    pub fn of(launch: &Launch) -> LaunchRecord {
        let first = launch.repos.first();
        LaunchRecord {
            session: launch.session.clone(),
            repos: launch.repos.iter().map(|r| r.dir.clone()).collect(),
            target: first.map(|r| r.target.clone()).unwrap_or_default(),
            base: first.map(|r| r.base.clone()).unwrap_or_default(),
            merge_base: first.is_some_and(|r| r.merge_base),
            include_untracked: first.is_some_and(|r| r.include_untracked),
            ignore_whitespace: launch.ignore_whitespace,
            context: launch.context,
            fixture: launch.fixture.clone(),
        }
    }

    /// `o` with the recorded repos, revisions and diff flags; what the user typed besides
    /// `--last` (`--background`, `--port`, `--goto`, `--comment`…) stays.
    pub fn apply(&self, o: &OpenArgs) -> OpenArgs {
        OpenArgs {
            target: Some(self.target.clone()).filter(|t| !t.is_empty()),
            base: Some(self.base.clone()).filter(|b| !b.is_empty()),
            repos: self.repos.clone(),
            session: Some(self.session.clone()),
            last: false,
            merge_base: self.merge_base,
            include_untracked: self.include_untracked,
            ignore_whitespace: self.ignore_whitespace,
            no_ignore_whitespace: !self.ignore_whitespace,
            context: self.context.unwrap_or(o.context),
            fixture: self.fixture.clone(),
            ..o.clone()
        }
    }
}

/// `<data dir>/launches/<session>.json`, beside `sessions/` (not in it: `--last` picks the
/// newest file there).
pub fn record_path(data_dir: &Path, session: &str) -> PathBuf {
    data_dir.join("launches").join(format!("{}.json", sanitize(session)))
}

pub fn write_record(data_dir: &Path, launch: &Launch) {
    let path = record_path(data_dir, &launch.session);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text = serde_json::to_string_pretty(&LaunchRecord::of(launch)).unwrap_or_default();
    if let Err(e) = std::fs::write(&path, text) {
        eprintln!("Rain: writing {} failed: {e}", path.display());
    }
}

/// `o` for the most recently saved session in `data_dir`, opened as it was last time.
pub fn last_open(o: &OpenArgs, data_dir: &Path) -> Result<OpenArgs, String> {
    let sessions = data_dir.join("sessions");
    let (id, _) = rain_app::App::last_session(&sessions).ok_or_else(|| format!("No saved session in {}", sessions.display()))?;
    let path = record_path(data_dir, &id);
    let text = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "Session {id} was opened before Rain kept launch records ({} is missing); open it once with its repos and revisions",
            path.display()
        )
    })?;
    let mut record: LaunchRecord = serde_json::from_str(&text).map_err(|e| format!("Invalid {}: {e}", path.display()))?;
    if record.session.is_empty() {
        record.session = id;
    }
    Ok(record.apply(o))
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use rain_core::RepoLaunch;

    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rain-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sessions")).unwrap();
        d
    }

    #[test]
    fn port_wins_then_session_then_env() {
        let d = temp("resolve");
        let sessions = d.join("sessions");
        let conn = |port: Option<u16>, session: Option<&str>| Conn {
            port,
            session: session.map(str::to_owned),
        };
        assert_eq!(resolve_in(&conn(Some(7), Some("x")), None, None, &sessions), Ok(7));
        assert_eq!(resolve_in(&conn(None, None), Some("4966".into()), None, &sessions), Ok(4966));
        assert!(resolve_in(&conn(None, None), Some("nope".into()), None, &sessions).is_err());
        let none = resolve_in(&conn(None, None), None, None, &sessions).unwrap_err();
        assert!(none.contains("--session"), "{none}");
        let missing = resolve_in(&conn(None, Some("review-1")), None, None, &sessions).unwrap_err();
        assert!(missing.starts_with("No Rain is open for session review-1"), "{missing}");

        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let lock = format!(r#"{{"port":{port},"pid":{},"startedAt":"t"}}"#, std::process::id());
        std::fs::write(sessions.join("review-1.lock"), lock).unwrap();
        let stale = resolve_in(&conn(None, Some("review-1")), None, None, &sessions).unwrap_err();
        assert!(stale.contains("is not running (stale lock"), "{stale}");
        let from_env = resolve_in(&conn(None, None), None, Some("review-1".into()), &sessions).unwrap_err();
        assert!(from_env.contains("stale lock"), "{from_env}");
        let _ = std::fs::remove_dir_all(d);
    }

    fn launch(session: &str) -> Launch {
        Launch {
            session: session.into(),
            repos: vec![RepoLaunch {
                dir: PathBuf::from("C:/src/server"),
                base: "origin/development".into(),
                target: "HEAD".into(),
                merge_base: true,
                include_untracked: false,
            }],
            ignore_whitespace: true,
            context: Some(5),
            ..Launch::default()
        }
    }

    #[test]
    fn last_opens_the_newest_session_as_it_was_opened() {
        let d = temp("last");
        let o = OpenArgs {
            last: true,
            background: true,
            goto: Some("a.cs:3".into()),
            ..OpenArgs::default()
        };
        assert!(last_open(&o, &d).unwrap_err().starts_with("No saved session"));
        let now = SystemTime::now();
        for (name, age) in [("older", 120), ("newest", 1), ("old", 60)] {
            let p = d.join("sessions").join(format!("{name}.json"));
            std::fs::write(&p, "{}").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(now - Duration::from_secs(age))
                .unwrap();
            write_record(&d, &launch(name));
        }
        let o2 = last_open(&o, &d).unwrap();
        assert_eq!(o2.session.as_deref(), Some("newest"));
        assert_eq!(o2.repos, [PathBuf::from("C:/src/server")]);
        assert_eq!(
            (o2.target.as_deref(), o2.base.as_deref()),
            (Some("HEAD"), Some("origin/development"))
        );
        assert!(o2.merge_base && o2.ignores_whitespace() && !o2.last);
        assert_eq!(o2.context, 5);
        assert!(o2.background && o2.goto.as_deref() == Some("a.cs:3"), "typed flags stay");

        std::fs::remove_file(record_path(&d, "newest")).unwrap();
        assert!(last_open(&o, &d).unwrap_err().contains("launch records"));
        let _ = std::fs::remove_dir_all(d);
    }
}
