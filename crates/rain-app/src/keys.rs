//! The window's key handler: the universal cursor keys, then Vim's motions (in the Vim
//! profile), then the active keymap's chords to [`App::run`]. The first key of a two-key
//! shortcut is held for the second.

use std::time::Instant;

use rain_core::command::Command;
use rain_core::keymap::{self, Chord, ChordKey};
use rain_diffview::{Caret, VimKey};
use rune::{Key, KeyEvent, Tx};

use crate::app::vim::vim_key_of;
use crate::app::{App, Phase};

/// The first key of a two-key shortcut, with when it acts alone if it does.
pub(crate) struct HeldKey {
    ev: KeyEvent,
    chord: Chord,
    due: Option<Instant>,
}

#[derive(Default)]
pub(crate) struct KeyWait {
    pub(crate) held: Option<HeldKey>,
    /// While a held key runs alone and the next is read after it: the event changed what the
    /// view reads, so a key that only waits doesn't ask for no view.
    pub(crate) loud: bool,
}

#[cfg(test)]
impl KeyWait {
    pub(crate) fn chord(&self) -> Option<Chord> {
        self.held.as_ref().map(|h| h.chord)
    }

    pub(crate) fn timed(&self) -> bool {
        self.held.as_ref().is_some_and(|h| h.due.is_some())
    }
}

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
    /// The rain and a shortcut recording hear every key before any focused control.
    pub(crate) fn holds_every_key(&self) -> bool {
        self.raining() || self.shortcuts.recording.is_some()
    }

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

    /// Whether Vim's motions are what a key on the diff means now.
    fn vim_keys(&self) -> bool {
        self.vim_live() && self.keymap_profile().vim_motions()
    }

    /// A key on the reader with nothing held: the tour, the cursor keys, Vim, then its command.
    fn dispatch(&mut self, ev: &KeyEvent, chord: Option<Chord>, cmd: Option<Command>, tx: &mut Tx) -> bool {
        if let Some(done) = self.tour_key(ev, tx) {
            return done;
        }
        if let Some(done) = self.universal_key(chord, tx) {
            return done;
        }
        if self.keymap_profile().vim_motions()
            && let Some(done) = self.vim_key(ev, tx)
        {
            return done;
        }
        match cmd {
            Some(c) => self.run(c, tx),
            None if is_escape(chord) => self.escape(tx),
            None => false,
        }
    }

    /// The first key of a two-key shortcut: held with no view run. One that also acts alone
    /// (`alone`, or a Vim motion) waits only the keymap's timeout, as Vim's `timeoutlen`.
    fn hold_key(&mut self, ev: &KeyEvent, chord: Chord, alone: bool, tx: &mut Tx) -> bool {
        let alone = alone || (self.vim_keys() && vim_key_of(ev).is_some_and(VimKey::acts_alone));
        let due = alone.then(|| {
            let wait = self.settings.keymap.timeout();
            tx.wake_after(wait);
            Instant::now() + wait
        });
        self.key_wait.held = Some(HeldKey {
            ev: ev.clone(),
            chord,
            due,
        });
        if !self.key_wait.loud {
            tx.live_only();
        }
        true
    }

    /// The key after a held one: Escape drops it, the shortcut's second key runs its command,
    /// and any other key, as in Vim, lets the first do what it does alone and is then read
    /// afresh.
    fn second_key(&mut self, first: HeldKey, ev: &KeyEvent, chord: Option<Chord>, tx: &mut Tx) -> bool {
        if is_escape(chord) {
            self.active_mut().vim_cancel();
            tx.live_only();
            return true;
        }
        if let Some(cmd) = chord.and_then(|c| keymap::with_active(|k| k.pair(&first.chord, &c))) {
            self.active_mut().vim_cancel();
            return self.run(cmd, tx);
        }
        let ran = self.replay(first, tx);
        self.key_wait.loud = ran;
        let took = on_key(self, ev, tx);
        self.key_wait.loud = false;
        took || ran
    }

    /// A held key does what it does alone.
    fn replay(&mut self, first: HeldKey, tx: &mut Tx) -> bool {
        let cmd = keymap::with_active(|k| k.lookup(&first.chord));
        self.key_wait.loud = true;
        let ran = self.dispatch(&first.ev, Some(first.chord), cmd, tx);
        self.key_wait.loud = false;
        ran
    }

    /// Whether keys go somewhere other than the reader's keymap: a text field, a dialog.
    fn keys_elsewhere(&self) -> bool {
        self.raining() || self.picker_normal() || self.shortcuts.open || self.playground.open || self.typing() || self.phase != Phase::Ready
    }

    /// A held key whose wait is over by `now` acts alone; true when that changed something.
    pub(crate) fn held_key_expire_at(&mut self, now: Instant, tx: &mut Tx) -> bool {
        let Some(due) = self.key_wait.held.as_ref().and_then(|h| h.due) else {
            return false;
        };
        if now < due {
            tx.wake_after(due - now);
            return false;
        }
        let Some(first) = self.key_wait.held.take() else {
            return false;
        };
        !self.keys_elsewhere() && self.replay(first, tx)
    }

    /// Each frame: a held key goes when the keys go elsewhere, and acts alone when its wait ends.
    pub(crate) fn held_key_poll(&mut self, tx: &mut Tx) -> bool {
        if self.key_wait.held.is_none() {
            return false;
        }
        if self.keys_elsewhere() {
            self.key_wait.held = None;
            return false;
        }
        self.held_key_expire_at(Instant::now(), tx)
    }
}

pub fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    // The rain holds every key: Enter took the break, Esc snoozes it.
    if s.raining() {
        s.key_wait.held = None;
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
    // Taken here, so a first key never outlives a key that goes to a text field or a dialog.
    let held = s.key_wait.held.take();
    if s.picker_normal() {
        return s.picker_normal_key(ev, tx);
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
    if s.playground.open && !s.typing() {
        return s.playground_key(chord, tx);
    }
    if ev.is_command('c') && !s.typing() {
        return s.copy_key(ev, tx);
    }
    if s.typing() {
        return is_escape(chord) && s.escape(tx);
    }
    if s.map_lab_key(chord) {
        return true;
    }
    let (cmd, starts) = match chord {
        Some(c) => keymap::with_active(|k| (k.lookup(&c), k.starts_pair(&c))),
        None => (None, false),
    };
    if s.phase != Phase::Ready {
        return match cmd {
            Some(c @ (Command::Reload | Command::ToggleTheme)) => s.run(c, tx),
            _ => is_escape(chord) && s.escape(tx),
        };
    }
    if let Some(first) = held {
        return s.second_key(first, ev, chord, tx);
    }
    if starts
        && let Some(c) = chord
        && !(s.vim_keys() && s.active().vim_waiting())
    {
        return s.hold_key(ev, c, cmd.is_some(), tx);
    }
    s.dispatch(ev, chord, cmd, tx)
}
