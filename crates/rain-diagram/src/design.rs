//! PR Lens's design constants (`renderer/design.ts` with difit's PATCHES.md sizes) and the two
//! palettes from `renderer/theme.ts`. Every measurement here is in diagram points at 100 %.

use rune::Color;
use rune::color::srgb_decode;

pub const DIAGRAM_MARGIN: f32 = 16.0;

pub const LANE_TOP: f32 = 44.0;
pub const LANE_HEADER_BASELINE: f32 = 68.0;
pub const LANE_PADDING_X: f32 = 16.0;
pub const LANE_GAP: f32 = 20.0;
pub const LANE_BOTTOM_PADDING: f32 = 20.0;
pub const LANE_CONTENT_WIDTH: f32 = 372.0;
pub const LANE_RADIUS: f32 = 12.0;
pub const LANE_LABEL_SIZE: f32 = 10.0;
pub const LANE_LABEL_TRACKING: f32 = 0.12;

pub const CONTENT_TOP: f32 = 92.0;
pub const ROW_GAP: f32 = 52.0;

pub const CARD_RADIUS: f32 = 10.0;
pub const CARD_GAP_X: f32 = 12.0;
pub const CARD_HEIGHT: f32 = 52.0;
pub const CARD_HEIGHT_WITH_SUBTITLE: f32 = 62.0;
pub const CARD_PADDING_X: f32 = 14.0;

pub const ICON_CHIP_SIZE: f32 = 26.0;
pub const ICON_CHIP_GAP: f32 = 10.0;
pub const ICON_CHIP_RADIUS: f32 = 7.0;
pub const ICON_MIN_CARD_WIDTH: f32 = 200.0;

pub const TITLE_SIZE: f32 = 12.0;
pub const TITLE_SIZE_SMALL: f32 = 11.5;
pub const TITLE_SIZE_MIN: f32 = 9.5;
pub const TITLE_SIZE_STEP: f32 = 0.5;
pub const SUBTITLE_SIZE: f32 = 9.0;

pub const BADGE_HEIGHT: f32 = 16.0;
pub const BADGE_PADDING_X: f32 = 9.0;
pub const BADGE_TEXT_SIZE: f32 = 8.5;
pub const BADGE_TRACKING: f32 = 0.06;
pub const BADGE_GAP: f32 = 6.0;
pub const BADGE_RADIUS: f32 = 8.0;
pub const BADGE_RISE: f32 = 8.0;

pub const TABLE_ROW_HEIGHT: f32 = 20.0;
pub const TABLE_ROWS_PADDING_Y: f32 = 6.0;
pub const TABLE_ROW_TEXT_SIZE: f32 = 11.0;
pub const TABLE_ROW_TYPE_SIZE: f32 = 9.5;
pub const TABLE_ROWS_MAX: usize = 24;
pub const TABLE_UNCHANGED_ROWS_MAX: usize = 8;
pub const TABLE_FOLD_MIN_HIDDEN: usize = 4;
pub const TABLE_FOLD_GAP: f32 = TABLE_ROWS_PADDING_Y;
pub const TABLE_FOLD_HEIGHT: f32 = TABLE_FOLD_GAP + TABLE_ROW_HEIGHT;
pub const KEY_CHIP_WIDTH: f32 = 24.0;
pub const KEY_CHIP_HEIGHT: f32 = 14.0;
pub const KEY_CHIP_GAP: f32 = 8.0;
pub const TABLE_INDEX_ROW_HEIGHT: f32 = 18.0;
pub const TABLE_INDEX_RULE_GAP: f32 = 8.0;
pub const TABLE_INDEX_UNCHANGED_MAX: usize = 3;
pub const TABLE_INDEX_FOLD_HEIGHT: f32 = TABLE_FOLD_GAP + TABLE_INDEX_ROW_HEIGHT;
pub const KEY_ADDED_STROKE_WIDTH: f32 = 2.25;
pub const OVER_PITCH: f32 = 16.0;
pub const OVER_CLEARANCE: f32 = 10.0;
pub const PORT_FAN_MAX: f32 = 5.0;

pub const PILL_HEIGHT: f32 = 15.0;
pub const PILL_PADDING_X: f32 = 8.0;
pub const PILL_TEXT_SIZE: f32 = 9.5;

pub const TRACK_CLEARANCE: f32 = 6.0;
pub const TRACK_PITCH_MAX: f32 = 16.0;
pub const TRACK_PITCH_MIN: f32 = 10.0;
pub const PORT_PITCH: f32 = 16.0;
pub const PORT_INSET: f32 = 14.0;
pub const BEND_RADIUS_MAX: f32 = 34.0;

pub const PARTICIPANT_TOP: f32 = 18.0;
pub const LIFELINE_GAP: f32 = 2.0;
pub const FIRST_MESSAGE_DROP: f32 = 41.0;
pub const MESSAGE_PITCH: f32 = 38.0;
pub const SELF_MESSAGE_PITCH: f32 = 54.0;
pub const FLOW_BOTTOM_PADDING: f32 = 24.0;
pub const COLUMN_MIN_WIDTH: f32 = 150.0;
pub const COLUMN_GAP: f32 = 80.0;
pub const ACTIVATION_HALF_WIDTH: f32 = 6.0;
pub const SELF_LOOP_REACH: f32 = 52.0;
pub const SELF_LOOP_DROP: f32 = 6.0;
pub const SELF_LOOP_CORNER: f32 = 7.0;
pub const SELF_LOOP_EXTENT: f32 = SELF_LOOP_DROP + SELF_LOOP_CORNER * 2.0;
/// A note beside a message: its width, and the gap it keeps from the arrow's right end.
pub const NOTE_WIDTH: f32 = 180.0;
pub const NOTE_GAP: f32 = 14.0;

/// Width of `text` at `size` in the diagram's monospace face (difit patch: 0.6 em per code point,
/// 1.2 em for East-Asian wide ones).
pub fn measure(text: &str, size: f32) -> f32 {
    text.chars().map(|c| if is_wide(c) { 1.2 } else { 0.6 }).sum::<f32>() * size
}

pub fn tracked_width(text: &str, size: f32, tracking: f32) -> f32 {
    measure(text, size) + text.chars().count() as f32 * tracking * size
}

fn is_wide(c: char) -> bool {
    matches!(c as u32, 0x1100..=0x115F | 0x2E80..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6 | 0x1F300..=0x1FAFF | 0x20000..=0x3FFFD)
}

/// Every colour a diagram paints, per theme (PR Lens `Palette`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DiagramStyle {
    pub dark: bool,
    pub background: Color,
    pub dot: Color,
    pub lane: Color,
    pub card: Color,
    pub card_border: Color,
    pub foreground: Color,
    pub muted: Color,
    pub edge: Color,
    pub chip: Color,
    pub pill: Color,
    pub pill_border: Color,
    pub lifeline: Color,
    pub added: Color,
    pub added_text: Color,
    pub added_fill: Color,
    pub added_border: Color,
    pub modified: Color,
    pub modified_text: Color,
    pub modified_fill: Color,
    pub modified_border: Color,
    pub removed: Color,
    pub removed_text: Color,
    pub removed_fill: Color,
    pub removed_border: Color,
    pub neutral_fill: Color,
    pub shadow: Color,
    /// The reviewed share of a node's files: its bar and the tint of a fully viewed card.
    pub viewed: Color,
    pub viewed_fill: Color,
    /// A node's thread-count badge and the selection ring.
    pub thread: Color,
    pub thread_text: Color,
    /// One hue per key family in the data-model lens; none of them is a delta tone.
    pub keys: [Color; 5],
    pub key_fills: [Color; 5],
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    let c = |v: u8| srgb_decode(v as f32 / 255.0);
    Color {
        r: c(r),
        g: c(g),
        b: c(b),
        a,
    }
}

fn hex(v: u32) -> Color {
    rgba((v >> 16) as u8, (v >> 8) as u8, v as u8, 1.0)
}

impl DiagramStyle {
    pub fn for_theme(dark: bool) -> DiagramStyle {
        if dark { Self::dark() } else { Self::light() }
    }

    /// The theme a rune-kit `Kit` was built for.
    pub fn from_kit(k: &rune_kit::Kit) -> DiagramStyle {
        Self::for_theme(k.dark)
    }

    pub fn light() -> DiagramStyle {
        let keys = [0x0f766e, 0xbf3989, 0x4f46e5, 0x8a6a4a, 0x4b6a8a];
        DiagramStyle {
            dark: false,
            background: hex(0xf4f5f7),
            dot: rgba(140, 149, 159, 0.5),
            lane: rgba(255, 255, 255, 0.55),
            card: hex(0xffffff),
            card_border: hex(0xd1d9e0),
            foreground: hex(0x1f2328),
            muted: hex(0x59636e),
            edge: hex(0x8c959f),
            chip: hex(0xf1f3f5),
            pill: hex(0xffffff),
            pill_border: hex(0xd8dee4),
            lifeline: hex(0xd1d9e0),
            added: hex(0x1f883d),
            added_text: hex(0x116329),
            added_fill: hex(0xb9f0c4),
            added_border: rgba(31, 136, 61, 0.55),
            modified: hex(0xbf8700),
            modified_text: hex(0x7d4e00),
            modified_fill: hex(0xfae17d),
            modified_border: rgba(154, 103, 0, 0.55),
            removed: hex(0xcf222e),
            removed_text: hex(0xa40e26),
            removed_fill: hex(0xffcecb),
            removed_border: rgba(207, 34, 46, 0.5),
            neutral_fill: hex(0xf1f3f5),
            shadow: rgba(31, 35, 40, 0.14),
            viewed: hex(0x0969da),
            viewed_fill: rgba(9, 105, 218, 0.08),
            thread: hex(0x8250df),
            thread_text: hex(0xffffff),
            keys: keys.map(hex),
            key_fills: keys.map(|k| hex(k).alpha(0.14)),
        }
    }

    pub fn dark() -> DiagramStyle {
        let keys = [0x2dd4bf, 0xf778ba, 0xa5b4fc, 0xc9a27a, 0x9fb3c8];
        DiagramStyle {
            dark: true,
            background: hex(0x0d1117),
            dot: rgba(110, 118, 129, 0.22),
            lane: rgba(110, 118, 129, 0.07),
            card: hex(0x1c2128),
            card_border: hex(0x3d444d),
            foreground: hex(0xe6edf3),
            muted: hex(0x9198a1),
            edge: hex(0x6e7681),
            chip: rgba(110, 118, 129, 0.18),
            pill: hex(0x0d1117),
            pill_border: hex(0x21262d),
            lifeline: hex(0x30363d),
            added: hex(0x3fb950),
            added_text: hex(0x3fb950),
            added_fill: rgba(46, 160, 67, 0.15),
            added_border: rgba(63, 185, 80, 0.4),
            modified: hex(0xd29922),
            modified_text: hex(0xd29922),
            modified_fill: rgba(187, 128, 9, 0.15),
            modified_border: rgba(210, 153, 34, 0.4),
            removed: hex(0xf85149),
            removed_text: hex(0xf85149),
            removed_fill: rgba(248, 81, 73, 0.12),
            removed_border: rgba(248, 81, 73, 0.4),
            neutral_fill: rgba(110, 118, 129, 0.18),
            shadow: rgba(0, 0, 0, 0.28),
            viewed: hex(0x58a6ff),
            viewed_fill: rgba(56, 139, 253, 0.10),
            thread: hex(0xa371f7),
            thread_text: hex(0x0d1117),
            keys: keys.map(hex),
            key_fills: keys.map(|k| hex(k).alpha(0.14)),
        }
    }
}
