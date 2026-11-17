//! Every file of the diff on screen folded to its header, or opened again.

use rain_core::FileDiff;

use super::App;
use crate::load::LARGE_FILE_LINES;

/// Whether `f` is too large to open with the rest: a load leaves it folded.
pub(super) fn large(f: &FileDiff) -> bool {
    f.lines.len() > LARGE_FILE_LINES || (f.generated && f.lines.len() > 2_000)
}

impl App {
    /// Folds every file of the diff on screen to its header, with one rebuild.
    pub fn collapse_all_files(&mut self) {
        self.active_mut().set_collapsed_where(true, |_| true);
        self.status = "All files collapsed".into();
    }

    /// Opens every file of the diff on screen, with one rebuild; the large ones a load folds
    /// stay folded (each opens from its header).
    pub fn expand_all_files(&mut self) {
        self.active_mut().set_collapsed_where(false, |f| !large(f));
        let a = self.active();
        let left = (0..a.files().len()).filter(|&l| a.collapsed(l)).count();
        self.status = match left {
            0 => "All files expanded".into(),
            1 => "Files expanded; 1 large file stays collapsed".into(),
            n => format!("Files expanded; {n} large files stay collapsed"),
        };
    }
}
