//! The playground's motion: a timeline clock, and values on that timeline bound to elements so a
//! frame moves them in the renderer without rebuilding the view.
//!
//! Every animated value here is a pure function of the timeline's `t`: [`Rig::publish`] sets
//! the bound [`Live`] values for a given `t`, so a frame, a scrub and a headless snapshot at the
//! same `t` draw the same picture. Nothing runs while the clock is paused.

use std::any::Any;
use std::cell::Cell;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rune::live::LiveValue;
use rune::{Color, El, Live, Spring};

/// How often a playing timeline moves its values.
pub const FRAME: Duration = Duration::from_millis(16);
/// Opacity of what is outside the focus or the highlight.
pub const DIM: f32 = 0.28;
/// The spring a highlight or focus fades with.
pub const FADE: Spring = Spring::new(0.25, 0.0);
/// The spring a camera move glides with.
pub const CAMERA: Spring = Spring::new(0.9, 0.0);

// ----- the clock -----

#[derive(Clone, Copy, Debug, PartialEq)]
enum Run {
    Paused,
    /// Play was pressed; the next tick anchors the timeline to the host clock it sees, so an
    /// old host time (a poll long ago) is never counted as played.
    Starting,
    Playing {
        host: f32,
        from: f32,
    },
}

/// What a tick did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// Paused: nothing moved and nothing was published.
    Idle,
    /// The timeline moved within its step.
    Moved,
    /// The timeline moved into another step.
    Stepped,
    /// The timeline reached its end or the stop it was playing to, and paused.
    Ended,
}

/// A play, pause and scrub clock over a timeline `duration` seconds long, driven by the host's
/// frame clock (seconds) only while it plays.
#[derive(Clone, Debug, PartialEq)]
pub struct Clock {
    t: f32,
    duration: f32,
    speed: f32,
    run: Run,
    /// Where a step forward stops.
    stop: Option<f32>,
}

impl Default for Clock {
    fn default() -> Clock {
        Clock::new(1.0)
    }
}

impl Clock {
    pub fn new(duration: f32) -> Clock {
        Clock {
            t: 0.0,
            duration: duration.max(0.01),
            speed: 1.0,
            run: Run::Paused,
            stop: None,
        }
    }

    pub fn t(&self) -> f32 {
        self.t
    }

    pub fn duration(&self) -> f32 {
        self.duration
    }

    pub fn set_duration(&mut self, d: f32) {
        self.duration = d.max(0.01);
        self.t = self.t.min(self.duration);
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// 0 to 1.
    pub fn progress(&self) -> f32 {
        (self.t / self.duration).clamp(0.0, 1.0)
    }

    pub fn playing(&self) -> bool {
        self.run != Run::Paused
    }

    pub fn at_end(&self) -> bool {
        self.t >= self.duration - 1e-4
    }

    /// Plays from here (from the start when at the end).
    pub fn play(&mut self) {
        self.stop = None;
        if self.at_end() {
            self.t = 0.0;
        }
        self.run = Run::Starting;
    }

    pub fn pause(&mut self) {
        self.run = Run::Paused;
        self.stop = None;
    }

    pub fn toggle(&mut self) {
        if self.playing() { self.pause() } else { self.play() }
    }

    /// Jumps to `t`, playing on from there if it was playing.
    pub fn seek(&mut self, t: f32) {
        self.t = t.clamp(0.0, self.duration);
        if self.playing() {
            self.run = Run::Starting;
        }
    }

    pub fn restart(&mut self) {
        self.t = 0.0;
        self.play();
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed.clamp(0.1, 8.0);
        if self.playing() {
            self.run = Run::Starting;
        }
    }

    /// Forward plays to the next mark after `t` and stops there; back jumps to the mark before.
    /// `marks` are timeline seconds in order.
    pub fn step(&mut self, marks: &[f32], forward: bool, reduced: bool) {
        let eps = 1e-3;
        if forward {
            let next = marks.iter().copied().find(|&m| m > self.t + eps).unwrap_or(self.duration);
            if reduced {
                self.t = next.min(self.duration);
                self.run = Run::Paused;
                return;
            }
            self.stop = Some(next.min(self.duration));
            self.run = Run::Starting;
        } else {
            let prev = marks.iter().copied().rev().find(|&m| m < self.t - eps).unwrap_or(0.0);
            self.t = prev.max(0.0);
            self.pause();
        }
    }

    /// Moves the timeline to the host clock `host`; `Idle` (and no change) while paused.
    pub fn tick(&mut self, host: f32) -> Tick {
        let (anchor, from) = match self.run {
            Run::Paused => return Tick::Idle,
            Run::Starting => {
                self.run = Run::Playing { host, from: self.t };
                return Tick::Moved;
            }
            Run::Playing { host, from } => (host, from),
        };
        let end = self.stop.unwrap_or(self.duration).min(self.duration);
        let t = from + (host - anchor).max(0.0) * self.speed;
        if t >= end {
            self.t = end;
            self.pause();
            return Tick::Ended;
        }
        self.t = t;
        Tick::Moved
    }
}

// ----- easing and blending -----

/// Smooth in and out: cubic.
pub fn ease(p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    if p < 0.5 {
        4.0 * p * p * p
    } else {
        1.0 - (-2.0 * p + 2.0).powi(3) / 2.0
    }
}

/// How far into the window `[start, start + dur]` the timeline is, 0 to 1.
pub fn window(t: f32, start: f32, dur: f32) -> f32 {
    if dur <= 0.0 {
        return if t >= start { 1.0 } else { 0.0 };
    }
    ((t - start) / dur).clamp(0.0, 1.0)
}

/// A value that blends between two others.
pub trait Lerp: Copy {
    fn lerp(a: Self, b: Self, k: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(a: f32, b: f32, k: f32) -> f32 {
        a + (b - a) * k
    }
}

impl Lerp for [f32; 2] {
    fn lerp(a: Self, b: Self, k: f32) -> Self {
        [f32::lerp(a[0], b[0], k), f32::lerp(a[1], b[1], k)]
    }
}

impl Lerp for [f32; 4] {
    fn lerp(a: Self, b: Self, k: f32) -> Self {
        [0, 1, 2, 3].map(|i| f32::lerp(a[i], b[i], k))
    }
}

impl Lerp for Color {
    fn lerp(a: Color, b: Color, k: f32) -> Color {
        Color {
            r: f32::lerp(a.r, b.r, k),
            g: f32::lerp(a.g, b.g, k),
            b: f32::lerp(a.b, b.b, k),
            a: f32::lerp(a.a, b.a, k),
        }
    }
}

/// Something on the timeline that publishes its bound values for a given `t`.
pub trait Animate {
    fn publish(&self, t: f32);
}

// ----- tracks -----

/// A value with keys on the timeline, eased between them and held before the first and after
/// the last.
#[derive(Clone)]
pub struct Track<T: LiveValue + Lerp> {
    pub live: Live<T>,
    keys: Arc<Vec<(f32, T)>>,
}

impl<T: LiveValue + Lerp> Track<T> {
    /// `keys` are `(t, value)` in time order; at least one.
    pub fn new(keys: Vec<(f32, T)>) -> Track<T> {
        assert!(!keys.is_empty(), "a track needs a key");
        let first = keys[0].1;
        Track {
            live: Live::new(first),
            keys: Arc::new(keys),
        }
    }

    pub fn at(&self, t: f32) -> T {
        let k = &self.keys;
        if t <= k[0].0 {
            return k[0].1;
        }
        for w in k.windows(2) {
            let ((t0, a), (t1, b)) = (w[0], w[1]);
            if t < t1 {
                return T::lerp(a, b, ease(window(t, t0, t1 - t0)));
            }
        }
        k[k.len() - 1].1
    }
}

impl<T: LiveValue + Lerp> Animate for Track<T> {
    fn publish(&self, t: f32) {
        self.live.set(self.at(t));
    }
}

// ----- keyed enter, move and leave -----

/// What happens to one keyed mark between the two states.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Only in the after state: it grows and fades in.
    Enter,
    /// In both, somewhere else: it glides.
    Move,
    /// Only in the before state: it shrinks and fades out.
    Leave,
    /// In both, in the same place.
    Stay,
}

impl Change {
    pub fn of(before: Option<[f32; 4]>, after: Option<[f32; 4]>) -> Change {
        match (before, after) {
            (None, Some(_)) => Change::Enter,
            (Some(_), None) => Change::Leave,
            (Some(a), Some(b)) if a != b => Change::Move,
            _ => Change::Stay,
        }
    }
}

/// `r` scaled by `k` about its centre.
pub fn shrink(r: [f32; 4], k: f32) -> [f32; 4] {
    let (w, h) = (r[2] * k, r[3] * k);
    [r[0] + (r[2] - w) / 2.0, r[1] + (r[3] - h) / 2.0, w, h]
}

/// One mark's rect and opacity through a before → after change in the window
/// `[start, start + dur]`.
#[derive(Clone)]
pub struct Keyed {
    pub rect: Live<[f32; 4]>,
    pub alpha: Live<f32>,
    pub change: Change,
    from: ([f32; 4], f32),
    to: ([f32; 4], f32),
    start: f32,
    dur: f32,
}

impl Keyed {
    pub fn new(before: Option<[f32; 4]>, after: Option<[f32; 4]>, start: f32, dur: f32) -> Keyed {
        let change = Change::of(before, after);
        let (from, to) = match (before, after) {
            (Some(a), Some(b)) => ((a, 1.0), (b, 1.0)),
            (None, Some(b)) => ((shrink(b, 0.6), 0.0), (b, 1.0)),
            (Some(a), None) => ((a, 1.0), (shrink(a, 0.8), 0.0)),
            (None, None) => (([0.0; 4], 0.0), ([0.0; 4], 0.0)),
        };
        Keyed {
            rect: Live::new(from.0),
            alpha: Live::new(from.1),
            change,
            from,
            to,
            start,
            dur,
        }
    }

    pub fn at(&self, t: f32) -> ([f32; 4], f32) {
        let k = ease(window(t, self.start, self.dur));
        (<[f32; 4]>::lerp(self.from.0, self.to.0, k), f32::lerp(self.from.1, self.to.1, k))
    }

    /// The rect it lays out at (the presentation rect moves from there).
    pub fn rest(&self) -> [f32; 4] {
        if self.change == Change::Leave { self.from.0 } else { self.to.0 }
    }

    /// `e` placed at its rect, moved and faded by the timeline.
    pub fn bind(&self, e: El) -> El {
        let r = self.rest();
        e.absolute(r[0], r[1])
            .size(r[2], r[3])
            .live_rect(&self.rect)
            .live_opacity(&self.alpha)
    }
}

impl Animate for Keyed {
    fn publish(&self, t: f32) {
        let (r, a) = self.at(t);
        self.rect.set(r);
        self.alpha.set(a);
    }
}

// ----- write-on -----

/// A canvas's strokes drawing on (or off) in the window `[start, start + dur]`, through
/// `Shape::trimmed` and `El::live_trim`: no shape is rebuilt.
#[derive(Clone)]
pub struct WriteOn {
    pub trim: Live<[f32; 2]>,
    start: f32,
    dur: f32,
    undraw: bool,
}

impl WriteOn {
    pub fn draw(start: f32, dur: f32) -> WriteOn {
        WriteOn::make(start, dur, false)
    }

    /// Drawn whole, then wiped away from its end back to its start.
    pub fn undraw(start: f32, dur: f32) -> WriteOn {
        WriteOn::make(start, dur, true)
    }

    fn make(start: f32, dur: f32, undraw: bool) -> WriteOn {
        let w = WriteOn {
            trim: Live::new([0.0, 0.0]),
            start,
            dur,
            undraw,
        };
        w.trim.set(w.at(0.0));
        w
    }

    pub fn at(&self, t: f32) -> [f32; 2] {
        let p = ease(window(t, self.start, self.dur));
        if self.undraw { [0.0, 1.0 - p] } else { [0.0, p] }
    }

    pub fn bind(&self, canvas: El) -> El {
        canvas.live_trim(&self.trim)
    }
}

impl Animate for WriteOn {
    fn publish(&self, t: f32) {
        self.trim.set(self.at(t));
    }
}

// ----- pulses -----

/// Total length of a polyline and the length at each of its points.
fn lengths(points: &[[f32; 2]]) -> Vec<f32> {
    let mut out = Vec::with_capacity(points.len());
    let mut d = 0.0;
    for (i, p) in points.iter().enumerate() {
        if i > 0 {
            let q = points[i - 1];
            d += ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt();
        }
        out.push(d);
    }
    out
}

/// The point `frac` (0 to 1) of the way along `points` by arc length.
pub fn along(points: &[[f32; 2]], frac: f32) -> [f32; 2] {
    along_with(points, &lengths(points), frac)
}

fn along_with(points: &[[f32; 2]], cum: &[f32], frac: f32) -> [f32; 2] {
    let Some(&total) = cum.last() else { return [0.0; 2] };
    if points.len() == 1 || total <= 0.0 {
        return points[0];
    }
    let s = frac.clamp(0.0, 1.0) * total;
    let i = cum.partition_point(|&c| c < s).clamp(1, points.len() - 1);
    let seg = cum[i] - cum[i - 1];
    let k = if seg > 0.0 { (s - cum[i - 1]) / seg } else { 0.0 };
    <[f32; 2]>::lerp(points[i - 1], points[i], k)
}

/// A dot running along a polyline by arc length: from `start`, one run each `period` seconds,
/// once or on repeat, fading in and out at the ends.
#[derive(Clone)]
pub struct Pulse {
    /// The dot's top-left, for `El::live_offset`.
    pub pos: Live<[f32; 2]>,
    pub alpha: Live<f32>,
    points: Arc<Vec<[f32; 2]>>,
    cum: Arc<Vec<f32>>,
    start: f32,
    period: f32,
    repeat: bool,
    size: f32,
}

impl Pulse {
    pub fn new(points: Vec<[f32; 2]>, start: f32, period: f32, repeat: bool, size: f32) -> Pulse {
        let cum = lengths(&points);
        let p = Pulse {
            pos: Live::new([0.0; 2]),
            alpha: Live::new(0.0),
            points: Arc::new(points),
            cum: Arc::new(cum),
            start,
            period: period.max(0.05),
            repeat,
            size,
        };
        p.publish(0.0);
        p
    }

    pub fn at(&self, t: f32) -> ([f32; 2], f32) {
        let local = (t - self.start) / self.period;
        if local < 0.0 || (!self.repeat && local > 1.0) || self.points.is_empty() {
            return (self.corner(0.0), 0.0);
        }
        let frac = if self.repeat { local.fract() } else { local.min(1.0) };
        let fade = (frac * 8.0).min((1.0 - frac) * 8.0).clamp(0.0, 1.0);
        (self.corner(frac), fade)
    }

    fn corner(&self, frac: f32) -> [f32; 2] {
        if self.points.is_empty() {
            return [0.0; 2];
        }
        let c = along_with(&self.points, &self.cum, frac);
        [c[0] - self.size / 2.0, c[1] - self.size / 2.0]
    }

    /// `dot` sized and moved along the line (place it in the line's coordinates).
    pub fn bind(&self, dot: El) -> El {
        dot.absolute(0.0, 0.0)
            .size(self.size, self.size)
            .live_offset(&self.pos)
            .live_opacity(&self.alpha)
    }
}

impl Animate for Pulse {
    fn publish(&self, t: f32) {
        let (p, a) = self.at(t);
        self.pos.set(p);
        self.alpha.set(a);
    }
}

// ----- the camera -----

/// A pan and zoom over a world element `view` points big: bind [`Camera::bind`] on the element
/// holding the whole picture. Scale is about the element's centre, as `El::live_scale` scales.
#[derive(Clone)]
pub struct Camera {
    pub offset: Live<[f32; 2]>,
    pub scale: Live<f32>,
    view: [f32; 2],
    /// `(t, centre, zoom)` keys, when the timeline moves the camera.
    keys: Arc<Vec<(f32, [f32; 2], f32)>>,
}

impl Camera {
    pub fn new(view: [f32; 2]) -> Camera {
        Camera {
            offset: Live::new([0.0; 2]),
            scale: Live::new(1.0),
            view,
            keys: Arc::new(Vec::new()),
        }
    }

    /// Moves on the timeline through `(t, centre, zoom)` keys, centres in world points.
    pub fn keyed(view: [f32; 2], keys: Vec<(f32, [f32; 2], f32)>) -> Camera {
        Camera {
            keys: Arc::new(keys),
            ..Camera::new(view)
        }
    }

    /// The offset and scale that put world point `centre` in the middle at `zoom`.
    pub fn pose(&self, centre: [f32; 2], zoom: f32) -> ([f32; 2], f32) {
        let mid = [self.view[0] / 2.0, self.view[1] / 2.0];
        ([-(centre[0] - mid[0]) * zoom, -(centre[1] - mid[1]) * zoom], zoom)
    }

    /// The zoom that fits `rect` with `pad` points around it, between 0.5 and 3.
    pub fn fit(&self, rect: [f32; 4], pad: f32) -> f32 {
        let zx = self.view[0] / (rect[2] + 2.0 * pad).max(1.0);
        let zy = self.view[1] / (rect[3] + 2.0 * pad).max(1.0);
        zx.min(zy).clamp(0.5, 3.0)
    }

    /// Looks at `rect` now (a click, a tour step); bound smoothly, it glides there.
    pub fn look_at(&self, rect: [f32; 4], pad: f32) {
        let zoom = self.fit(rect, pad);
        let (o, s) = self.pose([rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0], zoom);
        self.offset.set(o);
        self.scale.set(s);
    }

    /// Back to the whole picture.
    pub fn reset(&self) {
        self.offset.set([0.0; 2]);
        self.scale.set(1.0);
    }

    /// `world` moved by the camera; it glides unless motion is reduced.
    pub fn bind(&self, world: El, reduced: bool) -> El {
        if reduced {
            world.live_offset(&self.offset).live_scale(&self.scale)
        } else {
            world.live_offset(self.offset.smooth(CAMERA)).live_scale(self.scale.smooth(CAMERA))
        }
    }
}

impl Animate for Camera {
    fn publish(&self, t: f32) {
        let k = &self.keys;
        if k.is_empty() {
            return;
        }
        let (mut c, mut z) = (k[0].1, k[0].2);
        if t > k[0].0 {
            (c, z) = (k[k.len() - 1].1, k[k.len() - 1].2);
            for w in k.windows(2) {
                if t < w[1].0 {
                    let p = ease(window(t, w[0].0, w[1].0 - w[0].0));
                    (c, z) = (<[f32; 2]>::lerp(w[0].1, w[1].1, p), f32::lerp(w[0].2, w[1].2, p));
                    break;
                }
            }
        }
        let (o, s) = self.pose(c, z);
        self.offset.set(o);
        self.scale.set(s);
    }
}

// ----- highlight and focus -----

#[derive(Default)]
struct Marks {
    lit: BTreeSet<String>,
    /// Per id: its opacity (dimmed when something else is lit) and its lit weight (1 when lit).
    marks: HashMap<String, (Live<f32>, Live<f32>)>,
}

impl Marks {
    fn values(&self, id: &str) -> (f32, f32) {
        let on = self.lit.contains(id);
        let opacity = if self.lit.is_empty() || on { 1.0 } else { DIM };
        (opacity, if on { 1.0 } else { 0.0 })
    }
}

/// One channel of lit ids (what hover points at, or what a step focuses) and, per id, the
/// values bound to its marks. Setting it moves only those values: no view is rebuilt.
#[derive(Clone, Default)]
pub struct Highlight(Arc<Mutex<Marks>>);

impl Highlight {
    fn entry(&self, id: &str) -> (Live<f32>, Live<f32>) {
        let mut m = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(e) = m.marks.get(id) {
            return e.clone();
        }
        let (o, l) = m.values(id);
        let e = (Live::new(o), Live::new(l));
        m.marks.insert(id.to_owned(), e.clone());
        e
    }

    /// 1, or [`DIM`] while something else is lit.
    pub fn opacity(&self, id: &str) -> Live<f32> {
        self.entry(id).0
    }

    /// 1 while lit, else 0: bind an overlay's opacity to it.
    pub fn lit(&self, id: &str) -> Live<f32> {
        self.entry(id).1
    }

    /// Lights exactly `ids`; an empty set lights nothing and dims nothing.
    pub fn set<I: IntoIterator<Item = S>, S: Into<String>>(&self, ids: I) {
        let lit: BTreeSet<String> = ids.into_iter().map(Into::into).collect();
        let mut m = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if lit != m.lit {
            Highlight::light(&mut m, lit);
        }
    }

    /// Lights exactly `ids`, which hold no repeats; when they are already lit it allocates
    /// nothing, so a timeline can call it every frame.
    fn set_distinct(&self, ids: &[String]) {
        let mut m = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if m.lit.len() != ids.len() || !ids.iter().all(|i| m.lit.contains(i)) {
            Highlight::light(&mut m, ids.iter().cloned().collect());
        }
    }

    fn light(m: &mut Marks, lit: BTreeSet<String>) {
        m.lit = lit;
        for (id, (o, l)) in &m.marks {
            let (ov, lv) = m.values(id);
            o.set(ov);
            l.set(lv);
        }
    }

    pub fn clear(&self) {
        self.set(Vec::<String>::new());
    }

    pub fn current(&self) -> BTreeSet<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).lit.clone()
    }

    /// `e` lights `ids` while the pointer is over it, moving only bound values.
    pub fn hover(&self, e: El, ids: Vec<String>) -> El {
        let h = self.clone();
        e.on_hover_change_any(move |inside, tx| {
            if inside {
                h.set(ids.iter().cloned());
            } else {
                h.clear();
            }
            tx.live_only();
        })
    }
}

/// `e`'s opacity bound to `live`, fading unless motion is reduced.
pub fn fade(e: El, live: &Live<f32>, reduced: bool) -> El {
    if reduced {
        e.live_opacity(live)
    } else {
        e.live_opacity(live.smooth(FADE))
    }
}

/// A highlight channel driven by the timeline: from each key's `t`, its ids are lit.
#[derive(Clone)]
pub struct FocusTrack {
    pub channel: Highlight,
    keys: Arc<Vec<(f32, Vec<String>)>>,
}

impl FocusTrack {
    pub fn new(channel: Highlight, mut keys: Vec<(f32, Vec<String>)>) -> FocusTrack {
        for (_, ids) in &mut keys {
            ids.sort_unstable();
            ids.dedup();
        }
        FocusTrack {
            channel,
            keys: Arc::new(keys),
        }
    }
}

impl Animate for FocusTrack {
    fn publish(&self, t: f32) {
        let ids = self
            .keys
            .iter()
            .rev()
            .find(|(at, _)| *at <= t)
            .map_or(&[][..], |(_, ids)| ids.as_slice());
        self.channel.set_distinct(ids);
    }
}

// ----- the rig -----

/// A named point on the timeline: Step back and forward move between them.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub at: f32,
    pub label: String,
}

/// Everything one style animates, built once per style, schema and canvas size: its laid-out
/// data, the timeline's length and steps, and the values the timeline moves.
pub struct Rig {
    pub duration: f32,
    pub steps: Vec<Step>,
    /// What hover lights.
    pub highlight: Highlight,
    /// What the timeline focuses.
    pub focus: Highlight,
    pub camera: Camera,
    tracks: Vec<Box<dyn Animate>>,
    data: Box<dyn Any>,
    published: Cell<u64>,
}

impl Rig {
    /// A rig over a style's own laid-out `data` (read back with [`Rig::data`]).
    pub fn new(data: impl Any, view: [f32; 2], duration: f32) -> Rig {
        Rig {
            duration: duration.max(0.01),
            steps: Vec::new(),
            highlight: Highlight::default(),
            focus: Highlight::default(),
            camera: Camera::new(view),
            tracks: Vec::new(),
            data: Box::new(data),
            published: Cell::new(0),
        }
    }

    pub fn step(mut self, at: f32, label: impl Into<String>) -> Rig {
        self.steps.push(Step { at, label: label.into() });
        self
    }

    /// Puts `a` on the timeline; [`Rig::publish`] moves it.
    pub fn add(&mut self, a: impl Animate + 'static) {
        self.tracks.push(Box::new(a));
    }

    pub fn data<T: 'static>(&self) -> Option<&T> {
        self.data.downcast_ref()
    }

    /// Sets every bound value for timeline time `t`.
    pub fn publish(&self, t: f32) {
        self.published.set(self.published.get() + 1);
        for a in &self.tracks {
            a.publish(t);
        }
        self.camera.publish(t);
    }

    /// How many times it has published, for tests that check a paused clock does no work.
    pub fn publishes(&self) -> u64 {
        self.published.get()
    }

    /// The step `t` is in: the last that starts at or before it.
    pub fn step_at(&self, t: f32) -> Option<usize> {
        self.steps.iter().rposition(|s| s.at <= t + 1e-3)
    }

    /// Just before the next step after `t` starts (or the end): what reduced motion shows, so a
    /// step lands whole and holds while the clock runs through it.
    pub fn held(&self, t: f32) -> f32 {
        let eps = 1e-3;
        self.steps
            .iter()
            .map(|s| s.at)
            .find(|&at| at > t + eps)
            .map_or(self.duration, |at| at - eps)
    }

    /// Step starts and the end, for [`Clock::step`].
    pub fn marks(&self) -> Vec<f32> {
        let mut m: Vec<f32> = self.steps.iter().map(|s| s.at).collect();
        m.push(self.duration);
        m
    }
}
