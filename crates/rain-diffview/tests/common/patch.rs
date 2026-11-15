//! Test and example helpers: a small unified-diff parser and a two-text differ that build core
//! `FileDiff`s, ported from rune-kit's `diff/model.rs`. The app gets its files from
//! `rain-git`; this exists so the view's tests and example need no git.

#![allow(dead_code)]

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use rain_core::diff::{FileDiff, FileDiffBuilder, FileStatus, LineKind};
use rain_diffview::myers::{self, Op};

/// Lines of context around each change in a diff computed from texts, as `git diff` keeps.
const CONTEXT: u32 = 3;

/// Each line's byte range in `text`, without its line break.
fn split(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, _) in text.match_indices('\n') {
        let end = if text[..i].ends_with('\r') { i - 1 } else { i };
        out.push(&text[start..end]);
        start = i + 1;
    }
    if start < text.len() {
        let end = if text.ends_with('\r') { text.len() - 1 } else { text.len() };
        out.push(&text[start..end]);
    }
    out
}

/// The diff of `old` to `new`, a file at `path`, with three lines of context around each change.
/// Both texts stay with it, so every unchanged range between hunks can be expanded.
pub fn from_texts(path: &str, old: &str, new: &str) -> FileDiff {
    between(Some(path), Some(path), old, new)
}

/// [`from_texts`] for a rename (`old_path` differs), an added file (`old_path` `None`, `old`
/// empty) or a deleted one (`new_path` `None`, `new` empty).
pub fn between(old_path: Option<&str>, new_path: Option<&str>, old: &str, new: &str) -> FileDiff {
    let status = match (old_path, new_path) {
        (None, _) => FileStatus::Added,
        (_, None) => FileStatus::Deleted,
        (Some(a), Some(b)) if a != b => FileStatus::Renamed,
        _ => FileStatus::Modified,
    };
    let path = new_path.or(old_path).unwrap_or("");
    let mut b = FileDiffBuilder::new(0, path, status);
    if status == FileStatus::Renamed {
        b.file_mut().old_path = old_path.map(str::to_owned);
    }
    if old.contains('\0') || new.contains('\0') {
        b.file_mut().binary = true;
        return b.finish();
    }
    let (a, c) = (split(old), split(new));
    let ends = |t: &str| !t.is_empty() && !t.ends_with('\n');
    hunks(&mut b, &a, &c, ends(old), ends(new));
    let mut f = b.finish();
    f.old_text = Some(Arc::from(old));
    f.new_text = Some(Arc::from(new));
    f
}

fn intern<'a>(s: &[&'a str], open: bool, ids: &mut HashMap<(&'a str, bool), u32>) -> Vec<u32> {
    let last = s.len();
    s.iter()
        .enumerate()
        .map(|(i, l)| {
            let key = (*l, i + 1 == last && open);
            let next = ids.len() as u32;
            *ids.entry(key).or_insert(next)
        })
        .collect()
}

fn hunks(b: &mut FileDiffBuilder, old: &[&str], new: &[&str], old_open: bool, new_open: bool) {
    let mut ids: HashMap<(&str, bool), u32> = HashMap::new();
    let ia = intern(old, old_open, &mut ids);
    let ib = intern(new, new_open, &mut ids);
    let ops = myers::diff(&ia, &ib);
    let mut changes: Vec<(Range<u32>, Range<u32>)> = Vec::new();
    let (mut i, mut j) = (0u32, 0u32);
    for op in &ops {
        match *op {
            Op::Equal { len, .. } => {
                i += len as u32;
                j += len as u32;
            }
            Op::Delete { len, .. } => {
                match changes.last_mut() {
                    Some(c) if c.0.end == i && c.1.end == j => c.0.end += len as u32,
                    _ => changes.push((i..i + len as u32, j..j)),
                }
                i += len as u32;
            }
            Op::Insert { len, .. } => {
                match changes.last_mut() {
                    Some(c) if c.0.end == i && c.1.end == j => c.1.end += len as u32,
                    _ => changes.push((i..i, j..j + len as u32)),
                }
                j += len as u32;
            }
        }
    }
    let mut groups: Vec<Vec<(Range<u32>, Range<u32>)>> = Vec::new();
    for c in changes {
        match groups.last_mut().and_then(|g| g.last().cloned().map(|l| (g, l))) {
            Some((g, last)) if c.0.start - last.0.end <= 2 * CONTEXT => g.push(c),
            _ => groups.push(vec![c]),
        }
    }
    let (olen, nlen) = (old.len() as u32, new.len() as u32);
    let line = |s: &[&str], n: u32| s.get(n as usize).copied().unwrap_or("").to_owned();
    for g in groups {
        let (first, last) = (&g[0], &g[g.len() - 1]);
        let start = first.0.start.saturating_sub(CONTEXT);
        let shift = first.0.start - start;
        let (os, ns) = (start, first.1.start - shift);
        let end_old = (last.0.end + CONTEXT).min(olen);
        let mut lines: Vec<(LineKind, u32, u32, String)> = Vec::new();
        let (mut i, mut j) = (os, ns);
        for c in &g {
            while i < c.0.start {
                lines.push((LineKind::Context, i + 1, j + 1, line(old, i)));
                i += 1;
                j += 1;
            }
            for n in c.0.clone() {
                lines.push((LineKind::Removed, n + 1, 0, line(old, n)));
            }
            for n in c.1.clone() {
                lines.push((LineKind::Added, 0, n + 1, line(new, n)));
            }
            i = c.0.end;
            j = c.1.end;
        }
        while i < end_old {
            lines.push((LineKind::Context, i + 1, j + 1, line(old, i)));
            i += 1;
            j += 1;
        }
        let (oc, nc) = (i - os, j - ns);
        b.push_hunk(
            if oc == 0 { os } else { os + 1 },
            oc,
            if nc == 0 { ns } else { ns + 1 },
            nc,
            &section(old, os),
        );
        for (kind, o, n, text) in lines {
            b.push_line(kind, o, n, &text);
            let eof = match kind {
                LineKind::Removed | LineKind::Context => o == olen && old_open,
                LineKind::Added => n == nlen && new_open,
            };
            if eof {
                b.mark_no_newline();
            }
        }
    }
}

/// The nearest line above 0-based line `before` that starts with a letter, `_` or `$`, as `git
/// diff` names a hunk by default.
fn section(s: &[&str], before: u32) -> String {
    (0..before.min(s.len() as u32))
        .rev()
        .take(2000)
        .map(|n| s[n as usize])
        .find(|l| l.starts_with(|c: char| c.is_alphabetic() || c == '_' || c == '$'))
        .map(|l| l.trim_end().chars().take(80).collect())
        .unwrap_or_default()
}

struct Pending {
    old: Option<String>,
    new: Option<String>,
    status: FileStatus,
    binary: bool,
    b: Option<FileDiffBuilder>,
}

impl Pending {
    fn new(old: Option<String>, new: Option<String>) -> Pending {
        Pending {
            old,
            new,
            status: FileStatus::Modified,
            binary: false,
            b: None,
        }
    }

    fn status(&self) -> FileStatus {
        if self.status != FileStatus::Modified {
            return self.status;
        }
        match (&self.old, &self.new) {
            (None, Some(_)) => FileStatus::Added,
            (Some(_), None) => FileStatus::Deleted,
            (Some(a), Some(b)) if a != b => FileStatus::Renamed,
            _ => FileStatus::Modified,
        }
    }

    fn builder(&mut self) -> &mut FileDiffBuilder {
        let status = self.status();
        let path = self.new.clone().or_else(|| self.old.clone()).unwrap_or_default();
        self.b.get_or_insert_with(|| FileDiffBuilder::new(0, path, status))
    }

    fn finish(mut self) -> FileDiff {
        let status = self.status();
        let binary = self.binary;
        let old = self.old.clone();
        let b = self.builder();
        let f = b.file_mut();
        f.status = status;
        f.binary = binary;
        if matches!(status, FileStatus::Renamed | FileStatus::Copied) {
            f.old_path = old;
        }
        self.b.take().map(FileDiffBuilder::finish).unwrap_or_default()
    }
}

/// Every file in a unified diff as `git diff` writes it: renames, copies, added and deleted
/// files, binary files and `\ No newline at end of file`.
pub fn parse(patch: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    let lines: Vec<&str> = patch.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let mut i = 0;
    let mut cur: Option<Pending> = None;
    let mut in_header = false;
    while i < lines.len() {
        let l = lines[i];
        if let Some(rest) = l.strip_prefix("diff --git ") {
            files.extend(cur.take().map(Pending::finish));
            let (a, b) = git_paths(rest);
            cur = Some(Pending::new(a, b));
            in_header = true;
            i += 1;
            continue;
        }
        if l.starts_with("--- ")
            && lines.get(i + 1).is_some_and(|n| n.starts_with("+++ "))
            && lines.get(i + 2).is_some_and(|n| n.starts_with("@@ "))
        {
            let (a, b) = (path_of(&l[4..]), path_of(&lines[i + 1][4..]));
            match cur.as_mut() {
                Some(f) if in_header && f.b.is_none() => {
                    f.old = a;
                    f.new = b;
                }
                _ => {
                    files.extend(cur.take().map(Pending::finish));
                    cur = Some(Pending::new(a, b));
                }
            }
            in_header = false;
            i += 2;
            continue;
        }
        if let Some(f) = cur.as_mut() {
            if l.starts_with("@@ ") {
                in_header = false;
                if let Some(h) = hunk_header(l) {
                    i = hunk_body(f.builder(), h, &lines, i + 1);
                    continue;
                }
            } else if in_header {
                header_line(f, l);
            }
        }
        i += 1;
    }
    files.extend(cur.map(Pending::finish));
    files
}

fn header_line(f: &mut Pending, l: &str) {
    let word = |p: &str| l.strip_prefix(p).map(str::trim);
    if word("new file mode ").is_some() {
        f.status = FileStatus::Added;
        f.old = None;
    } else if word("deleted file mode ").is_some() {
        f.status = FileStatus::Deleted;
        f.new = None;
    } else if let Some(p) = word("rename from ") {
        f.old = Some(unquote(p));
        f.status = FileStatus::Renamed;
    } else if let Some(p) = word("rename to ") {
        f.new = Some(unquote(p));
        f.status = FileStatus::Renamed;
    } else if let Some(p) = word("copy from ") {
        f.old = Some(unquote(p));
        f.status = FileStatus::Copied;
    } else if let Some(p) = word("copy to ") {
        f.new = Some(unquote(p));
        f.status = FileStatus::Copied;
    } else if l.starts_with("Binary files ") || l == "GIT binary patch" {
        f.binary = true;
    }
}

fn git_paths(rest: &str) -> (Option<String>, Option<String>) {
    if rest.starts_with('"')
        && let Some((a, b)) = split_quoted(rest)
    {
        return (Some(strip_prefix(&a)), Some(strip_prefix(&b)));
    }
    let half = rest.len() / 2;
    if rest.len() % 2 == 1
        && rest.as_bytes().get(half) == Some(&b' ')
        && rest.is_char_boundary(half)
        && rest[..half]
            .strip_prefix("a/")
            .zip(rest[half + 1..].strip_prefix("b/"))
            .is_some_and(|(a, b)| a == b)
    {
        return (Some(strip_prefix(&rest[..half])), Some(strip_prefix(&rest[half + 1..])));
    }
    match rest.find(" b/") {
        Some(at) => (Some(strip_prefix(&rest[..at])), Some(strip_prefix(&rest[at + 1..]))),
        None => (None, None),
    }
}

fn split_quoted(rest: &str) -> Option<(String, String)> {
    let mut escaped = false;
    let end = rest.char_indices().skip(1).find_map(|(i, c)| match c {
        _ if escaped => {
            escaped = false;
            None
        }
        '\\' => {
            escaped = true;
            None
        }
        '"' => Some(i + 1),
        _ => None,
    })?;
    Some((unquote(&rest[..end]), unquote(rest[end..].trim_start())))
}

fn unquote(p: &str) -> String {
    let Some(inner) = p.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return p.to_owned();
    };
    let mut bytes = Vec::new();
    let mut it = inner.bytes().peekable();
    while let Some(c) = it.next() {
        if c != b'\\' {
            bytes.push(c);
            continue;
        }
        match it.next() {
            Some(b'n') => bytes.push(b'\n'),
            Some(b't') => bytes.push(b'\t'),
            Some(b'r') => bytes.push(b'\r'),
            Some(d @ b'0'..=b'7') => {
                let mut v = u32::from(d - b'0');
                for _ in 0..2 {
                    match it.peek() {
                        Some(&e @ b'0'..=b'7') => {
                            v = v * 8 + u32::from(e - b'0');
                            it.next();
                        }
                        _ => break,
                    }
                }
                bytes.push(v as u8);
            }
            Some(other) => bytes.push(other),
            None => bytes.push(b'\\'),
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn strip_prefix(p: &str) -> String {
    let p = unquote(p);
    match p.get(..2) {
        Some("a/" | "b/") => p[2..].to_owned(),
        _ => p,
    }
}

fn path_of(s: &str) -> Option<String> {
    let s = if s.starts_with('"') { s } else { s.split('\t').next().unwrap_or(s) };
    let s = s.trim_end();
    if s == "/dev/null" {
        return None;
    }
    Some(strip_prefix(s))
}

struct Header {
    os: u32,
    oc: u32,
    ns: u32,
    nc: u32,
    section: String,
}

fn hunk_header(l: &str) -> Option<Header> {
    let rest = l.strip_prefix("@@ -")?;
    let (ranges, section) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let range = |r: &str| -> Option<(u32, u32)> {
        match r.split_once(',') {
            Some((s, c)) => Some((s.parse().ok()?, c.parse().ok()?)),
            None => Some((r.parse().ok()?, 1)),
        }
    };
    let ((os, oc), (ns, nc)) = (range(old)?, range(new)?);
    Some(Header {
        os,
        oc,
        ns,
        nc,
        section: section.trim().to_owned(),
    })
}

/// Reads hunk `h`'s lines from `lines[i..]` until its counts are used up; returns the index of
/// the line after it.
fn hunk_body(b: &mut FileDiffBuilder, h: Header, lines: &[&str], mut i: usize) -> usize {
    let first = |start: u32, count: u32| if count == 0 { start.saturating_add(1) } else { start.max(1) };
    let (mut old, mut new) = (first(h.os, h.oc), first(h.ns, h.nc));
    let (mut old_left, mut new_left) = (h.oc, h.nc);
    b.push_hunk(h.os, h.oc, h.ns, h.nc, &h.section);
    let mut any = false;
    while i < lines.len() {
        let l = lines[i];
        if l.starts_with('\\') {
            if any {
                b.mark_no_newline();
            }
            i += 1;
            continue;
        }
        if old_left == 0 && new_left == 0 {
            break;
        }
        let (kind, text) = match l.as_bytes().first() {
            Some(b' ') => (LineKind::Context, &l[1..]),
            None if i + 1 < lines.len() => (LineKind::Context, ""),
            Some(b'-') if old_left > 0 => (LineKind::Removed, &l[1..]),
            Some(b'+') if new_left > 0 => (LineKind::Added, &l[1..]),
            _ => break,
        };
        match kind {
            LineKind::Context if old_left > 0 && new_left > 0 => {
                old_left -= 1;
                new_left -= 1;
                b.push_line(kind, old, new, text);
                old = old.saturating_add(1);
                new = new.saturating_add(1);
            }
            LineKind::Context => break,
            LineKind::Removed => {
                old_left -= 1;
                b.push_line(kind, old, 0, text);
                old = old.saturating_add(1);
            }
            LineKind::Added => {
                new_left -= 1;
                b.push_line(kind, 0, new, text);
                new = new.saturating_add(1);
            }
        }
        any = true;
        i += 1;
    }
    if let Some(last) = b.file_mut().hunks.last_mut() {
        last.old_count -= old_left;
        last.new_count -= new_left;
    }
    i
}
