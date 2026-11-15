//! Subsets (a filter reads as its own diff over some of the files, sharing their text with the
//! full diff) and chapters, which are a grouping of files now, not a reading mode.

use std::sync::Arc;

use rain_diffview::DiffState;
use rune::Tx;

use super::App;
use crate::load::LARGE_FILE_LINES;

/// A diff over some of the files, in the given order.
pub struct Subset {
    pub diff: DiffState,
    /// Full-list index of each of `diff`'s files.
    pub files: Vec<usize>,
}

impl Subset {
    /// The files' diff with the app's repo ids, highlighter and mode; files over
    /// `LARGE_FILE_LINES` lines start collapsed. Whole files and opened ranges carry over from
    /// `prev` for the files it held, else from the full diff.
    pub fn new(app: &App, files: Vec<usize>, prev: Option<&Subset>) -> Subset {
        let mut diff = DiffState::new(files.iter().map(|&g| Arc::clone(&app.diff.files()[g])).collect());
        diff.set_repo_ids(app.repos.iter().map(|r| r.id.clone()).collect());
        if let Some(palette) = app.hl_palette {
            diff.set_highlighter(rain_syntax::highlighter(palette));
        }
        diff.set_mode(app.diff.mode());
        diff.set_collapsed_where(true, |f| f.lines.len() > LARGE_FILE_LINES);
        diff.set_opened(files.iter().enumerate().filter_map(|(l, &g)| {
            let from_prev = prev.and_then(|p| p.files.iter().position(|&x| x == g).and_then(|pl| p.diff.opened(pl)));
            Some((l, from_prev.or_else(|| app.diff.opened(g))?))
        }));
        Subset { diff, files }
    }
}

impl App {
    /// Chapters no longer narrow the reader; kept for callers that refresh one.
    pub(crate) fn in_chapter(&self) -> bool {
        false
    }

    /// Makes chapter `i` the current one.
    pub fn enter_chapter(&mut self, i: usize) {
        self.chapter = i.min(self.chapters.chapters.len().saturating_sub(1));
    }

    /// Full-list indexes of chapter `i`'s files in the diff, in chapter order.
    pub fn chapter_files(&self, i: usize) -> Vec<usize> {
        let mut files: Vec<usize> = Vec::new();
        for item in self.chapters.chapters.get(i).map(|c| c.items.as_slice()).unwrap_or_default() {
            if let Some(g) = self.index_of(&item.repo_path)
                && !files.contains(&g)
            {
                files.push(g);
            }
        }
        files
    }

    /// Jumps to chapter `i`'s first file in the diff.
    pub fn goto_chapter(&mut self, i: usize, tx: &mut Tx) {
        self.enter_chapter(i);
        match self.chapter_files(self.chapter).first() {
            Some(&g) => {
                self.goto_file(g, tx);
                if let Some(c) = self.chapters.chapters.get(self.chapter) {
                    self.status = format!("Chapter {}: {}", self.chapter + 1, c.title);
                }
            }
            None => self.status = "This chapter has no files in the diff".into(),
        }
    }

    pub fn step_chapter(&mut self, forward: bool, tx: &mut Tx) {
        let n = self.chapters.chapters.len();
        if n == 0 {
            self.status = "No chapters: this session has no overview walkthrough".into();
            return;
        }
        let next = if forward {
            (self.chapter + 1).min(n - 1)
        } else {
            self.chapter.saturating_sub(1)
        };
        self.goto_chapter(next, tx);
    }

    pub fn chapter_done(&self, i: usize) -> bool {
        let files = self.chapter_files(i);
        !files.is_empty() && files.iter().all(|&f| self.is_viewed(f))
    }
}
