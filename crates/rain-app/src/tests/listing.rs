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
