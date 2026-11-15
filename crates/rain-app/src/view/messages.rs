//! Message primitives every feature uses: a strip across the reader island in one of four
//! tones, and a snackbar with at most one action (toasts live in the Conversation module).

use rune::{Cursor, El, Tx, el, row, text};
use rune_kit::Kit;

use super::icons;
use super::ui::{outline, sr};
use crate::app::App;
use crate::theme::{TYPE, radius};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code, reason = "tones the round-3 features pick")]
pub enum Tone {
    Info,
    Warning,
    Danger,
    Success,
}

/// A message's action: a label and what it runs.
pub type Action = (&'static str, fn(&mut App, &mut Tx));

fn tone_colors(t: Tone, k: &Kit) -> (rune::Color, rune::Color) {
    match t {
        Tone::Info => (k.c.info.bg, k.c.info.fg),
        Tone::Warning => (k.c.warning.bg, k.c.warning.fg),
        Tone::Danger => (k.c.danger.bg, k.c.danger.fg),
        Tone::Success => (k.c.success.bg, k.c.success.fg),
    }
}

fn tone_icon(t: Tone) -> &'static rune::Icon {
    match t {
        Tone::Info => &icons::INFO,
        Tone::Warning => &icons::WARN,
        Tone::Danger => &icons::WARN,
        Tone::Success => &icons::RELOAD,
    }
}

/// A quiet action: muted text, the hover overlay.
fn quiet_button(key: String, label: &str, k: &Kit) -> El {
    k.interactive(
        row()
            .key(key)
            .role("button")
            .label(label.to_owned())
            .h(24.0)
            .px(8.0)
            .items_center()
            .radius(radius::SMALL)
            .cursor(Cursor::Pointer)
            .focusable(true)
            .child(text(label.to_owned()).font(TYPE.small).color(k.c.text_muted).a11y_hidden()),
    )
}

/// A full-width strip above the diff (new commits, a reload, a load problem): the tone's tint,
/// its icon, the message, then the first action outlined and any second one quiet.
pub(crate) fn banner(key: &str, msg: String, tone: Tone, actions: &[Action], k: &Kit) -> El {
    let (surf, _) = sr(k);
    let (bg, fg) = tone_colors(tone, k);
    let mut r = row()
        .key(key.to_owned())
        .role("status")
        .label(msg.clone())
        .w_full()
        .min_h(34.0)
        .pl(12.0)
        .pr(10.0)
        .py(4.0)
        .gap(10.0)
        .items_center()
        .fill(bg)
        .child(icons::draw(tone_icon(tone), 16.0, fg))
        .child(
            text(msg)
                .font(TYPE.small)
                .color(k.c.text_base)
                .truncate()
                .grow(1.0)
                .shrink()
                .a11y_hidden(),
        );
    for (i, &(label, f)) in actions.iter().enumerate() {
        let b = if i == 0 {
            outline(format!("{key}-action"), label, None, 24.0, k)
        } else {
            quiet_button(format!("{key}-action-{i}"), label, k)
        };
        r = r.child(b.on_press_tx(f));
    }
    r.child(
        el().pin(Some(0.0), None, Some(0.0), Some(0.0))
            .h(1.0)
            .fill(surf.hairline)
            .a11y_hidden(),
    )
}

/// A rounded box in its tone, inset in the reader (a load problem, a stale brief, a round).
pub(crate) fn inset_banner(key: &str, msg: String, tone: Tone, actions: &[Action], k: &Kit) -> El {
    let (bg, fg) = tone_colors(tone, k);
    let ring = match tone {
        Tone::Info => k.c.info.border,
        Tone::Warning => k.c.warning.border,
        Tone::Danger => k.c.danger.border,
        Tone::Success => k.c.success.border,
    };
    let mut r = row()
        .key(key.to_owned())
        .role("status")
        .label(msg.clone())
        .w_full()
        .min_h(34.0)
        .px(12.0)
        .py(6.0)
        .gap(10.0)
        .items_center()
        .radius(radius::BUTTON)
        .fill(bg)
        .border(1.0, ring)
        .child(icons::draw(tone_icon(tone), 16.0, fg))
        .child(
            text(msg)
                .font(TYPE.small)
                .color(k.c.text_base)
                .wrap()
                .grow(1.0)
                .shrink()
                .a11y_hidden(),
        );
    for (i, &(label, f)) in actions.iter().enumerate() {
        let b = if i == 0 {
            outline(format!("{key}-action"), label, None, 24.0, k)
        } else {
            quiet_button(format!("{key}-action-{i}"), label, k)
        };
        r = r.child(b.on_press_tx(f));
    }
    row().w_full().px(10.0).pt(8.0).child(r)
}

/// A one-line note that the next one replaces: inverted (text on the ground's colour), with
/// one bold action.
pub(crate) fn snackbar(key: &str, msg: String, action: Option<Action>, k: &Kit) -> El {
    let (surf, r) = sr(k);
    let mut b = row()
        .key(key.to_owned())
        .role("status")
        .label(msg.clone())
        .h(38.0)
        .pl(14.0)
        .pr(8.0)
        .gap(14.0)
        .items_center()
        .radius(radius::CARD)
        .fill(k.c.text_base)
        .shadow(12.0, 30.0, r.shadow)
        .child(text(msg).font(TYPE.ui).color(surf.bar).truncate().shrink().a11y_hidden());
    if let Some((label, f)) = action {
        b = b.child(
            row()
                .key(format!("{key}-action"))
                .role("button")
                .label(label)
                .h(28.0)
                .px(8.0)
                .items_center()
                .radius(radius::SMALL)
                .cursor(Cursor::Pointer)
                .hover(surf.bar.alpha(0.08))
                .child(text(label).font(TYPE.strong).color(r.on_accent).a11y_hidden())
                .on_press_tx(f),
        );
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_build_in_every_scheme() {
        for scheme in crate::theme::Scheme::ALL {
            let k = crate::theme::look(scheme).kit;
            let go: Action = ("Reload", |s: &mut App, tx: &mut Tx| s.reload_now(tx));
            for t in [Tone::Info, Tone::Warning, Tone::Danger, Tone::Success] {
                let _ = banner("b", "3 new commits".into(), t, &[go], &k);
            }
            let _ = snackbar("s", "Viewed".into(), Some(go), &k);
        }
    }
}
