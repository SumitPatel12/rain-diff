//! The Conversation sidebar's thread cards.

use rain_comments::ops;
use rain_core::{Anchor, Side};
use rune::{Key, Mods, Runtime};

use super::*;
use crate::app::Overlay;

const LONG: &str = "The voucher total is summed before the vouchers exist, so the account's balance is off by every voucher added in this request. Move RefreshTotal inside the same unit of work.\n\nSee `AccountService.Save`.";

/// The story with a long comment and a short one, the Conversation open.
fn two_comments() -> (Runtime<App>, String, String) {
    let mut a = app(story(), true);
    let g = (0..a.keys.len()).find(|&g| a.diff.files()[g].additions > 3).unwrap();
    let f = a.diff.files()[g].clone();
    let line = f.lines.iter().find(|l| l.new != 0).unwrap().new;
    let (repo, path) = (a.repo_tag(g).to_owned(), f.path.clone());
    let at = Anchor::Line { side: Side::New, line };
    let (long, short) = a.mutate(|s| {
        (
            ops::add_thread(s, &repo, &path, at, LONG, None, None),
            ops::add_thread(s, &repo, &path, at, "Nice.", None, None),
        )
    });
    a.open_overlay(Overlay::Conversation);
    (runtime(a), long, short)
}

fn info(rt: &Runtime<App>, key: &str) -> rune::ElementInfo {
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && e.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no element {key}"))
}

fn tap(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_move(x, y);
    settle(rt);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

#[test]
fn a_clipped_comment_shows_more_and_folds_back_while_a_short_one_has_no_control() {
    let (mut rt, long, short) = two_comments();
    assert!(rt.find(&format!("conv-more-{short}")).is_none(), "nothing hidden, no control");
    let more = format!("conv-more-{long}");
    let card = format!("conv-{long}");
    let b = info(&rt, &more);
    assert!(b.focusable && b.role == "button");
    assert_eq!(b.label.as_deref(), Some("Show the whole comment"));
    assert_eq!(b.expanded, Some(false));
    assert!(rt.find(&format!("conv-full-{long}")).is_none());
    let folded = info(&rt, &card).rect[3];

    tap(&mut rt, &more);
    assert!(rt.state.conversation.expanded.contains(&long));
    assert!(rt.state.layout.is_open(Overlay::Conversation), "the card's own press doesn't fire");
    assert!(rt.find(&format!("conv-full-{long}")).is_some());
    let texts: Vec<String> = rt.elements().into_iter().filter_map(|e| e.text).collect();
    assert!(
        texts.iter().any(|t| t.contains("Move RefreshTotal inside")),
        "the second sentence shows"
    );
    let b = info(&rt, &more);
    assert_eq!(b.label.as_deref(), Some("Show less of this comment"));
    assert_eq!(b.expanded, Some(true));
    assert!(info(&rt, &card).rect[3] > folded + 20.0, "the card grows to the whole text");
    assert!(rt.find(&format!("conv-full-{short}")).is_none(), "per thread");

    tap(&mut rt, &more);
    assert!(rt.state.conversation.expanded.is_empty());
    assert!(rt.find(&format!("conv-full-{long}")).is_none());
    assert!((info(&rt, &card).rect[3] - folded).abs() < 0.5, "back to one line");
}

#[test]
fn show_more_is_reached_with_tab_and_pressed_with_space() {
    let (mut rt, long, _) = two_comments();
    let more = format!("conv-more-{long}");
    let mut reached = false;
    for _ in 0..300 {
        press(&mut rt, Key::Tab, Mods::default());
        if info(&rt, &more).focused {
            reached = true;
            break;
        }
    }
    assert!(reached, "Tab reaches Show more");
    press(&mut rt, Key::Text(" ".into()), Mods::default());
    assert!(rt.state.conversation.expanded.contains(&long), "Space presses it");
}

#[test]
fn a_one_sentence_comment_too_wide_for_the_card_is_clipped() {
    let mut a = app(story(), true);
    let g = (0..a.keys.len()).find(|&g| a.diff.files()[g].additions > 3).unwrap();
    let f = a.diff.files()[g].clone();
    let line = f.lines.iter().find(|l| l.new != 0).unwrap().new;
    let (repo, path) = (a.repo_tag(g).to_owned(), f.path.clone());
    let at = Anchor::Line { side: Side::New, line };
    let id = a.mutate(|s| {
        ops::add_thread(
            s,
            &repo,
            &path,
            at,
            "Validate the amount before the voucher is saved to the account",
            None,
            None,
        )
    });
    a.open_overlay(Overlay::Conversation);
    let rt = runtime(a);
    assert!(rt.find(&format!("conv-more-{id}")).is_some());
}
