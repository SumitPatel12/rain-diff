//! The break reminder: minutes of active use, counted from input, then rain until a break.

use std::time::{Duration, Instant};

use rune::Tx;

use super::App;

/// Input this recent makes the time since the last check count as active (reading counts).
pub(crate) const ATTENTIVE: Duration = Duration::from_secs(5 * 60);
/// No input this long counts as a break: the count starts over.
pub(crate) const IDLE_RESET: Duration = Duration::from_secs(10 * 60);
/// How often the count is checked while it is clear; one cheap poll each.
const CHECK_EVERY: Duration = Duration::from_secs(15);
/// A longer gap between checks (the machine slept) counts this much at most.
const MAX_STEP: Duration = Duration::from_secs(60);
/// The clouds roll in and the water rises over this long.
pub(crate) const FLOOD_IN: Duration = Duration::from_secs(9);
/// The water drains away over this long.
pub(crate) const DRAIN: Duration = Duration::from_millis(1400);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weather {
    Clear,
    /// Raining since then.
    Flood(Instant),
    /// Draining since then, from the level it had reached.
    Drain(Instant, u16),
}

#[derive(Clone, Debug)]
pub struct Rest {
    pub active: Duration,
    /// The frame clock in seconds, which every weather animation is drawn from.
    pub clock: f32,
    /// The clock the weather was last drawn at.
    pub drawn: f32,
    /// The count to put back when a preview's rain ends.
    pub preview: Option<Duration>,
    pub checked: Option<Instant>,
    pub weather: Weather,
}

impl Default for Rest {
    fn default() -> Rest {
        Rest {
            active: Duration::ZERO,
            clock: 0.0,
            drawn: -1.0,
            preview: None,
            checked: None,
            weather: Weather::Clear,
        }
    }
}

/// How far the water has risen, 0 to 1, `t` after the rain began.
pub(crate) fn flood_level(t: Duration) -> f32 {
    let rise = (t.as_secs_f32() - 2.0) / (FLOOD_IN.as_secs_f32() - 2.0);
    let x = rise.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

impl App {
    fn break_after(&self) -> Duration {
        Duration::from_secs_f64(self.settings.break_reminder.minutes.max(0.05) * 60.0)
    }

    pub fn raining(&self) -> bool {
        matches!(self.rest.weather, Weather::Flood(_))
    }

    pub(crate) fn rest_poll(&mut self, tx: &mut Tx) -> bool {
        // The loader's clouds rain from the clock too.
        let loading = matches!(self.phase, super::Phase::Loading(_));
        if loading {
            tx.wake_after(Duration::ZERO);
        }
        let changed = self.rest_tick_at(Instant::now(), rune::last_input());
        match self.rest.weather {
            Weather::Clear if self.settings.break_reminder.enabled => tx.wake_after(CHECK_EVERY),
            Weather::Clear => {}
            // The rain is drawn from the clock: one frame after another until it is clear.
            _ => tx.wake_after(Duration::ZERO),
        }
        // Redraw only when the clock has moved since the last drawing.
        let animating = loading || self.rest.weather != Weather::Clear;
        let moved = animating && self.rest.clock != self.rest.drawn;
        self.rest.drawn = self.rest.clock;
        changed || moved
    }

    /// Steps the count to `now`; true when the weather changed.
    pub(crate) fn rest_tick_at(&mut self, now: Instant, last_input: Option<Instant>) -> bool {
        match self.rest.weather {
            Weather::Drain(at, _) if now.saturating_duration_since(at) >= DRAIN => {
                self.rest.weather = Weather::Clear;
                self.rest.checked = Some(now);
                true
            }
            Weather::Clear => {
                let prev = self.rest.checked.replace(now);
                if !self.settings.break_reminder.enabled {
                    self.rest.active = Duration::ZERO;
                    return false;
                }
                if let (Some(prev), Some(input)) = (prev, last_input) {
                    let quiet = now.saturating_duration_since(input);
                    if quiet >= IDLE_RESET {
                        self.rest.active = Duration::ZERO;
                    } else if quiet < ATTENTIVE {
                        self.rest.active += now.saturating_duration_since(prev).min(MAX_STEP);
                    }
                }
                if self.rest.active >= self.break_after() {
                    self.rest.weather = Weather::Flood(now);
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    /// Rains now to show what the reminder looks like; ending it leaves the count as it was.
    pub fn rest_preview(&mut self) {
        if self.rest.weather == Weather::Clear {
            self.rest.preview = Some(self.rest.active);
            self.rest.weather = Weather::Flood(Instant::now());
        }
    }

    /// "I took a break": the water drains and the count starts over.
    pub fn rest_taken(&mut self) {
        self.rest.active = Duration::ZERO;
        self.drain();
    }

    /// "Snooze": the water drains and comes back after the snooze.
    pub fn rest_snooze(&mut self) {
        let snooze = Duration::from_secs_f64(self.settings.break_reminder.snooze_minutes.max(0.0) * 60.0);
        self.rest.active = self.break_after().saturating_sub(snooze);
        self.drain();
    }

    fn drain(&mut self) {
        if let Some(active) = self.rest.preview.take() {
            self.rest.active = active;
        }
        let now = Instant::now();
        let level = match self.rest.weather {
            Weather::Flood(at) => flood_level(now.saturating_duration_since(at)),
            _ => 0.0,
        };
        self.rest.weather = Weather::Drain(now, (level * 1000.0) as u16);
        self.rest.checked = Some(now);
    }
}
