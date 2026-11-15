//! The cite peek: a floating card under a cite with the cited lines as the diff draws them
//! (numbers, tints, word changes, syntax colours), and the same lines in the pop-out window.

use rain_core::LineKind;
use rune::{Color, El, Span, TextStyle, Tx, col, el, row, text};
use rune_kit::syntax::Paint;
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, Placement, popover};

use crate::app::{App, PeekContent};
use crate::theme::Look;

const WIDTH: f32 = 640.0;
const MAX_HEIGHT: f32 = 420.0;
const ROW_H: f32 = 20.0;

/// `t` of the way from `base` toward `hue` in OKLab, at `base`'s alpha, as the diff tints.
fn tint(base: Color, hue: Color, t: f32) -> Color {
    let [a, b] = [base.to_oklab(), hue.to_oklab()];
    Color::from_oklab([0, 1, 2, 3].map(|i| if i == 3 { a[3] } else { a[i] + (b[i] - a[i]) * t }))
}

struct Tints {
    line: Color,
    gutter: Color,
    word: Color,
    sign: Color,
}

/// `t` of the way from `a` to `b` in sRGB, as the diff mixes a theme's own line fills.
fn srgb_mix(a: Color, b: Color, t: f32) -> Color {
    use rune::color::{srgb_decode, srgb_encode};
    let m = |x: f32, y: f32| srgb_decode(srgb_encode(x) + (srgb_encode(y) - srgb_encode(x)) * t);
    Color {
        r: m(a.r, b.r),
        g: m(a.g, b.g),
        b: m(a.b, b.b),
        a: a.a,
    }
}

fn tints(look: &Look, kind: LineKind) -> Option<Tints> {
    let k = &look.kit;
    let fills = look.scheme.change_fills();
    let (hue, fill) = match kind {
        LineKind::Added => (k.c.success.icon, fills.map(|f| f.0)),
        LineKind::Removed => (k.c.danger.icon, fills.map(|f| f.1)),
        LineKind::Context => return None,
    };
    if let Some(line) = fill {
        return Some(Tints {
            line,
            gutter: srgb_mix(line, hue, 0.12),
            word: srgb_mix(line, hue, 0.25),
            sign: hue,
        });
    }
    let bg = k.c.bg_base;
    let d = k.dark;
    Some(Tints {
        line: tint(bg, hue, if d { 0.13 } else { 0.1 }),
        gutter: tint(bg, hue, if d { 0.2 } else { 0.17 }),
        word: tint(bg, hue, if d { 0.32 } else { 0.27 }),
        sign: hue,
    })
}

fn code_font(k: &Kit) -> TextStyle {
    TextStyle {
        size: 13.0,
        line_height: ROW_H,
        ..k.ty.mono
    }
}

/// The peek's lines: what the card and the pop-out window both draw.
pub(crate) fn code(c: &PeekContent, look: &Look) -> El {
    let k = &look.kit;
    let font = code_font(k);
    let joined = c.rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>().join("\n");
    let palette = look.scheme.syntax();
    let highlights = rain_syntax::highlight(palette, c.lang.as_deref(), &joined);
    let theme = look.scheme.code_kinds(k);
    let mut out = col().key("peek-code").w_full().py(4.0);
    if c.rows.is_empty() {
        return out.child(
            text("No lines of this file are in the diff")
                .font(k.ty.small)
                .color(k.c.text_muted)
                .padding(12.0),
        );
    }
    let mut start = 0usize;
    for (i, r) in c.rows.iter().enumerate() {
        let end = start + r.text.len();
        if r.gap_before {
            out = out.child(
                row()
                    .h(16.0)
                    .w_full()
                    .pl(12.0)
                    .items_center()
                    .fill(tint(k.c.bg_base, k.c.info.icon, if k.dark { 0.08 } else { 0.05 }))
                    .child(text("⋯").font(k.ty.small).color(k.c.text_faint)),
            );
        }
        let mut spans: Vec<Span> = highlights
            .iter()
            .filter(|(rg, _)| rg.end > start && rg.start < end)
            .map(|(rg, p)| {
                let color = match p {
                    Paint::Color(c) => *c,
                    Paint::Kind(kind) => theme.color(*kind),
                };
                Span::new(rg.start.max(start) - start, rg.end.min(end) - start).color(color)
            })
            .collect();
        let t = tints(look, r.kind);
        if let Some(t) = &t {
            for w in &r.words {
                spans.push(Span::new(w.start.min(r.text.len()), w.end.min(r.text.len())).bg(t.word, 2.0));
            }
        }
        let n = if r.new != 0 { r.new } else { r.old };
        let (sign, sign_color) = match r.kind {
            LineKind::Added => ("+", t.as_ref().map_or(k.c.text_faint, |t| t.sign)),
            LineKind::Removed => ("−", t.as_ref().map_or(k.c.text_faint, |t| t.sign)),
            LineKind::Context => (" ", k.c.text_faint),
        };
        let gutter = row()
            .w(56.0)
            .h_full()
            .pr(8.0)
            .justify_end()
            .items_center()
            .fill(t.as_ref().map_or(k.c.bg_base, |t| t.gutter))
            .child(
                text(n.to_string())
                    .font(font)
                    .color(if r.cited { k.c.text_base } else { k.c.text_faint }),
            );
        out = out.child(
            row()
                .key(format!("peek-row-{i}"))
                .w_full()
                .h(ROW_H)
                .fill(t.as_ref().map_or(k.c.bg_base, |t| t.line))
                .child(
                    el().w(3.0)
                        .h_full()
                        .fill(if r.cited { k.c.text_accent } else { Color::TRANSPARENT }),
                )
                .child(gutter)
                .child(text(sign).font(font).color(sign_color).w(18.0).pl(6.0))
                .child(
                    text(r.text.replace('\t', "    ").clone())
                        .font(font)
                        .color(k.c.text_base)
                        .spans(if r.text.contains('\t') { Vec::new() } else { spans })
                        .grow(1.0)
                        .shrink()
                        .min_w(0.0),
                ),
        );
        start = end + 1;
    }
    out
}

/// The card's head: where the lines are, then Open in reader, Pop out and close.
fn head(c: &PeekContent, k: &Kit) -> El {
    let small = |label: &str, key: &str, f: fn(&mut App, &mut Tx)| {
        Button::new(label)
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Small)
            .on_press_tx(f)
            .build(k)
            .key(key.to_owned())
    };
    row()
        .key("peek-head")
        .w_full()
        .px(12.0)
        .py(6.0)
        .gap(6.0)
        .items_center()
        .fill(k.c.layer_01)
        .child(
            text(c.title.clone())
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .truncate()
                .shrink()
                .grow(1.0)
                .label(c.key.clone()),
        )
        .child(small("Open in reader  ↵", "peek-open", |s, tx| s.peek_jump(tx)))
        .child(small("Pop out", "peek-popout", |s, _| s.pop_out_peek()))
        .child(small("×", "peek-close", |s, _| {
            s.close_peek();
        }))
}

/// The open peek, hung under its anchor; `None` when closed.
pub(super) fn layer(s: &App, look: &Look) -> Option<El> {
    let k = &look.kit;
    let t = s.brief.peek.open.as_ref()?;
    let c = s.peek_content(t);
    let width = WIDTH.min((s.diff_width - 32.0).max(320.0));
    let card = col()
        .key("peek")
        .role("dialog")
        .label(format!("Peek {}", c.title))
        .w(width)
        .max_h(MAX_HEIGHT)
        .min_h(0.0)
        .clip_rounded(k.radius.xl)
        .on_press(|_: &mut App| {})
        .on_hover_change(|s: &mut App, inside, tx: &mut Tx| s.peek_card_hover(inside, tx))
        .child(head(&c, k))
        .child(el().w_full().h(1.0).fill(k.c.border_weak))
        .child(col().w_full().min_h(0.0).shrink().child(popover::fit_scroll(code(&c, look))));
    let surface = popover::raised(k, card, k.radius.xl);
    Some(popover::anchored(k, surface, &t.anchor, Placement::BottomStart, 6.0))
}
