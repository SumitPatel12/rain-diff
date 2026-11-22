//! Window and view preferences kept across runs in `%LOCALAPPDATA%\Rain\settings.json`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rain_core::keymap::{Keymap, Profile};
use serde::{Deserialize, Serialize};

use crate::theme::Scheme;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
    Everforest,
    GruvboxHard,
    GruvboxMedium,
}

impl Theme {
    /// Every pick in the Theme menu's order, with its label.
    pub const PICKS: [(Theme, &'static str); 6] = [
        (Theme::System, "System"),
        (Theme::Light, "Light"),
        (Theme::Dark, "Dark"),
        (Theme::Everforest, "Everforest"),
        (Theme::GruvboxHard, "Gruvbox Dark Hard"),
        (Theme::GruvboxMedium, "Gruvbox Dark Medium"),
    ];

    /// The scheme drawn for this setting; `System` follows `system_dark`.
    pub fn scheme(self, system_dark: bool) -> Scheme {
        match self {
            Theme::System if system_dark => Scheme::Dark,
            Theme::System | Theme::Light => Scheme::Light,
            Theme::Dark => Scheme::Dark,
            Theme::Everforest => Scheme::Everforest,
            Theme::GruvboxHard => Scheme::GruvboxHard,
            Theme::GruvboxMedium => Scheme::GruvboxMedium,
        }
    }

    /// The setting for a `--theme` value that names one of Rain's own schemes; `None` for
    /// `dark`, `light` and the rest, which Rune's `--theme` handles.
    pub fn from_flag(v: &str) -> Option<Theme> {
        match v {
            "everforest" => Some(Theme::Everforest),
            "gruvbox-hard" => Some(Theme::GruvboxHard),
            "gruvbox-medium" => Some(Theme::GruvboxMedium),
            _ => None,
        }
    }
}

/// `T` from its JSON value, or its default when the value names nothing `T` knows, so an
/// unknown theme or a newer file's value doesn't throw away the rest of the settings.
fn or_default<'de, D: serde::Deserializer<'de>, T: serde::de::DeserializeOwned + Default>(d: D) -> Result<T, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    #[default]
    Compact,
    Comfortable,
}

/// How the diff lays out: split falls back to unified when a side would be too narrow (and
/// says so), auto does the same silently, unified never splits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ViewLayout {
    Auto,
    #[default]
    Split,
    Unified,
}

impl ViewLayout {
    pub const ALL: [ViewLayout; 3] = [ViewLayout::Auto, ViewLayout::Split, ViewLayout::Unified];

    pub fn label(self) -> &'static str {
        match self {
            ViewLayout::Auto => "Auto",
            ViewLayout::Split => "Split",
            ViewLayout::Unified => "Unified",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodeSize {
    S,
    #[default]
    M,
    L,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub window: (f32, f32),
    pub sidebar_width: f32,
    #[serde(deserialize_with = "or_default")]
    pub theme: Theme,
    pub density: Density,
    pub layout: ViewLayout,
    pub code_size: CodeSize,
    /// The file list beside the reader; closed it is a rail.
    pub list_open: bool,
    /// Bit `i` set: list section `i` is collapsed.
    pub sidebar_sections: u16,
    /// The `Ctrl+P` picker's panel, as last dragged.
    #[serde(deserialize_with = "or_default")]
    pub picker_size: (f32, f32),
    /// Find in the diff matches fuzzily, not the text as typed.
    #[serde(deserialize_with = "or_default")]
    pub find_fuzzy: bool,
    /// Open-in-editor command with `{path}` and `{line}`; `None` runs [`DEFAULT_EDITOR`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
    /// Who answers Question-mode threads, and how.
    pub ask: AskSettings,
    /// After this much active use the window rains until you take a break.
    pub break_reminder: BreakSettings,
    /// The shortcut profile and the user's own keys over it.
    #[serde(deserialize_with = "or_default")]
    pub keymap: KeymapSettings,
    /// Set when the file held round-2 keys, so the caller rewrites it once without them.
    #[serde(skip)]
    pub migrated: bool,
}

pub const PICKER_MIN: (f32, f32) = (560.0, 300.0);

pub const DEFAULT_EDITOR: &str = r#"code --goto "{path}:{line}""#;

/// Round-2 keys read once and never written again.
const LEGACY_KEYS: [&str; 5] = ["splitView", "inspectorWidth", "inspectorOpen", "inspectorSections", "mode"];

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            window: (1440.0, 900.0),
            sidebar_width: 284.0,
            theme: Theme::System,
            density: Density::Compact,
            layout: ViewLayout::Split,
            code_size: CodeSize::M,
            list_open: true,
            sidebar_sections: 0,
            picker_size: (940.0, 520.0),
            find_fuzzy: false,
            editor: None,
            ask: AskSettings::default(),
            break_reminder: BreakSettings::default(),
            keymap: KeymapSettings::default(),
            migrated: false,
        }
    }
}

/// `keymap: { profile: "vim" | "vscode" | "jetbrains", overrides: { "<command id>": ["Ctrl+K", "g d"] | [] } }`,
/// a key being one chord or two with a space between; `timeoutMs` is optional.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct KeymapSettings {
    #[serde(deserialize_with = "or_default")]
    pub profile: Profile,
    /// As written: entries that aren't a list of key names are reported and ignored.
    #[serde(deserialize_with = "or_default", skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, serde_json::Value>,
    /// How long a key that acts alone and also starts a two-key shortcut waits for the second.
    #[serde(deserialize_with = "or_default", skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u32>,
}

/// Vim's `timeoutlen`.
const KEY_TIMEOUT_MS: u32 = 1000;

impl KeymapSettings {
    pub fn timeout(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.timeout_ms.unwrap_or(KEY_TIMEOUT_MS).into())
    }

    /// The overrides that are lists of strings, and a line for each entry that isn't.
    pub fn overrides(&self) -> (BTreeMap<String, Vec<String>>, Vec<String>) {
        let mut ok = BTreeMap::new();
        let mut bad = Vec::new();
        for (id, v) in &self.overrides {
            let keys = v
                .as_array()
                .and_then(|a| a.iter().map(|k| k.as_str().map(str::to_owned)).collect::<Option<Vec<_>>>());
            match keys {
                Some(keys) => {
                    ok.insert(id.clone(), keys);
                }
                None => bad.push(format!("`{id}` needs a list of keys")),
            }
        }
        (ok, bad)
    }

    pub fn set_overrides(&mut self, overrides: &BTreeMap<String, Vec<String>>) {
        self.overrides = overrides
            .iter()
            .map(|(id, keys)| (id.clone(), serde_json::Value::from(keys.clone())))
            .collect();
    }

    /// The keymap these settings make, and what in them was ignored.
    pub fn build(&self) -> (Keymap, Vec<String>) {
        let (ok, mut bad) = self.overrides();
        let (km, more) = Keymap::with_overrides(self.profile, &ok);
        bad.extend(more);
        (km, bad)
    }
}

/// The break reminder: minutes of active use (input within the last few minutes), then a flood.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BreakSettings {
    pub enabled: bool,
    /// Active minutes before the rain; fractions work (`0.5` to try it).
    pub minutes: f64,
    pub snooze_minutes: f64,
}

impl Default for BreakSettings {
    fn default() -> BreakSettings {
        BreakSettings {
            enabled: true,
            minutes: 90.0,
            snooze_minutes: 10.0,
        }
    }
}

/// Which answerer a question goes to first.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AskPrefer {
    /// The agent holding `/api/events?for=agent`, with the launcher as the fallback.
    #[default]
    Attached,
    Launcher,
}

/// The local command that answers a question: the prompt on stdin, the answer on stdout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AskSettings {
    pub enabled: bool,
    /// `{repos}` (each repo's top folder), `{port}` and `{session}` are filled in.
    pub command: String,
    pub timeout_sec: u64,
    pub max_concurrent: usize,
    pub prefer: AskPrefer,
    /// How long an idle attached agent gets before the launcher takes the question.
    pub handoff_sec: u64,
    /// Paths whose code is never put in a prompt, and that a `claude` command may not read.
    pub redact: Vec<String>,
    /// The user allowed the launcher once; until then a question waits for that.
    pub allowed: bool,
}

pub const DEFAULT_ASK_COMMAND: &str = "claude -p --output-format text --allowedTools Read,Grep,Glob --add-dir {repos}";

impl Default for AskSettings {
    fn default() -> AskSettings {
        AskSettings {
            enabled: true,
            command: DEFAULT_ASK_COMMAND.into(),
            timeout_sec: 120,
            max_concurrent: 1,
            prefer: AskPrefer::Attached,
            handoff_sec: 20,
            redact: ["**/*.env", "**/appsettings*.json", "**/secrets*", "**/*.pem", "**/*.key"]
                .map(String::from)
                .to_vec(),
            allowed: false,
        }
    }
}

impl Settings {
    /// `%LOCALAPPDATA%\Rain\settings.json`, the temp dir when `LOCALAPPDATA` is unset.
    pub fn default_path() -> PathBuf {
        rain_comments::store::data_dir().join("settings.json")
    }

    /// The settings at `path`; missing or unreadable files give the defaults.
    pub fn load(path: &Path) -> Settings {
        Settings::try_load(path).unwrap_or_default()
    }

    /// The settings at `path`, the defaults when it doesn't exist; an error names a file that
    /// exists but can't be read or parsed, so it isn't saved over.
    pub fn try_load(path: &Path) -> Result<Settings, String> {
        match std::fs::read(path) {
            Ok(b) => Settings::parse(&b).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Parses a settings file; `splitView: false` without `layout` becomes unified.
    pub fn parse(bytes: &[u8]) -> Result<Settings, serde_json::Error> {
        let raw: serde_json::Value = serde_json::from_slice(bytes)?;
        let mut s: Settings = serde_json::from_value(raw.clone())?;
        s.clamp();
        let Some(obj) = raw.as_object() else { return Ok(s) };
        if !obj.contains_key("layout") && obj.get("splitView") == Some(&serde_json::Value::Bool(false)) {
            s.layout = ViewLayout::Unified;
        }
        s.migrated = LEGACY_KEYS.iter().any(|k| obj.contains_key(*k));
        Ok(s)
    }

    /// Keeps hand-edited durations where `Duration` and `Instant` arithmetic can't overflow.
    fn clamp(&mut self) {
        const DAY_SECS: u64 = 86_400;
        let b = &mut self.break_reminder;
        b.minutes = b.minutes.clamp(0.05, 1440.0);
        b.snooze_minutes = b.snooze_minutes.clamp(0.05, 1440.0);
        let a = &mut self.ask;
        a.timeout_sec = a.timeout_sec.clamp(1, DAY_SECS);
        a.handoff_sec = a.handoff_sec.min(DAY_SECS);
        a.max_concurrent = a.max_concurrent.clamp(1, 64);
        let (w, h) = self.picker_size;
        if !(w.is_finite() && h.is_finite() && w >= PICKER_MIN.0 && h >= PICKER_MIN.1) {
            self.picker_size = Settings::default().picker_size;
        }
    }

    /// Writes every key, atomically.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        rain_comments::store::write_atomic(path, &bytes)
    }

    /// Writes only what changed since `base` (what this window last loaded or saved) over the
    /// file as it is now, so another window's changes to other keys survive. A missing file gets
    /// every key; a malformed hand edit is reported and left untouched.
    pub fn save_merged(&self, base: &Settings, path: &Path) -> std::io::Result<()> {
        let mine = serde_json::to_value(self).map_err(std::io::Error::other)?;
        let was = serde_json::to_value(base).map_err(std::io::Error::other)?;
        let mut disk = match std::fs::read(path) {
            Ok(b) => {
                serde_json::from_slice::<serde_json::Value>(&b).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => mine.clone(),
            Err(e) => return Err(e),
        };
        if !disk.is_object() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "settings must be a JSON object",
            ));
        }
        merge_changes(&mut disk, &was, &mine);
        let bytes = serde_json::to_vec_pretty(&disk).map_err(std::io::Error::other)?;
        rain_comments::store::write_atomic(path, &bytes)
    }

    pub fn section_collapsed(&self, section: u8) -> bool {
        self.sidebar_sections & (1 << section) != 0
    }

    pub fn toggle_section(&mut self, section: u8) {
        self.sidebar_sections ^= 1 << section;
    }
}

/// Puts into `disk` each leaf where `mine` differs from `was`; nested objects merge key by key.
fn merge_changes(disk: &mut serde_json::Value, was: &serde_json::Value, mine: &serde_json::Value) {
    use serde_json::Value;
    let (Some(was), Some(mine), Some(disk)) = (was.as_object(), mine.as_object(), disk.as_object_mut()) else {
        return;
    };
    for k in was.keys().chain(mine.keys().filter(|k| !was.contains_key(*k))) {
        let (w, m) = (was.get(k), mine.get(k));
        if w == m {
            continue;
        }
        if let (Some(w @ Value::Object(_)), Some(m @ Value::Object(_)), Some(d)) = (w, m, disk.get_mut(k).filter(|d| d.is_object())) {
            merge_changes(d, w, m);
            continue;
        }
        match m {
            Some(m) => {
                disk.insert(k.clone(), m.clone());
            }
            None => {
                disk.remove(k);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_windows_saving_keep_each_others_changes() {
        let dir = std::env::temp_dir().join(format!("rain-settings-merge-{}", std::process::id()));
        let path = dir.join("settings.json");
        let _ = std::fs::remove_dir_all(&dir);
        let start = Settings::default();
        start.save(&path).unwrap();
        let mut a = Settings::load(&path);
        let mut b = Settings::load(&path);
        let (base_a, base_b) = (a.clone(), b.clone());
        a.theme = Theme::Everforest;
        a.ask.allowed = true;
        a.save_merged(&base_a, &path).unwrap();
        b.window = (800.0, 600.0);
        b.save_merged(&base_b, &path).unwrap();
        let back = Settings::load(&path);
        assert_eq!(back.theme, Theme::Everforest, "B's resize doesn't undo A's theme");
        assert!(back.ask.allowed, "nested keys merge too");
        assert_eq!(back.window, (800.0, 600.0));
        std::fs::write(&path, br#"{"theme": "dark", "fromANewerBuild": 1}"#).unwrap();
        let mut c = back.clone();
        c.code_size = CodeSize::S;
        c.save_merged(&back, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("fromANewerBuild") && text.contains(r#""theme": "dark""#), "{text}");
        assert_eq!(Settings::load(&path).code_size, CodeSize::S);
        for invalid in [r#"{"editor":"my-editor",}"#, "[]", "null"] {
            std::fs::write(&path, invalid).unwrap();
            let error = c
                .save_merged(&back, &path)
                .expect_err("a broken hand edit must survive a later save");
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn settings_round_trip_and_default_on_garbage() {
        let dir = std::env::temp_dir().join(format!("rain-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        let s = Settings {
            layout: ViewLayout::Unified,
            sidebar_sections: 0b101,
            list_open: false,
            code_size: CodeSize::L,
            theme: Theme::Dark,
            ..Settings::default()
        };
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path), s);
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        assert!(Settings::try_load(&path).is_err());
        assert_eq!(Settings::try_load(&dir.join("missing.json")), Ok(Settings::default()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn editor_is_read_and_only_written_when_set() {
        let s: Settings = serde_json::from_str(r#"{"editor": "rider64 --line {line} \"{path}\""}"#).unwrap();
        assert_eq!(s.editor.as_deref(), Some(r#"rider64 --line {line} "{path}""#));
        assert!(!serde_json::to_string(&Settings::default()).unwrap().contains("editor"));
    }

    #[test]
    fn split_is_the_default_and_split_view_false_migrates_to_unified() {
        assert_eq!(Settings::default().layout, ViewLayout::Split);
        let old = Settings::parse(br#"{"splitView": false, "theme": "dark", "inspectorOpen": true, "inspectorSections": 3}"#).unwrap();
        assert_eq!(old.layout, ViewLayout::Unified);
        assert_eq!(old.theme, Theme::Dark);
        assert!(old.migrated);
        let on = Settings::parse(br#"{"splitView": true}"#).unwrap();
        assert_eq!(on.layout, ViewLayout::Split);
        let chosen = Settings::parse(br#"{"splitView": false, "layout": "auto"}"#).unwrap();
        assert_eq!(chosen.layout, ViewLayout::Auto, "an explicit layout wins");
        assert!(!Settings::parse(br#"{"layout": "split"}"#).unwrap().migrated);
        let out = serde_json::to_string(&old).unwrap();
        for k in LEGACY_KEYS {
            assert!(!out.contains(k), "{k} is not written: {out}");
        }
    }

    #[test]
    fn ask_defaults_fill_in_and_partial_files_keep_the_rest() {
        let d = AskSettings::default();
        assert!(d.enabled);
        assert_eq!(d.command, DEFAULT_ASK_COMMAND);
        assert_eq!((d.timeout_sec, d.max_concurrent, d.handoff_sec), (120, 1, 20));
        assert_eq!(d.prefer, AskPrefer::Attached);
        let old = Settings::parse(br#"{"layout": "split"}"#).unwrap();
        assert_eq!(old.ask, d, "a file without `ask` gets the defaults");
        let s = Settings::parse(br#"{"ask": {"enabled": false, "prefer": "launcher", "timeoutSec": 30}}"#).unwrap();
        assert!(!s.ask.enabled);
        assert_eq!(s.ask.prefer, AskPrefer::Launcher);
        assert_eq!(s.ask.timeout_sec, 30);
        assert_eq!(s.ask.command, DEFAULT_ASK_COMMAND);
        let out = serde_json::to_string(&s).unwrap();
        assert!(out.contains(r#""timeoutSec":30"#) && out.contains(r#""maxConcurrent":1"#), "{out}");
    }

    #[test]
    fn huge_hand_edited_durations_are_clamped_on_load() {
        let s = Settings::parse(
            br#"{"breakReminder": {"minutes": 1e300, "snoozeMinutes": -5},
                 "ask": {"timeoutSec": 18446744073709551615, "handoffSec": 18446744073709551615, "maxConcurrent": 0}}"#,
        )
        .unwrap();
        assert_eq!((s.break_reminder.minutes, s.break_reminder.snooze_minutes), (1440.0, 0.05));
        assert_eq!((s.ask.timeout_sec, s.ask.handoff_sec, s.ask.max_concurrent), (86_400, 86_400, 1));
        let _ = std::time::Instant::now() + std::time::Duration::from_secs(s.ask.handoff_sec);
        let _ = std::time::Duration::from_secs_f64(s.break_reminder.minutes * 60.0);
    }

    #[test]
    fn an_old_chrome_key_is_ignored_and_never_written_back() {
        let dir = std::env::temp_dir().join(format!("rain-settings-chrome-{}", std::process::id()));
        let path = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        for old in ["islands", "islands-deep", "zed"] {
            let text = format!(r#"{{"theme": "dark", "chrome": "{old}", "codeSize": "L"}}"#);
            std::fs::write(&path, text).unwrap();
            let s = Settings::try_load(&path).expect("an old file still loads");
            assert_eq!((s.theme, s.code_size), (Theme::Dark, CodeSize::L), "the other keys are kept");
            assert!(!s.migrated);
            s.save(&path).unwrap();
            let out = std::fs::read_to_string(&path).unwrap();
            assert!(!out.contains("chrome"), "{out}");
            assert_eq!(Settings::load(&path), s);
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn every_theme_round_trips_and_an_unknown_one_falls_back() {
        let dir = std::env::temp_dir().join(format!("rain-settings-theme-{}", std::process::id()));
        let path = dir.join("settings.json");
        for (theme, word) in [
            (Theme::System, "system"),
            (Theme::Light, "light"),
            (Theme::Dark, "dark"),
            (Theme::Everforest, "everforest"),
            (Theme::GruvboxHard, "gruvbox-hard"),
            (Theme::GruvboxMedium, "gruvbox-medium"),
        ] {
            let s = Settings {
                theme,
                code_size: CodeSize::L,
                ..Settings::default()
            };
            s.save(&path).unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(text.contains(&format!("\"theme\": \"{word}\"")), "{text}");
            assert_eq!(Settings::load(&path), s);
            let typed = Settings::parse(format!(r#"{{"theme": "{word}"}}"#).as_bytes()).unwrap();
            assert_eq!(typed.theme, theme);
        }
        for odd in [r#""solarized""#, "42", "null", r#""Dark""#] {
            let s = Settings::parse(format!(r#"{{"theme": {odd}, "codeSize": "L"}}"#).as_bytes())
                .unwrap_or_else(|e| panic!("{odd} still loads: {e}"));
            assert_eq!((s.theme, s.code_size), (Theme::System, CodeSize::L), "{odd}");
        }
        assert_eq!(Settings::parse(br#"{"codeSize": "S"}"#).unwrap().theme, Theme::System);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_theme_draws_its_scheme() {
        assert_eq!(Theme::System.scheme(true), Scheme::Dark);
        assert_eq!(Theme::System.scheme(false), Scheme::Light);
        assert_eq!(Theme::Light.scheme(true), Scheme::Light);
        assert_eq!(Theme::Everforest.scheme(false), Scheme::Everforest);
        assert_eq!(Theme::GruvboxHard.scheme(false), Scheme::GruvboxHard);
        assert_eq!(Theme::GruvboxMedium.scheme(false), Scheme::GruvboxMedium);
        assert_eq!(Theme::from_flag("gruvbox-hard"), Some(Theme::GruvboxHard));
        assert_eq!(Theme::from_flag("dark"), None, "Rune's own --theme values stay Rune's");
    }

    #[test]
    fn two_key_shortcuts_and_the_timeout_round_trip_and_old_files_read_as_before() {
        use rain_core::Command;
        let old = Settings::parse(br#"{"keymap": {"profile": "vim", "overrides": {"comment": ["Ctrl+M"]}}}"#).unwrap();
        assert_eq!(old.keymap.timeout_ms, None);
        assert_eq!(old.keymap.timeout(), std::time::Duration::from_millis(1000), "Vim's timeoutlen");
        let text = serde_json::to_string(&old).unwrap();
        assert!(!text.contains("timeoutMs"), "an unset timeout is never written: {text}");
        let (km, problems) = old.keymap.build();
        assert!(problems.is_empty() && km.waiting_chord().is_none());
        assert_eq!(km.hint(Command::Comment), "Ctrl+M");

        let new = Settings::parse(br#"{"keymap": {"timeoutMs": 400, "overrides": {"reload": ["R", "g r"]}}}"#).unwrap();
        assert_eq!(new.keymap.timeout(), std::time::Duration::from_millis(400));
        let (km, problems) = new.keymap.build();
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(
            km.chords(Command::Reload).iter().map(|k| k.display()).collect::<Vec<_>>(),
            ["R", "g r"]
        );
        let back = Settings::parse(serde_json::to_string(&new).unwrap().as_bytes()).unwrap();
        assert_eq!(back.keymap, new.keymap);

        let junk = Settings::parse(br#"{"keymap": {"timeoutMs": "soon", "overrides": {"reload": ["g r x"]}}}"#).unwrap();
        assert_eq!(junk.keymap.timeout_ms, None, "a bad timeout is the default");
        assert_eq!(junk.keymap.build().1.len(), 1);
    }

    #[test]
    fn keymap_overrides_round_trip_and_bad_entries_are_reported_not_fatal() {
        use rain_core::Command;
        let dir = std::env::temp_dir().join(format!("rain-settings-keymap-{}", std::process::id()));
        let path = dir.join("settings.json");
        let mut s = Settings::default();
        assert_eq!(s.keymap.profile, Profile::Vim, "Vim is the default profile");
        assert!(!serde_json::to_string(&s).unwrap().contains("overrides"));
        s.keymap.profile = Profile::JetBrains;
        let mut o = BTreeMap::new();
        o.insert("comment".to_owned(), vec!["Ctrl+M".to_owned()]);
        o.insert("ask".to_owned(), vec![]);
        s.keymap.set_overrides(&o);
        s.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains(r#""profile": "jetbrains""#) && text.contains(r#""comment": ["#),
            "{text}"
        );
        let back = Settings::load(&path);
        assert_eq!(back, s);
        let (km, problems) = back.keymap.build();
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(km.hint(Command::Comment), "Ctrl+M");
        assert!(km.chords(Command::Ask).is_empty());
        assert_eq!(km.hint(Command::NextHunk), "F7");

        let odd = Settings::parse(
            br#"{"codeSize": "L", "keymap": {"profile": "emacs", "overrides": {"comment": "Ctrl+M", "find": [3], "no-such": ["Ctrl+J"], "picker": ["Ctrl+Q"], "go-to-file": ["Ctrl+Q", "Hyper+X"], "reply": ["Up"]}}}"#,
        )
        .expect("bad keymap entries never fail the file");
        assert_eq!((odd.code_size, odd.keymap.profile), (CodeSize::L, Profile::Vim));
        let (km, problems) = odd.keymap.build();
        assert_eq!(problems.len(), 6, "{problems:?}");
        assert_eq!(km.hint(Command::Picker), "Ctrl+Q");
        assert_eq!(km.hint(Command::Comment), "c", "an ignored entry leaves the default");
        assert_eq!(
            km.hint(Command::Reply),
            "",
            "the universal key is refused, the entry still replaces"
        );
        let junk = Settings::parse(br#"{"keymap": 42, "theme": "dark"}"#).unwrap();
        assert_eq!((junk.keymap, junk.theme), (KeymapSettings::default(), Theme::Dark));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn sections_fold_by_bit() {
        let mut s = Settings::default();
        assert!(!s.section_collapsed(3));
        s.toggle_section(3);
        assert!(s.section_collapsed(3));
        assert_eq!(s.sidebar_sections, 0b1000);
        s.toggle_section(3);
        assert_eq!(s.sidebar_sections, 0);
    }
}
