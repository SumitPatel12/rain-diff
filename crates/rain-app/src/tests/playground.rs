use rain_core::command::Command;
use rune::{Key, Mods, Runtime};

use super::*;
use crate::app::App;

fn on(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn lineage() -> usize {
    style("lineage")
}

#[test]
fn the_command_opens_the_page_and_escape_closes_it() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, tx| {
        s.run(Command::DiagramPlayground, tx);
    });
    settle(&mut rt);
    assert!(rt.state.playground.open);
    assert!(rt.find("playground").is_some());
    assert_eq!(rt.state.playground.lab.style().id(), "tour", "it opens on style 1, the tour");
    assert!(rt.find("tour").is_some());
    for n in 1..=rain_lab::registry().len() {
        assert!(rt.find(&format!("playground-style-{n}")).is_some(), "style {n} in the rail");
    }
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.playground.open);
    assert!(rt.find("playground").is_none());
}

#[test]
fn the_more_menu_and_the_palette_open_it() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| s.menus.more = true);
    settle(&mut rt);
    let at = rt
        .elements()
        .into_iter()
        .find(|e| !e.exiting && e.role.starts_with("menuitem") && e.label.as_deref() == Some("Diagram playground"))
        .map(|e| (e.rect[0] + e.rect[2] / 2.0, e.rect[1] + e.rect[3] / 2.0))
        .expect("the ⋯ menu lists it");
    rt.pointer_move(at.0, at.1);
    rt.pointer_down(at.0, at.1);
    rt.pointer_up(at.0, at.1);
    settle(&mut rt);
    assert!(rt.state.playground.open, "from the ⋯ menu");
    assert!(!rt.state.menus.more);
    on(&mut rt, "playground-close");
    assert!(!rt.state.playground.open);

    let rows = crate::palette::filtered(&rt.state, "diagram playground");
    assert_eq!(
        rows.first().map(|e| e.item.clone()),
        Some(crate::palette::Item::Cmd(Command::DiagramPlayground))
    );
    assert_eq!(rows[0].hint, "", "no key by default");
    rt.update(|s, tx| {
        s.open_palette();
        s.palette.edit.text = "diagram playground".into();
        crate::palette::run_selected(s, tx);
    });
    settle(&mut rt);
    assert!(rt.state.playground.open, "from the palette");
}

#[test]
fn the_keyboard_shortcuts_screen_lists_it_without_a_key() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| s.open_shortcuts());
    settle(&mut rt);
    assert!(rt.state.shortcuts.matches().contains(&Command::DiagramPlayground));
    rt.update(|s, _| s.shortcuts.search.text = "diagram".into());
    settle(&mut rt);
    assert!(rt.find("sc-diagram-playground").is_some());
    assert!(rt.find("sc-diagram-playground-key-0").is_none(), "no key in the profile");
}

#[test]
fn picking_a_style_switches_the_canvas() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| s.open_playground());
    settle(&mut rt);
    on(&mut rt, "playground-style-3");
    assert_eq!(rt.state.playground.lab.style().id(), "lineage");
    assert!(rt.find("icicle-frame-record").is_none());
    press(&mut rt, Key::Down, Mods::default());
    assert_eq!(rt.state.playground.lab.style().id(), "icicle", "Down goes to the next style");
    assert!(rt.find("icicle-frame-record").is_some());
    on(&mut rt, "playground-style-2");
    assert_eq!(rt.state.playground.lab.style().id(), "flowchart");
    assert_eq!(rt.state.playground.lab.schema(), rain_lab::SchemaId::S, "it plays the scene script");
    rt.update(|s, _| s.playground.lab.seek_frac(1.0));
    settle(&mut rt);
    let (x, y) = rt.find("flowchart-shape-account-service").unwrap();
    rt.pointer_move(x, y);
    settle(&mut rt);
    let lit = rt.state.playground.lab.rig().unwrap().highlight.current();
    assert!(lit.contains("account-service"), "a box lights itself: {lit:?}");
    assert!(rt.find("flowchart-conclusion").is_some());
    assert!(
        rt.find("flowchart-step-6-link-1").is_some(),
        "each beat is a card naming its shapes"
    );
    assert!(rt.find("playground-schema").is_none(), "one schema, no switch");
    on(&mut rt, "playground-style-1");
    assert!(rt.find("tour-map-station-voucher-service").is_some());
    assert!(rt.find("playground-schema").is_none(), "the tour reads the trace only");
}

#[test]
fn playing_moves_values_without_new_views_and_a_paused_page_does_no_work() {
    let mut a = app(story(), true);
    a.open_playground();
    a.pick_style(lineage());
    let mut rt = runtime(a);
    let rig = rt.state.playground.lab.rig().expect("built by the view");
    let published = rig.publishes();
    for k in 0..20 {
        rt.state.rest.clock = k as f32 * 0.016;
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(rig.publishes(), published, "paused: nothing published");

    rt.update(|s, _| {
        s.rest.clock = 1.0;
        s.playground.lab.play();
    });
    rt.frame(1.0 / 60.0);
    let commits = rt.stats.commits;
    let published = rig.publishes();
    for k in 1..=10 {
        rt.state.rest.clock = 1.0 + k as f32 * 0.016;
        rt.frame(1.0 / 60.0);
    }
    assert!(rig.publishes() >= published + 10, "each frame publishes");
    assert_eq!(rt.stats.commits, commits, "and rebuilds nothing");
    assert!(rt.state.playground.lab.clock.t() > 0.1);

    press(&mut rt, Key::Text(" ".into()), Mods::default());
    assert!(!rt.state.playground.lab.playing(), "Space pauses");
    let published = rig.publishes();
    for k in 0..10 {
        rt.state.rest.clock = 3.0 + k as f32;
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(rig.publishes(), published, "paused again: nothing");
}

/// Style `id` plays ten frames on its whole timeline's middle without a single new view.
fn plays_without_rebuilds(id: &str, key: &str) {
    let mut a = app(story(), true);
    a.open_playground();
    a.pick_style(rain_lab::registry().iter().position(|s| s.id() == id).unwrap());
    let mut rt = runtime(a);
    settle(&mut rt);
    assert!(rt.find(key).is_some(), "{id} draws {key}");
    let rig = rt.state.playground.lab.rig().expect("built by the view");
    rt.update(|s, _| {
        s.rest.clock = 1.0;
        s.playground.lab.seek_frac(0.3);
        s.playground.lab.play();
    });
    rt.frame(1.0 / 60.0);
    let (commits, published) = (rt.stats.commits, rig.publishes());
    for k in 1..=10 {
        rt.state.rest.clock = 1.0 + k as f32 * 0.016;
        rt.frame(1.0 / 60.0);
    }
    assert!(rig.publishes() >= published + 10, "{id}: each frame publishes");
    assert_eq!(rt.stats.commits, commits, "{id}: and rebuilds nothing");
}

#[test]
fn the_lineage_plays_without_rebuilds() {
    plays_without_rebuilds("lineage", "lineage-row-db:Vouchers.Amount");
}

#[test]
fn closing_the_page_pauses_it() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| {
        s.open_playground();
        s.playground.lab.play();
    });
    settle(&mut rt);
    assert!(rt.state.playground.lab.playing());
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.playground.open);
    assert!(!rt.state.playground.lab.playing());
}

#[test]
fn every_playground_scene_draws_its_style() {
    for n in 1..=rain_lab::registry().len() {
        let mut a = app(story(), true);
        shell_scene(&format!("playground-{n}"))(&mut a);
        let mut rt = runtime(a);
        settle(&mut rt);
        assert!(rt.state.playground.open, "{n}");
        assert_eq!(rt.state.playground.lab.style_index() + 1, n);
        assert!(rt.find("lab-canvas").is_some(), "{n}");
    }
}

#[test]
fn the_step_buttons_and_reduced_motion_land_on_steps() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| {
        s.open_playground();
        s.pick_style(lineage());
    });
    settle(&mut rt);
    rt.set_system_reduced_motion(true);
    settle(&mut rt);
    on(&mut rt, "playground-forward");
    assert_eq!(rt.state.playground.lab.step().map(|(i, _)| i), Some(1));
    on(&mut rt, "playground-play");
    assert!(
        rt.state.playground.lab.playing(),
        "reduced motion plays, it does not jump to the end"
    );
    rt.update(|s, _| s.playground.lab.seek_frac(1.0));
    settle(&mut rt);
    assert!(rt.state.playground.lab.clock.at_end());
    let last = rt.state.playground.lab.rig().unwrap().steps.len() - 1;
    on(&mut rt, "playground-back");
    assert_eq!(
        rt.state.playground.lab.step().map(|(i, _)| i),
        Some(last),
        "back from the end: the last step"
    );
    on(&mut rt, "playground-back");
    assert_eq!(rt.state.playground.lab.step().map(|(i, _)| i), Some(last - 1));
    assert!(rt.find("playground-step").is_some());
}

fn style(id: &str) -> usize {
    rain_lab::registry().iter().position(|s| s.id() == id).unwrap()
}

#[test]
fn the_flowchart_and_icicle_play_without_rebuilding_and_rest_when_paused() {
    for (id, key) in [("flowchart", "flowchart-shape-voucher-service"), ("icicle", "icicle-frame-record")] {
        let mut a = app(story(), true);
        a.open_playground();
        a.pick_style(style(id));
        let mut rt = runtime(a);
        assert!(rt.find(key).is_some(), "{id}: {key}");
        let rig = rt.state.playground.lab.rig().expect("built by the view");
        rt.update(|s, _| {
            s.rest.clock = 1.0;
            s.playground.lab.play();
        });
        rt.frame(1.0 / 60.0);
        let (commits, published) = (rt.stats.commits, rig.publishes());
        for k in 1..=10 {
            rt.state.rest.clock = 1.0 + k as f32 * 0.016;
            rt.frame(1.0 / 60.0);
        }
        assert!(rig.publishes() >= published + 10, "{id}: each frame publishes");
        assert_eq!(rt.stats.commits, commits, "{id}: and rebuilds nothing");
        rt.update(|s, _| s.playground.lab.pause());
        let published = rig.publishes();
        for k in 0..10 {
            rt.state.rest.clock = 3.0 + k as f32;
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(rig.publishes(), published, "{id}: paused, nothing");
    }
}

#[test]
fn pressing_an_icicle_frame_zooms_into_it_and_the_top_frame_zooms_out() {
    let mut a = app(story(), true);
    a.open_playground();
    a.pick_style(style("icicle"));
    let mut rt = runtime(a);
    rt.update(|s, _| s.playground.lab.seek_frac(1.0));
    settle(&mut rt);
    on(&mut rt, "icicle-frame-record");
    assert_eq!(rt.state.playground.lab.pick(), Some("record"));
    on(&mut rt, "icicle-frame-record");
    assert_eq!(rt.state.playground.lab.pick(), None, "pressing the zoomed frame zooms back out");
    on(&mut rt, "icicle-tab-1");
    assert_eq!(rt.state.playground.lab.pick(), Some("q"), "the query's call tree");
    assert!(rt.find("icicle-frame-q.total").is_some());
    on(&mut rt, "icicle-frame-q.total");
    assert_eq!(rt.state.playground.lab.pick(), Some("q.total"));
    on(&mut rt, "icicle-frame-q.total");
    assert_eq!(rt.state.playground.lab.pick(), Some("q"), "zooming out stays on the query");
    on(&mut rt, "icicle-tab-0");
    assert_eq!(rt.state.playground.lab.pick(), None);
}

#[test]
fn a_style_plays_ten_frames_without_rebuilding_and_rests_when_paused() {
    for (id, schema) in [
        ("tour", rain_lab::SchemaId::A),
        ("flowchart", rain_lab::SchemaId::S),
        ("lineage", rain_lab::SchemaId::B),
    ] {
        let mut a = app(story(), true);
        a.open_playground();
        a.pick_style(rain_lab::registry().iter().position(|s| s.id() == id).unwrap());
        a.playground.lab.set_schema(schema);
        let mut rt = runtime(a);
        settle(&mut rt);
        assert!(rt.find(id).is_some(), "{id} draws");
        let rig = rt.state.playground.lab.rig().expect("built by the view");
        rt.update(|s, _| {
            s.rest.clock = 1.0;
            s.playground.lab.play();
        });
        rt.frame(1.0 / 60.0);
        let (commits, published) = (rt.stats.commits, rig.publishes());
        for k in 1..=10 {
            rt.state.rest.clock = 1.0 + k as f32 * 0.016;
            rt.frame(1.0 / 60.0);
        }
        assert!(rig.publishes() >= published + 10, "{id} {schema:?}: each frame publishes");
        assert_eq!(rt.stats.commits, commits, "{id} {schema:?}: and rebuilds nothing");
        rt.update(|s, _| s.playground.lab.pause());
        let published = rig.publishes();
        for k in 0..5 {
            rt.state.rest.clock = 3.0 + k as f32;
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(rig.publishes(), published, "{id} {schema:?}: paused, nothing");
    }
}

#[test]
fn a_whole_play_rebuilds_only_at_the_end_and_a_paused_page_asks_for_no_frames() {
    for (id, pick) in [
        ("tour", None),
        ("flowchart", None),
        ("lineage", None),
        ("icicle", None),
        ("icicle", Some("q")),
    ] {
        let mut a = app(story(), true);
        a.settings.break_reminder.enabled = false;
        a.open_playground();
        a.pick_style(style(id));
        a.playground.lab.act(rain_lab::Act::Pick(pick.map(str::to_owned)));
        let mut rt = runtime(a);
        settle(&mut rt);
        rt.update(|s, _| {
            s.rest.clock = 1.0;
            s.playground.lab.play();
        });
        rt.frame(1.0 / 60.0);
        let commits = rt.stats.commits;
        let mut k = 0;
        while rt.state.playground.lab.playing() {
            k += 1;
            rt.state.rest.clock = 1.0 + k as f32 / 60.0;
            rt.frame(1.0 / 60.0);
        }
        assert_eq!(
            rt.stats.commits - commits,
            1,
            "{id} {pick:?}: steps change no view; the end does, once"
        );
        assert!(rt.state.playground.lab.clock.at_end());

        rt.update(|s, _| {
            s.playground.lab.seek_frac(0.5);
            s.playground.lab.pause();
        });
        let mut to_rest = 0;
        while rt.frame(1.0 / 60.0) {
            to_rest += 1;
            assert!(to_rest < 180, "{id} {pick:?}: paused, it comes to rest within 3 s");
        }
        for _ in 0..60 {
            assert!(!rt.frame(1.0 / 60.0), "{id} {pick:?}: at rest, no frame asks for another");
        }
        assert_eq!(
            rt.wake_deadline(),
            None,
            "{id} {pick:?}: and nothing wakes it (the break reminder is off)"
        );
    }
}

fn ms(t: std::time::Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

/// The `p` quantile (0 to 1) of `v`.
fn quantile(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(((v.len().max(1) - 1) as f64 * p).round() as usize).copied().unwrap_or(0.0)
}

/// Per style at 1440 × 900: the frames that open it and that lay it out again for a new size,
/// a frame while it plays and while it is paused, its allocations and its elements.
/// `cargo test -p rain-app --release playground_timings -- --ignored --nocapture`.
#[test]
#[ignore = "timings, run by hand in release"]
fn playground_timings() {
    use std::time::Instant;
    let cases: [(&str, &str, Option<&str>); 5] = [
        ("tour", "tour", None),
        ("flowchart", "flowchart", None),
        ("lineage", "lineage", None),
        ("icicle", "icicle", None),
        ("icicle query", "icicle", Some("q")),
    ];
    println!(
        "| style | open ms | resize ms | play p50 / p95 ms | paused p50 / p95 ms | allocs per frame playing / paused | rebuilds while playing | elements | paused: frames to rest, then frames asked | wake |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for (name, id, pick) in cases {
        let mut a = app(story(), true);
        a.settings.break_reminder.enabled = false;
        let mut rt = sized_runtime(a, (1440.0, 900.0));
        settle(&mut rt);
        let t = Instant::now();
        rt.update(|s, _| {
            s.open_playground();
            s.pick_style(style(id));
            if let Some(p) = pick {
                s.playground.lab.act(rain_lab::Act::Pick(Some(p.into())));
            }
        });
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        let open = ms(t);
        settle(&mut rt);
        let elements = rt.elements().iter().filter(|e| e.path.contains("lab-canvas")).count();

        let mut resize = Vec::new();
        for size in [(1280.0, 820.0), (1440.0, 900.0), (1360.0, 860.0), (1440.0, 900.0)] {
            rt.set_viewport(size, 1.0);
            let t = Instant::now();
            for _ in 0..3 {
                rt.frame(1.0 / 60.0);
            }
            resize.push(ms(t));
            settle(&mut rt);
        }

        rt.update(|s, _| {
            s.rest.clock = 1.0;
            s.playground.lab.seek_frac(0.0);
            s.playground.lab.play();
        });
        rt.frame(1.0 / 60.0);
        let frames = ((rt.state.playground.lab.clock.duration() * 60.0) as usize).min(1500);
        let commits = rt.stats.commits;
        let (mut play, mut play_allocs) = (Vec::new(), 0);
        for k in 1..=frames {
            rt.state.rest.clock = 1.0 + k as f32 / 60.0;
            let (t, n) = (Instant::now(), allocations());
            rt.frame(1.0 / 60.0);
            play.push(ms(t));
            play_allocs += allocations() - n;
        }
        let rebuilds = rt.stats.commits - commits;

        rt.update(|s, _| {
            s.playground.lab.seek_frac(0.5);
            s.playground.lab.pause();
        });
        let mut to_rest = 0;
        while rt.frame(1.0 / 60.0) && to_rest < 600 {
            to_rest += 1;
        }
        let rig = rt.state.playground.lab.rig().unwrap();
        let (published, commits) = (rig.publishes(), rt.stats.commits);
        let (mut idle, mut idle_allocs, mut wanted) = (Vec::new(), 0, 0);
        for k in 0..240 {
            rt.state.rest.clock = 100.0 + k as f32 / 60.0;
            let (t, n) = (Instant::now(), allocations());
            wanted += usize::from(rt.frame(1.0 / 60.0));
            idle.push(ms(t));
            idle_allocs += allocations() - n;
        }
        assert_eq!(rig.publishes(), published, "{name}: paused, nothing published");
        assert_eq!(rt.stats.commits, commits, "{name}: paused, nothing rebuilt");
        println!(
            "| {name} | {open:.1} | {:.1} | {:.2} / {:.2} | {:.3} / {:.3} | {:.0} / {:.0} | {rebuilds} in {frames} frames | {elements} | {to_rest} frames, then {wanted} of 240 | {:?} |",
            quantile(&mut resize, 0.5),
            quantile(&mut play.clone(), 0.5),
            quantile(&mut play, 0.95),
            quantile(&mut idle.clone(), 0.5),
            quantile(&mut idle, 0.95),
            play_allocs as f64 / frames as f64,
            idle_allocs as f64 / 240.0,
            rt.wake_deadline(),
        );
    }
    let mut rt = sized_runtime(app(story(), true), (1440.0, 900.0));
    settle(&mut rt);
    let mut idle = Vec::new();
    for _ in 0..240 {
        let t = Instant::now();
        rt.frame(1.0 / 60.0);
        idle.push(ms(t));
    }
    println!(
        "| (diff screen, no playground) | | | | {:.3} / {:.3} | | | | |",
        quantile(&mut idle.clone(), 0.5),
        quantile(&mut idle, 0.95)
    );
}
