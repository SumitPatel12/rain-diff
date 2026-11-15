//! Message primitives every feature uses: a banner across the reader and a snackbar with at
//! most one action (toasts live in the Conversation module).

use rune::{El, Tx, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit};

use crate::app::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code, reason = "tones the round-3 features pick")]
pub enum Tone {
    Info,
    Warning,
    Danger,
    Success,
}

/// A message's one action: a label and what it runs.
pub type Action = (&'static str, fn(&mut App, &mut Tx));

fn tone_colors(t: Tone, k: &Kit) -> (rune::Color, rune::Color) {
    match t {
        Tone::Info => (k.c.info.bg, k.c.info.fg),
        Tone::Warning => (k.c.warning.bg, k.c.warning.fg),
        Tone::Danger => (k.c.danger.bg, k.c.danger.fg),
        Tone::Success => (k.c.success.bg, k.c.success.fg),
    }
}

fn action_button(key: &str, a: Action, k: &Kit) -> El {
    let (label, f) = a;
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .on_press_tx(f)
        .build(k)
        .key(format!("{key}-action"))
}

/// A full-width strip above the diff (stale data, a load problem, new commits).
pub(crate) fn banner(key: &str, msg: String, tone: Tone, action: Option<Action>, k: &Kit) -> El {
    let (bg, fg) = tone_colors(tone, k);
    let mut r = row()
        .key(key.to_owned())
        .role("status")
        .label(msg.clone())
        .w_full()
        .px(12.0)
        .py(6.0)
        .gap(10.0)
        .items_center()
        .fill(bg)
        .child(text(msg).font(k.ty.small).color(fg).truncate().grow(1.0).shrink().a11y_hidden());
    if let Some(a) = action {
        r = r.child(action_button(key, a, k));
    }
    r
}

/// A one-line note at the bottom of the reader that the next one replaces.
pub(crate) fn snackbar(key: &str, msg: String, action: Option<Action>, k: &Kit) -> El {
    let mut r = row()
        .key(key.to_owned())
        .role("status")
        .label(msg.clone())
        .px(14.0)
        .py(8.0)
        .gap(12.0)
        .items_center()
        .radius(k.radius.md)
        .fill(k.c.layer_03)
        .border(1.0, k.c.border_base)
        .child(text(msg).font(k.ty.ui).color(k.c.text_base).a11y_hidden());
    if let Some(a) = action {
        r = r.child(action_button(key, a, k));
    }
    k.elevate(r, &k.elevation.floating)
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
                let _ = banner("b", "3 new commits".into(), t, Some(go), &k);
            }
            let _ = snackbar("s", "Viewed".into(), Some(go), &k);
        }
    }
}
