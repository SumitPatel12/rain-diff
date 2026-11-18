//! What the narrated styles share: a caption per beat that crossfades into the next, marks that
//! dim with the timeline's focus and the pointer's highlight, and boxed animations for a rig.

use rune::{El, col, el, text};

use super::paint::{LABEL, STRONG, TITLE};
use crate::motion::{self, Animate, Rig, Track};
use crate::style::Cx;

/// One beat's caption: a title, a sentence, and its opacity on the timeline.
pub(crate) struct Caption {
    pub title: String,
    pub text: String,
    pub alpha: Track<f32>,
}

/// Captions for beats starting at each `t`: the one before fades out, then this one fades in,
/// so two never show over each other.
pub(crate) fn captions(beats: Vec<(f32, String, String)>) -> Vec<Caption> {
    let ends: Vec<Option<f32>> = (0..beats.len()).map(|i| beats.get(i + 1).map(|b| b.0)).collect();
    beats
        .into_iter()
        .zip(ends)
        .map(|((s, title, text), e)| {
            let mut k = if s <= 0.0 {
                vec![(0.0, 1.0)]
            } else {
                vec![(s - 0.15, 0.0), (s, 1.0)]
            };
            if let Some(e) = e {
                k.push((e - 0.3, 1.0));
                k.push((e - 0.15, 0.0));
            }
            Caption {
                title,
                text,
                alpha: Track::new(k),
            }
        })
        .collect()
}

/// The captions stacked in one place, `w` wide, at `at`.
pub(crate) fn caption_els(cx: &Cx, id: &str, caps: &[Caption], at: [f32; 2], w: f32) -> Vec<El> {
    caps.iter()
        .enumerate()
        .map(|(i, c)| {
            col()
                .key(format!("{id}-caption-{i}"))
                .absolute(at[0], at[1])
                .w(w)
                .gap(4.0)
                .child(text(c.title.clone()).font(TITLE).color(cx.ink.foreground).a11y_hidden())
                .child(text(c.text.clone()).font(LABEL).color(cx.ink.muted).wrap().a11y_hidden())
                .live_opacity(&c.alpha.live)
                .a11y_hidden()
        })
        .collect()
}

/// What the diagram tells you, in a sentence or two worked out from the sample, `w` wide, in a
/// faint well.
pub(crate) fn conclusion(cx: &Cx, id: &str, says: &str, w: f32) -> El {
    rune::row()
        .key(format!("{id}-conclusion"))
        .w(w)
        .px(12.0)
        .py(6.0)
        .radius(6.0)
        .fill(cx.ink.foreground.alpha(0.05))
        .role("note")
        .label(says.to_owned())
        .child(
            text(says.to_owned())
                .font(STRONG)
                .color(cx.ink.foreground)
                .wrap()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .a11y_hidden(),
        )
}

/// "1 part", "3 parts".
pub(crate) fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `e` (placed in a `size` canvas) dimmed when the timeline focuses, or the pointer lights,
/// something other than `id`.
pub(crate) fn layered(cx: &Cx, rig: &Rig, id: &str, e: El) -> El {
    let [w, h] = cx.size;
    let inner = motion::fade(
        el().absolute(0.0, 0.0).size(w, h).child(e).a11y_hidden(),
        &rig.focus.opacity(id),
        cx.reduced,
    );
    motion::fade(
        el().absolute(0.0, 0.0).size(w, h).child(inner).a11y_hidden(),
        &rig.highlight.opacity(id),
        cx.reduced,
    )
}

/// Animations collected before the rig exists, added to it in one go.
#[derive(Default)]
pub(crate) struct Tracks(Vec<Box<dyn Animate>>);

impl Tracks {
    pub fn push(&mut self, a: impl Animate + 'static) {
        self.0.push(Box::new(a));
    }

    pub fn onto(self, rig: &mut Rig) {
        for a in self.0 {
            rig.add(Boxed(a));
        }
    }
}

struct Boxed(Box<dyn Animate>);

impl Animate for Boxed {
    fn publish(&self, t: f32) {
        self.0.publish(t);
    }
}
