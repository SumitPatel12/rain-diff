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

/// A patch adding a small file and a file that is one 2 MB line ending in `needle`.
fn wide_patch() -> PathBuf {
    let line = format!("{} needle", "var a=b.c(d,e)+f[g];".repeat(100_000));
    let p = format!(
        "diff --git a/a.rs b/a.rs\nnew file mode 100644\n--- /dev/null\n+++ b/a.rs\n@@ -0,0 +1,2 @@\n+one\n+two\n\
         diff --git a/min.js b/min.js\nnew file mode 100644\n--- /dev/null\n+++ b/min.js\n@@ -0,0 +1 @@\n+{line}\n"
    );
    let d = temp_dir();
    std::fs::create_dir_all(&d).unwrap();
    let path = d.join("wide.patch");
    std::fs::write(&path, p).unwrap();
    path
}

#[test]
fn a_file_that_is_one_enormous_line_starts_folded_and_find_still_reaches_into_it() {
    let mut rt = runtime(app(wide_patch(), false));
    let min = rt.state.keys.iter().position(|k| k.ends_with("min.js")).unwrap();
    let mut want = vec![false; 2];
    want[min] = true;
    assert_eq!(collapsed(&rt), want, "the load folds only the one-line file");
    rt.update(|s, _| s.expand_all_files());
    assert_eq!(collapsed(&rt), want, "and Expand all leaves it folded");

    rt.update(|s, tx| {
        s.find.edit.text = "needle".into();
        s.find_step(true, tx);
    });
    settle(&mut rt);
    assert_eq!(rt.state.find.hits, vec![(min, 0)], "the match past the cut counts");
    assert_eq!(rt.state.status, "Match 1 of 1 · in the part of a long line that isn't drawn");
    assert!(!rt.state.active().collapsed(min), "going to it opens the file");
    let at = rain_diffview::LineRef::new(rain_core::diff::Side::New, 1);
    assert_eq!(rt.state.active().cursor(), Some((min, at)), "and lands on the line");
    let line = rt.state.active().copy_text().expect("the cursor's line");
    assert!(
        line.len() > 2_000_000 && line.ends_with("needle"),
        "copying it takes the whole line"
    );

    rt.update(|s, tx| {
        s.find.edit.text = "var".into();
        s.find_step(true, tx);
    });
    assert_eq!(rt.state.status, "Match 1 of 1", "a match in the drawn part needs no note");
}

/// A patch changing one word of a file that is a single line of `chars` characters.
fn one_line_patch(chars: usize) -> PathBuf {
    let mut new = "var a=b.c(d,e)+f[g];".repeat(chars / 20 + 1);
    new.truncate(chars);
    let old = new.replacen("var a", "let a", 1);
    let d = temp_dir();
    std::fs::create_dir_all(&d).unwrap();
    let path = d.join("one-line.patch");
    std::fs::write(
        &path,
        format!("diff --git a/web/app.js b/web/app.js\nindex 1111111..2222222 100644\n--- a/web/app.js\n+++ b/web/app.js\n@@ -1 +1 @@\n-{old}\n+{new}\n"),
    )
    .unwrap();
    path
}

#[test]
fn a_one_line_file_that_is_the_whole_diff_starts_folded() {
    let a = app(one_line_patch(200_000), false);
    assert!(a.diff.collapsed(0), "the load folds it");
    let mut rt = runtime(a);
    assert_eq!(collapsed(&rt), [true], "opening at the first file leaves it folded");
    assert_eq!(rt.state.active().row_count(), 1, "the header alone");

    rt.update(|s, tx| s.toggle_reader_mode(tx));
    settle(&mut rt);
    assert!(rt.state.single_mode());
    assert_eq!(collapsed(&rt), [true], "and so does showing it alone");
    rt.update(|s, tx| s.toggle_reader_mode(tx));
    settle(&mut rt);
    assert_eq!(collapsed(&rt), [true], "and going back to the stream");

    rt.update(|s, _| s.active_mut().set_collapsed(0, false));
    settle(&mut rt);
    assert!(rt.state.active().row_count() > 1, "it opens from its header");
}

/// `cargo test -p rain-app --release --lib -- --ignored --nocapture one_line_file_of_two_megabytes`
#[test]
#[ignore]
fn a_one_line_file_of_two_megabytes_loads_and_installs() {
    let launch = Launch {
        session: "t".into(),
        fixture: Some(one_line_patch(2_000_000)),
        ..Launch::default()
    };
    let dir = temp_dir();
    let mut a = App::new(
        launch.clone(),
        Arc::new(Shared::default()),
        Some(dir.join("sessions")),
        Some(dir.join("settings.json")),
    );
    let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1e3;
    let t = std::time::Instant::now();
    let data = crate::load::load(&launch, &a.shared, &|_| {}).expect("the patch loads");
    let load_ms = ms(t);
    let t = std::time::Instant::now();
    a.install(data);
    let install_ms = ms(t);
    let t = std::time::Instant::now();
    let rt = runtime(a);
    println!(
        "PERF one_line_app chars=2000000 load_ms={load_ms:.1} install_ms={install_ms:.1} first_frames_ms={:.1} folded={} rows={}",
        ms(t),
        rt.state.diff.collapsed(0),
        rt.state.diff.row_count()
    );
    assert!(rt.state.diff.collapsed(0));
    assert!(install_ms < 1000.0, "install took {install_ms:.0} ms");
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
