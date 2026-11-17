//! The token layer: the roles of every scheme, the kits built from them, the islands the shell
//! draws and the `⋯` menu's Appearance picks.

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

/// The redesign's role set (round 1b `kit.css`), which Everforest paints with.
const D1: [&str; 44] = [
    "#13171a", "#21282c", "#1b2023", "#242b2f", "#283035", "#161a1d", "#2c3438", "#384246", "#4f585e", "#2d3a2f", "#d3c6aa", "#9da9a0",
    "#737d78", "#7fbbb3", "#a7c080", "#1a2016", "#283126", "#45533a", "#dbbc7f", "#332e23", "#5a4e34", "#e67e80", "#37292b", "#643f42",
    "#22302e", "#3b5450", "#d699b6", "#2a2330", "#46374f", "#212a20", "#2d2325", "#384a2e", "#553538", "#171b1d", "#252d30", "#543a48",
    "#5a4e2a", "#8a7438", "#e69875", "#83c092", "#859289", "#a59c86", "#060809", "#000000",
];

/// morhetz/gruvbox's dark backgrounds, light foregrounds and bright accents.
const GRUVBOX: [&str; 20] = [
    "#1d2021", "#282828", "#32302f", "#3c3836", "#504945", "#665c54", "#7c6f64", "#928374", "#ebdbb2", "#d5c4a1", "#bdae93", "#a89984",
    "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#fe8019", "#fbf1c7",
];

#[test]
fn everforest_is_the_redesign_role_set() {
    let l = look(Scheme::Everforest);
    let (s, r) = (l.s, l.r);
    assert_eq!(
        [
            s.bar,
            s.panel,
            s.reader,
            s.card_head,
            s.well,
            s.hairline,
            s.card_border,
            s.strong_line,
            s.selected_row
        ]
        .map(hex),
        [
            "#13171a", "#21282c", "#1b2023", "#242b2f", "#161a1d", "#2c3438", "#384246", "#4f585e", "#2d3a2f"
        ]
    );
    assert_eq!(
        [r.raised, r.focus, r.must, r.accent, r.on_accent, r.note, r.note_bg, r.note_border].map(hex),
        [
            "#283035", "#a7c080", "#e67e80", "#a7c080", "#1a2016", "#d699b6", "#2a2330", "#46374f"
        ]
    );
    assert_eq!(
        [
            r.add_bg,
            r.del_bg,
            r.add_word,
            r.del_word,
            r.filler,
            r.cursor_line,
            r.selection,
            r.find_hit,
            r.find_current
        ]
        .map(hex),
        [
            "#212a20", "#2d2325", "#384a2e", "#553538", "#171b1d", "#252d30", "#543a48", "#5a4e2a", "#8a7438"
        ]
    );
    let c = &l.kit.c;
    assert_eq!(
        [c.text_base, c.text_muted, c.text_faint, c.icon_faint, c.text_accent, c.bg_accent].map(hex),
        ["#d3c6aa", "#9da9a0", "#9da9a0", "#737d78", "#7fbbb3", "#a7c080"]
    );
    assert_eq!(
        [c.success, c.warning, c.danger, c.info].map(|st| [st.fg, st.bg, st.border].map(hex)),
        [
            ["#a7c080", "#283126", "#45533a"],
            ["#dbbc7f", "#332e23", "#5a4e34"],
            ["#e67e80", "#37292b", "#643f42"],
            ["#7fbbb3", "#22302e", "#3b5450"],
        ]
    );
    for t in rain_syntax::Token::ALL {
        let h = hex(Scheme::Everforest.syntax().color(t));
        assert!(D1.contains(&h.as_str()) || h == "#d3c6aa", "{t:?} {h}");
    }
}

#[test]
fn every_scheme_gives_every_role_and_reads() {
    for scheme in Scheme::ALL {
        let l = look(scheme);
        let s = l.s;
        assert_eq!((l.scheme, l.dark), (scheme, scheme != Scheme::Light));
        assert_eq!(l.kit.dark, l.dark);
        assert_eq!(scheme_of(&l.kit), scheme, "a kit tells its scheme");
        let c = &l.kit.c;
        assert_eq!([c.page, c.bg_deep, c.bg_base, c.layer_01], [s.bar, s.bar, s.reader, s.card_head]);
        assert_eq!(
            [c.border_weak, c.border_base, c.border_strong],
            [s.hairline, s.card_border, s.strong_line]
        );
        assert_eq!(
            (l.kit.radius.xs, l.kit.radius.sm, l.kit.radius.md, l.kit.radius.lg, l.kit.radius.xl),
            (2.0, 3.0, 4.0, 6.0, 8.0)
        );
        let lift = contrast(s.panel, s.bar);
        assert!(lift >= 1.1, "islands lift off the ground: {lift:.3} ({scheme:?})");
        for (name, surface) in [
            ("ground", s.bar),
            ("panel", s.panel),
            ("reader", s.reader),
            ("card head", s.card_head),
            ("well", s.well),
        ] {
            let text = contrast(c.text_base, surface);
            assert!(text >= 4.5, "body text on the {name}: {text:.2} ({scheme:?})");
        }
        let r = l.r;
        for (name, fill) in [("added", r.add_bg), ("removed", r.del_bg)] {
            let text = contrast(c.text_base, fill);
            assert!(text >= 4.5, "code on {name} lines: {text:.2} ({scheme:?})");
        }
    }
}

#[test]
fn secondary_text_reads_on_every_ground_and_row_state() {
    use rune::theme::{srgb_decode, srgb_encode};
    // Windows blend in sRGB, as a browser does.
    let on = |ink: Color, under: Color| {
        let m = |b: f32, t: f32| srgb_decode(srgb_encode(b) + (srgb_encode(t) - srgb_encode(b)) * ink.a);
        Color {
            r: m(under.r, ink.r),
            g: m(under.g, ink.g),
            b: m(under.b, ink.b),
            a: 1.0,
        }
    };
    let mut low = Vec::new();
    for scheme in Scheme::ALL {
        let l = look(scheme);
        let (c, s, r) = (&l.kit.c, l.s, l.r);
        let grounds = [
            ("ground", s.bar),
            ("panel", s.panel),
            ("reader", s.reader),
            ("card head", s.card_head),
            ("well", s.well),
            ("raised", r.raised),
            ("selected row", on(s.selected_row, s.panel)),
            ("hovered row", on(c.hover, s.panel)),
            ("hovered menu row", on(c.hover, r.raised)),
            ("added line", r.add_bg),
            ("removed line", r.del_bg),
            ("note", r.note_bg),
            ("cursor line", r.cursor_line),
        ];
        for (name, g) in grounds {
            for (level, ink) in [("muted", c.text_muted), ("faint", c.text_faint)] {
                let ratio = contrast(on(ink, g), g);
                println!("{scheme:?} {level} on {name}: {ratio:.2}");
                if ratio < 4.5 {
                    low.push(format!("{scheme:?} {level} on the {name}: {ratio:.2}"));
                }
            }
        }
        let mark = contrast(on(c.icon_faint, s.panel), s.panel);
        let muted = contrast(on(c.text_muted, s.panel), s.panel);
        println!("{scheme:?} faint mark on panel: {mark:.2}, muted {muted:.2}");
        assert!(mark < muted, "faint marks stay quieter than muted text ({scheme:?})");
    }
    assert!(low.is_empty(), "secondary text under 4.5:1: {low:#?}");
}

#[test]
fn palette_schemes_paint_only_their_palettes_hexes() {
    for (scheme, palette) in [
        (Scheme::Everforest, &D1[..]),
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
        for col in opaque {
            assert!(
                palette.contains(&hex(col).as_str()),
                "{scheme:?} paints {} from outside its palette",
                hex(col)
            );
        }
        assert!(l.kit.ring.is_some(), "rings follow the palette's border");
    }
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
            s.menus.more_sub = Some(crate::app::MoreSub::Appearance);
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
    let appearance = item(&rt, "Appearance");
    tap(&mut rt, appearance);
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
        s.menus.more_sub = Some(crate::app::MoreSub::Appearance);
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
fn the_shell_draws_islands_on_the_ground() {
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
        let el = |rt: &Runtime<App>, key: &str| {
            rt.elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key) && !e.exiting)
                .unwrap_or_else(|| panic!("no {key}"))
        };
        let app = el(&rt, "app");
        assert_eq!(app.fill, l.s.bar, "{scheme:?}");
        let (list, reader) = (el(&rt, "side-panel"), el(&rt, "main"));
        assert_eq!((list.radius, reader.radius), (10.0, 10.0), "rounded islands");
        assert_eq!(list.fill, l.s.panel);
        assert_eq!(reader.fill, l.s.reader);
        let gap = reader.rect[0] - (list.rect[0] + list.rect[2]);
        assert_eq!(gap, 8.0, "an 8 point gap, which resizes the list");
        assert_eq!(list.rect[0], 8.0, "the ground shows around the islands");
        let top = el(&rt, "toolbar");
        assert_eq!(top.rect[3], 44.0);
        let status = el(&rt, "status-line");
        assert_eq!(status.rect[3], 30.0);
        assert_eq!(status.fill, l.s.bar);
        assert!(
            el(&rt, "tab-strip").fill == l.s.well,
            "the Overview/Changes switch sits on the well"
        );
        assert!(
            rt.elements().iter().any(|e| e.fill == l.r.add_bg),
            "added lines in {scheme:?}'s own fill"
        );
    }
}

#[test]
fn dragging_the_gap_resizes_the_list_within_its_range() {
    let mut rt = sized_runtime(app(story(), true), (1600.0, 1000.0));
    let grip = rt.find("list-grip").expect("the gap between the islands");
    let drag = |rt: &mut Runtime<App>, from: (f32, f32), dx: f32| {
        rt.pointer_move(from.0, from.1);
        rt.pointer_down(from.0, from.1);
        rt.pointer_move(from.0 + dx / 2.0, from.1);
        rt.pointer_move(from.0 + dx, from.1);
        rt.pointer_up(from.0 + dx, from.1);
        settle(rt);
    };
    let before = rt.state.sidebar.width();
    drag(&mut rt, grip, 60.0);
    assert_eq!(rt.state.sidebar.width(), before + 60.0);
    let grip = rt.find("list-grip").unwrap();
    drag(&mut rt, grip, 600.0);
    assert_eq!(rt.state.sidebar.width(), 520.0, "at most 520");
    let grip = rt.find("list-grip").unwrap();
    drag(&mut rt, grip, -280.0);
    assert_eq!(rt.state.sidebar.width(), 240.0);
    let grip = rt.find("list-grip").unwrap();
    drag(&mut rt, grip, -30.0);
    assert_eq!(rt.state.sidebar.width(), 220.0, "at least 220");
}
