//! The file header's `Whole file e` button.

use rune::{El, Tx};
use rune_kit::Kit;

use super::icons;
use super::ui::Ghost;
use crate::app::App;

/// For full-list file `g`, on its header row; `None` hides it (no git repo to read from, or
/// nothing to open around). Disabled, it says why in its tooltip.
pub(super) fn header_button(s: &App, g: usize, k: &Kit) -> Option<El> {
    let f = s.diff.files().get(g)?;
    if !s.has_repo(g) {
        return None;
    }
    let local = s.local(g)?;
    let (label, help, disabled) = match s.whole_blocker(g) {
        Some(_) if f.hunks.is_empty() || f.binary => return None,
        Some(why) => ("Whole file", why, true),
        None if s.active().needs_texts(local) => ("Whole file…", "Reading the whole file  e", false),
        None if s.is_whole(g) => ("Whole file", "Show only the changes  e", false),
        None => ("Whole file", "Show the whole file with changes inline  e", false),
    };
    let b = Ghost::new(format!("whole-{g}"))
        .icon(&icons::FILE)
        .label(label)
        .hint("e")
        .help(help)
        .on(s.is_whole(g))
        .height(26.0)
        .build(k)
        .checked(s.is_whole(g));
    Some(if disabled {
        b.opacity(0.45)
    } else {
        b.on_press_tx(move |s: &mut App, tx: &mut Tx| s.toggle_whole(g, tx))
    })
}
