//! The round-3 shell: zoom, overlays and their `Esc` order, settings migration, dispatch.

use rain_core::command::Command;

use super::*;

fn shift() -> Mods {
    Mods {
        shift: true,
        ..Mods::default()
    }
}

#[test]
fn shift_escape_twice_restores_the_layout_exactly() {
    let mut rt = sized_runtime(app(story(), true), (1920.0, 1080.0));
    rt.update(|s, _| {
        s.sidebar.set_width(410.0);
        s.open_overlay(Overlay::Map);
        s.open_overlay(Overlay::Conversation);
    });
    settle(&mut rt);
    let before = rt.state.layout_snapshot();
    let reader = rt.state.diff_width;
    press(&mut rt, Key::Escape, shift());
    assert!(rt.state.layout.is_zoomed());
    assert!(!rt.state.sidebar.is_open() && rt.state.layout.overlays.is_empty());
    assert!(rt.find("rail").is_none(), "zoom hides the rail too");
    assert!(rt.state.diff_width > reader + 300.0, "{reader} -> {}", rt.state.diff_width);
    assert!(rt.state.status.starts_with("Zoomed"), "{}", rt.state.status);
    assert!(
        crate::app::layout::is_zoomed_status(&rt.state.status),
        "the status bar leaves it to the zoom chip"
    );
    assert!(!crate::app::layout::is_zoomed_status("Zoomed out of nothing"));
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.layout.is_zoomed(), "Esc is not a way out of zoom");
    press(&mut rt, Key::Escape, shift());
    assert!(!rt.state.layout.is_zoomed());
    assert_eq!(rt.state.layout_snapshot(), before);
    assert_eq!(rt.state.layout.overlays, [Overlay::Map, Overlay::Conversation]);
    assert!(rt.state.settings.list_open, "zoom never changes the saved preference");
}

#[test]
fn zoomed_keys_switch_tabs_and_open_files_and_map_as_overlays() {
    let mut a = app(story(), true);
    seed_notes(&mut a);
    let mut rt = sized_runtime(a, (1920.0, 1080.0));
    assert!(rt.find("tab-strip").is_some(), "the tabs sit above the reader");
    rt.update(|s, _| s.toggle_zoom());
    settle(&mut rt);
    assert!(rt.find("tab-strip").is_none(), "zoom hides the tabs");
    typed(&mut rt, "o");
    assert_eq!(rt.state.reader_tab(), crate::app::ReaderTab::Overview);
    assert!(rt.find("overview").is_some(), "o still switches to the Overview");
    assert!(rt.find("story-notes").is_some());
    press(&mut rt, Key::Text("b".into()), ctrl());
    assert!(rt.state.layout.is_open(Overlay::Files));
    assert!(rt.find("overlay-files").is_some());
    assert!(!rt.state.sidebar.is_open(), "the list itself stays hidden");
    typed(&mut rt, "m");
    assert_eq!(rt.state.layout.top(), Some(Overlay::Map));
}

#[test]
fn escape_closes_one_layer_at_a_time_in_order() {
    let mut a = app(story(), true);
    seed_threads(&mut a);
    let id = a.mirror.threads[0].id.clone();
    let mut rt = runtime(a);
    let n = rt.state.diff.files()[0].lines.iter().find(|l| l.new != 0).unwrap().new;
    rt.update(|s, tx| {
        s.apply_filter(&rain_tree::TreeFilter {
            repos: [0].into(),
            ..Default::default()
        });
        s.focused_thread = Some(id.clone());
        s.open_overlay(Overlay::Tour);
        s.open_find();
        s.find.edit.focused = false;
        s.active_mut()
            .set_cursor(Some((0, rain_diffview::LineRef::new(rain_core::Side::New, n))));
        s.comment(tx);
        s.composer_edit.focused = false;
        s.open_overlay(Overlay::Map);
        s.open_overlay(Overlay::Files);
        s.open_palette();
    });
    settle(&mut rt);
    let esc = |rt: &mut Runtime<App>| press(rt, Key::Escape, Mods::default());
    esc(&mut rt);
    assert!(!rt.state.palette.open);
    esc(&mut rt);
    assert_eq!(rt.state.layout.top(), Some(Overlay::Map), "the top overlay first");
    esc(&mut rt);
    assert_eq!(rt.state.layout.overlays, [Overlay::Tour]);
    assert!(rt.state.composer.is_some());
    esc(&mut rt);
    assert!(rt.state.composer.is_none(), "then the composer");
    assert!(rt.state.find.open);
    esc(&mut rt);
    assert!(!rt.state.find.open, "then find");
    assert!(rt.state.layout.is_open(Overlay::Tour));
    esc(&mut rt);
    assert!(!rt.state.layout.is_open(Overlay::Tour), "then the tour");
    assert!(rt.state.focused_thread.is_some());
    esc(&mut rt);
    assert!(rt.state.focused_thread.is_none(), "then the focused thread");
    assert!(rt.state.filter_active());
    esc(&mut rt);
    assert!(!rt.state.filter_active(), "then the filter");
}

#[test]
fn round_two_settings_load_unified_and_are_rewritten_without_old_keys() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    std::fs::write(
        &path,
        br#"{"splitView": false, "inspectorWidth": 300, "inspectorOpen": true, "inspectorSections": 5, "mode": "queue", "sidebarWidth": 300, "theme": "dark"}"#,
    )
    .unwrap();
    let a = App::new(Launch::default(), Arc::new(Shared::default()), None, Some(path.clone()));
    assert_eq!(a.settings.layout, ViewLayout::Unified);
    assert_eq!(a.settings.sidebar_width, 300.0);
    assert_eq!(a.sidebar.width(), 300.0);
    let text = std::fs::read_to_string(&path).unwrap();
    for gone in ["splitView", "inspector", "\"mode\""] {
        assert!(!text.contains(gone), "{gone} is not written back: {text}");
    }
    assert!(text.contains("\"layout\": \"unified\""), "{text}");
    assert!(text.contains("\"theme\": \"dark\""), "{text}");
    assert_eq!(Settings::load(&path).layout, ViewLayout::Unified, "and reads the same next time");
}

#[test]
fn list_sections_fold_and_survive_a_restart() {
    let mut a = app(story(), true);
    seed_threads(&mut a);
    let mut rt = runtime(a);
    assert!(
        rt.find("list-files").is_some() && rt.find("list-waiting").is_none(),
        "Waiting starts folded"
    );
    let files = rt.find("list-section-0").unwrap();
    let waiting = rt.find("list-section-1").unwrap();
    assert!(waiting.1 > files.1, "Waiting on you is the last section");
    rt.pointer_down(waiting.0, waiting.1);
    rt.pointer_up(waiting.0, waiting.1);
    settle(&mut rt);
    assert!(rt.find("list-waiting").is_some());
    assert!(
        rt.state.settings.section_collapsed(crate::app::board::WAITING),
        "differs from its default"
    );
    let path = rt.state.settings_path.clone().unwrap();
    let again = App::new(rt.state.launch.clone(), Arc::new(Shared::default()), None, Some(path));
    assert!(!again.section_folded(crate::app::board::WAITING), "opened stays open");
    assert!(!again.settings.section_collapsed(crate::app::board::FILES));
}

#[test]
fn the_window_size_is_saved_when_it_changes() {
    let mut rt = sized_runtime(app(story(), true), (1500.0, 900.0));
    let path = rt.state.settings_path.clone().unwrap();
    assert_eq!(Settings::load(&path).window, (1500.0, 900.0));
    rt.set_viewport((1700.0, 1000.0), 1.0);
    settle(&mut rt);
    assert_eq!(Settings::load(&path).window, (1700.0, 1000.0));
}

#[test]
fn every_command_reaches_a_handler() {
    for c in Command::ALL {
        let mut a = app(story(), true);
        seed_threads(&mut a);
        let mut rt = runtime(a);
        let mut handled = false;
        rt.update(|s, tx| handled = s.run(c, tx));
        settle(&mut rt);
        assert!(handled || c == Command::Escape, "{c:?}");
        assert_eq!(rt.state.phase, Phase::Ready, "{c:?}");
    }
}

#[test]
fn t_without_a_brief_says_so_and_the_accent_button_opens_finish() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| s.status.clear());
    press(&mut rt, Key::Text("T".into()), shift());
    assert_eq!(rt.state.status, "No tour: the brief has no steps or cites", "T");
    assert!(!rt.state.layout.is_open(Overlay::Tour));
    rt.update(|s, _| s.status.clear());
    let (x, y) = rt.find("finish-review").expect("the accent button");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(rt.state.layout.is_open(Overlay::Finish), "the accent button opens Finish review");
    assert!(rt.find("finish-submit").is_some());
}

#[test]
fn the_toolbar_carries_title_meter_more_and_finish() {
    let mut a = app(story(), true);
    seed_threads(&mut a);
    let mut rt = runtime(a);
    let title = crate::view::toolbar_title(&rt.state);
    assert!(title.contains("server") && title.contains('→'), "{title}");
    let r = &rt.state.repos[0];
    assert!(title.contains(&format!("{} → {}", r.base, r.target)), "base to target: {title}");
    for key in [
        "title",
        "meter",
        "more-menu",
        "finish-review",
        "status-waiting",
        "status-layout",
        "tab-strip",
        "filter-chips-button",
    ] {
        assert!(rt.find(key).is_some(), "{key}");
    }
    for gone in [
        "progress-must",
        "progress-review",
        "waiting",
        "layout",
        "open-map",
        "rail-wait",
        "rail-real",
    ] {
        assert!(rt.find(gone).is_none(), "{gone} left the top bar");
    }
    let toolbar = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("toolbar")).unwrap();
    let bottom = toolbar.rect[1] + toolbar.rect[3];
    let (_, status_y) = rt.find("status-waiting").unwrap();
    assert!(status_y > bottom + 500.0, "waiting sits in the status bar");
    let chip = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("status-waiting"))
        .unwrap()
        .rect;
    let on_chip = |r: [f32; 4]| r[0] >= chip[0] && r[0] < chip[0] + chip[2] && r[1] >= chip[1] && r[1] < chip[1] + chip[3];
    assert!(
        !rt.elements().iter().any(|e| e.text.as_deref() == Some("w") && on_chip(e.rect)),
        "the key lives in the tooltip, not on the chip"
    );
    assert_eq!(rt.state.waiting_count(), 2);
    let (x, y) = rt.find("status-waiting").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(rt.state.focused_thread.is_some(), "the waiting button goes to a waiting thread");
    let shows = rt
        .elements()
        .iter()
        .any(|e| e.text.as_deref().is_some_and(|t| t.starts_with("File ")));
    assert!(shows, "the status line says where the reader is");
}

#[test]
fn the_meter_counts_must_review_and_review_only_with_must_first() {
    use rain_core::FileTag;
    let mut rt = runtime(app(story(), true));
    let tag = |s: &App, t: FileTag| s.keys.iter().filter(|k| s.tags.get(*k) == Some(&t)).count();
    let (must, review) = (tag(&rt.state, FileTag::MustReview), tag(&rt.state, FileTag::Review));
    assert!(
        must > 0 && must + review < rt.state.keys.len(),
        "the fixture has Must and files not counted"
    );
    let text = |rt: &Runtime<App>, key: &str| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(key))
            .and_then(|e| e.text)
            .unwrap_or_default()
    };
    assert_eq!(text(&rt, "meter-text"), format!("0 / {} viewed", must + review));
    assert_eq!(text(&rt, "meter-must"), format!("Must 0/{must}"));
    let first_must = (0..rt.state.keys.len())
        .find(|&g| rt.state.tags.get(&rt.state.keys[g]) == Some(&FileTag::MustReview))
        .unwrap();
    rt.update(|s, _| s.set_viewed(first_must, true));
    settle(&mut rt);
    assert_eq!(text(&rt, "meter-must"), format!("Must 1/{must}"));
    assert_eq!(text(&rt, "meter-text"), format!("1 / {} viewed", must + review));
}

#[test]
fn the_status_line_says_overview_on_that_tab() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| s.set_reader_tab(crate::app::ReaderTab::Overview));
    settle(&mut rt);
    let hints = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("status-hints"))
        .and_then(|e| e.label)
        .unwrap_or_default();
    assert!(hints.starts_with("Overview"), "{hints}");
    assert!(rt.find("current-bar").is_none(), "no file is lit in the list on the Overview");
}

/// A patch adding one file under a very long path.
fn long_path_patch() -> std::path::PathBuf {
    let path = format!(
        "src/Acme.Shop.IntegrationTests/Acme.Shop.Api/Features/StorefrontTheme/{}/GetStorefrontThemeStylesControllerTests.cs",
        "Nested/Deeper/StillDeeper/GetStorefrontThemeStyles"
    );
    let p = format!("diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,2 @@\n+one\n+two\n");
    let d = temp_dir();
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("long.patch");
    std::fs::write(&f, p).unwrap();
    f
}

#[test]
fn a_long_path_is_cut_in_the_middle_and_leaves_the_header_actions_in_place() {
    let rt = sized_runtime(app(long_path_patch(), false), (1200.0, 800.0));
    let els = rt.elements();
    let find = |key: &str| {
        els.iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some(key))
            .unwrap_or_else(|| panic!("no {key}"))
    };
    let area = find("diff-area");
    let right = area.rect[0] + area.rect[2];
    for key in ["viewed-0", "file-more-0"] {
        let e = find(key);
        assert!(
            e.rect[2] > 20.0 && e.rect[0] + e.rect[2] <= right + 0.5,
            "{key} stays whole and in view: {:?} in {:?}",
            e.rect,
            area.rect
        );
    }
    let name = find("name");
    assert!(
        name.rect[0] + name.rect[2] < find("viewed-0").rect[0],
        "the file name stays before Viewed"
    );
}
