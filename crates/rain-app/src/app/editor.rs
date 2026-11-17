//! Open in editor (Ctrl+O, the file header's Open button) and copy location (`Shift+Y`).

use std::io;
use std::path::PathBuf;

use rain_core::{FileStatus, Side};
use rune::Tx;

use super::App;
use crate::settings::DEFAULT_EDITOR;

/// Starts the editor from its argv without waiting for it; tests swap in a recorder.
pub type Launcher = Box<dyn Fn(&[String]) -> io::Result<()>>;

impl App {
    /// Full-list file `g` on disk under its repo's folder, or why it can't open in an editor.
    pub(crate) fn disk_path(&self, g: usize) -> Result<PathBuf, &'static str> {
        let f = self.diff.files().get(g).ok_or("No file")?;
        let repo = self.repos.get(f.repo as usize).filter(|r| !r.dir.as_os_str().is_empty());
        let Some(repo) = repo else {
            return Err("No working folder: this diff comes from a patch file");
        };
        let path = repo.dir.join(&f.path);
        if f.status == FileStatus::Deleted || !path.is_file() {
            return Err("Not on disk: deleted or not checked out");
        }
        Ok(path)
    }

    /// The cursor's line when it is on full-list file `g`, as a new-side line unless the file is
    /// deleted: an old-side line maps to the new line where it was removed.
    pub(crate) fn cursor_line(&self, g: usize) -> Option<u32> {
        let (local, at) = self.active().cursor()?;
        if self.global(local) != g {
            return None;
        }
        let f = &self.diff.files()[g];
        if at.side == Side::New || f.status == FileStatus::Deleted {
            return Some(at.line);
        }
        let i = f
            .lines
            .iter()
            .position(|l| l.old == at.line && l.kind != rain_core::LineKind::Added)?;
        let after = f.lines[i..].iter().find(|l| l.new > 0).map(|l| l.new);
        after.or_else(|| f.lines[..i].iter().rev().find(|l| l.new > 0).map(|l| l.new + 1))
    }

    pub fn open_in_editor(&mut self) {
        match self.current() {
            Some(g) => self.open_file_in_editor(g),
            None => self.status = "No file to open".into(),
        }
    }

    /// Opens full-list file `g` at the cursor line (or its top) with the `editor` setting.
    pub fn open_file_in_editor(&mut self, g: usize) {
        let path = match self.disk_path(g) {
            Ok(p) => p,
            Err(why) => {
                self.status = format!("Can't open in editor: {why}");
                return;
            }
        };
        let line = self.cursor_line(g).unwrap_or(1);
        let template = self.settings.editor.as_deref().unwrap_or(DEFAULT_EDITOR);
        let argv = editor_argv(template, &path.to_string_lossy(), line);
        if argv.is_empty() {
            self.status = "Can't open in editor: the editor setting is empty".into();
            return;
        }
        self.status = match (self.launcher)(&argv) {
            Ok(()) => "Opened in editor".into(),
            Err(e) => format!("Could not start {}: {e}", argv[0]),
        };
    }

    /// `<repoId>/<path>:<line>` of the cursor line (or `<repoId>/<path>` with no cursor on the
    /// file); one-repo windows leave the repo id out.
    pub fn location(&self, g: usize) -> Option<String> {
        let f = self.diff.files().get(g)?;
        let path = if self.repos.len() > 1 {
            f.display_path(&self.repos)
        } else {
            f.path.clone()
        };
        Some(match self.cursor_line(g) {
            Some(line) => format!("{path}:{line}"),
            None => path,
        })
    }

    pub fn copy_location(&mut self, tx: &mut Tx) {
        match self.current().and_then(|g| self.location(g)) {
            Some(loc) => {
                tx.copy(loc.clone());
                self.status = format!("Copied {loc}");
            }
            None => self.status = "No file to copy the location of".into(),
        }
    }
}

/// The template split into arguments (double quotes group, and are dropped), with `{path}` and
/// `{line}` filled in each.
pub(crate) fn editor_argv(template: &str, path: &str, line: u32) -> Vec<String> {
    let mut args = Vec::new();
    let (mut cur, mut quoted, mut any) = (String::new(), false, false);
    for c in template.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any || !cur.is_empty() {
                    args.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        args.push(cur);
    }
    args.into_iter()
        .map(|a| a.replace("{line}", &line.to_string()).replace("{path}", path))
        .collect()
}

/// Starts `argv` detached, with no console window. On Windows the program is looked up on
/// `PATH` with `PATHEXT` (so `code`, a `.cmd`, resolves) and started directly: std quotes the
/// arguments for an `.exe` and escapes them for a batch file, `%` included. Only a program not
/// found that way (a cmd built-in such as `start`) goes through `cmd /C`.
pub(crate) fn spawn_detached(argv: &[String]) -> io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        if let Some(exe) = resolve(&argv[0], std::env::var_os("PATH").as_deref(), &pathext) {
            return std::process::Command::new(exe)
                .args(&argv[1..])
                .creation_flags(CREATE_NO_WINDOW)
                .spawn()
                .map(drop);
        }
        // cmd expands %VAR% even inside quotes and has no escape for it on its command line.
        if argv.iter().any(|a| a.contains(['%', '\n', '\r'])) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is not on PATH, and cmd would expand the % in the path", argv[0]),
            ));
        }
        let line = argv.iter().map(|a| cmd_quote(a)).collect::<Vec<_>>().join(" ");
        std::process::Command::new("cmd")
            .raw_arg(format!("/D /V:OFF /S /C \"{line}\""))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(drop)
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new(&argv[0]).args(&argv[1..]).spawn().map(drop)
    }
}

/// `prog` as a file: a path with an extension as given, else with each `pathext` extension
/// appended; a bare name is searched for in each `path` folder, as cmd would (minus the current
/// folder).
#[cfg(windows)]
fn resolve(prog: &str, path: Option<&std::ffi::OsStr>, pathext: &str) -> Option<PathBuf> {
    use std::path::Path;
    let pick = |base: &Path| -> Option<PathBuf> {
        if base.extension().is_some() && base.is_file() {
            return Some(base.to_path_buf());
        }
        pathext.split(';').filter(|e| !e.is_empty()).find_map(|e| {
            let mut s = base.as_os_str().to_owned();
            s.push(e);
            let c = PathBuf::from(s);
            c.is_file().then_some(c)
        })
    };
    let p = Path::new(prog);
    if prog.is_empty() {
        return None;
    }
    if p.is_absolute() || p.components().count() > 1 {
        return pick(p);
    }
    std::env::split_paths(path?).find_map(|d| pick(&d.join(p)))
}

#[cfg(windows)]
fn cmd_quote(a: &str) -> String {
    if !a.is_empty() && !a.contains([' ', '\t', '&', '(', ')', '^', '|', '<', '>', '"']) {
        return a.to_owned();
    }
    format!("\"{}\"", a.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_split_on_spaces_keep_quoted_parts_and_fill_placeholders() {
        assert_eq!(
            editor_argv(DEFAULT_EDITOR, r"C:\my repo\a.cs", 27),
            ["code", "--goto", r"C:\my repo\a.cs:27"]
        );
        assert_eq!(
            editor_argv(r#""C:\Program Files\Rider\rider64.exe"  --line {line} {path}"#, r"C:\r\b.cs", 3),
            [r"C:\Program Files\Rider\rider64.exe", "--line", "3", r"C:\r\b.cs"]
        );
        assert!(editor_argv("   ", "p", 1).is_empty());
        assert_eq!(editor_argv("ed {path}", r"C:\r\notes{line}.md", 4), ["ed", r"C:\r\notes{line}.md"]);
    }

    #[cfg(windows)]
    #[test]
    fn a_batch_editor_gets_paths_with_cmd_metacharacters_verbatim() {
        let dir = std::env::temp_dir().join(format!("rain-editor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("out.txt");
        std::fs::write(dir.join("ed.cmd"), "@>\"%~dp0out.txt\" echo \"%~1\"\r\n").unwrap();
        let path = std::ffi::OsString::from(&dir);
        assert_eq!(resolve("ed", Some(&path), ".exe;.cmd"), Some(dir.join("ed.cmd")));
        assert_eq!(resolve("nope", Some(&path), ".exe;.cmd"), None);
        let ed = dir.join("ed.cmd").to_string_lossy().into_owned();
        let tricky = r"C:\r\a&b %USERNAME% (x) ^y!.cs".to_owned();
        spawn_detached(&[ed, tricky.clone()]).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let got = loop {
            if let Ok(s) = std::fs::read_to_string(&out)
                && !s.is_empty()
            {
                break s;
            }
            assert!(std::time::Instant::now() < deadline, "the editor never ran");
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        assert_eq!(got.trim_end(), format!("\"{tricky}\""));
        let _ = std::fs::remove_dir_all(dir);
    }
}
