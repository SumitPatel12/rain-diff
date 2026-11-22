//! The data folder and the session file on disk: `%LOCALAPPDATA%\Rain\sessions\<id>.json`,
//! written atomically, plus a debounced background saver for the shell.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rain_core::{Session, Shared};

const DATA_DIR: &str = "Rain";
const LEGACY_DATA_DIR: &str = "redline";

/// `%LOCALAPPDATA%\Rain` on Windows, `~/Library/Application Support/Rain` on macOS, the XDG data
/// folder elsewhere; first copied over from the pre-rename `redline` folder, or on macOS and Linux
/// from the temp-dir `Rain` older builds used, when only that one exists.
pub fn data_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let base = data_base(std::env::consts::OS, |k| std::env::var_os(k));
        if let Err(e) = migrate_legacy(&base) {
            eprintln!("Rain: copying {} failed: {e}", base.join(LEGACY_DATA_DIR).display());
        }
        let temp = std::env::temp_dir().join(DATA_DIR);
        if base.join(DATA_DIR) != temp
            && cfg!(unix)
            && let Err(e) = copy_once(&temp, &base.join(DATA_DIR))
        {
            eprintln!("Rain: copying {} failed: {e}", temp.display());
        }
        base.join(DATA_DIR)
    })
    .clone()
}

/// The folder Rain's folder goes in. The temp dir is the last resort: macOS purges it.
fn data_base(os: &str, var: impl Fn(&str) -> Option<OsString>) -> PathBuf {
    let path = |k: &str| var(k).map(PathBuf::from).filter(|p| p.is_absolute());
    if let Some(local) = var("LOCALAPPDATA") {
        return local.into();
    }
    let home = path("HOME");
    match os {
        "windows" => None,
        "macos" => home.map(|h| h.join("Library").join("Application Support")),
        _ => path("XDG_DATA_HOME").or_else(|| home.map(|h| h.join(".local").join("share"))),
    }
    .unwrap_or_else(std::env::temp_dir)
}

/// Copies `base\redline` to `base\Rain` when only the old folder exists; the old one is left in
/// place. Returns whether it copied. A failed copy leaves no `Rain` folder behind.
pub fn migrate_legacy(base: &Path) -> io::Result<bool> {
    copy_once(&base.join(LEGACY_DATA_DIR), &base.join(DATA_DIR))
}

/// Copies the folder `old` to `new` when only `old` exists, through a staging folder.
fn copy_once(old: &Path, new: &Path) -> io::Result<bool> {
    if new.exists() || !old.is_dir() {
        return Ok(false);
    }
    let staging = new.with_file_name(format!("{DATA_DIR}.migrating-{}", std::process::id()));
    let copied = copy_dir(old, &staging).and_then(|()| std::fs::rename(&staging, new));
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&staging);
        // Another process may have migrated first.
        return if new.exists() { Ok(false) } else { Err(e) };
    }
    Ok(true)
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let dest = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

pub struct SessionFile;

impl SessionFile {
    /// `<data_dir>\sessions`.
    pub fn dir() -> PathBuf {
        data_dir().join("sessions")
    }

    pub fn path(session_id: &str) -> PathBuf {
        Self::path_in(&Self::dir(), session_id)
    }

    pub fn path_in(dir: &Path, session_id: &str) -> PathBuf {
        dir.join(format!("{}.json", sanitize(session_id)))
    }

    /// The session at `path`; a missing file is an empty session.
    pub fn load(path: &Path) -> io::Result<Session> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Session::default()),
            Err(e) => Err(e),
        }
    }

    /// Where the session the last save replaced is kept: `<id>.json.bak`, one generation.
    pub fn backup_path(path: &Path) -> PathBuf {
        let mut bak = path.as_os_str().to_owned();
        bak.push(".bak");
        bak.into()
    }

    /// The session the last save replaced, when that copy is there and readable.
    pub fn load_backup(path: &Path) -> Option<Session> {
        serde_json::from_slice(&std::fs::read(Self::backup_path(path)).ok()?).ok()
    }

    /// [`SessionFile::load`], or [`SessionFile::load_backup`] when the file is there but is not a
    /// session; `true` says the backup was used. Nothing on disk changes.
    pub fn load_or_backup(path: &Path) -> io::Result<(Session, bool)> {
        match Self::load(path) {
            Ok(s) => Ok((s, false)),
            Err(e) if e.kind() == io::ErrorKind::InvalidData => Self::load_backup(path).map(|s| (s, true)).ok_or(e),
            Err(e) => Err(e),
        }
    }

    /// Moves an unreadable session file out of the way as `<id>.json.corrupt` (with a timestamp
    /// when that name is taken) and returns where it went.
    pub fn set_aside(path: &Path) -> io::Result<PathBuf> {
        let mut kept = path.with_extension("json.corrupt");
        if kept.exists() {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis());
            kept = path.with_extension(format!("json.{ms}.corrupt"));
        }
        std::fs::rename(path, &kept)?;
        Ok(kept)
    }

    /// What the window says after opening the backup of an unreadable session file.
    pub fn backup_notice(path: &Path, kept: Option<&Path>) -> String {
        let name = |p: &Path| p.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let mut notice = format!("{} was unreadable: opened the save before it", name(path));
        if let Some(kept) = kept {
            notice.push_str(&format!("; the unreadable file is kept as {}", name(kept)));
        }
        notice
    }

    /// Writes compact JSON next to `path` and renames it over, so a crash never leaves half a file.
    pub fn save(path: &Path, session: &Session) -> io::Result<()> {
        let bytes = serde_json::to_vec(session).map_err(io::Error::other)?;
        keep_backup(path);
        write_atomic(path, &bytes)
    }

    /// Serialises `session` for [`Snapshot::write`] after its lock is let go. Take it under the
    /// session's lock: snapshots of one path land in the order they were taken.
    pub fn snapshot(path: &Path, session: &Session) -> io::Result<Snapshot> {
        static TAKEN: AtomicU64 = AtomicU64::new(1);
        Ok(Snapshot {
            path: path.to_owned(),
            seq: TAKEN.fetch_add(1, Ordering::SeqCst),
            bytes: serde_json::to_vec(session).map_err(io::Error::other)?,
        })
    }
}

/// A session serialised under its lock, written without holding it.
#[must_use = "a snapshot saves nothing until written"]
pub struct Snapshot {
    path: PathBuf,
    seq: u64,
    bytes: Vec<u8>,
}

impl Snapshot {
    /// Writes the snapshot unless a newer one of the same path already landed (it holds
    /// everything this one does).
    pub fn write(self) -> io::Result<()> {
        static WRITTEN: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<u64>>>>> = OnceLock::new();
        let slot = {
            let mut paths = WRITTEN.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner());
            paths.entry(self.path.clone()).or_default().clone()
        };
        let mut last = slot.lock().unwrap_or_else(|e| e.into_inner());
        if self.seq < *last {
            return Ok(());
        }
        keep_backup(&self.path);
        write_atomic(&self.path, &self.bytes)?;
        *last = self.seq;
        Ok(())
    }
}

/// A name beside `path` no other writer uses.
fn temp_beside(path: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}-{}.tmp", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    tmp.into()
}

/// Gives the session file at `path` a second name, [`SessionFile::backup_path`], just before a
/// save replaces it: a hard link, so no bytes are written. A save never fails for its backup.
fn keep_backup(path: &Path) {
    let tmp = temp_beside(path);
    let kept = std::fs::hard_link(path, &tmp).and_then(|()| std::fs::rename(&tmp, SessionFile::backup_path(path)));
    // A rename onto another name of the same file does nothing and leaves both.
    if kept.is_err() || cfg!(unix) {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Writes `bytes` to a flushed temp file beside `path` and renames it over `path`.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = temp_beside(path);
    // Flushed before the rename: NTFS journals the rename, not the data, so a power cut could
    // otherwise leave the session file empty.
    let written = std::fs::File::create(&tmp).and_then(|mut f| {
        std::io::Write::write_all(&mut f, bytes)?;
        f.sync_all()
    });
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    // Windows briefly locks a just-written file (antivirus, indexer) or a target another saver is replacing.
    let mut tries = 0;
    loop {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) if tries < 20 && matches!(e.raw_os_error(), Some(5 | 32)) => {
                tries += 1;
                std::thread::sleep(Duration::from_millis(10 * tries));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(e);
            }
        }
    }
}

/// File-name-safe session id: `[A-Za-z0-9._-]` kept, anything else `_`, empty as `default`.
pub fn sanitize(id: &str) -> String {
    let s: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim_matches('.');
    if s.is_empty() { "default".to_owned() } else { s.to_owned() }
}

enum Msg {
    Touch,
    /// A change that can wait [`Autosave::LATER`] (the reading place).
    TouchLater,
    Flush(Sender<()>),
}

/// Saves the shared session to its file `debounce` after the last [`Autosave::touch`] (at most
/// five debounces after the first unsaved one), on its own thread. Dropping it saves any pending
/// change.
pub struct Autosave {
    tx: Option<Sender<Msg>>,
    worker: Option<JoinHandle<()>>,
}

/// When the next save is due, from what is pending: `debounce` after the last touch, at most
/// `debounce * 5` after the first; a change that can wait at most [`Autosave::LATER`] after it.
#[derive(Default)]
struct Due {
    dirty_since: Option<Instant>,
    touched: Option<Instant>,
    later: Option<Instant>,
}

impl Due {
    fn at(&self, debounce: Duration) -> Option<Instant> {
        let soon = self
            .touched
            .zip(self.dirty_since)
            .map(|(t, first)| (t + debounce).min(first + debounce * 5));
        match (soon, self.later) {
            (Some(a), Some(b)) => Some(a.min(b + Autosave::LATER)),
            (a, b) => a.or(b.map(|b| b + Autosave::LATER)),
        }
    }
}

/// Starts a debounced saver of `shared` to `path`. Call [`Autosave::touch`] after each change.
pub fn autosave(shared: Arc<Shared>, path: PathBuf, debounce: Duration) -> Autosave {
    let (tx, rx) = mpsc::channel::<Msg>();
    let worker = std::thread::Builder::new()
        .name("rain-autosave".into())
        .spawn(move || {
            // Serialised under the lock and written after it, so the window never waits on the
            // disk; snapshot order keeps an older one from landing after a newer one.
            let save = || {
                let snap = SessionFile::snapshot(&path, &shared.lock());
                if let Err(e) = snap.and_then(Snapshot::write) {
                    eprintln!("Rain: saving {} failed: {e}", path.display());
                }
            };
            let mut due = Due::default();
            loop {
                let msg = match due.at(debounce) {
                    Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
                    None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                };
                let pending = due.at(debounce).is_some();
                match msg {
                    Ok(Msg::Touch) => {
                        let now = Instant::now();
                        due.dirty_since.get_or_insert(now);
                        due.touched = Some(now);
                    }
                    Ok(Msg::TouchLater) => {
                        due.later.get_or_insert_with(Instant::now);
                    }
                    Ok(Msg::Flush(done)) => {
                        if pending {
                            save();
                        }
                        due = Due::default();
                        let _ = done.send(());
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        save();
                        due = Due::default();
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        if pending {
                            save();
                        }
                        return;
                    }
                }
            }
        })
        .ok();
    Autosave { tx: Some(tx), worker }
}

impl Autosave {
    /// The longest a [`Autosave::touch_later`] change waits for its save.
    pub const LATER: Duration = Duration::from_secs(2);

    pub fn touch(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Touch);
        }
    }

    /// A change that can ride along with the next save, or wait [`Autosave::LATER`] for its own:
    /// moving through the code saves the place once every couple of seconds at most.
    pub fn touch_later(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::TouchLater);
        }
    }

    /// Saves now if anything is pending, and waits for the write.
    pub fn flush(&self) {
        let (done, wait) = mpsc::channel();
        if let Some(tx) = &self.tx
            && tx.send(Msg::Flush(done)).is_ok()
        {
            let _ = wait.recv();
        }
    }
}

impl Drop for Autosave {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&str, &str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |k| vars.iter().find(|(n, _)| *n == k).map(|(_, v)| OsString::from(v))
    }

    #[test]
    fn data_base_is_never_the_temp_dir_on_macos() {
        let home = if cfg!(windows) { "C:/Users/me" } else { "/Users/me" };
        assert_eq!(
            data_base("macos", env(&[("HOME", home)])),
            Path::new(home).join("Library").join("Application Support")
        );
        assert_eq!(
            data_base("windows", env(&[("LOCALAPPDATA", "C:/x"), ("HOME", home)])),
            Path::new("C:/x")
        );
        assert_eq!(data_base("windows", env(&[("HOME", home)])), std::env::temp_dir());
        assert_eq!(
            data_base("linux", env(&[("HOME", home)])),
            Path::new(home).join(".local").join("share")
        );
        assert_eq!(data_base("macos", env(&[])), std::env::temp_dir());
    }

    #[test]
    fn copy_once_copies_only_when_the_new_folder_is_missing() {
        let root = std::env::temp_dir().join(format!("rain-copy-once-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (old, new) = (root.join("tmp").join("Rain"), root.join("support").join("Rain"));
        std::fs::create_dir_all(old.join("sessions")).unwrap();
        std::fs::write(old.join("sessions").join("a.json"), "{}").unwrap();
        assert!(copy_once(&old, &new).unwrap());
        assert_eq!(std::fs::read_to_string(new.join("sessions").join("a.json")).unwrap(), "{}");
        assert!(!copy_once(&old, &new).unwrap(), "an existing folder is left alone");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rain-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn version(n: u64) -> Session {
        Session {
            id: "s".into(),
            version: n,
            ..Session::default()
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_save_keeps_the_session_it_replaces_as_the_one_backup() {
        let dir = scratch("backup");
        let path = dir.join("s.json");
        SessionFile::save(&path, &version(1)).unwrap();
        assert_eq!(names(&dir), ["s.json"], "nothing to keep on the first save");
        assert_eq!(SessionFile::load_backup(&path), None);
        SessionFile::save(&path, &version(2)).unwrap();
        assert_eq!(SessionFile::load_backup(&path), Some(version(1)));
        SessionFile::snapshot(&path, &version(3)).unwrap().write().unwrap();
        assert_eq!(SessionFile::load_backup(&path), Some(version(2)), "a snapshot keeps one too");
        assert_eq!(SessionFile::load(&path).unwrap(), version(3));
        assert_eq!(names(&dir), ["s.json", "s.json.bak"], "one backup and no temp files");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_backup_is_a_second_name_for_the_old_file_not_a_second_write() {
        let dir = scratch("backup-link");
        let path = dir.join("s.json");
        std::fs::write(&path, b"old").unwrap();
        keep_backup(&path);
        let mut main = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        std::io::Write::write_all(&mut main, b" and more").unwrap();
        drop(main);
        assert_eq!(
            std::fs::read(SessionFile::backup_path(&path)).unwrap(),
            b"old and more",
            "one file under two names: keeping it copied no bytes"
        );
        assert_eq!(names(&dir), ["s.json", "s.json.bak"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_backup_that_already_names_the_session_file_leaves_no_litter() {
        let dir = scratch("backup-same");
        let path = dir.join("s.json");
        SessionFile::save(&path, &version(1)).unwrap();
        std::fs::hard_link(&path, SessionFile::backup_path(&path)).unwrap();
        SessionFile::save(&path, &version(2)).unwrap();
        assert_eq!(names(&dir), ["s.json", "s.json.bak"]);
        assert_eq!(SessionFile::load_backup(&path), Some(version(1)));
        assert_eq!(SessionFile::load(&path).unwrap(), version(2));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn an_unreadable_session_file_falls_back_to_its_backup() {
        let dir = scratch("backup-load");
        let path = dir.join("s.json");
        SessionFile::save(&path, &version(1)).unwrap();
        SessionFile::save(&path, &version(2)).unwrap();
        assert_eq!(SessionFile::load_or_backup(&path).unwrap(), (version(2), false));

        std::fs::write(&path, b"{\"threads\": [").unwrap();
        assert_eq!(SessionFile::load_or_backup(&path).unwrap(), (version(1), true));
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"threads\": [", "reading moves nothing");

        let kept = SessionFile::set_aside(&path).unwrap();
        assert_eq!(kept, dir.join("s.json.corrupt"));
        assert_eq!(
            SessionFile::backup_notice(&path, Some(&kept)),
            "s.json was unreadable: opened the save before it; the unreadable file is kept as s.json.corrupt"
        );
        SessionFile::save(&path, &version(1)).unwrap();
        assert_eq!(
            SessionFile::load_backup(&path),
            Some(version(1)),
            "the good backup outlives the recovery"
        );
        assert_eq!(names(&dir), ["s.json", "s.json.bak", "s.json.corrupt"]);

        std::fs::write(&path, b"not json").unwrap();
        std::fs::write(SessionFile::backup_path(&path), b"nor this").unwrap();
        let e = SessionFile::load_or_backup(&path).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData, "both unreadable: the error load gives today");

        std::fs::remove_file(&path).unwrap();
        std::fs::write(SessionFile::backup_path(&path), serde_json::to_vec(&version(1)).unwrap()).unwrap();
        assert_eq!(
            SessionFile::load_or_backup(&path).unwrap(),
            (Session::default(), false),
            "a missing file is an empty session, not its old backup"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
