//! The Diagram playground page: the styles in a rail, the picked one on the canvas with its
//! timeline controls above (`super::lab`, shared with the Map), and how to read it beside, with
//! the sample's raw JSON on request.

use rain_lab::{SchemaId, registry};
use rune::{Cursor, Cx, El, ScrollSpec, col, el, row, text};
use rune_kit::Kit;

use super::icons;
use super::lab::Slot;
use super::ui::{Activate, Ghost};
use crate::app::App;
use crate::theme::{Look, TYPE, radius, space};

const RAIL_W: f32 = 290.0;
const NOTE_W: f32 = 320.0;

pub(super) fn page(s: &App, look: &Look, cx: &mut Cx<App>) -> El {
    let k = &look.kit;
    let reduced = cx.is_reduced_motion();
    let close = Ghost::new("playground-close")
        .icon(&icons::CLOSE)
        .help("Close  Esc")
        .build(k)
        .on_activate(|s: &mut App| {
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
        .child(
            super::island(stage(s, look, reduced), look.s.reader, look)
                .grow(1.0)
                .min_w(0.0)
                .h_full(),
        )
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

fn stage(s: &App, look: &Look, reduced: bool) -> El {
    let ink = look.scheme.diagram();
    super::lab::stage(s, Slot::Playground, reduced, &ink, &look.kit, &look.s, &look.r).unwrap_or_else(el)
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
                        .on_activate(|s: &mut App| s.playground.lab.raw = !s.playground.lab.raw),
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
