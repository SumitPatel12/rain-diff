//! `Ctrl+P`: the picker. Fuzzy over the one ordered list (files, groups) plus the Brief's
//! sections, the Map's nodes, the tour's steps and the threads waiting on you.

use rain_comments::ops;
use rain_diagram::DiagramEvent;
use rain_triage::Group;
use rune::{TextEdit, Tx};

use super::board::Pick;
use super::layout::Overlay;
use super::{App, ME};

/// Rows the picker lists at most.
pub const PICKER_ROWS: usize = 60;

#[derive(Default)]
pub struct PickerState {
    pub edit: TextEdit,
    pub sel: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    File(usize),
    Group(Group),
    Brief(String),
    Node(String),
    Step(String),
    Thread(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// `File`, `Group`, `Brief`, `Map`, `Tour`, `Waiting`.
    pub kind: &'static str,
    pub label: String,
    /// The folder, a count or the section's first words.
    pub detail: String,
    pub target: Target,
}

/// How well `query` matches `hay` as a subsequence, higher is better; `None` when it doesn't.
/// Word starts (after `/ . _ - ` or a lower→upper step) and runs of adjacent letters score.
pub fn fuzzy(query: &str, hay: &str) -> Option<i32> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(0);
    }
    let h: Vec<char> = hay.chars().collect();
    let mut score = 0;
    let mut qi = 0;
    let mut last: Option<usize> = None;
    for (i, &c) in h.iter().enumerate() {
        if qi == q.len() {
            break;
        }
        if c.to_lowercase().next() != Some(q[qi]) {
            continue;
        }
        let prev = i.checked_sub(1).map(|p| h[p]);
        let start = prev.is_none_or(|p| matches!(p, '/' | '.' | '_' | '-' | ' ') || (p.is_lowercase() && c.is_uppercase()));
        score += 1;
        if start {
            score += 3;
        }
        match last {
            Some(l) if l + 1 == i => score += 4,
            Some(l) => score -= ((i - l - 1) as i32).min(4),
            None => score -= (i as i32).min(4),
        }
        last = Some(i);
        qi += 1;
    }
    (qi == q.len()).then_some(score * 4 - h.len() as i32 / 8)
}

impl App {
    /// `Ctrl+P`: opens the picker with the keyboard in its box.
    pub fn open_picker(&mut self, _tx: &mut Tx) {
        self.show_picker("");
    }

    /// Opens the picker on `query`.
    pub fn show_picker(&mut self, query: &str) {
        self.picker.edit = TextEdit::default();
        self.picker.edit.text = query.to_owned();
        self.picker.edit.set_caret(query.len());
        self.picker.edit.focused = true;
        self.picker.sel = 0;
        self.open_overlay(Overlay::Picker);
    }

    pub fn close_picker(&mut self) {
        self.picker.edit.focused = false;
        self.close_overlay(Overlay::Picker);
    }

    /// Everything the picker can go to, in list order.
    fn picker_entries(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        for g in self.list_order() {
            let key = &self.keys[g];
            let (dir, name) = key.rsplit_once('/').unwrap_or(("", key.as_str()));
            out.push(Entry {
                kind: "File",
                label: name.to_owned(),
                detail: dir.to_owned(),
                target: Target::File(g),
            });
        }
        for g in Group::ALL.into_iter().filter(|g| !g.is_change()) {
            let n = (0..self.keys.len()).filter(|&f| self.group_of(f) == g).count();
            if n > 0 {
                out.push(Entry {
                    kind: "Group",
                    label: g.label().to_owned(),
                    detail: format!("{n} files"),
                    target: Target::Group(g),
                });
            }
        }
        if let Some(b) = &self.mirror.brief {
            for s in b.sections() {
                out.push(Entry {
                    kind: "Brief",
                    label: s.title.to_owned(),
                    detail: rain_comments::label::first_sentence(s.text, 60),
                    target: Target::Brief(s.id.to_owned()),
                });
            }
            for (i, st) in b.tour_steps().into_iter().enumerate() {
                out.push(Entry {
                    kind: "Tour",
                    label: format!("{}. {}", i + 1, st.title),
                    detail: st.cite.path.clone(),
                    target: Target::Step(match st.cite.lines {
                        Some((a, _)) => format!("{}:{a}", st.cite.path),
                        None => st.cite.path.clone(),
                    }),
                });
            }
        }
        if let Some(l) = &self.overview {
            for n in &l.document.graph.nodes {
                out.push(Entry {
                    kind: "Map",
                    label: n.label.clone(),
                    detail: n.kind.clone(),
                    target: Target::Node(n.id.clone()),
                });
            }
        }
        for t in self.mirror.threads.iter().filter(|t| ops::waiting_on_user(t, ME)) {
            let body = t.messages.last().map_or("", |m| m.body.as_str());
            out.push(Entry {
                kind: "Waiting",
                label: rain_comments::label::first_sentence(body, 70),
                detail: t.file_path.clone(),
                target: Target::Thread(t.id.clone()),
            });
        }
        out
    }

    /// The entries matching the picker's text, best first (list order on ties).
    pub fn picker_rows(&self) -> Vec<Entry> {
        let q = self.picker.edit.text.trim().to_owned();
        let all = self.picker_entries();
        if q.is_empty() {
            return all.into_iter().take(PICKER_ROWS).collect();
        }
        let mut scored: Vec<(i32, usize, Entry)> = all
            .into_iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let name = fuzzy(&q, &e.label).map(|s| s + 8);
                let full = fuzzy(&q, &format!("{}/{}", e.detail, e.label));
                name.max(full).map(|s| (s, i, e))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        scored.into_iter().take(PICKER_ROWS).map(|(_, _, e)| e).collect()
    }

    pub fn picker_move(&mut self, down: bool) {
        let n = self.picker_rows().len();
        self.picker.sel = if down {
            (self.picker.sel + 1).min(n.saturating_sub(1))
        } else {
            self.picker.sel.saturating_sub(1)
        };
    }

    /// `Enter` goes to the selected entry; zoom is restored first unless `keep_zoom` (Ctrl+Enter).
    pub fn picker_go(&mut self, keep_zoom: bool, tx: &mut Tx) {
        let rows = self.picker_rows();
        let Some(e) = rows.get(self.picker.sel.min(rows.len().saturating_sub(1))).cloned() else {
            return;
        };
        self.close_picker();
        if self.layout.is_zoomed() && !keep_zoom {
            self.toggle_zoom();
        }
        match e.target {
            Target::File(g) => self.goto_file(g, tx),
            Target::Group(g) => {
                if self.single_mode() {
                    self.open_pick(Pick::Group(g), tx);
                } else {
                    self.unfold_section(super::board::section_of(g));
                    if let Some(f) = self.list_order().into_iter().find(|&f| self.group_of(f) == g) {
                        self.goto_file(f, tx);
                    }
                }
            }
            Target::Brief(_) => {
                self.show_overview();
                self.status = format!("Brief › {}", e.label);
            }
            Target::Node(id) => self.diagram_event(DiagramEvent::Node(id), tx),
            Target::Step(spec) => {
                if !self.tour_from_picker(&e.label, tx) {
                    self.goto_spec(&spec, tx);
                    self.status = format!("Tour {}", e.label);
                }
            }
            Target::Thread(id) => self.goto_thread(&id, tx),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fuzzy;

    #[test]
    fn word_starts_and_runs_win() {
        let vsc = |h| fuzzy("vsc", h).unwrap_or(i32::MIN);
        assert!(vsc("VouchersController.cs") > vsc("VoucherService.cs"));
        assert!(vsc("VouchersController.cs") > vsc("voucher.service.ts"));
        assert!(fuzzy("zz", "VoucherService.cs").is_none());
        assert_eq!(fuzzy("", "x"), Some(0));
    }
}
