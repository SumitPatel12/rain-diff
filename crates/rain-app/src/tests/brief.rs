//! The Brief on the Overview tab, its cites and check, comments and questions on Brief sections,
//! Map nodes and notes, and the Map's marks.

use rain_core::brief::BriefDoc;
use rain_core::{AnchorState, AnswerStatus, EventKind, OrphanReason, Side, TargetKind, ThreadKind};
use rune::{Key, Mods, Runtime};

use super::*;
use crate::app::{ComposeMode, PeekTarget, ReaderTab};

const STORY_BRIEF: &str = include_str!("../../../../fixtures/brief/story.json");

fn story_brief() -> BriefDoc {
    let mut d: BriefDoc = serde_json::from_str(STORY_BRIEF).unwrap();
    d.assign_ids();
    d
}

fn with_brief(overview: bool) -> Runtime<App> {
    let mut rt = runtime(app(story(), overview));
    rt.update(|s, _| {
        s.mutate(|ss| {
            ss.brief = Some(story_brief());
            ss.bump_brief();
        });
        s.set_reader_tab(ReaderTab::Overview);
    });
    settle(&mut rt);
    rt
}

fn click(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("{key} is drawn"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

#[test]
fn the_overview_shows_every_section_under_headings_and_b_switches_to_the_changes() {
    let mut rt = with_brief(false);
    for k in [
        "overview-title",
        "h2-intent",
        "h2-how",
        "h2-invariants",
        "h2-not-changed",
        "h2-not-covered",
        "h2-verify",
        "brief-facts",
        "brief-sec-intent",
        "brief-sec-how.1",
        "brief-sec-how.2",
        "brief-sec-how.3",
        "brief-sec-inv.1",
        "brief-sec-not-changed",
        "brief-sec-not-covered",
        "brief-sec-verify",
        "brief-tour",
    ] {
        assert!(rt.find(k).is_some(), "{k}");
    }
    let headings: Vec<String> = rt
        .elements()
        .into_iter()
        .filter(|e| e.role == "heading" && !e.exiting)
        .filter_map(|e| e.text)
        .collect();
    for h in [
        "Vouchers at checkout",
        "Intent",
        "How it works",
        "1. The API records a voucher",
        "2. One Voucher row in the same unit of work",
        "Invariants",
        "Verify by hand",
    ] {
        assert!(headings.iter().any(|x| x == h), "{h} in {headings:?}");
    }
    typed(&mut rt, "o");
    assert_eq!(rt.state.reader_tab(), ReaderTab::Changes);
    assert!(rt.find("brief-sec-how.2").is_none());
    assert!(rt.find("diff-col").is_some(), "Changes shows the diff, no Brief above it");
    typed(&mut rt, "o");
    assert!(rt.find("brief-sec-how.2").is_some());
}

#[test]
fn facts_come_from_triage_and_count_the_uncited() {
    let rt = with_brief(false);
    let f = &rt.state.brief.facts;
    assert_eq!(f.files, rt.state.keys.len());
    assert!(f.migrations >= 1);
    let c = rt.state.brief_check().unwrap();
    assert_eq!(c.phantom_paths, ["server/src/Services/Missing.cs"]);
    assert!(
        !c.files_not_cited
            .iter()
            .any(|k| k.contains("/generated/") || k.ends_with(".Designer.cs")),
        "notCovered and generated files are not asked for: {:?}",
        c.files_not_cited
    );
    assert!(c.files_not_cited.iter().any(|k| k.ends_with("account-page.component.ts")));
    let installed = rt.state.shared.brief_check(rt.state.brief_doc().unwrap());
    assert_eq!(
        installed["phantomPaths"][0], "server/src/Services/Missing.cs",
        "GET /api/brief reads the same check"
    );
}

#[test]
fn a_cite_chip_moves_the_cursor_to_its_lines() {
    let mut rt = with_brief(false);
    click(&mut rt, "cite-how.2-0");
    assert_eq!(rt.state.reader_tab(), ReaderTab::Changes, "a cite opens the Changes tab");
    assert_eq!(rt.state.current_key().as_deref(), Some("server/src/Services/VoucherService.cs"));
    let (_, at) = rt.state.active().cursor().expect("a cursor");
    assert_eq!((at.side, at.line), (Side::New, 27));
}

#[test]
fn a_code_span_resolves_and_peeks_the_cited_lines_with_six_around() {
    let mut rt = with_brief(false);
    let peek = rt.state.peek_content(&PeekTarget {
        key: "server/src/Services/VoucherService.cs".into(),
        lines: Some((27, 27)),
        side: Side::New,
        anchor: "x".into(),
        label: "VoucherService.cs:27".into(),
    });
    let cited: Vec<u32> = peek.rows.iter().filter(|r| r.cited).map(|r| r.new).collect();
    assert_eq!(cited, [27]);
    let at = peek.rows.iter().position(|r| r.cited).unwrap();
    assert!(
        at <= 6 && peek.rows.len() - at - 1 <= 6,
        "six lines each side at most: {}",
        peek.rows.len()
    );
    assert!(peek.rows.len() >= 7);
    assert_eq!(
        peek.lang.as_deref(),
        rain_diffview::language("server/src/Services/VoucherService.cs")
    );
    rt.update(|s, tx| s.goto_cite("VouchersController.cs", Some((23, 23)), Side::New, tx));
    settle(&mut rt);
    assert_eq!(rt.state.current_key().as_deref(), Some("server/src/Api/VouchersController.cs"));
    rt.update(|s, tx| s.goto_cite("server/src/Services/Missing.cs", None, Side::New, tx));
    assert!(rt.state.status.contains("not in this diff"));
}

#[test]
fn c_on_a_focused_section_comments_on_it_and_the_card_sits_under_it() {
    let mut rt = with_brief(false);
    click(&mut rt, "sec-head-how.2");
    assert_eq!(rt.state.brief.focus.as_deref(), Some("how.2"));
    typed(&mut rt, "c");
    let d = rt.state.brief.composer.clone().expect("the artifact composer");
    assert_eq!(d.mode, ComposeMode::Comment);
    assert_eq!(d.title, "Brief › How 2");
    assert!(rt.find("artifact-composer").is_some());
    assert!(rt.state.composer.is_none(), "not the diff's composer");
    typed(&mut rt, "Why the same UoW? c");
    assert_eq!(
        rt.state.brief.edit.text, "Why the same UoW? c",
        "keys go to the text, not to commands"
    );
    press(&mut rt, Key::Enter, ctrl());
    let t = rt.state.shared.lock().threads.last().cloned().expect("a thread");
    assert_eq!(t.file_path, "@brief");
    let target = t.target.clone().unwrap();
    assert_eq!(target.kind, TargetKind::Brief);
    assert_eq!(target.section.as_deref(), Some("how.2"));
    assert_eq!(t.kind, Some(ThreadKind::Comment));
    assert!(
        t.code_snapshot.as_ref().unwrap().content.contains("same unit of work"),
        "the section text is the snapshot"
    );
    assert!(t.answer.is_none());
    assert!(
        rt.find(&format!("athread-{}", t.id)).is_some(),
        "the card is drawn under the section"
    );
    assert_eq!(rt.state.section_threads("how.2").len(), 1);
    assert!(rt.find("sec-dot").is_some());
}

#[test]
fn question_mode_queues_an_answer_and_tells_the_agent() {
    let mut rt = with_brief(false);
    click(&mut rt, "sec-head-how.1");
    typed(&mut rt, "?");
    assert_eq!(rt.state.brief.composer.as_ref().map(|d| d.mode), Some(ComposeMode::Question));
    typed(&mut rt, "Where is the amount checked?");
    press(&mut rt, Key::Enter, ctrl());
    let t = rt.state.shared.lock().threads.last().cloned().unwrap();
    assert_eq!(t.kind, Some(ThreadKind::Question));
    assert_eq!(t.answer.as_ref().map(|a| a.status), Some(AnswerStatus::Queued));
    let asks = rt.state.shared.events_since(0, &[EventKind::Ask]);
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0].thread.as_deref(), Some(t.id.as_str()));
    assert_eq!(asks[0].data["target"]["section"], "how.1");
}

#[test]
fn at_claude_in_a_comment_makes_it_a_question_and_esc_cancels() {
    let mut rt = with_brief(false);
    rt.update(|s, _| s.comment_on_section("verify", None, ComposeMode::Comment));
    settle(&mut rt);
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.brief.composer.is_none());
    rt.update(|s, _| s.comment_on_section("verify", None, ComposeMode::Comment));
    rt.update(|s, _| s.submit_artifact("@claude is a 0 voucher allowed?".into()));
    let t = rt.state.shared.lock().threads.last().cloned().unwrap();
    assert_eq!(t.kind, Some(ThreadKind::Question));
}

#[test]
fn moving_the_cursor_takes_c_back_to_the_diff() {
    let mut rt = with_brief(false);
    click(&mut rt, "sec-head-how.3");
    typed(&mut rt, "j");
    typed(&mut rt, "c");
    assert!(rt.state.brief.composer.is_none(), "the focus went stale when the cursor moved");
    assert!(rt.state.composer.is_some(), "c comments on the diff line");
}

#[test]
fn a_removed_section_orphans_its_thread_with_the_text_it_was_on() {
    let mut rt = with_brief(false);
    rt.update(|s, _| s.comment_on_section("how.2", None, ComposeMode::Comment));
    rt.update(|s, _| s.submit_artifact("Is this really one transaction?".into()));
    let id = rt.state.shared.lock().threads.last().unwrap().id.clone();
    rt.update(|s, _| {
        s.mutate(|ss| {
            let mut d = story_brief();
            d.how.remove(1);
            ss.brief = Some(d);
            ss.bump_brief();
            rain_comments::ops::reanchor_targets(ss);
        });
    });
    settle(&mut rt);
    let t = rt.state.shared.lock().threads.iter().find(|t| t.id == id).cloned().unwrap();
    assert_eq!(t.anchor_state, AnchorState::Orphaned);
    let o = t.orphan.unwrap();
    assert_eq!(o.reason, OrphanReason::SectionRemoved);
    assert!(o.snapshot.content.starts_with("One Voucher row in the same unit of work"));
    assert!(rt.find(&format!("orphan-{id}")).is_some(), "shown with its last-seen text");
    assert!(rt.find("brief-orphans").is_some());
    rt.update(|_, tx| tx.scroll_into_view(format!("orphan-{id}")));
    settle(&mut rt);
    let els = rt.elements();
    let orphans = els.iter().find(|e| e.key.as_deref() == Some("brief-orphans")).unwrap();
    assert!(orphans.path.contains("overview-text"), "at the end of the page: {}", orphans.path);
    let card = els
        .iter()
        .find(|e| e.key.as_deref() == Some(format!("orphan-{id}").as_str()))
        .unwrap();
    assert!(
        card.visible[3] >= card.rect[3] - 0.5,
        "the card is not clipped: {:?} of {:?}",
        card.visible,
        card.rect
    );
}

#[test]
fn without_a_brief_the_story_note_stands_in_and_takes_comments() {
    let mut rt = runtime(app(story(), false));
    rt.update(|s, _| {
        crate::seed_notes(s);
        s.set_reader_tab(ReaderTab::Overview);
    });
    settle(&mut rt);
    assert!(rt.find("story-notes").is_some());
    assert!(rt.find("overview-empty").is_some(), "how to get a brief");
    assert!(rt.find("brief-facts").is_some());
    let id = rt.state.story_notes()[0].id.clone();
    click(&mut rt, "note-comment");
    assert_eq!(rt.state.brief.composer.as_ref().map(|d| d.target.kind), Some(TargetKind::Note));
    rt.update(|s, _| s.submit_artifact("Which AC is this?".into()));
    let t = rt.state.shared.lock().threads.last().cloned().unwrap();
    assert_eq!(t.file_path, format!("@note:{id}"));
    assert_eq!(t.target.unwrap().note.as_deref(), Some(id.as_str()));
    assert_eq!(rt.state.note_threads(&id).len(), 1);
}

#[test]
fn map_nodes_take_comments_and_show_viewed_shares() {
    let mut rt = with_brief(true);
    typed(&mut rt, "m");
    assert!(rt.state.layout.is_open(Overlay::Map));
    rt.update(|s, tx| {
        s.diagram_event(
            rain_diagram::DiagramEvent::Hover(Some(rain_diagram::ItemId::Node("voucher-service".into()))),
            tx,
        )
    });
    settle(&mut rt);
    assert_eq!(rt.state.diagram.selected.as_deref(), Some("voucher-service"));
    assert!(rt.find("map-pane").is_some());
    typed(&mut rt, "c");
    let d = rt.state.brief.composer.clone().expect("composer on the node");
    assert_eq!(d.target.node.as_deref(), Some("voucher-service"));
    assert!(d.title.starts_with("Map › "));
    typed(&mut rt, "Should this be async?");
    press(&mut rt, Key::Enter, ctrl());
    let t = rt.state.shared.lock().threads.last().cloned().unwrap();
    assert_eq!(t.file_path, "@map");
    assert_eq!(t.target.as_ref().unwrap().kind, TargetKind::Node);
    assert_eq!(rt.state.map_marks()["voucher-service"].threads, 1);
    assert_eq!(rt.state.map_marks()["voucher-service"].viewed, 0.0);
    let g = rt.state.index_of("server/src/Services/VoucherService.cs").unwrap();
    rt.update(|s, _| s.set_viewed(g, true));
    settle(&mut rt);
    assert_eq!(rt.state.map_marks()["voucher-service"].viewed, 1.0);
    assert!(rt.find("mark:voucher-service#2").is_some(), "tint, bar and badge are drawn");
}

#[test]
fn map_tabs_never_repeat_and_a_press_still_opens_the_file() {
    let mut rt = with_brief(true);
    typed(&mut rt, "m");
    let names = rain_overview::tab_names(&rt.state.overview.as_ref().unwrap().targets);
    let unique: std::collections::BTreeSet<&String> = names.iter().collect();
    assert_eq!(unique.len(), names.len(), "{names:?}");
    assert!(rt.find("map-tabs-0").is_some());
    assert!(rt.find("map-legend").is_some());
    rt.update(|s, tx| s.diagram_event(rain_diagram::DiagramEvent::Node("voucher-service".into()), tx));
    settle(&mut rt);
    assert!(!rt.state.layout.is_open(Overlay::Map));
    assert_eq!(rt.state.current_key().as_deref(), Some("server/src/Services/VoucherService.cs"));
}

#[test]
fn braces_step_through_the_map_views_and_the_tabs_take_the_keyboard() {
    let mut rt = with_brief(true);
    typed(&mut rt, "}");
    assert!(rt.state.status.contains("m opens the Map"), "{}", rt.state.status);
    typed(&mut rt, "m");
    let shown: Vec<usize> = (0..rt.state.layouts.len()).filter(|&i| rt.state.layouts[i].is_some()).collect();
    assert!(shown.len() > 1, "the overview fixture has several views");
    assert_eq!(rt.state.diagram.target, shown[0]);
    typed(&mut rt, "}");
    assert_eq!(rt.state.diagram.target, shown[1]);
    typed(&mut rt, "{");
    typed(&mut rt, "{");
    assert_eq!(rt.state.diagram.target, *shown.last().unwrap(), "wraps round");
    let tab = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("map-tabs-0")).unwrap();
    assert!(tab.focusable);
    let palette: Vec<String> = crate::palette::filtered(&rt.state, "map view")
        .into_iter()
        .map(|e| format!("{} {}", e.label, e.hint))
        .collect();
    assert!(palette.contains(&"Next map view }".to_owned()), "{palette:?}");
    assert!(palette.contains(&"Previous map view {".to_owned()), "{palette:?}");
}

#[test]
fn the_map_never_fits_its_titles_under_eleven_points() {
    let mut rt = sized_runtime(app(story(), true), (1440.0, 900.0));
    rt.update(|s, _| s.toggle_map());
    settle(&mut rt);
    let l = rt.state.layouts[rt.state.diagram.target].clone().unwrap();
    let canvas = rt
        .elements()
        .into_iter()
        .find(|e| !e.exiting && e.key.as_deref() == Some("diagram-canvas"))
        .unwrap();
    let title = canvas.rect[2] / l.width * rain_diagram::design::TITLE_SIZE;
    assert!(title >= 11.0 - 0.01, "titles at {title} pt");
}

#[test]
fn a_narrow_map_stacks_its_lanes() {
    let a = app(story(), true);
    let mut rt = sized_runtime(a, (700.0, 600.0));
    rt.update(|s, _| s.toggle_map());
    settle(&mut rt);
    assert!(rt.find("map-stacked").is_some(), "lanes stack under 700 pt of diagram");
    let mut wide = sized_runtime(app(story(), true), (1920.0, 1080.0));
    wide.update(|s, _| s.toggle_map());
    settle(&mut wide);
    assert!(wide.find("map-stacked").is_none());
}

fn scene_runtime(name: &str, dark: bool) -> Runtime<App> {
    let setup = crate::spec().scenes.iter().find(|(n, _)| *n == name).map(|(_, f)| *f).unwrap();
    let mut a = app(story(), true);
    setup(&mut a);
    let mut rt = Runtime::new(a, view, (1920.0, 1080.0), rune::Env::new(dark, 1.0));
    rt.on_key = Some(on_key);
    rt.poll = Some(|s: &mut App, _, tx| s.poll(tx));
    rt.frame(0.0);
    settle(&mut rt);
    rt
}

#[test]
fn the_brief_and_map_scenes_seed_what_they_show() {
    for dark in [true, false] {
        let rt = scene_runtime("overview", dark);
        assert_eq!(rt.state.reader_tab(), ReaderTab::Overview);
        assert!(rt.find("brief-sec-how.2").is_some());
        assert!(rt.find("artifact-composer").is_some(), "a Question composer under how.2");
        assert_eq!(rt.state.section_threads("how.2").len(), 1);
        assert_eq!(rt.state.section_threads("how.1").len(), 1, "the answered question");
        let rt = scene_runtime("overview-empty", dark);
        assert!(rt.find("overview-empty").is_some());
        assert!(rt.find("brief-sec-how.2").is_none());
        let rt = scene_runtime("peek", dark);
        assert!(rt.find("peek").is_some(), "the peek scene opens a peek");
        assert!(rt.find("peek-row-0").is_some());
        let rt = scene_runtime("map", dark);
        assert!(rt.find("overlay-map").is_some());
        assert!(rt.find("map-pane").is_some());
        assert_eq!(rt.state.map_marks()["voucher-service"].threads, 1);
    }
}

#[test]
fn tour_steps_follow_the_brief() {
    let rt = with_brief(false);
    let steps = rt.state.tour_steps();
    assert_eq!(steps.len(), 6, "one per how[] cite without a tour");
    assert_eq!(steps[0].key.as_deref(), Some("server/src/Api/VouchersController.cs"));
    assert_eq!(steps[5].key, None, "a cite to no diff file");
}
