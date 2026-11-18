//! What every style draws with: type sizes on Rain's scale, the colours a change status takes,
//! and a few small parts (a centred label, a legend swatch).

use rain_diagram::DiagramStyle;
use rune::tok::TextStyle;
use rune::{Color, El, el, row, text};

use crate::schema::Status;

pub const TITLE: TextStyle = TextStyle::new(14.0, 600.0).lh(20.0);
pub const LABEL: TextStyle = TextStyle::new(12.0, 400.0).lh(16.0);
pub const STRONG: TextStyle = TextStyle::new(12.0, 600.0).lh(16.0);
pub const SMALL: TextStyle = TextStyle::new(11.0, 400.0).lh(14.0);
pub const MICRO: TextStyle = TextStyle::new(11.0, 600.0).lh(14.0);
pub const MONO: TextStyle = TextStyle::new(11.0, 400.0).mono().lh(14.0);

/// The colours a mark takes for its change status.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tone {
    pub fill: Color,
    pub border: Color,
    pub text: Color,
    /// Lines and arrows.
    pub stroke: Color,
}

pub fn tone(ink: &DiagramStyle, s: Status) -> Tone {
    match s {
        Status::Added => Tone {
            fill: ink.added_fill,
            border: ink.added_border,
            text: ink.added_text,
            stroke: ink.added,
        },
        Status::Changed => Tone {
            fill: ink.modified_fill,
            border: ink.modified_border,
            text: ink.modified_text,
            stroke: ink.modified,
        },
        Status::Removed => Tone {
            fill: ink.removed_fill,
            border: ink.removed_border,
            text: ink.removed_text,
            stroke: ink.removed,
        },
        Status::Same => Tone {
            fill: ink.neutral_fill,
            border: ink.card_border,
            text: ink.foreground,
            stroke: ink.edge,
        },
    }
}

/// The tier a layer is drawn in: the web is the frontend, the API and the domain the backend,
/// the data the database.
pub fn tier(layer: &str) -> &'static str {
    match layer {
        "web" | "ui" => "Frontend",
        "api" | "domain" => "Backend",
        "data" | "db" => "Database",
        _ => "Other",
    }
}

/// `t` centred in a `w` × `h` box.
pub fn centred(t: impl Into<String>, font: TextStyle, color: Color, w: f32, h: f32) -> El {
    row()
        .size(w, h)
        .center()
        .child(text(t.into()).font(font).color(color).a11y_hidden())
}

/// A legend entry: a swatch and its meaning.
pub fn swatch(label: &str, fill: Color, border: Color, ink: &DiagramStyle) -> El {
    row()
        .gap(6.0)
        .items_center()
        .child(el().size(12.0, 12.0).radius(3.0).fill(fill).border(1.0, border).a11y_hidden())
        .child(text(label.to_owned()).font(SMALL).color(ink.muted).a11y_hidden())
}
