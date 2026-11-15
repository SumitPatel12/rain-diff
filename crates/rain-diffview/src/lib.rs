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

mod model;
#[doc(hidden)]
pub mod myers;
mod state;
mod view;

#[cfg(test)]
#[path = "../tests/common/patch.rs"]
mod testing;

pub use model::{hunk_header, language};
pub use state::{
    Align, Annotation, DiffMode, DiffState, EXPAND_STEP, Expand, Layout, LineRange, LineRef, Metrics, Opened, SpanSelection, Spot,
};
pub use view::{DiffColors, DiffView, SCROLL_KEY};
