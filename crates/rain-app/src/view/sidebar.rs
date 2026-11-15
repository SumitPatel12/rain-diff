//! The list island's host.

use rune::{El, col};

use super::board;
use crate::app::App;
use crate::theme::Look;

/// The list as the Files overlay shows it.
pub(super) fn sidebar(s: &App, look: &Look) -> El {
    col().key("side").w_full().h_full().min_h(0.0).child(board::board(s, look))
}

/// The list in its island.
pub(super) fn pane(s: &App, look: &Look) -> El {
    col()
        .key("side-panel")
        .h_full()
        .min_h(0.0)
        .fill(look.s.panel)
        .child(sidebar(s, look))
}
