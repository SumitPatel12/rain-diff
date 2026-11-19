//! The list's change groups fold on their own, and marking a file viewed folds it in the diff.

use rain_triage::Group;

use super::*;
use crate::app::board::Row;

fn files_of(a: &App, g: Group) -> usize {
    a.listing()
        .rows
        .iter()
        .filter(|r| matches!(r, Row::File(f) if a.group_of(*f) == g))
        .count()
}

#[test]
fn a_change_group_folds_under_its_heading() {
    let mut a = app(story(), true);
    let must = files_of(&a, Group::Must);
    assert!(must > 0, "the story has Must review files");
    a.toggle_sub(Group::Must);
    assert_eq!(files_of(&a, Group::Must), 0, "folded, its files leave the list");
    assert!(a.listing().rows.contains(&Row::Sub(Group::Must)), "its heading stays to unfold it");
    assert!(files_of(&a, Group::Review) > 0, "the other groups stay open");
    a.toggle_sub(Group::Must);
    assert_eq!(files_of(&a, Group::Must), must);
}

#[test]
fn marking_a_file_viewed_folds_it_and_unmarking_opens_it() {
    let mut a = app(story(), true);
    assert!(!a.active().collapsed(0));
    a.set_viewed(0, true);
    assert!(a.active().collapsed(0), "viewed folds the file to its header");
    a.set_viewed(0, false);
    assert!(!a.active().collapsed(0), "unviewed opens it again");
}

fn top_of(rt: &rune::Runtime<App>, key: &str) -> f32 {
    rt.elements()
        .iter()
        .find(|e| e.key.as_deref() == Some(key))
        .map(|e| e.rect[1])
        .unwrap_or(f32::NAN)
}

#[test]
fn marking_a_file_viewed_puts_the_next_open_file_at_the_top() {
    let mut rt = runtime(app(story(), true));
    assert!(rt.state.active().files().len() >= 3, "the story has three files");
    rt.update(|s, _| s.active_mut().set_collapsed(1, true));
    settle(&mut rt);
    rt.update(|s, tx| s.mark_viewed(0, true, tx));
    for _ in 0..3 {
        settle(&mut rt); // the scroll glides there
    }
    assert!(rt.state.active().collapsed(0));
    let (head, view) = (top_of(&rt, "df2"), top_of(&rt, "scroll"));
    assert!((head - view).abs() < 0.5, "file 2's header at the top: {head} vs {view}");

    rt.update(|s, tx| s.mark_viewed(2, false, tx));
    settle(&mut rt);
    assert!(!rt.state.active().collapsed(2), "unmarking scrolls nothing and opens it");
}

#[test]
fn mark_all_viewed_marks_and_folds_every_shown_file() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, tx| {
        s.run(rain_core::Command::MarkAllViewed, tx);
    });
    settle(&mut rt);
    let a = &rt.state;
    let n = a.keys.len();
    assert!((0..n).all(|g| a.is_viewed(g)), "every file viewed");
    assert!((0..a.active().files().len()).all(|l| a.active().collapsed(l)), "and folded");
    assert_eq!(a.status, format!("Marked {n} files viewed"));
    assert_eq!(a.mirror.viewed.len(), n, "saved in the session");
    rt.update(|s, tx| {
        s.run(rain_core::Command::MarkAllViewed, tx);
    });
    assert_eq!(rt.state.status, "Every file is already viewed");
}
