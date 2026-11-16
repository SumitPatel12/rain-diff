//! The diff's scrollbar: where the viewport is over the whole diff, and pressing or dragging on
//! it to move there.

use std::cell::Cell;
use std::rc::Rc;

use rain_diffview::SCROLL_KEY;
use rune::{Live, Phase, ScrollPos, Tx};

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

impl App {
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
