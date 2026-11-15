//! The samples in both palettes and modes. `cargo run -p rain-syntax --example syntax -- --scene light --snap out.png`

use rain_syntax::{Mode, Palette, highlighter_with, samples, warm};
use rune::tok::{TextStyle, type_};
use rune::{AppSpec, Cx, El, col, row, text};
use rune_kit::Kit;
use rune_kit::syntax::SyntaxTheme;

const CODE: TextStyle = TextStyle::new(12.0, 400.0).mono();

struct Demo {
    dark: Option<bool>,
    mode: Mode,
}

fn sample(k: &Kit, mode: Mode, palette: Palette, lang: &str, code: &str) -> El {
    let hl = highlighter_with(mode, palette).highlight(Some(lang), code);
    let spans = SyntaxTheme::from_kit(k).spans(&hl, 0);
    col()
        .gap(4.0)
        .padding(10.0)
        .fill(k.c.layer_01)
        .child(text(lang).font(type_::BODY).color(k.c.text_muted))
        .child(text(code.trim_end()).font(CODE).color(k.c.text_base).spans(spans))
}

fn view(s: &Demo, cx: &mut Cx<Demo>) -> El {
    let dark = s.dark.unwrap_or(cx.system_dark());
    let k = Kit::neutral(dark);
    let palette = Palette::for_dark(dark);
    let [cs, rest @ ..] = samples::ALL;
    let others = col()
        .gap(8.0)
        .children(rest.iter().map(|(lang, code)| sample(&k, s.mode, palette, lang, code)));
    let body = row().gap(8.0).child(sample(&k, s.mode, palette, cs.0, cs.1)).child(others);
    let page = col()
        .gap(8.0)
        .padding(12.0)
        .child(
            text(format!("rain-syntax  {palette:?}  {:?}", s.mode))
                .font(type_::TITLE)
                .color(k.c.text_base),
        )
        .child(body);
    col()
        .key("page")
        .w_full()
        .h_full()
        .fill(k.c.bg_base)
        .scroll(rune::ScrollSpec { stick_bottom: false })
        .child(page)
}

fn main() {
    warm();
    rune::run(AppSpec {
        scenes: &[
            ("dark", |s: &mut Demo| s.dark = Some(true)),
            ("light", |s: &mut Demo| s.dark = Some(false)),
            ("rich-dark", |s: &mut Demo| (s.dark, s.mode) = (Some(true), Mode::Rich)),
            ("rich-light", |s: &mut Demo| (s.dark, s.mode) = (Some(false), Mode::Rich)),
        ],
        ..AppSpec::new(
            "rain-syntax",
            (1400.0, 1000.0),
            || Demo {
                dark: None,
                mode: Mode::Fast,
            },
            view,
        )
    });
}
