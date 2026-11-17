//! The floating Verify checklist: it shows the Brief's items over the diff, a tick strikes one
//! and lands in the session, and its head folds it to a pill.

use rain_core::BriefDoc;
use rain_core::brief::Section;

use super::*;

fn with_verify() -> Runtime<App> {
    let mut a = app(story(), true);
    a.mutate(|s| {
        s.brief = Some(BriefDoc {
            title: "t".into(),
            verify: Some(Section {
                id: "verify".into(),
                text: "- Open the account page\n- Add a voucher of 0 and see it refused\n".into(),
                ..Section::default()
            }),
            ..BriefDoc::default()
        });
        s.bump_brief();
    });
    a.set_reader_tab(crate::app::ReaderTab::Changes);
    runtime(a)
}

#[test]
fn the_checklist_floats_over_the_diff_and_ticks_into_the_session() {
    let mut rt = with_verify();
    assert!(rt.find("verify").is_some(), "the card floats in the reader");
    assert!(rt.find("verify-1").is_some(), "one row per item");
    let (x, y) = rt.find("verify-0").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(rt.state.verify_done("Open the account page"));
    assert_eq!(rt.state.shared.lock().verified, ["Open the account page"], "saved in the session");
    let (x, y) = rt.find("verify-head").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(rt.find("verify-0").is_none(), "folded to its head");
    assert!(rt.find("verify-head").is_some());
}

#[test]
fn shift_h_folds_and_opens_the_checklist_and_its_rows_take_the_keyboard() {
    let mut rt = with_verify();
    press(
        &mut rt,
        Key::Text("H".into()),
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    assert!(rt.find("verify-0").is_none(), "folded to its head");
    press(
        &mut rt,
        Key::Text("H".into()),
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    assert!(rt.find("verify-0").is_some(), "open again");
    let focusable = |k: &str| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(k))
            .is_some_and(|e| e.focusable)
    };
    assert!(focusable("verify-head") && focusable("verify-0"));
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
fn escape_lets_go_of_a_focused_section() {
    let mut rt = with_verify();
    rt.update(|s, _| s.focus_section("verify"));
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.brief.focus.is_none());
}
