//! Find's fuzzy mode: off until switched on, remembered, bounded, and marking what it matched.

use rain_core::command::Command;
use rain_core::fuzzy::{Matcher, Query};
use rune::{Key, Mods, Runtime};

use super::*;
use crate::settings::Settings;

fn alt() -> Mods {
    Mods {
        alt: true,
        ..Mods::default()
    }
}

fn click(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("{key} is drawn"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn info(rt: &Runtime<App>, key: &str) -> rune::ElementInfo {
    let all = rt.elements().into_iter();
    all.filter(|e| !e.exiting)
        .find(|e| e.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("{key} is drawn"))
}

/// Find's hits for `q`, searched from scratch.
fn hits(rt: &mut Runtime<App>, q: &'static str) -> Vec<(usize, u32)> {
    rt.update(move |s, _| {
        s.find.edit.text = q.into();
        s.run_find();
    });
    rt.state.find.hits.clone()
}

fn line(rt: &Runtime<App>, (f, i): (usize, u32)) -> String {
    rt.state.active().files()[f].line_text(i).to_owned()
}

#[test]
fn a_failed_save_of_the_fuzzy_switch_stays_in_the_status_line() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    let mut a = App::new(
        rain_core::Launch::default(),
        std::sync::Arc::new(rain_core::Shared::default()),
        None,
        Some(path.clone()),
    );
    a.toggle_find_fuzzy();
    assert_eq!(a.status, "Find: fuzzy matching", "a save that works leaves the switch's line");
    assert!(Settings::load(&path).find_fuzzy);
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir_all(&path).unwrap();
    a.toggle_find_fuzzy();
    assert!(!a.settings.find_fuzzy, "the switch still turns");
    assert!(a.status.starts_with("settings not saved"), "{}", a.status);
}

#[test]
fn fuzzy_find_is_off_by_default_and_old_settings_load_with_it_off() {
    assert!(!Settings::default().find_fuzzy);
    let old = Settings::parse(br#"{"theme": "dark", "sidebarWidth": 300}"#).expect("an old file loads");
    assert!(!old.find_fuzzy);
    assert_eq!(old.sidebar_width, 300.0);
    for odd in [r#""yes""#, "1", "null", "[true]"] {
        let s = Settings::parse(format!(r#"{{"findFuzzy": {odd}, "sidebarWidth": 300}}"#).as_bytes())
            .unwrap_or_else(|e| panic!("{odd} still loads: {e}"));
        assert!(!s.find_fuzzy && s.sidebar_width == 300.0, "{odd}");
    }
    assert!(Settings::parse(br#"{"findFuzzy": true}"#).unwrap().find_fuzzy);
    assert!(
        serde_json::to_string(&Settings::default())
            .unwrap()
            .contains(r#""findFuzzy":false"#)
    );
}

#[test]
fn the_switch_turns_fuzzy_on_from_the_bar_the_keyboard_and_the_palette_and_is_remembered() {
    let mut rt = runtime(app(story(), true));
    let path = rt.state.settings_path.clone().unwrap();
    typed(&mut rt, "/");
    typed(&mut rt, "qzj");
    assert_eq!(info(&rt, "find-count").text.as_deref(), Some("no matches"));
    let button = info(&rt, "find-fuzzy");
    assert_eq!(button.help.as_deref(), Some("Fuzzy match  Alt+F"), "the key is in the tooltip");
    assert_eq!(button.label.as_deref(), Some("Fuzzy"));
    let face: Vec<String> = rt
        .elements()
        .into_iter()
        .filter(|e| e.text.as_deref().is_some_and(|t| t.contains("Fuzzy")))
        .filter_map(|e| e.text)
        .collect();
    assert_eq!(face, ["Fuzzy"], "and not on the button's face");
    let places = |rt: &Runtime<App>| ["find-count", "find-prev", "find-next", "find-fuzzy", "find-close"].map(|k| info(rt, k).rect);
    let before = places(&rt);

    press(&mut rt, Key::Text("f".into()), alt());
    assert!(rt.state.settings.find_fuzzy, "Alt+F in the box");
    assert_eq!(rt.state.find.edit.text, "qzj", "it types nothing");
    assert!(rt.state.find.edit.focused, "and the box keeps the keyboard");
    assert!(Settings::load(&path).find_fuzzy, "saved");
    assert_eq!(places(&rt), before, "nothing in the bar moves");

    click(&mut rt, "find-fuzzy");
    assert!(!rt.state.settings.find_fuzzy, "a press switches it back");
    assert!(!Settings::load(&path).find_fuzzy);
    assert_eq!(places(&rt), before);

    rt.update(|s, _| {
        s.open_find();
        s.find.edit.text = "vouchserv".into();
        s.run_find();
    });
    settle(&mut rt);
    assert!(rt.state.find.hits.is_empty(), "exact: no line holds that text");
    press(&mut rt, Key::Text("f".into()), alt());
    assert!(!rt.state.find.hits.is_empty(), "switching searches the query again, fuzzily");
    assert!(tab_to(&mut rt, "find-fuzzy"), "Tab reaches it");
    press(&mut rt, Key::Enter, Mods::default());
    assert!(!rt.state.settings.find_fuzzy, "Enter presses it");
    assert!(rt.state.find.hits.is_empty());
    press(&mut rt, Key::Enter, Mods::default());
    assert!(rt.state.settings.find_fuzzy);

    press(&mut rt, Key::Escape, Mods::default());
    rt.update(|s, _| s.close_find());
    settle(&mut rt);
    assert_eq!(Command::from_id("toggle-fuzzy-find"), Some(Command::ToggleFindFuzzy));
    let rows = crate::palette::filtered(&rt.state, "fuzzy find");
    assert_eq!(rows[0].item, crate::palette::Item::Cmd(Command::ToggleFindFuzzy), "{rows:?}");
    rt.update(|s, tx| {
        s.run(Command::ToggleFindFuzzy, tx);
    });
    assert!(!rt.state.settings.find_fuzzy && rt.state.status.contains("exact"));
    rt.update(|s, tx| {
        s.run(Command::ToggleFindFuzzy, tx);
    });
    assert!(rt.state.settings.find_fuzzy && Settings::load(&path).find_fuzzy);

    let again = App::new(
        rain_core::Launch {
            session: "t2".into(),
            fixture: Some(story()),
            ..rain_core::Launch::default()
        },
        std::sync::Arc::new(rain_core::Shared::default()),
        None,
        Some(path),
    );
    assert!(again.settings.find_fuzzy, "a new window opens with it on");
}

#[test]
fn fuzzy_find_matches_each_word_closely_in_document_order() {
    let mut rt = runtime(app(story(), true));
    let exact = hits(&mut rt, "voucherservice");
    assert!(exact.len() >= 2, "{exact:?}");
    rt.update(|s, _| s.toggle_find_fuzzy());

    let found = hits(&mut rt, "vouchserv");
    assert!(found.windows(2).all(|w| w[0] < w[1]), "document order, a line once: {found:?}");
    for h in &exact {
        assert!(found.contains(h), "a line holding VoucherService fits: {}", line(&rt, *h));
    }
    let mut m = Matcher::in_words();
    let q = Query::new("vouchserv");
    let mut want = Vec::new();
    for (fi, f) in rt.state.active().files().iter().enumerate() {
        for i in 0..f.lines.len() as u32 {
            if m.fit(&q, f.line_text(i), 0).score().is_some() {
                want.push((fi, i));
            }
        }
    }
    assert_eq!(found, want, "the lines the diff marks letters in");

    let two = hits(&mut rt, "service voucher");
    assert!(!two.is_empty());
    assert_eq!(hits(&mut rt, "voucher service"), two, "words in any order");
    let one = hits(&mut rt, "voucher");
    assert!(
        one.len() > two.len() && two.iter().all(|h| one.contains(h)),
        "every word must match"
    );

    rt.update(|s, _| s.toggle_find_fuzzy());
    let spaced = hits(&mut rt, "public class");
    assert!(!spaced.is_empty(), "the fixture has such lines");
    rt.update(|s, _| s.toggle_find_fuzzy());
    let glued = hits(&mut rt, "publicclass");
    assert!(spaced.iter().all(|h| glued.contains(h)), "neighbouring words may be typed as one");
    let both = hits(&mut rt, "class public");
    assert!(spaced.iter().all(|h| both.contains(h)), "or as two, in any order");
    assert!(!hits(&mut rt, "vuocherservice").is_empty(), "a long word may have a letter wrong");
    assert!(hits(&mut rt, "   ").is_empty(), "a blank query matches nothing");
    assert!(hits(&mut rt, "qzj").is_empty());
}

#[test]
fn enter_steps_through_fuzzy_matches_in_document_order() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| s.toggle_find_fuzzy());
    typed(&mut rt, "/");
    typed(&mut rt, "vouch");
    let found = rt.state.find.hits.clone();
    assert!(found.len() >= 3, "{found:?}");
    assert_eq!(info(&rt, "find-count").text, Some(format!("1 of {}", found.len())));
    let cursor_line = |rt: &Runtime<App>| {
        let (f, at) = rt.state.active().cursor().expect("a cursor");
        let file = &rt.state.active().files()[f];
        file.lines
            .iter()
            .position(|r| {
                if at.side == rain_core::Side::New {
                    r.new == at.line
                } else {
                    r.old == at.line && r.new == 0
                }
            })
            .map(|i| (f, i as u32))
    };
    press(&mut rt, Key::Enter, Mods::default());
    assert_eq!(rt.state.find.at, 1);
    assert_eq!(rt.state.status, format!("Match 2 of {}", found.len()));
    assert_eq!(cursor_line(&rt), Some(found[1]), "the reader goes to the match");
    press(&mut rt, Key::Enter, Mods::default());
    assert_eq!((rt.state.find.at, cursor_line(&rt)), (2, Some(found[2])));
    let back = Mods {
        shift: true,
        ..Mods::default()
    };
    press(&mut rt, Key::Enter, back);
    press(&mut rt, Key::Enter, back);
    press(&mut rt, Key::Enter, back);
    assert_eq!(rt.state.find.at, found.len() - 1, "Shift+Enter steps back, around the top");
    assert_eq!(rt.state.find.hits, found, "stepping searches nothing again");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.find.open);
}

/// A patch adding one file of `lines` lines, each about 40 bytes.
pub(super) fn big_patch(lines: usize) -> std::path::PathBuf {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let mut text = format!("diff --git a/big.txt b/big.txt\nnew file mode 100644\n--- /dev/null\n+++ b/big.txt\n@@ -0,0 +1,{lines} @@\n");
    for i in 0..lines {
        text.push_str(&format!("+let total_{i:06} = ledger.sum(entries);\n"));
    }
    let path = dir.join("big.patch");
    std::fs::write(&path, text).unwrap();
    path
}

#[test]
fn a_large_diff_is_searched_fuzzily_on_enter_not_on_every_key_and_hits_are_capped() {
    let mut rt = runtime(app(big_patch(30_000), false));
    let bytes: usize = rt.state.active().files().iter().map(|f| f.text.len()).sum();
    assert!(bytes > 1 << 20 && bytes < 8 << 20, "{bytes}");
    assert!(rt.state.find_is_live(), "exact find still searches as you type");
    rt.update(|s, _| s.toggle_find_fuzzy());
    assert!(!rt.state.find_is_live());
    typed(&mut rt, "/");
    typed(&mut rt, "ledsum");
    assert_eq!(rt.state.find.query, "", "typing searched nothing");
    assert!(rt.state.find.hits.is_empty());
    press(&mut rt, Key::Enter, Mods::default());
    assert_eq!(rt.state.find.query, "ledsum");
    assert_eq!(rt.state.find.hits.len(), 10_000, "capped");
    assert_eq!(rt.state.find.hits[..2], [(0, 0), (0, 1)]);
}
