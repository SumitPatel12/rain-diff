//! File tree with viewed checkboxes and review-tag filters: one root per repo, a viewed
//! checkbox on every row (tri-state on folders), +/− counts, comment badges, status and tag
//! glyphs with labels, chips that filter by repo and by tag, and a text filter. Built on rune-kit's
//! [`TreeModel`](rune_kit::TreeModel) (virtualised rows, keyboard, reveal).

mod model;
pub mod sample;
mod view;

#[cfg(test)]
mod tests;

pub use model::{FileEntry, Node, NodeKind, Progress, TREE_KEY, TreeFilter, TreeState, text_matches};
pub use view::{COMMENT, TreeView, tag_short, tag_slug};
