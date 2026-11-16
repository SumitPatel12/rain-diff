//! The cite peek: a floating card under a cite with the cited lines as the diff draws them
//! (numbers, tints, word changes, syntax colours), and the same lines in the pop-out window.

use rain_core::LineKind;
use rune::{Color, El, Span, TextStyle, Tx, col, el, row, text};
use rune_kit::syntax::Paint;
use rune_kit::{Kit, Placement, popover};

use super::icons;
use super::ui::{Ghost, outline, sr};
use crate::app::{App, PeekContent};
use crate::theme::{Look, TYPE, radius};

const MAX_HEIGHT: f32 = 420.0;
const ROW_H: f32 = 20.0;

struct Tints {
    line: Color,
    gutter: Color,
    word: Color,
    sign: Color,
}

/// The diff's fills for a changed line, from the scheme's roles.
fn tints(look: &Look, kind: LineKind) -> Option<Tints> {
    let (k, r) = (&look.kit, &look.r);
    let (line, word, sign) = match kind {
        LineKind::Added => (r.add_bg, r.add_word, k.c.success.fg),
        LineKind::Removed => (r.del_bg, r.del_word, k.c.danger.fg),
        LineKind::Context => return None,
    };
    Some(Tints {
        line,
        gutter: line,
        word,
        sign,
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
    let num_font = TextStyle {
        size: 12.0,
        line_height: ROW_H,
        ..k.ty.mono
    };
    let joined = c.rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>().join("\n");
    let palette = look.scheme.syntax();
    let highlights = rain_syntax::highlight(palette, c.lang.as_deref(), &joined);
    let theme = look.scheme.code_kinds(k);
    // Rows take their natural width (the longest line) and stretch to the widest, so the code
    // scrolls sideways instead of being cut.
    let mut out = col().key("peek-code").py(4.0);
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
                    .pl(12.0)
                    .items_center()
                    .fill(look.s.well)
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
        let (sign, sign_color) = match r.kind {
            LineKind::Added => ("+", t.as_ref().map_or(k.c.text_faint, |t| t.sign)),
            LineKind::Removed => ("−", t.as_ref().map_or(k.c.text_faint, |t| t.sign)),
            LineKind::Context => (" ", k.c.text_faint),
        };
        let ink = if r.cited { k.c.text_base } else { k.c.text_faint };
        let number = |n: u32| {
            row()
                .w(38.0)
                .h_full()
                .pr(8.0)
                .justify_end()
                .items_center()
                .child(text(if n == 0 { String::new() } else { n.to_string() }).font(num_font).color(ink))
        };
        let gutter = row()
            .h_full()
            .fill(t.as_ref().map_or(k.c.bg_base, |t| t.gutter))
            .child(el().w(14.0).a11y_hidden())
            .child(number(r.old))
            .child(number(r.new));
        out = out.child(
            row()
                .key(format!("peek-row-{i}"))
                .h(ROW_H)
                .pr(16.0)
                .fill(t.as_ref().map_or(k.c.bg_base, |t| t.line))
                .child(gutter)
                .child(text(sign).font(font).color(sign_color).w(18.0).pl(4.0))
                .child(
                    text(r.text.replace('\t', "    ").clone())
                        .font(font)
                        .color(k.c.text_base)
                        .spans(if r.text.contains('\t') { Vec::new() } else { spans })
                        .single_line(),
                ),
        );
        start = end + 1;
    }
    out
}

/// `server · src/Services/VoucherService.cs :25-33`: the folder muted, the name strong.
fn place(c: &PeekContent, k: &Kit) -> El {
    let (path, lines) = c.title.rsplit_once(':').map_or((c.title.as_str(), ""), |(p, l)| (p, l));
    let full = if c.key.ends_with(path) { c.key.as_str() } else { path };
    let (dir, name) = full.rsplit_once('/').map_or(("", full), |(d, n)| (d, n));
    let dir = if dir.is_empty() {
        String::new()
    } else {
        format!("{}/", dir.replacen('/', " · ", 1))
    };
    row()
        .key("peek-place")
        .label(c.key.clone())
        .gap(0.0)
        .items_center()
        .shrink()
        .min_w(0.0)
        .child(text(dir).font(TYPE.small).color(k.c.text_muted).truncate_head().shrink().min_w(0.0))
        .child(text(name.to_owned()).font(TYPE.label).color(k.c.text_base))
        .child(
            text(if lines.is_empty() { String::new() } else { format!(" :{lines}") })
                .font(TYPE.micro_mono)
                .color(k.c.text_faint),
        )
}

/// The card's head: where the lines are, and close.
fn head(c: &PeekContent, k: &Kit) -> El {
    row()
        .key("peek-head")
        .w_full()
        .h(34.0)
        .pl(12.0)
        .pr(6.0)
        .gap(8.0)
        .items_center()
        .child(icons::draw(&icons::FILE, 14.0, k.c.text_muted))
        .child(place(c, k).grow(1.0))
        .child(
            Ghost::new("peek-close")
                .icon(&icons::CLOSE)
                .help("Close  Esc")
                .height(24.0)
                .build(k)
                .on_press(|s: &mut App| {
                    s.close_peek();
                }),
        )
}

/// The card's foot: Open in reader, Pop out, and the keys.
fn foot(k: &Kit) -> El {
    let (surf, r) = sr(k);
    row()
        .key("peek-foot")
        .w_full()
        .h(38.0)
        .px(12.0)
        .gap(8.0)
        .items_center()
        .fill(surf.card_head)
        .child(
            k.interactive_with(
                row()
                    .key("peek-open")
                    .role("button")
                    .label("Open in reader")
                    .h(24.0)
                    .px(8.0)
                    .gap(6.0)
                    .center()
                    .radius(radius::SMALL)
                    .fill(r.accent)
                    .cursor(rune::Cursor::Pointer)
                    .help("Open in reader  Enter")
                    .child(text("Open in reader").font(TYPE.small).color(r.on_accent).a11y_hidden()),
                k.c.contrast_hover,
                k.c.contrast_pressed,
            )
            .on_press_tx(|s: &mut App, tx: &mut Tx| s.peek_jump(tx)),
        )
        .child(
            outline("peek-popout", "Pop out", None, 24.0, k)
                .child(icons::draw(&icons::POPOUT, 14.0, k.c.text_muted))
                .on_press(|s: &mut App| s.pop_out_peek()),
        )
        .child(el().grow(1.0).a11y_hidden())
        .child(text("Space on a chip peeks · Enter jumps").font(TYPE.small).color(k.c.text_faint))
}

/// The open peek, hung under its anchor; `None` when closed.
pub(super) fn layer(s: &App, look: &Look) -> Option<El> {
    let k = &look.kit;
    let t = s.brief.peek.open.as_ref()?;
    let c = s.peek_content(t);
    // The reader's width, so long lines get room; anything longer scrolls sideways.
    let width = (s.diff_width - 32.0).max(320.0);
    let card = col()
        .key("peek")
        .role("dialog")
        .label(format!("Peek {}", c.title))
        .w(width)
        .max_h(MAX_HEIGHT)
        .min_h(0.0)
        .clip_rounded(radius::CARD)
        .on_press(|_: &mut App| {})
        .on_hover_change(|s: &mut App, inside, tx: &mut Tx| s.peek_card_hover(inside, tx))
        .child(head(&c, k))
        .child(el().w_full().h(1.0).fill(k.c.border_weak))
        .child(
            col()
                .key("peek-scroll")
                .w_full()
                .min_h(0.0)
                .shrink()
                .scroll_xy()
                .child(code(&c, look).min_w(width)),
        )
        .child(el().w_full().h(1.0).fill(k.c.border_weak))
        .child(foot(k));
    let surface = popover::raised(k, card, radius::CARD);
    Some(popover::anchored(k, surface, &t.anchor, Placement::BottomStart, 6.0))
}
