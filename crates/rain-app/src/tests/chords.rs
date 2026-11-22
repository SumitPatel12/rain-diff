//! Two-key shortcuts: the first key held for the second, Vim's rule for a key that also acts
//! alone, and what a key that starts no shortcut costs.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use rain_core::Command;
use rune::{Key, KeyEvent, Mods, Runtime};

use super::vim::{at, vim_rt};
use super::*;

/// Sets the keymap's overrides; the wait is a minute, so no test's clock ends it.
fn bind(rt: &mut Runtime<App>, keys: &[(&str, &[&str])]) {
    let o: BTreeMap<String, Vec<String>> = keys
        .iter()
        .map(|(id, k)| ((*id).to_owned(), k.iter().map(|s| (*s).to_owned()).collect()))
        .collect();
    rt.update(|s, _| {
        s.settings.keymap.timeout_ms = Some(60_000);
        s.settings.keymap.set_overrides(&o);
        s.apply_keymap();
    });
    settle(rt);
}

fn tap(rt: &mut Runtime<App>, c: &str) {
    rt.key(KeyEvent {
        key: Key::Text(c.into()),
        mods: Mods::default(),
    });
}

fn held(rt: &Runtime<App>) -> Option<String> {
    rt.state.key_wait.chord().map(|c| c.display())
}

/// The poll as it runs `after` from now.
fn later(rt: &mut Runtime<App>, after: Duration) {
    rt.update(|s, tx| {
        s.held_key_expire_at(Instant::now() + after, tx);
    });
    settle(rt);
}

fn help(rt: &Runtime<App>, key: &str) -> Option<String> {
    rt.elements()
        .into_iter()
        .find(|e| !e.exiting && e.key.as_deref() == Some(key))
        .and_then(|e| e.help)
}

fn list(rt: &Runtime<App>) -> bool {
    rt.state.sidebar.is_open()
}

#[test]
fn a_two_key_shortcut_runs_on_its_second_key_and_reads_as_both_keys() {
    let mut rt = vim_rt(40);
    bind(&mut rt, &[("toggle-list", &["Ctrl+B", "q l"]), ("finish-review", &["q f"])]);
    assert!(!rt.state.status.contains("ignored"), "{}", rt.state.status);
    let open = list(&rt);
    typed(&mut rt, "q");
    assert_eq!((list(&rt), held(&rt).as_deref()), (open, Some("q")), "q waits");
    typed(&mut rt, "l");
    assert_eq!((list(&rt), held(&rt)), (!open, None), "q then l toggles the list");
    press(&mut rt, Key::Command('b'), ctrl());
    assert_eq!(list(&rt), open, "its single key still does");

    assert_eq!(Command::FinishReview.hint(), "q f");
    assert_eq!(
        help(&rt, "finish-review").as_deref(),
        Some("Verdict and summary for this round  q f"),
        "the tooltip"
    );
    assert_eq!(crate::keys::tip("Finish", Command::FinishReview), "Finish  q f");
    let hint = |c: Command| {
        crate::palette::entries(&rt.state)
            .into_iter()
            .find(|e| e.item == crate::palette::Item::Cmd(c))
            .map(|e| e.hint)
    };
    assert_eq!(hint(Command::FinishReview).as_deref(), Some("q f"), "the palette");
    assert_eq!(hint(Command::ToggleList).as_deref(), Some("Ctrl+B"), "the first key is the hint");
}

/// Line builds, commits and the next wake after the same keys.
fn cost_of_plain_keys(pairs: bool) -> (u64, u64, Option<f64>) {
    let mut rt = vim_rt(400);
    let keys: &[(&str, &[&str])] = if pairs { &[("toggle-density", &["q d"])] } else { &[] };
    bind(&mut rt, keys);
    typed(&mut rt, "jj");
    let (builds, commits) = (rain_diffview::line_builds(), rune::commit_generation());
    for _ in 0..25 {
        for k in ["l", "j", "w", "k"] {
            tap(&mut rt, k);
            rt.frame(1.0 / 60.0);
        }
        for key in [Key::Down, Key::Right] {
            rt.key(KeyEvent {
                key,
                mods: Mods::default(),
            });
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(held(&rt), None);
    }
    (
        rain_diffview::line_builds() - builds,
        rune::commit_generation() - commits,
        rt.wake_deadline(),
    )
}

#[test]
fn a_key_that_starts_no_shortcut_builds_commits_and_wakes_exactly_as_without_any() {
    let (without, with) = (cost_of_plain_keys(false), cost_of_plain_keys(true));
    println!("150 keys: {without:?} without a two-key shortcut, {with:?} with one");
    assert_eq!(with, without, "line builds, commits, next wake");
}

#[test]
fn a_key_that_only_starts_shortcuts_waits_with_no_timer_and_no_view() {
    let mut rt = vim_rt(40);
    bind(&mut rt, &[("toggle-density", &["q d"])]);
    typed(&mut rt, "j");
    settle(&mut rt);
    rt.check_live_only(false);
    let (wake, commits) = (rt.wake_deadline(), rune::commit_generation());
    tap(&mut rt, "q");
    for _ in 0..120 {
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(held(&rt).as_deref(), Some("q"));
    assert!(!rt.state.key_wait.timed(), "nothing to fall back to, so no timeout");
    assert_eq!(rt.wake_deadline(), wake, "no timer");
    assert_eq!(rune::commit_generation() - commits, 0, "the view never ran");
    later(&mut rt, Duration::from_secs(3600));
    assert_eq!(held(&rt).as_deref(), Some("q"), "an hour on it still waits");
    let density = rt.state.settings.density;
    typed(&mut rt, "d");
    assert_ne!(rt.state.settings.density, density, "q then d");
}

#[test]
fn a_key_that_also_acts_alone_waits_the_timeout_then_acts() {
    let mut rt = vim_rt(40);
    bind(&mut rt, &[("toggle-list", &["x"]), ("toggle-density", &["x d"])]);
    let said = rt.state.status.clone();
    assert!(
        said.contains("x also starts a two-key shortcut") && said.contains("waits 60000 ms"),
        "{said}"
    );
    typed(&mut rt, "j");
    let (open, density) = (list(&rt), rt.state.settings.density);

    tap(&mut rt, "x");
    rt.frame(1.0 / 60.0);
    assert_eq!(held(&rt).as_deref(), Some("x"));
    assert!(rt.state.key_wait.timed(), "a timer runs");
    assert!(rt.wake_deadline().is_some());
    assert_eq!(list(&rt), open, "not yet");
    later(&mut rt, Duration::from_secs(30));
    assert_eq!((list(&rt), held(&rt).as_deref()), (open, Some("x")), "half the wait");
    later(&mut rt, Duration::from_secs(61));
    assert_eq!((list(&rt), held(&rt)), (!open, None), "the wait over, x acts alone");

    typed(&mut rt, "xd");
    assert_ne!(rt.state.settings.density, density, "x then d is the shortcut");
    assert_eq!(list(&rt), !open, "and x did not act");

    let line = at(&rt).0;
    typed(&mut rt, "xj");
    assert_eq!(list(&rt), open, "another key: x acts alone at once");
    assert_eq!(at(&rt).0, line + 1, "and that key is read afresh");
    assert_eq!(held(&rt), None);

    typed(&mut rt, "xx");
    assert_eq!((list(&rt), held(&rt).as_deref()), (!open, Some("x")), "the second x waits in turn");
    press(&mut rt, Key::Escape, Mods::default());
    assert_eq!((list(&rt), held(&rt)), (!open, None));

    rt.update(|s, _| s.settings.keymap.timeout_ms = Some(20_000));
    typed(&mut rt, "x");
    later(&mut rt, Duration::from_secs(1));
    assert_eq!(held(&rt).as_deref(), Some("x"));
    later(&mut rt, Duration::from_secs(21));
    assert_eq!((list(&rt), held(&rt)), (open, None), "the setting is the wait");

    rt.update(|s, _| s.settings.keymap.timeout_ms = Some(0));
    typed(&mut rt, "x");
    assert_eq!((list(&rt), held(&rt)), (!open, None), "no wait: the next frame's poll runs x");
}

#[test]
fn vims_waiting_keys_and_counts_work_beside_shortcuts_on_their_keys() {
    let mut rt = vim_rt(40);
    bind(&mut rt, &[("toggle-density", &["g d"]), ("reload", &["w x"])]);
    typed(&mut rt, "j");
    assert_eq!(at(&rt), (1, 0));
    typed(&mut rt, "G");
    assert_eq!(at(&rt).0, 40);
    typed(&mut rt, "g");
    assert!(rt.state.key_wait.timed(), "g is Vim's too");
    typed(&mut rt, "g");
    assert_eq!((at(&rt), held(&rt)), ((1, 0), None), "gg is still Vim's");
    typed(&mut rt, "5gg");
    assert_eq!(at(&rt).0, 5, "with its count");
    typed(&mut rt, "ggfg");
    assert_eq!((at(&rt), held(&rt)), ((1, 15), None), "after f, g is the character to find");

    let density = rt.state.settings.density;
    typed(&mut rt, "3gd");
    assert_ne!(rt.state.settings.density, density, "g then d");
    typed(&mut rt, "j");
    assert_eq!(at(&rt).0, 2, "and the count went with it");

    typed(&mut rt, "gg");
    typed(&mut rt, "w");
    assert_eq!((at(&rt), held(&rt).as_deref()), ((1, 0), Some("w")), "w waits for x");
    let commits = rune::commit_generation();
    tap(&mut rt, "3");
    rt.frame(1.0 / 60.0);
    assert_eq!(at(&rt), (1, 3), "3 is no x: w moves");
    assert!(rune::commit_generation() > commits, "and the move is drawn, though 3 only waits");
    assert!(rt.live_only_warnings().is_empty(), "{:#?}", rt.live_only_warnings());
    press(&mut rt, Key::Escape, Mods::default());
    typed(&mut rt, "w");
    later(&mut rt, Duration::from_secs(61));
    assert_eq!((at(&rt), held(&rt)), ((1, 8), None), "the wait over, w moves");
}

#[test]
fn escape_drops_a_held_key_and_is_not_escape_too() {
    let mut rt = vim_rt(40);
    bind(&mut rt, &[("toggle-list", &["Ctrl+B", "q l"])]);
    typed(&mut rt, "jv");
    assert!(rt.state.active().visual().is_some());
    let open = list(&rt);
    typed(&mut rt, "q");
    press(&mut rt, Key::Escape, Mods::default());
    assert_eq!(held(&rt), None);
    assert!(rt.state.active().visual().is_some(), "Escape went to the held key only");
    typed(&mut rt, "l");
    assert_eq!(list(&rt), open, "l alone is Vim's l");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.active().visual().is_none(), "the next Escape is Escape");
}

#[test]
fn a_held_key_never_outlives_focus_in_a_text_field() {
    type Open = fn(&mut App, &mut rune::Tx);
    let fields: [(&str, Open); 3] = [
        ("find", |s, tx| {
            s.run(Command::Find, tx);
        }),
        ("composer", |s, tx| {
            s.run(Command::Comment, tx);
        }),
        ("picker", |s, tx| s.open_picker(tx)),
    ];
    for (what, open_it) in fields {
        for first in ["q", "x"] {
            let mut rt = vim_rt(40);
            bind(&mut rt, &[("toggle-list", &["x"]), ("toggle-density", &["q d", "x d"])]);
            typed(&mut rt, "j");
            let (open, density) = (list(&rt), rt.state.settings.density);
            typed(&mut rt, first);
            assert_eq!(held(&rt).as_deref(), Some(first), "{what}");
            rt.update(open_it);
            settle(&mut rt);
            assert!(rt.state.typing() || rt.state.picker_normal(), "{what}: the keys are the field's");
            assert_eq!(held(&rt), None, "{what}: {first} went when the field took the keys");
            typed(&mut rt, "d");
            later(&mut rt, Duration::from_secs(61));
            assert_eq!(
                (list(&rt), rt.state.settings.density),
                (open, density),
                "{what}: neither the shortcut nor {first} alone ran"
            );
        }
    }
}

#[test]
fn arrows_and_every_default_key_do_what_they_did_beside_a_two_key_shortcut() {
    let mut rt = vim_rt(40);
    bind(&mut rt, &[("toggle-density", &["q d", "Ctrl+Q Ctrl+D"])]);
    press(&mut rt, Key::Down, Mods::default());
    press(&mut rt, Key::Down, Mods::default());
    assert_eq!((at(&rt).0, held(&rt)), (2, None), "Down");
    let shift = Mods {
        shift: true,
        ..Mods::default()
    };
    press(&mut rt, Key::Down, shift);
    assert!(rt.state.active().visual().is_some(), "Shift+Down selects");
    assert_eq!(at(&rt).0, 3);
    press(&mut rt, Key::Escape, Mods::default());
    press(&mut rt, Key::Up, Mods::default());
    press(&mut rt, Key::Right, Mods::default());
    assert_eq!(at(&rt), (2, 1), "Up, Right");
    let open = list(&rt);
    press(&mut rt, Key::Command('b'), ctrl());
    assert_eq!(list(&rt), !open, "Ctrl+B");
    typed(&mut rt, "c");
    assert!(rt.state.composer.is_some(), "c comments at once");
    assert_eq!(held(&rt), None);
}

#[test]
fn two_key_settings_load_save_and_report_what_they_refuse() {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");
    std::fs::write(
        &path,
        r#"{"keymap": {"timeoutMs": 250, "overrides": {"toggle-list": ["Ctrl+B", "g l", "Up x", "q Escape", "a b c"], "zoom": ["g l"]}}}"#,
    )
    .unwrap();
    let mut a = App::new(
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
    assert!(a.status.contains("3 more"), "four entries refused: {}", a.status);
    assert_eq!(a.settings.keymap.timeout(), Duration::from_millis(250));
    let keys = |c: Command| rain_core::keymap::with_active(|k| k.chords(c).iter().map(|s| s.display()).collect::<Vec<_>>());
    assert_eq!(keys(Command::ToggleList), ["Ctrl+B", "g l"]);
    assert!(
        keys(Command::Zoom).is_empty(),
        "g l was taken; the entry still replaced Shift+Escape"
    );

    a.load_now();
    let mut rt = runtime(a);
    rt.update(|s, _| s.open_shortcuts());
    settle(&mut rt);
    let drawn = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("sc-toggle-list-key-1"))
        .and_then(|e| e.label);
    assert!(drawn.is_some_and(|l| l.ends_with("key g l")), "the screen shows both keys");
    rt.update(|s, _| {
        s.start_recording(Command::ToggleList);
        s.record_shortcut(rain_core::Chord::parse("Ctrl+M").unwrap());
        s.save_shortcuts();
    });
    let saved = crate::settings::Settings::load(rt.state.settings_path.as_ref().unwrap());
    let (ov, bad) = saved.keymap.overrides();
    assert!(bad.is_empty());
    assert_eq!(
        ov.get("toggle-list").map(Vec::as_slice),
        Some(&["Ctrl+B".to_owned(), "g l".to_owned(), "Ctrl+M".to_owned()][..]),
        "saved as written"
    );
    assert_eq!(saved.keymap.timeout_ms, Some(250));
}
