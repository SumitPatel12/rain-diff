//! Key chords, the three built-in shortcut profiles and the keymap in force: a profile's
//! defaults with the user's overrides over them. The window installs it with [`set_active`];
//! a key press is one hash lookup, every hint reads the same map.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::command::Command;

/// A key as a chord names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChordKey {
    /// A character as typed: letters lowercase, punctuation as it arrives (`?`, `]`).
    Char(char),
    Space,
    Tab,
    Enter,
    Escape,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
}

const NAMED: [(ChordKey, &str); 15] = [
    (ChordKey::Space, "Space"),
    (ChordKey::Tab, "Tab"),
    (ChordKey::Enter, "Enter"),
    (ChordKey::Escape, "Escape"),
    (ChordKey::Backspace, "Backspace"),
    (ChordKey::Delete, "Delete"),
    (ChordKey::Left, "Left"),
    (ChordKey::Right, "Right"),
    (ChordKey::Up, "Up"),
    (ChordKey::Down, "Down"),
    (ChordKey::Home, "Home"),
    (ChordKey::End, "End"),
    (ChordKey::PageUp, "PageUp"),
    (ChordKey::PageDown, "PageDown"),
    (ChordKey::Escape, "Esc"),
];

impl ChordKey {
    fn name(self) -> Option<&'static str> {
        NAMED.iter().find(|(k, _)| *k == self).map(|(_, n)| *n)
    }

    fn parse(s: &str) -> Option<ChordKey> {
        let mut chars = s.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            return (!c.is_whitespace() && !c.is_control()).then_some(ChordKey::Char(c));
        }
        if let Some(n) = s.strip_prefix(['F', 'f']).and_then(|n| n.parse::<u8>().ok()) {
            return (1..=24).contains(&n).then_some(ChordKey::F(n));
        }
        let alias = match s.to_ascii_lowercase().as_str() {
            "return" => Some(ChordKey::Enter),
            "del" => Some(ChordKey::Delete),
            "pgup" => Some(ChordKey::PageUp),
            "pgdn" | "pgdown" => Some(ChordKey::PageDown),
            _ => None,
        };
        alias.or_else(|| NAMED.iter().find(|(_, n)| n.eq_ignore_ascii_case(s)).map(|(k, _)| *k))
    }
}

/// US-layout punctuation and digits with the character Shift makes of each.
const SHIFTED: [(char, char); 21] = [
    ('`', '~'),
    ('1', '!'),
    ('2', '@'),
    ('3', '#'),
    ('4', '$'),
    ('5', '%'),
    ('6', '^'),
    ('7', '&'),
    ('8', '*'),
    ('9', '('),
    ('0', ')'),
    ('-', '_'),
    ('=', '+'),
    ('[', '{'),
    (']', '}'),
    ('\\', '|'),
    (';', ':'),
    ('\'', '"'),
    (',', '<'),
    ('.', '>'),
    ('/', '?'),
];

/// A key with its modifiers. Letters are stored lowercase with Shift; Shift on any other
/// character is dropped, since the character already says it (`?` is Shift+/).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub key: ChordKey,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Chord {
    pub fn new(key: ChordKey, ctrl: bool, shift: bool, alt: bool) -> Chord {
        match key {
            ChordKey::Char(c) if c.is_alphabetic() => {
                let lower = c.to_lowercase().next().unwrap_or(c);
                Chord {
                    key: ChordKey::Char(lower),
                    ctrl,
                    shift: shift || lower != c,
                    alt,
                }
            }
            ChordKey::Char(_) => Chord {
                key,
                ctrl,
                shift: false,
                alt,
            },
            _ => Chord { key, ctrl, shift, alt },
        }
    }

    pub fn plain(key: ChordKey) -> Chord {
        Chord::new(key, false, false, false)
    }

    /// `Ctrl+Shift+L`, `J` (Shift+j), `?`, `Alt+Left`, `F7`; `Cmd` reads as Ctrl. `None` for
    /// anything else.
    pub fn parse(s: &str) -> Option<Chord> {
        let s = s.trim();
        let (mods, key) = match s.strip_suffix("++") {
            Some(head) => (head, "+"),
            None if s == "+" => ("", "+"),
            None => s.rsplit_once('+').unwrap_or(("", s)),
        };
        let (mut ctrl, mut shift, mut alt) = (false, false, false);
        for m in mods.split('+').filter(|m| !m.is_empty()) {
            match m.trim().to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" | "command" => ctrl = true,
                "shift" => shift = true,
                "alt" | "option" | "opt" => alt = true,
                _ => return None,
            }
        }
        let shifted = |c: char| SHIFTED.iter().find(|(b, _)| *b == c).map(|(_, up)| *up);
        let key = match ChordKey::parse(key.trim())? {
            // `Ctrl+Shift+-` arrives as `_`, as Shift types it on a US layout.
            ChordKey::Char(c) if (ctrl || alt) && shift && shifted(c).is_some() => ChordKey::Char(shifted(c)?),
            // `Ctrl+K` is written upper case; only a bare `K` means Shift.
            ChordKey::Char(c) if ctrl || alt => ChordKey::Char(c.to_lowercase().next().unwrap_or(c)),
            k => k,
        };
        Some(Chord::new(key, ctrl, shift, alt))
    }

    /// As every hint shows it: `J`, `Ctrl+K`, `Ctrl+Shift+L`, `Space`, `Alt+Left`, `?`, and a
    /// shifted character after Ctrl or Alt as its key: `Ctrl+Shift+-`.
    pub fn display(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push_str("Ctrl+");
        }
        if self.alt {
            out.push_str("Alt+");
        }
        let letter = matches!(self.key, ChordKey::Char(c) if c.is_alphabetic());
        let base = match self.key {
            ChordKey::Char(c) if self.ctrl || self.alt => SHIFTED.iter().find(|(_, up)| *up == c).map(|(b, _)| *b),
            _ => None,
        };
        if (self.shift && (!letter || self.ctrl || self.alt)) || base.is_some() {
            out.push_str("Shift+");
        }
        if let Some(b) = base {
            out.push(b);
            return out;
        }
        match self.key {
            ChordKey::Char(c) if letter && (self.shift || self.ctrl || self.alt) => out.extend(c.to_uppercase()),
            ChordKey::Char(c) => out.push(c),
            ChordKey::F(n) => {
                out.push('F');
                out.push_str(&n.to_string());
            }
            k => out.push_str(k.name().unwrap_or("?")),
        }
        out
    }

    /// What this chord does in every profile, when it is one of the keys no profile or
    /// override takes: the cursor and selection keys, copy, select all, Escape and Enter.
    pub fn universal(&self) -> Option<&'static str> {
        use ChordKey::*;
        let (c, s, a) = (self.ctrl, self.shift, self.alt);
        if a {
            return None;
        }
        Some(match self.key {
            Left | Right if c => "Move by word (Shift extends the selection)",
            Left | Right => "Move by character (Shift extends the selection)",
            Up | Down if !c => "Move by line (Shift extends the selection)",
            Home | End if c => "First / last line (Shift extends the selection)",
            Home | End => "Line start / end (Shift extends the selection)",
            PageUp | PageDown if !c => "Page up / down (Shift extends the selection)",
            Char('c') if c && !s => "Copy the selection, or the line",
            Char('a') if c && !s => "Select the file's lines",
            Escape if !c && !s => "Clear the selection, close the top layer",
            Enter if !c && !s => "Open the focused file or item",
            _ => return None,
        })
    }

    /// The command a universal chord already does, which a profile may name it for.
    pub fn universal_command(&self) -> Option<Command> {
        if self.ctrl || self.shift || self.alt {
            return None;
        }
        match self.key {
            ChordKey::Down => Some(Command::CursorDown),
            ChordKey::Up => Some(Command::CursorUp),
            ChordKey::Escape => Some(Command::Escape),
            _ => None,
        }
    }
}

/// The universal keys as the shortcuts screen and the docs list them.
pub const UNIVERSAL: [(&str, &str); 11] = [
    ("Up / Down", "Move the cursor by line, keeping its column"),
    ("Left / Right", "Move the cursor by character"),
    ("Ctrl+Left / Ctrl+Right", "Move the cursor by word"),
    ("Home / End", "First non-blank, then column 0 / line end"),
    ("Ctrl+Home / Ctrl+End", "The file's first / last line"),
    ("PageUp / PageDown", "A page up / down, the cursor with it"),
    ("Shift + any of these", "Extend the selection"),
    ("Ctrl+C", "Copy the selection, or the cursor's line"),
    ("Ctrl+A", "Select the current file's lines"),
    ("Escape", "Clear the selection, close the top layer"),
    ("Enter", "Open the focused file or item"),
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    #[default]
    Vim,
    #[serde(rename = "vscode")]
    VsCode,
    JetBrains,
}

impl Profile {
    pub const ALL: [Profile; 3] = [Profile::Vim, Profile::VsCode, Profile::JetBrains];

    pub fn label(self) -> &'static str {
        match self {
            Profile::Vim => "Vim",
            Profile::VsCode => "VS Code",
            Profile::JetBrains => "JetBrains",
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Profile::Vim => "vim",
            Profile::VsCode => "vscode",
            Profile::JetBrains => "jetbrains",
        }
    }

    /// Whether Vim's motions and visual mode (`hjkl`, `w`, `gg`, `v`, ...) drive the diff.
    pub fn vim_motions(self) -> bool {
        self == Profile::Vim
    }

    /// The profile's chords, as `Chord::parse` reads them.
    pub fn defaults(self) -> &'static [(&'static str, Command)] {
        match self {
            Profile::Vim => &VIM,
            Profile::VsCode => &VS_CODE,
            Profile::JetBrains => &JETBRAINS,
        }
    }
}

use Command as C;

/// Leaves `h l w b e W B 0 ^ $ g G f t ; , v V y` and digits to Vim (`DiffState::vim`).
static VIM: [(&str, Command); 50] = [
    ("j", C::CursorDown),
    ("k", C::CursorUp),
    ("n", C::NextHunk),
    ("p", C::PrevHunk),
    ("]", C::NextFile),
    ("[", C::PrevFile),
    ("J", C::NextSection),
    ("K", C::PrevSection),
    ("Alt+Left", C::Back),
    ("Alt+Right", C::Forward),
    ("i", C::NextWaiting),
    ("Space", C::ViewedAndNext),
    ("Shift+Space", C::ToggleViewed),
    ("A", C::MarkGroupViewed),
    ("u", C::UndoAccept),
    ("s", C::ToggleSplit),
    ("S", C::ToggleReaderMode),
    ("Ctrl+B", C::ToggleList),
    ("Shift+Escape", C::Zoom),
    ("z", C::ToggleWholeFile),
    ("E", C::ExpandAllContext),
    ("Z", C::CollapseAllContext),
    ("Ctrl+Shift+-", C::CollapseAllFiles),
    ("Ctrl+Shift+=", C::ExpandAllFiles),
    ("Ctrl+L", C::FilterFiles),
    ("Ctrl+Shift+L", C::ClearFilter),
    ("Ctrl+P", C::Picker),
    ("Ctrl+K", C::Palette),
    ("/", C::Find),
    ("c", C::Comment),
    ("?", C::Ask),
    ("r", C::Reply),
    ("a", C::Agree),
    ("x", C::Dismiss),
    ("d", C::Defer),
    ("C", C::Conversation),
    ("m", C::Map),
    ("}", C::NextMapView),
    ("{", C::PrevMapView),
    ("o", C::Brief),
    ("T", C::Tour),
    ("H", C::ToggleVerify),
    ("F", C::FinishReview),
    ("F1", C::KeySheet),
    ("Ctrl+,", C::KeySheet),
    ("Ctrl+O", C::OpenInEditor),
    ("Y", C::CopyLocation),
    ("R", C::Reload),
    ("Ctrl+Shift+D", C::ToggleDensity),
    ("Escape", C::Escape),
];

/// VS Code's own keys where it has one (Ctrl+P, Ctrl+Shift+P, Alt+F5, Ctrl+PageDown, ...);
/// no bare letters, so nothing reads as typing. Fold All / Unfold All are chord sequences
/// there (Ctrl+K Ctrl+0 / Ctrl+K Ctrl+J), so their files take Fold / Unfold's Ctrl+Shift+[ / ].
static VS_CODE: [(&str, Command); 32] = [
    ("Down", C::CursorDown),
    ("Up", C::CursorUp),
    ("Alt+F5", C::NextHunk),
    ("Alt+Shift+F5", C::PrevHunk),
    ("Ctrl+PageDown", C::NextFile),
    ("Ctrl+PageUp", C::PrevFile),
    ("Ctrl+Shift+PageDown", C::NextSection),
    ("Ctrl+Shift+PageUp", C::PrevSection),
    ("Alt+Left", C::Back),
    ("Alt+Right", C::Forward),
    ("F8", C::NextWaiting),
    ("Ctrl+Enter", C::ViewedAndNext),
    ("Ctrl+Shift+Enter", C::ToggleViewed),
    ("Ctrl+Z", C::UndoAccept),
    ("Ctrl+B", C::ToggleList),
    ("Shift+Escape", C::Zoom),
    ("Ctrl+Shift+[", C::CollapseAllFiles),
    ("Ctrl+Shift+]", C::ExpandAllFiles),
    ("Ctrl+Shift+E", C::FilterFiles),
    ("Ctrl+P", C::Picker),
    ("Ctrl+Shift+P", C::Palette),
    ("F1", C::Palette),
    ("Ctrl+F", C::Find),
    ("Ctrl+/", C::Comment),
    ("Ctrl+I", C::Ask),
    ("Ctrl+Shift+C", C::Conversation),
    ("Ctrl+Tab", C::Brief),
    ("Ctrl+,", C::KeySheet),
    ("Ctrl+O", C::OpenInEditor),
    ("Alt+Shift+C", C::CopyLocation),
    ("F5", C::Reload),
    ("Escape", C::Escape),
];

/// IntelliJ's Windows/Linux keymap: F7 / Shift+F7 differences, Alt+Left/Right files in the
/// diff viewer, Ctrl+Alt+Left/Right back and forward, F4 jump to source, Ctrl+K commit,
/// Ctrl+Shift+Minus / Equal collapse and expand all.
static JETBRAINS: [(&str, Command); 32] = [
    ("Down", C::CursorDown),
    ("Up", C::CursorUp),
    ("F7", C::NextHunk),
    ("Shift+F7", C::PrevHunk),
    ("Alt+Right", C::NextFile),
    ("Alt+Left", C::PrevFile),
    ("Ctrl+Shift+PageDown", C::NextSection),
    ("Ctrl+Shift+PageUp", C::PrevSection),
    ("Ctrl+Alt+Left", C::Back),
    ("Ctrl+Alt+Right", C::Forward),
    ("F2", C::NextWaiting),
    ("Ctrl+Enter", C::ViewedAndNext),
    ("Ctrl+Shift+Enter", C::ToggleViewed),
    ("Ctrl+Z", C::UndoAccept),
    ("Alt+1", C::ToggleList),
    ("Ctrl+Shift+F12", C::Zoom),
    ("Ctrl+Shift+-", C::CollapseAllFiles),
    ("Ctrl+Shift+=", C::ExpandAllFiles),
    ("Ctrl+L", C::FilterFiles),
    ("Ctrl+Shift+N", C::Picker),
    ("Ctrl+Shift+A", C::Palette),
    ("Ctrl+F", C::Find),
    ("Ctrl+/", C::Comment),
    ("Ctrl+I", C::Ask),
    ("Ctrl+Shift+C", C::Conversation),
    ("Ctrl+Tab", C::Brief),
    ("Ctrl+K", C::FinishReview),
    ("Ctrl+Alt+S", C::KeySheet),
    ("F4", C::OpenInEditor),
    ("Ctrl+Alt+Shift+C", C::CopyLocation),
    ("Ctrl+Alt+Y", C::Reload),
    ("Escape", C::Escape),
];

/// A profile's defaults with overrides over them, indexed both ways.
#[derive(Clone, Debug)]
pub struct Keymap {
    profile: Profile,
    by_chord: HashMap<Chord, Command>,
    by_cmd: HashMap<Command, Vec<Chord>>,
}

impl Keymap {
    pub fn defaults(profile: Profile) -> Keymap {
        Keymap::with_overrides(profile, &BTreeMap::new()).0
    }

    /// The profile's chords with `overrides` (command id to chords; empty unbinds) over them,
    /// and a line for each entry it ignored: an unknown command or key, a key already taken by
    /// another override, or a universal key named for another command.
    pub fn with_overrides(profile: Profile, overrides: &BTreeMap<String, Vec<String>>) -> (Keymap, Vec<String>) {
        let mut problems = Vec::new();
        let mut km = Keymap {
            profile,
            by_chord: HashMap::new(),
            by_cmd: HashMap::new(),
        };
        for (spec, cmd) in profile.defaults() {
            if let Some(ch) = Chord::parse(spec) {
                km.bind(ch, *cmd);
            }
        }
        let mut claimed: HashMap<Chord, Command> = HashMap::new();
        for (id, specs) in overrides {
            let Some(cmd) = Command::from_id(id) else {
                problems.push(format!("unknown command `{id}`"));
                continue;
            };
            km.unbind_all(cmd);
            for spec in specs {
                let Some(ch) = Chord::parse(spec) else {
                    problems.push(format!("`{spec}` for {id} is not a key"));
                    continue;
                };
                if ch.universal().is_some() && ch.universal_command() != Some(cmd) {
                    problems.push(format!("{} is a universal key; not bound to {id}", ch.display()));
                    continue;
                }
                if let Some(other) = claimed.get(&ch).filter(|o| **o != cmd) {
                    problems.push(format!("{} is already bound to {}; not bound to {id}", ch.display(), other.id()));
                    continue;
                }
                claimed.insert(ch, cmd);
                km.bind(ch, cmd);
            }
        }
        // A universal key keeps its command's hint even when an override emptied it.
        for (spec, cmd) in profile.defaults() {
            if let Some(ch) = Chord::parse(spec).filter(|c| c.universal_command() == Some(*cmd)) {
                km.bind(ch, *cmd);
            }
        }
        (km, problems)
    }

    /// Binds `chord` to `cmd`, taking it from whatever had it.
    fn bind(&mut self, chord: Chord, cmd: Command) {
        if let Some(old) = self.by_chord.insert(chord, cmd)
            && let Some(v) = self.by_cmd.get_mut(&old)
        {
            v.retain(|c| *c != chord);
        }
        let v = self.by_cmd.entry(cmd).or_default();
        if !v.contains(&chord) {
            v.push(chord);
        }
    }

    fn unbind_all(&mut self, cmd: Command) {
        for ch in self.by_cmd.remove(&cmd).unwrap_or_default() {
            self.by_chord.remove(&ch);
        }
    }

    pub fn profile(&self) -> Profile {
        self.profile
    }

    pub fn lookup(&self, chord: &Chord) -> Option<Command> {
        self.by_chord.get(chord).copied()
    }

    pub fn chords(&self, cmd: Command) -> &[Chord] {
        self.by_cmd.get(&cmd).map_or(&[], Vec::as_slice)
    }

    pub fn chord(&self, cmd: Command) -> Option<Chord> {
        self.chords(cmd).first().copied()
    }

    /// The first chord's display, empty when the command has none.
    pub fn hint(&self, cmd: Command) -> String {
        self.chord(cmd).map(|c| c.display()).unwrap_or_default()
    }

    /// Every binding, for tests and the shortcuts screen.
    pub fn bindings(&self) -> impl Iterator<Item = (&Chord, &Command)> {
        self.by_chord.iter()
    }
}

thread_local! {
    static ACTIVE: RefCell<Rc<Keymap>> = RefCell::new(Rc::new(Keymap::defaults(Profile::Vim)));
}

/// The keymap in force on this thread (the window's): Vim's defaults until one is set.
pub fn active() -> Rc<Keymap> {
    ACTIVE.with(|a| a.borrow().clone())
}

pub fn with_active<R>(f: impl FnOnce(&Keymap) -> R) -> R {
    ACTIVE.with(|a| f(&a.borrow()))
}

pub fn set_active(km: Keymap) {
    ACTIVE.with(|a| *a.borrow_mut() = Rc::new(km));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn ch(s: &str) -> Chord {
        Chord::parse(s).unwrap_or_else(|| panic!("{s} parses"))
    }

    #[test]
    fn every_default_parses_and_no_chord_is_bound_twice() {
        for p in Profile::ALL {
            let mut seen = HashSet::new();
            for (spec, cmd) in p.defaults() {
                let c = ch(spec);
                assert!(seen.insert(c), "{p:?}: {spec} twice");
                assert_eq!(Chord::parse(&c.display()), Some(c), "{spec} round-trips through {}", c.display());
                assert!(!cmd.is_legacy(), "{cmd:?}");
                if c.universal().is_some() {
                    assert_eq!(c.universal_command(), Some(*cmd), "{p:?}: {spec} is universal");
                }
            }
        }
    }

    #[test]
    fn non_vim_profiles_bind_no_bare_characters() {
        for p in [Profile::VsCode, Profile::JetBrains] {
            assert!(!p.vim_motions());
            for (spec, _) in p.defaults() {
                let c = ch(spec);
                let bare = matches!(c.key, ChordKey::Char(_) | ChordKey::Space) && !c.ctrl && !c.alt;
                assert!(!bare, "{p:?}: {spec} would read as typing");
            }
        }
        assert!(Profile::Vim.vim_motions());
    }

    #[test]
    fn chords_parse_and_display() {
        assert_eq!(ch("Ctrl+K").display(), "Ctrl+K");
        assert_eq!(ch("ctrl+shift+l").display(), "Ctrl+Shift+L");
        assert_eq!(ch("J"), Chord::new(ChordKey::Char('j'), false, true, false));
        assert_eq!(ch("Shift+j").display(), "J");
        assert_eq!(ch("?").display(), "?");
        assert_eq!(ch("Shift+/"), ch("/"), "Shift on punctuation is in the character");
        assert_eq!(ch("Alt+Shift+F5").display(), "Alt+Shift+F5");
        assert_eq!(ch("Ctrl+Alt+Shift+C").display(), "Ctrl+Alt+Shift+C");
        assert_eq!(ch("Cmd+P"), ch("Ctrl+P"));
        assert_eq!(ch("Ctrl++").key, ChordKey::Char('+'));
        assert_eq!(ch("esc"), ch("Escape"));
        assert_eq!(ch("pgdn").key, ChordKey::PageDown);
        assert_eq!(ch("Shift+Escape").display(), "Shift+Escape");
        assert_eq!(ch("Ctrl+,").display(), "Ctrl+,");
        let collapse = Chord::new(ChordKey::Char('_'), true, true, false);
        assert_eq!(ch("Ctrl+Shift+-"), collapse, "as Ctrl+Shift+Minus arrives");
        assert_eq!(ch("Ctrl+_"), collapse);
        assert_eq!(collapse.display(), "Ctrl+Shift+-");
        assert_eq!(ch("Ctrl++").display(), "Ctrl+Shift+=");
        assert_eq!(ch("Ctrl+Shift+[").display(), "Ctrl+Shift+[");
        assert_eq!(ch("Ctrl+Shift+["), Chord::new(ChordKey::Char('{'), true, false, false));
        assert_eq!(ch("Ctrl+-").display(), "Ctrl+-", "no Shift, no change");
        for bad in ["", "Ctrl+", "Hyper+K", "F99", "Ctrl+Nope", "ab"] {
            assert_eq!(Chord::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn universal_keys_are_the_cursor_copy_and_close_keys() {
        for s in [
            "Up",
            "Down",
            "Left",
            "Right",
            "Shift+Up",
            "Ctrl+Left",
            "Ctrl+Shift+Right",
            "Home",
            "Shift+End",
            "Ctrl+Home",
            "Ctrl+Shift+End",
            "PageUp",
            "Shift+PageDown",
            "Ctrl+C",
            "Ctrl+A",
            "Escape",
            "Enter",
        ] {
            assert!(ch(s).universal().is_some(), "{s}");
        }
        for s in [
            "Alt+Left",
            "Ctrl+PageDown",
            "Ctrl+Shift+C",
            "Shift+Escape",
            "Ctrl+Enter",
            "j",
            "Ctrl+Up",
        ] {
            assert!(ch(s).universal().is_none(), "{s}");
        }
    }

    #[test]
    fn overrides_replace_unbind_steal_and_report_what_they_ignore() {
        let mut o = BTreeMap::new();
        o.insert("comment".to_owned(), vec!["Ctrl+M".to_owned(), "q".to_owned()]);
        o.insert("ask".to_owned(), vec![]);
        o.insert("go-to-file".to_owned(), vec!["Ctrl+K".to_owned()]);
        o.insert(
            "defer".to_owned(),
            vec!["Down".to_owned(), "Ctrl+Nope".to_owned(), "Ctrl+M".to_owned()],
        );
        o.insert("no-such-thing".to_owned(), vec!["Ctrl+J".to_owned()]);
        o.insert("escape".to_owned(), vec![]);
        let (km, problems) = Keymap::with_overrides(Profile::Vim, &o);
        assert_eq!(km.lookup(&ch("Escape")), Some(Command::Escape), "a universal key can't be unbound");
        assert_eq!(km.lookup(&ch("Ctrl+M")), Some(Command::Comment));
        assert_eq!(km.lookup(&ch("q")), Some(Command::Comment));
        assert_eq!(km.lookup(&ch("c")), None, "the default went with the override");
        assert_eq!(km.hint(Command::Comment), "Ctrl+M");
        assert_eq!(km.lookup(&ch("?")), None);
        assert!(km.chords(Command::Ask).is_empty());
        assert_eq!(km.lookup(&ch("Ctrl+K")), Some(Command::Picker), "taken from the palette");
        assert!(km.chords(Command::Palette).is_empty());
        assert_eq!(km.lookup(&ch("Down")), None);
        assert!(km.chords(Command::Defer).is_empty());
        assert_eq!(problems.len(), 4, "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("universal")));
        assert!(problems.iter().any(|p| p.contains("already bound")));
        assert!(problems.iter().any(|p| p.contains("not a key")));
        assert!(problems.iter().any(|p| p.contains("no-such-thing")));
    }

    #[test]
    fn the_active_keymap_is_per_thread_and_replaceable() {
        assert_eq!(with_active(|k| k.profile()), Profile::Vim);
        set_active(Keymap::defaults(Profile::JetBrains));
        assert_eq!(Command::NextHunk.chord().map(|c| c.display()).as_deref(), Some("F7"));
        std::thread::spawn(|| assert_eq!(active().profile(), Profile::Vim)).join().unwrap();
        set_active(Keymap::defaults(Profile::Vim));
        assert_eq!(Command::NextHunk.chord().map(|c| c.display()).as_deref(), Some("n"));
    }

    #[test]
    fn profiles_switch_lookups_and_hints() {
        let cases = [
            (Command::Palette, ["Ctrl+K", "Ctrl+Shift+P", "Ctrl+Shift+A"]),
            (Command::Picker, ["Ctrl+P", "Ctrl+P", "Ctrl+Shift+N"]),
            (Command::Find, ["/", "Ctrl+F", "Ctrl+F"]),
            (Command::NextHunk, ["n", "Alt+F5", "F7"]),
            (Command::PrevHunk, ["p", "Alt+Shift+F5", "Shift+F7"]),
            (Command::Comment, ["c", "Ctrl+/", "Ctrl+/"]),
            (Command::ToggleList, ["Ctrl+B", "Ctrl+B", "Alt+1"]),
            (Command::KeySheet, ["F1", "Ctrl+,", "Ctrl+Alt+S"]),
            (Command::CollapseAllFiles, ["Ctrl+Shift+-", "Ctrl+Shift+[", "Ctrl+Shift+-"]),
            (Command::ExpandAllFiles, ["Ctrl+Shift+=", "Ctrl+Shift+]", "Ctrl+Shift+="]),
        ];
        for (cmd, want) in cases {
            for (p, w) in Profile::ALL.into_iter().zip(want) {
                let km = Keymap::defaults(p);
                assert_eq!(km.hint(cmd), w, "{p:?} {cmd:?}");
                assert_eq!(km.lookup(&ch(w)), Some(cmd), "{p:?} {w}");
            }
        }
        assert_eq!(Keymap::defaults(Profile::VsCode).lookup(&ch("j")), None);
        assert_eq!(Keymap::defaults(Profile::JetBrains).lookup(&ch("c")), None);
        assert_eq!(Keymap::defaults(Profile::Vim).hint(Command::CollapseAllContext), "Z");
        for p in [Profile::VsCode, Profile::JetBrains] {
            assert!(Keymap::defaults(p).chords(Command::CollapseAllContext).is_empty(), "{p:?}");
        }
    }
}
