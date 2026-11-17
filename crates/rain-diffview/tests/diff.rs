//! `rain_diffview`: patches parse into core files of hunks, texts diff line by line and word
//! by word (lazily), folded ranges open, and `DiffView` lays annotations under their lines,
//! marks range and span comments, builds only the rows in view, selects lines from the gutter,
//! reports text spans selected over the code, moves a line cursor and copies one side's text.
//! Ported from rune-kit's `tests/diff.rs`.

mod common;

use std::fmt::Write;

use common::patch::{from_texts, parse};
use rain_core::comments::Anchor;
use rain_core::diff::{FileDiff, LineKind, Side};
use rain_diffview::{
    Align, Annotation, DiffMode, DiffState, DiffView, Expand, Layout, LineRange, LineRef, SpanSelection, Spot, hunk_header,
};
use rune::runtime::ElementInfo;
use rune::{Cx, El, Env, Key, KeyEvent, Mods, Runtime, TextEdit, TextStyle, Tx, col, el, text_input};
use rune_kit::Kit;

/// Files, hunks and lines as text: `-`/`+`/space, the old and new numbers, and word changes in
/// brackets.
fn dump(s: &DiffState) -> String {
    let mut out = String::new();
    for (fi, f) in s.files().iter().enumerate() {
        write!(out, "{:?} {:?} {:?}", f.status, f.old_path, f.path).unwrap();
        if f.binary {
            out.push_str(" binary");
        }
        writeln!(out, " +{} -{}", f.additions, f.deletions).unwrap();
        for h in &f.hunks {
            writeln!(out, "  {}", hunk_header(f, h)).unwrap();
            for i in h.lines.clone() {
                let l = &f.lines[i as usize];
                let mark = match l.kind {
                    LineKind::Added => '+',
                    LineKind::Removed => '-',
                    LineKind::Context => ' ',
                };
                let n = |n: u32| if n == 0 { String::from(".") } else { n.to_string() };
                let mut text = f.line_text(i).to_owned();
                for r in s.word_changes(fi, i).iter().rev() {
                    text.insert(r.end, ']');
                    text.insert(r.start, '[');
                }
                let eof = if l.no_newline { " (no newline)" } else { "" };
                writeln!(out, "  {mark} {:>2} {:>2} {text}{eof}", n(l.old), n(l.new)).unwrap();
            }
        }
    }
    out
}

const PATCH: &str = r#"diff --git a/src/old name.rs b/src/new name.rs
similarity index 90%
rename from src/old name.rs
rename to src/new name.rs
index 1111111..2222222 100644
--- a/src/old name.rs
+++ b/src/new name.rs
@@ -1,3 +1,3 @@ fn main() {
 fn main() {
-    println!("hi");
+    println!("hello");
 }
diff --git a/run.sh b/run.sh
old mode 100644
new mode 100755
diff --git a/logo.png b/logo.png
index 3333333..4444444 100644
Binary files a/logo.png and b/logo.png differ
diff --git a/README b/README
index 5555555..6666666 100644
--- a/README
+++ b/README
@@ -1,2 +1,2 @@
 first
-last
\ No newline at end of file
+last line
\ No newline at end of file
diff --git "a/caf\303\251.txt" "b/caf\303\251.txt"
new file mode 100644
index 0000000..7777777
--- /dev/null
+++ "b/caf\303\251.txt"
@@ -0,0 +1 @@
+bonjour
diff --git a/gone.txt b/gone.txt
deleted file mode 100644
index 8888888..0000000
--- a/gone.txt
+++ /dev/null
@@ -1 +0,0 @@
-bye
"#;

#[test]
fn a_patch_parses_renames_binaries_and_missing_newlines() {
    let golden = r#"Renamed Some("src/old name.rs") "src/new name.rs" +1 -1
  @@ -1,3 +1,3 @@ fn main() {
     1  1 fn main() {
  -  2  .     println!("[hi]");
  +  .  2     println!("[hello]");
     3  3 }
Modified None "run.sh" +0 -0
Modified None "logo.png" binary +0 -0
Modified None "README" +1 -1
  @@ -1,2 +1,2 @@
     1  1 first
  -  2  . last (no newline)
  +  .  2 last[ line] (no newline)
Added None "café.txt" +1 -0
  @@ -0,0 +1 @@
  +  .  1 bonjour
Deleted None "gone.txt" +0 -1
  @@ -1 +0,0 @@
  -  1  . bye
"#;
    assert_eq!(dump(&DiffState::new(parse(PATCH))), golden);
}

#[test]
fn two_texts_diff_into_hunks_with_word_changes() {
    let old: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    let new = old.replace("line 10\n", "line ten\n").replace("line 20\n", "line 20\nline 21\n");
    let f = from_texts("a.txt", &old, &new);
    let golden = r#"Modified None "a.txt" +2 -1
  @@ -7,7 +7,7 @@ line 6
     7  7 line 7
     8  8 line 8
     9  9 line 9
  - 10  . line [10]
  +  . 10 line [ten]
    11 11 line 11
    12 12 line 12
    13 13 line 13
  @@ -18,3 +18,4 @@ line 17
    18 18 line 18
    19 19 line 19
    20 20 line 20
  +  . 21 line 21
"#;
    assert_eq!(dump(&DiffState::new(vec![f])), golden);
}

#[test]
fn folded_ranges_open_twenty_lines_at_a_time() {
    let old: String = (1..=100).map(|i| format!("{i}\n")).collect();
    let new = old.replace("\n50\n", "\nfifty\n");
    let mut s = DiffState::new(vec![from_texts("n.txt", &old, &new)]);
    assert_eq!((s.hidden(0, 0), s.hidden(0, 1)), (Some(46), Some(47)));
    s.expand(0, 0, Expand::Up);
    assert_eq!(s.hidden(0, 0), Some(26));
    s.expand(0, 1, Expand::Down);
    s.expand(0, 1, Expand::Down);
    assert_eq!(s.hidden(0, 1), Some(7));
    s.expand(0, 1, Expand::Down);
    assert_eq!(s.hidden(0, 1), Some(0));
    s.expand(0, 0, Expand::All);
    assert_eq!(s.hidden(0, 0), Some(0));
    // A parsed patch has no whole texts: its ranges stay folded.
    let mut p = DiffState::new(parse(PATCH));
    p.expand(0, 1, Expand::All);
    assert_eq!(p.hidden(0, 1), None);
}

// ---- the view ----

struct App {
    diff: DiffState,
    selections: Vec<Option<LineRange>>,
    comments: Vec<LineRange>,
    spans: Vec<Option<SpanSelection>>,
    visible: Vec<usize>,
    actions: Vec<usize>,
}

impl App {
    fn new(diff: DiffState) -> App {
        App {
            diff,
            selections: Vec::new(),
            comments: Vec::new(),
            spans: Vec::new(),
            visible: Vec::new(),
            actions: Vec::new(),
        }
    }
}

const NOTE_H: f32 = 90.0;

fn view(s: &App, _: &mut Cx<App>) -> El {
    let k = Kit::neutral(true);
    let diff = DiffView::new(|s: &App| &s.diff, |s: &mut App| &mut s.diff)
        .annotation(|_: &App, a: &Annotation| el().key(format!("note-{}", a.id)).w_full().h(NOTE_H))
        .header_actions(|_: &App, f| el().key("act").size(28.0, 20.0).on_press(move |s: &mut App| s.actions.push(f)))
        .on_select(|s: &mut App, sel, _| s.selections.push(sel))
        .on_comment(|s: &mut App, lines, _| s.comments.push(lines))
        .on_span_select(|s: &mut App, span, _| s.spans.push(span))
        .on_visible_file(|s: &mut App, f| s.visible.push(f))
        .build(s, &k);
    col().w_full().h_full().child(diff)
}

fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    s.diff.key(ev, tx)
}

fn runtime(diff: DiffState, size: (f32, f32)) -> Runtime<App> {
    let mut rt = Runtime::new(App::new(diff), view, size, Env::new(true, 2.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    settle(&mut rt);
    rt
}

fn tick(rt: &mut Runtime<App>) {
    rt.frame(1.0 / 60.0);
}

/// Frames until scrolling glides and layout reports have settled.
fn settle(rt: &mut Runtime<App>) {
    for _ in 0..90 {
        tick(rt);
    }
}

/// The element whose path ends with `/{end}`.
fn find(rt: &Runtime<App>, end: &str) -> ElementInfo {
    let suffix = format!("/{end}");
    rt.elements()
        .into_iter()
        .find(|e| e.path.ends_with(&suffix) && !e.exiting)
        .unwrap_or_else(|| panic!("no {end}"))
}

fn has(rt: &Runtime<App>, end: &str) -> bool {
    let suffix = format!("/{end}");
    rt.elements().into_iter().any(|e| e.path.ends_with(&suffix) && !e.exiting)
}

fn rows(rt: &Runtime<App>) -> Vec<ElementInfo> {
    rt.elements()
        .into_iter()
        .filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("df")) && !e.exiting)
        .collect()
}

fn bottom(e: &ElementInfo) -> f32 {
    e.rect[1] + e.rect[3]
}

fn numbered(n: usize, every: usize) -> (String, String) {
    let old: String = (1..=n).map(|i| format!("let value_{i} = compute({i});\n")).collect();
    let new: String = (1..=n)
        .map(|i| {
            if i % every == 0 {
                format!("let value_{i} = compute({i}) + 1;\n")
            } else {
                format!("let value_{i} = compute({i});\n")
            }
        })
        .collect();
    (old, new)
}

fn text_key(c: &str) -> KeyEvent {
    KeyEvent {
        key: Key::Text(c.into()),
        mods: Mods::default(),
    }
}

#[test]
fn annotations_sit_under_their_line_and_push_the_rest_down() {
    let (old, new) = numbered(30, 10);
    let mut d = DiffState::new(vec![from_texts("a.rs", &old, &new)]);
    d.set_annotations(vec![
        Annotation::line(0, LineRef::new(Side::New, 10), "c1"),
        Annotation::line(0, LineRef::new(Side::New, 10), "c2"),
    ]);
    let mut rt = runtime(d, (900.0, 1200.0));
    let line = find(&rt, "df0.n10/line");
    let (c1, c2) = (find(&rt, "note-c1"), find(&rt, "note-c2"));
    assert_eq!(line.rect[3], 20.0);
    assert_eq!(find(&rt, "df0.c9").rect[2], 900.0, "short lines still fill the view's width");
    assert_eq!(c1.rect[1], bottom(&line), "the first note right under its line");
    assert_eq!(c2.rect[1], bottom(&c1), "the next under it");
    assert_eq!(find(&rt, "df0.c11").rect[1], bottom(&c2), "the next line after both");

    // Split: across both sides, as in unified.
    rt.state.diff.set_mode(DiffMode::Split);
    rt.invalidate();
    rt.frame(1.0);
    let line = find(&rt, "df0.o10/line");
    let c1 = find(&rt, "note-c1");
    assert_eq!(c1.rect[1], bottom(&line));
    assert!(c1.rect[0] == 0.0 && c1.rect[2] == 900.0, "the full width: {:?}", c1.rect);
    assert!(rt.state.diff.annotations().len() == 2);
}

#[test]
fn range_and_span_comments_mark_their_lines_and_sit_under_the_last() {
    let (old, new) = numbered(30, 10);
    let mut d = DiffState::new(vec![from_texts("a.rs", &old, &new)]);
    d.set_annotations(vec![
        Annotation::new(
            0,
            Anchor::Range {
                side: Side::New,
                start: 7,
                end: 9,
            },
            "r",
        ),
        Annotation::new(
            0,
            Anchor::Span {
                side: Side::New,
                start_line: 11,
                start_col: 4,
                end_line: 12,
                end_col: 9,
            },
            "s",
        ),
        Annotation::line(0, LineRef::new(Side::New, 13), "l"),
    ]);
    let mut rt = runtime(d, (900.0, 1400.0));
    let unified_keys: Vec<String> = rows(&rt).iter().filter_map(|e| e.key.clone()).collect();
    for (end, line) in [("note-r", "df0.c9/line"), ("note-s", "df0.c12/line"), ("note-l", "df0.c13/line")] {
        assert_eq!(find(&rt, end).rect[1], bottom(&find(&rt, line)), "{end} under {line}");
    }
    for key in [
        "df0.c7/gutter/range",
        "df0.c8/gutter/range",
        "df0.c9/line/gutter/range",
        "df0.c11/gutter/range",
        "df0.c12/line/gutter/range",
    ] {
        assert!(has(&rt, key), "{key} marked");
    }
    assert!(!has(&rt, "df0.n10/gutter/range") && !has(&rt, "df0.o10/gutter/range") && !has(&rt, "df0.c13/line/gutter/range"));
    let bar = find(&rt, "df0.c8/gutter/range");
    assert_eq!(bar.rect[3], 20.0, "the bar spans the line");

    // Split: the same rows, keyed alike, with the marks on the new half.
    rt.state.diff.set_mode(DiffMode::Split);
    rt.invalidate();
    settle(&mut rt);
    let split_keys: Vec<String> = rows(&rt).iter().filter_map(|e| e.key.clone()).collect();
    for k in ["df0.c7", "df0.c9", "df0.c12", "df0.c13", "df0.o10"] {
        assert!(
            unified_keys.iter().any(|u| u == k) && split_keys.iter().any(|u| u == k),
            "{k} kept its key"
        );
    }
    assert!(has(&rt, "df0.c8/new/range") && !has(&rt, "df0.c8/old/range"));
    assert_eq!(find(&rt, "note-s").rect[1], bottom(&find(&rt, "df0.c12/line")));
}

#[test]
fn a_long_diff_builds_only_the_rows_in_view_and_measures_tall_ones() {
    let (old, new) = numbered(4000, 3);
    let mut d = DiffState::new(vec![from_texts("long.rs", &old, &new)]);
    d.set_annotations(
        (1..=40)
            .map(|i| Annotation::line(0, LineRef::new(Side::New, i * 99), format!("n{i}")))
            .collect(),
    );
    let mut rt = runtime(d, (800.0, 600.0));
    let built = rows(&rt).len();
    assert!(built < 100, "{built} rows built for a 600pt view of 5300: the view and two below");
    for _ in 0..40 {
        rt.wheel(400.0, 300.0, -100_000.0);
        rt.frame(1.0 / 120.0);
    }
    settle(&mut rt);
    let last = find(&rt, "df0.c4000");
    assert!((bottom(&last) - 600.0).abs() < 1.0, "the last line ends the list: {:?}", last.rect);
    assert!(rows(&rt).len() < 160, "at most five views of rows: the view and two either side");
    let note = find(&rt, "note-n40");
    let line = find(&rt, "df0.n3960/line");
    assert_eq!(note.rect[1], bottom(&line));
    for _ in 0..40 {
        rt.wheel(400.0, 300.0, 100_000.0);
        rt.frame(1.0 / 120.0);
    }
    settle(&mut rt);
    assert_eq!(find(&rt, "df0").rect[1], 0.0);
}

/// Unified diff of a tabbed file: a removed line, an added one and context around them.
fn tabbed() -> DiffState {
    let old = "fn f() {\n\tlet a = 1;\n\tlet b = 2;\n\ta + b\n}\n";
    let new = "fn f() {\n\tlet a = 1;\n\tlet b = 3;\n\ta + b\n}\n";
    DiffState::new(vec![from_texts("f.rs", old, new)])
}

/// Presses at the start of `from`'s text and drags to 200pt into `to`'s.
fn drag_text(rt: &mut Runtime<App>, from: &str, to: &str) {
    let (from, to) = (find(rt, from), find(rt, to));
    let (x0, y0) = (from.rect[0] + 1.0, from.rect[1] + 10.0);
    let (x1, y1) = (to.rect[0] + 200.0, to.rect[1] + 10.0);
    rt.pointer_move(x0, y0);
    tick(rt);
    rt.pointer_down(x0, y0);
    tick(rt);
    for i in 1..=8 {
        let t = i as f32 / 8.0;
        rt.pointer_move(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t);
        tick(rt);
    }
    rt.pointer_up(x1, y1);
    tick(rt);
}

#[test]
fn copying_lines_takes_one_side_without_numbers_or_markers() {
    let mut d = tabbed();
    d.set_selection(Some(LineRange {
        file: 0,
        start: LineRef::new(Side::Old, 2),
        end: LineRef::new(Side::New, 4),
    }));
    assert_eq!(
        d.selected_text().as_deref(),
        Some("\tlet a = 1;\n\tlet b = 2;\n\ta + b"),
        "the old side's lines"
    );
    d.set_selection(Some(LineRange {
        file: 0,
        start: LineRef::new(Side::New, 2),
        end: LineRef::new(Side::New, 4),
    }));
    assert_eq!(
        d.selected_text().as_deref(),
        Some("\tlet a = 1;\n\tlet b = 3;\n\ta + b"),
        "the new side's"
    );

    let mut rt = runtime(tabbed(), (800.0, 400.0));
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    drag_text(&mut rt, "df0.c2/code/text", "df0.c4/code/text");
    assert_eq!(rt.selected_text().as_deref(), Some("\tlet a = 1;\n\tlet b = 3;\n\ta + b"));
    rt.key(KeyEvent {
        key: Key::Command('c'),
        mods: Mods {
            cmd: true,
            ..Mods::default()
        },
    });
    tick(&mut rt);
    assert_eq!(
        clipboard.text().as_deref().map(String::as_str),
        Some("\tlet a = 1;\n\tlet b = 3;\n\ta + b")
    );

    let mut split_diff = tabbed();
    split_diff.set_mode(DiffMode::Split);
    let mut rt = runtime(split_diff, (800.0, 400.0));
    rt.use_clipboard(&clipboard);
    for (cell_start, cell_end, expected) in [
        (
            "df0.c2/old/code/text",
            "df0.c4/old/code/text",
            "\tlet a = 1;\n\tlet b = 2;\n\ta + b",
        ),
        (
            "df0.c2/new/code/text",
            "df0.c4/new/code/text",
            "\tlet a = 1;\n\tlet b = 3;\n\ta + b",
        ),
    ] {
        drag_text(&mut rt, cell_start, cell_end);
        assert_eq!(rt.selected_text().as_deref(), Some(expected));
    }

    let old: String = (1..=30).map(|i| format!("\tline {i}\n")).collect();
    let new = old
        .replace("\tline 2\n", "\tline two\n")
        .replace("\tline 29\n", "\tline twenty-nine\n");
    let mut across = DiffState::new(vec![from_texts("gap.rs", &old, &new)]);
    assert_eq!(across.files()[0].hunks.len(), 2);
    across.set_selection(Some(LineRange {
        file: 0,
        start: LineRef::new(Side::New, 1),
        end: LineRef::new(Side::New, 30),
    }));
    let copied = across.selected_text().unwrap();
    assert!(!copied.contains("unchanged"), "no gap label in copied text: {copied}");
    assert!(!copied.contains("@@"), "no hunk header in copied text: {copied}");
    assert!(copied.starts_with("\tline 1\n\tline two\n\tline 3\n"));
    assert!(copied.ends_with("\n\tline twenty-nine\n\tline 30"));
}

#[test]
fn dragging_over_code_reports_the_span_by_line_and_column() {
    let mut rt = runtime(tabbed(), (800.0, 400.0));
    drag_text(&mut rt, "df0.c2/code/text", "df0.c4/code/text");
    let want = SpanSelection {
        file: 0,
        side: Side::New,
        start_line: 2,
        start_col: 0,
        end_line: 4,
        end_col: 6,
        text: "\tlet a = 1;\n\tlet b = 3;\n\ta + b".into(),
    };
    assert_eq!(rt.state.diff.span_selection(), Some(&want));
    assert_eq!(rt.state.spans.last(), Some(&Some(want.clone())));
    assert_eq!(
        want.anchor(),
        Anchor::Span {
            side: Side::New,
            start_line: 2,
            start_col: 0,
            end_line: 4,
            end_col: 6
        }
    );

    // Part of one line: from inside `let` to just past `b`.
    let t = find(&rt, "df0.n3/code/text");
    let adv = 13.0 * 0.6;
    let y = t.rect[1] + 10.0;
    let (x0, x1) = (t.rect[0] + adv * 5.5, t.rect[0] + adv * 9.5);
    rt.pointer_move(x0, y);
    tick(&mut rt);
    rt.pointer_down(x0, y);
    tick(&mut rt);
    rt.pointer_move(x1, y);
    tick(&mut rt);
    rt.pointer_up(x1, y);
    tick(&mut rt);
    let span = rt.state.diff.span_selection().cloned().expect("a span within one line");
    assert_eq!((span.side, span.start_line, span.end_line), (Side::New, 3, 3));
    let line = "\tlet b = 3;";
    let picked: String = line
        .chars()
        .skip(span.start_col as usize)
        .take((span.end_col - span.start_col) as usize)
        .collect();
    assert_eq!(picked, span.text, "columns {}..{} of {line:?}", span.start_col, span.end_col);
    assert!(span.text.contains("et") || span.text.contains('b'), "{span:?}");

    // Escape drops it, and the text selection with it.
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    tick(&mut rt);
    assert_eq!(rt.state.diff.span_selection(), None);
    assert!(rt.selected_text().is_none());

    // Split: the old column's lines by their old numbers.
    let mut split = tabbed();
    split.set_mode(DiffMode::Split);
    let mut rt = runtime(split, (800.0, 400.0));
    drag_text(&mut rt, "df0.c2/old/code/text", "df0.o3/old/code/text");
    let span = rt.state.diff.span_selection().cloned().expect("a split span");
    assert_eq!(
        (span.side, span.start_line, span.start_col, span.end_line, span.end_col),
        (Side::Old, 2, 0, 3, 11)
    );
    assert_eq!(span.text, "\tlet a = 1;\n\tlet b = 2;");
}

fn hover(rt: &mut Runtime<App>, end: &str) {
    let e = find(rt, end);
    rt.pointer_move(e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0);
    tick(rt);
    tick(rt);
}

fn press(rt: &mut Runtime<App>, end: &str) {
    let e = find(rt, end);
    let (x, y) = (e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0);
    rt.pointer_move(x, y);
    tick(rt);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    tick(rt);
}

#[test]
fn only_the_hovered_line_mounts_the_plus_and_ask() {
    for mode in [DiffMode::Unified, DiffMode::Split] {
        let (old, new) = numbered(20, 5);
        let mut d = DiffState::new(vec![from_texts("a.rs", &old, &new)]);
        d.set_mode(mode);
        let mut rt = runtime(d, (900.0, 900.0));
        let adds = |rt: &Runtime<App>| -> Vec<String> {
            rt.elements()
                .into_iter()
                .filter(|e| e.path.ends_with("/hover-add/plus") && !e.exiting)
                .map(|e| e.path)
                .collect()
        };
        let code = |n: u32| match mode {
            DiffMode::Split => format!("df0.c{n}/new/code"),
            _ => format!("df0.c{n}/code"),
        };
        assert!(adds(&rt).is_empty(), "{mode:?}: no line is hovered: {:?}", adds(&rt));
        hover(&mut rt, &code(4));
        let on = adds(&rt);
        assert_eq!(on.len(), 1, "{mode:?}: {on:?}");
        assert!(on[0].contains("/df0.c4/"), "{mode:?}: {on:?}");
        hover(&mut rt, &code(9));
        let on = adds(&rt);
        assert_eq!(on.len(), 1, "{mode:?}: {on:?}");
        assert!(on[0].contains("/df0.c9/"), "{mode:?}: {on:?}");
    }
}

#[test]
fn line_numbers_select_lines_and_the_plus_comments_on_them() {
    let (old, new) = numbered(20, 5);
    let mut rt = runtime(DiffState::new(vec![from_texts("a.rs", &old, &new)]), (800.0, 900.0));
    press(&mut rt, "df0.c3/gutter/nn");
    let three = LineRange::line(0, LineRef::new(Side::New, 3));
    assert_eq!(rt.state.diff.selection(), Some(three));
    assert_eq!(rt.state.selections, [Some(three)]);
    let (a, b) = (find(&rt, "df0.c3/gutter/nn"), find(&rt, "df0.n5/gutter/nn"));
    let x = a.rect[0] + a.rect[2] / 2.0;
    let (y0, y1) = (a.rect[1] + 10.0, b.rect[1] + 10.0);
    rt.pointer_down(x, y0);
    tick(&mut rt);
    for i in 1..=6 {
        rt.pointer_move(x, y0 + (y1 - y0) * i as f32 / 6.0);
        tick(&mut rt);
    }
    rt.pointer_up(x, y1);
    tick(&mut rt);
    let range = LineRange {
        file: 0,
        start: LineRef::new(Side::New, 3),
        end: LineRef::new(Side::New, 5),
    };
    assert_eq!(rt.state.diff.selection(), Some(range));
    assert_eq!(rt.state.selections.last(), Some(&Some(range)));
    assert_eq!(
        range.anchor(),
        Anchor::Range {
            side: Side::New,
            start: 3,
            end: 5
        }
    );
    hover(&mut rt, "df0.c4/code");
    let plus = find(&rt, "df0.c4/gutter/mk/hover-add/plus");
    let line = find(&rt, "df0.c4/gutter/nn");
    assert_eq!(
        (plus.rect[2], plus.rect[3]),
        (20.0, line.rect[3]),
        "the + presses over the line's full height"
    );
    press(&mut rt, "df0.c4/gutter/mk/hover-add/plus");
    assert_eq!(rt.state.comments, [range]);
    hover(&mut rt, "df0.c9/code");
    press(&mut rt, "df0.c9/gutter/mk/hover-add/plus");
    assert_eq!(rt.state.comments[1], LineRange::line(0, LineRef::new(Side::New, 9)));
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    tick(&mut rt);
    assert_eq!(rt.state.diff.selection(), None);
}

#[test]
fn unified_file_headers_span_the_viewport_while_code_scrolls_sideways() {
    let long = "x".repeat(400);
    let text = |n: &str| {
        (0..60)
            .map(|i| {
                if i == 1 {
                    format!("{long}\n")
                } else {
                    format!("let v{i} = {}{n};\n", i % 2)
                }
            })
            .collect::<String>()
    };
    let (old, new) = (text(""), text("+1"));
    let files = (0..2).map(|i| from_texts(&format!("dir/f{i}.rs"), &old, &new)).collect();
    let mut d = DiffState::new(files);
    d.set_mode(DiffMode::Unified);
    let mut rt = Runtime::new(App::new(d), view, (600.0, 400.0), Env::new(true, 2.0));
    rt.on_key = Some(on_key);
    rt.frame(0.0);
    let inside = |rt: &Runtime<App>, w: f32, end: &str| {
        let r = find(rt, end).rect;
        assert!(r[0] >= -0.5 && r[0] + r[2] <= w + 0.5 && r[2] > 0.0, "{end} within {w}: {r:?}");
    };
    let check = |rt: &Runtime<App>, w: f32| {
        inside(rt, w, "df0/inner/stat/adds");
        inside(rt, w, "df0/inner/stat/dels");
        inside(rt, w, "df0/inner/actions");
        let h = find(rt, "df0");
        assert!(
            h.rect[0].abs() < 0.5 && (h.rect[2] - w).abs() < 0.5,
            "header spans {w}: {:?}",
            h.rect
        );
    };
    check(&rt, 600.0);
    settle(&mut rt);
    for dx in [-250.0, -250.0] {
        rt.wheel_xy(300.0, 200.0, dx, 0.0);
        settle(&mut rt);
        check(&rt, 600.0);
    }
    let code = find(&rt, "df0.c2/code").rect;
    assert!(code[0] < -100.0, "the code did scroll: {code:?}");
    press(&mut rt, "df0/inner/actions");
    assert_eq!(rt.state.actions, vec![0]);
    for w in [800.0, 450.0, 600.0] {
        rt.set_viewport((w, 400.0), 2.0);
        rt.frame(0.0);
        check(&rt, w);
    }
    press(&mut rt, "df0/inner/actions");
    assert_eq!(rt.state.actions, vec![0, 0]);
    rt.wheel(300.0, 200.0, -150.0);
    settle(&mut rt);
    let sticky = find(&rt, "sticky");
    assert!(
        sticky.rect[0].abs() < 0.5 && (sticky.rect[2] - 600.0).abs() < 0.5,
        "sticky spans the viewport: {:?}",
        sticky.rect
    );
    inside(&rt, 600.0, "sticky/inner/stat/adds");
    press(&mut rt, "sticky/inner/actions");
    assert_eq!(rt.state.actions, vec![0, 0, 0]);
}

#[test]
fn cancelling_a_gutter_drag_restores_selection_without_notifying_a_drop() {
    let (old, new) = numbered(20, 5);
    let mut rt = runtime(DiffState::new(vec![from_texts("a.rs", &old, &new)]), (800.0, 900.0));
    press(&mut rt, "df0.c3/gutter/nn");
    let original = rt.state.diff.selection();
    let calls = rt.state.selections.len();
    let a = find(&rt, "df0.c4/gutter/nn");
    let (x, y) = (a.rect[0] + a.rect[2] / 2.0, a.rect[1] + 10.0);
    rt.pointer_down(x, y);
    rt.pointer_move(x, y + 40.0);
    tick(&mut rt);
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    tick(&mut rt);
    assert_eq!(rt.state.diff.selection(), original);
    assert_eq!(rt.state.selections.len(), calls);
    rt.pointer_up(x, y + 40.0);
    tick(&mut rt);
    assert_eq!(rt.state.selections.len(), calls);
}

#[test]
fn unified_gutters_stay_at_the_left_edge_while_code_scrolls_sideways() {
    let long = "x".repeat(400);
    let text = |n: &str| {
        (0..30)
            .map(|i| {
                if i == 1 {
                    format!("{long}\n")
                } else {
                    format!("let v{i} = {}{n};\n", i % 2)
                }
            })
            .collect::<String>()
    };
    let mut d = DiffState::new(vec![from_texts("a.rs", &text(""), &text("+1"))]);
    d.set_mode(DiffMode::Unified);
    let mut rt = runtime(d, (600.0, 400.0));
    let before = find(&rt, "df0.o6/code").rect;
    for _ in 0..2 {
        rt.wheel_xy(300.0, 200.0, -250.0, 0.0);
        settle(&mut rt);
    }
    let code = find(&rt, "df0.o6/code").rect;
    assert!(code[0] < before[0] - 100.0, "the code scrolled: {before:?} -> {code:?}");
    for end in ["df0.o6/gutter", "df0.o6/gutter/no", "df0.o6/gutter/nn"] {
        let r = find(&rt, end).rect;
        assert!(r[0] >= -0.5 && r[0] <= 100.0, "{end} stays in view: {r:?}");
    }
    let g = find(&rt, "df0.o6/gutter").rect;
    assert!(g[0].abs() < 0.5, "gutter at the left edge: {g:?}");
    let e = |k: &str| rt.elements().into_iter().find(|e| e.path.ends_with(k) && !e.exiting).unwrap();
    assert!(e("df0.o6/gutter").stuck > 100.0);
    press(&mut rt, "df0.o6/gutter/nn");
    assert_eq!(rt.state.diff.selection(), Some(LineRange::line(0, LineRef::new(Side::Old, 6))));
    assert!(rt.selected_text().is_none(), "gutter press does not select code under the gutter");
    let start = find(&rt, "df0.o4/gutter/no");
    let x = start.rect[0] + start.rect[2] / 2.0;
    let y0 = start.rect[1] + 10.0;
    rt.pointer_down(x, y0);
    tick(&mut rt);
    rt.wheel(300.0, 200.0, -80.0);
    settle(&mut rt);
    let end = find(&rt, "df0.o10/gutter/no");
    let y1 = end.rect[1] + 10.0;
    rt.pointer_move(x, y1);
    tick(&mut rt);
    rt.pointer_up(x, y1);
    tick(&mut rt);
    assert_eq!(
        rt.state.diff.selection(),
        Some(LineRange {
            file: 0,
            start: LineRef::new(Side::Old, 4),
            end: LineRef::new(Side::Old, 10)
        }),
    );
}

#[test]
fn rows_read_as_their_change_and_keys_move_between_files() {
    let (old, new) = numbered(200, 50);
    let files = (0..3).map(|i| from_texts(&format!("f{i}.rs"), &old, &new)).collect();
    let mut rt = runtime(DiffState::new(files), (800.0, 400.0));
    let els = rt.elements();
    let label = |key: &str| els.iter().find(|e| e.key.as_deref() == Some(key)).and_then(|e| e.label.clone());
    assert_eq!(label("df0.n50").as_deref(), Some("added line 50: let value_50 = compute(50) + 1;"));
    assert_eq!(label("df0.o50").as_deref(), Some("removed line 50: let value_50 = compute(50);"));
    assert_eq!(label("df0.c49").as_deref(), Some("unchanged line 49: let value_49 = compute(49);"));
    assert!(
        els.iter()
            .any(|e| e.role == "heading" && e.label.as_deref() == Some("f0.rs, 4 added, 4 removed"))
    );

    rt.key(text_key("]"));
    settle(&mut rt);
    assert_eq!(rt.state.diff.visible_file(), 1);
    assert!(find(&rt, "df1").rect[1].abs() < 1.0, "the next file's header at the top");
    rt.key(text_key("n"));
    settle(&mut rt);
    assert_eq!(rt.state.diff.visible_file(), 1);
    rt.key(text_key("["));
    settle(&mut rt);
    assert_eq!(rt.state.diff.visible_file(), 0);
    assert_eq!(rt.state.visible.last(), Some(&0));
}

#[test]
fn the_line_cursor_moves_by_line_hunk_and_file() {
    let (old, new) = numbered(200, 50);
    let files = (0..3).map(|i| from_texts(&format!("f{i}.rs"), &old, &new)).collect();
    let mut rt = runtime(DiffState::new(files), (800.0, 400.0));
    let cursor = |rt: &Runtime<App>| rt.state.diff.cursor();
    assert_eq!(cursor(&rt), None);
    rt.key(text_key("j"));
    tick(&mut rt);
    assert_eq!(
        cursor(&rt),
        Some((0, LineRef::new(Side::New, 47))),
        "the first line of the first file"
    );
    rt.key(text_key("j"));
    rt.key(text_key("j"));
    rt.key(text_key("j"));
    tick(&mut rt);
    assert_eq!(
        cursor(&rt),
        Some((0, LineRef::new(Side::Old, 50))),
        "a removed line takes the old side"
    );
    rt.key(text_key("j"));
    tick(&mut rt);
    assert_eq!(cursor(&rt), Some((0, LineRef::new(Side::New, 50))));
    rt.key(text_key("k"));
    tick(&mut rt);
    assert_eq!(cursor(&rt), Some((0, LineRef::new(Side::Old, 50))));
    assert_eq!(
        rt.state.diff.comment_at_cursor(),
        Some(LineRange::line(0, LineRef::new(Side::Old, 50)))
    );
    settle(&mut rt);
    assert!(find(&rt, "df0.o50/cursor").rect[2] > 0.0, "the cursor's row is built");

    rt.key(text_key("n"));
    settle(&mut rt);
    assert_eq!(cursor(&rt), Some((0, LineRef::new(Side::New, 97))), "the next hunk's first line");
    rt.key(text_key("p"));
    settle(&mut rt);
    assert_eq!(cursor(&rt), Some((0, LineRef::new(Side::New, 47))));

    // Walking down past the view scrolls the cursor's line into it.
    for _ in 0..24 {
        rt.key(text_key("j"));
        tick(&mut rt);
    }
    settle(&mut rt);
    let (f, at) = cursor(&rt).unwrap();
    assert_eq!(f, 0, "still in the first file: {at:?}");
    let key = match at.side {
        Side::Old => format!("df{f}.o{}", at.line),
        Side::New => rows(&rt)
            .iter()
            .filter_map(|e| e.key.clone())
            .find(|k| k.ends_with(&format!(".n{}", at.line)) || k.ends_with(&format!(".c{}", at.line)))
            .unwrap_or_default(),
    };
    let r = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key.as_str()) && !e.exiting)
        .expect("cursor row built");
    assert!(r.rect[1] >= 0.0 && bottom(&r) <= 400.5, "cursor row in view: {:?}", r.rect);

    rt.key(text_key("]"));
    settle(&mut rt);
    assert_eq!(cursor(&rt), None);
    rt.key(text_key("j"));
    tick(&mut rt);
    assert_eq!(cursor(&rt), Some((1, LineRef::new(Side::New, 47))), "the next file's first line");
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    tick(&mut rt);
    assert_eq!(cursor(&rt), None);
}

#[test]
fn scroll_to_line_reaches_a_folded_line_and_puts_the_cursor_there() {
    let (old, new) = numbered(400, 200);
    let mut rt = runtime(DiffState::new(vec![from_texts("a.rs", &old, &new)]), (800.0, 400.0));
    rt.state.diff.scroll_to_line(0, LineRef::new(Side::New, 120), &mut Tx::default());
    assert_eq!(rt.state.diff.cursor(), Some((0, LineRef::new(Side::New, 120))));
    assert_eq!(rt.state.diff.hidden(0, 0), Some(0), "the range holding it opened");
}

#[test]
fn focused_annotation_input_keeps_navigation_and_escape_keys() {
    struct ComposeApp {
        diff: DiffState,
        comment: TextEdit,
    }
    fn compose_view(s: &ComposeApp, _: &mut Cx<ComposeApp>) -> El {
        let k = Kit::neutral(true);
        let diff = DiffView::new(|s: &ComposeApp| &s.diff, |s: &mut ComposeApp| &mut s.diff)
            .annotation(|s: &ComposeApp, a: &Annotation| {
                col()
                    .w_full()
                    .child(
                        text_input(&s.comment, TextStyle::new(13.0, 400.0))
                            .key(format!("input-{}", a.id))
                            .size(300.0, 28.0)
                            .on_edit(|s: &mut ComposeApp| &mut s.comment),
                    )
                    .child(el().key(format!("pop-{}", a.id)).size(80.0, 20.0).anchored(
                        format!("input-{}", a.id),
                        rune::Side::Bottom,
                        rune::Align::Start,
                        4.0,
                    ))
            })
            .build(s, &k);
        col().w_full().h_full().child(diff)
    }
    fn compose_key(s: &mut ComposeApp, ev: &KeyEvent, tx: &mut Tx) -> bool {
        s.diff.key(ev, tx)
    }

    let long = "x".repeat(400);
    let (old, new) = numbered(60, 10);
    let old = old.replace("let value_9 = compute(9);", &long);
    let new = new.replace("let value_9 = compute(9);", &long);
    let files: Vec<FileDiff> = (0..2).map(|i| from_texts(&format!("f{i}.rs"), &old, &new)).collect();
    let mut diff = DiffState::new(files);
    diff.set_annotations(vec![Annotation::line(0, LineRef::new(Side::New, 10), "c1")]);
    let sel = LineRange::line(0, LineRef::new(Side::New, 10));
    diff.set_selection(Some(sel));
    let mut rt = Runtime::new(
        ComposeApp {
            diff,
            comment: TextEdit::default(),
        },
        compose_view,
        (600.0, 600.0),
        Env::new(true, 2.0),
    );
    rt.on_key = Some(compose_key);
    rt.frame(0.0);
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
    rt.wheel_xy(300.0, 200.0, -250.0, -40.0);
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    let find_el = |rt: &Runtime<ComposeApp>, end: &str| {
        let suffix = format!("/{end}");
        rt.elements().into_iter().find(|e| e.path.ends_with(&suffix) && !e.exiting).unwrap()
    };
    let code_x = find_el(&rt, "df0.n10/line/code").rect[0];
    assert!(code_x < -100.0, "code scrolled sideways: {code_x}");
    let inp = find_el(&rt, "input-c1");
    assert!((inp.rect[0]).abs() < 0.5, "input stays pinned at left edge: {:?}", inp.rect);
    let pop = find_el(&rt, "pop-c1");
    assert!(
        (pop.rect[0] - inp.rect[0].max(8.0)).abs() < 0.5 && (pop.rect[1] - (inp.rect[1] + inp.rect[3] + 4.0)).abs() < 0.5,
        "popover follows pinned input: {:?} vs {:?}",
        pop.rect,
        inp.rect
    );
    rt.pointer_down(inp.rect[0] + 10.0, inp.rect[1] + 10.0);
    rt.pointer_up(inp.rect[0] + 10.0, inp.rect[1] + 10.0);
    rt.frame(1.0 / 60.0);
    assert!(rt.state.comment.focused);
    for ch in ["]", "[", "j", "k", "n", "p"] {
        rt.key(KeyEvent {
            key: Key::Text(ch.into()),
            mods: Mods::default(),
        });
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(rt.state.comment.text, "][jknp", "keys type into the focused comment box");
    let inp = find_el(&rt, "input-c1");
    let caret = inp.caret.expect("focused input reports caret");
    assert!(
        caret[0] >= inp.rect[0] && caret[0] < inp.rect[0] + inp.rect[2],
        "caret sits inside pinned input: {caret:?} in {:?}",
        inp.rect
    );
    assert!(
        (find_el(&rt, "df0.n10/line/code").rect[0] - code_x).abs() < 1.0,
        "typing in pinned input did not reset horizontal scroll"
    );
    assert_eq!(rt.state.diff.visible_file(), 0, "did not jump files");
    assert_eq!(rt.state.diff.cursor(), None, "the cursor did not move");
    assert_eq!(rt.state.diff.selection(), Some(sel), "selection intact");
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    rt.frame(1.0 / 60.0);
    assert!(!rt.state.comment.focused, "Escape blurred the comment box");
    assert_eq!(
        rt.state.diff.selection(),
        Some(sel),
        "Escape did not steal the diff's selection while blurring the field"
    );
    rt.key(KeyEvent {
        key: Key::Escape,
        mods: Mods::default(),
    });
    rt.frame(1.0 / 60.0);
    assert_eq!(rt.state.diff.selection(), None);
}

/// The code cells' hover handler calls `Tx::live_only` when the copy side stays: debug builds
/// check those events a quarter second after the last commit, and the rows' hover paint must not
/// read as a view change.
#[test]
fn hovering_lines_at_rest_builds_nothing_new() {
    for mode in [DiffMode::Unified, DiffMode::Split] {
        let (old, new) = numbered(40, 3);
        let mut d = DiffState::new(vec![from_texts("a.rs", &old, &new)]);
        d.set_mode(mode);
        let mut rt = runtime(d, (900.0, 1200.0));
        let cells: Vec<[f32; 4]> = rt
            .elements()
            .into_iter()
            .filter(|e| e.path.ends_with("/code") && !e.exiting)
            .map(|e| e.rect)
            .collect();
        assert!(cells.len() > 30, "{mode:?}: {} code cells", cells.len());
        let checks = rt.live_only_events();
        for (i, r) in cells.iter().enumerate() {
            if i % 2 == 0 {
                rt.frame(0.3);
            }
            rt.pointer_move(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
            tick(&mut rt);
        }
        assert!(rt.live_only_events() > checks, "{mode:?}: some moves skipped the view");
        assert!(rt.live_only_warnings().is_empty(), "{mode:?}: {:#?}", rt.live_only_warnings());
    }
}

#[test]
fn resizing_while_wrapping_rebuilds_only_the_cursor_row() {
    let (old, new) = numbered(400, 40);
    let mut d = DiffState::new(vec![from_texts("a.rs", &old, &new)]);
    d.set_wrap(true);
    let mut rt = runtime(d, (900.0, 600.0));
    rt.update(|s, tx| {
        s.diff.cursor_step(true, tx);
    });
    settle(&mut rt);
    let (lines, folds) = (rain_diffview::line_builds(), rain_diffview::fold_builds());
    for i in 1..=20 {
        rt.set_viewport((900.0 + 9.0 * i as f32, 600.0), 2.0);
        tick(&mut rt);
    }
    settle(&mut rt);
    let (lines, folds) = (rain_diffview::line_builds() - lines, rain_diffview::fold_builds() - folds);
    assert_eq!(folds, 0, "gaps and notes don't read the width while wrapping");
    assert!(
        lines <= 20,
        "20 resize frames built {lines} lines: the cursor's row at most once each"
    );
}

/// A live-only event that changes nothing, checked at moments a person makes: while hovers fade
/// across the lines, a scroll glides or a text selection is held. The view built before and
/// after it must match.
#[test]
fn a_live_only_event_while_hovering_scrolling_or_selecting_builds_nothing_new() {
    for (mode, wrap) in [(DiffMode::Unified, false), (DiffMode::Split, false), (DiffMode::Unified, true)] {
        let (old, new) = numbered(400, 3);
        let mut d = DiffState::new(vec![from_texts("a.rs", &old, &new)]);
        d.set_mode(mode);
        d.set_wrap(wrap);
        let mut rt = runtime(d, (900.0, 600.0));
        rt.on_key = Some(|_, _, tx| {
            tx.live_only();
            true
        });
        rt.check_live_only(false);
        let probe = |rt: &mut Runtime<App>| {
            rt.check_live_only(true);
            rt.key(KeyEvent {
                key: Key::Function(5),
                mods: Mods::default(),
            });
            tick(rt);
            rt.check_live_only(false);
        };
        let sweep = |rt: &mut Runtime<App>, x: f32| {
            for i in 0..200 {
                rt.pointer_move(x + (i % 5) as f32, 30.0 + (i as f32 * 7.3) % 560.0);
                tick(rt);
                if i % 20 == 19 {
                    probe(rt);
                }
            }
        };
        sweep(&mut rt, 300.0);
        sweep(&mut rt, 700.0);
        rt.wheel(400.0, 300.0, -900.0);
        for _ in 0..3 {
            tick(&mut rt);
        }
        probe(&mut rt);
        sweep(&mut rt, 300.0);
        rt.pointer_down(260.0, 210.0);
        for i in 0..20 {
            rt.pointer_move(260.0 + i as f32 * 12.0, 210.0 + i as f32 * 3.0);
            tick(&mut rt);
        }
        probe(&mut rt);
        rt.pointer_up(500.0, 270.0);
        for _ in 0..20 {
            tick(&mut rt);
        }
        probe(&mut rt);
        sweep(&mut rt, 300.0);
        assert!(
            rt.live_only_warnings().is_empty(),
            "{mode:?} wrap {wrap}: {:#?}",
            rt.live_only_warnings()
        );
        rt.on_key = Some(|s, _, tx| {
            s.diff.set_mode(if s.diff.mode() == DiffMode::Split {
                DiffMode::Unified
            } else {
                DiffMode::Split
            });
            tx.live_only();
            true
        });
        rt.frame(0.3);
        probe(&mut rt);
        assert_eq!(rt.live_only_warnings().len(), 1, "{mode:?} wrap {wrap}");
    }
}

#[test]
fn calm_added_files_keep_the_bar_and_drop_the_wash() {
    let text: String = (1..=250).map(|i| format!("added line {i}\n")).collect();
    let added = common::patch::between(None, Some("big.cs"), "", &text);
    let small = common::patch::between(None, Some("small.cs"), "", "one\ntwo\n");
    fn calm_view(s: &App, _: &mut Cx<App>) -> El {
        let k = Kit::neutral(true);
        col().w_full().h_full().child(
            DiffView::new(|s: &App| &s.diff, |s: &mut App| &mut s.diff)
                .calm_added_files(true)
                .build(s, &k),
        )
    }
    let mut rt = Runtime::new(
        App::new(DiffState::new(vec![small, added])),
        calm_view,
        (800.0, 600.0),
        Env::new(true, 2.0),
    );
    rt.frame(0.0);
    settle(&mut rt);
    let fill = |end: &str| find(&rt, end).fill;
    assert_ne!(
        fill("df0.n1"),
        fill("df1.n1"),
        "a short added file keeps its wash; a long one is calm"
    );
    assert_eq!(fill("df1.n1"), fill("df1.n2"));
}

// ---- one-sided files, the spotlight, hunk ids ----

const THREE: &str = r#"diff --git a/m.rs b/m.rs
--- a/m.rs
+++ b/m.rs
@@ -1,3 +1,3 @@
 fn m() {
-    1
+    2
 }
diff --git a/new.rs b/new.rs
new file mode 100644
--- /dev/null
+++ b/new.rs
@@ -0,0 +1,3 @@
+fn n() {
+    3
+}
diff --git a/gone.rs b/gone.rs
deleted file mode 100644
--- a/gone.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-fn g() {
-}
"#;

fn right(e: &ElementInfo) -> f32 {
    e.rect[0] + e.rect[2]
}

#[test]
fn added_and_deleted_files_take_the_whole_width_in_split() {
    let mut d = DiffState::new(parse(THREE));
    d.set_mode(DiffMode::Split);
    assert_eq!(
        (d.layout(0), d.layout(1), d.layout(2)),
        (Layout::Split, Layout::Single, Layout::Single)
    );
    let mut rt = runtime(d, (900.0, 600.0));

    let pair = find(&rt, "df0.o2");
    assert!(has(&rt, "df0.o2/old") && has(&rt, "df0.o2/new"), "a modified file still pairs");
    let half = find(&rt, "df0.o2/new");
    assert!(half.rect[2] < 460.0, "two halves: {:?}", half.rect);

    for (row, side) in [("df1.n2", "added"), ("df2.o1", "deleted")] {
        assert!(
            !has(&rt, &format!("{row}/old")) && !has(&rt, &format!("{row}/new")),
            "{side}: no halves"
        );
        assert!(has(&rt, &format!("{row}/gutter/n")), "{side}: one number column");
        assert!(!has(&rt, &format!("{row}/gutter/no")), "{side}: not two");
        let code = find(&rt, &format!("{row}/code"));
        assert!(
            (right(&code) - right(&pair)).abs() < 1.0,
            "{side}: code runs to the right edge: {:?}",
            code.rect
        );
        assert!(code.rect[0] < 80.0, "{side}: and starts after one gutter: {:?}", code.rect);
        assert!(code.rect[2] > 800.0, "{side}: the whole width, no empty half: {:?}", code.rect);
    }

    rt.state.diff.set_mode(DiffMode::Unified);
    rt.invalidate();
    settle(&mut rt);
    assert!(has(&rt, "df1.n2/gutter/no"), "unified keeps both number columns");
    assert_eq!(rt.state.diff.layout(0), Layout::Single);
}

fn three_split_min_90() -> DiffState {
    let mut d = DiffState::new(parse(THREE));
    d.set_mode(DiffMode::Split);
    d.set_min_split_columns(90);
    d
}

#[test]
fn split_draws_unified_while_too_narrow_and_says_why() {
    let d = three_split_min_90();
    assert_eq!(d.layout_reason(), None, "nothing measured yet");
    let rt = runtime(d, (900.0, 600.0));
    let st = &rt.state.diff;
    let cols = st.columns_per_side().expect("the view reports its columns");
    assert!(cols < 90, "{cols}");
    assert!(st.narrow());
    assert_eq!(st.layout(0), Layout::Single);
    assert!(
        st.layout_reason()
            .is_some_and(|r| r.contains(&cols.to_string()) && r.contains("90"))
    );
    assert!(
        has(&rt, "df0.o2/gutter/no") && !has(&rt, "df0.o2/old"),
        "the modified file draws unified"
    );

    let wide = runtime(three_split_min_90(), (2400.0, 600.0));
    let st = &wide.state.diff;
    assert!(st.columns_per_side().is_some_and(|c| c >= 90));
    assert!(!st.narrow() && st.layout_reason().is_none());
    assert_eq!(st.layout(0), Layout::Split);
    assert!(has(&wide, "df0.o2/old"));
}

fn long_whole() -> DiffState {
    let old: String = (1..=400).map(|i| format!("line {i}\n")).collect();
    let new = old.replace("line 20\n", "twenty\n").replace("line 300\n", "three hundred\n");
    let mut d = DiffState::new(vec![from_texts("a.txt", &old, &new)]);
    d.set_whole(0, true);
    d
}

#[test]
fn the_spotlight_dims_rows_outside_it_and_outlines_the_range() {
    let mut rt = runtime(long_whole(), (900.0, 1200.0));
    assert!(!has(&rt, "spot"));
    rt.update(|s, _| {
        s.diff.set_spotlight(Some(Spot {
            file: 0,
            side: Side::New,
            lines: (20, 22),
            caption: Some("The endpoint".into()),
            step: Some((3, 9)),
        }))
    });
    settle(&mut rt);
    let (a, b) = rt.state.diff.spotlight_rows().unwrap();
    assert_eq!(rt.state.diff.row_key(a as usize).as_deref(), Some("df0.n20"));
    assert_eq!(rt.state.diff.row_key(b as usize).as_deref(), Some("df0.c22"));
    // Dimmed rows sit under a scrim rather than fading as layers.
    let opacity = |k: &str| find(&rt, k).opacity;
    for k in ["df0.n20", "df0.c21", "df0.c22", "df0"] {
        assert!((opacity(k) - 1.0).abs() < 0.01, "{k} is lit: {}", opacity(k));
        assert!(!has(&rt, &format!("{k}/dim")), "{k} is lit");
    }
    for k in ["df0.c5", "df0.o20", "df0.c23", "df0.c40"] {
        assert!((opacity(k) - 1.0).abs() < 0.01, "{k} takes no layer: {}", opacity(k));
        let (row, scrim) = (find(&rt, k), find(&rt, &format!("{k}/dim")));
        assert_eq!(scrim.rect, row.rect, "{k} is dimmed over its whole row");
    }
    let spot = find(&rt, "spot");
    let (first, last) = (find(&rt, "df0.n20"), find(&rt, "df0.c22"));
    assert!(
        (spot.rect[1] - (first.rect[1] - 2.0)).abs() < 1.0,
        "{:?} vs {:?}",
        spot.rect,
        first.rect
    );
    assert!(
        (bottom(&spot) - (bottom(&last) + 2.0)).abs() < 1.0,
        "{:?} vs {:?}",
        spot.rect,
        last.rect
    );
    assert!(has(&rt, "spot/caption"));

    rt.update(|s, _| s.diff.set_spotlight(None));
    settle(&mut rt);
    assert!(!has(&rt, "spot"));
    assert!(!has(&rt, "df0.c5/dim"));
}

#[test]
fn scroll_to_range_puts_the_range_in_the_upper_third() {
    let mut rt = runtime(long_whole(), (900.0, 600.0));
    rt.update(|s, tx| s.diff.scroll_to_range(0, Side::New, (300, 305), Align::Third, tx));
    settle(&mut rt);
    let st = &rt.state.diff;
    let h = st.viewport_height();
    assert_eq!(h, 600.0);
    let r = st.row_of_line(0, LineRef::new(Side::New, 300)).unwrap() as usize;
    let y = st.row_y(r) - st.scroll_offset();
    assert!((44.0..=h / 3.0).contains(&y), "row_y says {y}");
    let row = find(&rt, "df0.n300");
    assert!((44.0..=h / 3.0).contains(&row.rect[1]), "drawn at {:?}", row.rect);
    assert_eq!(st.cursor(), Some((0, LineRef::new(Side::New, 300))));
    assert_eq!(st.visible_hunk(), Some((0, 1)));

    rt.update(|s, tx| s.diff.scroll_to_range(0, Side::New, (1, 2), Align::Third, tx));
    settle(&mut rt);
    assert_eq!(rt.state.diff.scroll_offset(), 0.0, "the top can't go higher");
}

fn numbered_lines(n: u32) -> String {
    (1..=n).map(|i| format!("line {i}\n")).collect()
}

#[test]
fn hunk_ids_survive_an_insertion_above_and_carry_through_a_rewrite_or_split() {
    let base = numbered_lines(200);
    let v1 = base.replace("line 100\n", "hundred\n");
    let mut s = DiffState::new(vec![from_texts("a.txt", &base, &v1)]);
    assert_eq!(s.files()[0].hunks.len(), 1);
    let (id, hash) = (s.hunk_id(0, 0).unwrap(), s.hunk_hash(0, 0).unwrap());
    assert_eq!(id.len(), 16);
    assert_eq!(s.hunk_by_id(0, &id), Some(0));

    // An insertion far above: a new hunk before it; the old one keeps its id and hash.
    let v2 = v1.replace("line 10\n", "line 10\ninserted\n");
    s.set_files(vec![from_texts("a.txt", &base, &v2)]);
    assert_eq!(s.files()[0].hunks.len(), 2);
    assert_eq!(s.hunk_id(0, 1).as_deref(), Some(id.as_str()));
    assert_eq!(s.hunk_hash(0, 1).as_deref(), Some(hash.as_str()));
    assert_ne!(s.hunk_id(0, 0), s.hunk_id(0, 1));
    let inserted = s.hunk_id(0, 0).unwrap();

    // Rewritten in place: the same id by its old-side range, a new hash.
    let v3 = v2.replace("hundred\n", "one hundred, rewritten\n");
    s.set_files(vec![from_texts("a.txt", &base, &v3)]);
    assert_eq!(s.hunk_id(0, 1).as_deref(), Some(id.as_str()));
    assert_ne!(s.hunk_hash(0, 1).as_deref(), Some(hash.as_str()));
    assert_eq!(s.hunk_id(0, 0).as_deref(), Some(inserted.as_str()));

    // Rewritten into two hunks over the old one's range: the bigger keeps the id, the other is new.
    let v4 = base
        .replace("line 10\n", "line 10\ninserted\n")
        .replace("line 94\n", "ninety-four\n")
        .replace("line 106\n", "a\nb\nc\n");
    s.set_files(vec![from_texts("a.txt", &base, &v4)]);
    assert_eq!(s.files()[0].hunks.len(), 3);
    let ids: Vec<String> = (0..3).map(|h| s.hunk_id(0, h).unwrap()).collect();
    assert_eq!(ids[0], inserted);
    assert_eq!(ids[2], id, "the bigger part inherits");
    assert!(ids[1] != id && ids[1] != inserted, "the smaller part gets a new id");

    // A file the previous diff didn't have: its own ids.
    let mut other = DiffState::new(vec![from_texts("a.txt", &base, &v1)]);
    other.set_files(vec![from_texts("b.txt", &base, &v1)]);
    assert_ne!(other.hunk_id(0, 0), Some(id.clone()));
    assert_eq!(other.hunk_id(0, 1), None);
}

#[test]
fn renamed_files_keep_their_hunk_ids() {
    let base = numbered_lines(50);
    let v1 = base.replace("line 25\n", "twenty-five\n");
    let mut s = DiffState::new(vec![from_texts("old.txt", &base, &v1)]);
    let id = s.hunk_id(0, 0).unwrap();
    let mut moved = from_texts("new.txt", &base, &v1);
    moved.old_path = Some("old.txt".into());
    s.set_files(vec![moved]);
    assert_eq!(s.hunk_id(0, 0), Some(id));
}

#[test]
fn visible_hunk_follows_hunk_navigation() {
    let mut s = long_whole();
    s.collapse_expanded(0);
    let mut tx = Tx::default();
    assert!(s.next_hunk(true, &mut tx));
    assert_eq!(s.visible_hunk(), Some((0, 0)));
    assert!(s.next_hunk(true, &mut tx));
    assert_eq!(s.visible_hunk(), Some((0, 1)));
}
