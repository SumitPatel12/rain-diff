//! Small parts the redesign repeats: key caps, ghost and outlined buttons, chips, separators.

use std::ops::Range;
use std::rc::Rc;

use rune::{Color, Cursor, El, Icon, Key, KeyEvent, Mods, TextHighlight, Tx, el, row, text};
use rune_kit::Kit;

use super::icons;
use crate::app::App;
use crate::theme::{Roles, Surfaces, TYPE, radius, roles_of, surfaces_of};

/// A button's press that Enter fires too while the button has focus. Rune gives Enter on a
/// focused button to the default button and then the window's `on_key`, as AppKit does; only
/// Space presses it.
pub(crate) trait Activate {
    fn on_activate(self, f: impl Fn(&mut App) + 'static) -> El;
    fn on_activate_tx(self, f: impl Fn(&mut App, &mut Tx) + 'static) -> El;
}

impl Activate for El {
    fn on_activate(self, f: impl Fn(&mut App) + 'static) -> El {
        self.on_activate_tx(move |s: &mut App, _: &mut Tx| f(s))
    }

    fn on_activate_tx(self, f: impl Fn(&mut App, &mut Tx) + 'static) -> El {
        let f = Rc::new(f);
        let press = f.clone();
        self.on_press_tx(move |s: &mut App, tx: &mut Tx| press(s, tx))
            .on_key(move |s: &mut App, ev: &KeyEvent, tx: &mut Tx| {
                if ev.key != Key::Enter || ev.mods != Mods::default() || s.holds_every_key() {
                    return false;
                }
                if !tx.key_repeat() {
                    f(s, tx);
                }
                true
            })
    }
}

pub(crate) fn sr(k: &Kit) -> (Surfaces, Roles) {
    (surfaces_of(k), roles_of(k))
}

/// A key cap: `Space`, `]`, `i` (11/16 faint on the well, hairline ring).
pub(crate) fn kbd(t: impl Into<String>, k: &Kit) -> El {
    let s = surfaces_of(k);
    let t = t.into();
    // Letters with no ascender (`v`, `w`, `a`) fill only the lower part of the line; lift them a
    // point so they read as centred in the cap.
    let low = !t.is_empty() && t.chars().all(|c| "acemnorsuvwxz".contains(c));
    row()
        .h(16.0)
        .px(4.0)
        .pb(if low { 2.0 } else { 0.0 })
        .items_center()
        .radius(radius::KEY)
        .fill(s.well)
        .border(1.0, s.hairline)
        .a11y_hidden()
        // A 14 point line inside the 1 point ring, so the label sits in the cap's middle.
        .child(text(t).font(TYPE.micro.lh(14.0)).color(k.c.text_faint))
}

/// A key without its cap, for inside buttons and menu rows.
pub(crate) fn kbd_bare(t: impl Into<String>, k: &Kit) -> El {
    // On the label's 18 point line, so `c` and `?` sit on the same baseline as `Comment`.
    text(t.into())
        .font(TYPE.micro.lh(TYPE.small.line_height))
        .color(k.c.text_faint)
        .a11y_hidden()
}

/// Markdown in Rain's type: headings 18 / 16 / 16 semibold over 14 point body text.
pub(crate) fn md_style(k: &Kit) -> rune_kit::MarkdownStyle {
    use rune::tok::TextStyle;
    rune_kit::MarkdownStyle::from_kit(k)
        .heading_font(1, TextStyle::new(18.0, 600.0).lh(26.0))
        .heading_font(2, TextStyle::new(16.0, 600.0).lh(24.0))
        .heading_font(3, TextStyle::new(16.0, 600.0).lh(24.0))
}

/// The ink of the letters a query matched: the one hue that reads on a row, a hovered row and
/// the selected row in every scheme, and the hue find's hits have in the diff.
pub(crate) fn match_ink(k: &Kit) -> Color {
    k.c.warning.fg
}

/// A row's label with the letters its query matched (`marks`, byte ranges) in [`match_ink`].
/// Painted over the shaped text: a styled span could shape the label again and move its glyphs.
pub(crate) fn matched(label: impl Into<String>, marks: &[Range<usize>], k: &Kit) -> El {
    let ink = match_ink(k);
    text(label).font(TYPE.ui).color(k.c.text_base).text_highlights(
        marks
            .iter()
            .map(|r| TextHighlight::new(r.clone(), Color::TRANSPARENT).color(ink).pad(0.0)),
    )
}

/// A 1 × 16 point divider between toolbar groups.
pub(crate) fn vsep(k: &Kit) -> El {
    el().w(1.0).h(16.0).mx(4.0).fill(surfaces_of(k).hairline).a11y_hidden()
}

/// A ghost button: icon and/or label in muted 12 point text, the hover overlay, `on` pressed.
pub(crate) struct Ghost {
    key: String,
    icon: Option<&'static Icon>,
    label: Option<String>,
    hint: Option<String>,
    help: Option<String>,
    on: bool,
    height: f32,
    extra: Vec<El>,
}

impl Ghost {
    pub(crate) fn new(key: impl Into<String>) -> Ghost {
        Ghost {
            key: key.into(),
            icon: None,
            label: None,
            hint: None,
            help: None,
            on: false,
            height: 28.0,
            extra: Vec::new(),
        }
    }

    pub(crate) fn icon(mut self, i: &'static Icon) -> Self {
        self.icon = Some(i);
        self
    }

    pub(crate) fn label(mut self, l: impl Into<String>) -> Self {
        self.label = Some(l.into());
        self
    }

    /// A key shown after the label, without a cap.
    pub(crate) fn hint(mut self, k: impl Into<String>) -> Self {
        self.hint = Some(k.into());
        self
    }

    pub(crate) fn help(mut self, h: impl Into<String>) -> Self {
        self.help = Some(h.into());
        self
    }

    pub(crate) fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    pub(crate) fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    pub(crate) fn child(mut self, e: El) -> Self {
        self.extra.push(e);
        self
    }

    pub(crate) fn build(self, k: &Kit) -> El {
        let ink = if self.on { k.c.text_base } else { k.c.text_muted };
        let name = self.label.clone().or(self.help.clone()).unwrap_or_default();
        let mut b = row()
            .key(self.key)
            .role("button")
            .label(name)
            .h(self.height)
            .min_w(self.height)
            .px(6.0)
            .gap(6.0)
            .center()
            .radius(radius::BUTTON)
            .cursor(Cursor::Pointer)
            .focusable(true);
        if self.on {
            b = b.fill(k.c.pressed).checked(true);
        }
        // Keys live in the tooltip only, never on the button's face.
        let help = match (self.help, &self.hint) {
            (Some(h), Some(key)) => Some(format!("{h}  {key}")),
            (Some(h), _) => Some(h),
            (None, Some(key)) => self.label.as_ref().map(|l| format!("{l}  {key}")),
            (None, None) => None,
        };
        if let Some(h) = help {
            b = b.help(h);
        }
        if let Some(i) = self.icon {
            b = b.child(icons::draw(i, 16.0, k.c.text_base));
        }
        if let Some(l) = self.label {
            b = b.child(text(l).font(TYPE.small).color(ink).a11y_hidden());
        }
        k.interactive(b.children(self.extra)).press_scale(0.97)
    }
}

/// An outlined small button (`Accept 2`, thread actions): 12 point text, a card-border ring.
pub(crate) fn outline(key: impl Into<String>, label: impl Into<String>, hint: Option<&str>, h: f32, k: &Kit) -> El {
    let s = surfaces_of(k);
    let label = label.into();
    let mut b = row()
        .key(key.into())
        .role("button")
        .label(label.clone())
        .h(h)
        .px(8.0)
        .gap(6.0)
        .center()
        .radius(radius::SMALL)
        .border(1.0, s.card_border)
        .cursor(Cursor::Pointer)
        .focusable(true)
        .child(text(label.clone()).font(TYPE.small).color(k.c.text_base).a11y_hidden());
    if let Some(h) = hint {
        b = b.help(format!("{label}  {h}"));
    }
    k.interactive(b).press_scale(0.97)
}

/// The one filled button of a context: accent fill, 13/20/600.
pub(crate) fn primary(key: impl Into<String>, label: impl Into<String>, k: &Kit) -> El {
    let r = roles_of(k);
    let label = label.into();
    k.interactive_with(
        row()
            .key(key.into())
            .role("button")
            .label(label.clone())
            .h(28.0)
            .px(14.0)
            .center()
            .radius(radius::BUTTON)
            .fill(r.accent)
            .cursor(Cursor::Pointer)
            .focusable(true)
            .child(text(label).font(TYPE.strong).color(r.on_accent).a11y_hidden()),
        k.c.contrast_hover,
        k.c.contrast_pressed,
    )
    .press_scale(0.97)
}

/// A triage group's one letter and its colour: M Must review, R Review, S Skim, T Tests,
/// V Moves, F Formatting-only, G Generated, L Lockfile, H Huge, B Binary, I Ignored.
pub(crate) fn group_mark(g: rain_triage::Group, k: &Kit) -> (&'static str, rune::Color) {
    use rain_triage::Group::*;
    let r = roles_of(k);
    match g {
        Must => ("M", k.c.danger.fg),
        Review => ("R", k.c.info.fg),
        Skim => ("S", k.c.text_muted),
        Tests => ("T", k.c.success.fg),
        Moves => ("V", r.note),
        FormattingOnly => ("F", k.c.warning.fg),
        Generated => ("G", k.c.text_faint),
        Lockfile => ("L", k.c.text_faint),
        Huge => ("H", k.c.warning.fg),
        Binary => ("B", k.c.text_faint),
        Ignored => ("I", k.c.text_faint),
    }
}

/// A triage group as a one-letter tag, 16 points square, its full name on hover.
pub(crate) fn group_tag(g: rain_triage::Group, k: &Kit) -> El {
    let (letter, ink) = group_mark(g, k);
    row()
        .key("group-tag")
        .size(16.0, 16.0)
        .center()
        .radius(radius::KEY)
        .fill(k.alpha(ink, 0.16))
        .border(1.0, k.alpha(ink, 0.45))
        .help(g.label().to_owned())
        .label(g.label().to_owned())
        .child(
            text(letter)
                .font(rune::tok::TextStyle::new(10.0, 700.0).lh(14.0))
                .color(ink)
                .a11y_hidden(),
        )
}
