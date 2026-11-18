//! The Diagram playground page: diagram styles over one sample change, from the `⋯` menu,
//! the palette or a key the user binds. Its timeline moves in the renderer: while it plays, a
//! poll publishes the frame's values and asks for the next frame; paused or closed, it does
//! nothing.

use rain_core::command::Command;
use rain_core::keymap::{Chord, ChordKey};
use rain_lab::{Lab, Tick, motion::FRAME};
use rune::Tx;

use super::App;

#[derive(Default)]
pub struct PlaygroundState {
    pub open: bool,
    pub lab: Lab,
    /// The canvas as last laid out.
    pub size: [f32; 2],
}

impl App {
    pub fn open_playground(&mut self) {
        self.close_palette();
        self.menus.close();
        self.playground.open = true;
    }

    /// Closes the page (pausing it); true when it was open.
    pub fn close_playground(&mut self) -> bool {
        if !self.playground.open {
            return false;
        }
        self.playground.open = false;
        self.playground.lab.pause();
        true
    }

    pub fn toggle_playground(&mut self) {
        if !self.close_playground() {
            self.open_playground();
        }
    }

    pub fn pick_style(&mut self, i: usize) {
        self.playground.lab.select(i);
    }

    /// The canvas was laid out at `size`; a new size lays the style out again.
    pub fn playground_resized(&mut self, size: [f32; 2]) {
        let old = self.playground.size;
        if (size[0] - old[0]).abs() >= 1.0 || (size[1] - old[1]).abs() >= 1.0 {
            self.playground.size = size;
        }
    }

    /// One frame of a playing timeline: its values move without a new view, the step label
    /// included; only the end rebuilds it, for the play button.
    pub(crate) fn playground_poll(&mut self, tx: &mut Tx) -> bool {
        if !self.playground.open {
            return false;
        }
        match self.playground.lab.tick(self.rest.clock) {
            Tick::Idle => false,
            Tick::Moved | Tick::Stepped => {
                tx.wake_after(FRAME);
                false
            }
            Tick::Ended => true,
        }
    }

    /// Keys while the page is up: Space plays or pauses, Left and Right step, Home restarts,
    /// Up and Down change style, Escape closes. The palette, the key sheet and the theme still
    /// work; nothing reaches the diff underneath.
    pub(crate) fn playground_key(&mut self, chord: Option<Chord>, tx: &mut Tx) -> bool {
        let Some(c) = chord else { return false };
        let lab = &mut self.playground.lab;
        if !c.ctrl && !c.alt {
            match c.key {
                ChordKey::Escape if !c.shift => return self.escape(tx),
                ChordKey::Space => lab.toggle(),
                ChordKey::Left => lab.step_by(false),
                ChordKey::Right => lab.step_by(true),
                ChordKey::Home => lab.restart(),
                ChordKey::Up | ChordKey::Down => {
                    let n = rain_lab::registry().len();
                    let i = lab.style_index();
                    lab.select(if c.key == ChordKey::Up { (i + n - 1) % n } else { (i + 1) % n });
                }
                _ => return self.playground_command(c, tx),
            }
            return true;
        }
        self.playground_command(c, tx)
    }

    fn playground_command(&mut self, c: Chord, tx: &mut Tx) -> bool {
        match rain_core::keymap::with_active(|k| k.lookup(&c)) {
            Some(
                cmd @ (Command::Palette | Command::KeySheet | Command::DiagramPlayground | Command::ToggleTheme | Command::ToggleDensity),
            ) => self.run(cmd, tx),
            _ => false,
        }
    }
}
