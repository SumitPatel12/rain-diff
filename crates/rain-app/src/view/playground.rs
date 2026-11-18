//! The Diagram playground page: the styles in a rail, the picked one on the canvas with its
//! timeline controls above, and how to read it beside, with the sample's raw JSON on request.
//! The timeline moves bound values, the step label included; this view is rebuilt only for a
//! pick, play or pause, or a resize.

use rain_lab::{Act, SCRUB_W, SPEEDS, SchemaId, registry};
use rune::{Cursor, Cx, Drag, El, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Kit, SegmentedControl};

use super::icons;
use super::ui::{Ghost, vsep};
use crate::app::App;
use crate::theme::{Look, TYPE, radius, space};

/// The canvas's size before its first layout.
const FIRST_SIZE: [f32; 2] = [900.0, 620.0];
const RAIL_W: f32 = 290.0;
const NOTE_W: f32 = 320.0;

pub(super) fn page(s: &App, look: &Look, cx: &mut Cx<App>) -> El {
    let k = &look.kit;
    let lab = &s.playground.lab;
    lab.reduced.set(cx.is_reduced_motion());
    let close = Ghost::new("playground-close")
        .icon(&icons::CLOSE)
        .help("Close  Esc")
        .build(k)
        .on_press(|s: &mut App| {
            s.close_playground();
        });
    let head = row()
        .key("playground-head")
        .w_full()
        .h(space::TOP_BAR)
        .pl(14.0)
        .pr(10.0)
        .gap(10.0)
        .items_center()
        .child(icons::draw(&icons::DIAGRAM, 18.0, look.r.accent))
        .child(text("Diagram playground").font(TYPE.title).color(k.c.text_base))
        .child(
            text(format!("One change, “Vouchers at checkout”, drawn {} ways", registry().len()))
                .font(TYPE.small)
                .color(k.c.text_muted)
                .truncate()
                .shrink(),
        )
        .child(el().grow(1.0).a11y_hidden())
        .child(close);
    let body = row()
        .key("playground-body")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(space::GAP)
        .pb(space::GAP)
        .gap(space::GAP)
        .child(super::island(rail(s, look), look.s.panel, look).w(RAIL_W).h_full())
        .child(super::island(stage(s, look), look.s.reader, look).grow(1.0).min_w(0.0).h_full())
        .child(super::island(note(s, look), look.s.panel, look).w(NOTE_W).h_full());
    col()
        .key("playground")
        .role("dialog")
        .label("Diagram playground")
        .w_full()
        .h_full()
        .fill(look.s.bar)
        .on_press(|_: &mut App| {})
        .child(head)
        .child(body)
}

/// A schema's letter as a small chip; its name in the tooltip.
fn chip(id: SchemaId, k: &Kit, look: &Look) -> El {
    row()
        .h(16.0)
        .px(5.0)
        .center()
        .radius(radius::KEY)
        .fill(look.s.well)
        .border(1.0, look.s.hairline)
        .help(format!("Schema {}: {}", id.letter(), id.name()))
        .child(text(id.letter()).font(TYPE.micro_mono.lh(14.0)).color(k.c.text_muted).a11y_hidden())
}

fn rail(s: &App, look: &Look) -> El {
    let k = &look.kit;
    let on = s.playground.lab.style_index();
    let mut list = col()
        .key("playground-styles")
        .role("tablist")
        .label("Diagram styles")
        .w_full()
        .padding(6.0)
        .gap(2.0);
    for (i, st) in registry().iter().enumerate() {
        let picked = i == on;
        let n = i + 1;
        let mut chips = row().gap(4.0).items_center();
        for id in st.schemas() {
            chips = chips.child(chip(*id, k, look));
        }
        if !st.ready() {
            chips = chips.child(text("Coming next").font(TYPE.micro).color(k.c.text_muted).pl(4.0).a11y_hidden());
        }
        let number = row()
            .size(22.0, 22.0)
            .center()
            .radius(11.0)
            .fill(if picked { look.r.accent } else { look.s.well })
            .child(
                text(n.to_string())
                    .font(TYPE.mark)
                    .color(if picked { look.r.on_accent } else { k.c.text_muted })
                    .a11y_hidden(),
            )
            .a11y_hidden();
        let words = col()
            .grow(1.0)
            .shrink()
            .min_w(0.0)
            .gap(2.0)
            .child(
                text(st.name())
                    .font(TYPE.strong)
                    .color(if st.ready() { k.c.text_base } else { k.c.text_muted })
                    .a11y_hidden(),
            )
            .child(
                text(st.pitch())
                    .font(TYPE.small)
                    .color(k.c.text_muted)
                    .wrap()
                    .w_full()
                    .a11y_hidden(),
            )
            .child(chips.pt(2.0));
        let mut item = row()
            .key(format!("playground-style-{n}"))
            .role("tab")
            .label(format!("{n}. {}", st.name()))
            .w_full()
            .padding(8.0)
            .gap(10.0)
            .radius(radius::BUTTON)
            .cursor(Cursor::Pointer)
            .focusable(true)
            .child(number)
            .child(words)
            .on_press(move |s: &mut App| s.pick_style(i));
        if picked {
            item = item.fill(look.s.selected_row).checked(true);
        }
        list = list.child(k.interactive(item));
    }
    col().w_full().h_full().min_h(0.0).child(
        col()
            .key("playground-rail")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .scroll(ScrollSpec { stick_bottom: false })
            .child(list),
    )
}

/// The timeline controls over the canvas.
fn controls(s: &App, look: &Look) -> El {
    let k = &look.kit;
    let lab = &s.playground.lab;
    let style = lab.style();
    let mut bar = row()
        .key("playground-controls")
        .w_full()
        .h(space::READER_TOOL + 4.0)
        .px(10.0)
        .gap(6.0)
        .items_center()
        .fill(look.s.card_head);
    let schemas = style.schemas();
    let switch = (schemas.len() > 1).then(|| {
        let labels: Vec<String> = schemas.iter().map(|id| format!("{} · {}", id.letter(), short(*id))).collect();
        let picked = schemas.iter().position(|id| *id == lab.schema());
        row()
            .key("playground-schemas")
            .w_full()
            .px(10.0)
            .pt(6.0)
            .gap(8.0)
            .items_center()
            .fill(look.s.card_head)
            .child(text("Schema").font(TYPE.small).color(k.c.text_muted).a11y_hidden())
            .child(
                SegmentedControl::new(&labels, picked)
                    .label("Schema")
                    .fit()
                    .on_pick(|s: &mut App, i, _: &mut Tx| {
                        if let Some(id) = s.playground.lab.style().schemas().get(i).copied() {
                            s.playground.lab.set_schema(id);
                        }
                    })
                    .build(k)
                    .key("playground-schema"),
            )
    });
    let playing = lab.playing();
    bar = bar
        .child(
            Ghost::new("playground-play")
                .icon(if playing { &icons::PAUSE } else { &icons::PLAY })
                .help(if playing { "Pause  Space" } else { "Play  Space" })
                .on(playing)
                .build(k)
                .on_press(|s: &mut App| s.playground.lab.toggle()),
        )
        .child(
            Ghost::new("playground-back")
                .icon(&icons::CHEV_L)
                .help("Step back  ←")
                .build(k)
                .on_press(|s: &mut App| s.playground.lab.step_by(false)),
        )
        .child(
            Ghost::new("playground-forward")
                .icon(&icons::CHEV_R)
                .help("Step forward  →")
                .build(k)
                .on_press(|s: &mut App| s.playground.lab.step_by(true)),
        )
        .child(scrubber(s, look))
        .child(
            text("00.0 s / 00.0 s")
                .key("playground-time")
                .font(TYPE.micro_mono)
                .color(k.c.text_muted)
                .live_text(&lab.scrub.time)
                .a11y_hidden(),
        )
        .child(
            Ghost::new("playground-restart")
                .icon(&icons::RELOAD)
                .help("Restart  Home")
                .build(k)
                .on_press(|s: &mut App| s.playground.lab.restart()),
        )
        .child(vsep(k));
    let speeds: Vec<String> = SPEEDS.iter().map(|v| format!("{v}×")).collect();
    let speed = SPEEDS.iter().position(|v| (*v - lab.clock.speed()).abs() < 1e-3);
    let bar = bar
        .child(
            SegmentedControl::new(&speeds, speed)
                .label("Speed")
                .fit()
                .on_pick(|s: &mut App, i, _: &mut Tx| {
                    if let Some(v) = SPEEDS.get(i) {
                        s.playground.lab.set_speed(*v);
                    }
                })
                .build(k)
                .key("playground-speed"),
        )
        .child(step_label(s, look));
    match switch {
        Some(sw) => col().w_full().child(sw).child(bar),
        None => bar,
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

/// The step the timeline is in; it changes with the clock, without a rebuild.
fn step_label(s: &App, look: &Look) -> El {
    let k = &look.kit;
    text("")
        .key("playground-step")
        .role("status")
        .font(TYPE.small)
        .color(k.c.text_muted)
        .live_text(&s.playground.lab.scrub.step)
        .truncate_fade()
        .grow(1.0)
        .min_w(0.0)
        .pl(6.0)
}

/// The timeline: press or drag along it to scrub; its fill and thumb move with the clock.
fn scrubber(s: &App, look: &Look) -> El {
    let k = &look.kit;
    let scrub = &s.playground.lab.scrub;
    el().key("playground-scrub")
        .role("slider")
        .label("Timeline")
        .help("Drag to scrub the timeline")
        .w(SCRUB_W)
        .h(20.0)
        .mx(6.0)
        .cursor(Cursor::Pointer)
        .on_drag(|s: &mut App, d: &mut Drag, tx: &mut Tx| {
            let lab = &mut s.playground.lab;
            lab.pause();
            lab.seek_frac(d.x / SCRUB_W);
            if !matches!(d.phase, rune::Phase::Down) {
                tx.live_only();
            }
        })
        .child(
            el().absolute(0.0, 8.0)
                .size(SCRUB_W, 4.0)
                .radius(2.0)
                .fill(look.s.well)
                .border(1.0, look.s.hairline)
                .child(
                    el().absolute(0.0, 0.0)
                        .size(0.0, 4.0)
                        .radius(2.0)
                        .fill(look.r.accent)
                        .live_rect(&scrub.fill)
                        .a11y_hidden(),
                )
                .a11y_hidden(),
        )
        .child(
            el().key("playground-thumb")
                .absolute(-6.0, 4.0)
                .size(12.0, 12.0)
                .radius(6.0)
                .fill(k.c.text_base)
                .live_offset(&scrub.thumb)
                .a11y_hidden(),
        )
}

/// The controls and the style's canvas.
fn stage(s: &App, look: &Look) -> El {
    let k = &look.kit;
    let size = if s.playground.size[0] >= 1.0 {
        s.playground.size
    } else {
        FIRST_SIZE
    };
    let ink = look.scheme.diagram();
    let act = |e: El, a: Act| e.on_press(move |s: &mut App| s.playground.lab.act(a.clone()));
    let canvas = s.playground.lab.canvas(k, &ink, size, &act);
    col()
        .w_full()
        .h_full()
        .min_h(0.0)
        .child(controls(s, look))
        .child(el().w_full().h(1.0).fill(look.s.hairline).a11y_hidden())
        .child(
            col()
                .key("lab-canvas")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .fill(ink.background)
                .clip_rounded(0.0)
                .on_layout(|s: &mut App, r: [f32; 4]| s.playground_resized([r[2], r[3]]))
                .child(canvas),
        )
}

/// How to read the style, and the sample it reads as raw JSON on request.
fn note(s: &App, look: &Look) -> El {
    let k = &look.kit;
    let lab = &s.playground.lab;
    let style = lab.style();
    let schema = lab.schema();
    let how = style
        .how_to_read()
        .unwrap_or("This style is still to be built. Its name, pitch and schemas are in the list.");
    let mut panel = col()
        .key("playground-note")
        .w_full()
        .h_full()
        .min_h(0.0)
        .padding(14.0)
        .gap(10.0)
        .child(text("How to read this").font(TYPE.label).color(k.c.text_base))
        .child(
            text(how)
                .key("playground-how")
                .font(TYPE.small)
                .color(k.c.text_muted)
                .wrap()
                .w_full(),
        )
        .child(el().w_full().h(1.0).fill(look.s.hairline).a11y_hidden())
        .child(
            row()
                .w_full()
                .gap(8.0)
                .items_center()
                .child(chip(schema, k, look))
                .child(
                    text(schema.name())
                        .font(TYPE.small)
                        .color(k.c.text_base)
                        .grow(1.0)
                        .shrink()
                        .truncate(),
                )
                .child(
                    Ghost::new("playground-raw")
                        .label("Raw JSON")
                        .help(if lab.raw {
                            "Hide the sample's JSON"
                        } else {
                            "Show the sample's JSON"
                        })
                        .on(lab.raw)
                        .build(k)
                        .on_press(|s: &mut App| s.playground.lab.raw = !s.playground.lab.raw),
                ),
        );
    if lab.raw {
        let json = rune::memo(format!("playground-json-{}", schema.letter()), s.seen_scheme.get(), || {
            text(schema.raw().to_owned())
                .font(TYPE.micro_mono)
                .color(k.c.text_base)
                .selectable(true)
                .w_full()
        });
        panel = panel.child(
            col()
                .key("playground-json")
                .role("document")
                .label(format!("Sample {} as JSON", schema.letter()))
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .padding(8.0)
                .radius(radius::BUTTON)
                .fill(look.s.well)
                .border(1.0, look.s.hairline)
                .scroll(ScrollSpec { stick_bottom: false })
                .child(json),
        );
    }
    panel
}
