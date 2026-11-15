//! The Overview and Changes tabs: which one a session opens on, `b` and the palette, jumps that
//! switch to Changes, and the Markdown a section body renders (demoted headings, cite links).

use rain_core::Side;
use rain_core::brief::BriefDoc;
use rain_core::command::Command;
use rune::{Mods, Runtime};

use super::*;
use crate::app::{ComposeMode, ReaderTab};
use crate::view::{demote_headings, inline_cites, link_cites, parse_cite};

const STORY_BRIEF: &str = include_str!("../../../../fixtures/brief/story.json");

fn story_brief() -> BriefDoc {
    let mut d: BriefDoc = serde_json::from_str(STORY_BRIEF).unwrap();
    d.assign_ids();
    d
}

/// A session that already holds the story brief when the window loads.
fn opened_with_brief() -> Runtime<App> {
    runtime(brief_app())
}

fn brief_app() -> App {
    let dir = temp_dir();
    let shared = Arc::new(Shared::default());
    shared.lock().brief = Some(story_brief());
    let mut a = App::new(
        Launch {
            session: "t".into(),
            fixture: Some(story()),
            ..Launch::default()
        },
        shared,
        None,
        Some(dir.join("settings.json")),
    );
    a.load_now();
    a
}

#[test]
fn a_session_with_a_brief_opens_on_the_overview_and_one_without_on_the_changes() {
    let rt = opened_with_brief();
    assert_eq!(rt.state.reader_tab(), ReaderTab::Overview);
    assert!(rt.find("overview").is_some());
    assert!(rt.find("diff-col").is_none(), "no diff under the Overview");
    let plain = runtime(app(story(), false));
    assert_eq!(plain.state.reader_tab(), ReaderTab::Changes);
    assert!(plain.find("diff-col").is_some());
    assert!(plain.find("brief-facts").is_none(), "no Brief above the diff");
    assert!(plain.find("tab-strip").is_some());
}

#[test]
fn story_notes_alone_open_the_overview_and_a_reload_keeps_the_tab() {
    let launch = Launch {
        session: "t".into(),
        fixture: Some(story()),
        ..Launch::default()
    };
    let shared = Arc::new(Shared::default());
    let mut first = App::new(launch.clone(), shared.clone(), None, None);
    first.load_now();
    assert_eq!(first.reader_tab(), ReaderTab::Changes, "nothing to tell yet");
    crate::seed_notes(&mut first);
    assert_eq!(first.reader_tab(), ReaderTab::Changes, "a note arriving never moves the reader");
    let mut again = App::new(launch, shared, None, None);
    again.load_now();
    assert_eq!(again.reader_tab(), ReaderTab::Overview, "a session with a story note opens on it");
    let mut rt = opened_with_brief();
    typed(&mut rt, "b");
    assert_eq!(rt.state.reader_tab(), ReaderTab::Changes);
    rt.update(|s, _| s.reload());
    settle(&mut rt);
    assert_eq!(rt.state.reader_tab(), ReaderTab::Changes, "a reload keeps Changes");
    typed(&mut rt, "b");
    rt.update(|s, _| s.reload());
    settle(&mut rt);
    assert_eq!(rt.state.reader_tab(), ReaderTab::Overview, "and keeps the Overview");
}

#[test]
fn b_toggles_and_the_palette_goes_to_either_tab() {
    let mut rt = opened_with_brief();
    typed(&mut rt, "b");
    assert_eq!(rt.state.reader_tab(), ReaderTab::Changes);
    assert!(rt.find("diff-col").is_some());
    typed(&mut rt, "b");
    assert_eq!(rt.state.reader_tab(), ReaderTab::Overview);
    rt.update(|s, tx| {
        s.run(Command::GoChanges, tx);
    });
    assert_eq!(rt.state.reader_tab(), ReaderTab::Changes);
    rt.update(|s, tx| {
        s.run(Command::GoOverview, tx);
    });
    assert_eq!(rt.state.reader_tab(), ReaderTab::Overview);
    let labels: Vec<String> = crate::palette::filtered(&rt.state, "go to").into_iter().map(|e| e.label).collect();
    assert!(
        labels.iter().any(|l| l == "Go to Overview") && labels.iter().any(|l| l == "Go to Changes"),
        "{labels:?}"
    );
    assert_eq!(Command::Brief.label(), "Overview / Changes");
}

#[test]
fn the_tab_row_switches_with_the_mouse() {
    let mut rt = opened_with_brief();
    let (x, y) = rt.find("tab-changes").expect("the Changes tab");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert_eq!(rt.state.reader_tab(), ReaderTab::Changes);
    let (x, y) = rt.find("tab-overview").expect("the Overview tab");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert_eq!(rt.state.reader_tab(), ReaderTab::Overview);
}

#[test]
fn jumps_into_the_diff_switch_to_the_changes() {
    let mut rt = opened_with_brief();
    let back = |rt: &mut Runtime<App>| {
        rt.update(|s, _| s.set_reader_tab(ReaderTab::Overview));
        settle(rt);
    };
    rt.update(|s, tx| s.goto_cite("VouchersController.cs", Some((23, 23)), Side::New, tx));
    settle(&mut rt);
    assert_eq!(rt.state.reader_tab(), ReaderTab::Changes, "goto_cite");
    assert_eq!(rt.state.current_key().as_deref(), Some("server/src/Api/VouchersController.cs"));
    back(&mut rt);
    rt.update(|s, tx| s.goto_file(1, tx));
    assert_eq!(rt.state.reader_tab(), ReaderTab::Changes, "goto_file");
    rt.update(|s, _| crate::seed_threads(s));
    settle(&mut rt);
    let id = rt.state.mirror.threads.iter().find(|t| t.target.is_none()).unwrap().id.clone();
    back(&mut rt);
    rt.update(move |s, tx| s.goto_thread(&id, tx));
    assert_eq!(rt.state.reader_tab(), ReaderTab::Changes, "goto_thread");
    back(&mut rt);
    rt.update(|s, tx| s.start_tour(tx));
    assert_eq!(rt.state.reader_tab(), ReaderTab::Changes, "the tour starts on the Changes");
    press(&mut rt, rune::Key::Escape, Mods::default());
    back(&mut rt);
    rt.update(|s, _| s.show_picker("VoucherService"));
    settle(&mut rt);
    press(&mut rt, rune::Key::Enter, Mods::default());
    assert_eq!(rt.state.reader_tab(), ReaderTab::Changes, "a picker result");
}

#[test]
fn every_section_keeps_its_id_for_comments_on_the_overview() {
    let mut rt = opened_with_brief();
    let doc = story_brief();
    for sec in doc.sections() {
        assert!(rt.find(&format!("brief-sec-{}", sec.id)).is_some(), "{}", sec.id);
    }
    let (x, y) = rt.find("sec-head-inv.1").unwrap();
    rt.pointer_move(x, y);
    settle(&mut rt);
    assert!(rt.find("sec-actions").is_some(), "hovering a section shows Comment and Ask");
    let (x, y) = rt.find("sec-comment").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    let d = rt.state.brief.composer.clone().expect("a composer on the invariant");
    assert_eq!(d.target.section.as_deref(), Some("inv.1"));
    assert_eq!(d.title, "Brief › Invariant 1");
}

#[test]
fn headings_in_a_body_become_h4_outside_fenced_code() {
    let src = "# Big\ntext\n## Mid\n```md\n# not a heading\n```\n   ### three\n#hashtag\nSetext\n===\n~~~\n## kept\n~~~\n";
    let out = demote_headings(src);
    assert_eq!(
        out,
        "#### Big\ntext\n#### Mid\n```md\n# not a heading\n```\n#### three\n#hashtag\n#### Setext\n~~~\n## kept\n~~~\n"
    );
    assert_eq!(
        demote_headings("- item\n---\n"),
        "- item\n---\n",
        "a rule under a list stays a rule"
    );
    assert_eq!(demote_headings("Plain **bold**"), "Plain **bold**");
}

#[test]
fn resolvable_code_spans_become_cite_links_outside_fenced_code() {
    let keys: Vec<String> = ["server/src/Services/VoucherService.cs", "server/src/Api/VouchersController.cs"]
        .map(String::from)
        .to_vec();
    let src = "Calls `VoucherService.cs:27-31` then `Save()`.\n```\nsee `VoucherService.cs:1`\n```\nAlso [`VouchersController.cs`](https://x) and ``VouchersController.cs``.";
    let out = link_cites(src, &keys);
    assert_eq!(
        out,
        "Calls [`VoucherService.cs:27-31`](<rain-cite:server/src/Services/VoucherService.cs#27-31>) then `Save()`.\n```\nsee `VoucherService.cs:1`\n```\nAlso [`VouchersController.cs`](https://x) and [``VouchersController.cs``](<rain-cite:server/src/Api/VouchersController.cs>)."
    );
    assert_eq!(
        parse_cite("rain-cite:server/src/Services/VoucherService.cs#27-31"),
        Some(("server/src/Services/VoucherService.cs".into(), Some((27, 31))))
    );
    assert_eq!(parse_cite("rain-cite:a.cs"), Some(("a.cs".into(), None)));
    assert_eq!(parse_cite("https://x"), None);
    assert_eq!(link_cites("an `unclosed span", &keys), "an `unclosed span");
}

#[test]
fn bodies_render_as_markdown_without_raw_markers() {
    let shared = Arc::new(Shared::default());
    let mut doc = story_brief();
    doc.how[1].text = "The **voucher row** and the *payment* share one unit of work.\n\n## Why\n\n- one\n- two".into();
    shared.lock().brief = Some(doc);
    let mut a = App::new(
        Launch {
            session: "t".into(),
            fixture: Some(story()),
            ..Launch::default()
        },
        shared,
        None,
        None,
    );
    a.load_now();
    let rt = runtime(a);
    let texts: Vec<String> = rt
        .elements()
        .into_iter()
        .filter(|e| !e.exiting && e.path.contains("md-how.2"))
        .filter_map(|e| e.text)
        .collect();
    let all = texts.join("\n");
    assert!(all.contains("voucher row") && all.contains("payment"), "{all}");
    assert!(!all.contains("**") && !all.contains("*payment*"), "no raw markers: {all}");
    assert!(texts.iter().any(|t| t == "Why"), "the demoted heading draws as text: {texts:?}");
    assert!(rt.find("md-how.2").is_some());
}

#[test]
fn a_cite_the_text_already_links_gets_no_second_chip_and_the_tour_is_the_primary_action() {
    let keys: Vec<String> = ["server/src/Services/VoucherService.cs".to_owned()].to_vec();
    assert_eq!(
        inline_cites("Saves in `VoucherService.cs:27-31`.\n```\n`VoucherService.cs:1`\n```", &keys),
        vec![("server/src/Services/VoucherService.cs".to_owned(), Some((27, 31)))]
    );
    let shared = Arc::new(Shared::default());
    let mut doc = story_brief();
    doc.how[1].text = "Saved together in `VoucherService.cs:27-31`.".into();
    shared.lock().brief = Some(doc);
    let mut a = App::new(
        Launch {
            session: "t".into(),
            fixture: Some(story()),
            ..Launch::default()
        },
        shared,
        None,
        None,
    );
    a.load_now();
    let rt = sized_runtime(a, (1920.0, 1080.0));
    assert!(rt.find("cite-how.2-0").is_none(), "VoucherService.cs:27-31 is a link in the text");
    assert!(rt.find("cite-how.2-1").is_some(), "Voucher.cs is not, so its chip stays");
    assert!(rt.find("cite-how.1-0").is_some(), "a cite of other lines keeps its chip");
    let tour = rt.find("brief-tour").expect("Start tour");
    let title = rt.find("overview-title").unwrap();
    assert!(tour.1 > title.1 - 40.0, "under the title");
    let said = rt
        .elements()
        .into_iter()
        .filter(|e| e.path.contains("brief-tour"))
        .filter_map(|e| e.label.or(e.text))
        .collect::<Vec<_>>()
        .join(" ");
    assert!(said.contains("steps"), "says how long: {said}");
}

#[test]
fn the_overview_without_a_brief_says_how_to_get_one() {
    let mut rt = runtime(app(story(), false));
    rt.update(|s, _| s.set_reader_tab(ReaderTab::Overview));
    settle(&mut rt);
    assert!(rt.find("overview-empty").is_some());
    assert!(rt.find("brief-facts").is_some(), "the facts stand in");
    assert!(rt.find("brief-tour").is_none());
}

#[test]
fn facts_and_map_sit_beside_the_text_on_a_wide_window_and_above_it_on_a_narrow_one() {
    let wide = sized_runtime(
        {
            let mut a = app(story(), true);
            a.mutate(|s| s.brief = Some(story_brief()));
            a.set_reader_tab(ReaderTab::Overview);
            a
        },
        (1920.0, 1080.0),
    );
    assert!(wide.find("overview-side").is_some());
    assert!(wide.find("overview-map").is_some());
    let narrow = sized_runtime(
        {
            let mut a = app(story(), true);
            a.set_reader_tab(ReaderTab::Overview);
            a
        },
        (1366.0, 768.0),
    );
    assert!(narrow.find("overview-side").is_none());
    assert!(narrow.find("overview-cards").is_some());
}

fn live(rt: &Runtime<App>, key: &str) -> bool {
    rt.elements().iter().any(|e| !e.exiting && e.key.as_deref() == Some(key))
}

fn rect(rt: &Runtime<App>, key: &str) -> [f32; 4] {
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && e.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no element {key}"))
        .rect
}

fn tap(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_move(x, y);
    rt.frame(1.0 / 60.0);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// The story brief's Overview at 1920x1080.
fn brief_page() -> Runtime<App> {
    let mut rt = sized_runtime(brief_app(), (1920.0, 1080.0));
    settle(&mut rt);
    rt
}

/// The Overview with the first words of the Intent dragged over and released.
fn intent_selected() -> Runtime<App> {
    let mut rt = brief_page();
    let r = rect(&rt, "md-intent");
    let (x0, y) = (r[0] + 1.0, r[1] + 8.0);
    rt.pointer_move(x0, y);
    rt.frame(1.0 / 60.0);
    rt.pointer_down(x0, y);
    for i in 1..=8 {
        rt.pointer_move(x0 + 20.0 * i as f32, y);
        rt.frame(1.0 / 60.0);
    }
    assert!(!live(&rt, "sel-toolbar"), "no toolbar while dragging");
    rt.pointer_up(x0 + 160.0, y);
    settle(&mut rt);
    rt
}

#[test]
fn selected_overview_text_shows_the_toolbar_and_comment_quotes_it() {
    let mut rt = intent_selected();
    let (id, quoted) = rt.state.brief.selection.clone().expect("the selection is kept");
    assert_eq!(id, "intent");
    assert!(quoted.starts_with("Adds"), "{quoted}");
    for key in ["sel-toolbar", "sel-comment", "sel-ask", "sel-copy"] {
        assert!(live(&rt, key), "{key}");
    }
    let (bar, body, page) = (rect(&rt, "sel-toolbar"), rect(&rt, "md-intent"), rect(&rt, "overview"));
    assert!(bar[1] + bar[3] <= body[1] + 1.0, "above the selected line: {bar:?} vs {body:?}");
    assert!(bar[0] >= page[0] && bar[0] + bar[2] <= page[0] + page[2], "inside the page");
    tap(&mut rt, "sel-comment");
    let d = rt.state.brief.composer.clone().expect("the section composer");
    assert_eq!(d.target.section.as_deref(), Some("intent"));
    assert_eq!(d.target.quote.as_deref(), Some(quoted.as_str()), "the selected text is quoted");
    assert_eq!(d.mode, ComposeMode::Comment);
    assert!(live(&rt, "artifact-composer"));
    assert!(!live(&rt, "sel-toolbar"), "the composer replaces the toolbar");
    typed(&mut rt, "Which payments?");
    press(&mut rt, rune::Key::Enter, ctrl());
    let t = rt.state.shared.lock().threads.last().cloned().expect("a thread");
    assert_eq!(t.target.and_then(|t| t.section).as_deref(), Some("intent"));
    assert!(!live(&rt, "sel-toolbar"), "a sent comment doesn't bring the toolbar back");
}

#[test]
fn ask_and_copy_on_overview_text_and_esc_hides_the_toolbar() {
    let mut rt = intent_selected();
    tap(&mut rt, "sel-ask");
    assert_eq!(rt.state.brief.composer.as_ref().map(|d| d.mode), Some(ComposeMode::Question));

    let mut rt = intent_selected();
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    let want = rt.state.brief.selection.clone().unwrap().1;
    tap(&mut rt, "sel-copy");
    assert_eq!(clipboard.text().as_deref().map(String::as_str), Some(want.as_str()));

    let mut rt = intent_selected();
    press(&mut rt, rune::Key::Escape, Mods::default());
    assert!(!live(&rt, "sel-toolbar"), "Esc hides it");
    assert_eq!(rt.state.reader_tab(), ReaderTab::Overview);

    let mut rt = intent_selected();
    typed(&mut rt, "c");
    let d = rt.state.brief.composer.clone().expect("c opens the section composer");
    assert!(d.target.quote.is_some_and(|q| q.starts_with("Adds")), "c quotes the selection too");
}

#[test]
fn right_click_on_an_overview_section_comments_asks_or_copies() {
    let mut rt = brief_page();
    let r = rect(&rt, "md-how.1");
    let (x, y) = (r[0] + 40.0, r[1] + 8.0);
    rt.pointer_move(x, y);
    rt.frame(1.0 / 60.0);
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    settle(&mut rt);
    assert!(live(&rt, "section-menu-menu"), "the menu opens");
    let item = |rt: &Runtime<App>, label: &str| {
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.role == "menuitem" && e.label.as_deref() == Some(label))
            .and_then(|e| e.key)
            .unwrap_or_else(|| panic!("no menu item {label}"))
    };
    let key = item(&rt, "Ask about section");
    tap(&mut rt, &key);
    assert!(!live(&rt, "section-menu-menu"), "an item closes it");
    let d = rt.state.brief.composer.clone().expect("a composer");
    assert_eq!(d.target.section.as_deref(), Some("how.1"));
    assert_eq!(d.mode, ComposeMode::Question);

    let mut rt = intent_selected();
    let r = rect(&rt, "md-intent");
    rt.secondary_down(r[0] + 40.0, r[1] + 8.0);
    rt.secondary_up(r[0] + 40.0, r[1] + 8.0);
    settle(&mut rt);
    let key = item(&rt, "Comment on selection");
    tap(&mut rt, &key);
    let d = rt.state.brief.composer.clone().expect("a composer");
    assert!(d.target.quote.is_some_and(|q| q.starts_with("Adds")));
    press(&mut rt, rune::Key::Escape, Mods::default());
    assert!(rt.state.brief.composer.is_none());
}
