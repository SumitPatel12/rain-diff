//! Very long lines: what a row build and a cursor step cost as a line grows, counted in bytes
//! and allocations on the thread that builds the view.
//!
//! The tests hold a 2,000,000-character line to what its drawn part costs. The table of
//! numbers by length: `cargo test -p rain-diffview --test long_lines -- --ignored --nocapture`

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Instant;

use common::patch::from_texts;
use rain_core::comments::Anchor;
use rain_core::diff::{FileDiffBuilder, FileStatus, LineKind, Side};
use rain_diffview::{Annotation, DiffMode, DiffState, DiffView, LINE_CAP, LineRef, SCROLL_KEY, VimKey};
use rune::{Cx, El, Env, Runtime, col};
use rune_kit::Kit;

struct Counting;

thread_local! {
    static BYTES: Cell<u64> = const { Cell::new(0) };
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

fn count(size: usize) {
    let _ = BYTES.try_with(|b| b.set(b.get() + size as u64));
    let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count(l.size());
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        count(new);
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// What ran between two readings: bytes allocated, allocations, lines built, bytes of code
/// those lines drew.
#[derive(Clone, Copy, Debug)]
struct Cost {
    bytes: u64,
    allocs: u64,
    lines: u64,
    drawn: u64,
    ms: f64,
}

fn cost(run: impl FnOnce()) -> Cost {
    let (b, a, l) = (BYTES.get(), ALLOCS.get(), rain_diffview::line_builds());
    let d = rain_diffview::line_bytes();
    let t = Instant::now();
    run();
    Cost {
        bytes: BYTES.get() - b,
        allocs: ALLOCS.get() - a,
        lines: rain_diffview::line_builds() - l,
        drawn: rain_diffview::line_bytes() - d,
        ms: t.elapsed().as_secs_f64() * 1e3,
    }
}

struct App {
    diff: DiffState,
}

fn view(s: &App, _: &mut Cx<App>) -> El {
    let k = Kit::neutral(true);
    col()
        .w_full()
        .h_full()
        .child(DiffView::new(|s: &App| &s.diff, |s: &mut App| &mut s.diff).build(s, &k))
}

fn minified(chars: usize) -> String {
    let mut s = "var a=b.c(d,e)+f[g];".repeat(chars / 20 + 1);
    s.truncate(chars);
    s
}

/// A file whose second line is `chars` long: changed in one word when `paired`, else added.
fn file(path: &str, chars: usize, paired: bool) -> DiffState {
    let long = minified(chars);
    let new = format!("first\n{long}\nlast\n");
    let old = if paired {
        format!("first\n{}\nlast\n", long.replacen("var a", "let a", 1))
    } else {
        "first\nlast\n".to_owned()
    };
    DiffState::new(vec![from_texts(path, &old, &new)])
}

fn settle(rt: &mut Runtime<App>) {
    for _ in 0..12 {
        rt.frame(1.0 / 60.0);
    }
}

/// The first frames of the view, then one `l` on the long line: `(build, step)`.
fn build_and_step(mut diff: DiffState, mode: DiffMode, wrap: bool) -> (Cost, Cost) {
    diff.set_mode(mode);
    diff.set_wrap(wrap);
    let mut rt = None;
    let build = cost(|| {
        let mut r = Runtime::new(App { diff }, view, (1200.0, 800.0), Env::new(true, 1.0));
        r.frame(0.0);
        settle(&mut r);
        rt = Some(r);
    });
    let mut rt = rt.unwrap();
    rt.update(|s, tx| {
        let at = LineRef::new(Side::New, 2);
        s.diff.scroll_to_line(0, at, tx);
    });
    settle(&mut rt);
    rt.update(|s, tx| {
        s.diff.vim(VimKey::Char('l'), tx);
    });
    settle(&mut rt);
    let step = cost(|| {
        rt.update(|s, tx| {
            s.diff.vim(VimKey::Char('l'), tx);
        });
        rt.frame(1.0 / 60.0);
    });
    (build, step)
}

#[test]
#[ignore]
fn table_of_costs_by_line_length() {
    println!("chars      view          syntax pair | build: bytes allocs drawn ms | one `l`: bytes allocs lines drawn ms");
    for chars in [1_000usize, 10_000, 200_000, 2_000_000] {
        for (mode, wrap, name) in [
            (DiffMode::Unified, false, "unified"),
            (DiffMode::Unified, true, "unified+wrap"),
            (DiffMode::Split, false, "split"),
        ] {
            for (path, paired) in [("a.txt", false), ("a.js", false), ("a.js", true)] {
                let (b, s) = build_and_step(file(path, chars, paired), mode, wrap);
                println!(
                    "{chars:>9} {name:<13} {:<6} {:<4} | {:>11} {:>6} {:>9} {:>8.1} | {:>11} {:>6} {:>2} {:>9} {:>8.1}",
                    if path.ends_with(".js") { "js" } else { "none" },
                    if paired { "yes" } else { "no" },
                    b.bytes,
                    b.allocs,
                    b.drawn,
                    b.ms,
                    s.bytes,
                    s.allocs,
                    s.lines,
                    s.drawn,
                    s.ms
                );
            }
        }
    }
}

/// A `.js` file that is one line of `chars` characters with three small edits, as a state:
/// `(state, ms to build it, what the formatting check read)`.
fn one_line_file(chars: usize) -> (DiffState, f64, rain_core::formatting::Work) {
    let old = minified(chars);
    let mut new = old.clone();
    for at in [chars / 4, chars / 2, chars - chars / 4] {
        let d = at + old[at..].find("d,e").expect("an argument list");
        new.replace_range(d..d + 1, "z");
    }
    let mut b = FileDiffBuilder::new(0, "web/app.js", FileStatus::Modified);
    b.push_hunk(1, 1, 1, 1, "");
    b.push_line(LineKind::Removed, 1, 0, &old);
    b.push_line(LineKind::Added, 0, 1, &new);
    let f = b.finish();
    let before = rain_core::formatting::work();
    let t = Instant::now();
    let d = DiffState::new(vec![f]);
    let ms = t.elapsed().as_secs_f64() * 1e3;
    let now = rain_core::formatting::work();
    let read = rain_core::formatting::Work {
        ink: now.ink - before.ink,
        lexed: now.lexed - before.lexed,
    };
    (d, ms, read)
}

#[test]
fn the_state_of_a_one_line_file_builds_without_reading_the_line_to_compare_it() {
    assert_eq!(rain_core::formatting::LINE_MAX_BYTES, LINE_CAP, "what is compared is what is drawn");
    let (d, ms, read) = one_line_file(200_000);
    assert_eq!(read, rain_core::formatting::Work::default(), "the formatting check read the line");
    assert!(!d.formatting_only(0, 0) && !d.formatting_only(0, 1), "a line that long is a change");
    assert_eq!(d.row_count(), 4, "the header, the hunk and the two lines");
    assert!(ms < 1000.0, "building the state took {ms:.0} ms");
}

/// `cargo test -p rain-diffview --release --test long_lines -- --ignored --nocapture two_megabytes`
#[test]
#[ignore]
fn the_state_of_a_one_line_file_of_two_megabytes() {
    let (d, ms, read) = one_line_file(2_000_000);
    println!(
        "PERF one_line_state chars=2000000 state_ms={ms:.1} rows={} formatting_read={read:?}",
        d.row_count()
    );
    assert_eq!(read, rain_core::formatting::Work::default());
    assert!(ms < 1000.0, "building the state took {ms:.0} ms");
}

const VIEWS: [(DiffMode, bool); 3] = [(DiffMode::Unified, false), (DiffMode::Unified, true), (DiffMode::Split, false)];

#[test]
fn a_very_long_line_costs_what_its_drawn_part_costs() {
    for (mode, wrap) in VIEWS {
        let (build_cap, step_cap) = build_and_step(file("a.js", LINE_CAP, true), mode, wrap);
        let (build, step) = build_and_step(file("a.js", 200_000, true), mode, wrap);
        let what = format!("{mode:?} wrap {wrap}");
        assert!(step.lines >= 1, "{what}: the step rebuilt the cursor's row");
        for (name, long, cap) in [("the first frames", build, build_cap), ("a cursor step", step, step_cap)] {
            assert!(
                long.drawn <= 2 * long.lines * LINE_CAP as u64,
                "{what}, {name}: {} rows of at most two sides drew {} bytes of code",
                long.lines,
                long.drawn
            );
            assert!(
                long.bytes <= cap.bytes + cap.bytes / 4 + 64 * 1024,
                "{what}, {name}: allocated {} bytes, against {} for a line of {LINE_CAP} characters",
                long.bytes,
                cap.bytes
            );
        }
    }
}

fn runtime(diff: DiffState) -> Runtime<App> {
    let mut rt = Runtime::new(App { diff }, view, (1200.0, 800.0), Env::new(true, 1.0));
    rt.frame(0.0);
    settle(&mut rt);
    rt
}

/// The text and rect of the note under a cut line, when one is drawn.
fn cut_note(rt: &Runtime<App>) -> Option<(String, [f32; 4])> {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("cut") && !e.exiting)
        .map(|e| (e.text.unwrap_or_default(), e.rect))
}

#[test]
fn a_cut_line_says_how_much_is_not_drawn_and_a_normal_line_says_nothing() {
    for (mode, wrap) in VIEWS {
        let mut d = file("a.js", 200_000, false);
        d.set_mode(mode);
        d.set_wrap(wrap);
        let rt = runtime(d);
        let (note, _) = cut_note(&rt).unwrap_or_else(|| panic!("{mode:?} wrap {wrap}: no note"));
        assert_eq!(note, "… 186 KB more on this line, not drawn", "{mode:?} wrap {wrap}");

        let mut d = file("a.js", LINE_CAP, false);
        d.set_mode(mode);
        d.set_wrap(wrap);
        assert_eq!(
            cut_note(&runtime(d)),
            None,
            "{mode:?} wrap {wrap}: a line of {LINE_CAP} bytes is drawn whole"
        );
    }
}

#[test]
fn sideways_scrolling_reaches_the_cut_and_keeps_the_note_in_view() {
    let mut rt = runtime(file("a.txt", 200_000, false));
    let rows = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("rows")).unwrap().rect;
    let drawn = LINE_CAP as f32 * 7.8;
    assert!(
        rows[2] > drawn && rows[2] < drawn + 400.0,
        "the list is as wide as the drawn part: {}",
        rows[2]
    );
    rt.update(|_, tx| {
        tx.instant();
        tx.scroll_to_x(SCROLL_KEY, 60_000.0);
    });
    settle(&mut rt);
    let (_, at) = cut_note(&rt).expect("the note");
    assert!(at[0] > 0.0 && at[0] < 400.0, "the note holds to the left edge of the code: {at:?}");
}

#[test]
fn the_current_match_past_the_cut_is_named_in_the_note() {
    let long = format!("{} needle", minified(50_000));
    let mut d = DiffState::new(vec![from_texts(
        "a.txt",
        "first
",
        &format!(
            "first
{long}
"
        ),
    )]);
    let i = (0..d.files()[0].lines.len() as u32)
        .find(|&i| d.files()[0].line_text(i).len() > LINE_CAP)
        .unwrap();
    d.set_find("needle", Some((0, i)));
    let mut rt = runtime(d);
    assert_eq!(
        cut_note(&rt).map(|n| n.0).as_deref(),
        Some("… 39 KB more on this line, not drawn · the match is in it")
    );
    rt.update(|s, _| s.diff.set_find("var", Some((0, i))));
    settle(&mut rt);
    assert_eq!(cut_note(&rt).map(|n| n.0).as_deref(), Some("… 39 KB more on this line, not drawn"));
}

#[test]
fn comments_anchor_to_a_cut_line_and_a_span_past_the_cut_draws_nothing() {
    let mut d = file("a.js", 200_000, false);
    let at = LineRef::new(Side::New, 2);
    let span = Anchor::Span {
        side: Side::New,
        start_line: 2,
        start_col: 150_000,
        end_line: 2,
        end_col: 150_010,
    };
    d.set_annotations(vec![Annotation::line(0, at, "c1"), Annotation::new(0, span, "c2")]);
    let rows: Vec<&str> = d.annotation_rows().into_iter().map(|(_, id)| id).collect();
    assert_eq!(rows, ["c1", "c2"]);
    d.set_cursor(Some((0, at)));
    assert_eq!(d.comment_at_cursor().map(|l| (l.start, l.end)), Some((at, at)));
    let rt = runtime(d);
    assert!(cut_note(&rt).is_some());
}
