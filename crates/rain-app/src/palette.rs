//! Ctrl+K: commands with their chords, layouts, themes, file filters, notes, files, chapters and
//! threads, fuzzy-matched.

use std::rc::Rc;

use rain_core::command::Command;
use rain_core::fuzzy::{Fit, Matcher, Query};
use rain_core::keymap::Profile;
use rain_core::{FileTag, Note, NoteKind};
use rain_diffview::{LineRef, VimKey};
use rune::Tx;

use crate::app::App;
use crate::app::ask::{self, AskItem};
use crate::settings::{Theme, ViewLayout};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Cmd(Command),
    File(usize),
    Chapter(usize),
    Thread(String),
    /// Toggles a repo (by index) in the file filter.
    FilterRepo(u16),
    FilterTag(FileTag),
    Note(String),
    Layout(ViewLayout),
    Theme(Theme),
    Profile(Profile),
    Ask(AskItem),
    /// A row of [`VIM`].
    Vim(usize),
}

/// Vim's keys in the reader, listed with the commands so the key sheet shows them; a row runs
/// its keys (`f` and `t` then wait for the character).
pub const VIM: [(&str, &str, &[VimKey]); 21] = [
    ("Vim: left", "h", &[VimKey::Char('h')]),
    ("Vim: right", "l", &[VimKey::Char('l')]),
    ("Vim: next word", "w", &[VimKey::Char('w')]),
    ("Vim: previous word", "b", &[VimKey::Char('b')]),
    ("Vim: end of word", "e", &[VimKey::Char('e')]),
    ("Vim: next WORD", "W", &[VimKey::Char('W')]),
    ("Vim: previous WORD", "B", &[VimKey::Char('B')]),
    ("Vim: line start", "0", &[VimKey::Char('0')]),
    ("Vim: first non-blank", "^", &[VimKey::Char('^')]),
    ("Vim: line end", "$", &[VimKey::Char('$')]),
    ("Vim: first line of the file", "gg", &[VimKey::Char('g'), VimKey::Char('g')]),
    ("Vim: last line of the file", "G", &[VimKey::Char('G')]),
    ("Vim: find a character in the line", "f", &[VimKey::Char('f')]),
    ("Vim: till a character in the line", "t", &[VimKey::Char('t')]),
    ("Vim: repeat the find / backwards", "; ,", &[VimKey::Char(';')]),
    ("Vim: half a page down", "Ctrl+D", &[VimKey::HalfDown]),
    ("Vim: half a page up", "Ctrl+U", &[VimKey::HalfUp]),
    ("Vim: visual", "v", &[VimKey::Char('v')]),
    ("Vim: visual lines", "V", &[VimKey::Char('V')]),
    ("Vim: yank the line", "yy", &[VimKey::Char('y'), VimKey::Char('y')]),
    ("Vim: count, as in 3w or 5j", "1-9", &[]),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub item: Item,
    pub label: String,
    /// Chord, tag or location shown at the row's end.
    pub hint: String,
    pub kind: &'static str,
}

pub const MAX_ROWS: usize = 40;

/// How well `query` fits `text` (both lowercased by the caller), higher is better: see
/// [`rain_core::fuzzy`]. `None` when a term isn't in it, or only as scattered letters.
#[cfg(test)]
pub fn score(query: &str, text: &str) -> Option<i32> {
    fit(&mut Matcher::default(), &Query::new(query), text).score()
}

/// A path's file name weighs more than its folders; a ` / ` in a command's label is no folder.
fn name_at(text: &str) -> usize {
    match text.rfind('/') {
        Some(i) if !text[i + 1..].starts_with(char::is_whitespace) => i + 1,
        _ => 0,
    }
}

fn fit(m: &mut Matcher, q: &Query, text: &str) -> Fit {
    m.fit(q, text, name_at(text))
}

/// Per row, the byte ranges of its label that `query` matched: as typed, or forgiving a typo
/// for a row that only fits so. Worked out once for a query and its rows, then kept.
pub fn marks(s: &App, query: &str, rows: &[Entry]) -> Marks {
    let mut cache = s.palette.cache.borrow_mut();
    if let Some(m) = &cache.marked
        && m.query == query
        && m.labels.iter().eq(rows.iter().map(|e| &e.label))
    {
        return m.marks.clone();
    }
    let q = Query::new(&query.trim().to_lowercase());
    let loose = q.loosened();
    let (mut m, mut m_loose) = (Matcher::default(), Matcher::default());
    let marks: Marks = Rc::new(
        rows.iter()
            .map(|e| {
                let mut out = Vec::new();
                let at = name_at(&e.label);
                if m.marks(&q, &e.label, at, &mut out).score().is_none()
                    && let Some(loose) = &loose
                {
                    m_loose.marks(loose, &e.label, at, &mut out);
                }
                out
            })
            .collect(),
    );
    cache.marks_made += 1;
    cache.marked = Some(Marked {
        query: query.to_owned(),
        labels: rows.iter().map(|e| e.label.clone()).collect(),
        marks: marks.clone(),
    });
    marks
}

pub fn entries(s: &App) -> Vec<Entry> {
    let mut out: Vec<Entry> = Command::ALL
        .iter()
        .filter(|c| **c != Command::Palette && **c != Command::Escape)
        .map(|c| Entry {
            item: Item::Cmd(*c),
            label: c.label().to_owned(),
            hint: c.chord().map(|k| k.display()).unwrap_or_default(),
            kind: "Command",
        })
        .collect();
    if s.keymap_profile().vim_motions() {
        out.extend(VIM.iter().enumerate().map(|(i, (label, keys, _))| Entry {
            item: Item::Vim(i),
            label: (*label).to_owned(),
            hint: (*keys).to_owned(),
            kind: "Motion",
        }));
    }
    out.extend([ViewLayout::Split, ViewLayout::Unified].iter().map(|l| Entry {
        item: Item::Layout(*l),
        label: format!("Layout: {}", l.label().to_lowercase()),
        hint: if *l == s.settings.layout { "on".into() } else { String::new() },
        kind: "Setting",
    }));
    out.extend(Theme::PICKS.iter().map(|&(t, label)| Entry {
        item: Item::Theme(t),
        label: format!("Theme: {}", label.to_lowercase()),
        hint: if t == s.settings.theme { "on".into() } else { String::new() },
        kind: "Setting",
    }));
    out.extend(Profile::ALL.iter().map(|&p| Entry {
        item: Item::Profile(p),
        label: format!("Keyboard shortcuts: {}", p.label()),
        hint: if p == s.keymap_profile() { "on".into() } else { String::new() },
        kind: "Setting",
    }));
    out.extend(filter_entries(s));
    out.extend(note_entries(s));
    for (i, c) in s.chapters.chapters.iter().enumerate() {
        out.push(Entry {
            item: Item::Chapter(i),
            label: format!("Chapter {}: {}", i + 1, c.title),
            hint: format!("{} file(s)", c.items.len()),
            kind: "Chapter",
        });
    }
    for t in &s.mirror.threads {
        let first = t.messages.first().map_or("", |m| m.body.as_str());
        out.push(Entry {
            item: Item::Thread(t.id.clone()),
            label: rain_comments::label::first_sentence(first, 90),
            hint: rain_comments::anchor_label(&t.file_path, &t.anchor),
            kind: "Thread",
        });
    }
    for (i, k) in s.keys.iter().enumerate() {
        out.push(Entry {
            item: Item::File(i),
            label: k.clone(),
            hint: s.tags.get(k).map_or("", |t| t.label()).to_owned(),
            kind: "File",
        });
    }
    out
}

fn count_hint(n: u32, on: bool) -> String {
    let files = if n == 1 { "1 file".to_owned() } else { format!("{n} files") };
    if on { format!("on · {files}") } else { files }
}

/// `Filter: <repo>` (multi-repo only) and `Filter: <tag>` for every tag with files or set,
/// counted as the tree's chips count them.
fn filter_entries(s: &App) -> Vec<Entry> {
    let f = &s.tree.filter;
    let mut out = Vec::new();
    if s.tree.repos().len() > 1 {
        for (i, name) in s.tree.repos() {
            out.push(Entry {
                item: Item::FilterRepo(*i),
                label: format!("Filter: {name}"),
                hint: count_hint(s.tree.repo_count(*i), f.repos.contains(i)),
                kind: "Filter",
            });
        }
    }
    for t in FileTag::ALL {
        let (n, on) = (s.tree.tag_count(t), f.tags.contains(&t));
        if n > 0 || on {
            out.push(Entry {
                item: Item::FilterTag(t),
                label: format!("Filter: {}", t.label()),
                hint: count_hint(n, on),
                kind: "Filter",
            });
        }
    }
    out
}

fn note_entries(s: &App) -> Vec<Entry> {
    s.all_notes()
        .into_iter()
        .map(|n| {
            let text = n.title.clone().filter(|t| !t.trim().is_empty()).unwrap_or_else(|| n.body.clone());
            Entry {
                hint: note_place(&n),
                label: format!("Note: {}", rain_comments::label::first_sentence(&text, 90)),
                item: Item::Note(n.id),
                kind: "Note",
            }
        })
        .collect()
}

fn note_place(n: &Note) -> String {
    match (n.kind, &n.anchor) {
        (NoteKind::Story, _) => "Story".into(),
        (NoteKind::Line, Some(a)) => rain_comments::anchor_label(&n.key(), a),
        _ => n.key(),
    }
}

/// Story notes open the Overview; file and line notes jump to their place.
fn open_note(s: &mut App, id: &str, tx: &mut Tx) {
    let Some(n) = s.note(id) else { return };
    let line = n.anchor.map(|a| LineRef::new(a.side(), a.lines().0));
    match (n.kind, s.file_of_note(&n), line) {
        (NoteKind::Story, ..) => {
            s.show_overview();
            s.status = "Story note: on the Overview tab".into();
        }
        (NoteKind::Line, Some(g), Some(at)) => s.goto_line(g, at, tx),
        (_, Some(g), _) => s.goto_file(g, tx),
        (_, None, _) => s.status = format!("{} is not in this diff", n.key()),
    }
}

/// Toggles one axis of the tree's filter; the diff follows through [`App::on_filter_change`].
fn toggle_filter(s: &mut App, item: &Item, tx: &mut Tx) {
    let mut f = s.tree.filter.clone();
    match item {
        Item::FilterRepo(i) if !f.repos.remove(i) => {
            f.repos.insert(*i);
        }
        Item::FilterTag(t) if !f.tags.remove(t) => {
            f.tags.insert(*t);
        }
        _ => {}
    }
    s.tree.apply(&f);
    s.on_filter_change(&f, tx);
    if f.is_empty() {
        s.status = "Filter cleared".into();
    }
}

/// What [`entries`] reads, so they are rebuilt only when it changes.
type Source = (u64, u32, ViewLayout, Theme, u64, rain_tree::TreeFilter);

/// Per row, the byte ranges of its label a query matched.
pub type Marks = Rc<Vec<Vec<std::ops::Range<usize>>>>;

/// The last query, compiled; `loose` forgives a typo, when a term is long enough for one.
struct Asked {
    text: String,
    strict: Query,
    loose: Option<Query>,
}

struct Marked {
    query: String,
    labels: Vec<String>,
    marks: Marks,
}

/// [`entries`] with their labels lowercased, kept while what they show stands; and the last
/// query's matches, which a query typed on from it only narrows, so a redraw or a move of the
/// selection matches nothing again.
#[derive(Default)]
pub struct Cache {
    source: Option<Source>,
    entries: Vec<Entry>,
    lower: Vec<String>,
    asked: Option<Asked>,
    /// The last query's matches, and the entries holding all its letters.
    hits: Vec<(i32, usize)>,
    held: Vec<usize>,
    /// Its matches forgiving a typo, once nothing fitted as typed.
    loose_hits: Option<Vec<(i32, usize)>>,
    matcher: Matcher,
    loose_matcher: Matcher,
    marked: Option<Marked>,
    /// Passes over the entries so far, and times marks were worked out.
    pub scans: u32,
    pub marks_made: u32,
}

impl Cache {
    fn refresh(&mut self, s: &App) {
        let source = (
            s.seen_version,
            s.reloader.installs(),
            s.settings.layout,
            s.settings.theme,
            s.keymap_gen,
            s.tree.filter.clone(),
        );
        if self.source.as_ref() == Some(&source) {
            return;
        }
        self.entries = entries(s);
        self.lower = self.entries.iter().map(|e| e.label.to_lowercase()).collect();
        self.source = Some(source);
        self.asked = None;
        self.marked = None;
    }

    /// `(score, index)` of every entry matching `q`, in entry order.
    fn matches(&mut self, q: &str) -> Vec<(i32, usize)> {
        let from = match &self.asked {
            Some(last) if last.text == q => return self.hits.clone(),
            // Every term of the shorter query is in the longer one, whole or as a term's start.
            Some(last) if q.starts_with(last.text.as_str()) => std::mem::take(&mut self.held),
            _ => (0..self.lower.len()).collect(),
        };
        let strict = Query::new(q);
        if !strict.is_empty() {
            self.scans += 1;
        }
        let (mut hits, mut held) = (Vec::new(), Vec::new());
        for i in from {
            match fit(&mut self.matcher, &strict, &self.lower[i]) {
                Fit::Miss => {}
                Fit::Weak => held.push(i),
                Fit::Hit(sc) => {
                    held.push(i);
                    hits.push((sc, i));
                }
            }
        }
        self.asked = Some(Asked {
            text: q.to_owned(),
            loose: strict.loosened(),
            strict,
        });
        self.hits = hits;
        self.held = held;
        self.loose_hits = None;
        self.hits.clone()
    }

    /// Every entry matching the last query when a typo is forgiven: one pass, at the first ask.
    fn loose_matches(&mut self) -> Vec<(i32, usize)> {
        let Some(loose) = self.asked.as_ref().and_then(|a| a.loose.as_ref()) else {
            return Vec::new();
        };
        if self.loose_hits.is_none() {
            self.scans += 1;
            let m = &mut self.loose_matcher;
            let all = self.lower.iter().enumerate();
            self.loose_hits = Some(all.filter_map(|(i, l)| fit(m, loose, l).score().map(|sc| (sc, i))).collect());
        }
        self.loose_hits.clone().unwrap_or_default()
    }

    /// The last query's score for each of `labels` (lowercased), numbered from `first`.
    fn score_rows(&mut self, labels: &[String], first: usize, forgiving: bool) -> Vec<(i32, usize)> {
        let Some(asked) = &self.asked else { return Vec::new() };
        let (m, q) = match (&asked.loose, forgiving) {
            (Some(loose), true) => (&mut self.loose_matcher, loose),
            (None, true) => return Vec::new(),
            (_, false) => (&mut self.matcher, &asked.strict),
        };
        let rows = labels.iter().enumerate();
        rows.filter_map(|(j, l)| fit(m, q, l).score().map(|sc| (sc, first + j))).collect()
    }
}

/// Entries matching `query`, best first (ties keep commands before files).
pub fn filtered(s: &App, query: &str) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    let mut cache = s.palette.cache.borrow_mut();
    cache.refresh(s);
    let n = cache.entries.len();
    let ask_rows: Vec<Entry> = ask::palette_rows(s, query)
        .into_iter()
        .map(|(item, label, hint)| Entry {
            item: Item::Ask(item),
            label,
            hint,
            kind: "Setting",
        })
        .collect();
    let ask_labels: Vec<String> = ask_rows.iter().map(|e| e.label.to_lowercase()).collect();
    let mut scored = cache.matches(&q);
    scored.extend(cache.score_rows(&ask_labels, n, false));
    // Nothing fits as typed: everything once more, forgiving a typo.
    if scored.is_empty() {
        scored = cache.loose_matches();
        scored.extend(cache.score_rows(&ask_labels, n, true));
    }
    if !q.is_empty() {
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    scored
        .into_iter()
        .take(MAX_ROWS)
        .map(|(_, i)| if i < n { cache.entries[i].clone() } else { ask_rows[i - n].clone() })
        .collect()
}

pub fn run_selected(s: &mut App, tx: &mut Tx) {
    let rows = filtered(s, &s.palette.edit.text.clone());
    let Some(e) = rows.get(s.palette.sel.min(rows.len().saturating_sub(1))).cloned() else {
        s.close_palette();
        return;
    };
    s.close_palette();
    match e.item {
        Item::Cmd(c) => {
            s.run(c, tx);
        }
        Item::File(i) => s.goto_file(i, tx),
        Item::Chapter(i) => s.goto_chapter(i, tx),
        Item::Thread(id) => s.goto_thread(&id, tx),
        Item::FilterRepo(_) | Item::FilterTag(_) => toggle_filter(s, &e.item, tx),
        Item::Note(id) => open_note(s, &id, tx),
        Item::Layout(l) => s.set_layout(l),
        Item::Theme(t) => s.set_theme(t),
        Item::Profile(p) => s.set_profile(p),
        Item::Ask(a) => s.apply_ask_item(a),
        Item::Vim(i) => {
            for &k in VIM[i].2 {
                s.vim_run(k, tx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_on_finds_what_a_fresh_search_finds_and_changes_show() {
        let mut a = crate::tests::app(crate::tests::story(), true);
        let fresh = |a: &App, q: &str| -> Vec<(i32, usize)> {
            entries(a)
                .iter()
                .enumerate()
                .filter_map(|(i, e)| score(q, &e.label.to_lowercase()).map(|sc| (sc, i)))
                .collect()
        };
        for q in ["v", "vo", "vou", "vouc", "vo", "x", "", "s t"] {
            let cached = {
                let mut c = a.palette.cache.borrow_mut();
                c.refresh(&a);
                c.matches(q)
            };
            assert_eq!(cached, fresh(&a, q), "{q:?}");
        }
        let hint = |a: &App| filtered(a, "layout: unified").first().map(|e| e.hint.clone());
        assert_eq!(hint(&a).as_deref(), Some(""));
        a.settings.layout = ViewLayout::Unified;
        assert_eq!(hint(&a).as_deref(), Some("on"), "a setting change rebuilds the rows");
    }

    #[test]
    fn words_match_in_any_order_and_a_file_name_outweighs_its_folders() {
        let a = crate::tests::app(crate::tests::story(), true);
        let labels = |q: &str| -> Vec<String> { filtered(&a, q).into_iter().map(|e| e.label).collect() };
        assert_eq!(labels("dark theme")[0], "Theme: dark");
        let rows = labels("theme dark");
        assert_eq!(rows[0], "Theme: dark", "the shorter label first: {rows:?}");
        assert!(rows.contains(&"Theme: gruvbox dark hard".to_owned()), "{rows:?}");
        assert_eq!(labels("unified split")[0], "Split / unified", "a slash in a label is no folder");
        let files: Vec<String> = filtered(&a, "serv")
            .into_iter()
            .filter(|e| e.kind == "File")
            .map(|e| e.label)
            .collect();
        let at = |end: &str| {
            files
                .iter()
                .position(|f| f.ends_with(end))
                .unwrap_or_else(|| panic!("{end}: {files:?}"))
        };
        for name in ["AccountService.cs", "VoucherService.cs", "voucher.service.ts"] {
            assert!(at(name) < at("VouchersController.cs"), "{files:?}");
        }
        assert!(labels("zzzz").is_empty());
    }

    #[test]
    fn subsequence_scores_prefer_runs_and_word_starts() {
        assert!(score("split", "toggle split view").is_some());
        assert!(score("tsv", "toggle split view").is_some());
        assert!(score("xyz", "toggle split view").is_none());
        let run = score("split", "toggle split view").unwrap();
        let scattered = score("split", "show previous lines in tree").unwrap_or(i32::MIN);
        assert!(run > scattered);
        assert!(
            score("vouchers", "server/src/api/voucherscontroller.cs").unwrap()
                > score("vouchers", "server/src/v_o_u_c_h_e_r_s.cs").unwrap()
        );
    }
}
