//! Vim's keys in the reader: the diff's own motions and visual mode, ahead of the keymap.

use rain_core::command::Command;
use rain_diffview::{Vim, VimKey};
use rune::{Key, KeyEvent, Tx};

use super::{App, Overlay, ReaderTab};

/// The Vim key `ev` is, if any: Ctrl+D and Ctrl+U, Escape, or a typed character (a letter
/// typed with Shift upper case).
pub(crate) fn vim_key_of(ev: &KeyEvent) -> Option<VimKey> {
    let m = ev.mods;
    if m.alt || m.logo {
        return None;
    }
    let ctrl = m.ctrl || m.cmd;
    match &ev.key {
        Key::Escape if !ctrl && !m.shift => Some(VimKey::Escape),
        Key::Command(c) if !m.shift => half(*c),
        Key::Text(t) if ctrl => match (m.shift, t.as_str()) {
            (false, "d" | "D") => Some(VimKey::HalfDown),
            (false, "u" | "U") => Some(VimKey::HalfUp),
            _ => None,
        },
        Key::Text(t) => {
            let mut chars = t.chars();
            let c = chars.next().filter(|_| chars.next().is_none())?;
            Some(VimKey::Char(if m.shift && c.is_ascii_lowercase() {
                c.to_ascii_uppercase()
            } else {
                c
            }))
        }
        _ => None,
    }
}

fn half(c: char) -> Option<VimKey> {
    match c {
        'd' => Some(VimKey::HalfDown),
        'u' => Some(VimKey::HalfUp),
        _ => None,
    }
}

impl App {
    /// Whether the diff is what the keys move in: the Changes tab with no Map or Conversation
    /// over it.
    pub(crate) fn vim_live(&self) -> bool {
        self.reader_tab() == ReaderTab::Changes && !self.layout.is_open(Overlay::Map) && !self.layout.is_open(Overlay::Conversation)
    }

    /// A key for the diff's Vim motions; `None` leaves it to the keymap.
    pub(crate) fn vim_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> Option<bool> {
        if !self.vim_live() {
            return None;
        }
        let Some(key) = vim_key_of(ev) else {
            self.active_mut().vim_cancel();
            return None;
        };
        self.vim_run(key, tx)
    }

    /// One Vim key on the diff on screen, a yank copied and `c` / `?` opening the composer.
    pub(crate) fn vim_run(&mut self, key: VimKey, tx: &mut Tx) -> Option<bool> {
        match self.active_mut().vim(key, tx) {
            Vim::Pass => None,
            Vim::Done => Some(true),
            Vim::Yank(text) => {
                self.copied(text, tx);
                Some(true)
            }
            Vim::Comment => Some(self.run(Command::Comment, tx)),
            Vim::Ask => Some(self.run(Command::Ask, tx)),
        }
    }

    /// Puts `text` on the clipboard and says how many lines went.
    pub(crate) fn copied(&mut self, text: String, tx: &mut Tx) {
        let n = text.lines().count().max(1);
        tx.copy(text);
        self.status = if n == 1 {
            "Copied 1 line".into()
        } else {
            format!("Copied {n} lines")
        };
    }
}
