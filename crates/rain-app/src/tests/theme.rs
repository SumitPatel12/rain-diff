//! The chrome token layer: both styles' surfaces in both themes, the kits built from them, the
//! live switch, `--chrome` and the shell each style draws.

use rain_core::command::Command;
use rune::Color;

use super::*;
use crate::settings::Chrome;
use crate::theme::{TabStyle, look};

fn hex(c: Color) -> String {
    c.hex()[..7].to_owned()
}

/// WCAG contrast of two opaque colours.
fn contrast(a: Color, b: Color) -> f32 {
    let (la, lb) = (a.luminance(), b.luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[test]
fn every_chrome_and_theme_gives_the_design_table() {
    // ground, panel, reader, card_head, well, hairline, card_border, strong_line
    let table = [
        (
            Chrome::Zed,
            true,
            [
                "#2b2d32", "#232529", "#1c1d20", "#232529", "#18191c", "#3a3d44", "#464950", "#464950",
            ],
        ),
        (
            Chrome::Zed,
            false,
            [
                "#e3e4e7", "#eeeff1", "#fbfbfc", "#f1f2f4", "#f4f5f7", "#d4d6db", "#c4c6cb", "#c4c6cb",
            ],
        ),
        (
            Chrome::IslandsDeep,
            true,
            [
                "#0f1012", "#232529", "#232529", "#2a2c31", "#1e2023", "#2f3237", "#3a3d43", "#42454c",
            ],
        ),
        (
            Chrome::IslandsDeep,
            false,
            [
                "#e4e6ea", "#ffffff", "#ffffff", "#f5f6f8", "#f2f3f5", "#e8e9ec", "#d9dbe0", "#d0d3d8",
            ],
        ),
        (
            Chrome::Islands,
            true,
            [
                "#2e3034", "#1d1e21", "#1d1e21", "#25272b", "#191a1d", "#2c2e33", "#36383e", "#3b3e44",
            ],
        ),
        (
            Chrome::Islands,
            false,
            [
                "#e4e6ea", "#ffffff", "#ffffff", "#f5f6f8", "#f2f3f5", "#e8e9ec", "#d9dbe0", "#d0d3d8",
            ],
        ),
    ];
    for (chrome, dark, want) in table {
        let l = look(dark, chrome, Density::Compact);
        let s = l.s;
        let got = [
            s.ground,
            s.panel,
            s.reader,
            s.card_head,
            s.well,
            s.hairline,
            s.card_border,
            s.strong_line,
        ]
        .map(hex);
        assert_eq!(got, want, "{chrome:?} dark {dark}");
        assert_eq!((l.chrome, l.dark), (chrome, dark));
        assert_eq!(s.bar, s.ground);
        let c = &l.kit.c;
        assert_eq!(
            [c.page, c.bg_deep, c.bg_base, c.layer_01],
            [s.ground, s.ground, s.reader, s.card_head]
        );
        assert_eq!(
            [c.border_weak, c.border_base, c.border_strong],
            [s.hairline, s.card_border, s.strong_line]
        );
        assert!(s.selected_row.a > 0.0 && s.selected_row.a < 1.0, "selection is an accent tint");
        match chrome {
            Chrome::Zed => {
                assert_eq!(s.tab_active, s.reader, "Zed's active tab is the reader");
                assert_eq!((l.kit.radius.md, l.kit.radius.lg, l.kit.radius.xl), (4.0, 6.0, 8.0));
                assert_eq!(l.shell.radius.lg, 0.0, "no rounded regions");
                assert_eq!(l.shell.c.border_muted, s.hairline, "regions meet at hairlines");
                assert_eq!((l.shape.island_gap, l.shape.card_inset), (0.0, 0.0));
                assert_eq!(l.shape.file_frame, rain_diffview::FileFrame::Bar);
                assert_eq!(l.shape.tab_style, TabStyle::Merge);
                assert!(l.shape.region_lines && !l.shape.row_pill);
                let line = contrast(s.hairline, s.panel);
                assert!(line >= 1.25, "a hairline reads against the panel: {line:.3} (dark {dark})");
            }
            Chrome::Islands | Chrome::IslandsDeep => {
                assert_ne!(s.tab_active, s.reader);
                assert_eq!((l.kit.radius.md, l.kit.radius.lg, l.kit.radius.xl), (6.0, 8.0, 10.0));
                assert_eq!(l.shell.c.border_muted.a, 0.0, "islands have no border");
                assert!(!l.shape.region_lines && l.shape.row_pill);
                assert_eq!(l.shape.tab_style, TabStyle::Underline);
                let ratio = contrast(s.ground, s.panel);
                assert!(ratio >= 1.20, "ground vs island {ratio:.3} (dark {dark})");
                assert_eq!(contrast(s.ground, s.reader), ratio);
                let ground_darker = s.ground.luminance() < s.panel.luminance();
                let want = !dark || chrome == Chrome::IslandsDeep;
                assert_eq!(
                    ground_darker, want,
                    "{chrome:?}: which side of the islands the ground is (dark {dark})"
                );
            }
        }
    }
}

#[test]
fn islands_gaps_and_radii_follow_density_and_cards_come_in_on_narrow_windows() {
    let roomy = look(true, Chrome::Islands, Density::Comfortable).shape;
    let tight = look(true, Chrome::Islands, Density::Compact).shape;
    assert_eq!((roomy.island_gap, roomy.island_radius), (8.0, 10.0));
    assert_eq!((tight.island_gap, tight.island_radius), (4.0, 8.0));
    assert_eq!(look(true, Chrome::Islands, Density::Compact).shell.radius.lg, 8.0);
    let card = |w: f32| match tight.frame(w) {
        rain_diffview::FileFrame::Card { inset, gap, .. } => (inset, gap),
        f => panic!("{f:?}"),
    };
    assert_eq!(card(1920.0), (12.0, 12.0));
    assert_eq!(card(1366.0), (8.0, 12.0));
    assert_eq!(
        look(false, Chrome::Zed, Density::Comfortable).shape.frame(1366.0),
        rain_diffview::FileFrame::Bar
    );
}

#[test]
fn toggle_chrome_flips_live_and_saves() {
    let mut rt = sized_runtime(app(story(), true), (1920.0, 1080.0));
    assert_eq!(rt.state.chrome(), Chrome::Zed);
    assert!(rt.find("side-panel").is_some() && rt.find("side-island").is_none());
    let path = rt.state.settings_path.clone().unwrap();
    rt.update(|s, tx| {
        s.run(Command::ToggleChrome, tx);
    });
    settle(&mut rt);
    assert_eq!(rt.state.chrome(), Chrome::Islands);
    assert_eq!(Settings::load(&path).chrome, Chrome::Islands, "saved");
    assert!(rt.find("side-island").is_some(), "the next frame draws islands");
    let reader = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("main-pane")).unwrap();
    assert!(reader.radius > 0.0, "the reader is a rounded island");
    assert!(reader.rect[0] > 0.0 && reader.rect[0] < 1920.0);
    rt.update(|s, tx| {
        s.run(Command::ToggleChrome, tx);
    });
    settle(&mut rt);
    assert_eq!(Settings::load(&path).chrome, Chrome::Zed);
    let reader = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("main-pane")).unwrap();
    assert_eq!(reader.radius, 0.0, "Zed's reader is square");
    assert!(Command::PALETTE_ONLY.contains(&Command::ToggleChrome) && Command::ToggleChrome.chord().is_none());
}

#[test]
fn the_more_menu_switches_chrome_theme_and_density() {
    let mut rt = sized_runtime(app(story(), true), (1920.0, 1080.0));
    let tap = |rt: &mut Runtime<App>, at: (f32, f32)| {
        rt.pointer_move(at.0, at.1);
        rt.pointer_down(at.0, at.1);
        rt.pointer_up(at.0, at.1);
        settle(rt);
    };
    let more = rt.find("more-menu").expect("the top bar's ⋯");
    tap(&mut rt, more);
    assert!(rt.state.menus.more, "⋯ opens its menu");
    let item = |rt: &Runtime<App>, label: &str| {
        let e = rt
            .elements()
            .into_iter()
            .find(|e| !e.exiting && e.role.starts_with("menuitem") && e.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("no menu item {label}"));
        (e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0)
    };
    let chrome = item(&rt, "Chrome");
    tap(&mut rt, chrome);
    let islands = item(&rt, "Islands");
    tap(&mut rt, islands);
    assert_eq!(rt.state.chrome(), Chrome::Islands);
    assert!(!rt.state.menus.more, "a pick closes the menu");
    rt.update(|s, _| {
        s.menus.more = true;
        s.menus.more_sub = Some(crate::app::MoreSub::Theme);
    });
    settle(&mut rt);
    let light = item(&rt, "Light");
    tap(&mut rt, light);
    assert_eq!(rt.state.settings.theme, Theme::Light);
    rt.update(|s, _| {
        s.menus.more = true;
        s.menus.more_sub = Some(crate::app::MoreSub::Density);
    });
    settle(&mut rt);
    let roomy = item(&rt, "Comfortable");
    tap(&mut rt, roomy);
    assert_eq!(rt.state.settings.density, Density::Comfortable);
}

#[test]
fn chrome_flag_sets_the_run_and_scenes_without_saving() {
    let l = launch_from_args(&["--chrome".into(), "islands".into(), "--fixture".into(), "x.patch".into()]);
    assert_eq!(l.chrome.as_deref(), Some("islands"));
    assert_eq!(launch_from_args(&[]).chrome, None);
    let argv = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<String>>();
    let process = argv(&["rain.exe", "--snap", "a.png", "--chrome", "islands", "--scene", "files"]);
    assert_eq!(crate::chrome_arg(process.clone()).as_deref(), Some("islands"));
    assert_eq!(crate::chrome_arg(argv(&["rain.exe", "--chrome", "fluent"])), None);
    assert_eq!(
        crate::rune_argv(process.into_iter().skip(1)),
        argv(&["--snap", "a.png", "--scene", "files"]),
        "Rune never sees Rain's --chrome"
    );

    let mut a = app_with(Launch {
        session: "t".into(),
        fixture: Some(story()),
        chrome: Some("islands".into()),
        ..Launch::default()
    });
    assert_eq!(a.settings.chrome, Chrome::Zed, "the saved setting");
    assert_eq!(a.chrome(), Chrome::Islands, "this run");
    let path = a.settings_path.clone().unwrap();
    a.save_settings();
    assert_eq!(Settings::load(&path).chrome, Chrome::Zed, "the override is never saved");

    shell_scene("files")(&mut a);
    assert_eq!(a.chrome(), Chrome::Islands);
    assert_eq!(a.settings.chrome, Chrome::Islands);
    let rt = sized_runtime(a, (1366.0, 768.0));
    assert!(rt.find("side-island").is_some());

    let mut z = app(story(), true);
    z.launch.chrome = Some("islands".into());
    shell_scene("chrome-zed")(&mut z);
    assert_eq!(z.chrome(), Chrome::Zed, "the scene names its chrome");
    shell_scene("chrome-islands")(&mut z);
    assert_eq!(z.chrome(), Chrome::Islands);
    assert!(z.settings_path.is_none());
}

#[test]
fn zed_draws_hairlines_and_islands_gaps() {
    for dark in [true, false] {
        let mut a = app(story(), true);
        shell_scene("chrome-zed")(&mut a);
        let rt = Runtime::new(a, view, (1920.0, 1080.0), Env::new(dark, 1.0));
        let mut rt = rt;
        rt.frame(0.0);
        settle(&mut rt);
        let l = look(dark, Chrome::Zed, Density::Compact);
        let main = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("main-pane")).unwrap();
        let side = rt.find("side-panel").unwrap();
        assert_eq!(main.radius, 0.0);
        assert!(main.rect[0] >= side.0, "the reader meets the list edge to edge");
        let line = |rt: &Runtime<App>, key: &str| {
            rt.elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key))
                .map(|e| (e.rect, e.fill))
        };
        let (side_line, fill) = line(&rt, "reader-side-line").expect("a hairline between list and reader");
        assert_eq!(fill, l.s.hairline);
        assert_eq!(side_line[2], 1.0);
        assert!(side_line[3] > 500.0, "full height: {side_line:?}");
        let (top, _) = line(&rt, "reader-top-line").expect("the bar's line over the reader");
        assert!(top[2] > 1000.0, "across the reader: {top:?}");
        let (status, fill) = line(&rt, "status-line").expect("a hairline over the status bar");
        assert_eq!(fill, l.s.hairline);
        assert!(
            status[2] >= 1919.0 && status[1] > 1000.0,
            "the full width at the bottom: {status:?}"
        );
        assert!(
            rt.elements()
                .iter()
                .any(|e| e.key.as_deref() == Some("tab-strip") && e.fill == l.s.panel)
        );

        let mut a = app(story(), true);
        shell_scene("chrome-islands")(&mut a);
        let mut rt = Runtime::new(a, view, (1920.0, 1080.0), Env::new(dark, 1.0));
        rt.frame(0.0);
        settle(&mut rt);
        let l = look(dark, Chrome::Islands, Density::Compact);
        let island = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("side-panel")).unwrap();
        let main = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("main-pane")).unwrap();
        assert_eq!(island.radius, 8.0);
        assert_eq!(main.radius, 8.0);
        let gap = main.rect[0] - (island.rect[0] + island.rect[2]);
        assert_eq!(gap, l.shape.island_gap, "a gap of ground between the islands");
        assert_eq!(island.rect[0], l.shape.island_gap);
        let app = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("app")).unwrap();
        assert_eq!(app.fill, l.s.ground);
        let cards: Vec<_> = rt.elements().into_iter().filter(|e| e.key.as_deref() == Some("card")).collect();
        assert!(!cards.is_empty(), "file cards");
        assert!(
            cards.iter().all(|c| c.fill == l.s.reader),
            "inside an island a card has no outline, only its head band and the gap"
        );
        for key in ["reader-side-line", "reader-top-line", "status-line"] {
            assert!(rt.find(key).is_none(), "islands draw no {key}");
        }
    }
}

#[test]
fn islands_dark_ground_is_a_third_style_on_its_own_and_the_toggle_leaves_islands_for_zed() {
    assert_eq!(Chrome::parse("islands-deep"), Some(Chrome::IslandsDeep));
    let text = serde_json::to_string(&Chrome::IslandsDeep).unwrap();
    assert_eq!(text, "\"islands-deep\"");
    let mut rt = sized_runtime(app(story(), true), (1920.0, 1080.0));
    rt.update(|s, _| s.set_chrome(Chrome::IslandsDeep));
    settle(&mut rt);
    let path = rt.state.settings_path.clone().unwrap();
    assert_eq!(Settings::load(&path).chrome, Chrome::IslandsDeep, "saved");
    assert!(rt.find("side-island").is_some(), "drawn as islands");
    rt.update(|s, tx| {
        s.run(Command::ToggleChrome, tx);
    });
    assert_eq!(rt.state.chrome(), Chrome::Zed);
}
