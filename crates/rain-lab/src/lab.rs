//! The playground's state: which style and schema are on, the timeline clock, the style's
//! cached [`Rig`], and the scrubber's bound values. The host app keeps one, draws
//! [`Lab::canvas`] and calls [`Lab::tick`] from its poll while [`Lab::playing`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use rain_diagram::DiagramStyle;
use rune::{El, Live, LiveText};

use crate::motion::{Clock, Rig, Tick};
use crate::schema::{Samples, SchemaId, samples};
use crate::style::{Act, Cx, Style, registry};

/// The scrubber's track width, in points.
pub const SCRUB_W: f32 = 240.0;
/// The speeds offered.
pub const SPEEDS: [f32; 3] = [0.5, 1.0, 2.0];

/// The scrubber's values, moved with the timeline without a rebuild.
#[derive(Clone)]
pub struct Scrub {
    /// The filled part of the track.
    pub fill: Live<[f32; 4]>,
    /// The thumb's offset along the track.
    pub thumb: Live<[f32; 2]>,
    /// `1.2 s / 4.0 s`.
    pub time: LiveText,
    /// `Step 2 of 5 · Checked, then stored`.
    pub step: LiveText,
}

impl Default for Scrub {
    fn default() -> Scrub {
        Scrub {
            fill: Live::new([0.0, 0.0, 0.0, 4.0]),
            thumb: Live::new([0.0, 0.0]),
            time: LiveText::new("0.0 s / 0.0 s"),
            step: LiveText::new(""),
        }
    }
}

#[derive(Clone, PartialEq)]
struct Key {
    style: usize,
    schema: SchemaId,
    pick: Option<String>,
    size: [i32; 2],
    ink: DiagramStyle,
}

pub struct Lab {
    /// The documents drawn; `None` is the built-in samples.
    data: Option<Rc<Samples>>,
    style: usize,
    schema: SchemaId,
    pub clock: Clock,
    /// The sample's raw JSON is shown.
    pub raw: bool,
    pick: Option<String>,
    /// The OS asks for reduced motion, as the last view saw it.
    pub reduced: Cell<bool>,
    pub scrub: Scrub,
    rig: RefCell<Option<(Key, Rc<Rig>)>>,
    /// The step the timeline was last in.
    step_seen: Cell<Option<usize>>,
    /// What the scrubber's texts last said, in tenths of a second and a step, so a frame that
    /// changes neither formats nothing.
    shown: Cell<Option<(i32, Option<usize>)>>,
}

impl Default for Lab {
    fn default() -> Lab {
        Lab::new()
    }
}

impl Lab {
    /// Opens on the first style that is built.
    pub fn new() -> Lab {
        let first = registry().iter().position(|s| s.ready()).unwrap_or(0);
        Lab::build(None, first)
    }

    /// Draws `data` (a review's diagrams) instead of the samples, opening on style `first`.
    pub fn over(data: Samples, first: usize) -> Lab {
        Lab::build(Some(Rc::new(data)), first)
    }

    fn build(data: Option<Rc<Samples>>, first: usize) -> Lab {
        let mut lab = Lab {
            data,
            style: 0,
            schema: SchemaId::A,
            clock: Clock::default(),
            raw: false,
            pick: None,
            reduced: Cell::new(false),
            scrub: Scrub::default(),
            rig: RefCell::new(None),
            step_seen: Cell::new(None),
            shown: Cell::new(None),
        };
        lab.select(first);
        lab
    }

    pub fn style_index(&self) -> usize {
        self.style
    }

    pub fn style(&self) -> &'static dyn Style {
        registry()[self.style]
    }

    pub fn schema(&self) -> SchemaId {
        self.schema
    }

    pub fn pick(&self) -> Option<&str> {
        self.pick.as_deref()
    }

    /// Puts style `i` on, on its first schema, at the start of its timeline, paused.
    pub fn select(&mut self, i: usize) {
        let i = i.min(registry().len() - 1);
        self.style = i;
        self.schema = registry()[i].schemas().first().copied().unwrap_or(SchemaId::A);
        self.pick = None;
        self.rewind();
    }

    /// Reads the style from another of its schemas, from the start.
    pub fn set_schema(&mut self, s: SchemaId) {
        if self.style().schemas().contains(&s) && s != self.schema {
            self.schema = s;
            self.rewind();
        }
    }

    fn rewind(&mut self) {
        let speed = self.clock.speed();
        self.clock = Clock::default();
        self.clock.set_speed(speed);
        self.step_seen.set(None);
        self.publish();
    }

    /// The rig for `cx`, built when the style, schema, pick, size or colours changed.
    fn rig_for(&self, cx: &Cx) -> Rc<Rig> {
        let key = Key {
            style: self.style,
            schema: self.schema,
            pick: self.pick.clone(),
            size: [cx.size[0].round() as i32, cx.size[1].round() as i32],
            ink: *cx.ink,
        };
        if let Some((k, rig)) = &*self.rig.borrow()
            && *k == key
        {
            return rig.clone();
        }
        let rig = Rc::new(self.style().rig(cx));
        *self.rig.borrow_mut() = Some((key, rig.clone()));
        self.shown.set(None);
        self.publish_with(&rig);
        rig
    }

    /// The rig last built, if any.
    pub fn rig(&self) -> Option<Rc<Rig>> {
        self.rig.borrow().as_ref().map(|(_, r)| r.clone())
    }

    /// The style drawn for a canvas `size` big; `act` makes an element send an [`Act`].
    pub fn canvas(&self, kit: &rune_kit::Kit, ink: &DiagramStyle, size: [f32; 2], act: &dyn Fn(El, Act) -> El) -> El {
        let cx = Cx {
            samples: self.data.as_deref().unwrap_or(samples()),
            schema: self.schema,
            kit,
            ink,
            size,
            reduced: self.reduced.get(),
            pick: self.pick.as_deref(),
            act,
        };
        let rig = self.rig_for(&cx);
        self.style().view(&cx, &rig)
    }

    fn duration(&self) -> f32 {
        self.rig().map_or(self.clock.duration(), |r| r.duration)
    }

    /// Keeps the clock's length to the rig's.
    fn sync(&mut self) {
        let d = self.duration();
        if (d - self.clock.duration()).abs() > 1e-4 {
            self.clock.set_duration(d);
        }
    }

    fn publish_with(&self, rig: &Rig) {
        let t = self.clock.t();
        rig.publish(t);
        let p = (t / rig.duration).clamp(0.0, 1.0);
        self.scrub.fill.set([0.0, 0.0, SCRUB_W * p, 4.0]);
        self.scrub.thumb.set([SCRUB_W * p, 0.0]);
        let (tenths, step) = ((t * 10.0).round() as i32, rig.step_at(t));
        let was = self.shown.replace(Some((tenths, step)));
        if was.map(|w| w.0) != Some(tenths) {
            self.scrub
                .time
                .set(format!("{:.1} s / {:.1} s", tenths as f32 / 10.0, rig.duration));
        }
        if was.map(|w| w.1) != Some(step) {
            self.scrub.step.set(match step {
                Some(i) => format!("Step {} of {} · {}", i + 1, rig.steps.len(), rig.steps[i].label),
                None => String::new(),
            });
        }
    }

    /// Sets every bound value for the clock's time.
    pub fn publish(&self) {
        if let Some(rig) = self.rig() {
            self.publish_with(&rig);
        }
    }

    pub fn playing(&self) -> bool {
        self.clock.playing()
    }

    /// The step the timeline is in, and its label.
    pub fn step(&self) -> Option<(usize, String)> {
        let rig = self.rig()?;
        let i = rig.step_at(self.clock.t())?;
        Some((i, rig.steps[i].label.clone()))
    }

    /// Moves the timeline to the host's frame clock `host` (seconds) and publishes it, without
    /// allocating unless a scrubber text changes. Paused, it does nothing at all.
    pub fn tick(&mut self, host: f32) -> Tick {
        if !self.clock.playing() {
            return Tick::Idle;
        }
        self.sync();
        let tick = self.clock.tick(host);
        self.publish();
        let step = self.rig().and_then(|r| r.step_at(self.clock.t()));
        let before = self.step_seen.replace(step);
        if tick == Tick::Moved && before.is_some() && before != step {
            return Tick::Stepped;
        }
        tick
    }

    pub fn toggle(&mut self) {
        self.sync();
        self.clock.toggle(self.reduced.get());
        self.publish();
    }

    pub fn play(&mut self) {
        self.sync();
        self.clock.play(self.reduced.get());
        self.publish();
    }

    pub fn pause(&mut self) {
        self.clock.pause();
    }

    pub fn restart(&mut self) {
        self.sync();
        self.clock.restart(self.reduced.get());
        self.publish();
    }

    /// Forward plays to the next step and stops; back jumps to the step before.
    pub fn step_by(&mut self, forward: bool) {
        self.sync();
        let marks = self.rig().map(|r| r.marks()).unwrap_or_default();
        self.clock.step(&marks, forward, self.reduced.get());
        self.publish();
    }

    /// Jumps to `frac` (0 to 1) of the timeline.
    pub fn seek_frac(&mut self, frac: f32) {
        self.sync();
        self.clock.seek(frac.clamp(0.0, 1.0) * self.clock.duration());
        self.publish();
    }

    pub fn set_speed(&mut self, speed: f32) {
        self.clock.set_speed(speed);
    }

    pub fn act(&mut self, a: Act) {
        match a {
            Act::Pick(p) => self.pick = p,
            Act::Seek(t) => {
                self.sync();
                self.clock.seek(t);
                self.publish();
            }
            Act::Step(d) => self.step_by(d > 0),
        }
    }
}
