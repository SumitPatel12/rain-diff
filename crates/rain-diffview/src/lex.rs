//! Syntax colours for the diff's lines, [`CHUNK`] lines of one side at a time. A chunk is lexed
//! from the state the chunk before it ended in, so a comment, string or template that runs over
//! a chunk's end keeps its colour; where the diff doesn't hold the line before, from a fresh one.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use rain_core::diff::Side;
use rune_kit::syntax::{Highlight, Paint};

/// What a [`Lexer`] was in the middle of at a line end (an open comment, string or template), as
/// the lexer made it; `None` at a fresh start.
pub type Carry = Option<Arc<dyn Any + Send + Sync>>;

/// Colours code that may start inside a construct earlier lines opened, where a
/// [`Highlighter`](rune_kit::syntax::Highlighter) always starts fresh.
pub trait Lexer: Send + Sync {
    /// `code`'s highlights (whole lines; ranges in order, apart and on character boundaries)
    /// when the lines before it left `from` open, and what `code` leaves open in turn.
    fn lex(&self, lang: Option<&str>, code: &str, from: &Carry) -> (Vec<Highlight>, Carry);

    /// Whether code lexed from `a` would come out the same from `b`.
    fn same(&self, a: &Carry, b: &Carry) -> bool;

    /// Whether `lang` costs milliseconds a chunk, not microseconds.
    fn slow(&self, lang: Option<&str>) -> bool;
}

pub(crate) const CHUNK: u32 = 128;
const CHUNK_CAP: usize = 512;
/// How many chunks back a fast lexer runs for a start state nothing holds, as after a jump far
/// down a file: 4096 lines. A construct open for longer than that starts the chunk fresh.
pub(crate) const LOOKBACK: u32 = 32;
/// The same for a slow lexer on the worker: 1024 lines. On the caller's thread it runs back none.
pub(crate) const SLOW_LOOKBACK: u32 = 8;
/// Jobs the worker has waiting, and chunks waiting on a job; past it the oldest is dropped.
const QUEUE_CAP: usize = 8;

/// Wakes the window from another thread.
pub(crate) type Wake = Arc<dyn Fn() + Send + Sync>;

pub(crate) type ChunkKey = (u32, Side, u32);

/// The text of one chunk: a line per line number, empty where the diff holds none.
pub(crate) struct ChunkText {
    text: String,
    /// Byte offset of each line in the text, and one past the last.
    starts: Vec<u32>,
    /// The lines the diff holds, as runs of line indexes: the lexer starts fresh on each.
    runs: Vec<Range<u32>>,
}

impl ChunkText {
    pub(crate) fn new<'a>(line: impl Fn(u32) -> Option<&'a str>) -> ChunkText {
        let mut t = ChunkText {
            text: String::new(),
            starts: Vec::with_capacity(CHUNK as usize + 1),
            runs: Vec::new(),
        };
        for k in 0..CHUNK {
            t.starts.push(t.text.len() as u32);
            if let Some(l) = line(k) {
                t.text.push_str(l);
                match t.runs.last_mut() {
                    Some(r) if r.end == k => r.end = k + 1,
                    _ => t.runs.push(k..k + 1),
                }
            }
            t.text.push('\n');
        }
        t.starts.push(t.text.len() as u32);
        t
    }

    /// Whether the diff holds the chunk's first line, so the chunk before it sets its state.
    pub(crate) fn leads(&self) -> bool {
        self.runs.first().is_some_and(|r| r.start == 0)
    }

    /// The chunk lexed from `from`, and the state its last line ends in.
    pub(crate) fn lex(self, from: Carry, lex: impl Fn(&str, &Carry) -> (Vec<Highlight>, Carry)) -> Chunk {
        let fresh: Carry = None;
        let mut out = Vec::new();
        let mut end = None;
        for run in &self.runs {
            let at = self.starts[run.start as usize] as usize;
            let code = &self.text[at..self.starts[run.end as usize] as usize];
            let (hl, e) = lex(code, if run.start == 0 { &from } else { &fresh });
            let mut last = 0;
            out.extend(
                hl.into_iter()
                    .filter(|(r, _)| {
                        let ok = r.start >= last
                            && r.start < r.end
                            && r.end <= code.len()
                            && code.is_char_boundary(r.start)
                            && code.is_char_boundary(r.end);
                        if ok {
                            last = r.end;
                        }
                        ok
                    })
                    .map(|(r, p)| (r.start + at..r.end + at, p)),
            );
            end = if run.end == CHUNK { e } else { None };
        }
        Chunk {
            leads: self.leads(),
            starts: self.starts,
            hl: out,
            from,
            end,
        }
    }
}

/// Highlights of [`CHUNK`] lines of one side, by line number.
pub(crate) struct Chunk {
    starts: Vec<u32>,
    hl: Vec<Highlight>,
    leads: bool,
    /// The state it was lexed from and the one it ends in.
    from: Carry,
    pub(crate) end: Carry,
}

pub(crate) struct Held {
    pub(crate) chunk: Chunk,
    /// Nonzero once its colours changed after rows could have drawn them.
    pub(crate) rev: Cell<u32>,
    /// Lexed from a state the chunk before it no longer ends in.
    pub(crate) stale: Cell<bool>,
}

impl Held {
    pub(crate) fn new(chunk: Chunk, rev: u32) -> Rc<Held> {
        Rc::new(Held {
            chunk,
            rev: Cell::new(rev),
            stale: Cell::new(false),
        })
    }
}

impl Chunk {
    /// The highlights of line `k` of the chunk, relative to the line.
    pub(crate) fn line(&self, k: usize) -> Vec<(Range<usize>, Paint)> {
        let (a, b) = (self.starts[k] as usize, self.starts[k + 1] as usize - 1);
        let first = self.hl.partition_point(|(r, _)| r.end <= a);
        self.hl[first..]
            .iter()
            .take_while(|(r, _)| r.start < b)
            .map(|(r, p)| (r.start.max(a) - a..r.end.min(b) - a, *p))
            .filter(|(r, _)| !r.is_empty())
            .collect()
    }

    /// Whether the chunk before it ending in `end` leaves this one's colours as they are.
    pub(crate) fn follows(&self, end: &Carry, same: impl Fn(&Carry, &Carry) -> bool) -> bool {
        !self.leads || same(&self.from, end)
    }
}

/// Highlighted chunks, oldest evicted first once [`CHUNK_CAP`] are held.
#[derive(Default)]
pub(crate) struct ChunkCache {
    map: HashMap<ChunkKey, Rc<Held>>,
    order: VecDeque<ChunkKey>,
}

impl ChunkCache {
    pub(crate) fn get(&self, k: &ChunkKey) -> Option<&Rc<Held>> {
        self.map.get(k)
    }

    pub(crate) fn insert(&mut self, k: ChunkKey, c: Rc<Held>) {
        if self.map.insert(k, c).is_none() {
            self.order.push_back(k);
        }
        while self.map.len() > CHUNK_CAP {
            let Some(old) = self.order.pop_front() else { break };
            self.map.remove(&old);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.map.clear();
        self.order.clear();
    }

    pub(crate) fn retain(&mut self, keep: impl Fn(&ChunkKey) -> bool) {
        self.map.retain(|k, _| keep(k));
        self.order.retain(|k| keep(k));
    }
}

/// Consecutive chunks of one side of a file for the worker, the first lexed from `from`.
struct Job {
    epoch: u64,
    first: ChunkKey,
    lang: Option<String>,
    texts: Vec<ChunkText>,
    from: Carry,
    lexer: Arc<dyn Lexer>,
}

pub(crate) struct Lexed {
    epoch: u64,
    pub(crate) first: ChunkKey,
    pub(crate) chunks: Vec<Chunk>,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
    running: bool,
}

fn locked(q: &Mutex<Queue>) -> std::sync::MutexGuard<'_, Queue> {
    q.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Lexes a slow language's chunks off the caller's thread, the newest job first, on a thread
/// that lives while there are jobs. The caller draws a chunk plain until [`Worker::done`] has it.
pub(crate) struct Worker {
    wake: Wake,
    queue: Arc<Mutex<Queue>>,
    tx: Sender<Lexed>,
    rx: Receiver<Lexed>,
    /// Changes when the files or the lexer do, so a result from before is dropped.
    epoch: u64,
    /// Chunks in a job that is not back yet.
    asked: RefCell<HashSet<ChunkKey>>,
    /// Chunks that start from one in `asked`, to ask for once it is back.
    waiting: RefCell<VecDeque<ChunkKey>>,
}

impl Worker {
    pub(crate) fn new(wake: Wake) -> Worker {
        let (tx, rx) = channel();
        Worker {
            wake,
            queue: Arc::default(),
            tx,
            rx,
            epoch: 0,
            asked: RefCell::default(),
            waiting: RefCell::default(),
        }
    }

    /// Whether chunk `k` is on its way, or waits for one that is.
    pub(crate) fn has(&self, k: &ChunkKey) -> bool {
        self.asked.borrow().contains(k) || self.waiting.borrow().contains(k)
    }

    pub(crate) fn asked(&self, k: &ChunkKey) -> bool {
        self.asked.borrow().contains(k)
    }

    #[cfg(test)]
    pub(crate) fn idle(&self) -> bool {
        self.asked.borrow().is_empty() && self.waiting.borrow().is_empty()
    }

    /// Queues `texts`, the chunks from `first` on. False when that pushed the oldest job out:
    /// its chunks are no longer on their way.
    pub(crate) fn push(&self, first: ChunkKey, lang: Option<&str>, texts: Vec<ChunkText>, from: Carry, lexer: Arc<dyn Lexer>) -> bool {
        let mut asked = self.asked.borrow_mut();
        asked.extend((first.2..).take(texts.len()).map(|c| (first.0, first.1, c)));
        let job = Job {
            epoch: self.epoch,
            first,
            lang: lang.map(str::to_owned),
            texts,
            from,
            lexer,
        };
        let mut q = locked(&self.queue);
        q.jobs.push_back(job);
        let dropped = if q.jobs.len() > QUEUE_CAP { q.jobs.pop_front() } else { None };
        if let Some(old) = &dropped {
            for c in (old.first.2..).take(old.texts.len()) {
                asked.remove(&(old.first.0, old.first.1, c));
            }
        }
        if !q.running {
            let (queue, tx, wake) = (self.queue.clone(), self.tx.clone(), self.wake.clone());
            let spawned = std::thread::Builder::new()
                .name("rain-syntax".into())
                .spawn(move || work(&queue, &tx, &*wake));
            q.running = spawned.is_ok();
        }
        dropped.is_none()
    }

    /// Notes that chunk `k` starts from one on its way. False when that pushed the oldest out.
    pub(crate) fn wait(&self, k: ChunkKey) -> bool {
        let mut waiting = self.waiting.borrow_mut();
        waiting.push_back(k);
        waiting.len() <= QUEUE_CAP || waiting.pop_front().is_none()
    }

    pub(crate) fn take_waiting(&self) -> VecDeque<ChunkKey> {
        self.waiting.take()
    }

    /// The jobs the worker finished since the last call, without those from before a
    /// [`Worker::reset`].
    pub(crate) fn done(&self) -> Vec<Lexed> {
        let mut asked = self.asked.borrow_mut();
        let done: Vec<Lexed> = self.rx.try_iter().filter(|d| d.epoch == self.epoch).collect();
        for d in &done {
            for c in (d.first.2..).take(d.chunks.len()) {
                asked.remove(&(d.first.0, d.first.1, c));
            }
        }
        done
    }

    /// Forgets every job: queued ones are dropped, and one under way comes back unwanted.
    pub(crate) fn reset(&mut self) {
        self.epoch += 1;
        locked(&self.queue).jobs.clear();
        self.asked.get_mut().clear();
        self.waiting.get_mut().clear();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        locked(&self.queue).jobs.clear();
    }
}

fn work(queue: &Mutex<Queue>, tx: &Sender<Lexed>, wake: &(dyn Fn() + Send + Sync)) {
    struct Running<'a>(&'a Mutex<Queue>);
    impl Drop for Running<'_> {
        fn drop(&mut self) {
            locked(self.0).running = false;
        }
    }
    let running = Running(queue);
    loop {
        let job = {
            let mut q = locked(queue);
            let Some(job) = q.jobs.pop_back() else {
                q.running = false;
                std::mem::forget(running);
                return;
            };
            job
        };
        let mut from = job.from;
        let mut chunks = Vec::with_capacity(job.texts.len());
        for text in job.texts {
            let chunk = text.lex(from, |code, from| job.lexer.lex(job.lang.as_deref(), code, from));
            from = chunk.end.clone();
            chunks.push(chunk);
        }
        let done = Lexed {
            epoch: job.epoch,
            first: job.first,
            chunks,
        };
        if tx.send(done).is_err() {
            return;
        }
        wake();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use rune_kit::syntax::Syntax;

    use super::*;
    use crate::DiffState;
    use crate::testing::{between, from_texts};

    /// Colours every line from a `/*` line to a `*/` line as a comment, and counts its calls.
    #[derive(Default)]
    struct Blocks {
        calls: AtomicUsize,
        slow: bool,
        /// Colours every line.
        all: bool,
        /// The threads it was called on.
        on: Mutex<Vec<std::thread::ThreadId>>,
        /// Each call waits for a message here, or for the sender to go.
        gate: Mutex<Option<Receiver<()>>>,
    }

    impl Lexer for Blocks {
        fn lex(&self, _: Option<&str>, code: &str, from: &Carry) -> (Vec<Highlight>, Carry) {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.on.lock().unwrap().push(std::thread::current().id());
            if let Some(gate) = self.gate.lock().unwrap().as_ref() {
                let _ = gate.recv();
            }
            let mut open = from.is_some();
            let mut out = Vec::new();
            let mut at = 0;
            for line in code.split_inclusive('\n') {
                open |= line.starts_with("/*");
                if open || self.all {
                    out.push((at..at + line.len() - 1, Paint::Kind(Syntax::Comment)));
                }
                open &= !line.starts_with("*/");
                at += line.len();
            }
            (out, open.then(|| Arc::new(()) as _))
        }

        fn same(&self, a: &Carry, b: &Carry) -> bool {
            a.is_some() == b.is_some()
        }

        fn slow(&self, _: Option<&str>) -> bool {
            self.slow
        }
    }

    /// A wake that sends on a channel, and a wait for the worker to owe `s` nothing more.
    fn waker() -> (Wake, impl Fn(&mut DiffState)) {
        let (tx, rx) = channel();
        let tx = Mutex::new(tx);
        let wake: Wake = Arc::new(move || {
            let _ = tx.lock().unwrap().send(());
        });
        let settle = move |s: &mut DiffState| {
            while s.colours_pending() {
                rx.recv_timeout(std::time::Duration::from_secs(60)).expect("the worker answers");
                s.poll_colours();
            }
        };
        (wake, settle)
    }

    #[test]
    fn a_slow_language_is_lexed_off_the_thread_and_comes_out_the_same() {
        let new = lines(600, |k| match k {
            100 => Some("/*"),
            400 => Some("*/"),
            _ => None,
        });
        let slow = || {
            Arc::new(Blocks {
                slow: true,
                ..Blocks::default()
            })
        };
        let mut inline = DiffState::new(vec![between(None, Some("a.zz"), "", &new)]);
        inline.set_lexer(slow(), None);
        let want: Vec<_> = (1..=600).map(|n| inline.highlights(0, Side::New, n)).collect();
        assert!(!want[149].is_empty() && want[449].is_empty());

        let (wake, settle) = waker();
        let lexer = slow();
        let mut s = DiffState::new(vec![between(None, Some("a.zz"), "", &new)]);
        s.set_lexer(lexer.clone(), Some(wake));
        for n in 1..=600 {
            assert!(s.highlights(0, Side::New, n).is_empty(), "line {n} draws plain at first");
        }
        let row = s.rows[s.row_of(0, Side::New, 150).unwrap() as usize];
        assert_eq!(s.colours_rev(row), (0, 0));
        settle(&mut s);
        assert_ne!(s.colours_rev(row), (0, 0), "the rows of a chunk that arrived rebuild");
        let got: Vec<_> = (1..=600).map(|n| s.highlights(0, Side::New, n)).collect();
        assert_eq!(got, want);
        let here = std::thread::current().id();
        let on = lexer.on.lock().unwrap();
        assert_eq!(on.len(), 5, "each chunk once");
        assert!(on.iter().all(|&t| t != here), "nothing was lexed on the thread that asked");
    }

    #[test]
    fn a_result_from_before_the_lexer_changed_is_dropped() {
        let (open, gate) = channel();
        let before = Arc::new(Blocks {
            slow: true,
            gate: Mutex::new(Some(gate)),
            ..Blocks::default()
        });
        let after = Arc::new(Blocks {
            slow: true,
            all: true,
            ..Blocks::default()
        });
        let (wake, settle) = waker();
        let mut s = DiffState::new(vec![between(None, Some("a.zz"), "", &lines(10, |_| Some("/*")))]);
        s.set_lexer(before.clone(), Some(wake.clone()));
        assert!(!comment(&s, Side::New, 1));
        while before.calls.load(Ordering::Relaxed) == 0 {
            std::thread::yield_now();
        }
        s.set_lexer(after.clone(), Some(wake));
        assert!(!s.colours_pending(), "the old lexer's chunk is no longer awaited");
        drop(open);
        while Arc::strong_count(&before) > 1 {
            std::thread::yield_now();
        }
        assert!(!s.poll_colours(), "the late result is not taken in");
        assert!(!comment(&s, Side::New, 1), "and the chunk is asked for again");
        settle(&mut s);
        assert!(comment(&s, Side::New, 1));
        assert_eq!((before.calls.load(Ordering::Relaxed), after.calls.load(Ordering::Relaxed)), (1, 1));
    }

    #[test]
    fn the_worker_keeps_a_bounded_queue_and_dropped_chunks_are_asked_for_again() {
        let (open, gate) = channel();
        let lexer = Arc::new(Blocks {
            slow: true,
            all: true,
            gate: Mutex::new(Some(gate)),
            ..Blocks::default()
        });
        let (wake, settle) = waker();
        let files: Vec<_> = (0..20).map(|_| between(None, Some("a.zz"), "", "one\n")).collect();
        let mut s = DiffState::new(files);
        s.set_lexer(lexer.clone(), Some(wake));
        let plain = |s: &DiffState| (0..20).filter(|&f| s.highlights(f, Side::New, 1).is_empty()).count();
        assert_eq!(plain(&s), 20);
        let row = s.rows[s.row_of(0, Side::New, 1).unwrap() as usize];
        assert_ne!(s.colours_rev(row), (0, 0), "the rows of a dropped chunk rebuild");
        drop(open);
        settle(&mut s);
        let lexed = lexer.calls.load(Ordering::Relaxed);
        assert!((QUEUE_CAP..=QUEUE_CAP + 1).contains(&lexed), "{lexed} chunks lexed of 20 asked for");
        let mut left = plain(&s);
        assert_eq!(left, 20 - lexed, "the dropped ones were never lexed");
        while left > 0 {
            settle(&mut s);
            left = plain(&s);
        }
        assert_eq!(lexer.calls.load(Ordering::Relaxed), 20, "each chunk once in the end");
    }

    /// `n` lines, `line k` each but for those `at` gives.
    fn lines(n: u32, at: impl Fn(u32) -> Option<&'static str>) -> String {
        (1..=n).map(|k| at(k).map_or(format!("line {k}\n"), |l| format!("{l}\n"))).collect()
    }

    fn added(new: &str) -> (DiffState, Arc<Blocks>) {
        let mut s = DiffState::new(vec![between(None, Some("a.zz"), "", new)]);
        let lexer = Arc::new(Blocks::default());
        s.set_lexer(lexer.clone(), None);
        (s, lexer)
    }

    fn comment(s: &DiffState, side: Side, n: u32) -> bool {
        !s.highlights(0, side, n).is_empty()
    }

    #[test]
    fn a_comment_over_a_chunk_end_keeps_its_colour_and_each_chunk_is_lexed_once() {
        let (s, lexer) = added(&lines(300, |k| match k {
            100 => Some("/*"),
            200 => Some("*/"),
            _ => None,
        }));
        for _ in 0..2 {
            for n in 1..=300 {
                assert_eq!(comment(&s, Side::New, n), (100..=200).contains(&n), "line {n}");
            }
            assert_eq!(lexer.calls.load(Ordering::Relaxed), 3, "one call a chunk, none on a second pass");
        }
        let row = s.rows[s.row_of(0, Side::New, 150).unwrap() as usize];
        assert_eq!(s.colours_rev(row), (0, 0), "nothing was coloured twice");
    }

    #[test]
    fn a_jump_far_down_lexes_a_bounded_run_back() {
        let (s, lexer) = added(&lines(40 * CHUNK, |k| (k == 4000).then_some("/*")));
        assert!(comment(&s, Side::New, 5000), "the comment opened within the run back");
        assert_eq!(lexer.calls.load(Ordering::Relaxed), LOOKBACK as usize + 1);
        assert!(comment(&s, Side::New, 5100));
        assert!(!comment(&s, Side::New, 3999));
        assert_eq!(
            lexer.calls.load(Ordering::Relaxed),
            LOOKBACK as usize + 1,
            "the run's chunks are held"
        );
    }

    #[test]
    fn a_guess_past_the_run_back_is_corrected_once_the_chunks_before_are_lexed() {
        let (s, lexer) = added(&lines(40 * CHUNK, |k| (k == 5).then_some("/*")));
        assert!(!comment(&s, Side::New, 5000), "opened further back than the run reaches");
        let guessed = 39 - LOOKBACK;
        let first = |c: u32| c * CHUNK + 1;
        for c in 0..guessed {
            assert!(comment(&s, Side::New, first(c) + 10));
        }
        let row = s.rows[s.row_of(0, Side::New, first(guessed)).unwrap() as usize];
        assert_ne!(s.colours_rev(row), (0, 0), "the guessed chunk's rows rebuild");
        for c in guessed..40 {
            assert!(comment(&s, Side::New, first(c)), "chunk {c}");
        }
        assert!(comment(&s, Side::New, 5000));
        let once = (LOOKBACK + 1 + guessed) as usize;
        assert_eq!(
            lexer.calls.load(Ordering::Relaxed),
            once + (LOOKBACK + 1) as usize,
            "each guessed chunk is lexed once more"
        );
    }

    #[test]
    fn a_hunk_starts_fresh_unless_the_diff_holds_the_line_before_it() {
        let old = lines(400, |_| None);
        let new = lines(400, |k| match k {
            104 | 127 => Some("/*"),
            120 => Some("changed"),
            _ => None,
        });
        let mut s = DiffState::new(vec![from_texts("a.zz", &old, &new)]);
        s.set_texts(0, None, None);
        s.set_lexer(Arc::new(Blocks::default()), None);
        assert!(comment(&s, Side::New, 104) && comment(&s, Side::New, 107), "open to its hunk's end");
        assert!(
            !comment(&s, Side::New, 120),
            "the lines between the hunks are not held: a fresh start"
        );
        for n in 127..=130 {
            assert!(comment(&s, Side::New, n), "line {n}: the hunk runs over the chunk's end");
        }
        assert!(!comment(&s, Side::Old, 104) && !comment(&s, Side::Old, 129));
        s.set_texts(0, Some(Arc::from(old.as_str())), Some(Arc::from(new.as_str())));
        for n in [104, 120, 129, 300, 400] {
            assert!(comment(&s, Side::New, n), "line {n} with the whole text");
        }
        assert!(!comment(&s, Side::Old, 300), "the old side holds no comment");
    }
}
