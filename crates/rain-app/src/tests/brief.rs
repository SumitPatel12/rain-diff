//! The Brief on the Overview tab, its cites and check, comments and questions on Brief sections,
//! Map nodes and notes, and the Map's marks.

use rain_core::brief::BriefDoc;
use rain_core::{AnchorState, AnswerStatus, EventKind, OrphanReason, Side, TargetKind, ThreadKind};
use rune::{Key, Mods, Runtime};

use super::*;
use crate::app::{ComposeMode, PeekTarget, ReaderTab};

const STORY_BRIEF: &str = include_str!("../../../../fixtures/brief/story.json");
/// A window narrow enough that the no-brief card can't fit its title and ask on one line.
const NARROW_CARD: (f32, f32) = (600.0, 900.0);

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
    rt.update(|s, tx| s.diagram_event(rain_diagram::DiagramEvent::Node("voucher-service".into()), tx));
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
    assert!(rt.state.layout.is_open(Overlay::Map), "the first press selects");
    rt.update(|s, tx| s.diagram_event(rain_diagram::DiagramEvent::Node("voucher-service".into()), tx));
    settle(&mut rt);
    assert!(!rt.state.layout.is_open(Overlay::Map), "the second opens the file");
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

fn brief_asks(rt: &Runtime<App>) -> Vec<rain_core::Event> {
    rt.state
        .shared
        .events_since(0, &[EventKind::Ask])
        .into_iter()
        .filter(|e| e.data["request"] == "brief")
        .collect()
}

fn attach_agent(rt: &mut Runtime<App>) {
    rt.state.shared.set_presence(rain_core::Presence {
        state: rain_core::PresenceState::Idle,
        seen_at: Some(rain_comments::clock::now_iso()),
        ..rain_core::Presence::default()
    });
}

fn live_el(rt: &Runtime<App>, key: &str) -> Option<rune::ElementInfo> {
    rt.elements().into_iter().find(|e| !e.exiting && e.key.as_deref() == Some(key))
}

#[test]
fn with_no_agent_attached_the_no_brief_card_shows_a_status_and_no_button() {
    let mut rt = runtime(app(story(), false));
    rt.update(|s, _| s.set_reader_tab(ReaderTab::Overview));
    settle(&mut rt);
    assert!(live_el(&rt, "ask-brief").is_none(), "no disabled button");
    let status = live_el(&rt, "brief-no-agent").expect("the status sits in the slot");
    assert!(
        status
            .help
            .as_deref()
            .is_some_and(|h| h.contains("rain brief set") && h.contains("docs/agent.md")),
        "the agent's how-to is in the tooltip: {:?}",
        status.help
    );
    let hint = live_el(&rt, "brief-no-agent-hint").expect("one hint line under the copy");
    assert!(hint.text.as_deref().is_some_and(|t| t.contains("rain skill")), "{:?}", hint.text);
    rt.update(|s, _| s.ask_for_brief());
    settle(&mut rt);
    assert!(brief_asks(&rt).is_empty(), "nothing is sent into the void");
    assert_eq!(rt.state.status, crate::app::NO_AGENT_FOR_BRIEF);
    assert!(rt.find("brief-asked").is_none());
}

#[test]
fn the_no_agent_status_turns_into_the_button_when_an_agent_attaches() {
    let mut rt = runtime(app(story(), false));
    rt.update(|s, _| s.set_reader_tab(ReaderTab::Overview));
    settle(&mut rt);
    let status = live_el(&rt, "brief-ask-slot").expect("the slot").rect;
    attach_agent(&mut rt);
    settle(&mut rt);
    assert!(live_el(&rt, "brief-no-agent").is_none());
    assert!(live_el(&rt, "brief-no-agent-hint").is_none(), "the hint goes with it");
    let button = live_el(&rt, "ask-brief").expect("the button replaces the status");
    assert_eq!(button.label.as_deref(), Some("Ask the agent to write one"));
    let slot = live_el(&rt, "brief-ask-slot").expect("the slot").rect;
    assert_eq!(slot[1], status[1], "the slot keeps its line");
    assert_eq!(slot[3], status[3], "and its height");
}

#[test]
fn waiting_for_the_brief_keeps_the_slot_in_place() {
    let mut rt = runtime(app(story(), false));
    attach_agent(&mut rt);
    rt.update(|s, _| s.set_reader_tab(ReaderTab::Overview));
    settle(&mut rt);
    let before = live_el(&rt, "brief-ask-slot").expect("the slot").rect;
    let card = live_el(&rt, "overview-empty").expect("the card").rect;
    click(&mut rt, "ask-brief");
    let waiting = live_el(&rt, "brief-asked").expect("the slot waits");
    let said: String = rt
        .elements()
        .into_iter()
        .filter(|e| !e.exiting && e.path.contains("brief-asked"))
        .filter_map(|e| e.text)
        .collect();
    assert!(said.contains("Waiting for the agent"), "{said}");
    assert!(
        rt.elements()
            .iter()
            .any(|e| !e.exiting && e.path.contains("brief-asked") && e.role == "progressbar"),
        "with a spinner"
    );
    let after = live_el(&rt, "brief-ask-slot").expect("the slot").rect;
    assert_eq!((after[1], after[3]), (before[1], before[3]), "no jump");
    assert_eq!(
        live_el(&rt, "overview-empty").expect("the card").rect[3],
        card[3],
        "the card keeps its height"
    );
    assert!(waiting.rect[0] + waiting.rect[2] <= card[0] + card[2], "inside the card");
}

#[test]
fn the_ask_sits_on_the_title_line_and_wraps_under_it_when_narrow() {
    let place = |size: (f32, f32)| {
        let mut rt = sized_runtime(app(story(), false), size);
        attach_agent(&mut rt);
        rt.update(|s, _| s.set_reader_tab(ReaderTab::Overview));
        settle(&mut rt);
        let title = live_el(&rt, "brief-empty-title").expect("the title").rect;
        let ask = live_el(&rt, "ask-brief").expect("the ask").rect;
        let card = live_el(&rt, "overview-empty").expect("the card").rect;
        (title, ask, card)
    };
    let (title, ask, card) = place((1600.0, 1000.0));
    assert!(
        ask[1] < title[1] + title[3] && title[1] < ask[1] + ask[3],
        "on the title line: {title:?} {ask:?}"
    );
    assert!(
        (card[0] + card[2] - 20.0 - (ask[0] + ask[2])).abs() < 1.0,
        "right-aligned: {ask:?} in {card:?}"
    );
    let (title, ask, card) = place(NARROW_CARD);
    assert!(ask[1] >= title[1] + title[3], "under the title: {title:?} {ask:?}");
    assert!((ask[0] - (card[0] + 20.0)).abs() < 1.0, "left-aligned: {ask:?} in {card:?}");
}

#[test]
fn the_no_brief_card_asks_the_attached_agent_and_waits_for_the_brief() {
    let mut rt = runtime(app(story(), false));
    let attach = attach_agent;
    attach(&mut rt);
    rt.update(|s, _| s.set_reader_tab(ReaderTab::Overview));
    settle(&mut rt);
    assert!(rt.find("brief-no-agent").is_none());
    click(&mut rt, "ask-brief");
    let asks = brief_asks(&rt);
    assert_eq!(asks.len(), 1, "one ask on the agent's channel");
    assert!(asks[0].thread.is_none(), "no thread, so the launcher never takes it");
    let question = asks[0].data["question"].as_str().unwrap_or_default().to_owned();
    assert!(question.contains("rain brief set"), "{question}");
    assert!(rt.find("brief-asked").is_some(), "the card says it is waiting");
    assert!(rt.find("ask-brief").is_none());
    assert!(
        rt.state.mirror.threads.iter().all(|t| t.messages[0].body != question),
        "no thread for it"
    );
    {
        let mut s = rt.state.shared.lock();
        s.brief = Some(story_brief());
        s.bump_brief();
    }
    attach(&mut rt);
    settle(&mut rt);
    assert!(rt.find("overview-empty").is_none(), "the brief replaces the card");
    assert!(!rt.state.brief.asked);
}

#[test]
fn enter_presses_the_focused_ask_button_and_ctrl_enter_does_not() {
    let mut rt = runtime(app(story(), false));
    attach_agent(&mut rt);
    rt.update(|s, _| s.set_reader_tab(ReaderTab::Overview));
    settle(&mut rt);
    assert!(tab_to(&mut rt, "ask-brief"), "Tab reaches the button");
    press(&mut rt, Key::Enter, ctrl());
    assert!(brief_asks(&rt).is_empty(), "Ctrl+Enter goes on to the app's shortcuts");
    press(&mut rt, Key::Enter, Mods::default());
    assert_eq!(brief_asks(&rt).len(), 1, "Enter presses it");
    assert!(rt.state.brief.asked);
}

#[test]
fn a_pending_ask_for_a_brief_holds_the_nudge_back_a_full_wait() {
    use std::time::{Duration, Instant};
    let mut rt = runtime(app(story(), false));
    settle(&mut rt);
    let t0 = Instant::now();
    attach_agent(&mut rt);
    rt.state.brief_nudge_at(t0);
    rt.update(|s, _| s.ask_for_brief());
    settle(&mut rt);
    let asked = Instant::now();
    assert!(rt.state.brief_nudge_at(t0 + Duration::from_secs(9)).is_some());
    assert!(
        rt.state.brief_nudge_at(t0 + Duration::from_secs(120)).is_some(),
        "the wait from attaching is not enough"
    );
    assert_eq!(nudges(&rt), 0, "no reminder while the ask is pending");
    assert_eq!(rt.state.brief_nudge_at(asked + Duration::from_secs(120)), None);
    assert_eq!(nudges(&rt), 1, "one reminder when the ask goes unanswered");
    rt.state.brief_nudge_at(asked + Duration::from_secs(600));
    assert_eq!(nudges(&rt), 1, "still once per session");
}

fn nudges(rt: &Runtime<App>) -> usize {
    rt.state.shared.events_since(0, &[EventKind::BriefMissing]).len()
}

#[test]
fn an_attached_agent_with_no_brief_is_nudged_once_after_two_minutes() {
    use std::time::{Duration, Instant};
    let mut rt = runtime(app(story(), false));
    settle(&mut rt);
    let t0 = Instant::now();
    assert_eq!(rt.state.brief_nudge_at(t0), None, "nobody attached, nothing to wait for");
    attach_agent(&mut rt);
    assert_eq!(
        rt.state.brief_nudge_at(t0),
        Some(crate::app::BRIEF_NUDGE_AFTER),
        "the clock starts on attach"
    );
    assert!(rt.state.brief_nudge_at(t0 + Duration::from_secs(119)).is_some());
    assert_eq!(nudges(&rt), 0, "none before two minutes");
    assert_eq!(rt.state.brief_nudge_at(t0 + Duration::from_secs(120)), None);
    let sent = rt.state.shared.events_since(0, &[EventKind::BriefMissing]);
    assert_eq!(sent.len(), 1, "one after");
    assert!(sent[0].thread.is_none() && sent[0].data["request"] == "brief");
    assert_eq!(serde_json::to_value(sent[0].kind).unwrap(), "brief-missing");
    rt.state.brief_nudge_at(t0 + Duration::from_secs(600));
    assert_eq!(nudges(&rt), 1, "once per session");
}

#[test]
fn a_brief_set_before_two_minutes_stops_the_nudge() {
    use std::time::{Duration, Instant};
    let mut rt = runtime(app(story(), false));
    settle(&mut rt);
    let t0 = Instant::now();
    attach_agent(&mut rt);
    rt.state.brief_nudge_at(t0);
    {
        let mut s = rt.state.shared.lock();
        s.brief = Some(story_brief());
        s.bump_brief();
    }
    settle(&mut rt);
    assert_eq!(rt.state.brief_nudge_at(t0 + Duration::from_secs(121)), None);
    assert_eq!(nudges(&rt), 0, "none after a brief");
}
