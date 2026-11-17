use std::sync::Mutex;

use rain_core::Side;
use rain_diffview::LineRef;

use super::*;

type Calls = Arc<Mutex<Vec<Vec<String>>>>;

/// A launcher that records each argv instead of starting an editor.
fn recorder(a: &mut App, fail: bool) -> Calls {
    let calls: Calls = Arc::default();
    let c = calls.clone();
    a.launcher = Box::new(move |argv: &[String]| {
        c.lock().unwrap().push(argv.to_vec());
        if fail {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "program not found"))
        } else {
            Ok(())
        }
    });
    calls
}

fn scratch() -> (PathBuf, App) {
    let dir = scratch_repo();
    let a = repo_app(&dir, Arc::new(Shared::default()), false);
    (dir, a)
}

fn file_index(a: &App, path: &str) -> usize {
    a.diff.files().iter().position(|f| f.path == path).unwrap()
}

fn put_cursor(rt: &mut Runtime<App>, g: usize, side: Side, line: u32) {
    rt.update(|s, _| {
        let l = s.local(g).unwrap();
        s.active_mut().set_cursor(Some((l, LineRef::new(side, line))));
    });
    settle(rt);
}

#[test]
fn shift_y_copies_the_cursor_location_and_maps_old_lines_to_new() {
    let (_dir, a) = scratch();
    let g = file_index(&a, "a.cs");
    let mut rt = runtime(a);
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    let shift_y = |rt: &mut Runtime<App>| {
        press(
            rt,
            Key::Text("Y".into()),
            Mods {
                shift: true,
                ..Mods::default()
            },
        )
    };
    put_cursor(&mut rt, g, Side::New, 40);
    shift_y(&mut rt);
    assert_eq!(clipboard.text().as_deref().map(String::as_str), Some("a.cs:40"));
    assert_eq!(rt.state.status, "Copied a.cs:40");

    put_cursor(&mut rt, g, Side::Old, 40);
    shift_y(&mut rt);
    assert_eq!(rt.state.status, "Copied a.cs:40", "the removed line maps to where it went");

    rt.update(|s, tx| {
        s.goto_file(g, tx);
        s.active_mut().set_cursor(None);
    });
    settle(&mut rt);
    shift_y(&mut rt);
    assert_eq!(rt.state.status, "Copied a.cs", "no cursor: the file");
}

#[test]
fn copy_location_names_the_repo_in_a_multi_repo_window() {
    let mut rt = runtime(app(story(), true));
    let clipboard = rune::clipboard::memory();
    rt.use_clipboard(&clipboard);
    assert!(rt.state.repos.len() > 1);
    let g = rt
        .state
        .diff
        .files()
        .iter()
        .position(|f| f.lines.iter().any(|l| l.new > 0))
        .unwrap();
    let line = rt.state.diff.files()[g].lines.iter().find(|l| l.new > 0).unwrap().new;
    put_cursor(&mut rt, g, Side::New, line);
    rt.update(|s, tx| {
        s.run(rain_core::command::Command::CopyLocation, tx);
    });
    settle(&mut rt);
    let want = format!("{}:{line}", rt.state.keys[g]);
    assert!(want.contains('/'), "{want}");
    assert_eq!(clipboard.text().as_deref().map(String::as_str), Some(want.as_str()));
    assert_eq!(rt.state.status, format!("Copied {want}"));
}

#[test]
fn ctrl_o_opens_the_file_at_the_cursor_line_with_the_editor_setting() {
    let (dir, mut a) = scratch();
    let calls = recorder(&mut a, false);
    let g = file_index(&a, "a.cs");
    let mut rt = runtime(a);
    put_cursor(&mut rt, g, Side::New, 40);
    press(&mut rt, Key::Text("o".into()), ctrl());
    let path = dir.join("a.cs").to_string_lossy().into_owned();
    assert_eq!(calls.lock().unwrap().last().unwrap(), &["code", "--goto", &format!("{path}:40")]);
    assert_eq!(rt.state.status, "Opened in editor");

    rt.state.settings.editor = Some(r#"rider --line {line} "{path}""#.into());
    rt.update(|s, _| s.open_file_in_editor(g));
    assert_eq!(calls.lock().unwrap().last().unwrap(), &["rider", "--line", "40", &path]);

    let other = file_index(&rt.state, "ws.cs");
    rt.update(|s, _| s.open_file_in_editor(other));
    assert_eq!(calls.lock().unwrap().last().unwrap()[2], "1", "no cursor there: the top");
    assert!(rt.find(&format!("file-more-{g}")).is_some(), "the header has a ⋯");
    rt.update(|s, _| s.menus.file = Some(g));
    settle(&mut rt);
    assert!(rt.find(&format!("open-{g}")).is_some(), "its menu opens the file in the editor");
}

#[test]
fn a_failed_start_and_patch_sessions_say_why() {
    let (_dir, mut a) = scratch();
    recorder(&mut a, true);
    let g = file_index(&a, "a.cs");
    a.open_file_in_editor(g);
    assert_eq!(a.status, "Could not start code: program not found");

    let mut a = app(story(), true);
    let calls = recorder(&mut a, false);
    assert_eq!(a.disk_path(0), Err("No working folder: this diff comes from a patch file"));
    a.open_in_editor();
    assert_eq!(
        a.status,
        "Can't open in editor: No working folder: this diff comes from a patch file"
    );
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn split_reads_unified_below_the_minimum_columns_a_side_and_comes_back_when_wide() {
    let a = app(story(), true);
    assert_eq!(a.settings.layout, ViewLayout::Split);
    let mut rt = sized_runtime(a, (1366.0, 768.0));
    assert_eq!(
        rt.state.effective_mode(),
        rain_diffview::DiffMode::Unified,
        "{}",
        rt.state.diff_width
    );
    let why = rt.state.layout_reason().expect("a reason for the toolbar");
    assert!(
        why.contains(&format!("split needs {}", crate::app::layout::SPLIT_MIN_COLUMNS)),
        "{why}"
    );
    assert_eq!(rt.state.settings.layout, ViewLayout::Split, "the preference stays split");

    rt.set_viewport((1920.0, 1080.0), 1.0);
    settle(&mut rt);
    assert_eq!(rt.state.effective_mode(), rain_diffview::DiffMode::Split, "{}", rt.state.diff_width);
    assert!(rt.state.layout_reason().is_none());

    rt.update(|s, _| s.set_layout(ViewLayout::Auto));
    rt.set_viewport((1366.0, 768.0), 1.0);
    settle(&mut rt);
    assert_eq!(rt.state.effective_mode(), rain_diffview::DiffMode::Unified);
    assert!(rt.state.layout_reason().is_none(), "auto falls back without a warning");
}

#[test]
fn hiding_the_list_widens_the_reader_and_is_remembered() {
    let mut rt = sized_runtime(app(story(), true), (1920.0, 1080.0));
    let before = rt.state.diff_width;
    press(&mut rt, Key::Text("b".into()), ctrl());
    assert!(!rt.state.sidebar.is_open());
    assert!(rt.find("rail").is_none(), "no rail: the reader takes the whole row");
    assert!(rt.state.diff_width > before + 200.0, "{before} -> {}", rt.state.diff_width);
    let path = rt.state.settings_path.clone().unwrap();
    assert!(!Settings::load(&path).list_open, "saved");
    press(&mut rt, Key::Text("b".into()), ctrl());
    assert!(rt.state.sidebar.is_open());
    assert!(rt.find("rail").is_none());
}
