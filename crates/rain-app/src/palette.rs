//! Ctrl+K: commands with their chords, layouts, file filters, notes, files, chapters and
//! threads, fuzzy-matched.

use rain_core::command::Command;
use rain_core::{FileTag, Note, NoteKind};
use rain_diffview::LineRef;
use rune::Tx;

use crate::app::App;
use crate::app::ask::{self, AskItem};
use crate::settings::ViewLayout;

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
    Ask(AskItem),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub item: Item,
    pub label: String,
    /// Chord, tag or location shown at the row's end.
    pub hint: String,
    pub kind: &'static str,
}

/// Rows the palette shows at most.
pub const MAX_ROWS: usize = 40;

/// A subsequence match of `query` in `text` (both lowercased by the caller), scored higher for
/// runs, word starts and an early first hit; `None` when not every query char appears in order.
pub fn score(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let t: Vec<char> = text.chars().collect();
    let mut score = 0;
    let mut ti = 0;
    let mut prev: Option<usize> = None;
    let mut first = None;
    for qc in query.chars() {
        if qc == ' ' {
            continue;
        }
        let at = (ti..t.len()).find(|&i| t[i] == qc)?;
        first.get_or_insert(at);
        score += 1;
        if prev == Some(at.wrapping_sub(1)) {
            score += 5;
        }
        if at == 0 || matches!(t[at - 1], ' ' | '/' | '.' | '_' | '-') {
            score += 3;
        }
        prev = Some(at);
        ti = at + 1;
    }
    Some(score * 4 - first.unwrap_or(0).min(40) as i32 - (t.len() as i32 / 16))
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
    out.extend([ViewLayout::Split, ViewLayout::Unified].iter().map(|l| Entry {
        item: Item::Layout(*l),
        label: format!("Layout: {}", l.label().to_lowercase()),
        hint: if *l == s.settings.layout { "on".into() } else { String::new() },
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

/// Entries matching `query`, best first (ties keep commands before files).
pub fn filtered(s: &App, query: &str) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    let ask_rows = ask::palette_rows(s, query).into_iter().map(|(item, label, hint)| Entry {
        item: Item::Ask(item),
        label,
        hint,
        kind: "Setting",
    });
    let mut scored: Vec<(i32, usize, Entry)> = entries(s)
        .into_iter()
        .chain(ask_rows)
        .enumerate()
        .filter_map(|(i, e)| score(&q, &e.label.to_lowercase()).map(|sc| (sc, i, e)))
        .collect();
    if !q.is_empty() {
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    scored.into_iter().take(MAX_ROWS).map(|(_, _, e)| e).collect()
}

/// Runs the selected row and closes the palette.
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
        Item::Ask(a) => s.apply_ask_item(a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
