//! The token layer: for a scheme, the colour roles regions paint and the kits every view and
//! rune-kit component builds with. Islands: the list, the reader and side panels are rounded
//! panels on a darker ground.
//!
//! Everforest is the redesign's own role set (Everforest-derived, round 1b's `kit.css`). Dark and
//! light are Rune's neutral kit; the two Gruvbox darks repaint the kit from morhetz/gruvbox's
//! `gruvbox.vim`. Every scheme defines every role, so a view never reaches for a hex.

use rain_diagram::DiagramStyle;
use rune::Color;
use rune::tok::TextStyle;
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

    /// The note rail's purple and the purple of note labels.
    pub fn note_purples(self) -> (Color, Color) {
        match self {
            Scheme::Dark => (rune::oklch(0.606, 0.219, 292.7), rune::oklch(0.72, 0.219, 292.7)),
            Scheme::Light => (rune::oklch(0.606, 0.219, 292.7), rune::oklch(0.52, 0.219, 292.7)),
            Scheme::Everforest => (hex(d1::NOTE), hex(d1::NOTE)),
            Scheme::GruvboxHard | Scheme::GruvboxMedium => (hex(gb::PURPLE), hex(gb::PURPLE)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surfaces {
    /// The window's ground: behind the islands, the top bar and the status line.
    pub bar: Color,
    /// The list island, thread cards and side islands.
    pub panel: Color,
    /// The reader island: the diff and the Overview.
    pub reader: Color,
    /// The sticky file header band.
    pub card_head: Color,
    /// Inset areas: inputs, the hunk bar, the group tray, Facts.
    pub well: Color,
    pub hairline: Color,
    pub card_border: Color,
    pub strong_line: Color,
    pub selected_row: Color,
}

/// The colour roles beyond the surfaces and the kit's palette: raised layers, agent notes, the
/// diff's fills, find hits and the tour's dim. Every scheme defines each one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Roles {
    /// Menus, popovers, the palette, modals and toasts.
    pub raised: Color,
    pub focus: Color,
    /// The Must review marker and progress segment.
    pub must: Color,
    pub accent: Color,
    pub on_accent: Color,
    /// Agent notes: the rail and label, the card fill and its ring.
    pub note: Color,
    pub note_bg: Color,
    pub note_border: Color,
    pub add_bg: Color,
    pub del_bg: Color,
    pub add_word: Color,
    pub del_word: Color,
    /// The empty side of a split row.
    pub filler: Color,
    pub cursor_line: Color,
    pub selection: Color,
    pub find_hit: Color,
    pub find_current: Color,
    /// What the tour lays over the reader outside its spotlight.
    pub spotlight_dim: Color,
    pub shadow: Color,
}

/// Type styles the redesign names (Segoe UI 400/600/700, JetBrains Mono 400).
#[derive(Clone, Copy, Debug)]
pub struct Type {
    pub title: TextStyle,
    pub ui: TextStyle,
    pub strong: TextStyle,
    pub small: TextStyle,
    pub label: TextStyle,
    pub micro: TextStyle,
    pub micro_mono: TextStyle,
    pub mark: TextStyle,
    pub code: TextStyle,
    pub gutter: TextStyle,
    pub h1: TextStyle,
    pub h2: TextStyle,
    pub body: TextStyle,
}

pub const TYPE: Type = Type {
    title: TextStyle::new(14.0, 600.0).lh(20.0),
    ui: TextStyle::new(13.0, 400.0).lh(20.0),
    strong: TextStyle::new(13.0, 600.0).lh(20.0),
    small: TextStyle::new(12.0, 400.0).lh(18.0),
    label: TextStyle::new(12.0, 600.0).lh(18.0),
    micro: TextStyle::new(11.0, 400.0).lh(16.0),
    micro_mono: TextStyle::new(11.0, 400.0).mono().lh(16.0),
    mark: TextStyle::new(10.0, 600.0).lh(16.0),
    code: TextStyle::new(13.0, 400.0).mono().lh(20.0),
    gutter: TextStyle::new(12.0, 400.0).mono().lh(20.0),
    h1: TextStyle::new(24.0, 600.0).lh(32.0),
    h2: TextStyle::new(16.0, 600.0).lh(24.0),
    body: TextStyle::new(14.0, 400.0).lh(22.0),
};

/// Radii by use.
pub mod radius {
    /// Word highlights.
    pub const WORD: f32 = 2.0;
    /// Key caps and the Viewed box.
    pub const KEY: f32 = 3.0;
    /// Chips, notes and gutter marks.
    pub const CHIP: f32 = 4.0;
    /// Small buttons.
    pub const SMALL: f32 = 5.0;
    /// Buttons, rows, fields and Viewed.
    pub const BUTTON: f32 = 6.0;
    /// Cards, menus and the tray.
    pub const CARD: f32 = 8.0;
    /// Islands and modals.
    pub const ISLAND: f32 = 10.0;
}

/// The redesign's spacing and density.
pub mod space {
    /// Between the islands and around them; the gap is the list's drag handle.
    pub const GAP: f32 = 8.0;
    pub const TOP_BAR: f32 = 44.0;
    pub const STATUS: f32 = 30.0;
    pub const READER_TOOL: f32 = 40.0;
}

#[derive(Clone, Debug)]
pub struct Look {
    pub scheme: Scheme,
    pub dark: bool,
    /// What every view and rune-kit component builds with.
    pub kit: Kit,
    /// `kit` with square corners for shell parts that draw their own.
    pub shell: Kit,
    pub s: Surfaces,
    pub r: Roles,
}

fn hex(s: &str) -> Color {
    rune::color::hex(s).unwrap_or(Color::BLACK)
}

/// The app icon's own colours (`assets/icon/rain.svg`), for the weather that echoes it; the same
/// in every scheme, as the icon is.
pub mod brand {
    use super::hex;
    use rune::Color;

    pub fn cloud_front() -> Color {
        hex("#7E88A6")
    }
    pub fn cloud_back() -> Color {
        hex("#4B5473")
    }
    pub fn red() -> Color {
        hex("#E5605A")
    }
    pub fn green() -> Color {
        hex("#3FBF7F")
    }
    pub fn sky_top() -> Color {
        hex("#2E3556")
    }
    pub fn sky_bottom() -> Color {
        hex("#121729")
    }
    /// The lightning: a bright core and a glow, purple and green.
    pub fn purple_core() -> Color {
        hex("#D2B2FF")
    }
    pub fn purple_glow() -> Color {
        hex("#B07CFF")
    }
    pub fn green_core() -> Color {
        hex("#A8F8D0")
    }
    pub fn green_glow() -> Color {
        hex("#6FF0B0")
    }
}

/// The redesign's roles (`round1b/src/kit.css`), Everforest-derived.
mod d1 {
    pub const GROUND: &str = "#13171a";
    pub const PANEL: &str = "#21282c";
    pub const READER: &str = "#1b2023";
    pub const CARD_HEAD: &str = "#242b2f";
    pub const RAISED: &str = "#283035";
    pub const WELL: &str = "#161a1d";
    pub const HAIRLINE: &str = "#2c3438";
    pub const CARD_BORDER: &str = "#384246";
    pub const STRONG_LINE: &str = "#4f585e";
    pub const SELECTED_ROW: &str = "#2d3a2f";
    pub const TEXT: &str = "#d3c6aa";
    pub const TEXT_MUTED: &str = "#9da9a0";
    pub const TEXT_FAINT: &str = "#737d78";
    pub const TEXT_ACCENT: &str = "#7fbbb3";
    pub const ACCENT: &str = "#a7c080";
    pub const ON_ACCENT: &str = "#1a2016";
    /// (fg, bg, border) for success, warning, danger and info.
    pub const STATES: [(&str, &str, &str); 4] = [
        ("#a7c080", "#283126", "#45533a"),
        ("#dbbc7f", "#332e23", "#5a4e34"),
        ("#e67e80", "#37292b", "#643f42"),
        ("#7fbbb3", "#22302e", "#3b5450"),
    ];
    pub const NOTE: &str = "#d699b6";
    pub const NOTE_BG: &str = "#2a2330";
    pub const NOTE_BORDER: &str = "#46374f";
    pub const MUST: &str = "#e67e80";
    pub const ADD_BG: &str = "#212a20";
    pub const DEL_BG: &str = "#2d2325";
    pub const ADD_WORD: &str = "#384a2e";
    pub const DEL_WORD: &str = "#553538";
    pub const FILLER: &str = "#171b1d";
    pub const CURSOR_LINE: &str = "#252d30";
    pub const SELECTION: &str = "#543a48";
    pub const FIND_HIT: &str = "#5a4e2a";
    pub const FIND_CURRENT: &str = "#8a7438";
    pub const FOCUS: &str = "#a7c080";
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
    pub const FG1: &str = "#ebdbb2";
    pub const FG3: &str = "#bdae93";
    pub const FG4: &str = "#a89984";
    pub const RED: &str = "#fb4934";
    pub const GREEN: &str = "#b8bb26";
    pub const YELLOW: &str = "#fabd2f";
    pub const BLUE: &str = "#83a598";
    pub const PURPLE: &str = "#d3869b";
}

/// ground, panel, reader, card_head, well, hairline, card_border, strong_line.
fn surface_hexes(scheme: Scheme) -> [&'static str; 8] {
    match scheme {
        Scheme::Dark => [
            "#18191c", "#232529", "#1c1d20", "#26282c", "#141517", "#2e3035", "#3a3d44", "#464950",
        ],
        Scheme::Light => [
            "#dcdde1", "#eeeff1", "#fbfbfc", "#f3f4f6", "#f4f5f7", "#dcdee2", "#c4c6cb", "#b4b7bd",
        ],
        Scheme::Everforest => [
            d1::GROUND,
            d1::PANEL,
            d1::READER,
            d1::CARD_HEAD,
            d1::WELL,
            d1::HAIRLINE,
            d1::CARD_BORDER,
            d1::STRONG_LINE,
        ],
        Scheme::GruvboxHard => [gb::BG0_H, gb::BG0, gb::BG0, gb::BG0_S, gb::BG0_H, gb::BG1, gb::BG2, gb::BG3],
        Scheme::GruvboxMedium => [gb::BG0_H, gb::BG0_S, gb::BG0, gb::BG1, gb::BG0_H, gb::BG1, gb::BG2, gb::BG3],
    }
}

/// The surface table for a scheme; `k`'s accent tints the selected row where the scheme has no
/// row colour of its own.
pub fn surfaces(scheme: Scheme, k: &Kit) -> Surfaces {
    let [bar, panel, reader, card_head, well, hairline, card_border, strong_line] = surface_hexes(scheme).map(hex);
    let selected_row = match scheme {
        Scheme::Everforest => hex(d1::SELECTED_ROW),
        _ => k.alpha(k.c.bg_accent, if scheme.is_dark() { 0.16 } else { 0.12 }),
    };
    Surfaces {
        bar,
        panel,
        reader,
        card_head,
        well,
        hairline,
        card_border,
        strong_line,
        selected_row,
    }
}

/// `t` of the way from `base` toward `hue` in OKLab, at `base`'s alpha.
fn tint(base: Color, hue: Color, t: f32) -> Color {
    let [a, b] = [base.to_oklab(), hue.to_oklab()];
    Color::from_oklab([0, 1, 2, 3].map(|i| if i == 3 { a[3] } else { a[i] + (b[i] - a[i]) * t }))
}

fn roles(scheme: Scheme, k: &Kit, s: &Surfaces) -> Roles {
    if scheme == Scheme::Everforest {
        return Roles {
            raised: hex(d1::RAISED),
            focus: hex(d1::FOCUS),
            must: hex(d1::MUST),
            accent: hex(d1::ACCENT),
            on_accent: hex(d1::ON_ACCENT),
            note: hex(d1::NOTE),
            note_bg: hex(d1::NOTE_BG),
            note_border: hex(d1::NOTE_BORDER),
            add_bg: hex(d1::ADD_BG),
            del_bg: hex(d1::DEL_BG),
            add_word: hex(d1::ADD_WORD),
            del_word: hex(d1::DEL_WORD),
            filler: hex(d1::FILLER),
            cursor_line: hex(d1::CURSOR_LINE),
            selection: hex(d1::SELECTION),
            find_hit: hex(d1::FIND_HIT),
            find_current: hex(d1::FIND_CURRENT),
            spotlight_dim: hex("#0a0c0d").alpha(0.55),
            shadow: Color::BLACK.alpha(0.45),
        };
    }
    let c = &k.c;
    let dark = scheme.is_dark();
    let bg = s.reader;
    let (add, del) = (c.success.icon, c.danger.icon);
    let note = scheme.note_purples().0;
    Roles {
        // Opaque: the kit's raised layers are see-through overlays, which leave dialogs glassy.
        raised: if dark { tint(s.reader, c.text_base, 0.07) } else { s.reader },
        focus: c.border_focus,
        must: c.danger.icon,
        accent: c.bg_accent,
        on_accent: c.submit_fg,
        note,
        note_bg: tint(bg, note, if dark { 0.12 } else { 0.08 }),
        note_border: tint(bg, note, if dark { 0.3 } else { 0.25 }),
        add_bg: tint(bg, add, if dark { 0.13 } else { 0.1 }),
        del_bg: tint(bg, del, if dark { 0.13 } else { 0.1 }),
        add_word: tint(bg, add, if dark { 0.32 } else { 0.27 }),
        del_word: tint(bg, del, if dark { 0.32 } else { 0.27 }),
        filler: tint(bg, s.bar, 0.5),
        cursor_line: tint(bg, c.text_base, if dark { 0.05 } else { 0.04 }),
        selection: k.selection(),
        find_hit: tint(bg, c.warning.icon, if dark { 0.35 } else { 0.3 }),
        find_current: tint(bg, c.warning.icon, if dark { 0.6 } else { 0.5 }),
        spotlight_dim: if dark { Color::BLACK.alpha(0.5) } else { Color::WHITE.alpha(0.55) },
        shadow: Color::BLACK.alpha(if dark { 0.45 } else { 0.18 }),
    }
}

/// A palette's roles: raised layers, text, and the accents for each state.
struct Tones {
    raised: [&'static str; 3],
    text: [&'static str; 3],
    primary: &'static str,
    on_primary: &'static str,
    link: &'static str,
    /// (fg, opaque tint, border) for success, warning, danger and info; no tint mixes the fg
    /// in at an alpha, no border uses the fg.
    states: [(&'static str, Option<&'static str>, Option<&'static str>); 4],
}

fn tones(scheme: Scheme) -> Option<Tones> {
    let gruvbox = Tones {
        raised: [gb::BG1, gb::BG2, gb::BG3],
        text: [gb::FG1, gb::FG3, gb::FG4],
        primary: gb::BLUE,
        on_primary: gb::BG0_H,
        link: gb::BLUE,
        states: [
            (gb::GREEN, None, None),
            (gb::YELLOW, None, None),
            (gb::RED, None, None),
            (gb::BLUE, None, None),
        ],
    };
    match scheme {
        Scheme::Dark | Scheme::Light => None,
        Scheme::Everforest => Some(Tones {
            raised: [d1::RAISED, d1::CARD_BORDER, d1::STRONG_LINE],
            text: [d1::TEXT, d1::TEXT_MUTED, d1::TEXT_FAINT],
            primary: d1::ACCENT,
            on_primary: d1::ON_ACCENT,
            link: d1::TEXT_ACCENT,
            states: d1::STATES.map(|(fg, bg, border)| (fg, Some(bg), Some(border))),
        }),
        Scheme::GruvboxHard | Scheme::GruvboxMedium => Some(gruvbox),
    }
}

/// Repaints the neutral dark kit from a palette, so no neutral grey or hue is left showing.
fn paint(kit: &mut Kit, t: &Tones, s: &Surfaces) {
    let [l2, l3, l4] = t.raised.map(hex);
    let [fg, muted, faint] = t.text.map(hex);
    let (primary, on_primary, link) = (hex(t.primary), hex(t.on_primary), hex(t.link));
    let state = |(accent, tint, border): (&str, Option<&str>, Option<&str>)| {
        let a = hex(accent);
        StateColors {
            bg: tint.map_or(a.alpha(0.15), hex),
            fg: a,
            border: border.map_or(a, hex),
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
    c.submit_fg = on_primary;
    c.submit_icon = on_primary;
    c.text_base = fg;
    c.text_muted = muted;
    c.text_faint = faint;
    c.text_inverse = s.reader;
    c.text_contrast = on_primary;
    c.text_accent = link;
    c.icon_base = fg;
    c.icon_muted = muted;
    c.icon_faint = faint;
    c.icon_inverse = s.reader;
    c.icon_contrast = on_primary;
    c.icon_accent = primary;
    c.border_focus = primary;
    c.hover = fg.alpha(0.06);
    c.pressed = fg.alpha(0.11);
    c.knob = fg;
    c.knob_on = fg;
    c.success = success;
    c.warning = warning;
    c.danger = danger;
    c.info = info;
    c.surface_base = l2;
    c.surface_weak = s.card_head;
    c.surface_hover = l2;
    c.surface_raised = l2;
    c.surface_raised_base = l2;
    c.surface_raised_hover = l3;
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
    c.border_muted = s.hairline;
    if scheme == Scheme::Everforest {
        c.scrim = hex("#060809").alpha(0.58);
    }
    kit.radius = Radius {
        xs: radius::WORD,
        sm: radius::KEY,
        md: radius::CHIP,
        lg: radius::BUTTON,
        xl: radius::CARD,
    };
    kit.ty.small = TYPE.small;
    kit.ty.small_medium = TYPE.small;
    let r = roles(scheme, &kit, &s);
    let mut shell = kit.clone();
    shell.radius.lg = 0.0;
    Look {
        scheme,
        dark,
        kit,
        shell,
        s,
        r,
    }
}

/// The scheme a kit from [`look`] was built for, told by its ground and card head (no two
/// schemes share both); a kit from elsewhere reads as dark or light.
pub fn scheme_of(k: &Kit) -> Scheme {
    let (page, head) = (k.c.page, k.c.layer_01);
    Scheme::ALL
        .into_iter()
        .find(|&s| hex(surface_hexes(s)[0]) == page && hex(surface_hexes(s)[3]) == head)
        .unwrap_or(if k.dark { Scheme::Dark } else { Scheme::Light })
}

/// The roles of the scheme a kit from [`look`] was built for.
pub fn roles_of(k: &Kit) -> Roles {
    static ROLES: std::sync::OnceLock<[Roles; 5]> = std::sync::OnceLock::new();
    let all = ROLES.get_or_init(|| Scheme::ALL.map(|s| look(s).r));
    let i = Scheme::ALL.iter().position(|&s| s == scheme_of(k)).unwrap_or(0);
    all[i]
}

/// The surfaces of the scheme a kit from [`look`] was built for.
pub fn surfaces_of(k: &Kit) -> Surfaces {
    static SURFACES: std::sync::OnceLock<[Surfaces; 5]> = std::sync::OnceLock::new();
    let all = SURFACES.get_or_init(|| Scheme::ALL.map(|s| look(s).s));
    let i = Scheme::ALL.iter().position(|&s| s == scheme_of(k)).unwrap_or(0);
    all[i]
}
