//! The Keyboard shortcuts screen: a draft of the profile and overrides the user edits (record a
//! key, replace a conflicting one, remove, reset), saved to the settings or thrown away.

use std::cell::RefCell;
use std::collections::BTreeMap;

use rain_core::command::Command;
use rain_core::keymap::{self, Chord, Keymap, Profile, Shortcut};
use rune::TextEdit;

use super::App;

/// A recorded key that another command already has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub cmd: Command,
    pub chord: Chord,
    pub holder: Command,
}

pub struct ShortcutsState {
    pub open: bool,
    pub profile: Profile,
    /// Command id to its keys, as the settings file keeps them.
    pub overrides: BTreeMap<String, Vec<String>>,
    /// The keymap the draft makes, rebuilt on each change.
    pub draft: Keymap,
    /// The profile's own keymap, rebuilt only when the profile changes.
    defaults: Keymap,
    /// Counts the draft's rebuilds, so rows and matches redo only after a change.
    pub generation: u64,
    /// The last search: its query, the draft's generation and what matched.
    found: RefCell<Option<(String, u64, Vec<Command>)>>,
    pub search: TextEdit,
    /// The command the next key press is recorded for.
    pub recording: Option<Command>,
    pub conflict: Option<Conflict>,
    /// The screen's own message line: a refused key, a conflict, what was saved.
    pub note: String,
}

impl Default for ShortcutsState {
    fn default() -> ShortcutsState {
        ShortcutsState {
            open: false,
            profile: Profile::Vim,
            overrides: BTreeMap::new(),
            draft: Keymap::defaults(Profile::Vim),
            defaults: Keymap::defaults(Profile::Vim),
            generation: 0,
            found: RefCell::new(None),
            search: TextEdit::default(),
            recording: None,
            conflict: None,
            note: String::new(),
        }
    }
}

/// The status line for settings entries the keymap ignored.
pub(crate) fn ignored_message(ignored: &[String]) -> String {
    match ignored {
        [] => String::new(),
        [one] => format!("Keyboard shortcuts: ignored {one}"),
        [first, rest @ ..] => format!("Keyboard shortcuts: ignored {first} and {} more", rest.len()),
    }
}

/// The commands the screen lists, in the palette's order.
pub fn listed() -> impl Iterator<Item = Command> {
    Command::ALL.into_iter()
}

impl ShortcutsState {
    fn rebuild(&mut self) {
        if self.defaults.profile() != self.profile {
            self.defaults = Keymap::defaults(self.profile);
        }
        self.draft = Keymap::with_overrides(self.profile, &self.overrides).0;
        self.generation += 1;
    }

    /// Commands whose label, id or keys match the search, in order; worked out again only when
    /// the query or the draft changed.
    pub fn matches(&self) -> Vec<Command> {
        let q = self.search.text.trim().to_lowercase();
        if let Some((last, generation, found)) = &*self.found.borrow()
            && *last == q
            && *generation == self.generation
        {
            return found.clone();
        }
        let found: Vec<Command> = listed()
            .filter(|c| {
                q.is_empty()
                    || c.label().to_lowercase().contains(&q)
                    || c.id().contains(&q)
                    || self.draft.chords(*c).iter().any(|k| k.display().to_lowercase().contains(&q))
            })
            .collect();
        *self.found.borrow_mut() = Some((q, self.generation, found.clone()));
        found
    }

    /// Whether the draft gives `cmd` other keys than its profile does.
    pub fn changed(&self, cmd: Command) -> bool {
        self.draft.chords(cmd) != self.defaults.chords(cmd)
    }

    fn set_keys(&mut self, cmd: Command, keys: &[Shortcut]) {
        let keys = keys.iter().map(Shortcut::display).collect();
        self.overrides.insert(cmd.id().to_owned(), keys);
    }

    fn add_key(&mut self, cmd: Command, chord: Chord) {
        let mut keys = self.draft.chords(cmd).to_vec();
        if !keys.contains(&chord.into()) {
            keys.push(chord.into());
        }
        self.set_keys(cmd, &keys);
        self.rebuild();
    }

    /// The overrides that differ from the profile, as the settings file keeps them.
    fn trimmed(&self) -> BTreeMap<String, Vec<String>> {
        self.overrides
            .iter()
            .filter(|(id, _)| Command::from_id(id).is_none_or(|c| self.changed(c)))
            .map(|(id, keys)| (id.clone(), keys.clone()))
            .collect()
    }
}

impl App {
    pub fn keymap_profile(&self) -> Profile {
        keymap::with_active(|k| k.profile())
    }

    /// Builds the keymap the settings describe and makes it the window's; returns what it
    /// ignored and says so in the status line, or else names a key a two-key shortcut slowed.
    pub fn apply_keymap(&mut self) -> Vec<String> {
        let (km, ignored) = self.settings.keymap.build();
        let waits = km.waiting_chord();
        keymap::set_active(km);
        self.keymap_gen += 1;
        self.key_wait.held = None;
        if !ignored.is_empty() {
            self.status = ignored_message(&ignored);
        } else if let Some((chord, cmd)) = waits {
            let ms = self.settings.keymap.timeout().as_millis();
            self.status = format!(
                "Keyboard shortcuts: {} also starts a two-key shortcut, so {} waits {ms} ms",
                chord.display(),
                cmd.label()
            );
        }
        ignored
    }

    /// Switches the shortcut profile now (the palette's rows), keeping the overrides.
    pub fn set_profile(&mut self, p: Profile) {
        self.settings.keymap.profile = p;
        self.apply_keymap();
        self.status = format!("Keyboard shortcuts: {}", p.label());
        // Last, so a failed save is what the status line says.
        self.save_settings();
    }

    pub fn open_shortcuts(&mut self) {
        self.close_palette();
        self.menus.close();
        let (overrides, _) = self.settings.keymap.overrides();
        let sh = &mut self.shortcuts;
        sh.open = true;
        sh.profile = self.settings.keymap.profile;
        sh.overrides = overrides;
        sh.search = TextEdit::default();
        sh.search.focused = true;
        sh.recording = None;
        sh.conflict = None;
        sh.note.clear();
        sh.rebuild();
    }

    /// Cancel: the draft goes; true when the screen was open. Escape first stops a recording
    /// or drops a conflict.
    pub fn close_shortcuts(&mut self) -> bool {
        let sh = &mut self.shortcuts;
        if !sh.open {
            return false;
        }
        if sh.recording.take().is_some() || sh.conflict.take().is_some() {
            sh.note.clear();
            return true;
        }
        sh.open = false;
        sh.search.focused = false;
        true
    }

    pub fn shortcuts_profile(&mut self, p: Profile) {
        let sh = &mut self.shortcuts;
        sh.profile = p;
        sh.conflict = None;
        sh.note = format!("{} keys; your own keys stay over them", p.label());
        sh.rebuild();
    }

    /// The next key press becomes one of `cmd`'s keys.
    pub fn start_recording(&mut self, cmd: Command) {
        let sh = &mut self.shortcuts;
        sh.recording = Some(cmd);
        sh.conflict = None;
        sh.search.focused = false;
        sh.note = format!("Press the keys for {} · Esc cancels", cmd.label());
    }

    /// A key pressed while recording: added to the command, refused when universal, or held as
    /// a conflict when another command has it.
    pub fn record_shortcut(&mut self, chord: Chord) {
        let sh = &mut self.shortcuts;
        let Some(cmd) = sh.recording else { return };
        if chord == Chord::plain(rain_core::ChordKey::Escape) {
            sh.recording = None;
            sh.note.clear();
            return;
        }
        if let Some(what) = chord.universal().filter(|_| chord.universal_command() != Some(cmd)) {
            sh.note = format!("{} is a universal key ({}); pick another", chord.display(), what.to_lowercase());
            return;
        }
        sh.recording = None;
        match sh.draft.lookup(&chord) {
            Some(holder) if holder != cmd => {
                sh.conflict = Some(Conflict { cmd, chord, holder });
                sh.note = format!("{} is already {}", chord.display(), holder.label());
            }
            _ => {
                sh.add_key(cmd, chord);
                sh.note = format!("{}: {}", cmd.label(), chord.display());
            }
        }
    }

    /// Replace: the conflicting key moves from its holder to the recorded command.
    pub fn resolve_conflict(&mut self, replace: bool) {
        let sh = &mut self.shortcuts;
        let Some(c) = sh.conflict.take() else { return };
        if !replace {
            sh.note.clear();
            return;
        }
        let keep: Vec<Shortcut> = sh.draft.chords(c.holder).iter().copied().filter(|k| *k != c.chord.into()).collect();
        sh.set_keys(c.holder, &keep);
        sh.add_key(c.cmd, c.chord);
        sh.note = format!("{} now {}; {} lost it", c.chord.display(), c.cmd.label(), c.holder.label());
    }

    pub fn remove_shortcut(&mut self, cmd: Command, keys: Shortcut) {
        let sh = &mut self.shortcuts;
        let keep: Vec<Shortcut> = sh.draft.chords(cmd).iter().copied().filter(|k| *k != keys).collect();
        sh.set_keys(cmd, &keep);
        sh.rebuild();
        sh.note = format!("Removed {} from {}", keys.display(), cmd.label());
    }

    pub fn reset_shortcut(&mut self, cmd: Command) {
        let sh = &mut self.shortcuts;
        sh.overrides.remove(cmd.id());
        sh.conflict = None;
        sh.rebuild();
        sh.note = format!("{}: the {} keys", cmd.label(), sh.profile.label());
    }

    pub fn reset_all_shortcuts(&mut self) {
        let sh = &mut self.shortcuts;
        sh.overrides.clear();
        sh.conflict = None;
        sh.rebuild();
        sh.note = format!("Every command: the {} keys", sh.profile.label());
    }

    /// Save: the draft goes to the settings file and becomes the window's keymap.
    pub fn save_shortcuts(&mut self) {
        let overrides = self.shortcuts.trimmed();
        self.settings.keymap.profile = self.shortcuts.profile;
        self.settings.keymap.set_overrides(&overrides);
        self.apply_keymap();
        self.shortcuts.open = false;
        self.shortcuts.search.focused = false;
        self.shortcuts.recording = None;
        self.shortcuts.conflict = None;
        self.status = format!("Keyboard shortcuts saved: {}", self.shortcuts.profile.label());
        // Last, so a failed save is what the status line says.
        self.save_settings();
    }
}
