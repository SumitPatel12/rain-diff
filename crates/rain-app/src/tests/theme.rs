//! The token layer: the surfaces of every scheme, the kits built from them, the shell they
//! draw and the `⋯` menu's Appearance picks.

use rain_core::command::Command;
use rune::Color;

use super::*;
use crate::theme::{Scheme, look, scheme_of};

fn hex(c: Color) -> String {
    c.hex()[..7].to_owned()
}

/// WCAG contrast of two opaque colours.
fn contrast(a: Color, b: Color) -> f32 {
    let (la, lb) = (a.luminance(), b.luminance());
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Rain's Everforest dark: every hex of the medium palette, and the hard variant's `bg_dim`.
const EVERFOREST: [&str; 24] = [
    "#1e2326", "#232a2e", "#2d353b", "#343f44", "#3d484d", "#475258", "#4f585e", "#56635f", "#543a48", "#514045", "#425047", "#3a515d",
    "#4d4c43", "#d3c6aa", "#e67e80", "#e69875", "#dbbc7f", "#a7c080", "#83c092", "#7fbbb3", "#d699b6", "#7a8478", "#859289", "#9da9a0",
];

/// morhetz/gruvbox's dark backgrounds, light foregrounds and bright accents.
const GRUVBOX: [&str; 20] = [
    "#1d2021", "#282828", "#32302f", "#3c3836", "#504945", "#665c54", "#7c6f64", "#928374", "#ebdbb2", "#d5c4a1", "#bdae93", "#a89984",
    "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#fe8019", "#fbf1c7",
];

#[test]
fn every_scheme_gives_the_design_table() {
    // bar, panel, reader, card_head, well, hairline, card_border, strong_line
    let table = [
        (
            Scheme::Dark,
            [
                "#2b2d32", "#232529", "#1c1d20", "#232529", "#18191c", "#3a3d44", "#464950", "#464950",
            ],
        ),
        (
            Scheme::Light,
            [
                "#e3e4e7", "#eeeff1", "#fbfbfc", "#f1f2f4", "#f4f5f7", "#d4d6db", "#c4c6cb", "#c4c6cb",
            ],
        ),
        (
            Scheme::Everforest,
            [
                "#1e2326", "#343f44", "#2d353b", "#343f44", "#2d353b", "#475258", "#4f585e", "#4f585e",
            ],
        ),
        (
            Scheme::GruvboxHard,
            [
                "#32302f", "#282828", "#1d2021", "#282828", "#1d2021", "#504945", "#665c54", "#7c6f64",
            ],
        ),
        (
            Scheme::GruvboxMedium,
            [
                "#3c3836", "#32302f", "#282828", "#32302f", "#1d2021", "#504945", "#665c54", "#7c6f64",
            ],
        ),
    ];
    assert_eq!(table.map(|(s, _)| s), Scheme::ALL, "one row per scheme");
    for (scheme, want) in table {
        let l = look(scheme);
        let s = l.s;
        let got = [
            s.bar,
            s.panel,
            s.reader,
            s.card_head,
            s.well,
            s.hairline,
            s.card_border,
            s.strong_line,
        ]
        .map(hex);
        assert_eq!(got, want, "{scheme:?}");
        assert_eq!((l.scheme, l.dark), (scheme, scheme != Scheme::Light));
        assert_eq!(l.kit.dark, l.dark);
        assert_eq!(scheme_of(&l.kit), scheme, "a kit tells its scheme");
        let c = &l.kit.c;
        assert_eq!([c.page, c.bg_deep, c.bg_base, c.layer_01], [s.bar, s.bar, s.reader, s.card_head]);
        assert_eq!(
            [c.border_weak, c.border_base, c.border_strong],
            [s.hairline, s.card_border, s.strong_line]
        );
        assert!(s.selected_row.a > 0.0 && s.selected_row.a < 1.0, "selection is an accent tint");
        assert_eq!((l.kit.radius.md, l.kit.radius.lg, l.kit.radius.xl), (4.0, 6.0, 8.0));
        assert_eq!(l.shell.radius.lg, 0.0, "no rounded regions");
        assert_eq!(l.shell.c.border_muted, s.hairline, "regions meet at hairlines");
        let line = contrast(s.hairline, s.panel);
        assert!(line >= 1.25, "a hairline reads against the panel: {line:.3} ({scheme:?})");
        for (name, surface) in [
            ("bar", s.bar),
            ("panel", s.panel),
            ("reader", s.reader),
            ("card head", s.card_head),
            ("well", s.well),
        ] {
            let text = contrast(c.text_base, surface);
            assert!(text >= 4.5, "body text on the {name}: {text:.2} ({scheme:?})");
        }
    }
}

#[test]
fn palette_schemes_paint_only_their_palettes_hexes() {
    for (scheme, palette) in [
        (Scheme::Everforest, &EVERFOREST[..]),
        (Scheme::GruvboxHard, &GRUVBOX[..]),
        (Scheme::GruvboxMedium, &GRUVBOX[..]),
    ] {
        let l = look(scheme);
        let c = &l.kit.c;
        let mut opaque = vec![
            c.page,
            c.bg_base,
            c.layer_01,
            c.layer_02,
            c.layer_03,
            c.layer_04,
            c.bg_accent,
            c.bg_button_neutral,
            c.text_base,
            c.text_muted,
            c.text_faint,
            c.text_accent,
            c.text_inverse,
            c.icon_base,
            c.icon_muted,
            c.icon_accent,
            c.border_muted,
            c.border_focus,
            c.submit_fg,
            c.surface_raised,
            c.std_border,
        ];
        for st in [c.success, c.warning, c.danger, c.info] {
            opaque.extend([st.fg, st.icon, st.border]);
            if st.bg.a == 1.0 {
                opaque.push(st.bg);
            }
        }
        let s = l.s;
        opaque.extend([
            s.bar,
            s.panel,
            s.reader,
            s.card_head,
            s.well,
            s.hairline,
            s.card_border,
            s.strong_line,
        ]);
        let d = scheme.diagram();
        opaque.extend([
            d.background,
            d.card,
            d.card_border,
            d.foreground,
            d.muted,
            d.added,
            d.modified,
            d.removed,
            d.thread,
        ]);
        if let Some((add, del)) = scheme.change_fills() {
            opaque.extend([add, del]);
        }
        opaque.extend(scheme.selection());
        opaque.push(scheme.note_purples().0);
        for t in rain_syntax::Token::ALL {
            opaque.push(scheme.syntax().color(t));
        }
        for col in opaque {
            assert!(
                palette.contains(&hex(col).as_str()),
                "{scheme:?} paints {} from outside its palette",
                hex(col)
            );
        }
        assert!(l.kit.ring.is_some(), "rings follow the palette's border");
    }
    let ef = look(Scheme::Everforest);
    assert_eq!(hex(ef.kit.c.success.bg), "#425047", "Everforest's own tints");
    assert_eq!(
        Scheme::Everforest.change_fills().map(|(a, d)| (hex(a), hex(d))),
        Some(("#425047".into(), "#514045".into()))
    );
    assert_eq!(Scheme::Everforest.selection().map(hex).as_deref(), Some("#543a48"));
    assert_eq!(Scheme::Dark.change_fills(), None, "dark and light keep the kit's tints");
}

#[test]
fn the_more_menu_switches_theme_and_density() {
    let mut rt = sized_runtime(app(story(), true), (1920.0, 1080.0));
    let tap = |rt: &mut Runtime<App>, at: (f32, f32)| {
        rt.pointer_move(at.0, at.1);
        rt.pointer_down(at.0, at.1);
        rt.pointer_up(at.0, at.1);
        settle(rt);
    };
    let open_themes = |rt: &mut Runtime<App>| {
        rt.update(|s, _| {
            s.menus.more = true;
            s.menus.more_sub = Some(crate::app::MoreSub::Theme);
        });
        settle(rt);
    };
    let more = rt.find("more-menu").expect("the top bar's ⋯");
    tap(&mut rt, more);
    assert!(rt.state.menus.more, "⋯ opens its menu");
    let items = |rt: &Runtime<App>, label: &str| {
        rt.elements()
            .into_iter()
            .filter(|e| !e.exiting && e.role.starts_with("menuitem") && e.label.as_deref() == Some(label))
            .map(|e| (e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0))
            .collect::<Vec<_>>()
    };
    let item = |rt: &Runtime<App>, label: &str| *items(rt, label).first().unwrap_or_else(|| panic!("no menu item {label}"));
    assert!(items(&rt, "Chrome").is_empty(), "Appearance has no chrome pick");
    let theme = item(&rt, "Theme");
    tap(&mut rt, theme);
    for label in ["System", "Light", "Dark", "Everforest", "Gruvbox Dark Hard", "Gruvbox Dark Medium"] {
        assert_eq!(items(&rt, label).len(), 1, "the Theme menu lists {label}");
    }
    let light = item(&rt, "Light");
    tap(&mut rt, light);
    assert_eq!(rt.state.settings.theme, Theme::Light);
    assert!(!rt.state.menus.more, "a pick closes the menu");
    for (label, theme, scheme) in [
        ("Everforest", Theme::Everforest, Scheme::Everforest),
        ("Gruvbox Dark Hard", Theme::GruvboxHard, Scheme::GruvboxHard),
        ("Gruvbox Dark Medium", Theme::GruvboxMedium, Scheme::GruvboxMedium),
        ("Dark", Theme::Dark, Scheme::Dark),
    ] {
        open_themes(&mut rt);
        let at = item(&rt, label);
        tap(&mut rt, at);
        assert_eq!(rt.state.settings.theme, theme);
        assert_eq!(rt.state.seen_scheme.get(), scheme, "{label} applies live");
        let app = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("app")).unwrap();
        assert_eq!(app.fill, look(scheme).s.bar, "{label} repaints the window");
    }
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
fn toggle_theme_and_the_palette_reach_every_scheme() {
    let mut rt = sized_runtime(app(story(), true), (1600.0, 1000.0));
    rt.update(|s, _| s.set_theme(Theme::Everforest));
    settle(&mut rt);
    assert_eq!(rt.state.seen_scheme.get(), Scheme::Everforest);
    rt.update(|s, tx| {
        let _ = s.run(Command::ToggleTheme, tx);
    });
    settle(&mut rt);
    assert_eq!(rt.state.settings.theme, Theme::Light, "from any dark scheme the toggle goes light");
    rt.update(|s, tx| {
        let _ = s.run(Command::ToggleTheme, tx);
    });
    settle(&mut rt);
    assert_eq!(rt.state.settings.theme, Theme::Dark, "and back to dark");
    let rows = crate::palette::filtered(&rt.state, "theme: gruvbox dark hard");
    assert_eq!(
        rows.first().map(|e| e.item.clone()),
        Some(crate::palette::Item::Theme(Theme::GruvboxHard))
    );
    rt.update(|s, tx| {
        s.open_palette();
        s.palette.edit.text = "theme: gruvbox dark hard".into();
        crate::palette::run_selected(s, tx);
    });
    settle(&mut rt);
    assert_eq!(rt.state.seen_scheme.get(), Scheme::GruvboxHard, "a palette pick applies live");
}

#[test]
fn the_theme_flag_picks_rains_own_schemes_only() {
    let args = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        crate::theme_in(args(&["rain", "--theme", "everforest", "--snap", "a.png"])),
        Some(Theme::Everforest)
    );
    assert_eq!(
        crate::theme_in(args(&["rain", "--theme", "gruvbox-medium"])),
        Some(Theme::GruvboxMedium)
    );
    assert_eq!(crate::theme_in(args(&["rain", "--theme", "dark"])), None);
    assert_eq!(crate::theme_in(args(&["rain", "--theme"])), None);
    assert_eq!(crate::theme_in(args(&["rain"])), None);
}

#[test]
fn the_shell_draws_hairlines() {
    for scheme in Scheme::ALL {
        let mut a = app(story(), true);
        shell_scene("files")(&mut a);
        a.settings.theme = match scheme {
            Scheme::Dark => Theme::Dark,
            Scheme::Light => Theme::Light,
            Scheme::Everforest => Theme::Everforest,
            Scheme::GruvboxHard => Theme::GruvboxHard,
            Scheme::GruvboxMedium => Theme::GruvboxMedium,
        };
        let mut rt = Runtime::new(a, view, (1920.0, 1080.0), Env::new(scheme.is_dark(), 1.0));
        rt.frame(0.0);
        settle(&mut rt);
        let l = look(scheme);
        let main = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("main-pane")).unwrap();
        let side = rt.find("side-panel").unwrap();
        assert_eq!(main.radius, 0.0);
        assert!(main.rect[0] >= side.0, "the reader meets the list edge to edge");
        let app = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("app")).unwrap();
        assert_eq!(app.fill, l.s.bar, "{scheme:?}");
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
        if let Some((add, _)) = scheme.change_fills() {
            assert!(rt.elements().iter().any(|e| e.fill == add), "added lines in {scheme:?}'s own fill");
        }
    }
}
