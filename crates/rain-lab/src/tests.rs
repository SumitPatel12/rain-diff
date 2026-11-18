use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use rain_diagram::DiagramStyle;
use rune::El;

use crate::Lab;
use crate::motion::{self, Clock, Highlight, Tick, Track};
use crate::schema::trace::{Span, spans};
use crate::schema::{Samples, SchemaId, Status, samples};
use crate::style::{Act, registry};

/// Counts the allocations each thread makes.
struct Counting;

thread_local! {
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

fn count() {
    let _ = ALLOCS.try_with(|n| n.set(n.get() + 1));
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        count();
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        count();
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

fn allocations() -> u64 {
    ALLOCS.with(Cell::get)
}

fn kit() -> rune_kit::Kit {
    rune_kit::Kit::neutral(true)
}

fn no_act(e: El, _: Act) -> El {
    e
}

/// A lab on the field lineage, its rig built for a 1200 × 800 canvas.
fn lineage_lab() -> Lab {
    let mut lab = Lab::new();
    let i = registry().iter().position(|s| s.id() == "lineage").unwrap();
    lab.select(i);
    let _ = lab.canvas(&kit(), &DiagramStyle::dark(), [1200.0, 800.0], &no_act);
    lab
}

#[test]
fn the_registry_lists_the_four_kept_styles_in_order() {
    let r = registry();
    let ids: Vec<_> = r.iter().map(|s| s.id()).collect();
    assert_eq!(ids, ["tour", "flowchart", "lineage", "icicle"]);
    for s in r {
        assert!(
            s.ready() && !s.name().is_empty() && !s.pitch().is_empty() && !s.schemas().is_empty(),
            "{}",
            s.id()
        );
    }
    for (i, s) in r.iter().enumerate() {
        assert_eq!(s.number() as usize, i + 1, "{}", s.id());
        assert!(s.how_to_read().is_some(), "{}", s.id());
    }
    assert_eq!(r[0].schemas(), &[SchemaId::A], "the tour reads the trace only");
    assert_eq!(r[1].schemas(), &[SchemaId::S], "the flowchart plays the scene script");
    assert_eq!(r[2].schemas(), &[SchemaId::B]);
    assert_eq!(r[3].schemas(), &[SchemaId::C]);
    assert_eq!(Lab::new().style().id(), "tour", "the page opens on style 1");
}

#[test]
fn every_sample_parses() {
    let s = Samples::parse().unwrap();
    for (title, id) in [(&s.a.title, "A"), (&s.b.title, "B"), (&s.c.title, "C")] {
        assert_eq!(title, "Vouchers at checkout", "{id}");
    }
    assert!(s.s.duration() > 5.0);
    for id in SchemaId::ALL {
        assert!(!id.raw().is_empty() && !id.name().is_empty());
    }
}

#[test]
fn the_samples_share_their_ids() {
    let s = samples();
    let stations: Vec<&str> =
        s.a.paths
            .iter()
            .flat_map(|p| &p.steps)
            .flat_map(|st| [st.from.as_str(), st.to.as_str()])
            .collect();
    for p in &s.a.paths {
        for st in &p.steps {
            assert!(s.a.entity(&st.from).is_some() && s.a.entity(&st.to).is_some(), "{}", st.call);
            for o in &st.carries {
                assert!(s.a.entity(o).is_some() && !stations.contains(&o.as_str()), "{o} is a data object");
            }
        }
    }
    let shapes: Vec<&str> = s.s.shapes.iter().map(|x| x.id.as_str()).collect();
    for id in shapes.iter().filter(|id| s.a.entity(id).is_some()) {
        assert!(stations.contains(id), "{id} is a station in A as in S");
    }
    for f in &s.b.fields {
        let (stage, rest) = f.id.split_once(':').expect("stage:Owner.name");
        assert!(rest.contains('.') && serde_json::to_value(f.stage).unwrap() == stage, "{}", f.id);
    }
    for l in &s.b.lineage {
        for hop in l.hops() {
            assert!(s.b.field(hop).is_some(), "{hop}");
        }
    }
    for b in &s.s.beats {
        for a in &b.actions {
            assert!(a.target.is_empty() || s.s.shape(&a.target).is_some(), "{}", a.target);
        }
        for sp in spans(b.say.as_deref().unwrap_or_default()) {
            if let Span::Link(id) = sp {
                assert!(s.s.shape(id).is_some(), "{id} in the scene");
            }
        }
    }
    for f in &s.b.fields {
        if let Some(to) = &f.references {
            assert!(s.b.field(to).is_some_and(|k| k.primary_key), "{} references a key", f.id);
        }
    }
}

#[test]
fn b_says_what_changed() {
    let b = &samples().b;
    let st = |id: &str| b.field(id).unwrap().status();
    assert_eq!(st("db:Vouchers.Amount"), Status::Added);
    assert_eq!(st("db:Accounts.LegacyTotal"), Status::Removed);
    assert_eq!(st("db:Accounts.Total"), Status::Changed, "no longer nullable");
    assert_eq!(st("db:Accounts.Id"), Status::Same);
}

#[test]
fn the_clock_plays_only_while_playing_and_is_the_same_at_the_same_host_time() {
    let mut c = Clock::new(4.0);
    assert_eq!(c.tick(10.0), Tick::Idle);
    assert_eq!(c.t(), 0.0);
    c.play(false);
    assert_eq!(c.tick(100.0), Tick::Moved, "the first tick anchors, however old the host time");
    assert_eq!(c.t(), 0.0);
    assert_eq!(c.tick(101.5), Tick::Moved);
    assert!((c.t() - 1.5).abs() < 1e-5);
    let mut d = Clock::new(4.0);
    d.play(false);
    d.tick(100.0);
    for h in [100.2, 100.9, 101.1] {
        d.tick(h);
    }
    d.tick(101.5);
    assert_eq!(c.t(), d.t(), "where it is depends on the host time only");
    c.pause();
    assert_eq!(c.tick(150.0), Tick::Idle);
    assert!((c.t() - 1.5).abs() < 1e-5, "paused, it holds");
    c.set_speed(2.0);
    c.play(false);
    c.tick(200.0);
    c.tick(201.0);
    assert!((c.t() - 3.5).abs() < 1e-5, "twice as fast");
    assert_eq!(c.tick(202.0), Tick::Ended);
    assert_eq!(c.t(), 4.0);
    assert!(!c.playing());
}

#[test]
fn steps_play_to_the_next_mark_and_jump_back() {
    let marks = [0.0, 1.0, 2.5, 4.0];
    let mut c = Clock::new(4.0);
    c.step(&marks, true, false);
    c.tick(0.0);
    assert_eq!(c.tick(5.0), Tick::Ended);
    assert_eq!(c.t(), 1.0, "stops at the next step");
    c.step(&marks, true, true);
    assert_eq!(c.t(), 2.5, "reduced motion lands there at once");
    c.step(&marks, false, false);
    assert_eq!(c.t(), 1.0);
    c.play(true);
    assert_eq!(c.t(), 4.0, "reduced motion plays to the end state");
    assert!(!c.playing());
    c.play(false);
    assert_eq!(c.t(), 0.0, "play at the end starts over");
}

#[test]
fn a_rig_publishes_the_same_values_for_the_same_time() {
    let mut lab = lineage_lab();
    let rig = lab.rig().unwrap();
    let rows = rig.data::<crate::styles::Flow>().unwrap();
    let amount = rows.rows.iter().find(|r| r.id == "ui:VoucherForm.amount").unwrap();
    let page = rows.rows.iter().find(|r| r.id == "db:Accounts.Total").unwrap();
    lab.seek_frac(0.4);
    let a = (amount.alpha.live.get(), page.tint.live.get(), lab.scrub.fill.get());
    lab.seek_frac(1.0);
    assert_eq!(amount.alpha.live.get(), 1.0);
    lab.seek_frac(0.0);
    assert_eq!(amount.alpha.live.get(), 0.0);
    lab.seek_frac(0.4);
    assert_eq!((amount.alpha.live.get(), page.tint.live.get(), lab.scrub.fill.get()), a);
}

#[test]
fn a_paused_lab_does_no_work() {
    let mut lab = lineage_lab();
    let rig = lab.rig().unwrap();
    let n = rig.publishes();
    for h in 0..50 {
        assert_eq!(lab.tick(h as f32), Tick::Idle);
    }
    assert_eq!(rig.publishes(), n, "paused, nothing is published");
    lab.play();
    let n = rig.publishes();
    lab.tick(0.0);
    lab.tick(0.1);
    assert_eq!(rig.publishes(), n + 2, "playing, each tick publishes once");
    let mut saw = Vec::new();
    for k in 1..400 {
        saw.push(lab.tick(0.1 + k as f32 * 0.05));
    }
    assert!(saw.contains(&Tick::Stepped), "the timeline crosses steps");
    assert_eq!(saw.iter().filter(|t| **t == Tick::Ended).count(), 1);
    assert!(!lab.playing());
}

#[test]
fn a_playing_frame_allocates_nothing_and_a_paused_one_does_nothing() {
    for (i, s) in registry().iter().enumerate() {
        for pick in [None, Some("q")].into_iter().filter(|p| p.is_none() || s.id() == "icicle") {
            let mut lab = Lab::new();
            lab.select(i);
            lab.act(Act::Pick(pick.map(str::to_owned)));
            let _ = lab.canvas(&kit(), &DiagramStyle::dark(), [1100.0, 760.0], &no_act);
            let rig = lab.rig().unwrap();
            lab.play();
            // Into every step once, so each text and highlight set has been made.
            let mut h = 0.0;
            while lab.playing() {
                lab.tick(h);
                h += 1.0 / 60.0;
            }
            lab.seek_frac(0.0);
            lab.play();
            lab.tick(0.0);
            let (mut quiet, mut worst, mut seen) = (0, 0, None);
            for k in 1..(rig.duration * 60.0) as usize {
                let n = allocations();
                lab.tick(k as f32 / 60.0);
                let made = allocations() - n;
                let shown = ((lab.clock.t() * 10.0).round() as i32, rig.step_at(lab.clock.t()));
                if seen == Some(shown) {
                    quiet += made;
                } else {
                    worst = worst.max(made);
                }
                seen = Some(shown);
            }
            assert_eq!(quiet, 0, "{} {pick:?}: a frame whose texts stay put allocates nothing", s.id());
            assert!(worst <= 32, "{} {pick:?}: {worst} allocations to change a text or a step", s.id());

            lab.pause();
            let (n, published) = (allocations(), rig.publishes());
            for k in 0..100 {
                assert_eq!(lab.tick(1000.0 + k as f32), Tick::Idle);
            }
            assert_eq!((allocations() - n, rig.publishes()), (0, published), "{}: paused, nothing", s.id());
        }
    }
}

#[test]
fn reduced_motion_shows_the_end_state() {
    let mut lab = lineage_lab();
    lab.reduced.set(true);
    lab.play();
    assert!(!lab.playing());
    assert!(lab.clock.at_end());
    let last = lab.rig().unwrap().steps.len() - 1;
    assert_eq!(lab.step().map(|(i, _)| i), Some(last));
}

#[test]
fn highlight_lights_its_ids_and_dims_the_rest() {
    let h = Highlight::default();
    let (a, b) = (h.opacity("a"), h.opacity("b"));
    assert_eq!((a.get(), b.get()), (1.0, 1.0));
    h.set(["a"]);
    assert_eq!((a.get(), b.get(), h.lit("a").get()), (1.0, motion::DIM, 1.0));
    let c = h.lit("c");
    assert_eq!(c.get(), 0.0, "a mark made later starts from the current set");
    h.clear();
    assert_eq!((a.get(), b.get(), h.lit("a").get()), (1.0, 1.0, 0.0));
}

#[test]
fn tracks_ease_between_keys_and_hold_outside_them() {
    let t = Track::new(vec![(1.0, 0.0f32), (2.0, 10.0)]);
    assert_eq!(t.at(0.0), 0.0);
    assert_eq!(t.at(1.5), 5.0);
    assert_eq!(t.at(9.0), 10.0);
    let p = motion::along(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], 0.75);
    assert_eq!(p, [10.0, 5.0], "by arc length");
    let k = motion::Keyed::new(None, Some([10.0, 10.0, 20.0, 20.0]), 0.0, 1.0);
    assert_eq!(k.change, motion::Change::Enter);
    assert_eq!(k.at(0.0).1, 0.0);
    assert_eq!(k.at(1.0), ([10.0, 10.0, 20.0, 20.0], 1.0));
    let w = motion::WriteOn::draw(1.0, 1.0);
    assert_eq!(w.at(0.0), [0.0, 0.0]);
    assert_eq!(w.at(5.0), [0.0, 1.0]);
}

#[test]
fn tour_text_links_split_out() {
    assert_eq!(
        spans("[[a]] posts to [[b]]."),
        vec![Span::Link("a"), Span::Text(" posts to "), Span::Link("b"), Span::Text(".")]
    );
    for step in &samples().a.tour {
        for s in spans(&step.text) {
            if let Span::Link(id) = s {
                assert!(samples().a.entity(id).is_some() || samples().a.path(id).is_some(), "{id}");
            }
        }
    }
}

#[test]
fn switching_styles_resets_the_timeline_and_picks_their_first_schema() {
    let mut lab = lineage_lab();
    lab.seek_frac(0.5);
    lab.select(0);
    assert_eq!(lab.style().id(), "tour");
    assert_eq!(lab.schema(), SchemaId::A);
    assert_eq!(lab.clock.t(), 0.0);
    lab.set_schema(SchemaId::S);
    assert_eq!(lab.schema(), SchemaId::A, "a schema the style doesn't read is refused");
    let _ = lab.canvas(&kit(), &DiagramStyle::light(), [800.0, 600.0], &no_act);
    assert!(lab.style().ready());
}

/// What laying a style out (its rig) and drawing it (its view) cost, median of 20:
/// `cargo test -p rain-lab --release rig_and_view_timings -- --ignored --nocapture`.
#[test]
#[ignore = "timings, run by hand in release"]
fn rig_and_view_timings() {
    use std::time::Instant;
    let (k, ink) = (kit(), DiagramStyle::dark());
    for s in registry() {
        for pick in [None, Some("q")].into_iter().filter(|p| p.is_none() || s.id() == "icicle") {
            let cx = crate::style::Cx {
                samples: samples(),
                schema: s.schemas()[0],
                kit: &k,
                ink: &ink,
                size: [1100.0, 760.0],
                reduced: false,
                pick,
                act: &no_act,
            };
            let (mut rig_ms, mut view_ms) = (Vec::new(), Vec::new());
            for _ in 0..20 {
                let t = Instant::now();
                let rig = s.rig(&cx);
                rig_ms.push(t.elapsed().as_secs_f64() * 1e3);
                let t = Instant::now();
                let _ = s.view(&cx, &rig);
                view_ms.push(t.elapsed().as_secs_f64() * 1e3);
            }
            rig_ms.sort_by(f64::total_cmp);
            view_ms.sort_by(f64::total_cmp);
            println!("{} {pick:?}: rig {:.2} ms, view {:.2} ms", s.id(), rig_ms[10], view_ms[10]);
        }
    }
}

/// A review's diagrams are the agent's, not the samples: tiny, sparse and odd documents that
/// pass the schema lay out, draw and play to the end without a panic in every style.
#[test]
fn every_style_draws_small_and_sparse_review_documents() {
    use rain_core::overview::OverviewDiagrams;
    use serde_json::json;
    let docs = [
        OverviewDiagrams {
            tour: Some(json!({ "title": "t", "entities": [{ "id": "a", "label": "A", "layer": "api" }],
                "paths": [{ "id": "p", "name": "P", "steps": [{ "from": "a", "to": "a", "call": "x()", "status": "same" }] }] })),
            flowchart: Some(json!({ "title": "t", "size": [10, 10], "shapes": [], "beats": [] })),
            lineage: Some(json!({ "title": "t" })),
            icicle: Some(json!({ "title": "t", "calls": { "id": "r", "name": "R" } })),
        },
        OverviewDiagrams {
            tour: Some(json!({ "title": "", "entities": [
                    { "id": "a", "label": "A", "layer": "mystery" }, { "id": "b", "label": "B", "layer": "db", "parent": "a" }],
                "paths": [{ "id": "p", "name": "P", "steps": [
                    { "from": "a", "to": "b", "call": "", "status": "removed", "fragment": "loop", "carries": ["b"] }] }],
                "tour": [{ "title": "only", "text": "[[a]] then [[nowhere]]", "focus": ["a", "b"], "camera": { "zoom": 9 } }] })),
            flowchart: Some(json!({ "title": "t", "size": [0, 0],
                "shapes": [{ "id": "x", "kind": "arrow", "at": [0, 0] }, { "id": "d", "kind": "dot", "at": [5, 5] }],
                "beats": [{ "t": 0, "actions": [{ "do": "pulse", "target": "x" }, { "do": "focus", "target": "" }] },
                          { "t": 0, "actions": [{ "do": "morph", "target": "d", "into": "" }] }] })),
            lineage: Some(json!({ "title": "t", "fields": [
                { "id": "db:A.x", "stage": "db", "before": null, "after": null },
                { "id": "ui:B.y", "stage": "ui", "after": { "name": "y", "type": "int" }, "references": "db:A.x" }],
                "lineage": [{ "from": "db:A.x", "to": "ui:B.y", "via": ["db:A.x"] }], "notes": [{ "card": "dto:None", "why": "w" }] })),
            icicle: Some(json!({ "title": "t", "calls": { "id": "r", "name": "R", "before": 0, "after": 0 },
                "profiles": [{ "title": "p", "calls": { "id": "q", "name": "Q", "children": [{ "id": "q1", "name": "Q1", "after": 1e9 }] } }] })),
        },
    ];
    let (k, ink) = (kit(), DiagramStyle::dark());
    for (n, d) in docs.iter().enumerate() {
        let (data, styles) = crate::review_diagrams(d);
        assert_eq!(styles.len(), 4, "doc {n}: all four read");
        for i in styles {
            let mut lab = Lab::over(data.clone(), i);
            for size in [[1100.0, 760.0], [40.0, 30.0]] {
                let _ = lab.canvas(&k, &ink, size, &no_act);
                lab.play();
                for f in 0..2000 {
                    if lab.tick(f as f32 / 30.0) == Tick::Ended || !lab.playing() {
                        break;
                    }
                }
                lab.step_by(false);
                lab.step_by(true);
                let _ = lab.canvas(&k, &ink, size, &no_act);
            }
        }
    }
}
