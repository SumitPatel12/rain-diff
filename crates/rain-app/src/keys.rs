//! The window's key handler: the universal cursor keys, then Vim's motions (in the Vim
//! profile), then the active keymap's chords to [`App::run`].

use rain_core::command::Command;
use rain_core::keymap::{self, Chord, ChordKey};
use rain_diffview::Caret;
use rune::{Key, KeyEvent, Tx};

use crate::app::{App, Phase};

/// The chord a key event is, or `None` for keys no chord names (modifiers, the Windows key).
pub fn chord_of(ev: &KeyEvent) -> Option<Chord> {
    let m = ev.mods;
    if m.logo {
        return None;
    }
    let key = match &ev.key {
        Key::Command(' ') => ChordKey::Space,
        Key::Command(c) => ChordKey::Char(*c),
        Key::Text(t) => {
            let mut chars = t.chars();
            match (chars.next(), chars.next()) {
                (Some(' '), None) => ChordKey::Space,
                (Some(c), None) => ChordKey::Char(c),
                _ => return None,
            }
        }
        Key::Tab => ChordKey::Tab,
        Key::Enter => ChordKey::Enter,
        Key::Escape => ChordKey::Escape,
        Key::Backspace => ChordKey::Backspace,
        Key::Delete => ChordKey::Delete,
        Key::Left => ChordKey::Left,
        Key::Right => ChordKey::Right,
        Key::Up => ChordKey::Up,
        Key::Down => ChordKey::Down,
        Key::Home => ChordKey::Home,
        Key::End => ChordKey::End,
        Key::PageUp => ChordKey::PageUp,
        Key::PageDown => ChordKey::PageDown,
        Key::Function(n) => ChordKey::F(*n),
        _ => return None,
    };
    Some(Chord::new(key, m.ctrl || m.cmd, m.shift, m.alt))
}

/// The command the active keymap binds to this key.
pub fn command_of(ev: &KeyEvent) -> Option<Command> {
    let c = chord_of(ev)?;
    keymap::with_active(|k| k.lookup(&c))
}

/// `text  key` for a tooltip; just `text` when the command has no key in the active keymap.
pub fn tip(text: &str, cmd: Command) -> String {
    match cmd.chord() {
        Some(c) => format!("{text}  {}", c.display()),
        None => text.to_owned(),
    }
}

/// `key what` for each command that has a key, joined with ` · `: `c comment · ? question`.
pub fn hints(parts: &[(Command, &str)]) -> String {
    let mut out = String::new();
    for (cmd, what) in parts {
        if let Some(c) = cmd.chord() {
            if !out.is_empty() {
                out.push_str(" · ");
            }
            out.push_str(&c.display());
            out.push(' ');
            out.push_str(what);
        }
    }
    out
}

/// `head · hints`, or `head` alone when no command has a key.
pub fn with_hints(head: &str, parts: &[(Command, &str)]) -> String {
    let h = hints(parts);
    if h.is_empty() { head.to_owned() } else { format!("{head} · {h}") }
}

fn is_escape(c: Option<Chord>) -> bool {
    c == Some(Chord::plain(ChordKey::Escape))
}

/// The cursor key a chord is in the diff, with whether Shift extends.
fn caret_of(c: Chord) -> Option<(Caret, bool)> {
    if c.alt {
        return None;
    }
    let k = match (c.key, c.ctrl) {
        (ChordKey::Left, false) => Caret::Left,
        (ChordKey::Right, false) => Caret::Right,
        (ChordKey::Left, true) => Caret::WordLeft,
        (ChordKey::Right, true) => Caret::WordRight,
        (ChordKey::Up, false) => Caret::Up,
        (ChordKey::Down, false) => Caret::Down,
        (ChordKey::Home, false) => Caret::Home,
        (ChordKey::End, false) => Caret::End,
        (ChordKey::Home, true) => Caret::FileStart,
        (ChordKey::End, true) => Caret::FileEnd,
        (ChordKey::PageUp, false) => Caret::PageUp,
        (ChordKey::PageDown, false) => Caret::PageDown,
        _ => return None,
    };
    Some((k, c.shift))
}

impl App {
    /// The keys every profile shares on the diff: cursor keys (Shift extends), Ctrl+A, and
    /// Escape out of a Shift selection outside Vim. `None` leaves the key to the rest.
    fn universal_key(&mut self, chord: Option<Chord>, tx: &mut Tx) -> Option<bool> {
        let c = chord?;
        if !self.vim_live() {
            return None;
        }
        if let Some((k, extend)) = caret_of(c) {
            self.active_mut().vim_cancel();
            return Some(self.active_mut().caret(k, extend, tx));
        }
        if c == Chord::new(ChordKey::Char('a'), true, false, false) {
            if self.active_mut().select_file() {
                self.status = "Selected the file's lines · Ctrl+C copies".into();
            }
            return Some(true);
        }
        if is_escape(chord) && !self.keymap_profile().vim_motions() && self.active().visual().is_some() {
            self.active_mut().end_visual();
            return Some(true);
        }
        None
    }

    /// Ctrl+C: the diff's line selection as before; else, with no text selected anywhere, the
    /// visual selection or the cursor's line. False leaves it to a text selection.
    fn copy_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        if self.active_mut().key(ev, tx) {
            return true;
        }
        if ev.mods.shift || ev.mods.alt || tx.text_selection().is_some() || !self.vim_live() {
            return false;
        }
        match self.active().copy_text() {
            Some(text) => {
                self.copied(text, tx);
                true
            }
            None => false,
        }
    }
}

pub fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    // The rain holds every key: Enter took the break, Esc snoozes it.
    if s.raining() {
        match ev.key {
            Key::Enter => s.rest_taken(),
            Key::Escape => s.rest_snooze(),
            _ => {}
        }
        return true;
    }
    if matches!(ev.key, Key::Modifier { .. }) {
        return s.active_mut().key(ev, tx);
    }
    let chord = chord_of(ev);
    if s.shortcuts.recording.is_some() {
        if let Some(c) = chord {
            s.record_shortcut(c);
        }
        return true;
    }
    if s.shortcuts.open {
        return is_escape(chord) && s.escape(tx);
    }
    if ev.is_command('c') && !s.typing() {
        return s.copy_key(ev, tx);
    }
    if s.typing() {
        return is_escape(chord) && s.escape(tx);
    }
    let cmd = chord.and_then(|c| keymap::with_active(|k| k.lookup(&c)));
    if s.phase != Phase::Ready {
        return match cmd {
            Some(c @ (Command::Reload | Command::ToggleTheme)) => s.run(c, tx),
            _ => is_escape(chord) && s.escape(tx),
        };
    }
    if let Some(done) = s.tour_key(ev, tx) {
        return done;
    }
    if let Some(done) = s.universal_key(chord, tx) {
        return done;
    }
    if s.keymap_profile().vim_motions()
        && let Some(done) = s.vim_key(ev, tx)
    {
        return done;
    }
    match cmd {
        Some(c) => s.run(c, tx),
        None if is_escape(chord) => s.escape(tx),
        None => false,
    }
}
