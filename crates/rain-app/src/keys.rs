//! The window's key handler: `rain_core::command::keymap()` chords to [`App::run`].

use rain_core::command::{self, Command};
use rune::{Key, KeyEvent, Tx};

use crate::app::{App, Phase};

/// The keymap's name for a key event: `(key, ctrl, shift, alt)`, or `None` for keys it never
/// binds.
pub fn chord_of(ev: &KeyEvent) -> Option<(String, bool, bool, bool)> {
    let m = ev.mods;
    if m.logo {
        return None;
    }
    let ctrl = m.ctrl || m.cmd;
    let key = match &ev.key {
        Key::Command(c) => return Some((c.to_string(), true, m.shift, m.alt)),
        Key::Text(t) if t == " " => "Space".to_owned(),
        Key::Text(t) => t.clone(),
        Key::Tab => "Tab".into(),
        Key::Escape => "Escape".into(),
        Key::Left => "Left".into(),
        Key::Right => "Right".into(),
        Key::Function(n) => format!("F{n}"),
        _ => return None,
    };
    Some((key, ctrl, m.shift, m.alt))
}

pub fn command_of(ev: &KeyEvent) -> Option<Command> {
    let (key, ctrl, shift, alt) = chord_of(ev)?;
    command::lookup_chord(&key, ctrl, shift, alt)
}

pub fn on_key(s: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    if matches!(ev.key, Key::Modifier { .. }) {
        return s.active_mut().key(ev, tx);
    }
    if ev.is_command('c') && !s.typing() {
        return s.active_mut().key(ev, tx);
    }
    let cmd = command_of(ev);
    if s.typing() {
        return match cmd {
            Some(Command::Escape) => s.escape(tx),
            _ => false,
        };
    }
    if s.phase != Phase::Ready {
        return match cmd {
            Some(c @ (Command::Reload | Command::ToggleTheme | Command::ToggleChrome | Command::Escape)) => s.run(c, tx),
            _ => false,
        };
    }
    if let Some(done) = s.tour_key(ev, tx) {
        return done;
    }
    match cmd {
        Some(c) => s.run(c, tx),
        None => false,
    }
}
