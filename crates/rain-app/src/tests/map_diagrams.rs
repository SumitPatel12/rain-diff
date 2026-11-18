//! The Map's animated diagrams (an overview's `diagrams` block) and its node pane.

use rain_core::Launch;
use rune::{Key, Mods, Runtime};

use super::*;
use crate::app::{App, MapTab, Overlay};

fn diagrams_overview() -> std::path::PathBuf {
    workspace("fixtures/overview/sample-overview-diagrams.json")
}

fn diagram_app() -> App {
    let mut a = app_with(Launch {
        session: "t".into(),
        fixture: Some(story()),
        overview: Some(diagrams_overview()),
        ..Launch::default()
    });
    a.settings.break_reminder.enabled = false;
    a
}

fn style(id: &str) -> usize {
    rain_lab::registry().iter().position(|s| s.id() == id).unwrap()
}

fn on(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key}"));
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// Frames until the page asks for none, then 60 more that must ask for none and set no wake.
fn comes_to_rest(rt: &mut Runtime<App>, what: &str) {
    let mut n = 0;
    while rt.frame(1.0 / 60.0) {
        n += 1;
        assert!(n < 180, "{what}: comes to rest within 3 s");
    }
    for _ in 0..60 {
        assert!(!rt.frame(1.0 / 60.0), "{what}: at rest, no frame asks for another");
    }
    assert_eq!(rt.wake_deadline(), None, "{what}: and nothing wakes it");
}

#[test]
fn a_review_with_diagrams_opens_the_map_on_them_first_and_keeps_the_old_views_after() {
    let mut rt = runtime(diagram_app());
    let tabs = rt.state.map_tabs();
    let labs: Vec<MapTab> = ["tour", "flowchart", "lineage", "icicle"]
        .into_iter()
        .map(|id| MapTab::Lab(style(id)))
        .collect();
    assert_eq!(tabs[..4], labs[..], "the four diagrams first, in the registry's order");
    assert!(tabs[4..].iter().all(|t| matches!(t, MapTab::View(_))) && tabs.len() > 5, "{tabs:?}");
    assert!(
        rt.state.diagram.lab.as_ref().unwrap().rig().is_none(),
        "nothing is laid out before the Map shows"
    );

    typed(&mut rt, "m");
    assert!(rt.state.layout.is_open(Overlay::Map));
    assert_eq!(rt.state.map_tab(), MapTab::Lab(style("tour")), "the first diagram is selected");
    assert!(rt.find("map-lab-canvas").is_some());
    assert!(rt.find("tour").is_some(), "the tour draws");
    assert!(rt.find("map-lab-play").is_some() && rt.find("map-lab-scrub").is_some());
    assert!(rt.find("map-pane").is_none() && rt.find("map-legend").is_none());

    for (n, id, key) in [(1, "flowchart", "flowchart"), (2, "lineage", "lineage"), (3, "icicle", "icicle")] {
        on(&mut rt, &format!("map-tabs-{n}"));
        assert_eq!(rt.state.map_tab(), MapTab::Lab(style(id)));
        assert!(rt.find(key).is_some(), "{id} draws");
    }
    on(&mut rt, "map-tabs-4");
    assert!(matches!(rt.state.map_tab(), MapTab::View(_)), "the old views follow");
    assert!(rt.find("map-legend").is_some() && rt.find("map-lab-canvas").is_none());
    typed(&mut rt, "{");
    assert_eq!(rt.state.map_tab(), MapTab::Lab(style("icicle")), "braces cross into the diagrams");

    let plain = runtime(app(story(), true));
    assert!(plain.state.map_tabs().iter().all(|t| matches!(t, MapTab::View(_))));
    assert!(!plain.state.diagram.lab_on);
}

#[test]
fn a_diagram_plays_on_the_map_without_rebuilds_and_idles_at_zero_work() {
    let mut rt = runtime(diagram_app());
    typed(&mut rt, "m");
    comes_to_rest(&mut rt, "paused on the Map");
    let rig = rt.state.diagram.lab.as_ref().unwrap().rig().expect("built when the tab showed");
    let published = rig.publishes();
    for _ in 0..30 {
        rt.frame(1.0 / 60.0);
    }
    assert_eq!(rig.publishes(), published, "paused: nothing published");

    rt.update(|s, _| s.rest.clock = 1.0);
    press(&mut rt, Key::Text(" ".into()), Mods::default());
    assert!(rt.state.diagram.lab.as_ref().unwrap().playing(), "Space plays");
    rt.frame(1.0 / 60.0);
    let (commits, published) = (rt.stats.commits, rig.publishes());
    for k in 1..=10 {
        rt.state.rest.clock = 1.0 + k as f32 * 0.016;
        rt.frame(1.0 / 60.0);
    }
    assert!(rig.publishes() >= published + 10, "each frame publishes");
    assert_eq!(rt.stats.commits, commits, "and rebuilds nothing");

    press(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.layout.is_open(Overlay::Map), "Escape leaves the Map");
    assert!(!rt.state.diagram.lab.as_ref().unwrap().playing(), "leaving it pauses the diagram");
    let published = rig.publishes();
    comes_to_rest(&mut rt, "off the Map");
    assert_eq!(rig.publishes(), published, "off the Map: nothing published");

    typed(&mut rt, "m");
    rt.update(|s, _| s.diagram.lab.as_mut().unwrap().play());
    rt.frame(1.0 / 60.0);
    on(&mut rt, "map-tabs-5");
    assert!(!rt.state.diagram.lab.as_ref().unwrap().playing(), "another tab pauses it");
    comes_to_rest(&mut rt, "on a graph view");
}

#[test]
fn the_node_pane_closes_and_stays_closed_until_a_node_is_pressed() {
    let mut rt = runtime(diagram_app());
    typed(&mut rt, "m");
    let first_view = rt.state.map_tabs().into_iter().position(|t| matches!(t, MapTab::View(_))).unwrap();
    on(&mut rt, &format!("map-tabs-{first_view}"));
    assert!(rt.find("map-pane").is_none(), "no pane on load");
    let (x, y) = rt.find("node:voucher-service").expect("the default view draws the service");
    rt.pointer_move(x, y);
    settle(&mut rt);
    assert!(rt.find("map-pane").is_none(), "a hover does not open it");

    on(&mut rt, "node:voucher-service");
    assert_eq!(rt.state.diagram.selected.as_deref(), Some("voucher-service"));
    assert!(rt.find("map-pane").is_some(), "a press opens it");
    assert!(rt.state.layout.is_open(Overlay::Map), "and stays on the Map");

    on(&mut rt, "map-pane-close");
    assert!(rt.find("map-pane").is_none(), "✕ closes it");
    let (x, y) = rt.find("node:voucher-service").unwrap();
    rt.pointer_move(x + 2.0, y + 2.0);
    rt.pointer_move(x, y);
    settle(&mut rt);
    on(&mut rt, "map-tabs-0");
    on(&mut rt, &format!("map-tabs-{first_view}"));
    rt.update(|s, _| s.map_resized(s.diagram.seen_width + 40.0));
    settle(&mut rt);
    assert!(rt.find("map-pane").is_none(), "it stays closed across hovers, tabs and re-renders");

    on(&mut rt, "node:voucher-service");
    assert!(rt.find("map-pane").is_some(), "a press opens it again");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.find("map-pane").is_none(), "Escape closes it");
    assert!(rt.state.layout.is_open(Overlay::Map), "and leaves the Map up");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.layout.is_open(Overlay::Map), "a second Escape leaves the Map");
}

fn ms(t: std::time::Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

fn quantile(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(f64::total_cmp);
    v.get(((v.len().max(1) - 1) as f64 * p).round() as usize).copied().unwrap_or(0.0)
}

/// A review with and without the four diagrams: overview validation, the whole open, the diff
/// screen idling, the Map's first show of each diagram, play and paused frames.
/// `cargo test -p rain-app --release review_diagram_timings -- --ignored --nocapture`.
#[test]
#[ignore = "timings, run by hand in release"]
fn review_diagram_timings() {
    use std::time::Instant;
    let read = |p: std::path::PathBuf| -> serde_json::Value { serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap() };
    let (plain_v, diag_v) = (read(overview()), read(diagrams_overview()));
    let validate = |v: &serde_json::Value| {
        let mut t = Vec::new();
        for _ in 0..50 {
            let s = Instant::now();
            rain_overview::load_value(v.clone()).unwrap();
            t.push(ms(s));
        }
        quantile(&mut t, 0.5)
    };
    let opened = |with: bool| {
        let mut t = Vec::new();
        for _ in 0..15 {
            let s = Instant::now();
            let a = if with { diagram_app() } else { app(story(), true) };
            t.push(ms(s));
            drop(a);
        }
        quantile(&mut t, 0.5)
    };
    let idle = |a: App| {
        let mut rt = sized_runtime(a, (1440.0, 900.0));
        rt.update(|s, _| s.settings.break_reminder.enabled = false);
        settle(&mut rt);
        let mut t = Vec::new();
        let mut asked = 0;
        for _ in 0..240 {
            let s = Instant::now();
            asked += usize::from(rt.frame(1.0 / 60.0));
            t.push(ms(s));
        }
        (quantile(&mut t.clone(), 0.5), quantile(&mut t, 0.95), asked)
    };
    let (vp, vd) = (validate(&plain_v), validate(&diag_v));
    let (op, od) = (opened(false), opened(true));
    let mut plain_app = app(story(), true);
    plain_app.settings.break_reminder.enabled = false;
    let ip = idle(plain_app);
    let id = idle(diagram_app());
    println!("| measure | without diagrams | with four diagrams |");
    println!("|---|---|---|");
    println!("| overview load + validate p50 ms | {vp:.3} | {vd:.3} |");
    println!("| review open (load_now) p50 ms | {op:.1} | {od:.1} |");
    println!(
        "| diff screen idle p50 / p95 ms, frames asked of 240 | {:.3} / {:.3}, {} | {:.3} / {:.3}, {} |",
        ip.0, ip.1, ip.2, id.0, id.1, id.2
    );

    println!();
    println!(
        "| diagram | first show ms | play p50 / p95 ms | rebuilds while playing | paused p50 / p95 ms | paused frames asked of 240 | wake |"
    );
    println!("|---|---|---|---|---|---|---|");
    for (n, id) in ["tour", "flowchart", "lineage", "icicle"].into_iter().enumerate() {
        let mut rt = sized_runtime(diagram_app(), (1440.0, 900.0));
        settle(&mut rt);
        let t = Instant::now();
        rt.update(|s, _| {
            s.toggle_map();
            s.pick_map_lab(style(id));
        });
        for _ in 0..3 {
            rt.frame(1.0 / 60.0);
        }
        let first = ms(t);
        settle(&mut rt);
        assert_eq!(rt.state.map_tab(), MapTab::Lab(style(id)), "{n}");
        rt.update(|s, _| {
            s.rest.clock = 1.0;
            s.diagram.lab.as_mut().unwrap().play();
        });
        rt.frame(1.0 / 60.0);
        let frames = ((rt.state.diagram.lab.as_ref().unwrap().clock.duration() * 60.0) as usize).min(1500);
        let commits = rt.stats.commits;
        let mut play = Vec::new();
        for k in 1..=frames {
            rt.state.rest.clock = 1.0 + k as f32 / 60.0;
            let t = Instant::now();
            rt.frame(1.0 / 60.0);
            play.push(ms(t));
        }
        let rebuilds = rt.stats.commits - commits;
        rt.update(|s, _| s.diagram.lab.as_mut().unwrap().pause());
        let mut n = 0;
        while rt.frame(1.0 / 60.0) && n < 600 {
            n += 1;
        }
        let (mut idle, mut asked) = (Vec::new(), 0);
        for _ in 0..240 {
            let t = Instant::now();
            asked += usize::from(rt.frame(1.0 / 60.0));
            idle.push(ms(t));
        }
        println!(
            "| {id} | {first:.1} | {:.2} / {:.2} | {rebuilds} in {frames} frames | {:.3} / {:.3} | {asked} | {:?} |",
            quantile(&mut play.clone(), 0.5),
            quantile(&mut play, 0.95),
            quantile(&mut idle.clone(), 0.5),
            quantile(&mut idle, 0.95),
            rt.wake_deadline(),
        );
    }
}
