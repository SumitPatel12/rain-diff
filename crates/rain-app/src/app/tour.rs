//! The Tour overlay: a pull-started walk over the brief's steps. Each step spotlights its range
//! in the reader (the rest dims, one outline slides between ranges), scrolls it to the upper
//! third and, under "My pass first", keeps the findings on it hidden until the user has read it.
//! Progress is kept per session beside the session file; after a reload a step whose code moved
//! follows its text and says "Diff changed".

use std::collections::BTreeSet;
use std::path::PathBuf;

use rain_core::brief::Cite;
use rain_core::{FileDiff, Side, Thread, ThreadState};
use rain_diffview::{Align, LineRef, Spot};
use rune::{Key, KeyEvent, Tx};
use serde::{Deserialize, Serialize};

use super::App;
use super::layout::Overlay;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TourStage {
    #[default]
    Start,
    Step,
    End,
}

/// Where a step's range sits in the loaded diff, and the text it held when the tour first saw it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchored {
    pub key: String,
    pub side: Side,
    pub lines: (u32, u32),
    /// The cite's own lines when this was anchored; a new brief that moves them re-anchors.
    pub cite_lines: Option<(u32, u32)>,
    pub text: Vec<String>,
    /// The code under the step changed since the tour first saw it.
    #[serde(default)]
    pub changed: bool,
}

/// A step as the card shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct TourView {
    pub id: String,
    pub title: String,
    pub text: String,
    pub cite: Cite,
    /// `None` when the cite names no file of this diff.
    pub at: Option<Anchored>,
    /// Set for a step the agent drives through `POST /api/highlight`.
    pub live: bool,
}

/// A thread on the step's range, and who found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub id: String,
    pub who: FoundBy,
    pub body: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoundBy {
    You,
    Agent,
    Both,
}

impl FoundBy {
    pub fn label(self) -> &'static str {
        match self {
            FoundBy::You => "you found",
            FoundBy::Agent => "agent found",
            FoundBy::Both => "both found",
        }
    }
}

/// What a session keeps of its tour.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    stage: TourStage,
    step: usize,
    my_pass_first: bool,
    understood: Vec<usize>,
    revealed: Vec<usize>,
    anchors: Vec<Option<Anchored>>,
}

/// Whether putting a step up scrolls the reader to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scroll {
    Never,
    IfHidden,
    Always,
}

impl Scroll {
    /// The stronger of two asks.
    fn max(a: Option<Scroll>, b: Scroll) -> Scroll {
        match (a, b) {
            (Some(Scroll::Always), _) | (_, Scroll::Always) => Scroll::Always,
            (Some(Scroll::IfHidden), _) | (_, Scroll::IfHidden) => Scroll::IfHidden,
            _ => Scroll::Never,
        }
    }
}

pub struct TourState {
    pub stage: TourStage,
    /// The step on screen, 0-based.
    pub step: usize,
    pub my_pass_first: bool,
    pub understood: BTreeSet<usize>,
    /// Steps whose findings were revealed under "My pass first".
    pub revealed: BTreeSet<usize>,
    /// One per step, in step order.
    pub anchors: Vec<Option<Anchored>>,
    /// Left with `Esc` mid-tour: `Enter` or `T` resumes.
    pub resumable: bool,
    /// The step an agent's highlight put up when the brief has no such step.
    pub live: Option<TourView>,
    /// Thread ids the reader leaves out while their step's findings are hidden.
    pub hidden: BTreeSet<String>,
    /// The spotlight should be put up (and the range scrolled to) at the next poll.
    pending: Option<Scroll>,
    /// The spot this tour last put up, to put it back after a rebuild and take it down on leave.
    spot: Option<(usize, Spot)>,
    /// The key list the anchors were checked against; a new allocation means a load installed.
    load_mark: usize,
    loaded: Option<PathBuf>,
}

impl Default for TourState {
    fn default() -> TourState {
        TourState {
            stage: TourStage::Start,
            step: 0,
            my_pass_first: true,
            understood: BTreeSet::new(),
            revealed: BTreeSet::new(),
            anchors: Vec::new(),
            resumable: false,
            live: None,
            hidden: BTreeSet::new(),
            pending: None,
            spot: None,
            load_mark: 0,
            loaded: None,
        }
    }
}

/// `(line, text)` of `side` in `f`, in line order.
fn side_lines(f: &FileDiff, side: Side) -> Vec<(u32, &str)> {
    let mut v: Vec<(u32, &str)> = f
        .lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| {
            let n = if side == Side::Old { l.old } else { l.new };
            (n != 0).then(|| (n, f.line_text(i as u32).trim_end()))
        })
        .collect();
    v.sort_by_key(|(n, _)| *n);
    v.dedup_by_key(|(n, _)| *n);
    v
}

fn text_at(lines: &[(u32, &str)], (a, b): (u32, u32)) -> Vec<String> {
    lines
        .iter()
        .filter(|(n, _)| (a..=b).contains(n))
        .map(|(_, t)| (*t).to_owned())
        .collect()
}

/// Where `text` sits now: its exact run of lines, the one nearest `near` when it repeats.
fn find_run(lines: &[(u32, &str)], text: &[String], near: u32) -> Option<(u32, u32)> {
    if text.is_empty() || text.iter().all(|t| t.trim().is_empty()) {
        return None;
    }
    lines
        .windows(text.len())
        .filter(|w| w.windows(2).all(|p| p[1].0 == p[0].0 + 1) && w.iter().zip(text).all(|((_, a), b)| a == b))
        .map(|w| (w[0].0, w[w.len() - 1].0))
        .min_by_key(|(a, _)| a.abs_diff(near))
}

impl App {
    // ----- steps -----

    /// The steps with where each sits in the loaded diff.
    pub fn tour_views(&self) -> Vec<TourView> {
        self.tour_steps()
            .into_iter()
            .enumerate()
            .map(|(i, s)| TourView {
                id: s.step.id,
                title: s.step.title,
                text: s.step.text,
                cite: s.step.cite,
                at: self.tour.anchors.get(i).cloned().flatten(),
                live: false,
            })
            .collect()
    }

    /// The step the card shows: the agent's live step, else the current one.
    pub fn tour_current(&self) -> Option<TourView> {
        if let Some(l) = &self.tour.live {
            return Some(l.clone());
        }
        self.tour_views().into_iter().nth(self.tour.step)
    }

    pub fn tour_open(&self) -> bool {
        self.layout.is_open(Overlay::Tour)
    }

    /// Anchors step `cite` in the loaded diff: its lines, else every line of its file's side.
    fn anchor_cite(&self, key: Option<&str>, cite: &Cite) -> Option<Anchored> {
        let key = key?;
        let f = self.index_of(key).and_then(|g| self.diff.files().get(g))?;
        let mut side = cite.side.unwrap_or(Side::New);
        let mut lines = side_lines(f, side);
        if lines.is_empty() && cite.side.is_none() {
            side = Side::Old;
            lines = side_lines(f, side);
        }
        let range = match cite.lines {
            Some((a, b)) => (a.min(b), a.max(b)),
            None => (lines.first()?.0, lines.last()?.0),
        };
        Some(Anchored {
            key: key.to_owned(),
            side,
            lines: range,
            cite_lines: cite.lines,
            text: text_at(&lines, range),
            changed: false,
        })
    }

    /// Brings the anchors in line with the steps and the loaded diff: new steps anchor fresh; a
    /// step whose text moved follows it, one whose cite changed re-anchors from the cite, and both
    /// read "Diff changed".
    fn reanchor_tour(&mut self) {
        let steps = self.tour_steps();
        let mut out: Vec<Option<Anchored>> = Vec::with_capacity(steps.len());
        for (i, st) in steps.iter().enumerate() {
            let fresh = self.anchor_cite(st.key.as_deref(), &st.step.cite);
            let Some(old) = self.tour.anchors.get(i).cloned().flatten() else {
                out.push(fresh);
                continue;
            };
            let Some(mut now) = fresh else {
                out.push(None);
                continue;
            };
            if old.key != now.key || old.cite_lines != now.cite_lines {
                now.changed = true;
                out.push(Some(now));
                continue;
            }
            let Some(f) = self.index_of(&old.key).and_then(|g| self.diff.files().get(g)) else {
                out.push(Some(now));
                continue;
            };
            let lines = side_lines(f, old.side);
            if text_at(&lines, old.lines) == old.text {
                out.push(Some(old));
                continue;
            }
            match find_run(&lines, &old.text, old.lines.0) {
                Some(range) => out.push(Some(Anchored {
                    lines: range,
                    changed: true,
                    ..old
                })),
                None => {
                    now.changed = true;
                    out.push(Some(now));
                }
            }
        }
        self.tour.anchors = out;
        if self.tour.step >= self.tour.anchors.len() && self.tour.stage == TourStage::Step {
            self.tour.stage = TourStage::End;
        }
    }

    // ----- findings -----

    /// Line threads on step `i`'s range, open or addressed, oldest first.
    pub fn tour_findings(&self, i: usize) -> Vec<Finding> {
        let at = if self.tour.live.is_some() {
            self.tour.live.as_ref().and_then(|l| l.at.clone())
        } else {
            self.tour.anchors.get(i).cloned().flatten()
        };
        let Some(at) = at else { return Vec::new() };
        let Some(g) = self.index_of(&at.key) else {
            return Vec::new();
        };
        let on: Vec<&Thread> = self
            .mirror
            .threads
            .iter()
            .filter(|t| matches!(t.state, ThreadState::Open | ThreadState::Addressed))
            .filter(|t| !t.file_path.starts_with('@') && t.anchor.side() == at.side)
            .filter(|t| {
                let (a, b) = t.anchor.lines();
                a <= at.lines.1 && b >= at.lines.0
            })
            .filter(|t| self.file_of_thread(t) == Some(g))
            .collect();
        let by_agent = |t: &Thread| {
            t.messages
                .first()
                .is_some_and(|m| m.author.as_deref().is_some_and(|a| !a.is_empty()))
        };
        let overlaps = |x: &Thread, y: &Thread| {
            let ((a, b), (c, d)) = (x.anchor.lines(), y.anchor.lines());
            a <= d && c <= b
        };
        on.iter()
            .map(|t| {
                let agent = by_agent(t);
                let other = on.iter().any(|o| by_agent(o) != agent && overlaps(o, t));
                Finding {
                    id: t.id.clone(),
                    who: match (agent, other) {
                        (_, true) => FoundBy::Both,
                        (true, false) => FoundBy::Agent,
                        (false, false) => FoundBy::You,
                    },
                    body: rain_comments::label::first_sentence(t.messages.first().map_or("", |m| m.body.as_str()), 90),
                }
            })
            .collect()
    }

    /// Whether step `i`'s findings are kept out of sight.
    pub fn tour_findings_hidden(&self, i: usize) -> bool {
        self.tour.my_pass_first && !self.tour.revealed.contains(&i)
    }

    /// Whether the reader leaves thread `id` out: its tour step's findings are hidden.
    pub(crate) fn tour_hides(&self, id: &str) -> bool {
        self.tour.hidden.contains(id)
    }

    fn refresh_hidden(&mut self) {
        let hidden: BTreeSet<String> =
            if self.tour_open() && self.tour.stage == TourStage::Step && self.tour_findings_hidden(self.tour.step) {
                self.tour_findings(self.tour.step).into_iter().map(|f| f.id).collect()
            } else {
                BTreeSet::new()
            };
        if hidden != self.tour.hidden {
            self.tour.hidden = hidden;
            self.rebuild_annotations();
        }
    }

    // ----- opening, stepping, leaving -----

    /// `T` or the Brief's button: the start card, or straight back to the step left with `Esc`.
    pub fn start_tour(&mut self, tx: &mut Tx) {
        self.load_tour();
        if self.tour_steps().is_empty() {
            self.status = "No tour: the brief has no steps or cites".into();
            return;
        }
        if self.tour_open() {
            self.status = self.tour_status();
            return;
        }
        if self.tour.resumable && self.tour.stage == TourStage::Step {
            self.resume_tour(tx);
            return;
        }
        self.set_reader_tab(super::overview::ReaderTab::Changes);
        self.reanchor_tour();
        self.tour.live = None;
        self.tour.stage = TourStage::Start;
        self.open_overlay(Overlay::Tour);
        self.status = "Tour · Enter starts · Esc skips to the reader".into();
        self.save_tour();
    }

    /// The start card's "Start": step 1, or the step the tour was left at.
    pub fn begin_tour(&mut self, tx: &mut Tx) {
        let n = self.tour_views().len();
        if n == 0 {
            return;
        }
        if self.tour.step >= n || self.tour.understood.len() >= n {
            self.tour.step = 0;
        }
        self.tour.stage = TourStage::Step;
        self.show_step(Scroll::Always, tx);
    }

    /// `Enter` after `Esc`: the same step, the reader where it was unless the range went off
    /// screen.
    pub fn resume_tour(&mut self, tx: &mut Tx) {
        self.tour.resumable = false;
        self.open_overlay(Overlay::Tour);
        if self.tour.stage == TourStage::Step {
            self.show_step(Scroll::IfHidden, tx);
        }
    }

    /// Puts up step `self.tour.step`: its file in the reader, the range spotlit and scrolled as
    /// `scroll` says.
    fn show_step(&mut self, scroll: Scroll, tx: &mut Tx) {
        self.tour.live = None;
        self.want(scroll);
        self.apply_tour(tx);
        self.status = self.tour_status();
        self.save_tour();
    }

    fn want(&mut self, scroll: Scroll) {
        self.tour.pending = Some(Scroll::max(self.tour.pending, scroll));
    }

    /// Spotlights the step on screen (scrolling when asked), and hides or shows its findings.
    fn apply_tour(&mut self, tx: &mut Tx) {
        let Some(scroll) = self.tour.pending.take() else { return };
        self.refresh_hidden();
        let Some(v) = self.tour_current().filter(|_| self.tour.stage == TourStage::Step) else {
            self.drop_tour_spot();
            return;
        };
        let Some(at) = v.at else {
            self.drop_tour_spot();
            return;
        };
        let Some(g) = self.index_of(&at.key) else { return };
        let (a, b) = at.lines;
        let go = match scroll {
            Scroll::Always => true,
            Scroll::IfHidden => !self.local(g).is_some_and(|l| self.range_on_screen(l, at.side, (a, b))),
            Scroll::Never => false,
        };
        if go {
            self.goto_line(g, LineRef::new(at.side, a), tx);
            if let Some(l) = self.local(g) {
                self.active_mut().scroll_to_range(l, at.side, (a, b), Align::Third, tx);
            }
        }
        let Some(l) = self.local(g) else { return };
        let step = (!v.live).then(|| (self.tour.step as u32 + 1, self.tour_views().len() as u32));
        let spot = Spot {
            file: l,
            side: at.side,
            lines: (a, b),
            caption: Some(v.title.clone()),
            step,
        };
        self.active_mut().set_spotlight(Some(spot.clone()));
        self.tour.spot = Some((g, spot));
    }

    /// Whether lines `(a, b)` of active file `l` have a row inside the viewport.
    fn range_on_screen(&self, l: usize, side: Side, (a, b): (u32, u32)) -> bool {
        let d = self.active();
        let h = d.viewport_height();
        if h <= 0.0 {
            return false;
        }
        let top = d.scroll_offset();
        (a..=b.min(a.saturating_add(400))).any(|n| {
            d.row_of_line(l, LineRef::new(side, n)).is_some_and(|r| {
                let y = d.row_y(r as usize);
                y >= top && y < top + h
            })
        })
    }

    fn drop_tour_spot(&mut self) {
        if let Some((_, spot)) = self.tour.spot.take()
            && self.active().spotlight() == Some(&spot)
        {
            self.active_mut().set_spotlight(None);
        }
    }

    /// `n`: the next step without marking this one understood; past the last, the end card.
    pub fn tour_next(&mut self, tx: &mut Tx) {
        let n = self.tour_views().len();
        if self.tour.live.take().is_some() && n == 0 {
            self.leave_tour();
            return;
        }
        if self.tour.step + 1 >= n {
            self.end_tour();
            return;
        }
        self.tour.step += 1;
        self.show_step(Scroll::Always, tx);
    }

    /// `p`: the previous step (from the end card, the last one).
    pub fn tour_prev(&mut self, tx: &mut Tx) {
        let n = self.tour_views().len();
        if n == 0 {
            return;
        }
        if self.tour.stage == TourStage::End {
            self.tour.stage = TourStage::Step;
            self.tour.step = n - 1;
        } else if self.tour.live.take().is_none() {
            if self.tour.step == 0 {
                self.status = "This is the first step".into();
                return;
            }
            self.tour.step -= 1;
        }
        self.show_step(Scroll::Always, tx);
    }

    /// Goes to step `i` (0-based).
    pub fn tour_goto(&mut self, i: usize, tx: &mut Tx) {
        if i >= self.tour_views().len() {
            return;
        }
        self.tour.step = i;
        self.tour.stage = TourStage::Step;
        self.show_step(Scroll::Always, tx);
    }

    /// What `Space` does on the current step, as `tour_space` decides it: `show findings`,
    /// `understood · next`, `understood · finish`, or `next` on a live step.
    pub fn tour_space_does(&self) -> &'static str {
        let i = self.tour.step;
        if self.tour.live.is_some() {
            "next"
        } else if self.tour_findings_hidden(i) && !self.tour_findings(i).is_empty() {
            "show findings"
        } else if i + 1 >= self.tour_views().len() {
            "understood · finish"
        } else {
            "understood · next"
        }
    }

    /// `Space`: reveals hidden findings first; else understood, the file viewed when no later
    /// step needs it, and the next step.
    pub fn tour_space(&mut self, tx: &mut Tx) {
        let i = self.tour.step;
        if self.tour.live.is_some() {
            self.tour_next(tx);
            return;
        }
        if self.tour_findings_hidden(i) && !self.tour_findings(i).is_empty() {
            self.tour.revealed.insert(i);
            self.want(Scroll::Never);
            self.apply_tour(tx);
            let n = self.tour_findings(i).len();
            let s = if n == 1 { "" } else { "s" };
            self.status = format!("{n} finding{s} on this step · Space {}", self.tour_space_does());
            self.save_tour();
            return;
        }
        self.tour.understood.insert(i);
        let views = self.tour_views();
        if let Some(at) = views.get(i).and_then(|v| v.at.clone()) {
            let later = views[i + 1..].iter().any(|v| v.at.as_ref().is_some_and(|x| x.key == at.key));
            if !later && let Some(g) = self.index_of(&at.key) {
                self.set_viewed(g, true);
            }
        }
        self.tour_next(tx);
    }

    fn end_tour(&mut self) {
        self.tour.stage = TourStage::End;
        self.tour.live = None;
        self.drop_tour_spot();
        self.refresh_hidden();
        let n = self.tour_uncovered().len();
        self.status = format!("Tour done · not covered by the tour: {n} file(s) · Esc closes");
        self.save_tour();
    }

    /// `Esc` with the tour on top: leaves it, keeping the step for `Enter`. False when it is not open.
    pub(crate) fn leave_tour(&mut self) -> bool {
        if !self.close_overlay(Overlay::Tour) {
            return false;
        }
        self.drop_tour_spot();
        self.tour.live = None;
        self.refresh_hidden();
        match self.tour.stage {
            TourStage::Step => {
                self.tour.resumable = true;
                self.status = format!("Resume tour {}/{} · Enter", self.tour.step + 1, self.tour_views().len());
            }
            TourStage::Start => self.status = "Tour skipped · T opens it".into(),
            TourStage::End => {
                self.tour.resumable = false;
                self.status.clear();
            }
        }
        self.save_tour();
        true
    }

    /// The start card's "My pass first" box.
    pub fn toggle_my_pass_first(&mut self) {
        self.tour.my_pass_first = !self.tour.my_pass_first;
        self.want(Scroll::Never);
        self.save_tour();
    }

    /// Real changes no step cites, in list order.
    pub fn tour_uncovered(&self) -> Vec<usize> {
        let cited: BTreeSet<String> = self.tour_views().into_iter().filter_map(|v| v.at.map(|a| a.key)).collect();
        let mut seen = BTreeSet::new();
        self.list_order()
            .into_iter()
            .chain(0..self.keys.len())
            .filter(|&g| seen.insert(g) && !cited.contains(&self.keys[g]) && !self.is_noise(&self.keys[g]))
            .collect()
    }

    /// The end card's jump: file `g` at the top, the reader's place following it.
    pub fn tour_jump(&mut self, g: usize, tx: &mut Tx) {
        self.goto_file(g, tx);
        self.active_mut().set_cursor(None);
    }

    /// `~M min`: half a minute a step and a second and a half a cited line.
    pub fn tour_minutes(&self) -> u32 {
        let views = self.tour_views();
        let lines: u32 = views
            .iter()
            .filter_map(|v| v.at.as_ref().map(|a| a.lines.1.saturating_sub(a.lines.0) + 1))
            .sum();
        ((views.len() as u32 * 30 + lines * 3 / 2).div_ceil(60)).max(1)
    }

    fn tour_status(&self) -> String {
        match self.tour.stage {
            TourStage::Start => "Tour · Enter starts · Esc skips to the reader".into(),
            TourStage::End => "Tour done · p last step · Esc closes".into(),
            TourStage::Step => {
                let n = self.tour_views().len();
                let changed = self.tour_current().and_then(|v| v.at).is_some_and(|a| a.changed);
                let mut s = format!(
                    "Tour {}/{n} · Space {} · n p · Esc leaves",
                    self.tour.step + 1,
                    self.tour_space_does()
                );
                if changed {
                    s = format!("{s} · Diff changed");
                }
                s
            }
        }
    }

    // ----- driven from outside -----

    /// `POST /api/highlight` with a step: the tour opens at step `i` of the brief when it has one
    /// for that file, else on a live step of the agent's. False when `step` is `None`.
    pub(crate) fn tour_highlight(
        &mut self,
        path: &str,
        lines: (u32, u32),
        side: Side,
        caption: Option<String>,
        step: Option<(u32, u32)>,
        tx: &mut Tx,
    ) -> bool {
        let Some((i, n)) = step else { return false };
        self.load_tour();
        self.reanchor_tour();
        let key = rain_overview::brief::resolve(path, &self.keys).map(str::to_owned);
        let (a, b) = (lines.0.min(lines.1), lines.0.max(lines.1));
        let idx = (i as usize).saturating_sub(1);
        let views = self.tour_views();
        self.open_overlay(Overlay::Tour);
        self.tour.resumable = false;
        self.tour.stage = TourStage::Step;
        let brief_step = views
            .get(idx)
            .filter(|v| key.is_some() && v.at.as_ref().map(|x| &x.key) == key.as_ref());
        if let Some(v) = brief_step {
            let side = v.at.as_ref().map_or(side, |x| x.side);
            self.tour.step = idx;
            if let Some(Some(at)) = self.tour.anchors.get_mut(idx) {
                at.lines = (a, b);
                at.side = side;
            }
            self.show_step(Scroll::Always, tx);
        } else {
            let at = key.as_deref().map(|k| Anchored {
                key: k.to_owned(),
                side,
                lines: (a, b),
                cite_lines: Some((a, b)),
                text: Vec::new(),
                changed: false,
            });
            if at.is_none() {
                self.status = format!("Claude pointed at {path}, which is not in this diff");
            }
            self.tour.live = Some(TourView {
                id: format!("live.{i}"),
                title: caption.clone().unwrap_or_else(|| format!("Step {i} of {n}")),
                text: caption.unwrap_or_default(),
                cite: Cite {
                    path: path.to_owned(),
                    lines: Some((a, b)),
                    side: Some(side),
                    thread: None,
                },
                at,
                live: true,
            });
            self.want(Scroll::Always);
            self.apply_tour(tx);
            if self.tour.live.as_ref().is_some_and(|l| l.at.is_some()) {
                self.status = format!("Claude's walkthrough · step {i}/{n} · n next · Esc leaves");
            }
        }
        true
    }

    /// Every ready poll: anchors after a load, the deferred spotlight, and the spot kept up while
    /// the tour is open (a rebuilt or swapped diff drops it).
    pub(crate) fn tour_tick(&mut self, tx: &mut Tx) -> bool {
        let mark = self.keys.as_ptr() as usize;
        let mut changed = false;
        if mark != self.tour.load_mark {
            self.tour.load_mark = mark;
            if !self.tour.anchors.is_empty() {
                self.reanchor_tour();
                if self.tour_open() {
                    self.want(Scroll::IfHidden);
                }
                if self.tour_open() && self.tour.stage == TourStage::Step {
                    self.status = self.tour_status();
                }
                self.save_tour();
                changed = true;
            }
        }
        if !self.tour_open() {
            if self.tour.spot.is_some() {
                self.drop_tour_spot();
                self.refresh_hidden();
                changed = true;
            }
            return changed;
        }
        if let Some((g, spot)) = self.tour.spot.clone()
            && self
                .local(g)
                .is_some_and(|l| l != spot.file || self.active().spotlight() != Some(&spot))
        {
            self.want(Scroll::Never);
        }
        if self.tour.pending.is_some() {
            self.apply_tour(tx);
            changed = true;
        }
        changed
    }

    /// The picker's tour entry `label` (`3. Title`): opens the tour at that step.
    pub(crate) fn tour_from_picker(&mut self, label: &str, tx: &mut Tx) -> bool {
        let Some(i) = label.split_once('.').and_then(|(n, _)| n.trim().parse::<usize>().ok()) else {
            return false;
        };
        self.load_tour();
        self.reanchor_tour();
        self.open_overlay(Overlay::Tour);
        self.tour.resumable = false;
        self.tour_goto(i.saturating_sub(1), tx);
        true
    }

    /// The tour's keys: `n` `p` `Space` on a step, `Enter`/`Space` on the start card, `Enter` to
    /// resume after `Esc`. `None` lets the key through.
    pub(crate) fn tour_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> Option<bool> {
        let m = ev.mods;
        if m.ctrl || m.cmd || m.alt || m.logo {
            return None;
        }
        let space = matches!(&ev.key, Key::Text(t) if t == " ");
        let enter = ev.key == Key::Enter;
        if !self.tour_open() {
            if enter && self.tour.resumable && !m.shift {
                self.resume_tour(tx);
                return Some(true);
            }
            return None;
        }
        let ch = match &ev.key {
            Key::Text(t) if !m.shift => t.as_str(),
            _ => "",
        };
        match self.tour.stage {
            TourStage::Start if space || enter => self.begin_tour(tx),
            TourStage::Step if space => self.tour_space(tx),
            TourStage::Step if ch == "n" => self.tour_next(tx),
            TourStage::Step | TourStage::End if ch == "p" => self.tour_prev(tx),
            TourStage::End if space || enter => {
                self.leave_tour();
            }
            _ => return None,
        }
        Some(true)
    }

    // ----- kept per session -----

    fn tour_path(&self) -> Option<PathBuf> {
        self.autosave.as_ref()?;
        Some(self.session_path.as_ref()?.with_extension("tour.json"))
    }

    /// Reads the session's tour once.
    fn load_tour(&mut self) {
        let Some(p) = self.tour_path() else { return };
        if self.tour.loaded.as_ref() == Some(&p) {
            return;
        }
        self.tour.loaded = Some(p.clone());
        let Some(saved) = std::fs::read(&p).ok().and_then(|b| serde_json::from_slice::<Saved>(&b).ok()) else {
            return;
        };
        self.tour.stage = saved.stage;
        self.tour.step = saved.step;
        self.tour.my_pass_first = saved.my_pass_first;
        self.tour.understood = saved.understood.into_iter().collect();
        self.tour.revealed = saved.revealed.into_iter().collect();
        self.tour.anchors = saved.anchors;
        self.tour.resumable = saved.stage == TourStage::Step;
    }

    fn save_tour(&self) {
        let Some(p) = self.tour_path() else { return };
        let saved = Saved {
            stage: self.tour.stage,
            step: self.tour.step,
            my_pass_first: self.tour.my_pass_first,
            understood: self.tour.understood.iter().copied().collect(),
            revealed: self.tour.revealed.iter().copied().collect(),
            anchors: self.tour.anchors.clone(),
        };
        if let Ok(b) = serde_json::to_vec_pretty(&saved) {
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = std::fs::write(&p, b);
        }
    }

    // ----- scenes -----

    /// Scenes: the story brief, the agent's two findings, the tour at `stage` (a step shows
    /// step 1 with its findings hidden).
    pub(crate) fn scene_tour(&mut self, name: &str) {
        let stage = match name {
            "tour-step" => TourStage::Step,
            "tour-end" => TourStage::End,
            _ => TourStage::Start,
        };
        self.scene_brief(true);
        crate::seed_threads(self);
        self.reanchor_tour();
        self.tour.load_mark = self.keys.as_ptr() as usize;
        self.open_overlay(Overlay::Tour);
        self.tour.stage = stage;
        self.tour.step = 0;
        match stage {
            TourStage::Start => {}
            TourStage::Step => {
                self.want(Scroll::Always);
            }
            TourStage::End => {
                self.tour.understood = (0..self.tour_views().len()).collect();
            }
        }
        self.status.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_is_found_nearest_where_it_was() {
        let lines: Vec<(u32, &str)> = vec![(1, "}"), (2, "a"), (3, "}"), (4, "x"), (5, "a"), (6, "}")];
        let run = vec!["a".to_owned(), "}".to_owned()];
        assert_eq!(find_run(&lines, &run, 5), Some((5, 6)));
        assert_eq!(find_run(&lines, &run, 1), Some((2, 3)));
        assert_eq!(find_run(&lines, &["zz".to_owned()], 1), None);
        assert_eq!(find_run(&lines, &[], 1), None);
    }

    #[test]
    fn a_gap_in_line_numbers_breaks_a_run() {
        let lines: Vec<(u32, &str)> = vec![(1, "a"), (5, "b")];
        assert_eq!(find_run(&lines, &["a".to_owned(), "b".to_owned()], 1), None);
    }
}
