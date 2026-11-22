//! When a working-tree or index diff is checked for edits made under the open window: one
//! thread per such repo, asleep unless the window has the focus, checking at once when the
//! focus returns and then less and less often while nothing changes.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// The first wait, and the wait after a check that found a change.
pub const MIN_WAIT: Duration = Duration::from_secs(2);
/// Each check that finds nothing new doubles the wait up to this.
pub const MAX_WAIT: Duration = Duration::from_secs(30);
/// A check is followed by at least this many times its own duration.
pub const COST_FACTOR: u32 = 20;

/// When one repo is next checked.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Pace {
    wait: Duration,
    due: Option<Instant>,
    refocus: u64,
}

impl Pace {
    pub(crate) fn new(now: Instant) -> Pace {
        Pace {
            wait: MIN_WAIT,
            due: Some(now + MIN_WAIT),
            refocus: 0,
        }
    }

    /// Without the focus nothing is due; a check is due at once when it returns.
    pub(crate) fn focus(&mut self, focused: bool, refocus: u64, now: Instant) {
        if !focused {
            self.due = None;
        } else if refocus != self.refocus || self.due.is_none() {
            self.due = Some(now);
            self.wait = MIN_WAIT;
        }
        self.refocus = refocus;
    }

    /// How long until the next check; `None` until the focus returns.
    pub(crate) fn sleep(&self, now: Instant) -> Option<Duration> {
        self.due.map(|d| d.saturating_duration_since(now))
    }

    pub(crate) fn ran(&mut self, now: Instant, cost: Duration, changed: bool) {
        let wait = if changed { MIN_WAIT } else { (self.wait * 2).min(MAX_WAIT) };
        self.wait = wait.max(cost * COST_FACTOR);
        self.due = Some(now + self.wait);
    }
}

/// One repo's checks; its thread and the tests both step it with [`Poller::tick`].
pub(crate) struct Poller {
    pub(crate) dir: PathBuf,
    pace: Pace,
}

impl Poller {
    pub(crate) fn new(dir: PathBuf, now: Instant) -> Poller {
        Poller { dir, pace: Pace::new(now) }
    }

    /// Runs `check` when one is due at `now`; `Some(changed)` when it ran.
    pub(crate) fn tick(&mut self, now: Instant, focus: (bool, u64), check: impl FnOnce(&Path) -> bool) -> Option<bool> {
        self.pace.focus(focus.0, focus.1, now);
        if !self.pace.sleep(now)?.is_zero() {
            return None;
        }
        let started = Instant::now();
        let changed = check(&self.dir);
        self.pace.ran(now, started.elapsed(), changed);
        Some(changed)
    }
}

#[derive(Default)]
struct GateState {
    blurred: bool,
    refocus: u64,
    stop: bool,
}

/// The window's focus as the pollers see it; dropping it ends their threads.
#[derive(Default)]
pub(crate) struct Gate(Arc<(Mutex<GateState>, Condvar)>);

impl Gate {
    fn state(inner: &(Mutex<GateState>, Condvar)) -> MutexGuard<'_, GateState> {
        inner.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn set_focus(&self, focused: bool) {
        let mut g = Gate::state(&self.0);
        if g.blurred != focused {
            return;
        }
        g.blurred = !focused;
        if focused {
            g.refocus += 1;
        }
        drop(g);
        self.0.1.notify_all();
    }

    /// Whether the window has the focus, and how many times it has come back.
    #[cfg(test)]
    pub(crate) fn focus(&self) -> (bool, u64) {
        let g = Gate::state(&self.0);
        (!g.blurred, g.refocus)
    }

    /// Steps `poller` on a thread until the gate is dropped, calling `changed` after a check
    /// that found a change.
    pub(crate) fn spawn(&self, mut poller: Poller, check: impl Fn(&Path) -> bool + Send + 'static, changed: impl Fn() + Send + 'static) {
        let inner = self.0.clone();
        let _ = std::thread::Builder::new().name("rain-edit-watch".into()).spawn(move || {
            loop {
                let seen = {
                    let g = Gate::state(&inner);
                    if g.stop {
                        return;
                    }
                    (!g.blurred, g.refocus)
                };
                if poller.tick(Instant::now(), seen, &check) == Some(true) {
                    changed();
                }
                let g = Gate::state(&inner);
                if g.stop {
                    return;
                }
                if (!g.blurred, g.refocus) != seen {
                    continue;
                }
                match poller.pace.sleep(Instant::now()) {
                    Some(d) => drop(inner.1.wait_timeout(g, d)),
                    None => drop(inner.1.wait(g)),
                }
            }
        });
    }
}

impl Drop for Gate {
    fn drop(&mut self) {
        Gate::state(&self.0).stop = true;
        self.0.1.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEC: Duration = Duration::from_secs(1);

    /// Steps a pace second by second for `secs`, checking whenever one is due; the seconds at
    /// which it checked.
    fn checks(p: &mut Pace, t0: Instant, from: u64, secs: u64, cost: Duration) -> Vec<u64> {
        (from..from + secs)
            .filter(|&s| {
                let now = t0 + SEC * s as u32;
                let due = p.sleep(now).is_some_and(|d| d.is_zero());
                if due {
                    p.ran(now, cost, false);
                }
                due
            })
            .collect()
    }

    #[test]
    fn an_idle_repo_is_checked_twice_a_minute_at_rest() {
        let t0 = Instant::now();
        let mut p = Pace::new(t0);
        let at = checks(&mut p, t0, 0, 600, Duration::from_millis(40));
        assert_eq!(&at[..6], &[2, 6, 14, 30, 60, 90], "the wait doubles to the cap");
        let last_minute = at.iter().filter(|&&s| s >= 540).count();
        assert_eq!(last_minute, 2);
    }

    #[test]
    fn a_slow_check_waits_twenty_times_its_own_cost() {
        let t0 = Instant::now();
        let mut p = Pace::new(t0);
        let at = checks(&mut p, t0, 0, 600, Duration::from_secs(3));
        assert_eq!(at, [2, 62, 122, 182, 242, 302, 362, 422, 482, 542]);
    }

    #[test]
    fn a_change_brings_the_next_check_close_again() {
        let t0 = Instant::now();
        let mut p = Pace::new(t0);
        checks(&mut p, t0, 0, 300, Duration::ZERO);
        let now = t0 + SEC * 300;
        p.ran(now, Duration::ZERO, true);
        assert_eq!(p.sleep(now), Some(MIN_WAIT));
    }

    #[test]
    fn nothing_is_due_without_the_focus_and_one_check_is_due_when_it_returns() {
        let t0 = Instant::now();
        let mut p = Pace::new(t0);
        p.focus(false, 0, t0);
        assert_eq!(p.sleep(t0 + SEC * 3600), None);
        assert!(checks(&mut p, t0, 0, 3600, Duration::ZERO).is_empty());
        let back = t0 + SEC * 3600;
        p.focus(true, 1, back);
        assert_eq!(p.sleep(back), Some(Duration::ZERO));
        p.ran(back, Duration::ZERO, false);
        p.focus(true, 1, back);
        assert_eq!(p.sleep(back), Some(MIN_WAIT * 2), "once, then the usual waits");
    }

    #[test]
    fn a_blur_and_refocus_between_two_steps_still_checks_at_once() {
        let t0 = Instant::now();
        let mut p = Pace::new(t0);
        p.focus(true, 0, t0);
        assert_eq!(p.sleep(t0), Some(MIN_WAIT));
        p.focus(true, 1, t0);
        assert_eq!(p.sleep(t0), Some(Duration::ZERO));
    }

    #[test]
    fn the_gate_counts_each_return_of_the_focus_once() {
        let g = Gate::default();
        assert_eq!(g.focus(), (true, 0));
        g.set_focus(true);
        assert_eq!(g.focus(), (true, 0), "already focused");
        g.set_focus(false);
        g.set_focus(false);
        assert_eq!(g.focus(), (false, 0));
        g.set_focus(true);
        assert_eq!(g.focus(), (true, 1));
    }
}
