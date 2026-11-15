use std::sync::Arc;
use std::time::Instant;

use rain_core::{Launch, Shared};
use rain_tree::TreeFilter;
use rain_triage::Group;
use rune::{Key, Mods, Runtime};

use super::*;
use crate::app::board::{CHANGES, FORMATTING, Pick, Row, SECTION_ORDER, WAITING, section_group, section_of};

fn groups_app() -> App {
    app(workspace("crates/rain-triage/tests/fixtures/groups.patch"), false)
}

/// Hovers `key` (header actions show on hover), then presses it.
fn tap(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_move(x, y);
    settle(rt);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn unfold_all(s: &mut App) {
    for sec in SECTION_ORDER {
        s.unfold_section(sec);
    }
}

/// The list's file rows, top to bottom.
fn listed(s: &App) -> Vec<usize> {
    s.listing()
        .rows
        .iter()
        .filter_map(|r| match r {
            Row::File(g) => Some(*g),
            _ => None,
        })
        .collect()
}

fn reader(s: &App) -> Vec<usize> {
    (0..s.active().files().len()).map(|l| s.global(l)).collect()
}

#[test]
fn waiting_on_you_is_the_last_section_and_drawn_last() {
    assert_eq!(SECTION_ORDER.last(), Some(&WAITING));
    let mut a = groups_app();
    seed_threads(&mut a);
    unfold_all(&mut a);
    let rt = sized_runtime(a, (1920.0, 1400.0));
    let waiting = rt.find(&format!("list-section-{WAITING}")).unwrap();
    for sec in SECTION_ORDER.into_iter().filter(|s| *s != WAITING) {
        if let Some(h) = rt.find(&format!("list-section-{sec}")) {
            assert!(h.1 < waiting.1, "section {sec} is above Waiting on you");
        }
    }
    assert!(rt.find("list-waiting").is_some());
}

#[test]
fn group_sections_start_folded_and_a_fold_survives_a_restart() {
    let mut rt = runtime(groups_app());
    assert!(rt.state.section_folded(FORMATTING), "groups start folded");
    assert!(!rt.state.section_folded(CHANGES));
    let first_ws = (0..rt.state.keys.len())
        .find(|&g| rt.state.group_of(g) == Group::FormattingOnly)
        .unwrap();
    assert!(rt.find(&format!("file-{}", rt.state.keys[first_ws])).is_none());
    tap(&mut rt, &format!("list-section-{FORMATTING}"));
    assert!(!rt.state.section_folded(FORMATTING));
    assert!(rt.find(&format!("file-{}", rt.state.keys[first_ws])).is_some());
    tap(&mut rt, &format!("list-section-{CHANGES}"));
    assert!(rt.state.section_folded(CHANGES));
    let path = rt.state.settings_path.clone().unwrap();
    let again = App::new(rt.state.launch.clone(), Arc::new(Shared::default()), None, Some(path));
    assert!(!again.section_folded(FORMATTING));
    assert!(again.section_folded(CHANGES));
}

#[test]
fn accept_marks_the_group_logs_why_spares_the_real_line_and_undoes() {
    let mut rt = runtime(groups_app());
    let s = &rt.state;
    let ws: Vec<usize> = (0..s.keys.len()).filter(|&g| s.group_of(g) == Group::FormattingOnly).collect();
    let real_line = (0..s.keys.len())
        .find(|&g| s.item(g).is_some_and(|i| i.real_line.is_some()))
        .expect("a formatting change with one real line");
    assert_eq!(section_of(s.group_of(real_line)), CHANGES, "it is read, not grouped");
    assert!(!ws.is_empty());
    let n = ws.len();
    tap(&mut rt, "accept-formatting-only");
    let s = &rt.state;
    assert!(ws.iter().all(|&g| s.is_viewed(g)));
    assert!(!s.is_viewed(real_line), "the real line is never credited");
    assert!(listed(s).contains(&real_line) || s.section_folded(CHANGES));
    assert!(s.list_order().contains(&real_line));
    let log = s.shared.lock().accepted.clone();
    assert_eq!(log.len(), 1);
    assert_eq!(log[0].group, rain_core::Group::FormattingOnly);
    assert_eq!(log[0].files.len(), n);
    assert!(
        log[0]
            .reason
            .starts_with(&format!("{n} formatting-only accepted without reading (")),
        "{}",
        log[0].reason
    );
    assert!(rt.find("accept-snack").is_some(), "the snackbar offers Undo");
    assert!(ws.iter().all(|g| !s.list_order().contains(g)), "accepted files leave the walk");
    typed(&mut rt, "u");
    let s = &rt.state;
    assert!(ws.iter().all(|&g| !s.is_viewed(g)), "undo clears the marks");
    assert!(s.shared.lock().accepted[0].undone, "and keeps the log");
    assert!(ws.iter().all(|g| s.list_order().contains(g)));
}

#[test]
fn shift_v_accepts_the_current_files_group_and_refuses_real_changes() {
    let mut rt = runtime(groups_app());
    let first = rt.state.current().unwrap();
    assert!(rt.state.group_of(first).is_change());
    press(
        &mut rt,
        Key::Text("V".into()),
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    assert!(
        rt.state.status.starts_with("Real changes are read one by one"),
        "{}",
        rt.state.status
    );
    let lock = (0..rt.state.keys.len()).find(|&g| rt.state.group_of(g) == Group::Lockfile).unwrap();
    press(
        &mut rt,
        Key::Text("S".into()),
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    rt.update(|s, tx| s.goto_file(lock, tx));
    settle(&mut rt);
    assert_eq!(rt.state.current(), Some(lock));
    press(
        &mut rt,
        Key::Text("V".into()),
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    assert!(rt.state.is_viewed(lock));
    assert!(rt.state.status.contains("lockfile accepted without reading"), "{}", rt.state.status);
}

#[test]
fn the_filter_narrows_the_list_and_the_reader_to_the_same_files_in_order() {
    let mut a = app(story(), true);
    unfold_all(&mut a);
    let mut rt = runtime(a);
    rt.update(|s, _| {
        s.apply_filter(&TreeFilter {
            text: "voucher".into(),
            ..TreeFilter::default()
        })
    });
    settle(&mut rt);
    let s = &rt.state;
    let list = listed(s);
    assert!(!list.is_empty() && list.len() < s.keys.len(), "{list:?}");
    assert_eq!(list, reader(s));
    assert!(list.iter().all(|&g| s.keys[g].to_lowercase().contains("voucher")));
    for sec in SECTION_ORDER {
        if let Some(g) = section_group(sec) {
            assert_eq!(
                s.listing().shown[sec as usize],
                list.iter().filter(|&&f| s.group_of(f) == g).count()
            );
        }
    }
}

#[test]
fn the_walk_is_the_list_order() {
    let mut a = app(story(), true);
    unfold_all(&mut a);
    let rt = runtime(a);
    assert_eq!(rt.state.walk_order(), listed(&rt.state));
    assert_eq!(rt.state.walk_order().len(), rt.state.keys.len());
}

#[test]
fn file_at_a_time_shows_one_item_and_pages() {
    let mut rt = runtime(app(story(), true));
    let first = rt.state.current().unwrap();
    press(
        &mut rt,
        Key::Text("S".into()),
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    assert!(rt.state.single_mode());
    assert_eq!(rt.state.shared.lock().ui.reader, rain_core::ReaderMode::Single);
    assert_eq!(reader(&rt.state), vec![first], "one file, the one we were on");
    let order = rt.state.pick_order();
    typed(&mut rt, "]");
    assert_eq!(rt.state.board.pick, order[1]);
    match order[1] {
        Pick::File(g) => {
            assert_eq!(reader(&rt.state), vec![g]);
            let key = rt.state.keys[g].clone();
            assert_eq!(rt.state.current(), Some(g), "the list selects the file the reader shows");
            assert!(rt.find(&format!("file-{key}")).is_some());
        }
        p => panic!("{p:?}"),
    }
    typed(&mut rt, "[");
    assert_eq!(reader(&rt.state), vec![first]);
    let group = *order.iter().find(|p| matches!(p, Pick::Group(_))).expect("a group card");
    rt.update(|s, tx| s.open_pick(group, tx));
    settle(&mut rt);
    assert!(rt.find("group-card").is_some());
    assert!(rt.state.active().files().is_empty());
    press(
        &mut rt,
        Key::Text("S".into()),
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    assert!(!rt.state.single_mode());
    assert_eq!(rt.state.active().files().len(), rt.state.keys.len(), "stream again");
}

#[test]
fn space_in_file_at_a_time_is_done_and_next() {
    let mut rt = runtime(app(story(), true));
    press(
        &mut rt,
        Key::Text("S".into()),
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    let first = rt.state.current().unwrap();
    press(&mut rt, Key::Text(" ".into()), Mods::default());
    assert!(rt.state.is_viewed(first));
    assert_ne!(rt.state.current(), Some(first));
    assert_eq!(reader(&rt.state).len(), 1);
}

#[test]
fn the_picker_finds_vouchers_controller_by_vsc_and_jumps() {
    let mut rt = runtime(app(story(), true));
    press(&mut rt, Key::Command('p'), ctrl());
    assert!(rt.state.layout.is_open(Overlay::Picker));
    assert!(rt.find("picker").is_some());
    typed(&mut rt, "vsc");
    let rows = rt.state.picker_rows();
    assert_eq!(rows[0].label, "VouchersController.cs", "{rows:?}");
    assert!(rt.find("picker-preview").is_some());
    press(&mut rt, Key::Enter, Mods::default());
    assert!(!rt.state.layout.is_open(Overlay::Picker));
    assert!(!rt.state.typing());
    let g = rt.state.current().unwrap();
    assert!(rt.state.keys[g].ends_with("VouchersController.cs"));

    rt.update(|s, _| s.toggle_zoom());
    settle(&mut rt);
    press(&mut rt, Key::Command('p'), ctrl());
    typed(&mut rt, "accountservice");
    press(&mut rt, Key::Enter, ctrl());
    assert!(rt.state.layout.is_zoomed(), "Ctrl+Enter stays zoomed");
    assert!(rt.state.keys[rt.state.current().unwrap()].ends_with("AccountService.cs"));
    press(&mut rt, Key::Command('p'), ctrl());
    typed(&mut rt, "vsc");
    press(&mut rt, Key::Enter, Mods::default());
    assert!(!rt.state.layout.is_zoomed(), "Enter restores the layout");
}

#[test]
fn the_status_bar_follows_a_picker_jump_from_a_line_in_another_file() {
    let mut rt = runtime(app(story(), true));
    let n = rt.state.diff.files()[0].lines.iter().find(|l| l.new != 0).unwrap().new;
    rt.update(move |s, _| {
        s.active_mut()
            .set_cursor(Some((0, rain_diffview::LineRef::new(rain_core::Side::New, n))))
    });
    settle(&mut rt);
    press(&mut rt, Key::Command('p'), ctrl());
    typed(&mut rt, "vsc");
    press(&mut rt, Key::Enter, Mods::default());
    let g = rt.state.current().unwrap();
    assert!(rt.state.keys[g].ends_with("VouchersController.cs"), "{}", rt.state.keys[g]);
    let order = rt.state.walk_order();
    let want = format!("File {} of {}", order.iter().position(|&x| x == g).unwrap() + 1, order.len());
    let shown = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("status-hints"))
        .and_then(|e| e.label)
        .unwrap_or_default();
    assert!(shown.starts_with(&want), "{shown:?} should start with {want:?}");
}

#[test]
fn the_picker_lists_groups_and_waiting_threads() {
    let mut a = app(story(), true);
    seed_threads(&mut a);
    let rt = runtime(a);
    let kinds: Vec<&str> = rt.state.picker_rows().iter().map(|e| e.kind).collect();
    for k in ["File", "Group", "Map", "Waiting"] {
        assert!(kinds.contains(&k), "{k}: {kinds:?}");
    }
}

#[test]
fn alt_arrows_walk_the_jump_history() {
    let mut rt = runtime(app(story(), true));
    let a = rt.state.current().unwrap();
    let b = rt.state.keys.len() - 1;
    let c = 2;
    rt.update(|s, tx| s.goto_file(b, tx));
    settle(&mut rt);
    rt.update(|s, tx| s.goto_file(c, tx));
    settle(&mut rt);
    let alt = Mods {
        alt: true,
        ..Mods::default()
    };
    press(&mut rt, Key::Left, alt);
    assert_eq!(rt.state.current(), Some(b));
    press(&mut rt, Key::Left, alt);
    assert_eq!(rt.state.current(), Some(a));
    press(&mut rt, Key::Right, alt);
    assert_eq!(rt.state.current(), Some(b));
    press(&mut rt, Key::Right, alt);
    assert_eq!(rt.state.current(), Some(c));
    press(&mut rt, Key::Right, alt);
    assert_eq!(rt.state.status, "No later place");
}

#[test]
fn changes_switch_to_folders_and_back() {
    let mut rt = runtime(app(story(), true));
    tap(&mut rt, "changes-mode");
    assert!(rt.state.board.folders);
    assert!(rt.find("rain-tree").is_some(), "the folder tree");
    tap(&mut rt, "changes-mode");
    assert!(!rt.state.board.folders);
    assert!(rt.find("list-scroll").is_some());
}

/// List build and scroll timings on the generated l fixture:
/// `RAIN_L_PATCH=… cargo test -p rain-app --release board_timings -- --ignored --nocapture`.
#[test]
#[ignore = "needs fixtures/gen/l"]
fn board_timings() {
    let path = std::env::var_os("RAIN_L_PATCH").map_or_else(|| workspace("fixtures/gen/l/diff.patch"), std::path::PathBuf::from);
    if !path.exists() {
        println!("no {}", path.display());
        return;
    }
    let mut a = app_with(Launch {
        session: "t".into(),
        fixture: Some(path),
        ..Launch::default()
    });
    unfold_all(&mut a);
    let mut build: f64 = 0.0;
    for i in 0..10 {
        a.toggle_changes_mode();
        let t = Instant::now();
        let l = a.listing();
        build = build.max(t.elapsed().as_secs_f64() * 1e3);
        assert!(!l.rows.is_empty(), "{i}");
    }
    if a.board.folders {
        a.toggle_changes_mode();
    }
    let rows = a.listing().rows.len();
    let mut rt = sized_runtime(a, (1920.0, 1080.0));
    let mut worst: f64 = 0.0;
    let (x, y) = rt.find("list-scroll").unwrap();
    for _ in 0..60 {
        let t = Instant::now();
        rt.wheel(x, y, 120.0);
        rt.frame(1.0 / 60.0);
        worst = worst.max(t.elapsed().as_secs_f64() * 1e3);
    }
    println!(
        "l: {} files, {rows} rows, list build worst {build:.2} ms, scroll frame worst {worst:.2} ms",
        rt.state.keys.len()
    );
    assert!(build < 5.0, "{build:.2} ms");
    assert!(worst < 16.0, "{worst:.2} ms");
}

fn text_of(rt: &Runtime<App>, key: &str) -> String {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .and_then(|e| e.text.or(e.label))
        .unwrap_or_default()
}

#[test]
fn a_list_click_makes_the_file_current_even_with_the_cursor_on_another() {
    let mut rt = sized_runtime(app(story(), true), (1920.0, 1080.0));
    rt.update(|s, _| s.set_reader_tab(crate::app::ReaderTab::Changes));
    settle(&mut rt);
    let order = rt.state.list_order();
    let n = rt.state.active().files().len();
    let a = order[0];
    let b = order[1..]
        .iter()
        .copied()
        .find(|&g| rt.state.group_of(g).is_change() && rt.state.local(g).is_some_and(|l| l + 1 < n))
        .unwrap();
    let place = order.iter().position(|&x| x == b).unwrap() + 1;
    let line = |s: &App, g: usize| {
        let f = &s.diff.files()[g];
        f.lines.iter().find(|l| l.new != 0).map(|l| l.new).unwrap()
    };
    let at = rain_diffview::LineRef::new(rain_core::Side::New, line(&rt.state, a));
    rt.update(|s, tx| s.goto_line(a, at, tx));
    settle(&mut rt);
    assert_eq!(rt.state.current(), Some(a), "the cursor's file");
    let row_key = format!("file-{}", rt.state.keys[b]);
    tap(&mut rt, &row_key);
    assert_eq!(rt.state.current(), Some(b), "the clicked file is current");
    let want = format!("File {place} of {}", order.len());
    assert!(text_of(&rt, "status-hints").starts_with(&want), "{}", text_of(&rt, "status-hints"));
    let row = rt.find(&row_key).unwrap();
    let bar = rt.find("current-bar").expect("a current row");
    assert!(
        (bar.1 - row.1).abs() < 2.0,
        "the list highlights the clicked file: {bar:?} vs {row:?}"
    );
    let next = rt.state.global(rt.state.local(b).unwrap() + 1);
    typed(&mut rt, "]");
    assert_eq!(rt.state.current(), Some(next), "] goes on from the clicked file");
}

#[test]
fn folded_groups_and_their_accept_stay_in_view_and_the_changes_read_under_must_review_review_and_skim() {
    let a = groups_app();
    let l = a.listing();
    let at = |r: &Row| l.rows.iter().position(|x| x == r);
    let changes = at(&Row::Header(CHANGES)).unwrap();
    assert!(
        at(&Row::Header(FORMATTING)).is_none() && l.footer.contains(&FORMATTING),
        "pinned, not scrolled"
    );
    let subs: Vec<Group> = l.rows[changes..]
        .iter()
        .filter_map(|r| match r {
            Row::Sub(g) => Some(*g),
            _ => None,
        })
        .collect();
    assert!(
        !subs.is_empty() && subs.windows(2).all(|w| w[0] != w[1]),
        "one heading per group: {subs:?}"
    );
    assert_eq!(l.changes.iter().sum::<usize>(), l.shown[CHANGES as usize]);
    let listed_changes: Vec<usize> = l.rows[changes..]
        .iter()
        .filter_map(|r| match r {
            Row::File(g) => Some(*g),
            _ => None,
        })
        .collect();
    let walk: Vec<usize> = a.list_order().into_iter().filter(|g| a.group_of(*g).is_change()).collect();
    assert_eq!(listed_changes, walk, "the walk reads the changes in the list's order");
    let mut rt = sized_runtime(a, (1366.0, 768.0));
    let (_, y) = rt.find("accept-formatting-only").expect("Accept shows at rest, without a hover");
    assert!(y < 768.0 - 28.0, "inside the window: {y}");
    let (x, sy) = rt.find("list-scroll").unwrap();
    rt.wheel(x, sy, -3000.0);
    settle(&mut rt);
    assert_eq!(rt.find("accept-formatting-only").map(|p| p.1), Some(y), "it does not scroll away");
    assert!(
        text_of(&rt, "list-sub-must").contains("Must review") || rt.find("list-sub-review").is_some(),
        "the chips' words"
    );
}

#[test]
fn a_filter_hides_empty_groups() {
    let mut rt = runtime(groups_app());
    assert!(rt.find(&format!("list-section-{FORMATTING}")).is_some());
    let only = rt.state.keys[rt.state.list_order()[0]].clone();
    rt.update(|s, _| {
        s.apply_filter(&TreeFilter {
            text: only.clone(),
            ..TreeFilter::default()
        })
    });
    settle(&mut rt);
    assert!(rt.find(&format!("list-section-{FORMATTING}")).is_none(), "no file of it matches");
    assert!(rt.find(&format!("list-section-{CHANGES}")).is_some());
}

#[test]
fn waiting_starts_folded_reads_plain_text_and_follows_the_filter() {
    let mut a = app(story(), true);
    seed_threads(&mut a);
    let g = a.list_order()[0];
    let path = a.diff.files()[g].path.clone();
    let repo = a.repo_tag(g).to_owned();
    let line = a.diff.files()[g].lines.iter().find(|l| l.new != 0).unwrap().new;
    a.mutate(|ss| {
        rain_comments::ops::add_thread(
            ss,
            &repo,
            &path,
            rain_core::Anchor::Line {
                side: rain_core::Side::New,
                line,
            },
            "**`ask` events can be skipped** for good. More here.",
            Some("claude"),
            None,
        );
    });
    let mut rt = runtime(a);
    assert!(rt.state.section_folded(WAITING), "a one-line header until opened");
    assert!(rt.find("list-waiting").is_none());
    assert!(rt.find(&format!("list-section-{WAITING}")).is_some());
    rt.update(|s, _| s.unfold_section(WAITING));
    settle(&mut rt);
    let texts: Vec<String> = rt.elements().into_iter().filter_map(|e| e.text).collect();
    assert!(
        texts.iter().any(|t| t.contains("claude: ask events can be skipped for good.")),
        "{texts:?}"
    );
    assert!(
        !texts
            .iter()
            .any(|t| t.starts_with("claude:") && (t.contains("**") || t.contains('`')))
    );
    rt.update(|s, _| {
        s.apply_filter(&TreeFilter {
            text: "no-such-file-anywhere".into(),
            ..TreeFilter::default()
        })
    });
    settle(&mut rt);
    assert!(rt.find("list-waiting").is_none(), "no waiting thread is on a file the filter shows");
}

/// The groups fixture with its first diff file's group already accepted, so the list starts
/// elsewhere.
fn landing_app(place: Option<rain_core::ReadingPlace>) -> Runtime<App> {
    let session = rain_core::Session {
        accepted: vec![rain_core::AcceptedGroup {
            group: rain_core::Group::Must,
            files: vec!["src/Api/PaymentsController.cs".into()],
            reason: "accepted".into(),
            ..Default::default()
        }],
        place,
        ..Default::default()
    };
    let mut a = App::new(
        Launch {
            session: "landing".into(),
            fixture: Some(workspace("crates/rain-triage/tests/fixtures/groups.patch")),
            ..Launch::default()
        },
        Arc::new(Shared::new(session)),
        None,
        None,
    );
    a.load_now();
    sized_runtime(a, (1600.0, 400.0))
}

#[test]
fn a_fresh_session_opens_at_the_lists_first_file() {
    let rt = landing_app(None);
    assert_eq!(rt.state.keys[0], "src/Api/PaymentsController.cs");
    let order = rt.state.list_order();
    assert_ne!(order[0], 0, "the accepted first diff file is not in the list");
    assert_eq!(rt.state.current(), Some(order[0]));
}

#[test]
fn a_saved_place_wins_over_the_lists_first_file() {
    let fresh = landing_app(None);
    let saved = fresh.state.list_order()[2];
    let rt = landing_app(Some(rain_core::ReadingPlace {
        file: fresh.state.keys[saved].clone(),
        ..Default::default()
    }));
    assert_ne!(saved, fresh.state.list_order()[0]);
    assert_eq!(rt.state.current(), Some(saved));
}
