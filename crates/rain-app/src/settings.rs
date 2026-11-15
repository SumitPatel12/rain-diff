//! Window and view preferences kept across runs in `%LOCALAPPDATA%\Rain\settings.json`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
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

/// The window's surface treatment: flat panels split by hairlines, or rounded islands on a ground
/// (in dark, JetBrains' lighter ground or a darker one).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Chrome {
    #[default]
    Zed,
    Islands,
    #[serde(rename = "islands-deep")]
    IslandsDeep,
}

impl Chrome {
    pub const ALL: [Chrome; 3] = [Chrome::Zed, Chrome::Islands, Chrome::IslandsDeep];

    pub fn label(self) -> &'static str {
        match self {
            Chrome::Zed => "Zed",
            Chrome::Islands => "Islands",
            Chrome::IslandsDeep => "Islands, dark ground",
        }
    }

    pub fn parse(s: &str) -> Option<Chrome> {
        match s.trim().to_ascii_lowercase().as_str() {
            "zed" => Some(Chrome::Zed),
            "islands" => Some(Chrome::Islands),
            "islands-deep" => Some(Chrome::IslandsDeep),
            _ => None,
        }
    }

    pub fn islands(self) -> bool {
        self != Chrome::Zed
    }
}

/// An unknown chrome name reads as the default instead of failing the whole file.
fn chrome_or_default<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Chrome, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(v.as_str().and_then(Chrome::parse).unwrap_or_default())
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
    pub theme: Theme,
    pub density: Density,
    #[serde(deserialize_with = "chrome_or_default")]
    pub chrome: Chrome,
    pub layout: ViewLayout,
    pub code_size: CodeSize,
    /// The file list beside the reader; closed it is a rail.
    pub list_open: bool,
    /// Bit `i` set: list section `i` is collapsed.
    pub sidebar_sections: u16,
    /// Open-in-editor command with `{path}` and `{line}`; `None` runs [`DEFAULT_EDITOR`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
    /// Who answers Question-mode threads, and how.
    pub ask: AskSettings,
    /// Set when the file held round-2 keys, so the caller rewrites it once without them.
    #[serde(skip)]
    pub migrated: bool,
}

pub const DEFAULT_EDITOR: &str = r#"code --goto "{path}:{line}""#;

/// Round-2 keys read once and never written again.
const LEGACY_KEYS: [&str; 5] = ["splitView", "inspectorWidth", "inspectorOpen", "inspectorSections", "mode"];

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            window: (1440.0, 900.0),
            sidebar_width: 300.0,
            theme: Theme::System,
            density: Density::Compact,
            chrome: Chrome::Zed,
            layout: ViewLayout::Split,
            code_size: CodeSize::M,
            list_open: true,
            sidebar_sections: 0,
            editor: None,
            ask: AskSettings::default(),
            migrated: false,
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
        let Some(obj) = raw.as_object() else { return Ok(s) };
        if !obj.contains_key("layout") && obj.get("splitView") == Some(&serde_json::Value::Bool(false)) {
            s.layout = ViewLayout::Unified;
        }
        s.migrated = LEGACY_KEYS.iter().any(|k| obj.contains_key(*k));
        Ok(s)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, bytes)
    }

    pub fn section_collapsed(&self, section: u8) -> bool {
        self.sidebar_sections & (1 << section) != 0
    }

    pub fn toggle_section(&mut self, section: u8) {
        self.sidebar_sections ^= 1 << section;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn chrome_defaults_to_zed_round_trips_and_unknown_falls_back() {
        assert_eq!(Settings::default().chrome, Chrome::Zed);
        assert_eq!(Settings::parse(b"{}").unwrap().chrome, Chrome::Zed);
        let s = Settings {
            chrome: Chrome::Islands,
            ..Settings::default()
        };
        let out = serde_json::to_string(&s).unwrap();
        assert!(out.contains(r#""chrome":"islands""#), "{out}");
        assert_eq!(Settings::parse(out.as_bytes()).unwrap(), s);
        let odd = Settings::parse(br#"{"chrome": "fluent", "theme": "dark", "codeSize": "L"}"#).unwrap();
        assert_eq!(odd.chrome, Chrome::Zed);
        assert_eq!((odd.theme, odd.code_size), (Theme::Dark, CodeSize::L), "other keys are kept");
        assert_eq!(Settings::parse(br#"{"chrome": 3}"#).unwrap().chrome, Chrome::Zed);
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
