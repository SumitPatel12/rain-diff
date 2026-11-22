//! A rain-lab style on a canvas with its timeline controls above: play, step, scrub, restart,
//! speed and the step label. The Diagram playground and the Map's diagram tabs both draw it, each
//! over its own [`Lab`]. The timeline moves bound values, the step label included; this view is
//! rebuilt only for a pick, play or pause, or a resize.

use rain_diagram::DiagramStyle;
use rain_lab::{Act, Lab, SCRUB_W, SPEEDS, SchemaId};
use rune::{Cursor, Drag, El, Tx, col, el, row, text};
use rune_kit::{Kit, SegmentedControl};

use super::icons;
use super::ui::{Activate, Ghost, vsep};
use crate::app::App;
use crate::theme::{Roles, Surfaces, TYPE, space};

/// The canvas's size before its first layout.
const FIRST_SIZE: [f32; 2] = [900.0, 620.0];

/// Which page's lab a control drives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Playground,
    Map,
}

impl Slot {
    /// `playground-play`, `map-lab-play`.
    fn key(self, name: &str) -> String {
        match self {
            Slot::Playground => format!("playground-{name}"),
            Slot::Map => format!("map-lab-{name}"),
        }
    }

    fn lab(self, s: &App) -> Option<&Lab> {
        match self {
            Slot::Playground => Some(&s.playground.lab),
            Slot::Map => s.diagram.lab.as_ref(),
        }
    }

    fn size(self, s: &App) -> [f32; 2] {
        let size = match self {
            Slot::Playground => s.playground.size,
            Slot::Map => s.diagram.lab_size,
        };
        if size[0] >= 1.0 { size } else { FIRST_SIZE }
    }

    fn with(self, s: &mut App, f: impl FnOnce(&mut Lab)) {
        let lab = match self {
            Slot::Playground => Some(&mut s.playground.lab),
            Slot::Map => s.diagram.lab.as_mut(),
        };
        if let Some(l) = lab {
            f(l);
        }
    }

    fn resized(self, s: &mut App, size: [f32; 2]) {
        match self {
            Slot::Playground => s.playground_resized(size),
            Slot::Map => s.map_lab_resized(size),
        }
    }
}

fn short(id: SchemaId) -> &'static str {
    match id {
        SchemaId::A => "Trace",
        SchemaId::B => "Before/after",
        SchemaId::C => "Call trees",
        SchemaId::S => "Script",
    }
}

fn controls(lab: &Lab, slot: Slot, k: &Kit, surf: &Surfaces, r: &Roles) -> El {
    let style = lab.style();
    let mut bar = row()
        .key(slot.key("controls"))
        .w_full()
        .h(space::READER_TOOL + 4.0)
        .px(10.0)
        .gap(6.0)
        .items_center()
        .fill(surf.card_head);
    let schemas = style.schemas();
    let switch = (schemas.len() > 1).then(|| {
        let labels: Vec<String> = schemas.iter().map(|id| format!("{} · {}", id.letter(), short(*id))).collect();
        let picked = schemas.iter().position(|id| *id == lab.schema());
        row()
            .key(slot.key("schemas"))
            .w_full()
            .px(10.0)
            .pt(6.0)
            .gap(8.0)
            .items_center()
            .fill(surf.card_head)
            .child(text("Schema").font(TYPE.small).color(k.c.text_muted).a11y_hidden())
            .child(
                SegmentedControl::new(&labels, picked)
                    .label("Schema")
                    .fit()
                    .on_pick(move |s: &mut App, i, _: &mut Tx| {
                        slot.with(s, |l| {
                            if let Some(id) = l.style().schemas().get(i).copied() {
                                l.set_schema(id);
                            }
                        })
                    })
                    .build(k)
                    .key(slot.key("schema")),
            )
    });
    let playing = lab.playing();
    bar = bar
        .child(
            Ghost::new(slot.key("play"))
                .icon(if playing { &icons::PAUSE } else { &icons::PLAY })
                .help(if playing { "Pause  Space" } else { "Play  Space" })
                .on(playing)
                .build(k)
                .on_activate(move |s: &mut App| slot.with(s, Lab::toggle)),
        )
        .child(
            Ghost::new(slot.key("back"))
                .icon(&icons::CHEV_L)
                .help("Step back  ←")
                .build(k)
                .on_activate(move |s: &mut App| slot.with(s, |l| l.step_by(false))),
        )
        .child(
            Ghost::new(slot.key("forward"))
                .icon(&icons::CHEV_R)
                .help("Step forward  →")
                .build(k)
                .on_activate(move |s: &mut App| slot.with(s, |l| l.step_by(true))),
        )
        .child(scrubber(lab, slot, k, surf, r))
        .child(
            text("00.0 s / 00.0 s")
                .key(slot.key("time"))
                .font(TYPE.micro_mono)
                .color(k.c.text_muted)
                .live_text(&lab.scrub.time)
                .a11y_hidden(),
        )
        .child(
            Ghost::new(slot.key("restart"))
                .icon(&icons::RELOAD)
                .help("Restart  Home")
                .build(k)
                .on_activate(move |s: &mut App| slot.with(s, Lab::restart)),
        )
        .child(vsep(k));
    let speeds: Vec<String> = SPEEDS.iter().map(|v| format!("{v}×")).collect();
    let speed = SPEEDS.iter().position(|v| (*v - lab.clock.speed()).abs() < 1e-3);
    let bar = bar
        .child(
            SegmentedControl::new(&speeds, speed)
                .label("Speed")
                .fit()
                .on_pick(move |s: &mut App, i, _: &mut Tx| {
                    if let Some(v) = SPEEDS.get(i) {
                        slot.with(s, |l| l.set_speed(*v));
                    }
                })
                .build(k)
                .key(slot.key("speed")),
        )
        .child(
            text("")
                .key(slot.key("step"))
                .role("status")
                .font(TYPE.small)
                .color(k.c.text_muted)
                .live_text(&lab.scrub.step)
                .truncate_fade()
                .grow(1.0)
                .min_w(0.0)
                .pl(6.0),
        );
    match switch {
        Some(sw) => col().w_full().child(sw).child(bar),
        None => bar,
    }
}

/// The timeline: press or drag along it to scrub; its fill and thumb move with the clock.
fn scrubber(lab: &Lab, slot: Slot, k: &Kit, surf: &Surfaces, r: &Roles) -> El {
    let scrub = &lab.scrub;
    el().key(slot.key("scrub"))
        .role("slider")
        .label("Timeline")
        .help("Drag to scrub the timeline")
        .w(SCRUB_W)
        .h(20.0)
        .mx(6.0)
        .cursor(Cursor::Pointer)
        .on_drag(move |s: &mut App, d: &mut Drag, tx: &mut Tx| {
            slot.with(s, |l| {
                l.pause();
                l.seek_frac(d.x / SCRUB_W);
            });
            if !matches!(d.phase, rune::Phase::Down) {
                tx.live_only();
            }
        })
        .child(
            el().absolute(0.0, 8.0)
                .size(SCRUB_W, 4.0)
                .radius(2.0)
                .fill(surf.well)
                .border(1.0, surf.hairline)
                .child(
                    el().absolute(0.0, 0.0)
                        .size(0.0, 4.0)
                        .radius(2.0)
                        .fill(r.accent)
                        .live_rect(&scrub.fill)
                        .a11y_hidden(),
                )
                .a11y_hidden(),
        )
        .child(
            el().key(slot.key("thumb"))
                .absolute(-6.0, 4.0)
                .size(12.0, 12.0)
                .radius(6.0)
                .fill(k.c.text_base)
                .live_offset(&scrub.thumb)
                .a11y_hidden(),
        )
}

/// The controls and the style's canvas; `None` when the slot has no lab.
pub(super) fn stage(s: &App, slot: Slot, reduced: bool, ink: &DiagramStyle, k: &Kit, surf: &Surfaces, r: &Roles) -> Option<El> {
    let lab = slot.lab(s)?;
    lab.reduced.set(reduced);
    let act = move |e: El, a: Act| e.on_press(move |s: &mut App| slot.with(s, |l| l.act(a.clone())));
    let canvas = lab.canvas(k, ink, slot.size(s), &act);
    let canvas_key = match slot {
        Slot::Playground => "lab-canvas".to_owned(),
        Slot::Map => "map-lab-canvas".to_owned(),
    };
    Some(
        col()
            .w_full()
            .h_full()
            .min_h(0.0)
            .child(controls(lab, slot, k, surf, r))
            .child(el().w_full().h(1.0).fill(surf.hairline).a11y_hidden())
            .child(
                col()
                    .key(canvas_key)
                    .w_full()
                    .grow(1.0)
                    .min_h(0.0)
                    .fill(ink.background)
                    .clip_rounded(0.0)
                    .on_layout(move |s: &mut App, r: [f32; 4]| slot.resized(s, [r[2], r[3]]))
                    .child(canvas),
            ),
    )
}
