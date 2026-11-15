//! The Tour overlay: pull-started, spotlit steps, My pass first, leave and resume, the end card,
//! agent-driven steps, a reload that moves a step's code, and its scenes.

use std::path::{Path, PathBuf};

use rain_core::brief::BriefDoc;
use rain_core::{Anchor, Side, UiRequest};
use rune::{Env, Key, Mods, Runtime};

use super::*;
use crate::app::{Overlay, TourStage};

const VOUCHERS: &str = "server/src/Api/VouchersController.cs";
const SERVICE: &str = "server/src/Services/VoucherService.cs";
const ENTITY: &str = "server/src/Data/Voucher.cs";

/// Five steps over the story fixture, each on a range of added lines.
const BRIEF: &str = r#"{
  "format": "rain-brief/v1",
  "title": "Vouchers at checkout",
  "how": [{ "id": "how.1", "title": "The API records a voucher", "text": "x" }],
  "tour": [
    { "id": "t1", "title": "The endpoint", "text": "Validates the amount, then records it.", "cite": { "path": "server/src/Api/VouchersController.cs", "lines": [20, 27] } },
    { "id": "t2", "title": "Storing the voucher", "text": "One row in the same unit of work.", "cite": { "path": "server/src/Services/VoucherService.cs", "lines": [26, 30] } },
    { "id": "t3", "title": "Totals", "text": "Vouchers join the total.", "cite": { "path": "server/src/Services/AccountService.cs", "lines": [44, 49] } },
    { "id": "t4", "title": "The entity", "text": "The Voucher row.", "cite": { "path": "server/src/Data/Voucher.cs", "lines": [6, 12] } },
    { "id": "t5", "title": "Client service", "text": "Posts the voucher.", "cite": { "path": "app/src/app/vouchers/voucher.service.ts", "lines": [1, 11] } }
  ]
}"#;

fn with_brief(mut a: App) -> App {
    let mut doc: BriefDoc = serde_json::from_str(BRIEF).unwrap();
    doc.assign_ids();
    a.mutate(|s| {
        s.brief = Some(doc);
        s.bump_brief();
    });
    a
}

fn shift() -> Mods {
    Mods {
        shift: true,
        ..Mods::default()
    }
}

fn key(k: &str) -> Key {
    Key::Text(k.into())
}

/// `T`, then `Enter` on the start card: step 1.
fn begin(rt: &mut Runtime<App>) {
    press(rt, key("T"), shift());
    assert!(rt.state.layout.is_open(Overlay::Tour));
    assert_eq!(rt.state.tour.stage, TourStage::Start);
    press(rt, Key::Enter, Mods::default());
    assert_eq!(rt.state.tour.stage, TourStage::Step);
}

/// The spot's outline, as laid out, starts inside the upper third of the diff column.
fn in_top_third(rt: &Runtime<App>) -> bool {
    let els = rt.elements();
    let rect = |k: &str| els.iter().find(|e| e.key.as_deref() == Some(k)).map(|e| e.rect);
    let (Some(spot), Some(col)) = (rect("spot"), rect("diff-col")) else {
        return false;
    };
    let y = spot[1] - col[1];
    y >= 0.0 && y <= col[3] / 3.0
}

/// Two agent findings on step 2's range (VoucherService 27 and 29).
fn seed_findings(a: &mut App) -> Vec<String> {
    let g = a.index_of(SERVICE).unwrap();
    let (repo, path) = (a.repo_tag(g).to_owned(), a.diff.files()[g].path.clone());
    a.mutate(|s| {
        [27, 29]
            .map(|line| {
                rain_comments::ops::add_thread(
                    s,
                    &repo,
                    &path,
                    Anchor::Line { side: Side::New, line },
                    "F1 issue: the save is outside the account's unit of work.",
                    Some("claude"),
                    None,
                )
            })
            .to_vec()
    })
}

fn annotated(rt: &Runtime<App>, id: &str) -> bool {
    rt.state.active().annotations().iter().any(|a| a.id == id)
}

#[test]
fn step_text_draws_backticked_code_as_code_spans() {
    let mut a = with_brief(app(story(), true));
    a.mutate(|s| {
        let step = &mut s.brief.as_mut().unwrap().tour[0];
        step.title = "The `Record` endpoint".into();
        step.text = "Calls `VoucherService.cs` then `Save()` once.".into();
        s.bump_brief();
    });
    let mut rt = runtime(a);
    begin(&mut rt);
    let text: String = rt
        .elements()
        .into_iter()
        .filter(|e| !e.exiting && e.path.contains("tour-text"))
        .filter_map(|e| e.text)
        .collect();
    assert!(
        text.contains("Calls") && text.contains("VoucherService.cs") && text.contains("Save()") && text.contains("once."),
        "{text}"
    );
    let raw: Vec<String> = rt
        .elements()
        .into_iter()
        .filter(|e| e.path.contains("tour") && e.text.as_deref().is_some_and(|t| t.contains('`')))
        .map(|e| e.path)
        .collect();
    assert!(raw.is_empty(), "raw backticks on the tour card: {raw:?}");
}

#[test]
fn the_tour_never_starts_by_itself() {
    let mut rt = runtime(with_brief(app(story(), true)));
    settle(&mut rt);
    assert!(!rt.state.layout.is_open(Overlay::Tour));
    assert!(rt.find("overlay-tour").is_none());
    assert!(rt.state.active().spotlight().is_none());
    rt.update(|s, _| s.reload());
    settle(&mut rt);
    assert!(!rt.state.layout.is_open(Overlay::Tour), "nor after a reload");
    rt.update(|s, _| s.set_reader_tab(crate::app::ReaderTab::Overview));
    settle(&mut rt);
    assert!(rt.find("brief-tour").is_some(), "the Overview offers it");
}

#[test]
fn the_start_card_says_how_long_and_my_pass_first_is_on() {
    let mut rt = runtime(with_brief(app(story(), true)));
    press(&mut rt, key("T"), shift());
    assert!(rt.find("tour-start").is_some());
    assert!(rt.state.tour.my_pass_first);
    let len = rt.state.tour_minutes();
    assert!(len >= 1);
    assert!(rt.state.active().spotlight().is_none(), "nothing is spotlit before Start");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.layout.is_open(Overlay::Tour), "Skip to the reader");
    assert!(!rt.state.tour.resumable);
}

#[test]
fn space_three_times_from_step_one_lands_on_step_four_spotlit_in_the_top_third() {
    let mut rt = runtime(with_brief(app(story(), true)));
    begin(&mut rt);
    assert_eq!(rt.state.tour.step, 0);
    let g = rt.state.index_of(VOUCHERS).unwrap();
    let spot = rt.state.active().spotlight().cloned().expect("step 1 spotlit");
    assert_eq!(
        (spot.file, spot.lines, spot.step),
        (rt.state.local(g).unwrap(), (20, 27), Some((1, 5)))
    );
    for _ in 0..3 {
        press(&mut rt, key(" "), Mods::default());
    }
    assert_eq!(rt.state.tour.step, 3);
    let g = rt.state.index_of(ENTITY).unwrap();
    let spot = rt.state.active().spotlight().cloned().expect("step 4 spotlit");
    assert_eq!((spot.file, spot.lines), (rt.state.local(g).unwrap(), (6, 12)));
    assert_eq!(rt.state.current(), Some(g));
    assert!(in_top_third(&rt), "the range sits in the upper third");
    assert!(rt.state.active().dimmed(0) || rt.state.local(g) == Some(0), "the rest dims");
    assert_eq!(rt.state.tour.understood.len(), 3);
    assert!(
        rt.state.is_viewed(rt.state.index_of(VOUCHERS).unwrap()),
        "understood on the file's last range marks it viewed"
    );
    assert!(rt.find("tour-step").is_some());
}

#[test]
fn n_and_p_move_without_marking() {
    let mut rt = runtime(with_brief(app(story(), true)));
    begin(&mut rt);
    press(&mut rt, key("n"), Mods::default());
    press(&mut rt, key("n"), Mods::default());
    assert_eq!(rt.state.tour.step, 2);
    press(&mut rt, key("p"), Mods::default());
    assert_eq!(rt.state.tour.step, 1);
    assert!(rt.state.tour.understood.is_empty());
    assert_eq!(rt.state.active().spotlight().unwrap().lines, (26, 30));
}

#[test]
fn my_pass_first_hides_the_seeded_findings_on_step_two_until_space() {
    let mut a = with_brief(app(story(), true));
    let ids = seed_findings(&mut a);
    let mut rt = runtime(a);
    assert!(ids.iter().all(|id| annotated(&rt, id)), "shown before the tour");
    begin(&mut rt);
    press(&mut rt, key("n"), Mods::default());
    assert_eq!(rt.state.tour.step, 1);
    assert_eq!(rt.state.tour_findings(1).len(), 2);
    assert!(ids.iter().all(|id| !annotated(&rt, id)), "hidden on the step");
    assert!(rt.find("tour-findings-hidden").is_some());
    assert!(ids.iter().all(|id| rt.find(&format!("tour-finding-{id}")).is_none()));

    press(&mut rt, key(" "), Mods::default());
    assert_eq!(rt.state.tour.step, 1, "the first Space shows them");
    assert!(ids.iter().all(|id| annotated(&rt, id)));
    for id in &ids {
        assert!(rt.find(&format!("tour-finding-{id}")).is_some());
    }
    assert!(rt.state.tour_findings(1).iter().all(|f| f.who == crate::app::FoundBy::Agent));

    press(&mut rt, key(" "), Mods::default());
    assert_eq!(rt.state.tour.step, 2, "the second is understood + next");

    let mut a = with_brief(app(story(), true));
    let ids = seed_findings(&mut a);
    a.tour.my_pass_first = false;
    let mut rt = runtime(a);
    begin(&mut rt);
    press(&mut rt, key("n"), Mods::default());
    assert!(ids.iter().all(|id| annotated(&rt, id)), "shown at once without My pass first");
}

#[test]
fn esc_leaves_keeping_the_place_and_enter_resumes_without_moving_the_reader() {
    let mut rt = runtime(with_brief(app(story(), true)));
    begin(&mut rt);
    press(&mut rt, key("n"), Mods::default());
    let (offset, cursor) = (rt.state.active().scroll_offset(), rt.state.active().cursor());
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.layout.is_open(Overlay::Tour));
    assert_eq!(rt.state.status, "Resume tour 2/5 · Enter");
    assert!(rt.state.active().spotlight().is_none(), "leaving takes the spotlight down");
    assert_eq!(rt.state.active().scroll_offset(), offset);

    press(&mut rt, Key::Enter, Mods::default());
    assert!(rt.state.layout.is_open(Overlay::Tour));
    assert_eq!((rt.state.tour.stage, rt.state.tour.step), (TourStage::Step, 1));
    assert_eq!(rt.state.active().scroll_offset(), offset, "the reader did not move");
    assert_eq!(rt.state.active().cursor(), cursor);
    assert_eq!(rt.state.active().spotlight().unwrap().lines, (26, 30));

    press(&mut rt, Key::Escape, Mods::default());
    press(&mut rt, key("T"), shift());
    assert_eq!((rt.state.tour.stage, rt.state.tour.step), (TourStage::Step, 1), "T resumes too");
}

#[test]
fn the_end_card_lists_the_files_the_tour_never_cites() {
    let mut rt = runtime(with_brief(app(story(), true)));
    begin(&mut rt);
    rt.update(|s, tx| s.tour_goto(4, tx));
    settle(&mut rt);
    press(&mut rt, key(" "), Mods::default());
    assert_eq!(rt.state.tour.stage, TourStage::End);
    assert!(rt.find("tour-end").is_some());
    assert!(rt.state.active().spotlight().is_none());
    let left: Vec<String> = rt.state.tour_uncovered().iter().map(|&g| rt.state.keys[g].clone()).collect();
    for k in [
        "app/src/app/account/account-page.component.ts",
        "app/src/app/vouchers/voucher-form.component.ts",
    ] {
        assert!(left.iter().any(|x| x == k), "{k} in {left:?}");
        assert!(rt.find(&format!("tour-uncovered-{k}")).is_some());
    }
    for cited in [VOUCHERS, SERVICE, ENTITY] {
        assert!(!left.iter().any(|x| x == cited));
    }
    assert!(!left.iter().any(|k| rt.state.is_noise(k)), "groups accepted unread are not listed");
    assert!(rt.state.status.contains(&format!("{} file(s)", left.len())));
    let first = rt.state.tour_uncovered()[0];
    rt.update(|s, tx| s.tour_jump(first, tx));
    settle(&mut rt);
    assert_eq!(rt.state.current(), Some(first));
}

#[test]
fn a_highlight_with_a_step_opens_the_tour_at_that_step() {
    let mut rt = runtime(with_brief(app(story(), true)));
    rt.state.shared.push_request(UiRequest::Highlight {
        path: SERVICE.into(),
        lines: (27, 29),
        side: Side::New,
        caption: Some("The save".into()),
        step: Some((2, 5)),
    });
    settle(&mut rt);
    assert!(rt.state.layout.is_open(Overlay::Tour));
    assert_eq!((rt.state.tour.stage, rt.state.tour.step), (TourStage::Step, 1));
    let spot = rt.state.active().spotlight().cloned().unwrap();
    assert_eq!((spot.lines, spot.step), ((27, 29), Some((2, 5))));
    assert!(in_top_third(&rt));

    rt.state.shared.push_request(UiRequest::Highlight {
        path: "app/src/app/vouchers/voucher-form.component.ts".into(),
        lines: (3, 5),
        side: Side::New,
        caption: Some("The form".into()),
        step: Some((7, 9)),
    });
    settle(&mut rt);
    let live = rt.state.tour_current().unwrap();
    assert!(live.live, "a step the brief does not have is the agent's own");
    assert_eq!(live.title, "The form");
    assert!(rt.find("tour-step").is_some());
    assert_eq!(rt.state.active().spotlight().unwrap().lines, (3, 5));

    rt.state.shared.push_request(UiRequest::Highlight {
        path: VOUCHERS.into(),
        lines: (22, 23),
        side: Side::New,
        caption: None,
        step: None,
    });
    settle(&mut rt);
    assert!(rt.state.status.starts_with("Claude: look at"), "no step: the plain spotlight");
}

#[test]
fn the_picker_opens_the_tour_at_a_step() {
    let mut rt = runtime(with_brief(app(story(), true)));
    rt.update(|s, _| s.show_picker("totals"));
    settle(&mut rt);
    let rows = rt.state.picker_rows();
    let at = rows.iter().position(|e| e.kind == "Tour").expect("a tour row");
    rt.state.picker.sel = at;
    press(&mut rt, Key::Enter, Mods::default());
    assert!(rt.state.layout.is_open(Overlay::Tour));
    assert_eq!(rt.state.tour.step, 2);
}

/// The story fixture with VoucherService's hunk two lines lower on the new side.
fn shifted_fixture(dir: &Path) -> PathBuf {
    let text = std::fs::read_to_string(story()).unwrap();
    let moved = text.replace(
        "@@ -23,6 +23,11 @@ public class VoucherService",
        "@@ -23,6 +25,11 @@ public class VoucherService",
    );
    assert_ne!(moved, text);
    let p = dir.join("story-moved.patch");
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(&p, moved).unwrap();
    p
}

#[test]
fn after_a_reload_a_step_whose_code_moved_reads_diff_changed_and_follows_it() {
    let dir = temp_dir();
    let fixture = dir.join("story.patch");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(story(), &fixture).unwrap();
    let mut rt = runtime(with_brief(app(fixture.clone(), true)));
    begin(&mut rt);
    let moved = shifted_fixture(&dir);
    std::fs::copy(&moved, &fixture).unwrap();
    rt.update(|s, _| s.reload());
    settle(&mut rt);
    let views = rt.state.tour_views();
    let two = views[1].at.clone().unwrap();
    assert!(two.changed, "step 2 reads Diff changed");
    assert_eq!(two.lines, (28, 32), "and follows its code");
    assert!(!views[0].at.as_ref().unwrap().changed, "step 1 did not move");
    assert_eq!(rt.state.tour.step, 0);
    assert_eq!(
        rt.state.active().spotlight().unwrap().lines,
        (20, 27),
        "the spotlight is back after the reload"
    );
    press(&mut rt, key("n"), Mods::default());
    assert!(rt.find("tour-changed").is_some());
    assert_eq!(rt.state.active().spotlight().unwrap().lines, (28, 32));
    assert!(rt.state.status.contains("Diff changed"));
}

#[test]
fn progress_is_kept_per_session() {
    let dir = temp_dir();
    let open = || {
        let mut a = App::new(
            rain_core::Launch {
                session: "t".into(),
                fixture: Some(story()),
                overview: Some(overview()),
                ..rain_core::Launch::default()
            },
            std::sync::Arc::new(rain_core::Shared::default()),
            Some(dir.join("sessions")),
            None,
        );
        a.load_now();
        with_brief(a)
    };
    let mut rt = runtime(open());
    begin(&mut rt);
    press(&mut rt, key(" "), Mods::default());
    press(&mut rt, key(" "), Mods::default());
    assert_eq!(rt.state.tour.step, 2);
    drop(rt);

    let mut rt = runtime(open());
    assert!(!rt.state.layout.is_open(Overlay::Tour), "kept, not started");
    press(&mut rt, key("T"), shift());
    assert_eq!((rt.state.tour.stage, rt.state.tour.step), (TourStage::Step, 2));
    assert_eq!(rt.state.tour.understood.len(), 2);
}

#[test]
fn the_tour_scenes_draw_in_both_themes_at_both_sizes() {
    for (name, want) in [("tour-start", "tour-start"), ("tour-step", "tour-step"), ("tour-end", "tour-end")] {
        for size in [(1366.0, 768.0), (1920.0, 1080.0)] {
            for dark in [true, false] {
                let mut a = app(story(), true);
                shell_scene(name)(&mut a);
                let mut rt = Runtime::new(a, view, size, Env::new(dark, 1.0));
                rt.poll = Some(|s: &mut App, _, tx| s.poll(tx));
                rt.frame(0.0);
                settle(&mut rt);
                let card = rt
                    .elements()
                    .into_iter()
                    .find(|e| e.key.as_deref() == Some("overlay-tour"))
                    .unwrap_or_else(|| panic!("{name} {size:?} {dark}"));
                assert!(rt.find(want).is_some(), "{name}");
                assert!((card.rect[2] - 360.0).abs() < 1.0, "360 pt: {:?}", card.rect);
                assert!(card.rect[1] + card.rect[3] <= size.1, "fits: {:?}", card.rect);
                assert!(
                    card.rect[0] + card.rect[2] >= size.0 - 40.0,
                    "docked at the reader's right, where lines end: {:?}",
                    card.rect
                );
                assert!(rt.find("scrim-tour").is_none(), "no scrim over the reader");
                if name == "tour-step" {
                    let spot = rt.state.active().spotlight().cloned().expect("spotlit");
                    assert_eq!(spot.step.map(|s| s.0), Some(1));
                    assert!(rt.find("tour-findings-hidden").is_some(), "the seeded findings wait");
                }
            }
        }
    }
}
