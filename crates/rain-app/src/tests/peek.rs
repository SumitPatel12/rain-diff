//! The cite peek: hover dwell and grace, presses, keys, closing on scroll, and the pop-out
//! window's state.

use std::time::{Duration, Instant};

use rain_core::brief::BriefDoc;
use rune::{Key, Mods, Runtime};

use super::*;
use crate::app::{PeekTarget, ReaderTab};
use crate::theme::Scheme;

const STORY_BRIEF: &str = include_str!("../../../../fixtures/brief/story.json");

fn overview() -> Runtime<App> {
    let mut a = app(story(), true);
    let mut d: BriefDoc = serde_json::from_str(STORY_BRIEF).unwrap();
    d.assign_ids();
    a.mutate(|s| {
        s.brief = Some(d);
        s.bump_brief();
    });
    a.set_reader_tab(ReaderTab::Overview);
    sized_runtime(a, (1920.0, 1080.0))
}

fn chip_target(rt: &Runtime<App>) -> PeekTarget {
    rt.state.sample_peek_target().expect("a cite with lines")
}

#[test]
fn a_hover_dwell_opens_the_peek_and_leaving_closes_it_after_the_grace() {
    let mut rt = overview();
    let t = chip_target(&rt);
    let at = Instant::now();
    rt.update(|s, _| {
        s.peek_hover_at(t.clone(), true, at);
        assert!(!s.peek_tick(at + Duration::from_millis(100)), "not before the dwell");
        assert!(s.brief.peek.open.is_none());
        assert!(s.peek_tick(at + Duration::from_millis(310)));
    });
    settle(&mut rt);
    assert_eq!(rt.state.brief.peek.open.as_ref(), Some(&t));
    assert!(rt.find("peek").is_some(), "the card is drawn");
    assert!(rt.find("peek-open").is_some() && rt.find("peek-popout").is_some() && rt.find("peek-close").is_some());
    let card = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("peek")).unwrap();
    assert!(card.rect[2] <= 640.5, "640 wide at most: {:?}", card.rect);
    assert!(card.rect[3] <= 420.5, "420 high at most: {:?}", card.rect);
    let anchor = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(t.anchor.as_str()))
        .unwrap();
    assert!(
        card.rect[1] >= anchor.rect[1] + anchor.rect[3] - 0.5 || card.rect[1] + card.rect[3] <= anchor.rect[1] + 0.5,
        "beside its chip"
    );
    let left = Instant::now();
    rt.update(|s, _| {
        s.peek_hover_at(t.clone(), false, left);
        s.peek_card_hover_at(true, left + Duration::from_millis(100));
        assert!(!s.peek_tick(left + Duration::from_millis(400)), "the card's hover keeps it");
        s.peek_card_hover_at(false, left + Duration::from_millis(500));
        assert!(!s.peek_tick(left + Duration::from_millis(600)), "the grace");
        assert!(s.peek_tick(left + Duration::from_millis(760)));
    });
    settle(&mut rt);
    assert!(rt.find("peek").is_none());
}

#[test]
fn a_real_pointer_dwell_on_a_chip_opens_it() {
    let mut rt = overview();
    let t = chip_target(&rt);
    let (x, y) = rt.find(&t.anchor).unwrap();
    rt.pointer_move(x, y);
    settle(&mut rt);
    std::thread::sleep(Duration::from_millis(320));
    settle(&mut rt);
    assert!(rt.state.brief.peek.open.is_some(), "the dwell ran out under the pointer");
    assert!(rt.find("peek").is_some());
}

#[test]
fn a_press_jumps_and_space_on_a_focused_chip_peeks_and_esc_closes() {
    let mut rt = overview();
    let t = chip_target(&rt);
    let mut focused = false;
    for _ in 0..400 {
        press(&mut rt, Key::Tab, Mods::default());
        if rt
            .elements()
            .iter()
            .any(|e| e.focused && e.key.as_deref() == Some(t.anchor.as_str()))
        {
            focused = true;
            break;
        }
    }
    assert!(focused, "Tab reaches the chip");
    press(&mut rt, Key::Text(" ".into()), Mods::default());
    assert_eq!(rt.state.brief.peek.open.as_ref(), Some(&t), "Space peeks");
    assert!(rt.state.brief.peek.pinned);
    assert_eq!(rt.state.reader_tab(), ReaderTab::Overview, "Space does not jump");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.brief.peek.open.is_none(), "Esc closes");
    press(&mut rt, Key::Enter, Mods::default());
    assert_eq!(rt.state.reader_tab(), ReaderTab::Changes, "Enter jumps");
    assert_eq!(rt.state.current_key().as_deref(), Some(t.key.as_str()));
}

#[test]
fn a_cite_link_in_the_text_opens_a_pinned_peek_and_open_in_reader_jumps() {
    let mut rt = overview();
    rt.update(|s, _| {
        let t = crate::view::cite_target(s, "server/src/Services/VoucherService.cs".into(), Some((27, 31)), "md-intent");
        s.open_peek(t, true);
    });
    settle(&mut rt);
    assert!(rt.find("peek").is_some());
    let at = Instant::now();
    rt.update(|s, _| {
        s.peek_card_hover_at(false, at);
        assert!(
            !s.peek_tick(at + Duration::from_secs(2)),
            "a pressed peek stays when the pointer leaves"
        );
    });
    let (x, y) = rt.find("peek-open").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(rt.find("peek").is_none());
    assert_eq!(rt.state.reader_tab(), ReaderTab::Changes);
    assert_eq!(rt.state.current_key().as_deref(), Some("server/src/Services/VoucherService.cs"));
    let (_, at) = rt.state.active().cursor().unwrap();
    assert_eq!(at.line, 27);
}

#[test]
fn scrolling_the_page_away_closes_the_peek() {
    let mut rt = overview();
    let t = chip_target(&rt);
    rt.update(|s, _| s.open_peek(t, true));
    settle(&mut rt);
    assert!(rt.state.brief.peek.open.is_some());
    rt.update(|_, tx| tx.scroll_to_y("overview", 300.0));
    settle(&mut rt);
    assert!(rt.state.brief.peek.open.is_none(), "the anchor scrolled away");
}

#[test]
fn the_peek_code_is_legible_mono_13_with_tints() {
    let mut rt = overview();
    let t = chip_target(&rt);
    rt.update(|s, _| s.open_peek(t, true));
    settle(&mut rt);
    let rows: Vec<_> = rt
        .elements()
        .into_iter()
        .filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("peek-row-")))
        .collect();
    assert!(rows.len() >= 7, "{}", rows.len());
    assert!(rows.iter().all(|r| r.rect[3] >= 19.5), "20pt rows");
    let added = rt
        .state
        .peek_content(&chip_target(&rt))
        .rows
        .iter()
        .any(|r| r.kind == rain_core::LineKind::Added);
    assert!(added, "the story's VoucherService lines are additions");
}

#[test]
fn pop_out_fills_the_window_state_and_a_second_replaces_it() {
    let rt = overview();
    let t = chip_target(&rt);
    let first = rt.state.peek_content(&t);
    let mut w = crate::PeekWindow::default();
    w.show(first.clone(), Scheme::Dark);
    assert_eq!(w.content.as_ref(), Some(&first));
    assert_eq!(w.scheme, Some(Scheme::Dark));
    let other = PeekTarget {
        key: "server/src/Api/VouchersController.cs".into(),
        lines: Some((23, 23)),
        ..t
    };
    let second = rt.state.peek_content(&other);
    w.show(second.clone(), Scheme::Everforest);
    assert_eq!(w.content, Some(second));
    assert_eq!(w.scheme, Some(Scheme::Everforest));
    fn send<T: Send>(_: &T) {}
    send(&w);
    let mut win = Runtime::new(w, crate::peek_window::view, (760.0, 480.0), rune::Env::new(true, 1.0));
    win.frame(0.0);
    assert!(win.find("peek-window-head").is_some());
    assert!(win.find("peek-row-0").is_some());
    let root = win
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("peek-window"))
        .unwrap();
    assert_eq!(
        root.fill,
        crate::theme::look(Scheme::Everforest).s.reader,
        "the pop-out keeps the main window's scheme"
    );
}

#[test]
fn a_diff_the_brief_does_not_cite_still_gives_the_pop_out_scene_lines() {
    let a = app(story(), false);
    let t = a.first_change_peek_target().expect("a file with additions");
    let (first, _) = t.lines.unwrap();
    let c = a.peek_content(&t);
    assert!(c.rows.iter().any(|r| r.cited && r.new == first), "the first added line is cited");
    let mut win = Runtime::new(
        crate::PeekWindow {
            content: Some(c),
            ..Default::default()
        },
        crate::peek_window::view,
        (760.0, 480.0),
        rune::Env::new(false, 1.0),
    );
    win.frame(0.0);
    assert!(win.find("peek-row-0").is_some(), "lines, not the empty placeholder");
}

#[test]
fn pop_out_without_a_window_says_so_and_closes_the_peek() {
    let mut rt = overview();
    let t = chip_target(&rt);
    rt.update(|s, _| s.open_peek(t, true));
    settle(&mut rt);
    let (x, y) = rt.find("peek-popout").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(rt.state.brief.peek.open.is_none());
    assert_eq!(rt.state.status, "No window to pop out into here");
}
