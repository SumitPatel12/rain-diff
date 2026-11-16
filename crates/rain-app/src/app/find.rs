//! `/`: find in the active diff.

use rain_core::diff::Side;
use rain_diffview::LineRef;
use rune::{TextEdit, Tx};

use super::App;

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

    /// Marks the query's matches in the diff, the current hit brighter.
    fn mark_find(&mut self) {
        let q = self.find.query.clone();
        let cur = self.find.hits.get(self.find.at).copied();
        self.active_mut().set_find(&q, cur);
    }

    /// Recomputes matches of the find box's text over the active diff's lines (case-insensitive
    /// for ASCII queries), capped at 10,000.
    pub fn run_find(&mut self) {
        let q = self.find.edit.text.clone();
        let scope: Vec<usize> = self.active().files().iter().map(|f| std::sync::Arc::as_ptr(f) as usize).collect();
        if q == self.find.query && scope == self.find.scope {
            self.mark_find();
            return;
        }
        self.find.query = q.clone();
        self.find.scope = scope;
        self.find.hits.clear();
        self.find.at = 0;
        if q.is_empty() {
            self.mark_find();
            return;
        }
        // ASCII queries match ASCII-case-insensitively, in place rather than over a lowered copy.
        let fold = q
            .is_ascii()
            .then(|| {
                regex::bytes::RegexBuilder::new(&regex::escape(&q))
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
                None => Box::new(f.text.match_indices(q.as_str()).map(|(at, _)| at)),
            };
            let mut last_line = u32::MAX;
            for at in matches {
                let i = f.lines.partition_point(|r| (r.end as usize) < at) as u32;
                if (i as usize) < f.lines.len() && i != last_line && f.lines[i as usize].start as usize <= at {
                    last_line = i;
                    hits.push((fi, i));
                    if hits.len() >= 10_000 {
                        break 'files;
                    }
                }
            }
        }
        self.find.hits = hits;
        self.mark_find();
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
        self.active_mut().scroll_to_line(f, at, tx);
        self.status = format!("Match {} of {}", self.find.at + 1, n);
    }
}
