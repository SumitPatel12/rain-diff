//! `/`: find in the active diff.

use rain_core::diff::Side;
use rain_core::fuzzy::{Matcher, Query};
use rain_diffview::LineRef;
use rune::{TextEdit, Tx};

use super::App;

/// Matches listed at most.
const MAX_HITS: usize = 10_000;
/// The most diff text searched on every keystroke; a larger diff is searched on Enter. Fuzzy
/// matching looks at each line longer, so its limit is lower.
const LIVE_BYTES: usize = 8 << 20;
const LIVE_BYTES_FUZZY: usize = 1 << 20;

#[derive(Default)]
pub struct Find {
    pub open: bool,
    pub edit: TextEdit,
    pub query: String,
    /// `(file in the active diff, line index)` per match.
    pub hits: Vec<(usize, u32)>,
    pub at: usize,
    /// The files `hits` index: the active diff's file pointers when they were found.
    scope: Vec<usize>,
    fuzzy: bool,
}

impl App {
    pub fn open_find(&mut self) {
        self.find.open = true;
        self.find.edit.focused = true;
    }

    pub fn close_find(&mut self) {
        self.find.open = false;
        self.find.edit.focused = false;
        self.active_mut().set_find("", None);
    }

    /// The find bar's Fuzzy switch: off, a line matches when it holds the text as typed; on,
    /// when each word of the query fuzzy-matches with its letters close together. Remembered.
    pub fn toggle_find_fuzzy(&mut self) {
        self.settings.find_fuzzy = !self.settings.find_fuzzy;
        self.status = if self.settings.find_fuzzy {
            "Find: fuzzy matching".into()
        } else {
            "Find: exact matching".into()
        };
        if self.find.open {
            self.run_find();
        }
        // Last, so a save that failed is what the status line says.
        self.save_settings();
    }

    /// Whether the diff is small enough to search again on every keystroke.
    pub fn find_is_live(&self) -> bool {
        let limit = if self.settings.find_fuzzy { LIVE_BYTES_FUZZY } else { LIVE_BYTES };
        self.active().files().iter().map(|f| f.text.len()).sum::<usize>() < limit
    }

    /// Marks the query's matches in the diff, the current hit brighter.
    fn mark_find(&mut self) {
        let q = self.find.query.clone();
        let cur = self.find.hits.get(self.find.at).copied();
        if self.find.fuzzy {
            self.active_mut().set_fuzzy_find(&q, cur);
        } else {
            self.active_mut().set_find(&q, cur);
        }
    }

    /// Recomputes matches of the find box's text over the active diff's lines (case-insensitive
    /// for ASCII queries, or fuzzy), capped at 10,000.
    pub fn run_find(&mut self) {
        let q = self.find.edit.text.clone();
        let fuzzy = self.settings.find_fuzzy;
        let scope: Vec<usize> = self.active().files().iter().map(|f| std::sync::Arc::as_ptr(f) as usize).collect();
        if q == self.find.query && scope == self.find.scope && fuzzy == self.find.fuzzy {
            self.mark_find();
            return;
        }
        self.find.query = q.clone();
        self.find.scope = scope;
        self.find.fuzzy = fuzzy;
        self.find.hits.clear();
        self.find.at = 0;
        if q.is_empty() {
            self.mark_find();
            return;
        }
        self.find.hits = if fuzzy { self.fuzzy_hits(&q) } else { self.exact_hits(&q) };
        self.mark_find();
    }

    fn exact_hits(&self, q: &str) -> Vec<(usize, u32)> {
        // ASCII queries match ASCII-case-insensitively, in place rather than over a lowered copy.
        let fold = q
            .is_ascii()
            .then(|| {
                regex::bytes::RegexBuilder::new(&regex::escape(q))
                    .case_insensitive(true)
                    .unicode(false)
                    .build()
                    .ok()
            })
            .flatten();
        let mut hits = Vec::new();
        'files: for (fi, f) in self.active().files().iter().enumerate() {
            let matches: Box<dyn Iterator<Item = usize>> = match &fold {
                Some(re) => Box::new(re.find_iter(f.text.as_bytes()).map(|m| m.start())),
                None => Box::new(f.text.match_indices(q).map(|(at, _)| at)),
            };
            let mut last_line = u32::MAX;
            for at in matches {
                let i = f.lines.partition_point(|r| (r.end as usize) < at) as u32;
                if (i as usize) < f.lines.len() && i != last_line && f.lines[i as usize].start as usize <= at {
                    last_line = i;
                    hits.push((fi, i));
                    if hits.len() >= MAX_HITS {
                        break 'files;
                    }
                }
            }
        }
        hits
    }

    /// The lines every term of `q` fuzzy-matches in, each with its letters close together, in
    /// document order: the lines [`rain_diffview::DiffState::set_fuzzy_find`] marks.
    fn fuzzy_hits(&self, q: &str) -> Vec<(usize, u32)> {
        let query = Query::new(q);
        let mut hits = Vec::new();
        if query.is_empty() {
            return hits;
        }
        let mut matcher = Matcher::in_words();
        'files: for (fi, f) in self.active().files().iter().enumerate() {
            for i in 0..f.lines.len() as u32 {
                if matcher.fit(&query, f.line_text(i), 0).score().is_some() {
                    hits.push((fi, i));
                    if hits.len() >= MAX_HITS {
                        break 'files;
                    }
                }
            }
        }
        hits
    }

    pub fn find_step(&mut self, forward: bool, tx: &mut Tx) {
        self.run_find();
        let n = self.find.hits.len();
        if n == 0 {
            self.status = format!("No match for \"{}\"", self.find.query);
            return;
        }
        self.find.at = if forward {
            (self.find.at + 1) % n
        } else {
            (self.find.at + n - 1) % n
        };
        self.mark_find();
        let (f, i) = self.find.hits[self.find.at];
        let Some(rec) = self.active().files().get(f).and_then(|x| x.lines.get(i as usize)).copied() else {
            return;
        };
        let at = if rec.new != 0 {
            LineRef::new(Side::New, rec.new)
        } else {
            LineRef::new(Side::Old, rec.old)
        };
        let hidden = self.hit_is_past_the_cut(f, i);
        self.active_mut().scroll_to_line(f, at, tx);
        self.status = if hidden {
            format!("Match {} of {} · in the part of a long line that isn't drawn", self.find.at + 1, n)
        } else {
            format!("Match {} of {}", self.find.at + 1, n)
        };
    }

    /// Whether the match on line `i` of file `f` lies past where a very long line stops being
    /// drawn, so nothing on screen marks it.
    fn hit_is_past_the_cut(&self, f: usize, i: u32) -> bool {
        let Some(file) = self.active().files().get(f) else { return false };
        let line = file.line_text(i);
        let drawn = rain_diffview::drawn_part(line);
        if drawn.len() == line.len() {
            return false;
        }
        let q = &self.find.query;
        if self.find.fuzzy {
            Matcher::in_words().fit(&Query::new(q), drawn, 0).score().is_none()
        } else if q.is_ascii() {
            !drawn.to_ascii_lowercase().contains(&q.to_ascii_lowercase())
        } else {
            !drawn.contains(q.as_str())
        }
    }
}
