//! The Tour overlay: a raised card at the reader's bottom-right, where code lines end, so the
//! spotlit lines' starts stay readable. No scrim: the reader's own dimming is the focus, and the
//! reader stays usable.

use rune::{Cursor, El, Tx, col, el, row, text};
use rune_kit::{ButtonVariant, Checkbox, Kit};

use super::ui::{outline, sr};
use super::{STATUS_HEIGHT, TOOLBAR_HEIGHT, muted};
use crate::app::{App, FoundBy, TourStage};
use crate::theme::{TYPE, radius};

const CARD_W: f32 = 440.0;
/// Room at the right for the diff's overview ruler.
const RULER_CLEAR: f32 = 30.0;
/// The status line under the reader.
const STATUS_H: f32 = STATUS_HEIGHT;

pub(super) fn tour(s: &App, k: &Kit) -> Option<El> {
    let card = match s.tour.stage {
        TourStage::Start => start(s, k),
        TourStage::Step => step(s, k)?,
        TourStage::End => end(s, k),
    };
    let panel = col()
        .key("overlay-tour")
        .role("dialog")
        .label("Tour")
        .w(CARD_W)
        .max_h(560.0)
        .gap(8.0)
        .padding(16.0)
        .clip_rounded(radius::ISLAND)
        .on_press(|_: &mut App| {})
        .child(card);
    Some(
        col()
            .key("tour-layer")
            .w_full()
            .h_full()
            .pt(TOOLBAR_HEIGHT + 8.0)
            .pb(STATUS_H + 12.0)
            .pr(RULER_CLEAR)
            .items_end()
            .justify_end()
            .child(rune_kit::popover::raised(k, panel, radius::ISLAND)),
    )
}

fn heading(t: impl Into<String>, k: &Kit) -> El {
    text(t.into())
        .font(rune::tok::TextStyle::new(15.0, 600.0).lh(22.0))
        .color(k.c.text_base)
        .wrap()
}

fn body(t: impl Into<String>, k: &Kit) -> El {
    text(t.into()).font(k.ty.ui).color(k.c.text_base).wrap()
}

/// The step's place, `2 / 6`, in the accent.
fn pill(t: impl Into<String>, key: &str, k: &Kit) -> El {
    let (_, r) = sr(k);
    text(t.into()).key(key.to_owned()).font(TYPE.label).color(r.accent)
}

/// The card's one filled action (`Contrast`), else an outlined one.
fn button(label: &str, key: &str, variant: ButtonVariant, k: &Kit, f: impl Fn(&mut App, &mut Tx) + 'static) -> El {
    let (_, r) = sr(k);
    if variant != ButtonVariant::Contrast {
        return outline(key.to_owned(), label.to_owned(), None, 26.0, k).on_press_tx(f);
    }
    k.interactive_with(
        row()
            .key(key.to_owned())
            .role("button")
            .label(label.to_owned())
            .h(26.0)
            .px(10.0)
            .center()
            .radius(radius::BUTTON)
            .fill(r.accent)
            .cursor(Cursor::Pointer)
            .focusable(true)
            .child(text(label.to_owned()).font(TYPE.small).color(r.on_accent).a11y_hidden()),
        k.c.contrast_hover,
        k.c.contrast_pressed,
    )
    .press_scale(0.97)
    .on_press_tx(f)
}

fn start(s: &App, k: &Kit) -> El {
    let n = s.tour_views().len();
    let title = s
        .brief_doc()
        .map(|d| d.title.clone())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "The brief".into());
    let resume = s.tour.step > 0 && s.tour.step < n && !s.tour.understood.is_empty();
    let mut c = col()
        .key("tour-start")
        .w_full()
        .gap(8.0)
        .child(heading(format!("Tour · {title}"), k))
        .child(muted(format!("{n} steps · ~{} min", s.tour_minutes()), k).key("tour-length"))
        .child(body(
            "Each step points at a range of the code; the rest of the diff dims while you read it.",
            k,
        ))
        .child(
            Checkbox::new(s.tour.my_pass_first)
                .label("My pass first")
                .on_toggle(|s: &mut App| s.toggle_my_pass_first())
                .build(k)
                .key("tour-my-pass"),
        )
        .child(muted("Findings on each step stay hidden until you have read it; Space shows them.", k).wrap());
    let mut actions = row().w_full().gap(8.0).items_center();
    let go = if resume {
        format!("Resume at {}/{n}", s.tour.step + 1)
    } else {
        "Start".to_owned()
    };
    actions = actions
        .child(button(&go, "tour-begin", ButtonVariant::Contrast, k, |s, tx| s.begin_tour(tx)))
        .child(button("Skip to the reader", "tour-skip", ButtonVariant::Ghost, k, |s, _| {
            s.leave_tour();
        }));
    c = c.child(actions);
    c
}

fn step(s: &App, k: &Kit) -> Option<El> {
    let v = s.tour_current()?;
    let n = s.tour_views().len();
    let i = s.tour.step;
    let place = if v.live { "Claude".to_owned() } else { format!("{}/{n}", i + 1) };
    let mut count = row().gap(8.0).items_center().child(pill(place, "tour-count", k));
    if s.tour.understood.contains(&i) && !v.live {
        count = count.child(muted("understood", k));
    }
    let head = col()
        .w_full()
        .gap(2.0)
        .child(count)
        .child(heading(v.title.replace('`', ""), k).w_full());
    let mut c = col().key("tour-step").w_full().gap(8.0).child(head);
    if v.at.as_ref().is_some_and(|a| a.changed) {
        c = c.child(
            row()
                .key("tour-changed")
                .role("status")
                .px(6.0)
                .py(2.0)
                .radius(k.radius.sm)
                .fill(k.c.warning.bg)
                .child(
                    text("Diff changed · the code under this step moved or changed since the tour began")
                        .font(k.ty.small)
                        .color(k.c.warning.fg)
                        .wrap(),
                ),
        );
    }
    if !v.text.trim().is_empty() && v.text != v.title {
        c = c.child(
            col()
                .key("tour-text")
                .w_full()
                .child(super::overview::md_body(s, "tour", &sentences(&v.text, 3), k)),
        );
    }
    let chip = match &v.at {
        Some(a) => {
            let spec = format!("{}:{}-{}", a.key, a.lines.0, a.lines.1);
            let label = rain_overview::brief::cite_labels(&[spec.as_str()], &s.keys).remove(0);
            let cite = rain_core::brief::Cite {
                path: a.key.clone(),
                lines: Some(a.lines),
                side: Some(a.side),
                ..Default::default()
            };
            super::overview::cite_chip(label, a.key.clone(), Some(a.key.clone()), &cite, "tour-cite".into(), k)
        }
        None => muted(format!("{} is not in this diff", v.cite.path), k)
            .wrap()
            .key("tour-cite-missing"),
    };
    c = c.child(row().w_full().gap(6.0).flex_wrap().child(chip));
    c = c.child(findings(s, i, k));
    let mut nav = row()
        .w_full()
        .gap(6.0)
        .items_center()
        .child(button("‹ p", "tour-prev", ButtonVariant::Ghost, k, |s, tx| s.tour_prev(tx)))
        .child(el().grow(1.0));
    let primary = if !v.live && s.tour_findings_hidden(i) && !s.tour_findings(i).is_empty() {
        "Show findings"
    } else if i + 1 >= n {
        "Understood · finish"
    } else {
        "Understood · next"
    };
    nav = nav
        .child(button(primary, "tour-space", ButtonVariant::Contrast, k, |s, tx| s.tour_space(tx)))
        .child(button("n ›", "tour-next", ButtonVariant::Ghost, k, |s, tx| s.tour_next(tx)));
    Some(c.child(nav))
}

/// "Findings at this step": a count while hidden, else who found what.
fn findings(s: &App, i: usize, k: &Kit) -> El {
    let f = s.tour_findings(i);
    let (surf, _) = sr(k);
    let mut c = col()
        .key("tour-findings")
        .w_full()
        .gap(6.0)
        .padding(10.0)
        .radius(radius::CARD)
        .fill(surf.well);
    if f.is_empty() {
        return c.child(muted("No findings on this range", k));
    }
    if s.tour_findings_hidden(i) && s.tour.live.is_none() {
        return c.child(
            muted(
                format!("{} finding(s) here, hidden until you have read it · Space shows them", f.len()),
                k,
            )
            .wrap()
            .key("tour-findings-hidden"),
        );
    }
    c = c.child(text("Findings at this step").font(k.ty.small_medium).color(k.c.text_muted));
    for x in f.iter().take(4) {
        let (fg, bg) = match x.who {
            FoundBy::Agent => (k.c.info.fg, k.c.info.bg),
            FoundBy::You => (k.c.success.fg, k.c.success.bg),
            FoundBy::Both => (k.c.warning.fg, k.c.warning.bg),
        };
        let id = x.id.clone();
        c = c.child(
            row()
                .key(format!("tour-finding-{}", x.id))
                .role("button")
                .w_full()
                .gap(6.0)
                .items_center()
                .on_press_tx(move |s: &mut App, tx: &mut Tx| s.goto_thread(&id, tx))
                .child(
                    row()
                        .px(5.0)
                        .radius(k.radius.sm)
                        .fill(bg)
                        .child(text(x.who.label()).font(k.ty.small).color(fg)),
                )
                .child(text(x.body.clone()).font(k.ty.small).color(k.c.text_base).truncate().shrink()),
        );
    }
    if f.len() > 4 {
        c = c.child(muted(format!("+{} more", f.len() - 4), k));
    }
    c
}

fn end(s: &App, k: &Kit) -> El {
    let n = s.tour_views().len();
    let left = s.tour_uncovered();
    let mut c = col()
        .key("tour-end")
        .w_full()
        .gap(8.0)
        .child(heading("Tour done", k))
        .child(muted(format!("{}/{n} steps understood", s.tour.understood.len()), k))
        .child(
            text(format!("Not covered by the tour: {} file(s)", left.len()))
                .key("tour-uncovered-count")
                .font(k.ty.ui_medium)
                .color(k.c.text_base),
        );
    let mut list = col().key("tour-uncovered").w_full().gap(1.0);
    for &g in left.iter().take(10) {
        let key = s.keys[g].clone();
        let (dir, name) = key.rsplit_once('/').unwrap_or(("", key.as_str()));
        list = list.child(
            k.interactive(
                row()
                    .key(format!("tour-uncovered-{key}"))
                    .role("button")
                    .w_full()
                    .px(6.0)
                    .py(3.0)
                    .gap(8.0)
                    .items_center()
                    .radius(k.radius.sm),
            )
            .on_press_tx(move |s: &mut App, tx: &mut Tx| s.tour_jump(g, tx))
            .child(text(name.to_owned()).font(k.ty.ui).color(k.c.text_base))
            .child(text(dir.to_owned()).font(k.ty.small).color(k.c.text_faint).truncate().shrink()),
        );
    }
    if left.len() > 10 {
        list = list.child(muted(format!("+{} more in the list", left.len() - 10), k));
    }
    c = c.child(list);
    c.child(
        row()
            .w_full()
            .gap(8.0)
            .child(button("◀ Last step (p)", "tour-back", ButtonVariant::Ghost, k, |s, tx| {
                s.tour_prev(tx)
            }))
            .child(el().grow(1.0))
            .child(button("Close", "tour-close", ButtonVariant::Contrast, k, |s, _| {
                s.leave_tour();
            })),
    )
}

/// The first `n` sentences of `t`.
fn sentences(t: &str, n: usize) -> String {
    let mut count = 0;
    for (i, c) in t.char_indices() {
        if matches!(c, '.' | '!' | '?') && t[i + 1..].starts_with(char::is_whitespace) {
            count += 1;
            if count == n {
                return t[..=i].to_owned();
            }
        }
    }
    t.to_owned()
}
