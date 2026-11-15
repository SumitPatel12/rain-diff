//! Release-only measurements over the xtask scratch repos; generate them first with
//! `cargo xtask fixtures --tier s|l|xl`. Run: `cargo test -p rain-git --release -- --ignored --nocapture`.

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

use rain_core::{FileDiff, LineRec, RepoLaunch};
use rain_git::{DiffOpts, diff, parse_reader, parse_str, resolve};

fn gen_dir(tier: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gen").join(tier)
}

fn require(p: &Path) {
    assert!(
        p.exists(),
        "{} is missing: run `cargo xtask fixtures --tier <tier>` first",
        p.display()
    );
}

fn commits() -> RepoLaunch {
    RepoLaunch {
        base: "HEAD~1".into(),
        target: "HEAD".into(),
        ..RepoLaunch::default()
    }
}

#[cfg(windows)]
fn peak_rss_mb() -> f64 {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged_pool: usize,
        quota_paged_pool: usize,
        quota_peak_non_paged_pool: usize,
        quota_non_paged_pool: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Counters::default()
    };
    // SAFETY: a pseudo-handle for this process and a correctly sized, writable struct.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    if ok == 0 { f64::NAN } else { c.peak_working_set as f64 / 1e6 }
}

#[cfg(not(windows))]
fn peak_rss_mb() -> f64 {
    f64::NAN
}

struct Sizes {
    lines: u64,
    text: usize,
    recs: usize,
    hunks: usize,
}

fn sizes(files: &[FileDiff]) -> Sizes {
    Sizes {
        lines: files.iter().map(|f| f.lines.len() as u64).sum(),
        text: files.iter().map(|f| f.text.capacity()).sum(),
        recs: files.iter().map(|f| f.lines.capacity() * std::mem::size_of::<LineRec>()).sum(),
        hunks: files
            .iter()
            .map(|f| f.hunks.capacity() * std::mem::size_of::<rain_core::Hunk>())
            .sum(),
    }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

/// `git diff` with its output drained through a pipe, as `diff()` reads it.
fn git_alone_ms(repo: &Path) -> f64 {
    let t = Instant::now();
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["diff", "--no-ext-diff", "--color=never", "--find-renames", "HEAD~1", "HEAD"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::copy(&mut child.stdout.take().unwrap(), &mut std::io::sink()).unwrap();
    assert!(child.wait().unwrap().success());
    ms(t)
}

/// Returns the process's peak RSS right after the parse, before the diff runs and their
/// equality check (which formats every file twice) raise it.
fn measure(tier: &str, parse_budget_ms: f64) -> f64 {
    let dir = gen_dir(tier);
    let patch = dir.join("diff.patch");
    require(&patch);
    let bytes = std::fs::metadata(&patch).unwrap().len();

    let t = Instant::now();
    let parsed = parse_reader(BufReader::with_capacity(1 << 18, std::fs::File::open(&patch).unwrap()), 0).unwrap();
    let parse_ms = ms(t);
    let peak = peak_rss_mb();
    let s = sizes(&parsed.files);
    println!(
        "[{tier}] parse {:.1} MB patch: {parse_ms:.0} ms, {} files, {} lines; arena {:.1} MB, line records {:.1} MB, hunks {:.2} MB; peak RSS {peak:.0} MB",
        bytes as f64 / 1e6,
        parsed.files.len(),
        s.lines,
        s.text as f64 / 1e6,
        s.recs as f64 / 1e6,
        s.hunks as f64 / 1e6,
    );
    drop(parsed);

    let repo = dir.join("repo");
    let best = |f: &dyn Fn() -> f64| (0..2).map(|_| f()).fold(f64::INFINITY, f64::min);
    let git_ms = best(&|| git_alone_ms(&repo));
    let run = |jobs: usize| {
        let t = Instant::now();
        let r = resolve(&repo, &commits()).unwrap();
        let opts = DiffOpts {
            jobs,
            ..DiffOpts::default()
        };
        let files = diff(&repo, 0, &r, &opts).unwrap();
        (ms(t), files)
    };
    let (one_ms, one) = run(1);
    let (par_ms, par) = run(0);
    let one_ms = one_ms.min(run(1).0);
    let par_ms = par_ms.min(run(0).0);
    assert_eq!(one.len(), par.len());
    for (a, b) in one.iter().zip(&par) {
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
    }
    let generated = one.iter().filter(|f| f.generated).count();
    println!(
        "[{tier}] best of 2: git diff alone {git_ms:.0} ms; resolve + diff() total {one_ms:.0} ms with 1 process, {par_ms:.0} ms with path groups ({generated} generated files)"
    );

    if !cfg!(debug_assertions) {
        assert!(
            parse_ms <= parse_budget_ms,
            "[{tier}] parse took {parse_ms:.0} ms > {parse_budget_ms} ms"
        );
    }
    peak
}

#[test]
#[ignore = "release perf; needs cargo xtask fixtures --tier l and --tier xl"]
fn perf_xl_then_l() {
    let peak = measure("xl", 400.0);
    println!(
        "[xl] peak RSS after the whole run, equality check included: {:.0} MB",
        peak_rss_mb()
    );
    if !cfg!(debug_assertions) && peak.is_finite() {
        assert!(peak <= 250.0, "peak RSS {peak:.0} MB > 250 MB");
    }
    measure("l", 100.0);
}

#[test]
#[ignore = "needs cargo xtask fixtures --tier s"]
fn xtask_s_repo_diff_equals_its_patch() {
    let dir = gen_dir("s");
    require(&dir.join("repo"));
    let repo = dir.join("repo");
    let r = resolve(&repo, &commits()).unwrap();
    let files = diff(&repo, 0, &r, &DiffOpts::default()).unwrap();
    let patch = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/s.patch")).unwrap();
    let parsed = parse_str(&patch, 0).files;
    let strip = |f: &FileDiff| {
        format!(
            "{:?}",
            FileDiff {
                generated: false,
                ..f.clone()
            }
        )
    };
    assert_eq!(files.len(), parsed.len());
    for (a, b) in files.iter().zip(&parsed) {
        assert_eq!(strip(a), strip(b));
    }
}
