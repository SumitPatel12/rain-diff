//! Runs the answering command: argv from a template (no shell), the prompt on stdin, the answer
//! on stdout, stderr kept in a log file, a timeout, and cancel that ends the whole process tree.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::thread;
use std::time::{Duration, Instant};

/// What `{repos}`, `{port}` and `{session}` stand for in the command template.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Subs {
    /// Every repo's top folder; an argument that is exactly `{repos}` becomes one per folder.
    pub repos: Vec<String>,
    pub port: Option<u16>,
    pub session: String,
}

/// The template split into arguments (double quotes group and are dropped) with the
/// placeholders filled in.
pub fn argv(template: &str, subs: &Subs) -> Vec<String> {
    let mut words = Vec::new();
    let (mut cur, mut quoted, mut any) = (String::new(), false, false);
    for c in template.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        words.push(cur);
    }
    let mut out = Vec::new();
    for w in words {
        if w.contains("{repos}") {
            // One argument per repo (`--add-dir={repos}` too), never one joined path.
            out.extend(subs.repos.iter().map(|r| fill(&w, subs, r)));
        } else {
            out.push(fill(&w, subs, ""));
        }
    }
    out
}

/// `word` with each placeholder filled in one pass, so a repo path holding `{port}` stays as is.
fn fill(word: &str, subs: &Subs, repo: &str) -> String {
    let port = subs.port.map(|p| p.to_string()).unwrap_or_default();
    let mut out = String::with_capacity(word.len());
    let mut rest = word;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let (value, len) = [("{repos}", repo), ("{port}", port.as_str()), ("{session}", subs.session.as_str())]
            .into_iter()
            .find(|(p, _)| rest.starts_with(p))
            .map_or(("{", 1), |(p, v)| (v, p.len()));
        out.push_str(value);
        rest = &rest[len..];
    }
    out.push_str(rest);
    out
}

pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// One run of the answering command.
#[derive(Clone, Default)]
pub struct Request {
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub stdin: String,
    pub timeout: Duration,
    /// Where stderr is written when the run ends.
    pub log: Option<PathBuf>,
    /// Called on the runner's thread once the outcome is ready.
    pub on_done: Option<Wake>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub cancelled: bool,
}

impl Outcome {
    /// Exit 0 with something on stdout.
    pub fn answered(&self) -> bool {
        self.code == Some(0) && !self.timed_out && !self.cancelled && !self.stdout.trim().is_empty()
    }

    /// `exit N: <first 200 chars of stderr>` for a failed run.
    pub fn error(&self) -> String {
        let code = self.code.map_or_else(|| "killed".to_owned(), |c| format!("exit {c}"));
        let head: String = self.stderr.trim().chars().take(200).collect();
        match (head.is_empty(), self.code) {
            (true, Some(0)) => "exit 0 with an empty answer".to_owned(),
            (true, _) => code,
            (false, _) => format!("{code}: {head}"),
        }
    }
}

/// A running answer the window polls; tests stand in their own.
pub trait Job {
    /// The outcome once the run has ended; `None` while it runs.
    fn try_wait(&mut self) -> Option<Outcome>;
    /// Ends the run (and its children); the outcome follows with `cancelled` set.
    fn cancel(&mut self);
    fn pid(&self) -> Option<u32>;
}

pub struct Handle {
    pid: u32,
    cancel: Arc<AtomicBool>,
    rx: Receiver<Outcome>,
    done: Option<Outcome>,
}

impl Handle {
    /// Blocks until the run ends.
    pub fn wait(mut self) -> Outcome {
        if let Some(o) = self.done.take() {
            return o;
        }
        self.rx.recv().unwrap_or_else(|_| runner_died())
    }
}

/// The outcome when the thread waiting on the run ended without sending one.
fn runner_died() -> Outcome {
    Outcome {
        stderr: "the runner stopped without an outcome".into(),
        ..Outcome::default()
    }
}

impl Job for Handle {
    fn try_wait(&mut self) -> Option<Outcome> {
        if self.done.is_none() {
            self.done = match self.rx.try_recv() {
                Ok(o) => Some(o),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(runner_died()),
            };
        }
        self.done.clone()
    }

    fn cancel(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    fn pid(&self) -> Option<u32> {
        Some(self.pid)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if self.done.is_none() {
            self.cancel.store(true, Ordering::SeqCst);
        }
    }
}

/// How often the waiter looks at the child, the cancel flag and the clock.
const TICK: Duration = Duration::from_millis(15);

/// Starts `req.argv` with the prompt on stdin. The program is looked up on `PATH` (with
/// `PATHEXT` on Windows) and started directly, never through a shell, with no console window.
pub fn spawn(req: Request) -> io::Result<Handle> {
    let Some(prog) = req.argv.first() else {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "the ask command is empty"));
    };
    let exe = resolve(prog).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("{prog} is not on PATH")))?;
    let mut cmd = Command::new(exe);
    cmd.args(&req.argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .envs(req.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    if let Some(d) = &req.cwd {
        cmd.current_dir(d);
    }
    // cmd (an npm `.cmd` shim) then never runs a `node` from its current folder.
    #[cfg(windows)]
    cmd.env("NoDefaultCurrentDirectoryInExePath", "1");
    own_tree(&mut cmd);
    let mut child = cmd.spawn()?;
    let tree = match Tree::of(&mut child) {
        Ok(t) => t,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
    };
    let pid = child.id();
    let stdin = child.stdin.take();
    let input = req.stdin.clone();
    thread::Builder::new().name("rain-ask-stdin".into()).spawn(move || {
        if let Some(mut w) = stdin {
            let _ = w.write_all(input.as_bytes());
        }
    })?;
    let out = reader(child.stdout.take());
    let err = reader(child.stderr.take());
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let (tx, rx) = channel();
    thread::Builder::new().name("rain-ask-wait".into()).spawn(move || {
        let deadline = Instant::now() + req.timeout;
        let (code, timed_out, cancelled) = watch(&mut child, &tree, &flag, deadline);
        drop(tree);
        let take = |r: Receiver<Vec<u8>>| String::from_utf8_lossy(&r.recv_timeout(Duration::from_secs(2)).unwrap_or_default()).into_owned();
        let outcome = Outcome {
            code,
            stdout: take(out),
            stderr: take(err),
            timed_out,
            cancelled,
        };
        if let Some(p) = &req.log {
            write_log(p, &req.argv, &outcome);
        }
        let _ = tx.send(outcome);
        if let Some(w) = &req.on_done {
            w();
        }
    })?;
    Ok(Handle {
        pid,
        cancel,
        rx,
        done: None,
    })
}

/// Waits for `child` to exit, ending it at `deadline` or when `cancel` is set:
/// `(exit code, timed out, cancelled)`.
fn watch(child: &mut Child, tree: &Tree, cancel: &AtomicBool, deadline: Instant) -> (Option<i32>, bool, bool) {
    loop {
        if let Ok(Some(st)) = child.try_wait() {
            return (st.code(), false, false);
        }
        let cancelled = cancel.load(Ordering::SeqCst);
        if cancelled || Instant::now() >= deadline {
            tree.kill(child);
            let _ = child.wait();
            return (None, !cancelled, cancelled);
        }
        thread::sleep(TICK);
    }
}

fn reader<R: Read + Send + 'static>(src: Option<R>) -> Receiver<Vec<u8>> {
    let (tx, rx) = channel();
    let _ = thread::Builder::new().name("rain-ask-read".into()).spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut r) = src {
            let _ = r.read_to_end(&mut buf);
        }
        let _ = tx.send(buf);
    });
    rx
}

fn write_log(path: &Path, argv: &[String], o: &Outcome) {
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let status = match (o.code, o.timed_out, o.cancelled) {
        (_, true, _) => "timed out".to_owned(),
        (_, _, true) => "cancelled".to_owned(),
        (Some(c), ..) => format!("exit {c}"),
        (None, ..) => "killed".to_owned(),
    };
    let program = argv.first().map_or("", String::as_str);
    let _ = std::fs::write(path, format!("{program}: {status}\n{}", o.stderr));
}

/// Windows: no console window, and the child starts suspended so it joins its job before it can
/// start anything. Elsewhere: the child leads a process group of its own.
#[cfg(windows)]
fn own_tree(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_SUSPENDED: u32 = 0x0000_0004;
    cmd.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);
}

#[cfg(unix)]
fn own_tree(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

#[cfg(not(any(windows, unix)))]
fn own_tree(_: &mut Command) {}

/// The child and everything it starts (a `.cmd` launcher's node process included). On Windows
/// a job object that kills its processes when closed, so they also end if Rain exits; elsewhere
/// the child's process group.
struct Tree {
    #[cfg(windows)]
    job: Option<job::Job>,
    #[cfg(unix)]
    group: i32,
}

impl Tree {
    /// Windows: puts the suspended child in a job, then lets it run.
    fn of(child: &mut Child) -> io::Result<Tree> {
        #[cfg(windows)]
        {
            let job = job::Job::for_child(child);
            job::resume(child.id())?;
            Ok(Tree { job })
        }
        #[cfg(unix)]
        {
            Ok(Tree {
                group: i32::try_from(child.id()).unwrap_or(0),
            })
        }
        #[cfg(not(any(windows, unix)))]
        {
            let _ = child;
            Ok(Tree {})
        }
    }

    fn kill(&self, child: &mut Child) {
        #[cfg(windows)]
        if let Some(j) = &self.job {
            j.terminate();
        }
        #[cfg(unix)]
        self.kill_group();
        let _ = child.kill();
    }

    #[cfg(unix)]
    fn kill_group(&self) {
        if self.group > 0 {
            unsafe extern "C" {
                fn kill(pid: i32, sig: i32) -> i32;
            }
            const SIGKILL: i32 = 9;
            // SAFETY: signals the process group this runner started; a negative pid names the group.
            unsafe {
                kill(-self.group, SIGKILL);
            }
        }
    }
}

#[cfg(unix)]
impl Drop for Tree {
    fn drop(&mut self) {
        // Like the Windows kill-on-close job, close inherited pipes even when the launcher
        // exited successfully; otherwise descendants outlive the answer and its readers.
        self.kill_group();
    }
}

#[cfg(windows)]
mod job {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

    pub struct Job(HANDLE);

    // The handle is only used to terminate and close the job, both thread-safe calls.
    unsafe impl Send for Job {}

    /// Resumes the threads of the suspended process `pid` (a new process has one).
    pub fn resume(pid: u32) -> std::io::Result<()> {
        // SAFETY: a thread snapshot walked with a correctly sized entry; every handle opened here
        // is checked and closed.
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snap == INVALID_HANDLE_VALUE {
                return Err(std::io::Error::last_os_error());
            }
            let mut e: THREADENTRY32 = std::mem::zeroed();
            e.dwSize = size_of::<THREADENTRY32>() as u32;
            let mut resumed = 0;
            let mut more = Thread32First(snap, &mut e) != 0;
            while more {
                if e.th32OwnerProcessID == pid {
                    let t = OpenThread(THREAD_SUSPEND_RESUME, 0, e.th32ThreadID);
                    if !t.is_null() {
                        if ResumeThread(t) != u32::MAX {
                            resumed += 1;
                        }
                        CloseHandle(t);
                    }
                }
                more = Thread32Next(snap, &mut e) != 0;
            }
            CloseHandle(snap);
            if resumed == 0 {
                return Err(std::io::Error::other("could not start the ask command's main thread"));
            }
            Ok(())
        }
    }

    impl Job {
        /// A kill-on-close job holding `child`, which must not have started anything yet.
        pub fn for_child(child: &Child) -> Option<Job> {
            // SAFETY: plain Win32 calls on a handle this function owns and the child's live handle.
            unsafe {
                let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if h.is_null() {
                    return None;
                }
                let job = Job(h);
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    (&raw const info).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 || AssignProcessToJobObject(h, child.as_raw_handle() as HANDLE) == 0 {
                    return None;
                }
                Some(job)
            }
        }

        pub fn terminate(&self) {
            // SAFETY: `self.0` is a job handle owned by `self`.
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: closing the handle this value owns, once.
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

/// `prog` as a file to run: a path is taken as given (trying `PATHEXT` when it has no
/// extension on Windows), a bare name is searched for on `PATH`.
fn resolve(prog: &str) -> Option<PathBuf> {
    if prog.is_empty() {
        return None;
    }
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
            .split(';')
            .filter(|e| !e.is_empty())
            .map(str::to_owned)
            .collect()
    } else {
        Vec::new()
    };
    let pick = |base: &Path| -> Option<PathBuf> {
        if base.is_file() && (base.extension().is_some() || !cfg!(windows)) {
            return Some(base.to_path_buf());
        }
        exts.iter().find_map(|e| {
            let mut s = base.as_os_str().to_owned();
            s.push(e);
            let c = PathBuf::from(s);
            c.is_file().then_some(c)
        })
    };
    let p = Path::new(prog);
    if p.is_absolute() || p.components().count() > 1 {
        return pick(p);
    }
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|d| pick(&d.join(p)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_fill_placeholders_and_spread_repos() {
        let subs = Subs {
            repos: vec![r"C:\r\server".into(), r"C:\my repos\app".into()],
            port: Some(4123),
            session: "ACME-1".into(),
        };
        assert_eq!(
            argv(
                "claude -p --output-format text --allowedTools Read,Grep,Glob --add-dir {repos}",
                &subs
            ),
            [
                "claude",
                "-p",
                "--output-format",
                "text",
                "--allowedTools",
                "Read,Grep,Glob",
                "--add-dir",
                r"C:\r\server",
                r"C:\my repos\app"
            ]
        );
        assert_eq!(
            argv(r#""C:\Program Files\a.exe" --port={port} --s "{session}""#, &subs),
            [r"C:\Program Files\a.exe", "--port=4123", "--s", "ACME-1"]
        );
        assert!(argv("   ", &subs).is_empty());
    }

    #[test]
    fn repos_inside_a_word_spread_and_repo_paths_keep_braces() {
        let subs = Subs {
            repos: vec![r"C:\r\{port}".into(), r"C:\r\{session}x".into()],
            port: Some(4123),
            session: "ACME-1".into(),
        };
        assert_eq!(
            argv("tool --add-dir={repos} --p={port} {x}", &subs),
            ["tool", r"--add-dir=C:\r\{port}", r"--add-dir=C:\r\{session}x", "--p=4123", "{x}"]
        );
        let none = Subs { repos: Vec::new(), ..subs };
        assert_eq!(argv("tool --add-dir={repos}", &none), ["tool"]);
    }

    #[test]
    fn a_wait_thread_that_dies_reads_as_a_failed_run() {
        let (tx, rx) = channel::<Outcome>();
        drop(tx);
        let mut h = Handle {
            pid: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            rx,
            done: None,
        };
        let o = h.try_wait().expect("a dead runner ends the job");
        assert!(!o.answered());
        assert!(o.error().contains("runner stopped"), "{}", o.error());
    }

    #[test]
    fn errors_read_as_exit_and_stderr_head() {
        let o = Outcome {
            code: Some(2),
            stderr: format!("boom {}", "x".repeat(400)),
            ..Outcome::default()
        };
        assert!(o.error().starts_with("exit 2: boom"));
        assert_eq!(o.error().chars().count(), "exit 2: ".len() + 200);
        assert!(!o.answered());
        let empty = Outcome {
            code: Some(0),
            ..Outcome::default()
        };
        assert!(!empty.answered());
        assert_eq!(empty.error(), "exit 0 with an empty answer");
    }

    #[test]
    fn a_missing_program_is_an_error_not_a_shell_fallback() {
        let e = spawn(Request {
            argv: vec!["rain-no-such-program-xyz".into()],
            timeout: Duration::from_secs(1),
            ..Request::default()
        })
        .err()
        .unwrap();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
    }
}
