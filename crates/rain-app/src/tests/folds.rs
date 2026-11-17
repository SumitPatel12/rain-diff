use rain_core::Command;
use rain_core::keymap::Profile;
use rune::{Key, Mods, Runtime};

use super::*;
use crate::load::LARGE_FILE_LINES;

/// A patch adding three small files and one over [`LARGE_FILE_LINES`] lines.
fn folds_patch() -> PathBuf {
    let mut p = String::new();
    for name in ["a.rs", "b.rs", "c.rs"] {
        p.push_str(&format!(
            "diff --git a/{name} b/{name}\nnew file mode 100644\n--- /dev/null\n+++ b/{name}\n@@ -0,0 +1,2 @@\n+one\n+two\n"
        ));
    }
    let n = LARGE_FILE_LINES + 1;
    p.push_str(&format!(
        "diff --git a/big.rs b/big.rs\nnew file mode 100644\n--- /dev/null\n+++ b/big.rs\n@@ -0,0 +1,{n} @@\n"
    ));
    for i in 0..n {
        p.push_str(&format!("+line {i}\n"));
    }
    let d = temp_dir();
    std::fs::create_dir_all(&d).unwrap();
    let path = d.join("folds.patch");
    std::fs::write(&path, p).unwrap();
    path
}

fn collapsed(rt: &Runtime<App>) -> Vec<bool> {
    let a = rt.state.active();
    (0..a.files().len()).map(|l| a.collapsed(l)).collect()
}

fn more_menu_item(rt: &mut Runtime<App>, label: &str) {
    rt.update(|s, _| s.menus.more = true);
    settle(rt);
    let at = rt
        .elements()
        .into_iter()
        .find(|e| !e.exiting && e.role.starts_with("menuitem") && e.label.as_deref() == Some(label))
        .map(|e| (e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0))
        .unwrap_or_else(|| panic!("no menu item {label}"));
    rt.pointer_move(at.0, at.1);
    rt.pointer_down(at.0, at.1);
    rt.pointer_up(at.0, at.1);
    settle(rt);
}

#[test]
fn collapse_and_expand_all_files_from_the_more_menu() {
    let mut rt = runtime(app(folds_patch(), false));
    let big = rt.state.keys.iter().position(|k| k.ends_with("big.rs")).unwrap();
    let mut want = vec![false; 4];
    want[big] = true;
    assert_eq!(collapsed(&rt), want, "the load folds only the large file");

    more_menu_item(&mut rt, "Collapse all files");
    assert_eq!(collapsed(&rt), vec![true; 4]);
    assert_eq!(rt.state.active().row_count(), 4, "headers only");
    assert_eq!(rt.state.status, "All files collapsed");
    assert!(!rt.state.menus.more, "the menu closes");

    more_menu_item(&mut rt, "Expand all files");
    assert_eq!(collapsed(&rt), want, "the large file stays folded");
    assert_eq!(rt.state.status, "Files expanded; 1 large file stays collapsed");
}

/// The key the `⋯` menu shows on the item `label`, which it opens.
fn menu_shortcut(rt: &mut Runtime<App>, label: &str) -> Option<String> {
    rt.update(|s, _| s.menus.more = true);
    settle(rt);
    let els = rt.elements();
    let item = els
        .iter()
        .position(|e| !e.exiting && e.role.starts_with("menuitem") && e.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("no menu item {label}"));
    let key = els
        .iter()
        .find(|e| e.parent == Some(item) && e.key.as_deref() == Some("shortcut"))
        .and_then(|e| e.text.clone());
    rt.update(|s, _| {
        s.menus.close();
    });
    settle(rt);
    key
}

#[test]
fn fold_commands_answer_their_keys_and_show_them_in_the_menu_and_palette_per_profile() {
    let shift_ctrl = Mods { shift: true, ..ctrl() };
    for (p, collapse, expand, keys) in [
        (Profile::Vim, '_', '+', ["Ctrl+Shift+-", "Ctrl+Shift+=", "Z"]),
        (Profile::VsCode, '{', '}', ["Ctrl+Shift+[", "Ctrl+Shift+]", ""]),
        (Profile::JetBrains, '_', '+', ["Ctrl+Shift+-", "Ctrl+Shift+=", ""]),
    ] {
        let mut rt = runtime(app(folds_patch(), false));
        rt.update(|s, _| s.set_profile(p));
        settle(&mut rt);
        let want = collapsed(&rt);
        press(&mut rt, Key::Command(collapse), shift_ctrl);
        assert_eq!(collapsed(&rt), vec![true; 4], "{p:?}: Ctrl+Shift+{collapse}");
        press(&mut rt, Key::Command(expand), shift_ctrl);
        assert_eq!(collapsed(&rt), want, "{p:?}: Ctrl+Shift+{expand}");

        let cmds = [Command::CollapseAllFiles, Command::ExpandAllFiles, Command::CollapseAllContext];
        let labels = ["Collapse all files", "Expand all files", "Collapse all context"];
        for ((c, label), key) in cmds.into_iter().zip(labels).zip(keys) {
            let shown = menu_shortcut(&mut rt, label);
            assert_eq!(shown.as_deref().unwrap_or(""), key, "{p:?}: the menu's {label}");
            let hint = crate::palette::entries(&rt.state)
                .into_iter()
                .find(|e| e.item == crate::palette::Item::Cmd(c))
                .map(|e| e.hint);
            assert_eq!(hint.as_deref(), Some(key), "{p:?}: the palette's {label}");
        }
    }
}
