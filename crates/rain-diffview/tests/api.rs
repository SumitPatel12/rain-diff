//! The read-only row API an overview ruler draws from, whole-file views, and the scroll hooks
//! that let an app follow and move the viewport without running its view.

mod common;

use std::cell::Cell;
use std::sync::Arc;

use common::patch::{from_texts, parse};
use rain_core::comments::Anchor;
use rain_core::diff::{FileDiff, LineKind, Side};
use rain_diffview::{Annotation, DiffMode, DiffState, DiffView, LineRef, SCROLL_KEY};
use rune::{Cx, El, Env, Live, Runtime, Tx, col};
use rune_kit::Kit;

const M_PATCH: &str = include_str!("../../../fixtures/m.patch");

fn m() -> DiffState {
    DiffState::new(parse(M_PATCH))
}

/// Every (file, line) each row shows, from the public API: a row of a file shows the lines
/// `row_of_line` maps to it.
fn lines_of(s: &DiffState) -> Vec<(usize, LineRef, u32)> {
    let mut out = Vec::new();
    for (f, file) in s.files().iter().enumerate() {
        for l in &file.lines {
            for (side, n) in [(Side::Old, l.old), (Side::New, l.new)] {
                if n != 0
                    && let Some(r) = s.row_of_line(f, LineRef::new(side, n))
                {
                    out.push((f, LineRef::new(side, n), r));
                }
            }
        }
    }
    out
}

#[test]
fn file_rows_and_row_file_agree_on_fixture_m() {
    for mode in [DiffMode::Unified, DiffMode::Split] {
        let mut s = m();
        s.set_mode(mode);
        assert!(s.files().len() > 10);
        assert_eq!(s.file_rows().len(), s.files().len());
        for (f, &r) in s.file_rows().iter().enumerate() {
            assert_eq!(s.row_file(r as usize), Some(f));
            assert_eq!(s.row_key(r as usize), Some(format!("df{f}")));
        }
        assert!(s.file_rows().windows(2).all(|w| w[0] < w[1]));
        assert_eq!(s.row_file(s.row_count()), None);
        let mut last = 0;
        for r in 0..s.row_count() {
            let f = s.row_file(r).unwrap();
            assert!(f >= last, "rows ascend by file");
            last = f;
        }
    }
}

#[test]
fn hunk_row_is_where_next_hunk_puts_the_cursor() {
    for mode in [DiffMode::Unified, DiffMode::Split] {
        let mut s = m();
        s.set_mode(mode);
        let expected: Vec<u32> = (0..s.files().len())
            .flat_map(|f| (0..s.files()[f].hunks.len()).map(move |h| (f, h)))
            .map(|(f, h)| s.hunk_row(f, h).expect("an open file's hunks have rows"))
            .collect();
        let mut stops = Vec::new();
        let mut tx = Tx::default();
        while s.next_hunk(true, &mut tx) {
            let (f, at) = s.cursor().unwrap();
            stops.push(s.row_of_line(f, at).unwrap());
        }
        assert_eq!(stops, expected, "{mode:?}");
        assert_eq!(s.hunk_row(0, s.files()[0].hunks.len()), None, "past the last hunk");
        s.set_collapsed(0, true);
        assert_eq!(s.hunk_row(0, 0), None, "a collapsed file's hunks have no rows");
    }
}

#[test]
fn row_of_line_finds_every_line_a_row_shows() {
    for mode in [DiffMode::Unified, DiffMode::Split] {
        let mut s = m();
        s.set_mode(mode);
        let found = lines_of(&s);
        let changed: usize = s.files().iter().map(|f| f.lines.len()).sum();
        assert!(found.len() >= changed, "{mode:?}: every hunk line has a row");
        for (f, _, r) in found {
            assert_eq!(s.row_file(r as usize), Some(f));
            assert!(s.row_key(r as usize).unwrap().starts_with(&format!("df{f}.")));
        }
        assert_eq!(s.row_of_line(0, LineRef::new(Side::New, 0)), None);
        assert_eq!(s.row_of_line(0, LineRef::new(Side::New, 9_999_999)), None);
        assert_eq!(s.row_of_line(9_999, LineRef::new(Side::New, 1)), None);
    }
}

#[test]
fn annotation_rows_list_each_placed_annotation_once_in_row_order() {
    let mut s = m();
    let added = |s: &DiffState, f: usize| {
        let l = s.files()[f].lines.iter().find(|l| l.kind == LineKind::Added).unwrap();
        LineRef::new(Side::New, l.new)
    };
    let (a0, a3) = (added(&s, 0), added(&s, 3));
    s.set_annotations(vec![
        Annotation::line(3, a3, "late"),
        Annotation::line(0, a0, "first"),
        Annotation::line(0, a0, "second"),
        Annotation::line(1, LineRef::new(Side::New, 9_999_999), "folded"),
        Annotation::new(
            0,
            Anchor::Range {
                side: Side::New,
                start: a0.line.saturating_sub(1).max(1),
                end: a0.line,
            },
            "range",
        ),
    ]);
    let r0 = s.row_of_line(0, a0).unwrap();
    let r3 = s.row_of_line(3, a3).unwrap();
    assert_eq!(
        s.annotation_rows(),
        vec![(r0, "first"), (r0, "second"), (r0, "range"), (r3, "late")]
    );
    s.set_collapsed(3, true);
    assert_eq!(
        s.annotation_rows().iter().filter(|a| a.1 == "late").count(),
        0,
        "a collapsed file places none"
    );
}

fn numbered(n: u32) -> String {
    (1..=n).map(|i| format!("line {i}\n")).collect()
}

/// 100 old lines: two lines changed, three inserted, one deleted, and unchanged ranges before,
/// between and after the hunks (the last one only known from the whole texts).
fn texts() -> (String, String) {
    let old = numbered(100);
    let mut new: Vec<String> = old.lines().map(str::to_owned).collect();
    new[4] = "five".into();
    new[49] = "fifty".into();
    new.remove(89);
    for k in 0..3 {
        new.insert(70 + k, format!("inserted {k}"));
    }
    (old, new.join("\n") + "\n")
}

fn without_texts(f: FileDiff) -> FileDiff {
    FileDiff {
        old_text: None,
        new_text: None,
        ..f
    }
}

fn nothing_hidden(s: &DiffState, f: usize) -> bool {
    (0..=s.files()[f].hunks.len()).all(|g| s.hidden(f, g) == Some(0))
}

#[test]
fn a_whole_file_shows_every_line_and_folds_back() {
    let (old, new) = texts();
    let mut s = DiffState::new(vec![from_texts("a.txt", &old, &new)]);
    let folded = s.row_count();
    let adds = s.files()[0].additions as usize;
    assert_eq!((adds, s.files()[0].deletions), (5, 3));
    assert!(
        s.hidden(0, s.files()[0].hunks.len()).is_some_and(|n| n > 0),
        "a trailing range to open"
    );
    assert!(!s.whole(0));

    s.set_whole(0, true);
    assert!(s.whole(0) && nothing_hidden(&s, 0));
    assert_eq!(
        s.row_count(),
        1 + 100 + adds,
        "unified: the header, every old line, every added line"
    );
    let last = s.row_of_line(0, LineRef::new(Side::Old, 100)).unwrap();
    assert_eq!(last as usize, s.row_count() - 1, "the trailing range is open");
    assert!(s.row_of_line(0, LineRef::new(Side::New, 1)).is_some());

    s.set_mode(DiffMode::Split);
    assert_eq!(
        s.row_count(),
        1 + 100 + 3,
        "split: changed lines pair, the three inserted stand alone"
    );
    s.set_mode(DiffMode::Unified);

    s.set_texts(0, Some(Arc::from(old.as_str())), Some(Arc::from(new.as_str())));
    assert!(s.whole(0) && nothing_hidden(&s, 0), "new texts keep it whole");
    assert_eq!(s.row_count(), 1 + 100 + adds);

    s.set_whole(0, false);
    assert!(!s.whole(0));
    assert_eq!(s.row_count(), folded);

    s.set_whole(0, true);
    s.collapse_expanded(0);
    assert!(!s.whole(0), "folding the ranges ends the whole-file view");
    assert_eq!(s.row_count(), folded);
}

#[test]
fn a_whole_file_asked_for_before_its_texts_opens_when_they_arrive() {
    let (old, new) = texts();
    let mut s = DiffState::new(vec![without_texts(from_texts("a.txt", &old, &new))]);
    let folded = s.row_count();
    assert!(!s.needs_texts(0));
    s.set_whole(0, true);
    assert!(s.whole(0) && s.needs_texts(0));
    assert_eq!(s.row_count(), folded, "nothing to show yet");
    s.set_texts(0, Some(Arc::from(old.as_str())), Some(Arc::from(new.as_str())));
    assert!(!s.needs_texts(0) && nothing_hidden(&s, 0));
    assert_eq!(s.row_count(), 1 + 100 + 5);

    s.set_texts(0, None, None);
    assert!(s.whole(0) && s.needs_texts(0), "dropping the texts keeps the ask");
    assert_eq!(s.row_count(), folded);
    s.set_whole(0, false);
    assert!(!s.needs_texts(0));
}

#[test]
fn expand_all_opens_the_files_it_picks() {
    let (old, new) = texts();
    let mut s = DiffState::new(vec![
        from_texts("a.txt", &old, &new),
        from_texts("skip.txt", &old, &new),
        without_texts(from_texts("c.txt", &old, &new)),
    ]);
    s.set_whole_where(true, |f| f.path != "skip.txt");
    assert!(s.whole(0) && !s.whole(1) && s.whole(2));
    assert!(nothing_hidden(&s, 0) && !nothing_hidden(&s, 1));
    assert!(s.needs_texts(2) && !s.needs_texts(0));
    s.set_whole_where(false, |_| true);
    assert!((0..3).all(|f| !s.whole(f) && !s.needs_texts(f)));
}

#[test]
fn opened_state_carries_to_another_diff_over_the_same_files() {
    let (old, new) = texts();
    let a = from_texts("a.txt", &old, &new);
    let b = from_texts("b.txt", &old, &new);
    let mut full = DiffState::new(vec![a.clone(), b.clone()]);
    full.set_whole(0, true);
    full.expand(1, 0, rain_diffview::Expand::Down);
    let opened_b = full.opened(1).unwrap();
    assert!(!opened_b.whole && opened_b.revealed.iter().any(|&r| r != (0, 0)));

    let mut sub = DiffState::new(vec![b, a]);
    let folded = sub.row_count();
    sub.set_opened([(0, 1), (1, 0)].map(|(l, g)| (l, full.opened(g).unwrap())));
    assert!(sub.whole(1) && nothing_hidden(&sub, 1));
    assert!(!sub.whole(0));
    assert_eq!(sub.opened(0).unwrap(), opened_b);
    assert!(sub.row_count() > folded);
}

#[test]
fn row_y_adds_the_taller_rows_above() {
    let files = (0..3).map(|i| from_texts(&format!("f{i}"), "a\n", "b\n")).collect();
    let s = DiffState::new(files);
    assert_eq!(s.row_count(), 9);
    assert_eq!(s.row_y(0), 0.0);
    assert_eq!(s.row_y(1), 38.0);
    assert_eq!(s.row_y(3), 38.0 + 40.0);
    assert_eq!(s.rows_height(), 9.0 * 20.0 + 3.0 * 18.0);
}

// ---- scroll hooks ----

thread_local! {
    static TOLD: Cell<u32> = const { Cell::new(0) };
}

struct App {
    diff: DiffState,
    at: Live<[f32; 4]>,
}

fn view(s: &App, _: &mut Cx<App>) -> El {
    let k = Kit::neutral(true);
    let diff = DiffView::new(|s: &App| &s.diff, |s: &mut App| &mut s.diff)
        .scroll_live(s.at.clone())
        .on_scroll_offset(|_| TOLD.with(|t| t.set(t.get() + 1)))
        .build(s, &k);
    col().w_full().h_full().child(diff)
}

fn settle(rt: &mut Runtime<App>) {
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
}

#[test]
fn scroll_live_follows_the_viewport_and_scroll_to_y_moves_it() {
    let old = numbered(400);
    let new = old.replace("line 10\n", "ten\n").replace("line 390\n", "three ninety\n");
    let mut diff = DiffState::new(vec![from_texts("a.txt", &old, &new)]);
    diff.set_whole(0, true);
    let at = Live::new([-1.0; 4]);
    let mut rt = Runtime::new(App { diff, at: at.clone() }, view, (800.0, 600.0), Env::new(true, 2.0));
    rt.frame(0.0);
    settle(&mut rt);
    let [offset, max, viewport, drawn] = at.get();
    assert_eq!(offset, 0.0);
    assert!(max > 6000.0, "the whole file's height less the viewport: {max}");
    assert_eq!((viewport, drawn), (600.0, 600.0));
    let told = TOLD.with(Cell::get);
    assert!(told >= 1, "the callback hears the first frame");

    rt.update(|_, tx| tx.scroll_to_y(SCROLL_KEY, 1000.0));
    rt.frame(1.0 / 60.0);
    assert_eq!(at.get()[0], 1000.0, "the frame that scrolls reports it");
    assert!(TOLD.with(Cell::get) > told);
    settle(&mut rt);
    assert_eq!(at.get()[0], 1000.0);
    let top = rt.state.diff.visible_rows().start;
    assert!(top > 30, "the virtual list moved with it: {top}");
    assert!(
        (rt.state.diff.row_y(top) - 1000.0).abs() < 2.0 * 20.0,
        "row_y estimates the scrolled offset"
    );

    rt.update(|_, tx| tx.scroll_to_y(SCROLL_KEY, 1e9));
    settle(&mut rt);
    let [offset, max, ..] = at.get();
    assert_eq!(offset, max, "clamped to the end");
}

/// Every row's key leads back to the row.
fn assert_keys_round_trip(s: &DiffState, what: &str) {
    for r in 0..s.row_count() {
        let key = s.row_key(r).unwrap();
        assert_eq!(s.row_of_key(&key), Some(r), "{what}: {key}");
    }
}

#[test]
fn row_of_key_finds_the_row_of_every_key() {
    for mode in [DiffMode::Unified, DiffMode::Split] {
        let mut s = m();
        s.set_mode(mode);
        assert_keys_round_trip(&s, "fixture m");
        s.set_collapsed(1, true);
        assert_keys_round_trip(&s, "a folded file");
        assert_eq!(s.row_of_key("df1.g0"), None, "a folded file keeps only its header row");
        assert_eq!(s.row_of_key("df1"), Some(s.file_rows()[1] as usize));

        let (old, new) = texts();
        let mut s = DiffState::new(vec![from_texts("a.txt", &old, &new), from_texts("b.txt", &old, &new)]);
        s.set_mode(mode);
        assert_keys_round_trip(&s, "two files");
        s.expand(0, 1, rain_diffview::Expand::Up);
        s.expand(1, 1, rain_diffview::Expand::Down);
        assert_keys_round_trip(&s, "part of a range opened");
        s.set_whole(1, true);
        assert_keys_round_trip(&s, "a whole file");
    }
    let s = m();
    for bad in [
        "",
        "df",
        "dfx",
        "df0.",
        "df0.c",
        "df0.cx",
        "df0.z1",
        "df0.note1",
        "df999999",
        "df0.o4000000000",
        "row",
    ] {
        assert_eq!(s.row_of_key(bad), None, "{bad:?}");
    }
}

/// `files` files that each replace `lines` lines, as a patch.
fn big_patch(files: usize, lines: usize) -> String {
    let mut out = String::new();
    for f in 0..files {
        out += &format!("diff --git a/src/Acme.Shop/File{f}.cs b/src/Acme.Shop/File{f}.cs\n");
        out += &format!("--- a/src/Acme.Shop/File{f}.cs\n+++ b/src/Acme.Shop/File{f}.cs\n@@ -1,{lines} +1,{lines} @@\n");
        for l in 0..lines {
            out += &format!("-old {f} {l}\n");
        }
        for l in 0..lines {
            out += &format!("+new {f} {l}\n");
        }
    }
    out
}

// A budget: 60 MB of state for a diff of 1.36 million lines. Raising it needs a reason in the commit.
const STATE_BYTES_PER_LINE: usize = 44;

fn shown(rt: &Runtime<App>, key: &str) -> bool {
    rt.elements()
        .into_iter()
        .any(|e| e.key.as_deref() == Some(key) && !e.exiting && e.visible[3] > 0.0)
}

#[test]
fn the_state_of_a_large_diff_stays_within_its_bytes_per_line() {
    let files = parse(&big_patch(40, 2500));
    let lines: usize = files.iter().map(|f| f.lines.len()).sum();
    assert_eq!(lines, 200_000);
    let mut s = DiffState::new(files);
    for mode in [DiffMode::Unified, DiffMode::Split] {
        s.set_mode(mode);
        let per_line = s.heap_bytes() as f64 / lines as f64;
        println!(
            "{mode:?}: {} rows, {} bytes, {per_line:.1} bytes a diff line",
            s.row_count(),
            s.heap_bytes()
        );
        assert!(
            s.heap_bytes() <= STATE_BYTES_PER_LINE * lines,
            "{mode:?}: {per_line:.1} bytes a diff line, over {STATE_BYTES_PER_LINE}"
        );
    }
}

#[test]
fn row_keys_are_built_for_the_rows_near_the_viewport_not_for_every_row() {
    let diff = DiffState::new(parse(&big_patch(40, 2500)));
    let rows = diff.row_count();
    assert!(rows >= 200_000, "{rows} rows");
    let keys = rain_diffview::row_keys_built;
    let t0 = keys();
    let at = Live::new([0.0; 4]);
    let mut rt = Runtime::new(App { diff, at }, view, (800.0, 600.0), Env::new(true, 1.0));
    rt.frame(0.0);
    let first = keys() - t0;
    settle(&mut rt);
    let settled = keys() - t0;
    println!("{rows} rows: {first} keys in the first frame, {settled} once the overscan filled");
    assert!(first < 400, "the first frame built {first} keys for {rows} rows");
    assert!(settled < 1000, "filling the overscan built {settled} keys");

    // Folding a file moves every row below it: the list keys what it shows again, no more.
    let t0 = keys();
    rt.update(|s, _| s.diff.set_collapsed(0, true));
    rt.frame(1.0 / 60.0);
    settle(&mut rt);
    let folded = keys() - t0;
    println!("folding the first file: {folded} keys");
    assert!(folded < 1500, "a fold built {folded} keys");
    assert_eq!(rt.state.diff.row_count(), rows - (rows / 40 - 1), "the file keeps its header row");
    assert!(shown(&rt, "df0") && shown(&rt, "df1"), "the folded header and the next file show");

    // A far row is found by its key without the rows between being keyed.
    let t0 = keys();
    let last = rt.state.diff.row_key(rt.state.diff.row_count() - 1).unwrap();
    let key = last.clone();
    rt.update(move |_, tx| {
        tx.instant();
        tx.scroll_into_view(key.clone());
    });
    rt.frame(1.0 / 60.0);
    assert!(shown(&rt, &last), "the last row shows a frame after it was asked for");
    settle(&mut rt);
    let jumped = keys() - t0;
    println!("jumping to the last row: {jumped} keys");
    assert!(jumped < 1500, "a jump built {jumped} keys");

    // Opening the file again, far above, keeps the reader's row where it was.
    let top = |rt: &Runtime<App>| {
        let row = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(last.as_str()) && !e.exiting);
        row.expect("the last row is mounted").rect[1]
    };
    let before = top(&rt);
    let t0 = keys();
    rt.update(|s, _| s.diff.set_collapsed(0, false));
    rt.frame(1.0 / 60.0);
    settle(&mut rt);
    let opened = keys() - t0;
    println!("opening it again: {opened} keys");
    assert!(opened < 1500, "an unfold built {opened} keys");
    assert_eq!(rt.state.diff.row_count(), rows);
    assert!(
        (top(&rt) - before).abs() < 0.5,
        "the reader's row stayed put: {before} to {}",
        top(&rt)
    );
}
