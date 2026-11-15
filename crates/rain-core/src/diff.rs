//! A compact diff: one text arena per file plus fixed-size line records, so a million-line
//! change costs a few bytes per line beyond its text.

use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthChar;

pub const TAB_WIDTH: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Old,
    #[default]
    New,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[repr(u8)]
pub enum LineKind {
    #[default]
    Context,
    Added,
    Removed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileStatus {
    #[default]
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
}

/// One diff line. `old`/`new` are 1-based line numbers, 0 when the line has no number on that
/// side; `start..end` is the line's byte range in [`FileDiff::text`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct LineRec {
    pub kind: LineKind,
    pub old: u32,
    pub new: u32,
    pub start: u32,
    pub end: u32,
    pub no_newline: bool,
}

/// `section` is the byte range in [`FileDiff::text`] of the text after the closing `@@`;
/// `lines` indexes [`FileDiff::lines`].
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Hunk {
    pub old_start: u32,
    pub old_count: u32,
    pub new_start: u32,
    pub new_count: u32,
    pub section: (u32, u32),
    pub lines: Range<u32>,
}

#[derive(Clone, Debug, Default)]
pub struct FileDiff {
    pub repo: u16,
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileStatus,
    pub binary: bool,
    pub generated: bool,
    pub contents_unavailable: bool,
    pub additions: u32,
    pub deletions: u32,
    /// Widest line in display columns (tabs = 4).
    pub max_columns: u32,
    /// Every hunk line's bytes (and hunk section texts), `'\n'`-joined.
    pub text: String,
    pub lines: Vec<LineRec>,
    pub hunks: Vec<Hunk>,
    /// Whole old/new texts, present only once fetched (for expanding context).
    pub old_text: Option<Arc<str>>,
    pub new_text: Option<Arc<str>>,
}

impl FileDiff {
    pub fn line_text(&self, i: u32) -> &str {
        let rec = &self.lines[i as usize];
        &self.text[rec.start as usize..rec.end as usize]
    }

    pub fn hunk_section(&self, h: &Hunk) -> &str {
        &self.text[h.section.0 as usize..h.section.1 as usize]
    }

    /// `<repoId>/<path>`, or the bare path when the repo index is unknown.
    pub fn display_path(&self, repos: &[RepoInfo]) -> String {
        match repos.get(self.repo as usize) {
            Some(r) if !r.id.is_empty() => crate::paths::join(&r.id, &self.path),
            _ => self.path.clone(),
        }
    }

    pub fn line_count(&self) -> u32 {
        self.lines.len() as u32
    }
}

/// Display width of `s` in columns: tabs advance to the next multiple of [`TAB_WIDTH`], wide
/// characters take two, control characters none.
pub fn display_columns(s: &str) -> u32 {
    let mut col = 0u32;
    if s.is_ascii() {
        for b in s.bytes() {
            col = if b == b'\t' {
                (col / TAB_WIDTH + 1) * TAB_WIDTH
            } else {
                col + u32::from(b >= 0x20 && b != 0x7f)
            };
        }
        return col;
    }
    for c in s.chars() {
        col = if c == '\t' {
            (col / TAB_WIDTH + 1) * TAB_WIDTH
        } else {
            col + c.width().unwrap_or(0) as u32
        };
    }
    col
}

/// Builds a [`FileDiff`] line by line, appending text to the arena.
#[derive(Debug, Default)]
pub struct FileDiffBuilder {
    file: FileDiff,
}

impl FileDiffBuilder {
    pub fn new(repo: u16, path: impl Into<String>, status: FileStatus) -> Self {
        FileDiffBuilder {
            file: FileDiff {
                repo,
                path: path.into(),
                status,
                ..FileDiff::default()
            },
        }
    }

    /// The file's metadata fields (paths, flags); lines go through [`Self::push_line`].
    pub fn file_mut(&mut self) -> &mut FileDiff {
        &mut self.file
    }

    pub fn reserve(&mut self, lines: usize, bytes: usize) {
        self.file.lines.reserve(lines);
        self.file.text.reserve(bytes);
    }

    fn append(&mut self, s: &str) -> (u32, u32) {
        if !self.file.text.is_empty() {
            self.file.text.push('\n');
        }
        let start = self.file.text.len() as u32;
        self.file.text.push_str(s);
        (start, self.file.text.len() as u32)
    }

    pub fn push_hunk(&mut self, old_start: u32, old_count: u32, new_start: u32, new_count: u32, section: &str) {
        let section = self.append(section);
        let at = self.file.lines.len() as u32;
        self.file.hunks.push(Hunk {
            old_start,
            old_count,
            new_start,
            new_count,
            section,
            lines: at..at,
        });
    }

    /// Appends a line to the current hunk (opening an empty one if none exists).
    pub fn push_line(&mut self, kind: LineKind, old: u32, new: u32, text: &str) {
        if self.file.hunks.is_empty() {
            self.push_hunk(old.max(1), 0, new.max(1), 0, "");
        }
        let (start, end) = self.append(text);
        self.file.max_columns = self.file.max_columns.max(display_columns(text));
        match kind {
            LineKind::Added => self.file.additions += 1,
            LineKind::Removed => self.file.deletions += 1,
            LineKind::Context => {}
        }
        self.file.lines.push(LineRec {
            kind,
            old,
            new,
            start,
            end,
            no_newline: false,
        });
        if let Some(h) = self.file.hunks.last_mut() {
            h.lines.end = self.file.lines.len() as u32;
        }
    }

    /// Marks the last pushed line as lacking a trailing newline (`\ No newline at end of file`).
    pub fn mark_no_newline(&mut self) {
        if let Some(l) = self.file.lines.last_mut() {
            l.no_newline = true;
        }
    }

    pub fn finish(self) -> FileDiff {
        self.file
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    /// The overview's repo id, or the folder leaf.
    pub id: String,
    pub dir: PathBuf,
    pub name: String,
    pub base: String,
    pub target: String,
    pub resolved_base: String,
    pub resolved_target: String,
    pub merge_base: bool,
    pub repository_id: String,
}

#[derive(Clone, Debug, Default)]
pub struct DiffSet {
    pub repos: Vec<RepoInfo>,
    pub files: Vec<FileDiff>,
    pub ignore_whitespace: bool,
}

impl DiffSet {
    pub fn total_lines(&self) -> u64 {
        self.files.iter().map(|f| f.lines.len() as u64).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_rec_is_at_most_24_bytes() {
        assert!(
            std::mem::size_of::<LineRec>() <= 24,
            "LineRec is {} bytes",
            std::mem::size_of::<LineRec>()
        );
    }

    #[test]
    fn builder_tracks_text_counts_and_columns() {
        let mut b = FileDiffBuilder::new(0, "src/a.cs", FileStatus::Modified);
        b.push_hunk(1, 2, 1, 2, " class A");
        b.push_line(LineKind::Context, 1, 1, "using System;");
        b.push_line(LineKind::Removed, 2, 0, "\tint x;");
        b.push_line(LineKind::Added, 0, 2, "\tint y; // 中文");
        b.mark_no_newline();
        let f = b.finish();
        assert_eq!(f.line_text(0), "using System;");
        assert_eq!(f.line_text(1), "\tint x;");
        assert_eq!(f.line_text(2), "\tint y; // 中文");
        assert_eq!(f.hunk_section(&f.hunks[0]), " class A");
        assert_eq!(f.hunks[0].lines, 0..3);
        assert_eq!((f.additions, f.deletions), (1, 1));
        assert!(f.lines[2].no_newline);
        assert_eq!(f.max_columns, 4 + "int y; // ".len() as u32 + 4);
    }

    #[test]
    fn columns_expand_tabs_to_the_next_stop() {
        assert_eq!(display_columns("\t"), 4);
        assert_eq!(display_columns("ab\tc"), 5);
        assert_eq!(display_columns("😀a"), 3);
    }

    #[test]
    fn display_path_prefixes_the_repo_id() {
        let repos = vec![RepoInfo {
            id: "server".into(),
            ..RepoInfo::default()
        }];
        let f = FileDiff {
            path: "src/x.cs".into(),
            ..FileDiff::default()
        };
        assert_eq!(f.display_path(&repos), "server/src/x.cs");
        assert_eq!(f.display_path(&[]), "src/x.cs");
    }
}
