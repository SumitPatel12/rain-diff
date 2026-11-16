//! The break reminder: active time counts only with recent input, a long quiet resets it, the
//! rain starts at the setting, holds the keys, and Enter / Esc / the buttons drain it.

use std::time::{Duration, Instant};

use super::*;
use crate::app::rest::{ATTENTIVE, IDLE_RESET, UNWATCHED, Weather};

const STEP: Duration = Duration::from_secs(15);

/// Ticks `a` every 15 s from `start` for `span`, with input `quiet` before each tick.
fn run(a: &mut App, start: Instant, span: Duration, quiet: Duration) -> Instant {
    let mut now = start;
    a.rest_tick_at(now, Some(now));
    while now < start + span {
        now += STEP;
        a.rest_tick_at(now, now.checked_sub(quiet));
    }
    now
}

fn reminder(minutes: f64) -> App {
    let mut a = app(story(), false);
    a.settings.break_reminder.minutes = minutes;
    a
}

#[test]
fn the_rain_holds_still_while_nobody_watches() {
    let mut a = reminder(90.0);
    let t = Instant::now();
    a.rest.weather = Weather::Flood(t);
    assert!(!a.rain_unwatched(t + UNWATCHED - STEP, None));
    assert!(a.rain_unwatched(t + UNWATCHED, None), "a minute with no input");
    assert!(
        a.rain_unwatched(t + UNWATCHED, Some(t - STEP)),
        "input before the rain doesn't count"
    );
    assert!(!a.rain_unwatched(t + UNWATCHED, Some(t + STEP)), "input starts the wait over");
    a.rest.weather = Weather::Drain(t, 0);
    assert!(!a.rain_unwatched(t + UNWATCHED * 2, None), "the drain always plays");
}

#[test]
fn active_use_floods_at_the_setting() {
    let mut a = reminder(2.0);
    let now = run(&mut a, Instant::now(), Duration::from_secs(105), Duration::ZERO);
    assert_eq!(a.rest.weather, Weather::Clear, "not yet at 1:45");
    run(&mut a, now, Duration::from_secs(30), Duration::ZERO);
    assert!(a.raining(), "raining at 2:00: {:?}", a.rest);
}

#[test]
fn reading_counts_but_a_long_quiet_is_a_break() {
    let mut a = reminder(90.0);
    let t = run(&mut a, Instant::now(), Duration::from_secs(600), ATTENTIVE / 2);
    assert!(
        a.rest.active >= Duration::from_secs(590),
        "reading still counts: {:?}",
        a.rest.active
    );
    run(&mut a, t, Duration::from_secs(60), IDLE_RESET);
    assert_eq!(a.rest.active, Duration::ZERO, "ten quiet minutes start the count over");
}

#[test]
fn input_older_than_attentive_pauses_the_count() {
    let mut a = reminder(90.0);
    run(&mut a, Instant::now(), Duration::from_secs(300), ATTENTIVE + STEP);
    assert_eq!(a.rest.active, Duration::ZERO);
}

#[test]
fn disabled_never_rains() {
    let mut a = reminder(0.5);
    a.settings.break_reminder.enabled = false;
    run(&mut a, Instant::now(), Duration::from_secs(120), Duration::ZERO);
    assert_eq!(a.rest.weather, Weather::Clear);
}

#[test]
fn snooze_comes_back_after_the_snooze_and_a_break_starts_over() {
    let mut a = reminder(2.0);
    a.settings.break_reminder.snooze_minutes = 1.0;
    let t = run(&mut a, Instant::now(), Duration::from_secs(135), Duration::ZERO);
    assert!(a.raining());
    a.rest_snooze();
    assert!(matches!(a.rest.weather, Weather::Drain(..)));
    assert_eq!(a.rest.active, Duration::from_secs(60));
    a.rest_tick_at(Instant::now() + Duration::from_secs(2), Some(t));
    assert_eq!(a.rest.weather, Weather::Clear, "drained");
    a.rest_taken();
    assert_eq!(a.rest.active, Duration::ZERO);
}

#[test]
fn the_rain_shows_a_card_and_holds_the_keys() {
    let mut a = reminder(90.0);
    a.rest.weather = Weather::Flood(Instant::now().checked_sub(Duration::from_secs(12)).unwrap());
    let mut rt = runtime(a);
    assert!(rt.find("flood").is_some());
    assert!(rt.find("break-card").is_some());
    assert!(rt.find("break-snooze").is_some());
    press(&mut rt, Key::Text("j".into()), Mods::default());
    assert!(rt.state.raining(), "a stray key does not end the rain");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.raining(), "Esc snoozes");
    assert!(rt.state.rest.active > Duration::ZERO, "a snooze keeps most of the count");
}

#[test]
fn the_buttons_drain_it() {
    let mut a = reminder(90.0);
    a.rest.weather = Weather::Flood(Instant::now().checked_sub(Duration::from_secs(12)).unwrap());
    let mut rt = runtime(a);
    let (x, y) = rt.find("break-taken").expect("the button");
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(!rt.state.raining());
    assert_eq!(rt.state.rest.active, Duration::ZERO);
}

#[test]
fn the_preview_rains_and_leaves_the_count_alone() {
    let mut a = reminder(90.0);
    a.rest.active = Duration::from_secs(1200);
    let mut rt = runtime(a);
    rt.update(|s, _| s.rest_preview());
    settle(&mut rt);
    assert!(rt.state.raining());
    press(&mut rt, Key::Enter, Mods::default());
    assert!(!rt.state.raining());
    assert_eq!(rt.state.rest.active, Duration::from_secs(1200), "a preview is not a break");
}

#[test]
fn settings_read_the_reminder_and_default_it() {
    let s = crate::settings::Settings::parse(br#"{"breakReminder": {"minutes": 45}}"#).unwrap();
    assert_eq!(s.break_reminder.minutes, 45.0);
    assert!(s.break_reminder.enabled);
    assert_eq!(s.break_reminder.snooze_minutes, 10.0);
    assert_eq!(crate::settings::Settings::default().break_reminder.minutes, 90.0);
}
