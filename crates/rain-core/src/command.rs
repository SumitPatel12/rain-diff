//! The commands the shell dispatches and the round-3 keymap; the palette lists them with their
//! chords, the status hints and tooltips read the same table.

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
    UndoAccept,
    ToggleSplit,
    LayoutAuto,
    ToggleReaderMode,
    ToggleList,
    Zoom,
    ToggleWholeFile,
    ExpandAllContext,
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
    pub const ALL: [Command; 51] = [
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
        Command::UndoAccept,
        Command::ToggleSplit,
        Command::LayoutAuto,
        Command::ToggleReaderMode,
        Command::ToggleList,
        Command::Zoom,
        Command::ToggleWholeFile,
        Command::ExpandAllContext,
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
        Command::Escape,
    ];

    /// Reached from the palette only.
    pub const PALETTE_ONLY: [Command; 5] = [
        Command::LayoutAuto,
        Command::CopyAll,
        Command::ToggleTheme,
        Command::GoOverview,
        Command::GoChanges,
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
            Command::UndoAccept => "Undo the last delete or accept",
            Command::ToggleSplit => "Split / unified",
            Command::LayoutAuto => "Layout: auto",
            Command::ToggleReaderMode => "Stream / file at a time",
            Command::ToggleList => "Show / hide the list",
            Command::Zoom => "Zoom / restore",
            Command::ToggleWholeFile => "Toggle whole file",
            Command::ExpandAllContext => "Expand all context",
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
            Command::Escape => "Close / clear",
            Command::NextChapter => "Next chapter",
            Command::PrevChapter => "Previous chapter",
            Command::ToggleChaptersQueue => "Switch chapters / queue",
            Command::ToggleInspector => "Toggle inspector",
            Command::Overview => "Overview diagrams",
            Command::FocusTree => "Focus file tree",
        }
    }

    /// The first chord bound to this command in [`keymap`], if any.
    pub fn chord(&self) -> Option<Chord> {
        keymap().iter().find(|(_, c)| c == self).map(|(k, _)| *k)
    }
}

/// `key` is a lowercase letter, a punctuation character as typed (`]`, `?`, `/`), or one of the
/// named keys `Space`, `Tab`, `Escape`, `Left`, `Right`, `F1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: &'static str,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    const fn new(key: &'static str, ctrl: bool, shift: bool, alt: bool) -> Chord {
        Chord { key, ctrl, shift, alt }
    }

    const fn key(key: &'static str) -> Chord {
        Chord::new(key, false, false, false)
    }

    const fn shift(key: &'static str) -> Chord {
        Chord::new(key, false, true, false)
    }

    const fn ctrl(key: &'static str) -> Chord {
        Chord::new(key, true, false, false)
    }

    const fn ctrl_shift(key: &'static str) -> Chord {
        Chord::new(key, true, true, false)
    }

    const fn alt(key: &'static str) -> Chord {
        Chord::new(key, false, false, true)
    }

    /// As shown in the palette: `J`, `Ctrl+K`, `Ctrl+Shift+L`, `Space`, `Alt+Left`.
    pub fn display(&self) -> String {
        let letter = self.key.len() == 1;
        let key = if letter && (self.shift || self.ctrl) {
            self.key.to_uppercase()
        } else {
            self.key.to_owned()
        };
        let key = if self.shift && (!letter || self.ctrl) {
            format!("Shift+{key}")
        } else {
            key
        };
        let key = if self.alt { format!("Alt+{key}") } else { key };
        if self.ctrl { format!("Ctrl+{key}") } else { key }
    }
}

static KEYMAP: [(Chord, Command); 47] = [
    (Chord::key("j"), Command::CursorDown),
    (Chord::key("k"), Command::CursorUp),
    (Chord::key("n"), Command::NextHunk),
    (Chord::key("p"), Command::PrevHunk),
    (Chord::key("]"), Command::NextFile),
    (Chord::key("["), Command::PrevFile),
    (Chord::shift("j"), Command::NextSection),
    (Chord::shift("k"), Command::PrevSection),
    (Chord::alt("Left"), Command::Back),
    (Chord::alt("Right"), Command::Forward),
    (Chord::key("w"), Command::NextWaiting),
    (Chord::key("Space"), Command::ViewedAndNext),
    (Chord::key("v"), Command::ToggleViewed),
    (Chord::shift("v"), Command::MarkGroupViewed),
    (Chord::key("u"), Command::UndoAccept),
    (Chord::key("s"), Command::ToggleSplit),
    (Chord::shift("s"), Command::ToggleReaderMode),
    (Chord::key("l"), Command::ToggleList),
    (Chord::shift("Escape"), Command::Zoom),
    (Chord::key("e"), Command::ToggleWholeFile),
    (Chord::shift("e"), Command::ExpandAllContext),
    (Chord::ctrl("l"), Command::FilterFiles),
    (Chord::key("f"), Command::FilterFiles),
    (Chord::ctrl_shift("l"), Command::ClearFilter),
    (Chord::ctrl("p"), Command::Picker),
    (Chord::ctrl("k"), Command::Palette),
    (Chord::key("/"), Command::Find),
    (Chord::key("c"), Command::Comment),
    (Chord::key("?"), Command::Ask),
    (Chord::key("r"), Command::Reply),
    (Chord::key("a"), Command::Agree),
    (Chord::key("x"), Command::Dismiss),
    (Chord::key("d"), Command::Defer),
    (Chord::shift("c"), Command::Conversation),
    (Chord::key("m"), Command::Map),
    (Chord::key("}"), Command::NextMapView),
    (Chord::key("{"), Command::PrevMapView),
    (Chord::key("b"), Command::Brief),
    (Chord::shift("t"), Command::Tour),
    (Chord::shift("h"), Command::ToggleVerify),
    (Chord::shift("f"), Command::FinishReview),
    (Chord::key("F1"), Command::KeySheet),
    (Chord::ctrl("o"), Command::OpenInEditor),
    (Chord::key("y"), Command::CopyLocation),
    (Chord::shift("r"), Command::Reload),
    (Chord::ctrl("d"), Command::ToggleDensity),
    (Chord::key("Escape"), Command::Escape),
];

pub fn keymap() -> &'static [(Chord, Command)] {
    &KEYMAP
}

/// The command bound to `key` with these modifiers and no Alt. Letters match case-insensitively;
/// shift on a punctuation key is ignored since the character already encodes it.
pub fn lookup(key: &str, ctrl: bool, shift: bool) -> Option<Command> {
    lookup_chord(key, ctrl, shift, false)
}

/// [`lookup`] with Alt.
pub fn lookup_chord(key: &str, ctrl: bool, shift: bool, alt: bool) -> Option<Command> {
    let lower = key.to_lowercase();
    let letter = lower.len() == 1 && lower.chars().all(|c| c.is_ascii_alphabetic());
    let shift = if letter { shift || key != lower } else { shift && key.len() > 1 };
    KEYMAP
        .iter()
        .find(|(c, _)| c.key.eq_ignore_ascii_case(&lower) && c.ctrl == ctrl && c.shift == shift && c.alt == alt)
        .map(|(_, cmd)| *cmd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_command_has_one_unique_chord() {
        let chords: HashSet<_> = keymap().iter().map(|(k, _)| *k).collect();
        assert_eq!(chords.len(), keymap().len(), "a chord is bound twice");
        for c in Command::ALL {
            assert!(!c.label().is_empty());
            assert!(!c.is_legacy());
            let n = keymap().iter().filter(|(_, x)| *x == c).count();
            let want = match c {
                c if Command::PALETTE_ONLY.contains(&c) => 0,
                Command::FilterFiles => 2,
                _ => 1,
            };
            assert_eq!(n, want, "{c:?} has {n} chords");
        }
        for c in Command::LEGACY {
            assert!(c.chord().is_none() && !Command::ALL.contains(&c), "{c:?}");
        }
        let labels: HashSet<_> = Command::ALL.iter().map(|c| c.label()).collect();
        assert_eq!(labels.len(), Command::ALL.len(), "labels are unique");
    }

    #[test]
    fn removed_round_two_chords_are_gone() {
        assert_eq!(lookup("i", false, false), None);
        assert_eq!(lookup("o", false, false), None);
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
        assert_eq!(lookup("w", false, false), Some(Command::NextWaiting));
        assert_eq!(lookup("m", false, false), Some(Command::Map));
        assert_eq!(lookup("b", false, false), Some(Command::Brief));
        assert_eq!(lookup("T", false, false), Some(Command::Tour));
        assert_eq!(lookup("}", false, true), Some(Command::NextMapView));
        assert_eq!(lookup("{", false, true), Some(Command::PrevMapView));
        assert_eq!(lookup("H", false, false), Some(Command::ToggleVerify));
        assert_eq!(lookup("h", false, false), None);
        assert_eq!(lookup("S", false, true), Some(Command::ToggleReaderMode));
        assert_eq!(lookup("s", false, false), Some(Command::ToggleSplit));
        assert_eq!(lookup("l", false, false), Some(Command::ToggleList));
        assert_eq!(lookup("f", false, false), Some(Command::FilterFiles));
        assert_eq!(lookup("l", true, false), Some(Command::FilterFiles));
        assert_eq!(lookup("p", true, false), Some(Command::Picker));
        assert_eq!(lookup("u", false, false), Some(Command::UndoAccept));
        assert_eq!(lookup("V", false, true), Some(Command::MarkGroupViewed));
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
    fn chords_display_as_the_palette_shows_them() {
        assert_eq!(Chord::ctrl("k").display(), "Ctrl+K");
        assert_eq!(Chord::shift("v").display(), "V");
        assert_eq!(Command::FilterFiles.chord().unwrap().display(), "Ctrl+L");
        assert_eq!(Command::ClearFilter.chord().unwrap().display(), "Ctrl+Shift+L");
        assert_eq!(Command::Zoom.chord().unwrap().display(), "Shift+Escape");
        assert_eq!(Command::Back.chord().unwrap().display(), "Alt+Left");
        assert_eq!(Command::OpenInEditor.chord().unwrap().display(), "Ctrl+O");
        assert_eq!(Command::KeySheet.chord().unwrap().display(), "F1");
    }
}
