use std::sync::Arc;
use std::time::Instant;

use rain_core::{Anchor, Disposition, Launch, RepoLaunch, Shared, Side};
use rain_diffview::{DiffMode, LineRange, LineRef, SpanSelection};
use rune::{Env, Key, KeyEvent, Mods, Runtime};

use super::*;
use crate::app::Subset;

#[test]
fn m_fixture_loads_into_the_tree_split_by_repo() {
    let rt = runtime(app(workspace("fixtures/m.patch"), true));
    let s = &rt.state;
    assert_eq!(s.phase, Phase::Ready);
    assert_eq!(s.tree.files.len(), 40);
    assert_eq!(s.diff.files().len(), 40);
    let ids: Vec<&str> = s.repos.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["app", "server"]);
    assert!(s.keys.iter().all(|k| k.starts_with("app/") || k.starts_with("server/")));
}

#[test]
fn a_loaded_diff_lands_at_once_without_animating_its_rows_in() {
    let mut a = App::new(
        Launch {
            session: "t".into(),
            fixture: Some(workspace("fixtures/m.patch")),
            ..Launch::default()
        },
        Arc::new(Shared::default()),
        None,
        None,
    );
    a.start_loading(Arc::new(|| {}));
    let mut rt = runtime(a);
    let t = Instant::now();
    while rt.state.phase != Phase::Ready && t.elapsed().as_secs() < 60 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(rt.state.phase, Phase::Ready);
    let els = rt.elements();
    let lines = els
        .iter()
        .filter(|e| e.path.contains("/diff/scroll/rows/") && e.key.as_deref() == Some("text"));
    assert!(lines.count() > 20, "the first lines are mounted");
    let moving = els.iter().filter(|e| !e.exiting && e.moving).count();
    assert!(moving < 20, "{moving} elements still moving on the frame the diff arrived");
}

#[test]
fn bracket_moves_to_the_next_file() {
    let mut rt = runtime(app(workspace("fixtures/m.patch"), false));
    assert_eq!(rt.state.current(), Some(0));
    typed(&mut rt, "]");
    assert_eq!(rt.state.current(), Some(1));
    typed(&mut rt, "]");
    typed(&mut rt, "[");
    assert_eq!(rt.state.current(), Some(1));
}

#[test]
fn space_marks_viewed_and_lands_on_the_next_unviewed_in_reading_order() {
    let mut rt = runtime(app(story(), true));
    let first = rt.state.keys[0].clone();
    assert_eq!(
        first, "server/src/Api/VouchersController.cs",
        "reading order puts the critical file first"
    );
    rt.state.set_viewed(1, true);
    assert_eq!(rt.state.current(), Some(0));
    press(&mut rt, Key::Text(" ".into()), Mods::default());
    assert!(rt.state.is_viewed(0));
    assert_eq!(rt.state.current(), Some(2), "skips the viewed file");
    assert!(rt.state.tree.file(&first).unwrap().viewed);
    assert!(
        rt.state
            .shared
            .lock()
            .viewed
            .iter()
            .any(|v| v.file_path == "src/Api/VouchersController.cs" && v.repo == "server")
    );
}

#[test]
fn line_selection_and_c_open_the_composer_and_ctrl_enter_saves_a_range_thread() {
    let mut rt = runtime(app(story(), true));
    let f = &rt.state.diff.files()[0];
    let news: Vec<u32> = f.lines.iter().filter(|l| l.new != 0).map(|l| l.new).collect();
    let (a, b) = (news[3], news[5]);
    rt.update(|s, _| {
        s.active_mut().set_selection(Some(LineRange {
            file: 0,
            start: LineRef::new(Side::New, a),
            end: LineRef::new(Side::New, b),
        }))
    });
    settle(&mut rt);
    typed(&mut rt, "c");
    let anchor = Anchor::Range {
        side: Side::New,
        start: a,
        end: b,
    };
    assert_eq!(rt.state.composer.as_ref().map(|c| c.anchor), Some(anchor));
    assert!(rt.find("composer").is_some(), "the composer is drawn under the lines");
    let before = rt.state.shared.lock().version;
    typed(&mut rt, "Why?");
    assert_eq!(rt.state.composer_edit.text, "Why?");
    press(&mut rt, Key::Enter, ctrl());
    let s = rt.state.shared.lock();
    assert!(s.version > before);
    let t = s.threads.last().expect("a thread");
    assert_eq!(t.anchor, anchor);
    assert_eq!(t.messages[0].body, "Why?");
    assert_eq!((t.repo.as_str(), t.file_path.as_str()), ("server", "src/Api/VouchersController.cs"));
    assert!(t.code_snapshot.is_some());
    drop(s);
    assert!(rt.state.composer.is_none());
}

#[test]
fn a_text_span_and_c_give_a_span_anchor() {
    let mut rt = runtime(app(story(), true));
    let n = rt.state.diff.files()[0].lines.iter().find(|l| l.new != 0).unwrap().new;
    rt.update(|s, _| {
        s.active_mut().set_span_selection(Some(SpanSelection {
            file: 0,
            side: Side::New,
            start_line: n,
            start_col: 2,
            end_line: n,
            end_col: 6,
            text: "ing ".into(),
        }))
    });
    typed(&mut rt, "c");
    let c = rt.state.composer.clone().expect("composer");
    assert!(matches!(
        c.anchor,
        Anchor::Span {
            start_col: 2,
            end_col: 6,
            ..
        }
    ));
    assert_eq!(c.preview, "ing ");
    typed(&mut rt, "x");
    press(&mut rt, Key::Enter, ctrl());
    let s = rt.state.shared.lock();
    assert!(matches!(s.threads.last().unwrap().anchor, Anchor::Span { .. }));
}

#[test]
fn c_on_the_cursor_line_saves_a_line_thread_and_r_builds_a_reply_chain() {
    let mut rt = runtime(app(story(), true));
    let n = rt.state.diff.files()[0].lines.iter().find(|l| l.new != 0).unwrap().new;
    rt.update(|s, _| s.active_mut().set_cursor(Some((0, LineRef::new(Side::New, n)))));
    settle(&mut rt);
    typed(&mut rt, "c");
    typed(&mut rt, "Line note");
    press(&mut rt, Key::Enter, ctrl());
    let id = {
        let s = rt.state.shared.lock();
        let t = s.threads.last().expect("a thread");
        assert_eq!(t.anchor, Anchor::Line { side: Side::New, line: n });
        t.id.clone()
    };
    for body in ["first reply", "second reply"] {
        typed(&mut rt, "r");
        assert_eq!(rt.state.reply_to.as_deref(), Some(id.as_str()));
        typed(&mut rt, body);
        press(&mut rt, Key::Enter, ctrl());
        assert!(rt.state.reply_to.is_none(), "the reply box closes once sent");
    }
    let s = rt.state.shared.lock();
    let bodies: Vec<&str> = rain_comments::ops::thread(&s, &id)
        .unwrap()
        .messages
        .iter()
        .map(|m| m.body.as_str())
        .collect();
    assert_eq!(bodies, ["Line note", "first reply", "second reply"]);
}

#[test]
fn escape_closes_the_composer_and_keeps_a_draft() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, tx| {
        s.active_mut().set_cursor(Some((0, LineRef::new(Side::New, 20))));
        s.comment(tx);
    });
    settle(&mut rt);
    typed(&mut rt, "draft");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.composer.is_none());
    assert_eq!(rt.state.shared.lock().drafts.len(), 1);
    typed(&mut rt, "j");
    assert!(
        rt.state.active().cursor().is_some(),
        "keys reach the diff once the composer is gone"
    );
}

#[test]
fn a_on_a_focused_card_agrees_and_again_clears() {
    let mut a = app(story(), true);
    seed_threads(&mut a);
    let id = a.mirror.threads[0].id.clone();
    a.focused_thread = Some(id.clone());
    let mut rt = runtime(a);
    typed(&mut rt, "a");
    assert_eq!(
        rain_comments::ops::thread(&rt.state.shared.lock(), &id).unwrap().disposition,
        Some(Disposition::Agree)
    );
    assert_eq!(rt.state.waiting_count(), 1, "the agreed thread no longer waits");
    typed(&mut rt, "a");
    assert_eq!(rain_comments::ops::thread(&rt.state.shared.lock(), &id).unwrap().disposition, None);
    typed(&mut rt, "?");
    assert_eq!(
        rain_comments::ops::thread(&rt.state.shared.lock(), &id).unwrap().disposition,
        Some(Disposition::Ask)
    );
}

#[test]
fn shift_j_moves_to_the_next_list_sections_first_file() {
    use crate::app::board::section_of;
    let mut rt = runtime(app(story(), true));
    let order = rt.state.list_order();
    let first = section_of(rt.state.group_of(order[0]));
    let want = *order.iter().find(|&&g| section_of(rt.state.group_of(g)) != first).unwrap();
    typed(&mut rt, "J");
    assert_eq!(rt.state.current(), Some(want));
    assert!(!rt.state.section_folded(section_of(rt.state.group_of(want))), "the section opens");
    assert!(rt.state.subset().is_none(), "a section doesn't narrow the reader");
    typed(&mut rt, "K");
    assert_eq!(rt.state.current(), Some(order[0]));
}
#[test]
fn find_hits_are_those_of_a_search_over_lowered_text() {
    let mut rt = runtime(app(story(), true));
    for q in ["VoUcHeR", "e", "(", "s.c", "ee", "voucher service"] {
        rt.update(|s, _| {
            s.find.edit.text = q.into();
            s.run_find();
        });
        let mut want = Vec::new();
        for (fi, f) in rt.state.active().files().iter().enumerate() {
            let text = f.text.to_ascii_lowercase();
            let mut last = u32::MAX;
            for (at, _) in text.match_indices(q.to_ascii_lowercase().as_str()) {
                let i = f.lines.partition_point(|r| (r.end as usize) < at) as u32;
                if (i as usize) < f.lines.len() && i != last && f.lines[i as usize].start as usize <= at {
                    last = i;
                    want.push((fi, i));
                }
            }
        }
        want.truncate(10_000);
        assert_eq!(rt.state.find.hits, want, "{q}");
    }
}

#[test]
fn find_hits_follow_the_diff_on_screen() {
    let mut rt = runtime(app(story(), true));
    typed(&mut rt, "/");
    typed(&mut rt, "voucher");
    assert!(!rt.state.find.hits.is_empty());
    press(&mut rt, Key::Escape, Mods::default());
    rt.update(|s, _| {
        s.apply_filter(&rain_tree::TreeFilter {
            repos: [0].into(),
            ..Default::default()
        })
    });
    settle(&mut rt);
    assert!(rt.state.filter.subset.is_some());
    typed(&mut rt, "/");
    press(&mut rt, Key::Enter, Mods::default());
    let files = rt.state.active().files().to_vec();
    for &(f, i) in &rt.state.find.hits {
        let line = files
            .get(f)
            .filter(|x| (i as usize) < x.lines.len())
            .map(|x| x.line_text(i).to_ascii_lowercase());
        assert!(line.is_some_and(|l| l.contains("voucher")), "stale hit ({f}, {i})");
    }
}

#[test]
fn a_pushed_overview_refreshes_the_chapters() {
    let mut rt = runtime(app(story(), true));
    {
        let mut s = rt.state.shared.lock();
        let doc = s.overview.as_mut().expect("the story has an overview");
        doc.graph.walkthrough.as_mut().expect("the story has a walkthrough").steps.reverse();
        s.overview_version += 1;
        s.version += 1;
    }
    rt.state.shared.notify();
    settle(&mut rt);
    let doc = rt.state.overview.as_ref().unwrap().document.clone();
    let want = rain_overview::chapters(&doc, &rt.state.keys, &rt.state.tags);
    let titles = |c: &rain_overview::Chapters| c.chapters.iter().map(|x| x.title.clone()).collect::<Vec<_>>();
    assert_eq!(titles(&rt.state.chapters), titles(&want));
}
#[test]
fn palette_runs_a_command_by_name() {
    let mut rt = sized_runtime(app(story(), true), (1920.0, 1080.0));
    assert_eq!(rt.state.settings.layout, ViewLayout::Split, "split is the default");
    assert_eq!(rt.state.diff.mode(), DiffMode::Split, "1920 pt leaves room to split");
    press(&mut rt, Key::Command('k'), ctrl());
    assert!(rt.state.palette.open);
    assert!(rt.find("palette").is_some());
    typed(&mut rt, "split");
    assert_eq!(rt.state.palette.edit.text, "split");
    press(&mut rt, Key::Enter, Mods::default());
    assert!(!rt.state.palette.open);
    assert_eq!(rt.state.settings.layout, ViewLayout::Unified);
    assert_eq!(rt.state.diff.mode(), DiffMode::Unified);
}

#[test]
fn narrow_windows_read_split_as_unified() {
    let mut a = app(story(), true);
    a.settings.layout = ViewLayout::Split;
    let mut rt = runtime(a);
    rt.state.set_diff_width(1000.0);
    assert_eq!(rt.state.diff.mode(), DiffMode::Unified);
    rt.state.set_diff_width(1700.0);
    assert_eq!(rt.state.diff.mode(), DiffMode::Split);
}

#[test]
fn m_opens_the_map_overlay_and_a_node_press_jumps_to_its_file() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| s.menus.more = true);
    settle(&mut rt);
    let map = rt
        .elements()
        .into_iter()
        .any(|e| !e.exiting && e.role == "menuitem" && e.label.as_deref() == Some("Map"));
    assert!(map, "the ⋯ menu offers the Map");
    rt.update(|s, _| {
        s.menus.close();
    });
    settle(&mut rt);
    typed(&mut rt, "m");
    assert!(rt.state.layout.is_open(Overlay::Map));
    assert!(rt.find("overlay-map").is_some());
    assert!(rt.find("main").is_some(), "the reader stays under the overlay");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.layout.is_open(Overlay::Map));
    typed(&mut rt, "m");
    rt.update(|s, tx| s.diagram_event(rain_diagram::DiagramEvent::Node("voucher-service".into()), tx));
    settle(&mut rt);
    assert!(!rt.state.layout.is_open(Overlay::Map));
    assert_eq!(rt.state.current_key().as_deref(), Some("server/src/Services/VoucherService.cs"));
}
#[test]
fn threads_imported_elsewhere_show_up_without_losing_the_place() {
    let mut rt = runtime(app(story(), true));
    typed(&mut rt, "]");
    let at = rt.state.current();
    {
        let mut s = rt.state.shared.lock();
        rain_comments::ops::add_thread(
            &mut s,
            "server",
            "src/Data/Voucher.cs",
            Anchor::Line { side: Side::New, line: 9 },
            "From HTTP",
            Some("claude"),
            None,
        );
    }
    settle(&mut rt);
    assert_eq!(rt.state.mirror.threads.len(), 1);
    assert_eq!(rt.state.diff.annotations().iter().filter(|a| !a.id.starts_with("note:")).count(), 1);
    assert_eq!(rt.state.current(), at);
    assert_eq!(rt.state.waiting_count(), 1);
}

#[test]
fn find_jumps_between_matches() {
    let mut rt = runtime(app(story(), true));
    typed(&mut rt, "/");
    assert!(rt.state.find.open);
    typed(&mut rt, "recordvoucher");
    assert!(rt.state.find.hits.len() >= 2, "{:?}", rt.state.find.hits);
    press(&mut rt, Key::Enter, Mods::default());
    assert!(rt.state.active().cursor().is_some());
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.find.open);
}

#[test]
fn session_survives_a_restart() {
    let dir = temp_dir();
    let launch = Launch {
        session: "restart".into(),
        fixture: Some(story()),
        overview: Some(overview()),
        ..Launch::default()
    };
    {
        let mut a = App::new(launch.clone(), Arc::new(Shared::default()), Some(dir.clone()), None);
        a.load_now();
        a.set_viewed(0, true);
        seed_threads(&mut a);
        a.autosave.take();
    }
    let mut b = App::new(launch, Arc::new(Shared::default()), Some(dir), None);
    b.load_now();
    assert!(b.is_viewed(0));
    assert_eq!(b.mirror.threads.len(), 2);
    assert_eq!(b.mirror.threads[0].messages.len(), 3);
}

#[test]
fn an_unreadable_session_file_is_kept_aside_not_overwritten() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let file = rain_comments::SessionFile::path_in(&dir, "corrupt");
    std::fs::write(&file, b"{\"threads\": [").unwrap();
    let launch = Launch {
        session: "corrupt".into(),
        fixture: Some(story()),
        ..Launch::default()
    };
    let mut a = App::new(launch, Arc::new(Shared::default()), Some(dir), None);
    a.load_now();
    a.autosave.take();
    assert_eq!(
        std::fs::read(file.with_extension("json.corrupt")).unwrap(),
        b"{\"threads\": [",
        "the unreadable file is kept"
    );
}

#[test]
fn a_broken_settings_file_is_reported_and_not_saved_over() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    std::fs::write(&path, b"{\"editor\": \"rider\",}").unwrap();
    let a = App::new(Launch::default(), Arc::new(Shared::default()), None, Some(path.clone()));
    assert!(a.status.starts_with("settings ignored"), "{}", a.status);
    a.save_settings();
    assert_eq!(std::fs::read(&path).unwrap(), b"{\"editor\": \"rider\",}");
}

#[test]
fn a_folder_that_is_not_a_repo_is_an_error_not_an_empty_diff() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let a = app_with(Launch {
        session: "err".into(),
        repos: vec![RepoLaunch {
            dir: dir.clone(),
            ..RepoLaunch::default()
        }],
        ..Launch::default()
    });
    match &a.phase {
        Phase::Error(msg) => assert!(msg.contains("not a git repository"), "{msg}"),
        p => panic!("expected an error, got {p:?}"),
    }
    let rt = runtime(a);
    assert!(rt.find("error").is_some());
}

#[test]
fn launch_flags_parse_from_leftover_arguments() {
    let args: Vec<String> = [
        "--fixture",
        "a.patch",
        "--overview",
        "o.json",
        "--repo",
        "x",
        "--repo",
        "y",
        "--base",
        "main",
        "--merge-base",
        "-w",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let l = launch_from_args(&args);
    assert_eq!(l.fixture, Some("a.patch".into()));
    assert_eq!(l.repos.len(), 2);
    assert!(l.repos.iter().all(|r| r.base == "main" && r.merge_base));
    assert!(l.ignore_whitespace);
}

/// Release-only: `cargo test -p rain-app --release -- --ignored --nocapture`. Needs
/// `cargo xtask fixtures --tier xl`.
#[test]
#[ignore]
fn xl_launch_to_first_frame() {
    let xl = std::env::var_os("RAIN_XL_PATCH").map_or_else(|| workspace("fixtures/gen/xl/diff.patch"), PathBuf::from);
    assert!(xl.exists(), "run cargo xtask fixtures --tier xl first ({})", xl.display());
    let launch = Launch {
        session: "xl".into(),
        fixture: Some(xl),
        clean: true,
        ..Launch::default()
    };
    let t0 = Instant::now();
    let dir = temp_dir();
    let mut a = App::new(launch.clone(), Arc::new(Shared::default()), Some(dir), None);
    a.settings.layout = ViewLayout::Unified;
    let data = load::load(&launch, &a.shared, &|_| {}).expect("xl loads");
    let lines: u64 = data.files.iter().map(|f| f.lines.len() as u64).sum();
    let load_ms = t0.elapsed().as_secs_f64() * 1e3;
    let t1 = Instant::now();
    a.install(data);
    let install_ms = t1.elapsed().as_secs_f64() * 1e3;
    let mut rt = Runtime::new(a, view, (1920.0, 1080.0), Env::new(true, 1.0));
    rt.on_key = Some(on_key);
    rt.poll = Some(|s: &mut App, _, tx| s.poll(tx));
    rt.frame(0.0);
    let data_to_frame = t1.elapsed().as_secs_f64() * 1e3;
    let launch_to_frame = t0.elapsed().as_secs_f64() * 1e3;
    let t2 = Instant::now();
    rt.state.settings.layout = ViewLayout::Split;
    rt.state.apply_split();
    rt.frame(1.0 / 60.0);
    let split_ms = t2.elapsed().as_secs_f64() * 1e3;
    let t3 = Instant::now();
    let last = rt.state.keys.len() - 1;
    rt.update(|s, tx| s.goto_file(last, tx));
    rt.frame(1.0 / 60.0);
    rt.frame(1.0 / 60.0);
    let jump_ms = t3.elapsed().as_secs_f64() * 1e3;
    let t4 = Instant::now();
    rt.key(KeyEvent {
        key: Key::Text(" ".into()),
        mods: Mods::default(),
    });
    rt.frame(1.0 / 60.0);
    let space_ms = t4.elapsed().as_secs_f64() * 1e3;
    let landed = rt.state.current().unwrap_or(0);
    println!(
        "Space landed on {} ({} lines, collapsed {}), rows {}",
        rt.state.keys[landed],
        rt.state.diff.files()[landed].lines.len(),
        rt.state.diff.collapsed(landed),
        rt.state.diff.row_count()
    );
    println!(
        "XL {} files, {lines} lines: load (parse+tags+hash) {load_ms:.0} ms, install {install_ms:.0} ms, data→first frame {data_to_frame:.0} ms, \
         launch→first frame {launch_to_frame:.0} ms, split switch {split_ms:.0} ms, jump to last file {jump_ms:.0} ms, Space {space_ms:.0} ms, collapsed {}",
        rt.state.keys.len(),
        (0..rt.state.keys.len()).filter(|&i| rt.state.diff.collapsed(i)).count()
    );
    assert!(data_to_frame < 2_000.0);
}

#[test]
fn whitespace_only_files_are_tagged_with_or_without_w() {
    let dir = scratch_repo();
    for w in [false, true] {
        let a = repo_app(&dir, Arc::new(Shared::default()), w);
        let key = a.keys.iter().find(|k| k.ends_with("ws.cs")).expect("ws.cs stays listed").clone();
        assert_eq!(a.tags[&key], rain_core::FileTag::FormattingOnly, "-w {w}");
        let code = a.keys.iter().find(|k| k.ends_with("a.cs")).unwrap();
        assert_eq!(a.tags[code], rain_core::FileTag::Review);
    }
}

#[test]
fn unchanged_ranges_open_once_the_whole_file_is_read() {
    let dir = scratch_repo();
    let mut a = repo_app(&dir, Arc::new(Shared::default()), false);
    let f = a.keys.iter().position(|k| k.ends_with("a.cs")).unwrap();
    assert!(!a.diff.expandable(f));
    let hidden = a.diff.hidden(f, 0).unwrap();
    assert!(hidden > 30);
    a.diff.request_expand(f, 0, rain_diffview::Expand::All);
    a.fetch_texts(f);
    assert!(a.diff.expandable(f));
    assert_eq!(a.diff.hidden(f, 0), Some(0));
}

#[test]
fn a_thread_follows_its_line_when_a_commit_moves_it() {
    let dir = scratch_repo();
    let shared = Arc::new(Shared::default());
    rain_comments::ops::add_thread(
        &mut shared.lock(),
        "",
        "a.cs",
        Anchor::Line { side: Side::New, line: 40 },
        "why forty?",
        Some("claude"),
        None,
    );
    let a = repo_app(&dir, shared.clone(), false);
    assert_eq!(shared.lock().threads[0].code_snapshot.as_ref().unwrap().content, "int forty;");
    drop(a);
    let lines: String = format!(
        "// one\n// two\n{}",
        numbered(80, |i| if i == 40 { "int forty;".into() } else { format!("int a{i};") })
    );
    std::fs::write(dir.join("a.cs"), lines).unwrap();
    git(&dir, &["commit", "-q", "-am", "three"]);
    let a = repo_app_from(&dir, "HEAD~2", shared.clone(), false);
    let t = shared.lock().threads[0].clone();
    assert_eq!(t.anchor, Anchor::Line { side: Side::New, line: 42 });
    assert_eq!(t.original, Some(Anchor::Line { side: Side::New, line: 40 }));
    assert!(!t.outdated);
    assert!(a.unplaced.is_empty(), "{:?}", a.unplaced);
}

#[test]
fn escape_with_nothing_open_reaches_the_diff() {
    let mut rt = runtime(app(story(), true));
    let n = rt.state.diff.files()[0].lines.iter().find(|l| l.new != 0).unwrap().new;
    rt.update(|s, _| s.active_mut().set_cursor(Some((0, LineRef::new(Side::New, n)))));
    settle(&mut rt);
    assert!(rt.state.active().cursor().is_some());
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.active().cursor().is_none(), "the diff took the Escape");
}

#[test]
fn a_subset_routes_active_global_and_local() {
    let mut rt = runtime(app(workspace("fixtures/m.patch"), false));
    rt.update(|s, _| {
        let sub = Subset::new(s, vec![5, 2], None);
        s.filter.subset = Some(sub);
    });
    settle(&mut rt);
    let s = &rt.state;
    assert_eq!(s.active().files().len(), 2);
    assert!(Arc::ptr_eq(&s.active().files()[0], &s.diff.files()[5]), "shares the file, no copy");
    assert_eq!((s.global(0), s.global(1), s.global(2)), (5, 2, usize::MAX));
    assert_eq!((s.local(2), s.local(5), s.local(0)), (Some(1), Some(0), None));
    assert_eq!(s.current(), Some(5));
    assert_eq!(s.active().mode(), s.diff.mode());
    typed(&mut rt, "]");
    assert_eq!(rt.state.current(), Some(2));
    typed(&mut rt, "]");
    assert_eq!(rt.state.current(), Some(2), "] stays inside the subset");
    rt.update(|s, _| s.filter.subset = None);
    settle(&mut rt);
    assert_eq!(rt.state.active().files().len(), 40);
}

#[test]
fn a_palette_chapter_jumps_to_its_first_file() {
    let mut rt = runtime(app(story(), true));
    let last = rt.state.chapters.chapters.len() - 1;
    let title = rt.state.chapters.chapters[last].title.clone();
    palette_pick(&mut rt, &format!("chapter {}: {title}", last + 1));
    assert_eq!(rt.state.chapter, last);
    assert_eq!(rt.state.current(), rt.state.chapter_files(last).first().copied());
}
#[test]
fn poll_hands_the_frame_to_apply_requests() {
    let before = crate::app::APPLY_CALLS.with(|c| c.get());
    let mut rt = runtime(app(story(), false));
    settle(&mut rt);
    let after = crate::app::APPLY_CALLS.with(|c| c.get());
    assert!(after > before, "apply_requests ran from poll ({before} -> {after})");
}

#[test]
fn round_two_keys_reach_their_commands() {
    let mut rt = runtime(app(story(), true));
    press(&mut rt, Key::Command('l'), ctrl());
    assert!(rt.state.sidebar.is_open());
    assert!(rt.state.tree.filter_edit.focused, "Ctrl+L starts filtering files");
    rt.update(|s, _| s.tree.filter_edit.focused = false);
    for k in ["e", "E"] {
        typed(&mut rt, k);
    }
    press(&mut rt, Key::Command('l'), Mods { shift: true, ..ctrl() });
    assert_eq!(rt.state.phase, Phase::Ready);
}

fn scene_runtime(a: App, size: (f32, f32)) -> Runtime<App> {
    let mut rt = Runtime::new(
        a,
        view,
        size,
        Env {
            system_dark: true,
            scale: 1.0,
        },
    );
    rt.on_key = Some(on_key);
    rt.poll = Some(|s: &mut App, _, tx| s.poll(tx));
    rt.frame(0.0);
    settle(&mut rt);
    rt
}

fn scene(name: &str) -> fn(&mut App) {
    crate::spec().scenes.iter().find(|(n, _)| *n == name).map(|(_, f)| *f).unwrap()
}

#[test]
fn every_scene_loads_and_draws_at_both_sizes_in_both_themes_without_an_inspector() {
    let names: Vec<&str> = crate::spec().scenes.iter().map(|(n, _)| *n).collect();
    for want in [
        "files",
        "split",
        "composer",
        "palette",
        "ruler",
        "filter",
        "wholefile",
        "notes",
        "error",
        "zoomed",
    ] {
        assert!(names.contains(&want), "scene {want} in {names:?}");
    }
    for gone in ["chapters", "queue", "comments", "inspector", "diagram"] {
        assert!(!names.contains(&gone), "scene {gone} is gone");
    }
    for (name, setup) in crate::spec().scenes {
        for size in [(1366.0, 768.0), (1920.0, 1080.0)] {
            for dark in [true, false] {
                let mut a = app(story(), true);
                setup(&mut a);
                assert!(a.settings_path.is_none() && a.autosave.is_none(), "{name} writes nothing");
                let mut rt = Runtime::new(a, view, size, Env::new(dark, 1.0));
                rt.on_key = Some(on_key);
                rt.poll = Some(|s: &mut App, _, tx| s.poll(tx));
                rt.frame(0.0);
                settle(&mut rt);
                assert!(rt.find("app").is_some(), "{name} draws at {size:?} dark {dark}");
                let insp = rt
                    .elements()
                    .iter()
                    .any(|e| e.key.as_deref().is_some_and(|k| k.starts_with("insp") || k == "inspector"));
                assert!(!insp, "{name} has no inspector element");
            }
        }
    }
}
#[test]
fn round_two_scenes_seed_what_they_show() {
    let mut a = app(story(), true);
    a.settings.theme = Theme::Light;
    a.settings.list_open = false;
    scene("ruler")(&mut a);
    assert_eq!(a.settings.theme, Theme::System, "--theme decides, not saved settings");
    assert!(a.sidebar.is_open());
    assert_eq!(a.mirror.notes.len(), 3);
    assert_eq!(a.mirror.threads.len(), 2);
    assert!(!a.find.hits.is_empty());
    let mut rt = scene_runtime(a, (1920.0, 1080.0));
    rt.update(|s, _| s.set_reader_tab(crate::app::ReaderTab::Overview));
    settle(&mut rt);
    assert!(rt.find("story-notes").is_some(), "the story note is on the Overview tab");

    let mut a = app(story(), true);
    scene("filter")(&mut a);
    let sub = a.filter.subset.as_ref().expect("a filter subset");
    assert!(!sub.files.is_empty() && sub.files.len() < a.keys.len());
    let rt = scene_runtime(a, (1920.0, 1080.0));
    assert!(rt.find("filter-bar").is_some());

    let repo = scratch_repo();
    let mut a = repo_app(&repo, Arc::new(Shared::default()), false);
    scene("wholefile")(&mut a);
    let g = a.keys.iter().position(|k| k.ends_with("a.cs")).unwrap();
    assert!(a.diff.whole(g));
    let rt = scene_runtime(a, (1920.0, 1080.0));
    assert_eq!(rt.state.active().files()[g].path, "a.cs");
    assert!(rt.state.diff.whole(g));
}

fn palette_pick(rt: &mut Runtime<App>, query: &str) {
    press(rt, Key::Command('k'), ctrl());
    typed(rt, query);
    press(rt, Key::Enter, Mods::default());
    assert!(!rt.state.palette.open);
}

#[test]
fn palette_filter_items_count_and_toggle_the_filter() {
    let mut rt = runtime(app(story(), true));
    let rows = crate::palette::filtered(&rt.state, "filter:");
    let server = rows.iter().find(|e| e.label == "Filter: server").expect("a repo item");
    let n = rt.state.keys.iter().filter(|k| k.starts_with("server/")).count();
    assert_eq!(server.hint, format!("{n} files"));
    assert!(rows.iter().any(|e| e.label == "Filter: Must review"), "{rows:?}");
    for t in rain_core::FileTag::ALL {
        let listed = rows.iter().any(|e| e.label == format!("Filter: {}", t.label()));
        assert_eq!(listed, rt.state.tree.tag_count(t) > 0, "{t:?}: only tags with files");
    }

    palette_pick(&mut rt, "filter: server");
    assert_eq!(rt.state.tree.filter.repos.len(), 1);
    assert_eq!(rt.state.active().files().len(), n);
    assert!(rt.find("filter-bar").is_some());
    let on = crate::palette::filtered(&rt.state, "filter: server");
    assert!(on[0].hint.starts_with("on · "), "{:?}", on[0]);

    palette_pick(&mut rt, "filter: must review");
    assert_eq!(rt.state.tree.filter.tags.len(), 1, "repo and tag combine");
    assert!(!rt.state.active().files().is_empty());
    for l in 0..rt.state.active().files().len() {
        let g = rt.state.global(l);
        assert!(rt.state.keys[g].starts_with("server/"));
        assert_eq!(rt.state.tags.get(&rt.state.keys[g]), Some(&rain_core::FileTag::MustReview));
    }

    palette_pick(&mut rt, "clear file filter");
    assert!(rt.state.filter.subset.is_none());
    assert!(rt.find("filter-bar").is_none());
}

#[test]
fn palette_note_items_jump_to_their_place() {
    let mut a = app(story(), true);
    crate::seed_notes(&mut a);
    let mut rt = runtime(a);
    let notes = crate::palette::filtered(&rt.state, "note:");
    assert!(notes.iter().filter(|e| e.kind == "Note").count() >= 3, "{notes:?}");
    let line = rt
        .state
        .mirror
        .notes
        .iter()
        .find(|n| n.kind == rain_core::NoteKind::Line)
        .unwrap()
        .clone();
    let at = line.anchor.unwrap().lines().0;

    palette_pick(&mut rt, "note: amounts are kept");
    let (l, cursor) = rt.state.active().cursor().expect("cursor on the note's line");
    assert_eq!(rt.state.keys[rt.state.global(l)], line.key());
    assert_eq!(cursor.line, at);

    assert_eq!(rt.state.reader_tab(), crate::app::ReaderTab::Changes);
    assert!(rt.find("story-notes").is_none(), "the story is on the Overview tab");
    palette_pick(&mut rt, "note: what this change");
    assert_eq!(
        rt.state.reader_tab(),
        crate::app::ReaderTab::Overview,
        "a story note opens the Overview"
    );
    assert_eq!(rt.state.status, "Story note: on the Overview tab");
    assert!(rt.find("story-notes").is_some());
}

#[test]
fn the_status_bar_says_when_split_is_too_narrow() {
    let shows = |rt: &Runtime<App>, t: &str| rt.elements().iter().any(|e| !e.exiting && e.text.as_deref() == Some(t));
    let wide = sized_runtime(app(story(), true), (1920.0, 1080.0));
    assert!(wide.find("status-layout").is_some());
    assert!(shows(&wide, "Split") && !shows(&wide, "Split · narrow"));
    let narrow = scene_runtime(app(story(), false), (1366.0, 768.0));
    assert!(crate::app::layout::columns_per_side(narrow.state.diff_width) < crate::app::layout::SPLIT_MIN_COLUMNS);
    assert_eq!(narrow.state.diff.mode(), DiffMode::Unified);
    assert!(shows(&narrow, "Split · narrow"));
    let why = narrow.state.layout_reason().unwrap();
    let layout = narrow
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("status-layout"))
        .unwrap();
    assert_eq!(layout.label.as_deref(), Some(why.as_str()), "the reason rides on the control");
}
#[test]
fn a_filter_that_matches_nothing_says_so() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| {
        let f = rain_tree::TreeFilter {
            text: "no-such-file-anywhere".into(),
            ..Default::default()
        };
        s.apply_filter(&f);
    });
    settle(&mut rt);
    assert!(rt.state.active().files().is_empty());
    assert!(rt.find("filter-bar").is_some());
}

#[test]
fn a_session_change_refreshes_a_stale_filter_subset() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| {
        let f = rain_tree::TreeFilter {
            commented_only: true,
            ..Default::default()
        };
        s.apply_filter(&f);
    });
    settle(&mut rt);
    assert!(rt.state.active().files().is_empty(), "no comments yet");
    rt.update(|s, _| crate::seed_threads(s));
    settle(&mut rt);
    assert_eq!(rt.state.active().files().len(), 1, "the commented file joins the subset");
}

#[test]
fn the_close_hook_marks_a_close_request() {
    use std::sync::atomic::Ordering;
    let mut rt = runtime(app(story(), false));
    assert!(!crate::CLOSE_REQUESTED.load(Ordering::SeqCst));
    rt.update(crate::on_close);
    assert!(crate::CLOSE_REQUESTED.load(Ordering::SeqCst));
}
