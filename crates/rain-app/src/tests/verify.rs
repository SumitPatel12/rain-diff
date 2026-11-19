//! The floating Verify checklist: folded to its head until opened, a tick strikes an item and
//! lands in the session, the reviewer edits, removes and adds items over the Brief's, and the
//! head drags the card about through a live offset.

use rain_core::BriefDoc;
use rain_core::brief::Section;
use rain_core::verify::{ItemRef, ItemSource};
use rune::{ElementInfo, Runtime};

use super::*;

fn brief(md: &str) -> BriefDoc {
    BriefDoc {
        title: "t".into(),
        verify: Some(Section {
            id: "verify".into(),
            text: md.into(),
            ..Section::default()
        }),
        ..BriefDoc::default()
    }
}

fn with_verify() -> Runtime<App> {
    let mut a = app(story(), true);
    a.mutate(|s| {
        s.brief = Some(brief("- Open the account page\n- Add a voucher of 0 and see it refused\n"));
        s.bump_brief();
    });
    a.set_reader_tab(crate::app::ReaderTab::Changes);
    runtime(a)
}

fn info(rt: &Runtime<App>, key: &str) -> ElementInfo {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no {key}"))
}

fn click(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn opened() -> Runtime<App> {
    let mut rt = with_verify();
    click(&mut rt, "verify-head");
    rt
}

fn texts(rt: &Runtime<App>) -> Vec<String> {
    rt.state.verify_items().into_iter().map(|i| i.text).collect()
}

#[test]
fn the_checklist_starts_folded_to_its_head_with_the_count() {
    let mut rt = with_verify();
    assert!(rt.find("verify-head").is_some(), "the card floats over the diff");
    assert!(rt.find("verify-0").is_none(), "folded by default");
    assert_eq!(info(&rt, "verify-count").text.as_deref(), Some("0 of 2 done"));
    click(&mut rt, "verify-head");
    assert!(rt.find("verify-1").is_some(), "a click opens it");
    assert!(rt.state.shared.lock().verify.open, "open is remembered in the session");
}

#[test]
fn a_tick_lands_in_the_session_and_the_head_folds_it() {
    let mut rt = opened();
    click(&mut rt, "verify-0");
    assert!(rt.state.verify_done("Open the account page"));
    assert_eq!(rt.state.shared.lock().verified, ["Open the account page"], "saved in the session");
    assert_eq!(info(&rt, "verify-count").text.as_deref(), Some("1 of 2 done"));
    click(&mut rt, "verify-head");
    assert!(rt.find("verify-0").is_none(), "folded to its head");
    assert!(rt.find("verify-head").is_some());
}

#[test]
fn shift_h_opens_and_folds_the_checklist_and_its_controls_take_the_keyboard() {
    let mut rt = with_verify();
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    press(&mut rt, Key::Text("H".into()), shift);
    assert!(rt.find("verify-0").is_some(), "open");
    let focusable = |rt: &Runtime<App>, k: &str| info(rt, k).focusable;
    for k in ["verify-head", "verify-0", "verify-edit-0", "verify-remove-0"] {
        assert!(focusable(&rt, k), "{k} takes focus");
    }
    press(&mut rt, Key::Text("H".into()), shift);
    assert!(rt.find("verify-0").is_none(), "folded again");
}

#[test]
fn shift_h_without_verify_items_says_so() {
    let mut rt = runtime(app(story(), true));
    press(
        &mut rt,
        Key::Text("H".into()),
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    assert!(rt.state.status.starts_with("Nothing to verify by hand"), "{}", rt.state.status);
}

#[test]
fn adding_editing_and_removing_items_persist_and_typing_runs_no_shortcuts() {
    let mut rt = opened();
    rt.update(|s, _| s.verify_card.add.focused = true);
    settle(&mut rt);
    assert!(rt.state.typing());
    let place = rt.state.active().cursor();
    typed(&mut rt, "jkH Check totals");
    assert_eq!(rt.state.verify_card.add.text, "jkH Check totals", "keys go to the field");
    assert_eq!(rt.state.active().cursor(), place, "the diff never moved");
    assert!(rt.state.verify_open(), "Shift+H typed into the field leaves the card open");
    press(&mut rt, Key::Enter, Mods::default());
    assert_eq!(texts(&rt).last().map(String::as_str), Some("jkH Check totals"));
    assert!(rt.state.verify_card.add.text.is_empty(), "the field empties for the next one");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.verify_card.add.focused, "Esc leaves the field");

    click(&mut rt, "verify-edit-0");
    assert_eq!(rt.state.verify_card.editing, Some(ItemRef::Brief("Open the account page".into())));
    assert_eq!(rt.state.verify_card.edit.text, "Open the account page");
    rt.update(|s, _| s.verify_card.edit.insert(" as admin"));
    press(&mut rt, Key::Enter, Mods::default());
    assert_eq!(rt.state.verify_card.editing, None, "Enter saves");
    assert_eq!(texts(&rt)[0], "Open the account page as admin");

    click(&mut rt, "verify-edit-1");
    rt.update(|s, _| s.verify_card.edit.insert(" twice"));
    press(&mut rt, Key::Escape, Mods::default());
    assert_eq!(rt.state.verify_card.editing, None, "Esc cancels");
    assert_eq!(texts(&rt)[1], "Add a voucher of 0 and see it refused");

    click(&mut rt, "verify-remove-1");
    assert_eq!(texts(&rt), ["Open the account page as admin", "jkH Check totals"]);
    let saved = rt.state.shared.lock().verify.clone();
    assert_eq!(saved.removed, ["Add a voucher of 0 and see it refused"]);
    assert_eq!(saved.added, ["jkH Check totals"]);

    let json = serde_json::to_string(&*rt.state.shared.lock()).unwrap();
    let back: rain_core::Session = serde_json::from_str(&json).unwrap();
    assert_eq!(
        back.verify_items().into_iter().map(|i| i.text).collect::<Vec<_>>(),
        texts(&rt),
        "the edits survive a save and reopen"
    );
    rt.update(|s, _| s.reload());
    settle(&mut rt);
    assert_eq!(texts(&rt), ["Open the account page as admin", "jkH Check totals"], "and a reload");
}

#[test]
fn a_new_brief_keeps_the_reviewers_edits() {
    let mut rt = opened();
    click(&mut rt, "verify-0");
    rt.update(|s, _| {
        s.start_verify_edit(ItemRef::Brief("Open the account page".into()), "Open the account page");
        s.verify_card.edit.insert(", signed in");
        s.save_verify_edit();
        s.remove_verify_item(&ItemRef::Brief("Add a voucher of 0 and see it refused".into()));
        s.verify_card.add.insert("Mine");
        s.add_verify_item();
    });
    settle(&mut rt);
    rt.update(|a, _| {
        a.mutate(|s| {
            s.brief = Some(brief(
                "- Open the account page\n- Add a voucher of 0 and see it refused\n- Pay with a card\n",
            ));
            s.bump_brief();
        });
    });
    settle(&mut rt);
    let items = rt.state.verify_items();
    assert_eq!(
        items.iter().map(|i| (i.text.as_str(), i.source)).collect::<Vec<_>>(),
        [
            ("Open the account page, signed in", ItemSource::Edited),
            ("Pay with a card", ItemSource::Brief),
            ("Mine", ItemSource::Added),
        ]
    );
    assert!(
        rt.state.verify_done("Open the account page, signed in"),
        "the tick followed the edit"
    );
}

fn drag_head(rt: &mut Runtime<App>, by: (f32, f32), steps: usize) -> u64 {
    let (x, y) = rt.find("verify-head").unwrap();
    rt.pointer_down(x, y);
    rt.frame(1.0 / 60.0);
    let commits = rune::commit_generation();
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        rt.pointer_move(x + by.0 * t, y + by.1 * t);
        rt.frame(1.0 / 60.0);
    }
    let during = rune::commit_generation() - commits;
    rt.pointer_up(x + by.0, y + by.1);
    settle(rt);
    during
}

#[test]
fn dragging_the_head_moves_the_card_with_no_view_run_and_keeps_it_on_screen() {
    let mut rt = with_verify();
    let start = info(&rt, "verify").rect;
    let main = info(&rt, "main").rect;
    assert!(
        (start[0] - (main[0] + 14.0)).abs() < 1.0 && (start[1] + start[3] - (main[1] + main[3] - 14.0)).abs() < 1.0,
        "by default at the reader's bottom left: {start:?} in {main:?}"
    );
    // Held still for as many frames, the window commits what time and hover bring anyway; 80
    // moves add at most what the pointer passes over, not a view run each.
    let held = drag_head(&mut rt, (0.0, 0.0), 80);
    assert!(rt.state.verify_open(), "a press that never moved is a click");
    click(&mut rt, "verify-head");
    let few = drag_head(&mut rt, (300.0, -400.0), 10);
    let many = drag_head(&mut rt, (-300.0, 400.0), 80);
    assert!(
        many <= held + few + 2,
        "{many} commits over 80 moves ({held} held still, {few} over 10): a move is a live value"
    );
    let home = info(&rt, "verify").rect;
    assert!((home[0] - start[0]).abs() < 1.0 && (home[1] - start[1]).abs() < 1.0, "{home:?}");
    drag_head(&mut rt, (300.0, -400.0), 10);
    let moved = info(&rt, "verify").rect;
    assert!(
        (moved[0] - start[0] - 300.0).abs() < 1.0 && (moved[1] - start[1] + 400.0).abs() < 1.0,
        "{moved:?} from {start:?}"
    );
    assert!(rt.find("verify-0").is_none(), "the drop doesn't open the card");
    let at = rt.state.shared.lock().verify.at.expect("the drop is saved in the session");
    assert!((at[0] - moved[0]).abs() < 1.0 && (at[1] - moved[1]).abs() < 1.0);

    let (x, y) = rt.find("verify-head").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    rt.frame(0.05);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert_eq!(rt.state.shared.lock().verify.at, None, "a double-click puts it back");
    assert!(!rt.state.verify_open(), "and its two clicks leave it as it was");
    let back = info(&rt, "verify").rect;
    assert!((back[0] - start[0]).abs() < 1.0 && (back[1] - start[1]).abs() < 1.0, "{back:?}");

    drag_head(&mut rt, (5000.0, 5000.0), 4);
    let r = info(&rt, "verify").rect;
    assert!(r[0] + r[2] <= 1600.0 && r[1] + r[3] <= 1000.0, "clamped inside the window: {r:?}");

    rt.set_viewport((900.0, 600.0), 1.0);
    settle(&mut rt);
    let r = info(&rt, "verify").rect;
    assert!(
        r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= 900.0 && r[1] + r[3] <= 600.0,
        "a smaller window keeps it on screen: {r:?}"
    );
}

#[test]
fn reset_position_is_a_button_while_the_card_is_open_and_moved() {
    let mut rt = opened();
    assert!(rt.find("verify-reset").is_none(), "nothing to reset at the default spot");
    drag_head(&mut rt, (200.0, -100.0), 5);
    assert!(rt.state.verify_open(), "the drop leaves it open");
    click(&mut rt, "verify-reset");
    assert_eq!(rt.state.shared.lock().verify.at, None);
}

#[test]
fn escape_lets_go_of_a_focused_section() {
    let mut rt = with_verify();
    rt.update(|s, _| s.focus_section("verify"));
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.brief.focus.is_none());
}
