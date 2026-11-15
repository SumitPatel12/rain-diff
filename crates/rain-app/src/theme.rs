//! The chrome token layer: for a chrome style (Zed or Islands), theme and density, the surfaces
//! regions paint, their shape, and the kits every view and rune-kit component builds with.

use rain_diffview::FileFrame;
use rune::Color;
use rune_kit::{Kit, Radius};

use crate::settings::{Chrome, Density};

/// Below this window width Islands' file cards sit closer to the reader's edges.
pub const NARROW: f32 = 1500.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surfaces {
    /// Behind everything: Islands' sea, Zed's bar colour.
    pub ground: Color,
    pub bar: Color,
    /// Sidebar and (Zed) tab strip.
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
    pub tab_active: Color,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabStyle {
    /// The active tab takes the reader's fill and opens into it.
    Merge,
    /// The active tab is filled and underlined in the accent.
    Underline,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    pub island_gap: f32,
    pub island_radius: f32,
    /// Hairlines between bar and body, sidebar and reader, tabs and content, body and status.
    pub region_lines: bool,
    pub file_frame: FileFrame,
    pub card_inset: f32,
    pub card_gap: f32,
    /// The list's current row is an inset pill rather than a full-width band.
    pub row_pill: bool,
    pub tab_style: TabStyle,
}

impl Shape {
    /// The file frame at a window `width`: Islands' cards come in closer on narrow windows.
    pub fn frame(&self, width: f32) -> FileFrame {
        match self.file_frame {
            FileFrame::Card { gap, radius, .. } => FileFrame::Card {
                inset: if width > 0.0 && width < NARROW { 8.0 } else { self.card_inset },
                gap,
                radius,
            },
            f => f,
        }
    }

    /// Points the file frame takes from the reader's width at a window `width`.
    pub fn card_sides(&self, width: f32) -> f32 {
        match self.frame(width) {
            FileFrame::Card { inset, .. } => 2.0 * (inset + 1.0),
            FileFrame::Bar => 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Look {
    pub chrome: Chrome,
    pub dark: bool,
    /// What every view and rune-kit component builds with.
    pub kit: Kit,
    /// `kit` with the main pane's corner and edge for `SplitView::build` only.
    pub shell: Kit,
    pub s: Surfaces,
    pub shape: Shape,
}

fn hex(s: &str) -> Color {
    rune::color::hex(s).unwrap_or(Color::BLACK)
}

/// The surface table for a chrome style and theme; `accent` tints the selection fills.
pub fn surfaces(chrome: Chrome, dark: bool, k: &Kit) -> Surfaces {
    let accent = k.c.bg_accent;
    let [ground, panel, reader, card_head, well, hairline, card_border, strong_line] = match (chrome, dark) {
        (Chrome::Zed, true) => [
            "#2b2d32", "#232529", "#1c1d20", "#232529", "#18191c", "#3a3d44", "#464950", "#464950",
        ],
        (Chrome::Zed, false) => [
            "#e3e4e7", "#eeeff1", "#fbfbfc", "#f1f2f4", "#f4f5f7", "#d4d6db", "#c4c6cb", "#c4c6cb",
        ],
        (Chrome::Islands, true) => [
            "#2e3034", "#1d1e21", "#1d1e21", "#25272b", "#191a1d", "#2c2e33", "#36383e", "#3b3e44",
        ],
        (Chrome::IslandsDeep, true) => [
            "#0f1012", "#232529", "#232529", "#2a2c31", "#1e2023", "#2f3237", "#3a3d43", "#42454c",
        ],
        (Chrome::Islands | Chrome::IslandsDeep, false) => [
            "#e4e6ea", "#ffffff", "#ffffff", "#f5f6f8", "#f2f3f5", "#e8e9ec", "#d9dbe0", "#d0d3d8",
        ],
    }
    .map(hex);
    let (selected, tab) = match (chrome, dark) {
        (Chrome::Zed, true) => (0.16, None),
        (Chrome::Zed, false) => (0.12, None),
        (Chrome::Islands | Chrome::IslandsDeep, true) => (0.18, Some(0.10)),
        (Chrome::Islands | Chrome::IslandsDeep, false) => (0.12, Some(0.08)),
    };
    Surfaces {
        ground,
        bar: ground,
        panel,
        reader,
        card_head,
        well,
        hairline,
        card_border,
        strong_line,
        selected_row: k.alpha(accent, selected),
        tab_active: tab.map_or(reader, |a| k.alpha(accent, a)),
    }
}

pub fn shape(chrome: Chrome, density: Density) -> Shape {
    match chrome {
        Chrome::Zed => Shape {
            island_gap: 0.0,
            island_radius: 0.0,
            region_lines: true,
            file_frame: FileFrame::Bar,
            card_inset: 0.0,
            card_gap: 0.0,
            row_pill: false,
            tab_style: TabStyle::Merge,
        },
        Chrome::Islands | Chrome::IslandsDeep => {
            let (gap, radius) = match density {
                Density::Comfortable => (8.0, 10.0),
                Density::Compact => (4.0, 8.0),
            };
            Shape {
                island_gap: gap,
                island_radius: radius,
                region_lines: false,
                file_frame: FileFrame::Card {
                    inset: 12.0,
                    gap: 12.0,
                    radius: 8.0,
                },
                card_inset: 12.0,
                card_gap: 12.0,
                row_pill: true,
                tab_style: TabStyle::Underline,
            }
        }
    }
}

pub fn look(dark: bool, chrome: Chrome, density: Density) -> Look {
    let mut kit = Kit::neutral(dark);
    let s = surfaces(chrome, dark, &kit);
    let shape = shape(chrome, density);
    let c = &mut kit.c;
    c.page = s.ground;
    c.bg_deep = s.ground;
    c.layer_01 = s.card_head;
    c.bg_base = s.reader;
    c.border_weak = s.hairline;
    c.border_base = s.card_border;
    c.border_strong = s.strong_line;
    kit.radius = match chrome {
        Chrome::Zed => Radius {
            xs: 2.0,
            sm: 3.0,
            md: 4.0,
            lg: 6.0,
            xl: 8.0,
        },
        Chrome::Islands | Chrome::IslandsDeep => Radius {
            xs: 2.0,
            sm: 4.0,
            md: 6.0,
            lg: 8.0,
            xl: 10.0,
        },
    };
    let mut shell = kit.clone();
    shell.radius.lg = shape.island_radius;
    shell.c.border_muted = if shape.region_lines { s.hairline } else { Color::TRANSPARENT };
    Look {
        chrome,
        dark,
        kit,
        shell,
        s,
        shape,
    }
}
