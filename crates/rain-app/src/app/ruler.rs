//! The diff's overview ruler: where the viewport is over the whole diff, and where its hunks,
//! files, threads, notes and find hits are.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use rain_comments::ops;
use rain_core::diff::{LineKind, Side};
use rain_diffview::{DiffState, LineRef, SCROLL_KEY};
use rune::{Color, Live, Phase, ScrollPos, Tx};
use rune_kit::Kit;

use super::App;

/// The strip's width in points.
const RULER_W: f32 = 14.0;
/// The thumb never gets shorter than this, however long the diff.
const MIN_THUMB: f32 = 12.0;

/// Fed by the diff viewport's scroll handler, read by the ruler's thumb without a view run.
pub struct RulerState {
    /// The viewport's vertical `[offset, max, viewport, drawn]` from its last `ScrollPos`.
    pub live: Live<[f32; 4]>,
    /// The thumb's rect on the track, `[x, y, w, h]`.
    pub thumb: Live<[f32; 4]>,
    /// The track's height, from its layout; shared with the scroll handler.
    pub track: Rc<Cell<f32>>,
    /// The pointer is over the track or holds it.
    pub hot: Live<f32>,
    /// The full-list file the pointer last pointed at on the track.
    pub pointing: Option<usize>,
    /// Marks of the last build, by what they were built from.
    cache: RefCell<Option<(u64, Rc<Vec<Mark>>)>>,
    /// How many times marks were built (each one walks every hunk).
    pub builds: Cell<u32>,
}

impl RulerState {
    /// The strip's width in points.
    pub const WIDTH: f32 = RULER_W;

    /// The diff viewport's scroll handler: moves the thumb every frame it scrolls, with no view run.
    pub fn follower(&self) -> impl Fn(ScrollPos) + 'static {
        let (thumb, track) = (self.thumb.clone(), self.track.clone());
        move |at| place_thumb(&thumb, [at.offset[1], at.max[1], at.viewport[1], at.drawn[1]], track.get())
    }
}

impl Default for RulerState {
    fn default() -> RulerState {
        RulerState {
            live: Live::new([0.0; 4]),
            thumb: Live::new([0.0; 4]),
            track: Rc::new(Cell::new(0.0)),
            hot: Live::new(0.0),
            pointing: None,
            cache: RefCell::new(None),
            builds: Cell::new(0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MarkKind {
    /// A hunk, with the share of its changed lines that are added.
    Hunk {
        added: f32,
    },
    File,
    Comment {
        waiting: bool,
    },
    Note,
    Find,
}

/// One mark on a track: `y` and `h` in whole points from the track's top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mark {
    kind: MarkKind,
    pub y: f32,
    pub h: f32,
}

impl Mark {
    /// `hunk`, `file`, `comment`, `waiting` (a thread waiting on the user), `note` or `find`.
    #[cfg(test)]
    pub fn name(&self) -> &'static str {
        match self.kind {
            MarkKind::Hunk { .. } => "hunk",
            MarkKind::File => "file",
            MarkKind::Comment { waiting: false } => "comment",
            MarkKind::Comment { waiting: true } => "waiting",
            MarkKind::Note => "note",
            MarkKind::Find => "find",
        }
    }

    /// A hunk's share of changed lines that are added.
    #[cfg(test)]
    pub fn added(&self) -> Option<f32> {
        match self.kind {
            MarkKind::Hunk { added } => Some(added),
            _ => None,
        }
    }

    /// `[x0, y0, x1, y1]` on the strip: file starts across it, every other mark inset 3.
    pub fn rect(&self) -> [f32; 4] {
        let (x0, x1) = match self.kind {
            MarkKind::File => (1.0, RULER_W),
            _ => (3.0, RULER_W - 3.0),
        };
        [x0, self.y, x1, self.y + self.h]
    }

    /// Drawn as an outline rather than filled (none are, in the redesign).
    pub fn outlined(&self) -> bool {
        false
    }

    /// Hunks a quiet neutral (where the changes are, not what they do), file starts the strong
    /// line, threads info, waiting ones warning, notes the note colour, find hits the current hit's.
    pub fn color(&self, k: &Kit, note: Color) -> Color {
        match self.kind {
            MarkKind::Hunk { .. } => k.c.text_faint,
            MarkKind::File => k.c.border_strong,
            MarkKind::Comment { waiting: true } => k.c.warning.fg,
            MarkKind::Comment { waiting: false } => k.c.info.fg,
            MarkKind::Note => note,
            MarkKind::Find => crate::theme::roles_of(k).find_current,
        }
    }
}

/// The thumb on a track `track` long for a viewport at `[offset, max, viewport, _]`: `(top, len)`.
fn thumb_span(at: [f32; 4], track: f32) -> (f32, f32) {
    let [offset, max, viewport, _] = at;
    let content = (max + viewport).max(1.0);
    let len = (track * viewport / content).clamp(MIN_THUMB.min(track), track);
    let top = (track * offset / content).clamp(0.0, (track - len).max(0.0));
    (top, len)
}

/// Where `row` starts on a track `track` long, as a share of the rows' estimated height.
fn row_frac(d: &DiffState, row: usize) -> f32 {
    let total = d.rows_height();
    if total <= 0.0 {
        0.0
    } else {
        (d.row_y(row) / total).clamp(0.0, 1.0)
    }
}

fn line_ref(rec: &rain_core::diff::LineRec) -> LineRef {
    if rec.new != 0 {
        LineRef::new(Side::New, rec.new)
    } else {
        LineRef::new(Side::Old, rec.old)
    }
}

impl App {
    /// What the marks are built from; equal keys give equal marks.
    fn marks_key(&self, track: f32) -> u64 {
        let d = self.active();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let subset = self
            .subset()
            .map(|v| (v.files.len(), v.files.first().copied(), v.files.last().copied()));
        (subset, self.chapter).hash(&mut h);
        (
            d.row_count(),
            d.rows_height().to_bits(),
            d.file_rows().len(),
            d.mode() == rain_diffview::DiffMode::Split,
        )
            .hash(&mut h);
        d.annotations().hash(&mut h);
        // Of the session, marks read only the threads (who each waits on); notes come in as annotations.
        (self.threads_rev, &self.find.query, self.find.hits.len(), track.round() as i32).hash(&mut h);
        h.finish()
    }

    /// The ruler's marks for a track `track` points long, built again only when the rows,
    /// annotations, session, find or track height changed.
    pub(crate) fn ruler_marks(&self, track: f32) -> Rc<Vec<Mark>> {
        let key = self.marks_key(track);
        if let Some((k, m)) = &*self.ruler.cache.borrow()
            && *k == key
        {
            return m.clone();
        }
        let m = Rc::new(marks(self, track));
        self.ruler.builds.set(self.ruler.builds.get() + 1);
        *self.ruler.cache.borrow_mut() = Some((key, m.clone()));
        m
    }

    /// The row at a share `frac` of the active diff's height.
    fn ruler_row_at(&self, frac: f32) -> Option<usize> {
        let d = self.active();
        let n = d.row_count();
        if n == 0 {
            return None;
        }
        let y = frac.clamp(0.0, 1.0) * d.rows_height();
        let (mut lo, mut hi) = (0usize, n);
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if d.row_y(mid) <= y { lo = mid } else { hi = mid }
        }
        Some(lo)
    }

    /// The full-list file under a share `frac` of the track.
    pub(crate) fn ruler_file_at(&self, frac: f32) -> Option<usize> {
        let row = self.ruler_row_at(frac)?;
        let local = self.active().row_file(row)?;
        let g = self.global(local);
        (g < self.keys.len()).then_some(g)
    }

    /// A press or drag on the track at `y` of `h` points: scroll so that spot sits mid-viewport.
    pub(crate) fn ruler_drag(&mut self, d: &mut rune::Drag, tx: &mut Tx) {
        let h = if d.h > 0.0 { d.h } else { self.ruler.track.get() };
        if h <= 0.0 || self.active().row_count() == 0 {
            return;
        }
        let frac = (d.y / h).clamp(0.0, 1.0);
        let [_, max, viewport, _] = self.ruler.live.get();
        tx.scroll_to_y(SCROLL_KEY, (frac * (max + viewport) - viewport / 2.0).clamp(0.0, max.max(0.0)));
        let file = self.ruler_file_at(frac);
        match d.phase {
            Phase::Down => {
                self.active_mut().set_cursor(None);
                self.ruler.hot.set(1.0);
                self.point_at(file);
            }
            Phase::Up => {
                self.ruler.hot.set(0.0);
                self.ruler.pointing = None;
            }
            _ if file == self.ruler.pointing => tx.live_only(),
            _ => self.point_at(file),
        }
    }

    /// The pointer over the track at `y` points from its top, or gone (`None`).
    pub(crate) fn ruler_hover(&mut self, y: Option<f32>, tx: &mut Tx) {
        let h = self.ruler.track.get();
        let file = y.filter(|_| h > 0.0).and_then(|y| self.ruler_file_at(y / h));
        self.ruler.hot.set(if y.is_some() { 1.0 } else { 0.0 });
        if y.is_none() {
            self.ruler.pointing = None;
        }
        if file.is_none() || file == self.ruler.pointing {
            tx.live_only();
            return;
        }
        self.point_at(file);
    }

    fn point_at(&mut self, file: Option<usize>) {
        self.ruler.pointing = file;
        if let Some(g) = file {
            self.status = self.keys[g].clone();
        }
    }

    /// The track's laid-out height; the thumb follows at once.
    pub(crate) fn set_ruler_track(&mut self, h: f32, tx: &mut Tx) {
        let old = self.ruler.track.replace(h);
        place_thumb(&self.ruler.thumb, self.ruler.live.get(), h);
        if (old - h).abs() < 0.5 {
            tx.live_only();
        }
    }
}

/// Puts the thumb where a viewport at `at` puts it on a track `track` long.
fn place_thumb(thumb: &Live<[f32; 4]>, at: [f32; 4], track: f32) {
    let (top, len) = thumb_span(at, track);
    thumb.snap([1.0, top, RULER_W - 2.0, len]);
}

/// Every mark for a track `track` points long, from the active diff, the session's threads,
/// notes and find hits. O(hunk lines + annotations + hits).
fn marks(s: &App, track: f32) -> Vec<Mark> {
    let d = s.active();
    let mut out: Vec<Mark> = Vec::new();
    if d.row_count() == 0 || track <= 0.0 {
        return out;
    }
    let at = |row: usize| (row_frac(d, row) * track).floor();
    for &r in d.file_rows() {
        out.push(Mark {
            kind: MarkKind::File,
            y: at(r as usize),
            h: 1.0,
        });
    }
    for (fi, f) in d.files().iter().enumerate() {
        for (hi, hunk) in f.hunks.iter().enumerate() {
            let Some(first) = d.hunk_row(fi, hi) else { continue };
            let (mut add, mut del) = (0u32, 0u32);
            let mut last = first;
            for i in hunk.lines.clone() {
                let rec = &f.lines[i as usize];
                match rec.kind {
                    LineKind::Added => add += 1,
                    LineKind::Removed => del += 1,
                    LineKind::Context => {}
                }
                if i + 1 == hunk.lines.end
                    && let Some(r) = d.row_of_line(fi, line_ref(rec))
                {
                    last = last.max(r);
                }
            }
            if add + del == 0 {
                continue;
            }
            let y = at(first as usize);
            let end = (row_frac(d, last as usize + 1) * track).ceil();
            let m = Mark {
                kind: MarkKind::Hunk {
                    added: add as f32 / (add + del) as f32,
                },
                y,
                h: (end - y).max(2.0),
            };
            if out.last() != Some(&m) {
                out.push(m);
            }
        }
    }
    let waiting: HashMap<&str, bool> = s
        .mirror
        .threads
        .iter()
        .map(|t| (t.id.as_str(), ops::waiting_on_user(t, crate::app::ME)))
        .collect();
    let mut ticks: Vec<Mark> = Vec::new();
    for (row, id) in d.annotation_rows() {
        let kind = match waiting.get(id) {
            Some(&w) => MarkKind::Comment { waiting: w },
            None if id == "composer" => continue,
            None => MarkKind::Note,
        };
        ticks.push(Mark {
            kind,
            y: at(row as usize),
            h: 3.0,
        });
    }
    for &(fi, li) in &s.find.hits {
        let Some(rec) = d.files().get(fi).and_then(|f| f.lines.get(li as usize)) else {
            continue;
        };
        if let Some(row) = d.row_of_line(fi, line_ref(rec)) {
            ticks.push(Mark {
                kind: MarkKind::Find,
                y: at(row as usize),
                h: 2.0,
            });
        }
    }
    ticks.dedup();
    out.extend(ticks);
    out
}
