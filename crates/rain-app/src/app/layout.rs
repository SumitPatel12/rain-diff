//! The shell's layout: the list (open or hidden), zoom with an exact restore, the overlay stack
//! and its one `Esc` order, and the width rules that decide split and docking.

use rain_diffview::DiffMode;
use rune_kit::PaneState;

use super::App;
use crate::settings::ViewLayout;

/// Below this window width the Conversation and Map are always overlays.
pub const WIDE: f32 = 1600.0;
/// A split side with fewer code columns than this reads unified; long lines wrap.
pub const SPLIT_MIN_COLUMNS: u32 = 60;
/// The code font's column advance (13 pt JetBrains Mono).
pub const MONO_ADVANCE: f32 = 7.8;
/// One split side's gutter: marks, line numbers, sign and the code's end padding.
pub const SIDE_GUTTER: f32 = 100.0;

/// A submenu of the top bar's `⋯` menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoreSub {
    /// Theme and Density.
    Appearance,
}

/// Which of the shell's menus and popovers is open; one at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Menus {
    /// The top bar's `⋯`, and which of its submenus.
    pub more: bool,
    pub more_sub: Option<MoreSub>,
    /// The filter chips popover under the list's filter box.
    pub chips: bool,
    /// A file header's `⋯`, by full-list file.
    pub file: Option<usize>,
    /// The status bar's diff layout menu.
    pub layout: bool,
}

impl Menus {
    /// Closes whatever is open; true when something was.
    pub fn close(&mut self) -> bool {
        let open = *self != Menus::default();
        *self = Menus::default();
        open
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Overlay {
    Files,
    Conversation,
    Map,
    Tour,
    Picker,
    Composer,
    Finish,
}

impl Overlay {
    pub const ALL: [Overlay; 7] = [
        Overlay::Files,
        Overlay::Conversation,
        Overlay::Map,
        Overlay::Tour,
        Overlay::Picker,
        Overlay::Composer,
        Overlay::Finish,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Overlay::Files => "Files",
            Overlay::Conversation => "Conversation",
            Overlay::Map => "Map",
            Overlay::Tour => "Tour",
            Overlay::Picker => "Picker",
            Overlay::Composer => "Composer",
            Overlay::Finish => "Finish review",
        }
    }
}

/// What zoom hides and `Shift+Esc` puts back.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutSnapshot {
    pub list_open: bool,
    pub list_width: f32,
    pub overlays: Vec<Overlay>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutState {
    /// Open overlays, bottom first; the last is on top.
    pub overlays: Vec<Overlay>,
    /// The layout before zoom; `Some` while zoomed.
    pub zoomed: Option<LayoutSnapshot>,
}

impl LayoutState {
    pub fn is_open(&self, o: Overlay) -> bool {
        self.overlays.contains(&o)
    }

    pub fn top(&self) -> Option<Overlay> {
        self.overlays.last().copied()
    }

    /// Opens `o` on top (moving it there when already open).
    pub fn open(&mut self, o: Overlay) {
        self.overlays.retain(|x| *x != o);
        self.overlays.push(o);
    }

    pub fn close(&mut self, o: Overlay) -> bool {
        let before = self.overlays.len();
        self.overlays.retain(|x| *x != o);
        before != self.overlays.len()
    }

    pub fn toggle(&mut self, o: Overlay) -> bool {
        if self.is_open(o) {
            self.close(o);
            false
        } else {
            self.open(o);
            true
        }
    }

    /// Closes the topmost overlay `Esc` closes first: everything but the tour, which `Esc`
    /// leaves only after the composer and find.
    pub fn pop_layer(&mut self) -> Option<Overlay> {
        let i = self.overlays.iter().rposition(|o| *o != Overlay::Tour)?;
        Some(self.overlays.remove(i))
    }

    pub fn is_zoomed(&self) -> bool {
        self.zoomed.is_some()
    }
}

/// Mono columns one split side gets in `width` points of diff.
pub fn columns_per_side(width: f32) -> u32 {
    if !width.is_finite() {
        return u32::MAX;
    }
    ((width / 2.0 - SIDE_GUTTER) / MONO_ADVANCE).max(0.0) as u32
}

impl App {
    /// `Shift+Esc`: hides the list and every overlay; again puts back exactly what was there.
    pub fn toggle_zoom(&mut self) {
        match self.layout.zoomed.take() {
            Some(snap) => {
                self.sidebar.set_width(snap.list_width);
                self.sidebar.set_open(snap.list_open);
                self.layout.overlays = snap.overlays;
                self.status.clear();
            }
            None => {
                self.layout.zoomed = Some(self.layout_snapshot());
                self.sidebar.set_open(false);
                self.layout.overlays.clear();
                self.status = ZOOMED.into();
            }
        }
        self.apply_split();
    }

    pub fn layout_snapshot(&self) -> LayoutSnapshot {
        LayoutSnapshot {
            list_open: self.sidebar.is_open(),
            list_width: self.sidebar.width(),
            overlays: self.layout.overlays.clone(),
        }
    }

    /// `Ctrl+B`: shows or hides the list. Zoom keeps it hidden.
    pub fn toggle_list(&mut self) {
        if self.layout.is_zoomed() {
            self.toggle_overlay(Overlay::Files);
            return;
        }
        self.sidebar.toggle();
        self.settings.list_open = self.sidebar.is_open();
        self.save_settings();
        self.apply_split();
    }

    /// Whether the list is hidden by `Ctrl+B` rather than by zoom.
    pub fn rail(&self) -> bool {
        !self.sidebar.is_open() && !self.layout.is_zoomed()
    }

    pub fn toggle_overlay(&mut self, o: Overlay) -> bool {
        self.layout.toggle(o)
    }

    pub fn open_overlay(&mut self, o: Overlay) {
        self.layout.open(o);
    }

    pub fn close_overlay(&mut self, o: Overlay) -> bool {
        self.layout.close(o)
    }

    /// Conversation and Map float over the reader below [`WIDE`] or while zoomed; above it
    /// they may dock.
    pub fn overlays_float(&self) -> bool {
        self.layout.is_zoomed() || self.window_width < WIDE
    }

    /// The diff mode the layout setting and the width give, and why it differs from the setting.
    pub fn layout_decision(&self) -> (DiffMode, Option<String>) {
        let cols = columns_per_side(self.diff_width);
        let fits = cols >= SPLIT_MIN_COLUMNS;
        match self.settings.layout {
            ViewLayout::Unified => (DiffMode::Unified, None),
            ViewLayout::Auto => (if fits { DiffMode::Split } else { DiffMode::Unified }, None),
            ViewLayout::Split if fits => (DiffMode::Split, None),
            ViewLayout::Split => (
                DiffMode::Unified,
                Some(format!(
                    "Each side would get {cols} columns; split needs {SPLIT_MIN_COLUMNS}. Widen the window, hide the list (Ctrl+B) or zoom (Shift+Esc)"
                )),
            ),
        }
    }

    /// Why the diff isn't split though the setting says so, for the toolbar.
    pub fn layout_reason(&self) -> Option<String> {
        self.layout_decision().1
    }
}

pub const ZOOMED: &str = "Zoomed · Ctrl+L files · C conversation · m map · o overview · Shift+Esc restore";

/// The list pane as the settings describe it.
pub(crate) fn list_pane(width: f32, open: bool) -> PaneState {
    let p = PaneState::new(width).range(220.0, 520.0);
    if open { p } else { p.collapsed() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stack_opens_on_top_and_pops_around_the_tour() {
        let mut l = LayoutState::default();
        l.open(Overlay::Tour);
        l.open(Overlay::Map);
        l.open(Overlay::Conversation);
        l.open(Overlay::Map);
        assert_eq!(l.overlays, [Overlay::Tour, Overlay::Conversation, Overlay::Map]);
        assert_eq!(l.top(), Some(Overlay::Map));
        assert_eq!(l.pop_layer(), Some(Overlay::Map));
        assert_eq!(l.pop_layer(), Some(Overlay::Conversation));
        assert_eq!(l.pop_layer(), None, "the tour is left by its own step");
        assert!(l.is_open(Overlay::Tour));
        assert!(!l.toggle(Overlay::Tour));
        assert!(l.overlays.is_empty());
    }

    #[test]
    fn columns_follow_the_width() {
        let d = crate::settings::Settings::default();
        assert!(
            columns_per_side(d.window.0 - d.sidebar_width - 8.0) >= SPLIT_MIN_COLUMNS,
            "the default window opens split"
        );
        assert!(columns_per_side(1920.0 - 340.0) >= SPLIT_MIN_COLUMNS);
        assert!(columns_per_side(1366.0 - 340.0) < SPLIT_MIN_COLUMNS);
        assert_eq!(columns_per_side(0.0), 0);
        assert_eq!(columns_per_side(f32::MAX), u32::MAX);
    }
}
