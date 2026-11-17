//! The file header's `Whole file` button, `Changes only` while the file shows whole.

use rain_core::command::Command;
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
        // The same label while it reads, so the button keeps its width.
        None if s.active().needs_texts(local) => ("Whole file", "Reading the whole file", false),
        None if s.is_whole(g) => ("Changes only", "Show only the changes", false),
        None => ("Whole file", "Show the whole file with changes inline", false),
    };
    let mut b = Ghost::new(format!("whole-{g}"))
        .icon(if s.is_whole(g) { &icons::FOLD_ALL } else { &icons::FILE })
        .label(label)
        .help(help);
    if let Some(c) = Command::ToggleWholeFile.chord() {
        b = b.hint(c.display());
    }
    let b = b.height(26.0).build(k);
    Some(if disabled {
        b.opacity(0.45)
    } else {
        b.on_press_tx(move |s: &mut App, tx: &mut Tx| s.toggle_whole(g, tx))
    })
}
