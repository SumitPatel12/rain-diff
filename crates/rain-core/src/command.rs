//! The commands the shell dispatches; [`crate::keymap`] binds them to chords, and the palette,
//! status hints and tooltips read the active keymap through [`Command::chord`].

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    CursorDown,
    CursorUp,
    NextHunk,
    PrevHunk,
    NextFile,
    PrevFile,
    NextSection,
    PrevSection,
    Back,
    Forward,
    NextWaiting,
    ViewedAndNext,
    ToggleViewed,
    MarkGroupViewed,
    MarkAllViewed,
    UndoAccept,
    ToggleSplit,
    LayoutAuto,
    ToggleReaderMode,
    ToggleList,
    Zoom,
    ToggleWholeFile,
    ExpandAllContext,
    CollapseAllContext,
    CollapseAllFiles,
    ExpandAllFiles,
    FilterFiles,
    ClearFilter,
    Picker,
    Palette,
    Find,
    Comment,
    Ask,
    Reply,
    Agree,
    Dismiss,
    Defer,
    Conversation,
    Map,
    NextMapView,
    PrevMapView,
    Brief,
    GoOverview,
    GoChanges,
    Tour,
    ToggleVerify,
    FinishReview,
    KeySheet,
    OpenInEditor,
    CopyLocation,
    CopyAll,
    Reload,
    ToggleTheme,
    ToggleDensity,
    DiagramPlayground,
    Escape,
    /// Round-2 commands with no chord and no place in [`Command::ALL`]; the round-2 shell still
    /// dispatches them until the round-3 shell replaces it.
    NextChapter,
    PrevChapter,
    ToggleChaptersQueue,
    ToggleInspector,
    Overview,
    FocusTree,
}

impl Command {
    pub const ALL: [Command; 56] = [
        Command::CursorDown,
        Command::CursorUp,
        Command::NextHunk,
        Command::PrevHunk,
        Command::NextFile,
        Command::PrevFile,
        Command::NextSection,
        Command::PrevSection,
        Command::Back,
        Command::Forward,
        Command::NextWaiting,
        Command::ViewedAndNext,
        Command::ToggleViewed,
        Command::MarkGroupViewed,
        Command::MarkAllViewed,
        Command::UndoAccept,
        Command::ToggleSplit,
        Command::LayoutAuto,
        Command::ToggleReaderMode,
        Command::ToggleList,
        Command::Zoom,
        Command::ToggleWholeFile,
        Command::ExpandAllContext,
        Command::CollapseAllContext,
        Command::CollapseAllFiles,
        Command::ExpandAllFiles,
        Command::FilterFiles,
        Command::ClearFilter,
        Command::Picker,
        Command::Palette,
        Command::Find,
        Command::Comment,
        Command::Ask,
        Command::Reply,
        Command::Agree,
        Command::Dismiss,
        Command::Defer,
        Command::Conversation,
        Command::Map,
        Command::NextMapView,
        Command::PrevMapView,
        Command::Brief,
        Command::GoOverview,
        Command::GoChanges,
        Command::Tour,
        Command::ToggleVerify,
        Command::FinishReview,
        Command::KeySheet,
        Command::OpenInEditor,
        Command::CopyLocation,
        Command::CopyAll,
        Command::Reload,
        Command::ToggleTheme,
        Command::ToggleDensity,
        Command::DiagramPlayground,
        Command::Escape,
    ];

    /// No key in any profile until the user binds one.
    pub const PALETTE_ONLY: [Command; 7] = [
        Command::MarkAllViewed,
        Command::LayoutAuto,
        Command::CopyAll,
        Command::ToggleTheme,
        Command::GoOverview,
        Command::GoChanges,
        Command::DiagramPlayground,
    ];

    pub const LEGACY: [Command; 6] = [
        Command::NextChapter,
        Command::PrevChapter,
        Command::ToggleChaptersQueue,
        Command::ToggleInspector,
        Command::Overview,
        Command::FocusTree,
    ];

    pub fn is_legacy(&self) -> bool {
        Command::LEGACY.contains(self)
    }

    pub fn label(&self) -> &'static str {
        match self {
            Command::CursorDown => "Next line",
            Command::CursorUp => "Previous line",
            Command::NextHunk => "Next hunk",
            Command::PrevHunk => "Previous hunk",
            Command::NextFile => "Next file",
            Command::PrevFile => "Previous file",
            Command::NextSection => "Next list section",
            Command::PrevSection => "Previous list section",
            Command::Back => "Back",
            Command::Forward => "Forward",
            Command::NextWaiting => "Next waiting on you",
            Command::ViewedAndNext => "Mark viewed and go to next",
            Command::ToggleViewed => "Toggle viewed",
            Command::MarkGroupViewed => "Accept this group",
            Command::MarkAllViewed => "Mark all files viewed",
            Command::UndoAccept => "Undo the last delete or accept",
            Command::ToggleSplit => "Split / unified",
            Command::LayoutAuto => "Layout: auto",
            Command::ToggleReaderMode => "Stream / file at a time",
            Command::ToggleList => "Show / hide the list",
            Command::Zoom => "Zoom / restore",
            Command::ToggleWholeFile => "Toggle whole file",
            Command::ExpandAllContext => "Expand all context",
            Command::CollapseAllContext => "Collapse all context",
            Command::CollapseAllFiles => "Collapse all files",
            Command::ExpandAllFiles => "Expand all files",
            Command::FilterFiles => "Filter files",
            Command::ClearFilter => "Clear file filter",
            Command::Picker => "Go to file",
            Command::Palette => "Command palette",
            Command::Find => "Find",
            Command::Comment => "Comment",
            Command::Ask => "Ask a question",
            Command::Reply => "Reply",
            Command::Agree => "Agree",
            Command::Dismiss => "Dismiss",
            Command::Defer => "Defer",
            Command::Conversation => "Conversation",
            Command::Map => "Map",
            Command::NextMapView => "Next map view",
            Command::PrevMapView => "Previous map view",
            Command::Brief => "Overview / Changes",
            Command::GoOverview => "Go to Overview",
            Command::GoChanges => "Go to Changes",
            Command::Tour => "Tour",
            Command::ToggleVerify => "Verify by hand: fold / unfold",
            Command::FinishReview => "Finish review",
            Command::KeySheet => "Keyboard shortcuts",
            Command::OpenInEditor => "Open in editor",
            Command::CopyLocation => "Copy location (path:line)",
            Command::CopyAll => "Copy all comments",
            Command::Reload => "Reload diff",
            Command::ToggleTheme => "Toggle theme",
            Command::ToggleDensity => "Toggle density",
            Command::DiagramPlayground => "Diagram playground",
            Command::Escape => "Close / clear",
            Command::NextChapter => "Next chapter",
            Command::PrevChapter => "Previous chapter",
            Command::ToggleChaptersQueue => "Switch chapters / queue",
            Command::ToggleInspector => "Toggle inspector",
            Command::Overview => "Overview diagrams",
            Command::FocusTree => "Focus file tree",
        }
    }

    /// The first chord bound to this command in the active keymap, if any.
    pub fn chord(&self) -> Option<Chord> {
        crate::keymap::with_active(|k| k.chord(*self))
    }

    /// Its first chord as hints show it; empty when it has none.
    pub fn hint(&self) -> String {
        self.chord().map(|c| c.display()).unwrap_or_default()
    }

    /// The stable name settings and overrides use: `next-hunk`, `palette`.
    pub fn id(&self) -> &'static str {
        match self {
            Command::CursorDown => "cursor-down",
            Command::CursorUp => "cursor-up",
            Command::NextHunk => "next-hunk",
            Command::PrevHunk => "prev-hunk",
            Command::NextFile => "next-file",
            Command::PrevFile => "prev-file",
            Command::NextSection => "next-section",
            Command::PrevSection => "prev-section",
            Command::Back => "back",
            Command::Forward => "forward",
            Command::NextWaiting => "next-waiting",
            Command::ViewedAndNext => "viewed-and-next",
            Command::ToggleViewed => "toggle-viewed",
            Command::MarkGroupViewed => "accept-group",
            Command::MarkAllViewed => "mark-all-viewed",
            Command::UndoAccept => "undo",
            Command::ToggleSplit => "toggle-split",
            Command::LayoutAuto => "layout-auto",
            Command::ToggleReaderMode => "toggle-reader-mode",
            Command::ToggleList => "toggle-list",
            Command::Zoom => "zoom",
            Command::ToggleWholeFile => "toggle-whole-file",
            Command::ExpandAllContext => "expand-all-context",
            Command::CollapseAllContext => "collapse-all-context",
            Command::CollapseAllFiles => "collapse-all-files",
            Command::ExpandAllFiles => "expand-all-files",
            Command::FilterFiles => "filter-files",
            Command::ClearFilter => "clear-filter",
            Command::Picker => "go-to-file",
            Command::Palette => "palette",
            Command::Find => "find",
            Command::Comment => "comment",
            Command::Ask => "ask",
            Command::Reply => "reply",
            Command::Agree => "agree",
            Command::Dismiss => "dismiss",
            Command::Defer => "defer",
            Command::Conversation => "conversation",
            Command::Map => "map",
            Command::NextMapView => "next-map-view",
            Command::PrevMapView => "prev-map-view",
            Command::Brief => "toggle-overview",
            Command::GoOverview => "go-overview",
            Command::GoChanges => "go-changes",
            Command::Tour => "tour",
            Command::ToggleVerify => "toggle-verify",
            Command::FinishReview => "finish-review",
            Command::KeySheet => "keyboard-shortcuts",
            Command::OpenInEditor => "open-in-editor",
            Command::CopyLocation => "copy-location",
            Command::CopyAll => "copy-all-comments",
            Command::Reload => "reload",
            Command::ToggleTheme => "toggle-theme",
            Command::ToggleDensity => "toggle-density",
            Command::DiagramPlayground => "diagram-playground",
            Command::Escape => "escape",
            Command::NextChapter => "next-chapter",
            Command::PrevChapter => "prev-chapter",
            Command::ToggleChaptersQueue => "toggle-chapters-queue",
            Command::ToggleInspector => "toggle-inspector",
            Command::Overview => "overview",
            Command::FocusTree => "focus-tree",
        }
    }

    /// The command [`Command::id`] names, among the ones a key can run.
    pub fn from_id(id: &str) -> Option<Command> {
        Command::ALL.into_iter().find(|c| c.id() == id)
    }
}

pub use crate::keymap::Chord;

/// The command the active keymap binds to `key` with these modifiers and no Alt. Letters match
/// case-insensitively; shift on a punctuation key is ignored since the character already
/// encodes it.
pub fn lookup(key: &str, ctrl: bool, shift: bool) -> Option<Command> {
    lookup_chord(key, ctrl, shift, false)
}

/// [`lookup`] with Alt; `key` is a character or a key name (`Space`, `Left`, `F1`).
pub fn lookup_chord(key: &str, ctrl: bool, shift: bool, alt: bool) -> Option<Command> {
    let mut spec = String::new();
    for (on, m) in [(ctrl, "Ctrl+"), (shift, "Shift+"), (alt, "Alt+")] {
        if on {
            spec.push_str(m);
        }
    }
    spec.push_str(key);
    let chord = Chord::parse(&spec)?;
    crate::keymap::with_active(|k| k.lookup(&chord))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_command_has_a_vim_chord_and_a_unique_id() {
        let vim = crate::keymap::Keymap::defaults(crate::keymap::Profile::Vim);
        for c in Command::ALL {
            assert!(!c.label().is_empty());
            assert!(!c.is_legacy());
            let n = vim.chords(c).len();
            let want = match c {
                _ if Command::PALETTE_ONLY.contains(&c) => 0,
                Command::KeySheet => 2,
                _ => 1,
            };
            assert_eq!(n, want, "{c:?} has {n} chords");
            assert_eq!(Command::from_id(c.id()), Some(c));
        }
        let ids: HashSet<_> = Command::ALL.iter().chain(&Command::LEGACY).map(|c| c.id()).collect();
        assert_eq!(ids.len(), Command::ALL.len() + Command::LEGACY.len(), "ids are unique");
        for c in Command::LEGACY {
            assert!(c.chord().is_none() && !Command::ALL.contains(&c), "{c:?}");
        }
        let labels: HashSet<_> = Command::ALL.iter().map(|c| c.label()).collect();
        assert_eq!(labels.len(), Command::ALL.len(), "labels are unique");
    }

    #[test]
    fn the_diagram_playground_has_a_stable_id_and_no_key_in_any_profile() {
        assert_eq!(Command::from_id("diagram-playground"), Some(Command::DiagramPlayground));
        for p in crate::keymap::Profile::ALL {
            assert!(
                crate::keymap::Keymap::defaults(p).chords(Command::DiagramPlayground).is_empty(),
                "{p:?}"
            );
        }
    }

    #[test]
    fn removed_round_two_chords_are_gone() {
        assert_eq!(lookup("Tab", false, false), None);
        assert_eq!(lookup("t", false, false), None);
        assert_eq!(lookup("C", false, true), Some(Command::Conversation));
        assert_eq!(lookup("F", false, true), Some(Command::FinishReview));
        assert_eq!(lookup("J", false, true), Some(Command::NextSection));
        assert!(Command::CopyAll.chord().is_none() && Command::ToggleTheme.chord().is_none());
    }

    #[test]
    fn round_three_chords() {
        assert_eq!(lookup("C", false, true), Some(Command::Conversation));
        assert_eq!(lookup("c", false, false), Some(Command::Comment));
        assert_eq!(lookup("?", false, true), Some(Command::Ask));
        assert_eq!(lookup("?", false, false), Some(Command::Ask));
        assert_eq!(lookup("i", false, false), Some(Command::NextWaiting));
        assert_eq!(lookup("m", false, false), Some(Command::Map));
        assert_eq!(lookup("o", false, false), Some(Command::Brief));
        assert_eq!(lookup("T", false, false), Some(Command::Tour));
        assert_eq!(lookup("}", false, true), Some(Command::NextMapView));
        assert_eq!(lookup("{", false, true), Some(Command::PrevMapView));
        assert_eq!(lookup("H", false, false), Some(Command::ToggleVerify));
        assert_eq!(lookup("h", false, false), None);
        assert_eq!(lookup("S", false, true), Some(Command::ToggleReaderMode));
        assert_eq!(lookup("s", false, false), Some(Command::ToggleSplit));
        assert_eq!(lookup("b", true, false), Some(Command::ToggleList));
        assert_eq!(lookup("l", true, false), Some(Command::FilterFiles));
        assert_eq!(lookup("p", true, false), Some(Command::Picker));
        assert_eq!(lookup("u", false, false), Some(Command::UndoAccept));
        assert_eq!(lookup("A", false, true), Some(Command::MarkGroupViewed));
        assert_eq!(lookup("Escape", false, true), Some(Command::Zoom));
        assert_eq!(lookup("Escape", false, false), Some(Command::Escape));
        assert_eq!(lookup("F1", false, false), Some(Command::KeySheet));
        assert_eq!(lookup_chord("Left", false, false, true), Some(Command::Back));
        assert_eq!(lookup_chord("Right", false, false, true), Some(Command::Forward));
        assert_eq!(lookup("Left", false, false), None);
        assert_eq!(lookup("j", false, false), Some(Command::CursorDown));
        assert_eq!(lookup("k", true, false), Some(Command::Palette));
        assert_eq!(lookup("Space", false, false), Some(Command::ViewedAndNext));
        assert_eq!(lookup("q", false, false), None);
    }

    #[test]
    fn vim_keys_are_free_and_the_commands_they_displaced_moved() {
        for k in [
            "h", "l", "w", "b", "e", "W", "B", "g", "G", "f", "t", "v", "V", "y", "$", "^", ";", ",", "0", "5",
        ] {
            let shift = k.chars().all(|c| c.is_ascii_uppercase());
            assert_eq!(lookup(k, false, shift), None, "{k} is Vim's");
        }
        assert_eq!(lookup("u", true, false), None, "Ctrl+U is Vim's");
        assert_eq!(lookup("d", true, false), None, "Ctrl+D is Vim's");
        for (key, ctrl, shift, cmd) in [
            ("i", false, false, Command::NextWaiting),
            ("o", false, false, Command::Brief),
            ("z", false, false, Command::ToggleWholeFile),
            ("b", true, false, Command::ToggleList),
            ("l", true, false, Command::FilterFiles),
            ("d", true, true, Command::ToggleDensity),
            ("Space", false, true, Command::ToggleViewed),
            ("A", false, true, Command::MarkGroupViewed),
            ("Y", false, true, Command::CopyLocation),
        ] {
            assert_eq!(lookup(key, ctrl, shift), Some(cmd), "{key}");
            assert_eq!(cmd.chord().map(|c| (c.ctrl, c.shift)), Some((ctrl, shift)), "{cmd:?}");
        }
        assert_eq!(Command::ToggleViewed.chord().unwrap().display(), "Shift+Space");
        assert_eq!(Command::ToggleDensity.chord().unwrap().display(), "Ctrl+Shift+D");
        assert_eq!(Command::ToggleList.chord().unwrap().display(), "Ctrl+B");
        assert_eq!(Command::CopyLocation.chord().unwrap().display(), "Y");
        assert_eq!(Command::MarkGroupViewed.chord().unwrap().display(), "A");
    }

    #[test]
    fn chords_display_as_the_palette_shows_them() {
        assert_eq!(Chord::parse("Ctrl+k").unwrap().display(), "Ctrl+K");
        assert_eq!(Chord::parse("Shift+v").unwrap().display(), "V");
        assert_eq!(Command::FilterFiles.chord().unwrap().display(), "Ctrl+L");
        assert_eq!(Command::ClearFilter.chord().unwrap().display(), "Ctrl+Shift+L");
        assert_eq!(Command::Zoom.chord().unwrap().display(), "Shift+Escape");
        assert_eq!(Command::Back.chord().unwrap().display(), "Alt+Left");
        assert_eq!(Command::OpenInEditor.chord().unwrap().display(), "Ctrl+O");
        assert_eq!(Command::KeySheet.chord().unwrap().display(), "F1");
    }
}
