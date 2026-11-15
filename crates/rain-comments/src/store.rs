//! The data folder and the session file on disk: `%LOCALAPPDATA%\Rain\sessions\<id>.json`,
//! written atomically, plus a debounced background saver for the shell.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use rain_core::{Session, Shared};

const DATA_DIR: &str = "Rain";
const LEGACY_DATA_DIR: &str = "redline";

/// `%LOCALAPPDATA%\Rain` (the temp dir when `LOCALAPPDATA` is unset), first copied over from the
/// pre-rename `%LOCALAPPDATA%\redline` when only that one exists.
pub fn data_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let base = std::env::var_os("LOCALAPPDATA").map_or_else(std::env::temp_dir, PathBuf::from);
        if let Err(e) = migrate_legacy(&base) {
            eprintln!("Rain: copying {} failed: {e}", base.join(LEGACY_DATA_DIR).display());
        }
        base.join(DATA_DIR)
    })
    .clone()
}

/// Copies `base\redline` to `base\Rain` when only the old folder exists; the old one is left in
/// place. Returns whether it copied. A failed copy leaves no `Rain` folder behind.
pub fn migrate_legacy(base: &Path) -> io::Result<bool> {
    let (old, new) = (base.join(LEGACY_DATA_DIR), base.join(DATA_DIR));
    if new.exists() || !old.is_dir() {
        return Ok(false);
    }
    let staging = base.join(format!("{DATA_DIR}.migrating-{}", std::process::id()));
    let copied = copy_dir(&old, &staging).and_then(|()| std::fs::rename(&staging, &new));
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

    /// Writes pretty JSON next to `path` and renames it over, so a crash never leaves half a file.
    pub fn save(path: &Path, session: &Session) -> io::Result<()> {
        let bytes = serde_json::to_vec_pretty(session).map_err(io::Error::other)?;
        write_atomic(path, &bytes)
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}-{}.tmp", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
    let tmp = PathBuf::from(tmp);
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
    Flush(Sender<()>),
}

/// Saves the shared session to its file `debounce` after the last [`Autosave::touch`] (at most
/// five debounces after the first unsaved one), on its own thread. Dropping it saves any pending
/// change.
pub struct Autosave {
    tx: Option<Sender<Msg>>,
    worker: Option<JoinHandle<()>>,
}

/// Starts a debounced saver of `shared` to `path`. Call [`Autosave::touch`] after each change.
pub fn autosave(shared: Arc<Shared>, path: PathBuf, debounce: Duration) -> Autosave {
    let (tx, rx) = mpsc::channel::<Msg>();
    let worker = std::thread::Builder::new()
        .name("rain-autosave".into())
        .spawn(move || {
            // Held through the rename: the HTTP routes save under the same lock, so an older
            // snapshot can never land on disk after a newer one.
            let save = || {
                let s = shared.lock();
                if let Ok(bytes) = serde_json::to_vec_pretty(&*s)
                    && let Err(e) = write_atomic(&path, &bytes)
                {
                    eprintln!("Rain: saving {} failed: {e}", path.display());
                }
            };
            let max_wait = debounce * 5;
            let mut dirty_since: Option<Instant> = None;
            loop {
                let msg = match dirty_since {
                    Some(t) => rx.recv_timeout(debounce.min(max_wait.saturating_sub(t.elapsed()))),
                    None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                };
                match msg {
                    Ok(Msg::Touch) => {
                        dirty_since.get_or_insert_with(Instant::now);
                    }
                    Ok(Msg::Flush(done)) => {
                        if dirty_since.take().is_some() {
                            save();
                        }
                        let _ = done.send(());
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        save();
                        dirty_since = None;
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        if dirty_since.is_some() {
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
    pub fn touch(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Touch);
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
