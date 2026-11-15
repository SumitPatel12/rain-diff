//! `DiffView::frame`: a file as a full-width header bar or as an inset card, with the same rows
//! and keys either way, so switching keeps the reading place.

mod common;

use common::patch::from_texts;
use rain_core::diff::Side;
use rain_diffview::{DiffState, DiffView, FileFrame, LineRef};
use rune::runtime::ElementInfo;
use rune::{Cx, El, Env, Runtime, col};
use rune_kit::Kit;

const CARD: FileFrame = FileFrame::Card {
    inset: 12.0,
    gap: 12.0,
    radius: 8.0,
};

struct App {
    diff: DiffState,
    frame: FileFrame,
}

fn view(s: &App, _: &mut Cx<App>) -> El {
    let k = Kit::neutral(true);
    let diff = DiffView::new(|s: &App| &s.diff, |s: &mut App| &mut s.diff)
        .frame(s.frame)
        .build(s, &k);
    col().w_full().h_full().child(diff)
}

fn settle(rt: &mut Runtime<App>) {
    for _ in 0..90 {
        rt.frame(1.0 / 60.0);
    }
}

fn runtime(frame: FileFrame) -> Runtime<App> {
    let old: String = (1..=40).map(|i| format!("line {i}\n")).collect();
    let a = from_texts("a.rs", &old, &old.replace("line 5\n", "line five\n"));
    let b = from_texts("b.rs", &old, &old.replace("line 30\n", "line thirty\n"));
    let mut rt = Runtime::new(
        App {
            diff: DiffState::new(vec![a, b]),
            frame,
        },
        view,
        (900.0, 1200.0),
        Env::new(true, 1.0),
    );
    rt.frame(0.0);
    settle(&mut rt);
    rt
}

fn find(rt: &Runtime<App>, end: &str) -> Option<ElementInfo> {
    let suffix = format!("/{end}");
    rt.elements().into_iter().find(|e| e.path.ends_with(&suffix) && !e.exiting)
}

fn row_keys(rt: &Runtime<App>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting && e.key.as_deref().is_some_and(|k| k.starts_with("df")))
        .filter_map(|e| e.key)
        .collect()
}

#[test]
fn a_bar_spans_the_view_with_no_card() {
    let rt = runtime(FileFrame::Bar);
    let head = find(&rt, "df1").expect("the second file's header");
    assert_eq!(head.rect[0], 0.0);
    assert_eq!(head.rect[2], 900.0, "full width");
    assert!(find(&rt, "df0/card").is_none(), "no card around a bar");
}

#[test]
fn a_card_is_inset_with_a_gap_and_rounded_ends() {
    let rt = runtime(CARD);
    let card = find(&rt, "df1/card").expect("the header row is in its card");
    assert_eq!(card.rect[0], 12.0, "inset from the left");
    assert_eq!(card.rect[2], 900.0 - 24.0, "inset from the right");
    let head = find(&rt, "df1").unwrap();
    assert_eq!(card.rect[1] - head.rect[1], 12.0, "the gap above the file");
    let lines: Vec<ElementInfo> = rt
        .elements()
        .into_iter()
        .filter(|e| !e.exiting && e.path.ends_with("/card") && e.path.contains("/df0."))
        .collect();
    assert!(!lines.is_empty(), "body rows sit in the card too");
    for l in &lines {
        assert_eq!((l.rect[0], l.rect[2]), (12.0, 876.0), "{}", l.path);
    }
    let first = find(&rt, "df0").unwrap();
    let next = find(&rt, "df1").unwrap();
    let last_row = rt
        .elements()
        .into_iter()
        .filter(|e| !e.exiting && e.path.contains("/df0.") && e.path.ends_with("/card"))
        .max_by(|a, b| a.rect[1].total_cmp(&b.rect[1]))
        .unwrap();
    assert!(first.rect[1] < last_row.rect[1] && last_row.rect[1] < next.rect[1]);
    assert_eq!(
        last_row.rect[1] + last_row.rect[3],
        next.rect[1],
        "the next file starts right under the last row"
    );
}

#[test]
fn switching_the_frame_keeps_rows_keys_and_the_cursor() {
    let mut rt = runtime(FileFrame::Bar);
    let at = LineRef::new(Side::New, 30);
    rt.update(|s, tx| s.diff.scroll_to_line(1, at, tx));
    settle(&mut rt);
    let rows_before = rt.state.diff.row_count();
    let keys_before = row_keys(&rt);
    let row = rt.state.diff.row_of_line(1, at).unwrap();
    assert!(rt.state.diff.visible_rows().contains(&(row as usize)), "the line is on screen");
    rt.update(|s, _| s.frame = CARD);
    settle(&mut rt);
    assert_eq!(rt.state.diff.row_count(), rows_before, "no rows added or removed");
    assert_eq!(rt.state.diff.cursor(), Some((1, at)), "the cursor stays");
    assert_eq!(rt.state.diff.row_of_line(1, at), Some(row));
    assert!(
        rt.state.diff.visible_rows().contains(&(row as usize)),
        "and its line stays on screen"
    );
    let keys_after = row_keys(&rt);
    assert!(keys_after.contains(&rt.state.diff.row_key(row as usize).unwrap()), "{keys_after:?}");
    assert!(!keys_before.is_empty() && !keys_after.is_empty());
    rt.update(|s, _| s.frame = FileFrame::Bar);
    settle(&mut rt);
    assert_eq!(rt.state.diff.cursor(), Some((1, at)));
    assert!(find(&rt, "df1/card").is_none(), "back to bars");
}
