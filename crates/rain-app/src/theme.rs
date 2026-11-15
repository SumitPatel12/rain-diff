//! The token layer: for a scheme, the surfaces regions paint and the kits every view and rune-kit
//! component builds with. Flat panels meet at hairlines.
//!
//! Dark and light are Rune's neutral kit on Zed-like surfaces. Everforest and the two Gruvbox
//! darks repaint the kit from their own palettes: Everforest from Rain's variant (the medium
//! dark palette on the hard variant's `bg_dim`), Gruvbox from morhetz/gruvbox's `gruvbox.vim`.

use rain_diagram::DiagramStyle;
use rune::Color;
use rune_kit::{Kit, Radius, StateColors};

/// A colour scheme the window can draw in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scheme {
    Dark,
    Light,
    Everforest,
    GruvboxHard,
    GruvboxMedium,
}

impl Scheme {
    pub const ALL: [Scheme; 5] = [
        Scheme::Dark,
        Scheme::Light,
        Scheme::Everforest,
        Scheme::GruvboxHard,
        Scheme::GruvboxMedium,
    ];

    pub fn is_dark(self) -> bool {
        self != Scheme::Light
    }

    /// The code colours that go with the scheme.
    pub fn syntax(self) -> rain_syntax::Palette {
        match self {
            Scheme::Dark => rain_syntax::Palette::GithubDark,
            Scheme::Light => rain_syntax::Palette::GithubLight,
            Scheme::Everforest => rain_syntax::Palette::Everforest,
            Scheme::GruvboxHard | Scheme::GruvboxMedium => rain_syntax::Palette::Gruvbox,
        }
    }

    /// Colours by token kind for Markdown code blocks: the kit's own in dark and light, the
    /// palette's in the others.
    pub fn code_kinds(self, k: &Kit) -> rune_kit::syntax::SyntaxTheme {
        let base = rune_kit::syntax::SyntaxTheme::from_kit(k);
        match self {
            Scheme::Dark | Scheme::Light => base,
            _ => self.syntax().syntax_theme(base),
        }
    }

    /// The Map's colours.
    pub fn diagram(self) -> DiagramStyle {
        match self {
            Scheme::Dark => DiagramStyle::dark(),
            Scheme::Light => DiagramStyle::light(),
            Scheme::Everforest => DiagramStyle::everforest(),
            Scheme::GruvboxHard => DiagramStyle::gruvbox(true),
            Scheme::GruvboxMedium => DiagramStyle::gruvbox(false),
        }
    }

    /// Added and removed lines' fills where the palette has its own (Everforest's `bg-green`
    /// and `bg-red`); `None` keeps the diff's tints of the kit's success and danger colours.
    pub fn change_fills(self) -> Option<(Color, Color)> {
        (self == Scheme::Everforest).then(|| (hex(ef::BG_GREEN), hex(ef::BG_RED)))
    }

    /// The code selection where the palette has its own (Everforest's `bg-visual`).
    pub fn selection(self) -> Option<Color> {
        (self == Scheme::Everforest).then(|| hex(ef::BG_VISUAL))
    }

    /// The note rail's purple and the purple of note labels.
    pub fn note_purples(self) -> (Color, Color) {
        match self {
            Scheme::Dark => (rune::oklch(0.606, 0.219, 292.7), rune::oklch(0.72, 0.219, 292.7)),
            Scheme::Light => (rune::oklch(0.606, 0.219, 292.7), rune::oklch(0.52, 0.219, 292.7)),
            Scheme::Everforest => (hex(ef::PURPLE), hex(ef::PURPLE)),
            Scheme::GruvboxHard | Scheme::GruvboxMedium => (hex(gb::PURPLE), hex(gb::PURPLE)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surfaces {
    /// Behind everything, and the top bar.
    pub bar: Color,
    /// Sidebar and tab strip.
    pub panel: Color,
    /// Reader, Overview page and diff body.
    pub reader: Color,
    pub card_head: Color,
    /// Inset areas: Facts list, filter box.
    pub well: Color,
    pub hairline: Color,
    pub card_border: Color,
    pub strong_line: Color,
    pub selected_row: Color,
}

#[derive(Clone, Debug)]
pub struct Look {
    pub scheme: Scheme,
    pub dark: bool,
    /// What every view and rune-kit component builds with.
    pub kit: Kit,
    /// `kit` with the main pane's corner and edge for `SplitView::build` only.
    pub shell: Kit,
    pub s: Surfaces,
}

fn hex(s: &str) -> Color {
    rune::color::hex(s).unwrap_or(Color::BLACK)
}

/// Rain's Everforest dark: sainnhe/everforest's medium dark palette, and the hard variant's
/// `bg_dim` as the page.
mod ef {
    pub const PAGE: &str = "#1e2326";
    pub const BG0: &str = "#2d353b";
    pub const BG1: &str = "#343f44";
    pub const BG2: &str = "#3d484d";
    pub const BG3: &str = "#475258";
    pub const BG4: &str = "#4f585e";
    pub const BG_VISUAL: &str = "#543a48";
    pub const BG_RED: &str = "#514045";
    pub const BG_GREEN: &str = "#425047";
    pub const BG_BLUE: &str = "#3a515d";
    pub const BG_YELLOW: &str = "#4d4c43";
    pub const FG: &str = "#d3c6aa";
    pub const RED: &str = "#e67e80";
    pub const YELLOW: &str = "#dbbc7f";
    pub const GREEN: &str = "#a7c080";
    pub const BLUE: &str = "#7fbbb3";
    pub const PURPLE: &str = "#d699b6";
    pub const GREY1: &str = "#859289";
    pub const GREY2: &str = "#9da9a0";
}

/// morhetz/gruvbox dark: `bg0_h`, `bg0`, `bg0_s`, `bg1`-`bg4`, `fg1`, `fg3`, `fg4` and the
/// bright accents, verbatim from `colors/gruvbox.vim`.
mod gb {
    pub const BG0_H: &str = "#1d2021";
    pub const BG0: &str = "#282828";
    pub const BG0_S: &str = "#32302f";
    pub const BG1: &str = "#3c3836";
    pub const BG2: &str = "#504945";
    pub const BG3: &str = "#665c54";
    pub const BG4: &str = "#7c6f64";
    pub const FG1: &str = "#ebdbb2";
    pub const FG3: &str = "#bdae93";
    pub const FG4: &str = "#a89984";
    pub const RED: &str = "#fb4934";
    pub const GREEN: &str = "#b8bb26";
    pub const YELLOW: &str = "#fabd2f";
    pub const BLUE: &str = "#83a598";
    pub const PURPLE: &str = "#d3869b";
}

/// bar, panel, reader, card_head, well, hairline, card_border, strong_line.
fn surface_hexes(scheme: Scheme) -> [&'static str; 8] {
    match scheme {
        Scheme::Dark => [
            "#2b2d32", "#232529", "#1c1d20", "#232529", "#18191c", "#3a3d44", "#464950", "#464950",
        ],
        Scheme::Light => [
            "#e3e4e7", "#eeeff1", "#fbfbfc", "#f1f2f4", "#f4f5f7", "#d4d6db", "#c4c6cb", "#c4c6cb",
        ],
        // Darken the page, never the cards: the ground drops below the ladder, panels stay bg1.
        Scheme::Everforest => [ef::PAGE, ef::BG1, ef::BG0, ef::BG1, ef::BG0, ef::BG3, ef::BG4, ef::BG4],
        Scheme::GruvboxHard => [gb::BG0_S, gb::BG0, gb::BG0_H, gb::BG0, gb::BG0_H, gb::BG2, gb::BG3, gb::BG4],
        Scheme::GruvboxMedium => [gb::BG1, gb::BG0_S, gb::BG0, gb::BG0_S, gb::BG0_H, gb::BG2, gb::BG3, gb::BG4],
    }
}

/// The surface table for a scheme; `k`'s accent tints the selection fill.
pub fn surfaces(scheme: Scheme, k: &Kit) -> Surfaces {
    let [bar, panel, reader, card_head, well, hairline, card_border, strong_line] = surface_hexes(scheme).map(hex);
    Surfaces {
        bar,
        panel,
        reader,
        card_head,
        well,
        hairline,
        card_border,
        strong_line,
        selected_row: k.alpha(k.c.bg_accent, if scheme.is_dark() { 0.16 } else { 0.12 }),
    }
}

/// A palette's roles: raised layers, text, and the accents for each state.
struct Tones {
    raised: [&'static str; 3],
    text: [&'static str; 3],
    primary: &'static str,
    link: &'static str,
    /// (accent, opaque tint) for success, warning, danger and info; no tint mixes the accent
    /// in at an alpha.
    states: [(&'static str, Option<&'static str>); 4],
}

fn tones(scheme: Scheme) -> Option<Tones> {
    let gruvbox = Tones {
        raised: [gb::BG1, gb::BG2, gb::BG3],
        text: [gb::FG1, gb::FG3, gb::FG4],
        primary: gb::BLUE,
        link: gb::BLUE,
        states: [(gb::GREEN, None), (gb::YELLOW, None), (gb::RED, None), (gb::BLUE, None)],
    };
    match scheme {
        Scheme::Dark | Scheme::Light => None,
        Scheme::Everforest => Some(Tones {
            raised: [ef::BG1, ef::BG2, ef::BG3],
            text: [ef::FG, ef::GREY2, ef::GREY1],
            primary: ef::GREEN,
            link: ef::BLUE,
            states: [
                (ef::GREEN, Some(ef::BG_GREEN)),
                (ef::YELLOW, Some(ef::BG_YELLOW)),
                (ef::RED, Some(ef::BG_RED)),
                (ef::BLUE, Some(ef::BG_BLUE)),
            ],
        }),
        Scheme::GruvboxHard | Scheme::GruvboxMedium => Some(gruvbox),
    }
}

/// Repaints the neutral dark kit from a palette, so no neutral grey or hue is left showing.
fn paint(kit: &mut Kit, t: &Tones, s: &Surfaces) {
    let [l2, l3, l4] = t.raised.map(hex);
    let [fg, muted, faint] = t.text.map(hex);
    let (primary, link) = (hex(t.primary), hex(t.link));
    let state = |(accent, tint): (&str, Option<&str>)| {
        let a = hex(accent);
        StateColors {
            bg: tint.map_or(a.alpha(0.15), hex),
            fg: a,
            border: a,
            icon: a,
        }
    };
    let [success, warning, danger, info] = t.states.map(state);
    let c = &mut kit.c;
    c.layer_02 = l2;
    c.layer_03 = l3;
    c.layer_04 = l4;
    c.bg_inverse = fg;
    c.bg_contrast = fg;
    c.bg_icon_button_contrast = fg;
    c.bg_button_neutral = l2;
    c.bg_accent = primary;
    c.submit_fg = s.bar;
    c.submit_icon = s.bar;
    c.text_base = fg;
    c.text_muted = muted;
    c.text_faint = faint;
    c.text_inverse = s.reader;
    c.text_contrast = s.reader;
    c.text_accent = link;
    c.icon_base = fg;
    c.icon_muted = muted;
    c.icon_faint = faint;
    c.icon_inverse = s.reader;
    c.icon_contrast = s.reader;
    c.icon_accent = primary;
    c.border_focus = primary;
    c.hover = fg.alpha(0.06);
    c.pressed = fg.alpha(0.10);
    c.knob = fg;
    c.knob_on = fg;
    c.success = success;
    c.warning = warning;
    c.danger = danger;
    c.info = info;
    c.surface_base = l2;
    c.surface_weak = s.card_head;
    c.surface_hover = l2;
    c.surface_raised = s.card_head;
    c.surface_raised_base = s.card_head;
    c.surface_raised_hover = l2;
    c.surface_float = fg;
    c.surface_disabled = s.card_head;
    c.border_hover = s.strong_line;
    c.border_active = primary;
    c.border_disabled = l2;
    c.text_strong = fg;
    c.text_weak = muted;
    c.text_invert_strong = s.reader;
    c.icon_weaker = faint;
    c.std_border = s.card_border;
    c.std_icon = muted;
    c.std_icon_invert = s.reader;
    c.std_icon_strong = fg;
    c.std_text = fg;
    kit.ring = Some(ring);
}

/// Elevation rings in the palette's border rather than the neutral kit's grey.
fn ring(c: &rune_kit::Palette) -> Color {
    c.border_base
}

pub fn look(scheme: Scheme) -> Look {
    let dark = scheme.is_dark();
    let mut kit = Kit::neutral(dark);
    let tones = tones(scheme);
    if let Some(t) = &tones {
        kit.c.bg_accent = hex(t.primary);
    }
    let s = surfaces(scheme, &kit);
    if let Some(t) = &tones {
        paint(&mut kit, t, &s);
    }
    let c = &mut kit.c;
    c.page = s.bar;
    c.bg_deep = s.bar;
    c.layer_01 = s.card_head;
    c.bg_base = s.reader;
    c.border_weak = s.hairline;
    c.border_base = s.card_border;
    c.border_strong = s.strong_line;
    if tones.is_some() {
        c.border_muted = s.hairline;
    }
    kit.radius = Radius {
        xs: 2.0,
        sm: 3.0,
        md: 4.0,
        lg: 6.0,
        xl: 8.0,
    };
    let mut shell = kit.clone();
    shell.radius.lg = 0.0;
    shell.c.border_muted = s.hairline;
    Look {
        scheme,
        dark,
        kit,
        shell,
        s,
    }
}

/// The scheme a kit from [`look`] was built for, told by its page colour (each scheme's bar is
/// its own); a kit from elsewhere reads as dark or light.
pub fn scheme_of(k: &Kit) -> Scheme {
    let page = k.c.page;
    Scheme::ALL
        .into_iter()
        .find(|&s| hex(surface_hexes(s)[0]) == page)
        .unwrap_or(if k.dark { Scheme::Dark } else { Scheme::Light })
}
