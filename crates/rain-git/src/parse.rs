//! Streaming unified-diff parser: reads a patch line by line straight into each file's arena.
//! Semantics follow rune-kit's `diff::model::parse` (MIT): `diff --git` sections, plain
//! `---`/`+++`/`@@` diffs, renames, copies, modes, binaries, quoted paths with C escapes,
//! `\ No newline at end of file`, context lines whose leading space was trimmed, CRLF.

use std::borrow::Cow;
use std::io::BufRead;

use rain_core::{FileDiff, FileDiffBuilder, FileStatus, LineKind};

/// What the patch says about a file beyond [`FileDiff`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileMeta {
    /// `similarity index N%` of a rename or copy.
    pub similarity: Option<u8>,
    pub old_mode: Option<String>,
    pub new_mode: Option<String>,
}

#[derive(Debug, Default)]
pub struct Patch {
    pub files: Vec<FileDiff>,
    /// Parallel to `files`.
    pub meta: Vec<FileMeta>,
    /// Under `-w`: paths whose every change was whitespace (they stay in `files` with no hunks).
    pub formatting_only: Vec<String>,
}

pub fn parse_str(patch: &str, repo: u16) -> Patch {
    parse_reader(patch.as_bytes(), repo).unwrap_or_default()
}

/// Parses a patch as it streams; memory is the files' arenas plus one line buffer.
pub fn parse_reader<R: BufRead>(mut r: R, repo: u16) -> std::io::Result<Patch> {
    let mut p = Parser::new(repo);
    let mut buf = Vec::with_capacity(256);
    loop {
        buf.clear();
        if r.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        let mut end = buf.len();
        if buf[end - 1] == b'\n' {
            end -= 1;
        }
        if end > 0 && buf[end - 1] == b'\r' {
            end -= 1;
        }
        let line = match std::str::from_utf8(&buf[..end]) {
            Ok(s) => Cow::Borrowed(s),
            Err(_) => String::from_utf8_lossy(&buf[..end]),
        };
        p.line(&line);
    }
    Ok(p.finish())
}

struct Cur {
    b: FileDiffBuilder,
    old_path: Option<String>,
    new_path: Option<String>,
    status: Option<FileStatus>,
    meta: FileMeta,
    unavailable: bool,
    binary: bool,
}

struct HunkState {
    first_line: u32,
    old: u32,
    new: u32,
    old_left: u32,
    new_left: u32,
}

struct Parser {
    repo: u16,
    out: Patch,
    cur: Option<Cur>,
    /// In a `diff --git` section, before its first hunk.
    in_header: bool,
    hunk: Option<HunkState>,
    /// A `---` line, then a `+++` line, held until an `@@` line confirms a plain file header.
    pending: Vec<String>,
}

impl Parser {
    fn new(repo: u16) -> Parser {
        Parser {
            repo,
            out: Patch::default(),
            cur: None,
            in_header: false,
            hunk: None,
            pending: Vec::new(),
        }
    }

    fn line(&mut self, l: &str) {
        if self.hunk.is_some() && self.body_line(l) {
            return;
        }
        match self.pending.len() {
            1 if l.starts_with("+++ ") => {
                self.pending.push(l.to_owned());
                return;
            }
            2 if l.starts_with("@@ ") => {
                let plus = self.pending.pop().unwrap_or_default();
                let minus = self.pending.pop().unwrap_or_default();
                let (a, b) = (path_of(&minus[4..]), path_of(&plus[4..]));
                match self.cur.as_mut() {
                    Some(c) if self.in_header => {
                        c.old_path = a;
                        c.new_path = b;
                    }
                    _ => {
                        self.finish_file();
                        self.cur = Some(self.new_file(a, b));
                    }
                }
                self.in_header = false;
            }
            _ => self.pending.clear(),
        }
        if let Some(rest) = l.strip_prefix("diff --git ") {
            self.finish_file();
            let (a, b) = git_paths(rest);
            self.cur = Some(self.new_file(a, b));
            self.in_header = true;
            return;
        }
        if l.starts_with("--- ") {
            self.pending.push(l.to_owned());
            return;
        }
        let Some(c) = self.cur.as_mut() else { return };
        if l.starts_with("@@ ") {
            self.in_header = false;
            if let Some((os, oc, ns, nc, section)) = hunk_header(l) {
                c.b.push_hunk(os, oc, ns, nc, section);
                self.hunk = Some(HunkState {
                    first_line: c.b.file_mut().lines.len() as u32,
                    old: os,
                    new: ns,
                    old_left: oc,
                    new_left: nc,
                });
            }
        } else if self.in_header {
            header_line(c, l);
        }
    }

    /// Feeds `l` to the open hunk; false (and the hunk closed) when `l` is not part of it.
    fn body_line(&mut self, l: &str) -> bool {
        let (Some(h), Some(c)) = (self.hunk.as_mut(), self.cur.as_mut()) else {
            return false;
        };
        if l.starts_with('\\') {
            if c.b.file_mut().lines.len() as u32 > h.first_line {
                c.b.mark_no_newline();
            }
            return true;
        }
        let kind = match l.as_bytes().first() {
            Some(b' ') | None if h.old_left > 0 && h.new_left > 0 => LineKind::Context,
            Some(b'-') if h.old_left > 0 => LineKind::Removed,
            Some(b'+') if h.new_left > 0 => LineKind::Added,
            _ => {
                self.end_hunk();
                return false;
            }
        };
        let text = l.get(1..).unwrap_or("");
        let (o, n) = match kind {
            LineKind::Context => {
                h.old_left -= 1;
                h.new_left -= 1;
                let at = (h.old, h.new);
                h.old = h.old.saturating_add(1);
                h.new = h.new.saturating_add(1);
                at
            }
            LineKind::Removed => {
                h.old_left -= 1;
                let o = h.old;
                h.old = h.old.saturating_add(1);
                (o, 0)
            }
            LineKind::Added => {
                h.new_left -= 1;
                let n = h.new;
                h.new = h.new.saturating_add(1);
                (0, n)
            }
        };
        if text.starts_with("Subproject commit ") {
            c.unavailable = true;
        }
        c.b.push_line(kind, o, n, text);
        true
    }

    /// Closes the hunk, shrinking its counts to the lines actually read (a truncated patch).
    fn end_hunk(&mut self) {
        let (Some(h), Some(c)) = (self.hunk.take(), self.cur.as_mut()) else {
            return;
        };
        if let Some(last) = c.b.file_mut().hunks.last_mut() {
            last.old_count -= h.old_left;
            last.new_count -= h.new_left;
        }
    }

    fn new_file(&self, old_path: Option<String>, new_path: Option<String>) -> Cur {
        Cur {
            b: FileDiffBuilder::new(self.repo, String::new(), FileStatus::Modified),
            old_path,
            new_path,
            status: None,
            meta: FileMeta::default(),
            unavailable: false,
            binary: false,
        }
    }

    fn finish_file(&mut self) {
        self.end_hunk();
        let Some(c) = self.cur.take() else { return };
        let status = c.status.unwrap_or(match (&c.old_path, &c.new_path) {
            (None, Some(_)) => FileStatus::Added,
            (Some(_), None) => FileStatus::Deleted,
            (Some(a), Some(b)) if a != b => FileStatus::Renamed,
            _ => FileStatus::Modified,
        });
        let Some(path) = c.new_path.clone().or_else(|| c.old_path.clone()) else {
            return;
        };
        let mut f = c.b.finish();
        f.path = path;
        f.status = status;
        f.old_path = match status {
            FileStatus::Renamed | FileStatus::Copied if c.old_path.as_ref() != Some(&f.path) => c.old_path,
            _ => None,
        };
        f.binary = c.binary;
        f.contents_unavailable = c.unavailable || c.binary;
        f.lines.shrink_to_fit();
        f.text.shrink_to_fit();
        self.out.files.push(f);
        self.out.meta.push(c.meta);
    }

    fn finish(mut self) -> Patch {
        self.finish_file();
        self.out
    }
}

fn header_line(c: &mut Cur, l: &str) {
    let word = |p: &str| l.strip_prefix(p).map(str::trim);
    let gitlink = |m: &str| m == "160000";
    if let Some(m) = word("old mode ") {
        c.unavailable |= gitlink(m);
        c.meta.old_mode = Some(m.to_owned());
    } else if let Some(m) = word("new mode ") {
        c.unavailable |= gitlink(m);
        c.meta.new_mode = Some(m.to_owned());
    } else if let Some(m) = word("new file mode ") {
        c.unavailable |= gitlink(m);
        c.meta.new_mode = Some(m.to_owned());
        c.status = Some(FileStatus::Added);
        c.old_path = None;
    } else if let Some(m) = word("deleted file mode ") {
        c.unavailable |= gitlink(m);
        c.meta.old_mode = Some(m.to_owned());
        c.status = Some(FileStatus::Deleted);
        c.new_path = None;
    } else if let Some(p) = word("rename from ") {
        c.old_path = Some(unquote(p));
        c.status = Some(FileStatus::Renamed);
    } else if let Some(p) = word("rename to ") {
        c.new_path = Some(unquote(p));
        c.status = Some(FileStatus::Renamed);
    } else if let Some(p) = word("copy from ") {
        c.old_path = Some(unquote(p));
        c.status = Some(FileStatus::Copied);
    } else if let Some(p) = word("copy to ") {
        c.new_path = Some(unquote(p));
        c.status = Some(FileStatus::Copied);
    } else if let Some(s) = word("similarity index ") {
        c.meta.similarity = s.trim_end_matches('%').parse().ok();
    } else if let Some(rest) = word("index ") {
        if let Some((_, mode)) = rest.split_once(' ') {
            c.unavailable |= gitlink(mode);
            c.meta.old_mode.get_or_insert_with(|| mode.to_owned());
            c.meta.new_mode.get_or_insert_with(|| mode.to_owned());
        }
    } else if l.starts_with("Binary files ") || l == "GIT binary patch" {
        c.binary = true;
    }
}

/// The two paths of a `diff --git a/x b/y` line. With unquoted paths containing spaces the
/// split is ambiguous; the `---`/`+++` or rename lines that follow correct it.
pub(crate) fn git_paths(rest: &str) -> (Option<String>, Option<String>) {
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
    match rest.find(" b/").or_else(|| rest.find(" \"b/")) {
        Some(at) => (Some(strip_prefix(&rest[..at])), Some(strip_prefix(&rest[at + 1..]))),
        None => (None, None),
    }
}

fn split_quoted(rest: &str) -> Option<(String, String)> {
    let end = quoted_end(rest)?;
    let a = unquote(&rest[..end]);
    let tail = rest[end..].trim_start();
    Some((a, unquote(tail)))
}

/// The byte after the closing quote of the quoted string `s` starts with.
fn quoted_end(s: &str) -> Option<usize> {
    let mut escaped = false;
    for (i, c) in s.char_indices().skip(1) {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '"' => return Some(i + 1),
            _ => {}
        }
    }
    None
}

/// A path as git writes it: bare, or in double quotes with C escapes (octal bytes for
/// non-ASCII).
pub fn unquote(p: &str) -> String {
    let Some(inner) = p.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return p.to_owned();
    };
    let mut bytes = Vec::with_capacity(inner.len());
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
            Some(b'a') => bytes.push(0x07),
            Some(b'b') => bytes.push(0x08),
            Some(b'f') => bytes.push(0x0c),
            Some(b'v') => bytes.push(0x0b),
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

/// The path of a `---` or `+++` line: `None` for `/dev/null`, without a trailing timestamp.
fn path_of(s: &str) -> Option<String> {
    let s = if s.starts_with('"') { s } else { s.split('\t').next().unwrap_or(s) };
    let s = s.trim_end();
    if s == "/dev/null" {
        return None;
    }
    Some(strip_prefix(s))
}

fn hunk_header(l: &str) -> Option<(u32, u32, u32, u32, &str)> {
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
    Some((os, oc, ns, nc, section.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unquote_handles_octal_utf8_and_escapes() {
        assert_eq!(unquote("\"a/\\347\\244\\274\\345\\210\\270.json\""), "a/礼券.json");
        assert_eq!(unquote("\"tab\\there\\\"q\\\\\""), "tab\there\"q\\");
        assert_eq!(unquote("plain path"), "plain path");
    }

    #[test]
    fn git_paths_split_spaces_and_quotes() {
        assert_eq!(git_paths("a/x y.cs b/x y.cs"), (Some("x y.cs".into()), Some("x y.cs".into())));
        assert_eq!(git_paths("a/old.cs b/new.cs"), (Some("old.cs".into()), Some("new.cs".into())));
        assert_eq!(
            git_paths("\"a/sp ace\\303\\251\" \"b/sp ace\\303\\251\""),
            (Some("sp aceé".into()), Some("sp aceé".into()))
        );
        assert_eq!(git_paths("a/x \"b/\\303\\251\""), (Some("x".into()), Some("é".into())));
    }

    #[test]
    fn hunk_header_defaults_counts_to_one() {
        assert_eq!(hunk_header("@@ -3 +4 @@ fn x()"), Some((3, 1, 4, 1, "fn x()")));
        assert_eq!(hunk_header("@@ malformed @@"), None);
    }
}
