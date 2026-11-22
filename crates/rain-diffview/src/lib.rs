//! A diff view for reviewing changes, vendored from rune-kit's `diff` module (MIT, see
//! `VENDORED.md`) and rebuilt on `rain-core`'s compact model for million-line diffs: files of
//! hunks, unified or split, with lazy word changes, syntax colours, expandable unchanged ranges,
//! line, range and span annotations, line selection, text-span selection, a line cursor, a
//! spotlight on a range and stable hunk ids, on a virtual list. In split mode a one-sided file
//! (added, deleted) takes the whole width.
//!
//! The app keeps a [`DiffState`] of the files in its state and draws it with [`DiffView`].

#[cfg(test)]
extern crate self as rain_diffview;

mod lex;
mod model;
mod motion;
#[doc(hidden)]
pub mod myers;
mod pairing;
mod state;
mod view;

#[cfg(test)]
#[path = "../tests/common/patch.rs"]
mod testing;

pub use lex::{Carry, Lexer};
pub use model::{hunk_header, language};
pub use state::{
    Align, Annotation, Caret, DiffMode, DiffState, EXPAND_STEP, Expand, FileTexts, LINE_CAP, Layout, LineRange, LineRef, Metrics, Moved,
    Opened, SpanSelection, Spot, Vim, VimKey, Visual, drawn_part, layout_builds, rekeys, row_keys_built, word_diffs,
};
pub use view::{DiffColors, DiffView, SCROLL_KEY, fold_builds, line_builds, line_bytes};
