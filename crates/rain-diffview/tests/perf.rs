//! Release-only numbers on the XL fixture (about 1.33M diff lines): how long the state takes to
//! build and rebuild, what it holds beyond the files, and how frames cost while scrolling.
//!
//! `cargo xtask fixtures --tier xl` writes `fixtures/gen/xl/diff.patch`; `RAIN_XL_PATCH`
//! points elsewhere. Run:
//! `cargo test -p rain-diffview --release --test perf -- --ignored --nocapture`

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

use rain_diffview::{DiffMode, DiffState, DiffView};
use rune::{Cx, El, Env, KeyEvent, Runtime, Tx, col};
use rune_kit::Kit;

struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let now = LIVE.fetch_add(l.size() as isize, Ordering::Relaxed) + l.size() as isize;
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            let now = LIVE.fetch_add(new as isize - l.size() as isize, Ordering::Relaxed) + new as isize - l.size() as isize;
            PEAK.fetch_max(now, Ordering::Relaxed);
        }
        q
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn mb(b: isize) -> f64 {
    b as f64 / (1024.0 * 1024.0)
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn xl_patch() -> Option<PathBuf> {
    let p = std::env::var_os("RAIN_XL_PATCH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gen/xl/diff.patch"));
    p.exists().then_some(p)
}

fn l_patch() -> Option<PathBuf> {
    let p = std::env::var_os("RAIN_L_PATCH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gen/l/diff.patch"));
    p.exists().then_some(p)
}

/// `cargo xtask fixtures --tier l` writes the L fixture; `RAIN_L_PATCH` points elsewhere.
#[test]
#[ignore]
fn l_spotlight_and_hunk_ids() {
    let Some(path) = l_patch() else {
        println!("SKIP: no L patch (cargo xtask fixtures --tier l, or RAIN_L_PATCH)");
        return;
    };
    let files = common::patch::parse(&std::fs::read_to_string(&path).unwrap());
    let mut state = DiffState::new(files);
    let big = (0..state.files().len()).max_by_key(|&f| state.files()[f].lines.len()).unwrap();
    let mid = state.files()[big].lines[state.files()[big].lines.len() / 2];
    let (side, n) = if mid.new != 0 {
        (rain_core::diff::Side::New, mid.new)
    } else {
        (rain_core::diff::Side::Old, mid.old)
    };
    let mut worst = 0.0f64;
    for mode in [DiffMode::Unified, DiffMode::Split] {
        state.set_mode(mode);
        for k in 0..20u32 {
            let t = Instant::now();
            state.set_spotlight(Some(rain_diffview::Spot {
                file: big,
                side,
                lines: (n, n + k),
                caption: None,
                step: None,
            }));
            worst = worst.max(ms(t));
            assert!(state.spotlight_rows().is_some());
        }
    }
    let files = state.files().to_vec();
    let t = Instant::now();
    let fresh = DiffState::new(files.clone());
    let fresh_ms = ms(t);
    drop(fresh);
    let t = Instant::now();
    state.set_files(files);
    let reconcile = ms(t);
    println!("set_files into an empty state {fresh_ms:.1} ms");
    println!(
        "l: {} rows; set_spotlight worst {worst:.3} ms; set_files with hunk-id reconciliation {reconcile:.1} ms",
        state.row_count()
    );
    assert!(worst < 2.0, "set_spotlight {worst:.3} ms");
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v[((v.len() as f64 - 1.0) * p).round() as usize]
}

struct App {
    diff: DiffState,
}

fn view(s: &App, _: &mut Cx<App>) -> El {
    let k = Kit::neutral(true);
    col().w_full().h_full().child(
        DiffView::new(|s: &App| &s.diff, |s: &mut App| &mut s.diff)
            .calm_added_files(true)
            .build(s, &k),
    )
}

fn to_last(s: &mut App, _: &KeyEvent, tx: &mut Tx) -> bool {
    let last = s.diff.row_count() - 1;
    let key = s.diff.row_key(last).unwrap();
    tx.instant();
    tx.scroll_into_view(key);
    true
}

#[test]
#[ignore]
fn xl_state_and_scroll() {
    let Some(path) = xl_patch() else {
        println!("SKIP: no XL patch (cargo xtask fixtures --tier xl, or RAIN_XL_PATCH)");
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap();
    let t = Instant::now();
    let files = common::patch::parse(&text);
    let parse_ms = ms(t);
    drop(text);
    let lines: usize = files.iter().map(|f| f.lines.len()).sum();
    let file_bytes: usize = files
        .iter()
        .map(|f| f.text.capacity() + f.lines.capacity() * 24 + f.path.capacity())
        .sum();
    println!(
        "xl: {} files, {lines} diff lines, files hold {:.1} MB; test parser {parse_ms:.0} ms",
        files.len(),
        file_bytes as f64 / 1048576.0
    );

    let before = LIVE.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);
    let t = Instant::now();
    let state = DiffState::new(files);
    let new_ms = ms(t);
    let extra = LIVE.load(Ordering::Relaxed) - before;
    let peak = PEAK.load(Ordering::Relaxed) - before;
    println!(
        "DiffState::new (set_files + rebuild): {new_ms:.1} ms, {} rows; extra {:.1} MB (peak {:.1} MB), heap_bytes {:.1} MB",
        state.row_count(),
        mb(extra),
        mb(peak),
        state.heap_bytes() as f64 / 1048576.0
    );

    let files = state.into_files();
    let mut state = DiffState::default();
    let t = Instant::now();
    state.set_files(files);
    let set_ms = ms(t);
    let t = Instant::now();
    state.set_collapsed(0, true);
    state.set_collapsed(0, false);
    let rebuild_ms = ms(t) / 2.0;
    let t = Instant::now();
    state.set_mode(DiffMode::Split);
    let split_ms = ms(t);
    let split_rows = state.row_count();
    let t = Instant::now();
    state.set_mode(DiffMode::Unified);
    let unified_ms = ms(t);
    let t = Instant::now();
    let mut keyed = 0usize;
    for i in 0..state.row_count() {
        keyed += state.row_key(i).map_or(0, |k| k.len());
    }
    let keys_ms = ms(t);
    println!(
        "set_files {set_ms:.1} ms; rebuild {rebuild_ms:.1} ms; split switch {split_ms:.1} ms ({split_rows} rows); unified switch {unified_ms:.1} ms"
    );
    println!(
        "formatting every row key (what Rune does once per row change): {keys_ms:.1} ms, {:.1} MB of key text",
        keyed as f64 / 1048576.0
    );

    let t = Instant::now();
    let mut rt = Runtime::new(App { diff: state }, view, (1600.0, 1000.0), Env::new(true, 1.0));
    rt.frame(0.0);
    let first_ms = ms(t);
    for _ in 0..30 {
        rt.frame(1.0 / 120.0);
    }
    println!(
        "Runtime first frame (keys for every row + the first window): {first_ms:.1} ms; RSS-like live heap {:.0} MB",
        mb(LIVE.load(Ordering::Relaxed))
    );

    let mut commit = Vec::new();
    for _ in 0..100 {
        rt.invalidate();
        let t = Instant::now();
        rt.frame(1.0 / 120.0);
        commit.push(ms(t));
    }
    println!("a commit (invalidate + frame) at rest: p50 {:.2} ms", pct(&mut commit, 0.5));

    let commits = rune::commit_generation();
    let mut times = Vec::new();
    for i in 0..1000 {
        let dy = if i % 50 < 40 { -60.0 } else { -900.0 };
        rt.wheel(800.0, 500.0, dy);
        let t = Instant::now();
        rt.frame(1.0 / 120.0);
        times.push(ms(t));
    }
    let commits = rune::commit_generation() - commits;
    let max = times.iter().copied().fold(0.0, f64::max);
    let (p50, p95) = (pct(&mut times.clone(), 0.5), pct(&mut times, 0.95));
    println!(
        "1000 wheel frames: p50 {p50:.2} ms, p95 {p95:.2} ms, max {max:.2} ms, {commits} view commits, ended in file {}",
        rt.state.diff.visible_file()
    );

    // Within one large file only: no file boundaries, so no sticky-header changes.
    rt.on_key = Some(|s, _, tx| {
        let big = s
            .diff
            .files()
            .iter()
            .enumerate()
            .max_by_key(|(_, f)| f.lines.len())
            .map_or(0, |(i, _)| i);
        tx.instant();
        s.diff.scroll_to_file(big, tx);
        true
    });
    rt.key(KeyEvent {
        key: rune::Key::Function(5),
        mods: rune::Mods::default(),
    });
    for _ in 0..30 {
        rt.frame(1.0 / 120.0);
    }
    for _ in 0..5 {
        rt.wheel(800.0, 500.0, -400.0);
        rt.frame(1.0 / 120.0);
    }
    let commits = rune::commit_generation();
    let mut times = Vec::new();
    for _ in 0..1000 {
        rt.wheel(20.0, 500.0, -60.0);
        let t = Instant::now();
        rt.frame(1.0 / 120.0);
        times.push(ms(t));
    }
    let commits = rune::commit_generation() - commits;
    let max = times.iter().copied().fold(0.0, f64::max);
    let (p50, p95) = (pct(&mut times.clone(), 0.5), pct(&mut times, 0.95));
    println!(
        "1000 wheel frames in file {} (pointer on the gutter): p50 {p50:.2} ms, p95 {p95:.2} ms, max {max:.2} ms, {commits} view commits",
        rt.state.diff.visible_file()
    );

    rt.on_key = Some(to_last);
    rt.key(KeyEvent {
        key: rune::Key::Function(5),
        mods: rune::Mods::default(),
    });
    let t = Instant::now();
    rt.frame(1.0 / 120.0);
    let jump_ms = ms(t);
    let last = rt.state.diff.row_key(rt.state.diff.row_count() - 1).unwrap();
    let shown = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(last.as_str()) && !e.exiting)
        .map(|e| e.rect);
    println!(
        "scroll_into_view(last of {} rows): {jump_ms:.1} ms, last row at {shown:?} in 1 frame",
        rt.state.diff.row_count()
    );
    assert!(
        shown.is_some_and(|r| r[1] >= -1.0 && r[1] + r[3] <= 1001.0),
        "the last row is on screen after one frame"
    );

    let mut state = std::mem::take(&mut rt.state.diff);
    drop(rt);
    let t = Instant::now();
    state.set_collapsed_where(true, |f| f.lines.len() > 50_000);
    let collapse_ms = ms(t);
    let rows = state.row_count();
    let t = Instant::now();
    let mut rt = Runtime::new(App { diff: state }, view, (1600.0, 1000.0), Env::new(true, 1.0));
    rt.frame(0.0);
    println!(
        "with files over 50k lines collapsed ({collapse_ms:.1} ms, {rows} rows): first frame {:.1} ms",
        ms(t)
    );
    let t = Instant::now();
    rt.state.diff.set_collapsed_where(false, |_| true);
    rt.invalidate();
    rt.frame(1.0 / 120.0);
    println!("opening them again (rebuild + Rune re-keying): {:.1} ms", ms(t));

    assert!(extra < 60 * 1024 * 1024, "DiffState extra {:.1} MB > 60 MB", mb(extra));
    assert!(
        set_ms < 200.0 && rebuild_ms < 300.0 && split_ms < 500.0,
        "budgets: set_files {set_ms:.0}, rebuild {rebuild_ms:.0}, split {split_ms:.0}"
    );
}
