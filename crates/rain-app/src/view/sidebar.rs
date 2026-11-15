//! The list pane's host: the list on the panel surface, framed per chrome.

use rune::{El, col};

use super::{board, hairline};
use crate::app::App;
use crate::theme::Look;

/// The list as the Files overlay shows it.
pub(super) fn sidebar(s: &App, look: &Look) -> El {
    col().key("side").w_full().h_full().min_h(0.0).child(board::board(s, look))
}

/// The list in the shell's sidebar: edge to edge under a hairline (Zed), or a rounded island
/// with the gap around it (Islands).
pub(super) fn pane(s: &App, look: &Look) -> El {
    let (gap, r) = (look.shape.island_gap, look.shape.island_radius);
    let list = col().key("side-panel").w_full().grow(1.0).min_h(0.0).fill(look.s.panel);
    if look.shape.region_lines {
        return list
            .child(hairline(look.s.hairline))
            .child(col().w_full().grow(1.0).min_h(0.0).child(sidebar(s, look)));
    }
    col()
        .key("side-island")
        .w_full()
        .h_full()
        .min_h(0.0)
        .pl(gap)
        .pr(gap)
        .pb(gap)
        .child(list.radius(r).clip_rounded(r).child(sidebar(s, look)))
}
