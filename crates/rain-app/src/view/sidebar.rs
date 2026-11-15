//! The list pane's host: the list on the panel surface.

use rune::{El, col};

use super::{board, hairline};
use crate::app::App;
use crate::theme::Look;

/// The list as the Files overlay shows it.
pub(super) fn sidebar(s: &App, look: &Look) -> El {
    col().key("side").w_full().h_full().min_h(0.0).child(board::board(s, look))
}

/// The list in the shell's sidebar, edge to edge under a hairline.
pub(super) fn pane(s: &App, look: &Look) -> El {
    col()
        .key("side-panel")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .fill(look.s.panel)
        .child(hairline(look.s.hairline))
        .child(col().w_full().grow(1.0).min_h(0.0).child(sidebar(s, look)))
}
