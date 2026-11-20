use rain_core::Command;
use rain_core::keymap::{Profile, with_active};
use rune::{Key, KeyEvent, Mods, Runtime};

use super::*;
use crate::settings::ViewLayout;

/// One modified file: line 2 and line 5 change, the rest is context.
fn modified_patch() -> PathBuf {
    let d = temp_dir();
    std::fs::create_dir_all(&d).unwrap();
    let p = "diff --git a/m.rs b/m.rs\n--- a/m.rs\n+++ b/m.rs\n@@ -1,6 +1,6 @@\n fn alpha(beta, gamma) {\n-    let old_name = 1;\n+    let new_name = 22;\n     keep_one();\n     keep_two();\n-    gone();\n+    added_here();\n }\n";
    let path = d.join("m.patch");
    std::fs::write(&path, p).unwrap();
    path
}

fn rt_in(profile: Profile, layout: ViewLayout) -> Runtime<App> {
    let mut rt = runtime(app(modified_patch(), false));
    rt.update(|s, _| {
        s.set_profile(profile);
        s.set_layout(layout);
    });
    settle(&mut rt);
    rt
}

fn shift() -> Mods {
    Mods {
        shift: true,
        ..Mods::default()
    }
}

fn ctrl_shift() -> Mods {
    Mods { shift: true, ..ctrl() }
}

fn at(rt: &Runtime<App>) -> (u32, usize) {
    let a = rt.state.active();
    (a.cursor().unwrap().1.line, a.cursor_byte().unwrap())
}

fn help(rt: &Runtime<App>, key: &str) -> Option<String> {
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && e.key.as_deref() == Some(key))
        .and_then(|e| e.help)
}

fn click(rt: &mut Runtime<App>, key: &str) {
    let r = rt
        .elements()
        .into_iter()
        .find(|e| !e.exiting && e.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("{key} is on screen"))
        .rect;
    let (x, y) = (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

#[test]
fn universal_keys_move_select_and_copy_in_every_profile_and_layout() {
    for profile in Profile::ALL {
        for layout in [ViewLayout::Unified, ViewLayout::Split] {
            let why = format!("{profile:?} {layout:?}");
            let mut rt = rt_in(profile, layout);
            let clipboard = rune::clipboard::memory();
            rt.use_clipboard(&clipboard);
            press(&mut rt, Key::Down, Mods::default());
            assert_eq!(at(&rt), (1, 0), "{why}: the first key puts the cursor on the first line");
            press(&mut rt, Key::Down, Mods::default());
            if layout == ViewLayout::Unified {
                let old = rain_diffview::LineRef::new(rain_core::Side::Old, 2);
                assert_eq!(rt.state.active().cursor().unwrap().1, old, "{why}: the removed line first");
                press(&mut rt, Key::Down, Mods::default());
            }
            assert_eq!(at(&rt).0, 2, "{why}");
            assert_eq!(rt.state.active().cursor().unwrap().1.side, rain_core::Side::New, "{why}");
            press(&mut rt, Key::End, Mods::default());
            assert_eq!(at(&rt), (2, 21), "{why}: End");
            press(&mut rt, Key::Home, Mods::default());
            assert_eq!(at(&rt), (2, 4), "{why}: Home, the first non-blank");
            press(&mut rt, Key::Home, Mods::default());
            assert_eq!(at(&rt), (2, 0), "{why}: Home again, column 0");
            press(&mut rt, Key::Right, ctrl());
            assert_eq!(at(&rt), (2, 4), "{why}: Ctrl+Right, a word");
            press(&mut rt, Key::Right, ctrl());
            assert_eq!(at(&rt), (2, 8), "{why}");
            press(&mut rt, Key::Left, Mods::default());
            assert_eq!(at(&rt), (2, 7), "{why}: Left");
            press(&mut rt, Key::Right, Mods::default());
            press(&mut rt, Key::End, shift());
            assert!(rt.state.active().visual().is_some(), "{why}: Shift+End selects");
            press(&mut rt, Key::Command('c'), ctrl());
            assert_eq!(clipboard.text().as_deref().map(String::as_str), Some("new_name = 22;"), "{why}");
            press(&mut rt, Key::Escape, Mods::default());
            assert!(rt.state.active().visual().is_none(), "{why}: Escape clears it");
            press(&mut rt, Key::Command('c'), ctrl());
            assert_eq!(
                clipboard.text().as_deref().map(String::as_str),
                Some("    let new_name = 22;"),
                "{why}: nothing selected copies the line"
            );
            press(&mut rt, Key::Down, Mods::default());
            assert_eq!(at(&rt), (3, 14), "{why}: Down after End keeps to the line's end");
            press(&mut rt, Key::End, ctrl());
            assert_eq!(at(&rt).0, 6, "{why}: Ctrl+End");
            press(&mut rt, Key::Home, ctrl());
            assert_eq!(at(&rt), (1, 0), "{why}: Ctrl+Home");
            press(&mut rt, Key::PageDown, Mods::default());
            assert_eq!(at(&rt).0, 6, "{why}: PageDown");
            press(&mut rt, Key::PageUp, shift());
            assert_eq!(at(&rt).0, 1, "{why}: Shift+PageUp");
            press(&mut rt, Key::Escape, Mods::default());
            press(&mut rt, Key::Command('a'), ctrl());
            press(&mut rt, Key::Command('c'), ctrl());
            assert_eq!(
                clipboard.text().map(|t| t.lines().count()),
                Some(6),
                "{why}: Ctrl+A then Ctrl+C copies the file's lines"
            );
            press(&mut rt, Key::Escape, Mods::default());
            assert!(rt.state.active().selection().is_none(), "{why}");
        }
    }
}

#[test]
fn shift_selection_comments_with_the_profiles_comment_key() {
    for (profile, key, mods) in [
        (Profile::Vim, Key::Text("c".into()), Mods::default()),
        (Profile::VsCode, Key::Command('/'), ctrl()),
        (Profile::JetBrains, Key::Command('/'), ctrl()),
    ] {
        let mut rt = rt_in(profile, ViewLayout::Unified);
        press(&mut rt, Key::Down, Mods::default());
        press(&mut rt, Key::Down, Mods::default());
        press(&mut rt, Key::Right, ctrl());
        press(&mut rt, Key::Right, ctrl());
        press(&mut rt, Key::Right, ctrl_shift());
        press(&mut rt, key, mods);
        let c = rt
            .state
            .composer
            .clone()
            .unwrap_or_else(|| panic!("{profile:?}: the comment key opens the composer"));
        assert!(
            matches!(
                c.anchor,
                rain_core::Anchor::Span {
                    start_line: 2,
                    start_col: 8,
                    end_line: 2,
                    ..
                }
            ),
            "{profile:?}: {:?}",
            c.anchor
        );
    }
}

#[test]
fn vim_motions_answer_only_in_the_vim_profile() {
    let mut rt = rt_in(Profile::VsCode, ViewLayout::Unified);
    press(&mut rt, Key::Down, Mods::default());
    typed(&mut rt, "jjwl$");
    assert_eq!(at(&rt), (1, 0), "letters don't move the cursor outside Vim");
    typed(&mut rt, "v");
    assert!(rt.state.active().visual().is_none());
    assert!(
        crate::palette::entries(&rt.state).iter().all(|e| e.kind != "Motion"),
        "no Vim rows in the palette"
    );
    rt.update(|s, _| s.set_profile(Profile::Vim));
    typed(&mut rt, "j");
    assert_eq!(at(&rt).0, 2);
    assert!(crate::palette::entries(&rt.state).iter().any(|e| e.kind == "Motion"));
}

#[test]
fn profile_switching_changes_lookups_and_every_hint() {
    let mut rt = rt_in(Profile::Vim, ViewLayout::Unified);
    let palette_hint = |s: &App, c: Command| {
        crate::palette::entries(s)
            .into_iter()
            .find(|e| e.item == crate::palette::Item::Cmd(c))
            .map(|e| e.hint)
            .unwrap_or_default()
    };
    let cases = [
        (Profile::Vim, "Ctrl+B", "F", "c", "n"),
        (Profile::VsCode, "Ctrl+B", "", "Ctrl+/", "Alt+F5"),
        (Profile::JetBrains, "Alt+1", "Ctrl+K", "Ctrl+/", "F7"),
    ];
    for (p, list, finish, comment, hunk) in cases {
        rt.update(|s, _| s.set_profile(p));
        settle(&mut rt);
        assert_eq!(with_active(|k| k.profile()), p);
        assert_eq!(Command::Comment.hint(), comment, "{p:?}");
        assert_eq!(palette_hint(&rt.state, Command::Comment), comment, "{p:?}: the palette");
        assert_eq!(palette_hint(&rt.state, Command::NextHunk), hunk, "{p:?}");
        assert_eq!(
            help(&rt, "list-toggle").as_deref(),
            Some(format!("Hide the file list  {list}").as_str()),
            "{p:?}"
        );
        let want = if finish.is_empty() {
            "Verdict and summary for this round".to_owned()
        } else {
            format!("Verdict and summary for this round  {finish}")
        };
        assert_eq!(help(&rt, "finish-review"), Some(want), "{p:?}");
        let hints = crate::view::hint_pairs(&rt.state);
        assert!(hints.iter().any(|(k, w)| k == comment && w == "comment"), "{p:?}: {hints:?}");
        assert!(hints.iter().all(|(k, _)| !k.is_empty()), "{p:?}: {hints:?}");
        rt.update(|s, _| s.toggle_zoom());
        let zoomed = rt.state.status.clone();
        rt.update(|s, _| s.toggle_zoom());
        let restore = Command::Zoom.hint();
        assert!(zoomed.ends_with(&format!("{restore} restore")), "{p:?}: {zoomed}");
        assert_eq!(rt.state.settings.keymap.profile, p, "saved with the settings");
    }
    rt.update(|s, _| s.set_profile(Profile::JetBrains));
    let before = rt.state.reader_tab();
    press(&mut rt, Key::Tab, ctrl());
    assert_ne!(rt.state.reader_tab(), before, "Ctrl+Tab switches Overview and Changes in JetBrains");
    press(&mut rt, Key::Function(7), Mods::default());
    press(&mut rt, Key::Function(7), Mods::default());
    assert!(rt.state.active().cursor().is_some(), "F7 goes to a hunk in JetBrains");
}

#[test]
fn arrows_still_drive_the_palette_picker_and_tree() {
    for p in Profile::ALL {
        let mut rt = runtime(app(story(), false));
        rt.update(|s, _| s.set_profile(p));
        rt.update(|s, _| s.open_palette());
        settle(&mut rt);
        press(&mut rt, Key::Down, Mods::default());
        press(&mut rt, Key::Down, Mods::default());
        assert_eq!(rt.state.palette.sel, 2, "{p:?}: Down moves the palette's row");
        assert!(rt.state.active().cursor().is_none(), "{p:?}: not the diff's cursor");
        press(&mut rt, Key::Escape, Mods::default());
        assert!(!rt.state.palette.open);

        rt.update(|s, tx| s.open_picker(tx));
        settle(&mut rt);
        let first = rt.state.picker.sel;
        press(&mut rt, Key::Down, Mods::default());
        assert_ne!(rt.state.picker.sel, first, "{p:?}: Down moves the picker's row");
        assert!(rt.state.active().cursor().is_none(), "{p:?}");
        press(&mut rt, Key::Escape, Mods::default());
        if p.vim_motions() {
            press(&mut rt, Key::Escape, Mods::default());
        }
        assert!(!rt.state.layout.is_open(Overlay::Picker), "{p:?}");

        rt.update(|s, _| s.board.folders = true);
        settle(&mut rt);
        let focused = |rt: &Runtime<App>| rt.elements().into_iter().find(|e| e.focused).and_then(|e| e.key);
        for _ in 0..80 {
            if focused(&rt).as_deref() == Some(rain_tree::TREE_KEY) {
                break;
            }
            press(&mut rt, Key::Tab, Mods::default());
        }
        assert_eq!(focused(&rt).as_deref(), Some(rain_tree::TREE_KEY), "{p:?}");
        let before = rt.state.tree.model.cursor().map(str::to_owned);
        press(&mut rt, Key::Down, Mods::default());
        assert_ne!(
            rt.state.tree.model.cursor().map(str::to_owned),
            before,
            "{p:?}: Down moves in the tree"
        );
        assert!(rt.state.active().cursor().is_none(), "{p:?}: and not in the diff");
    }
}

fn search(rt: &mut Runtime<App>, q: &str) {
    rt.update(|s, _| s.shortcuts.search.text = q.into());
    settle(rt);
}

#[test]
fn the_shortcuts_screen_records_reports_a_conflict_saves_and_cancels() {
    let mut rt = rt_in(Profile::Vim, ViewLayout::Unified);
    press(&mut rt, Key::Function(1), Mods::default());
    assert!(rt.state.shortcuts.open, "F1 opens the shortcuts screen");
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.key.as_deref() == Some("shortcuts") && e.role == "dialog")
    );

    let generation = rt.state.shortcuts.generation;
    typed(&mut rt, "comment");
    assert_eq!(rt.state.shortcuts.search.text, "comment", "typing goes to the search field");
    assert_eq!(rt.state.shortcuts.generation, generation, "searching doesn't rebuild the draft");
    assert!(rt.state.shortcuts.matches().contains(&Command::Comment));
    assert!(!rt.state.shortcuts.matches().contains(&Command::Find));
    click(&mut rt, "sc-comment-record");
    assert_eq!(rt.state.shortcuts.recording, Some(Command::Comment));
    press(&mut rt, Key::Down, Mods::default());
    assert!(rt.state.shortcuts.note.contains("universal"), "{}", rt.state.shortcuts.note);
    assert_eq!(rt.state.shortcuts.recording, Some(Command::Comment), "still recording");
    press(&mut rt, Key::Command('m'), ctrl());
    assert_eq!(rt.state.shortcuts.recording, None);
    let keys = |rt: &Runtime<App>, c: Command| rt.state.shortcuts.draft.chords(c).iter().map(|k| k.display()).collect::<Vec<_>>();
    assert_eq!(keys(&rt, Command::Comment), ["c", "Ctrl+M"]);
    assert_eq!(Command::Comment.hint(), "c", "nothing changes before Save");

    search(&mut rt, "find");
    click(&mut rt, "sc-find-record");
    press(&mut rt, Key::Command('k'), ctrl());
    let conflict = rt.state.shortcuts.conflict.expect("Ctrl+K is the palette's");
    assert_eq!((conflict.cmd, conflict.holder), (Command::Find, Command::Palette));
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.key.as_deref() == Some("sc-conflict") && e.role == "alert")
    );
    click(&mut rt, "sc-conflict-replace");
    assert_eq!(keys(&rt, Command::Find), ["/", "Ctrl+K"]);
    assert!(keys(&rt, Command::Palette).is_empty());

    search(&mut rt, "comment");
    click(&mut rt, "sc-comment-remove-0");
    assert_eq!(keys(&rt, Command::Comment), ["Ctrl+M"]);
    click(&mut rt, "sc-save");
    assert!(!rt.state.shortcuts.open);
    assert_eq!(
        with_active(|k| k.lookup(&rain_core::Chord::parse("Ctrl+K").unwrap())),
        Some(Command::Find)
    );
    assert_eq!(Command::Comment.hint(), "Ctrl+M");
    let path = rt.state.settings_path.clone().unwrap();
    let saved = crate::settings::Settings::load(&path);
    let (ov, bad) = saved.keymap.overrides();
    assert!(bad.is_empty());
    assert_eq!(ov.get("comment").map(Vec::as_slice), Some(&["Ctrl+M".to_owned()][..]));
    assert_eq!(ov.get("palette").map(Vec::len), Some(0));
    assert_eq!(ov.get("find").map(Vec::len), Some(2));

    press(&mut rt, Key::Command(','), ctrl());
    assert!(rt.state.shortcuts.open, "Ctrl+, opens it too");
    click(&mut rt, "sc-reset-all");
    rt.update(|s, _| s.shortcuts_profile(Profile::VsCode));
    settle(&mut rt);
    assert_eq!(rt.state.shortcuts.draft.hint(Command::Palette), "Ctrl+Shift+P");
    assert!(!rt.state.shortcuts.changed(Command::Palette), "against the new profile's keys");
    click(&mut rt, "sc-cancel");
    assert!(!rt.state.shortcuts.open);
    assert_eq!(rt.state.settings.keymap.profile, Profile::Vim, "Cancel keeps the profile");
    assert_eq!(Command::Comment.hint(), "Ctrl+M", "and the saved keys");

    rt.update(|s, _| s.open_shortcuts());
    settle(&mut rt);
    search(&mut rt, "comment");
    click(&mut rt, "sc-comment-reset");
    assert_eq!(keys(&rt, Command::Comment), ["c"]);
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.shortcuts.open, "Escape cancels");
    assert_eq!(Command::Comment.hint(), "Ctrl+M");
}

#[test]
fn bad_keymap_settings_are_ignored_with_a_status_line() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    std::fs::write(
        &path,
        r#"{"keymap": {"profile": "vscode", "overrides": {"comment": ["Ctrl+Nope"], "zap": []}}}"#,
    )
    .unwrap();
    let a = App::new(
        Launch {
            session: "t".into(),
            fixture: Some(story()),
            ..Launch::default()
        },
        Arc::new(Shared::default()),
        Some(dir.join("sessions")),
        Some(path),
    );
    assert!(a.status.starts_with("Keyboard shortcuts: ignored"), "{}", a.status);
    assert_eq!(a.keymap_profile(), Profile::VsCode);
    assert_eq!(Command::Comment.hint(), "", "the bad entry still replaced the default");
}

#[test]
fn cursor_keys_rebuild_only_the_rows_they_touch_in_every_profile() {
    for p in Profile::ALL {
        let mut rt = rt_in(p, ViewLayout::Unified);
        press(&mut rt, Key::Down, Mods::default());
        press(&mut rt, Key::Down, Mods::default());
        let (builds, commits) = (rain_diffview::line_builds(), rune::commit_generation());
        for _ in 0..40 {
            for key in [Key::Right, Key::Left] {
                rt.key(KeyEvent {
                    key,
                    mods: Mods::default(),
                });
                rt.frame(1.0 / 60.0);
            }
        }
        let (b, c) = (rain_diffview::line_builds() - builds, rune::commit_generation() - commits);
        println!("{p:?} 80 arrows: {b} line builds, {c} commits");
        assert!(b <= 80, "{p:?}: 80 arrow presses built {b} lines: the cursor's row once each");
        assert!(c <= 90, "{p:?}: {c} commits for 80 arrow presses");
        settle(&mut rt);
        let commits = rune::commit_generation();
        for _ in 0..60 {
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(rune::commit_generation() - commits, 0, "{p:?}: idle frames commit nothing");
    }
}
