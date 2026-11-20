//! The Finish review dialog, the strips above the reader that rounds and reloads add (new
//! commits banner, reload message, round picker) and a file header's "changed since you viewed".

use rain_core::Verdict;
use rune::{Cursor, El, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, SegmentedControl, TextArea};

use super::icons;
use super::messages::{self, Tone};
use super::ui::{Activate, outline, sr};
use crate::app::{App, VERDICTS, verdict_label};
use crate::theme::{TYPE, radius};

/// One verdict as a large toggle: its icon and name, tinted by tone when picked.
fn verdict_button(s: &App, v: Verdict, k: &Kit) -> El {
    let (surf, _) = sr(k);
    let on = s.review.verdict == Some(v);
    let (icon, tone) = match v {
        Verdict::Approve => (&icons::CHECK, k.c.success),
        Verdict::RequestChanges => (&icons::RELOAD, k.c.warning),
        Verdict::Comment => (&icons::CHAT, k.c.info),
    };
    let ink = if on { tone.fg } else { k.c.text_base };
    let mut b = row()
        .key(format!("verdict-{}", verdict_label(v).to_lowercase().replace(' ', "-")))
        .role("radio")
        .label(verdict_label(v))
        .checked(on)
        .w(0.0)
        .grow(1.0)
        .h(40.0)
        .gap(8.0)
        .center()
        .radius(radius::CARD)
        .cursor(Cursor::Pointer)
        .focusable(true)
        .child(icons::draw(icon, 16.0, ink))
        .child(text(verdict_label(v)).font(TYPE.ui).color(ink).a11y_hidden())
        .on_press(move |s: &mut App| s.set_verdict(v));
    b = if on {
        b.fill(tone.bg).border(2.0, tone.fg)
    } else {
        k.interactive(b.border(1.0, surf.card_border))
    };
    b
}

/// The dialog, centred under the toolbar: the verdict, the summary, what is still on your list,
/// then Cancel and Submit review.
pub(super) fn finish(s: &App, k: &Kit) -> Option<El> {
    let (surf, r) = sr(k);
    let round = s.mirror.round();
    let mut verdicts = row().key("finish-verdict").role("radiogroup").label("Verdict").w_full().gap(8.0);
    for v in VERDICTS {
        verdicts = verdicts.child(verdict_button(s, v, k));
    }
    let mut body = col()
        .w_full()
        .gap(14.0)
        .child(
            text(format!("Finish review · round {round}"))
                .font(rune::tok::TextStyle::new(18.0, 600.0).lh(24.0))
                .color(k.c.text_base),
        )
        .child(verdicts)
        .child(
            TextArea::new(&s.review.summary)
                .label("Summary")
                .rows(3)
                .on_edit(|s: &mut App| &mut s.review.summary)
                .build(k)
                .key("finish-summary"),
        );
    let warnings = s.finish_warnings();
    let mut list = col()
        .key("finish-warnings")
        .w_full()
        .child(text("Still on your list").font(TYPE.label).color(k.c.text_base).pb(4.0));
    if warnings.is_empty() {
        list = list.child(text("Nothing left on your list.").font(TYPE.small).color(k.c.success.fg));
    }
    for (i, w) in warnings.iter().enumerate() {
        let mut line = row()
            .key(format!("finish-warning-{i}"))
            .w_full()
            .min_h(32.0)
            .gap(10.0)
            .items_center()
            .child(
                el().size(6.0, 6.0)
                    .radius(3.0)
                    .fill(if w.go_to_must { r.must } else { k.c.icon_faint }),
            )
            .child(text(w.text.clone()).font(TYPE.ui).color(k.c.text_base).wrap().grow(1.0).shrink());
        if w.go_to_must {
            line = line.child(
                outline("finish-go-must", "Go to first", None, 24.0, k).on_activate_tx(|s: &mut App, tx: &mut Tx| s.goto_first_must(tx)),
            );
        }
        list = list.child(line).child(el().w_full().h(1.0).fill(surf.hairline).a11y_hidden());
    }
    body = body.child(list);
    if let Some(why) = &s.review.refused {
        body = body.child(
            text(why.clone())
                .key("finish-refused")
                .role("alert")
                .font(TYPE.small)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    let ready = s.review.verdict.is_some();
    let mut submit = row()
        .key("finish-submit")
        .role("button")
        .label("Submit review")
        .help(if ready {
            "Record this round  Ctrl+Enter"
        } else {
            "Pick a verdict first"
        })
        .h(28.0)
        .px(12.0)
        .gap(8.0)
        .center()
        .radius(radius::BUTTON)
        .fill(r.accent)
        .focusable(true)
        .child(text("Submit review").font(TYPE.strong).color(r.on_accent).a11y_hidden());
    submit = if ready {
        k.interactive_with(submit.cursor(Cursor::Pointer), k.c.contrast_hover, k.c.contrast_pressed)
            .press_scale(0.97)
            .on_activate_tx(|s: &mut App, tx: &mut Tx| {
                s.submit_review(tx);
            })
    } else {
        submit.opacity(0.45).disabled(true)
    };
    body = body.child(
        row()
            .w_full()
            .gap(8.0)
            .items_center()
            .child(el().grow(1.0).a11y_hidden())
            .child(
                k.interactive(
                    row()
                        .key("finish-cancel")
                        .role("button")
                        .label("Cancel")
                        .h(28.0)
                        .px(10.0)
                        .gap(6.0)
                        .items_center()
                        .radius(radius::BUTTON)
                        .cursor(Cursor::Pointer)
                        .help("Cancel  Esc")
                        .child(text("Cancel").font(TYPE.ui).color(k.c.text_base).a11y_hidden()),
                )
                .on_press(|s: &mut App| s.close_finish()),
            )
            .child(submit),
    );
    let panel = col()
        .key("overlay-finish-review")
        .role("dialog")
        .label("Finish review")
        .w(600.0)
        .padding(22.0)
        .radius(radius::ISLAND)
        .fill(r.raised)
        .border(1.0, surf.card_border)
        .shadow(24.0, 64.0, r.shadow)
        .on_press(|_: &mut App| {})
        .child(body);
    Some(row().w_full().pt(22.0).justify_center().child(panel))
}

/// Above the reader: new commits, what the last reload changed, and the round picker.
pub(super) fn reader_strips(s: &App, k: &Kit) -> Vec<El> {
    let mut out = Vec::new();
    if let Some(st) = &s.reloader.banner {
        out.push(messages::banner(
            "reload-banner",
            st.message(),
            Tone::Warning,
            &[("Reload", |s: &mut App, tx: &mut Tx| s.reload_now(tx))],
            k,
        ));
    }
    if let Some((msg, _)) = &s.reloader.toast {
        let show = s.reloader.counts.is_some_and(|c| c.moved + c.addressed + c.orphaned > 0);
        let dismiss: messages::Action = ("Dismiss", |s: &mut App, tx: &mut Tx| s.dismiss_reload_toast(tx));
        let actions: Vec<messages::Action> = if show {
            vec![("Show", |s: &mut App, tx: &mut Tx| s.show_reload_changes(tx)), dismiss]
        } else {
            vec![dismiss]
        };
        out.push(messages::banner("reload-toast", msg.clone(), Tone::Success, &actions, k));
    }
    if let Some(r) = round_picker(s, k) {
        out.push(r);
    }
    out
}

/// `Compare: Base │ Round 1 │ Round 2 (latest)` once a round is finished, with Reset.
fn round_picker(s: &App, k: &Kit) -> Option<El> {
    let rounds = &s.mirror.rounds;
    if rounds.is_empty() {
        return None;
    }
    let last = rounds.len() - 1;
    let mut labels = vec!["Base".to_owned()];
    labels.extend(rounds.iter().enumerate().map(|(i, r)| {
        if i == last {
            format!("Round {} (latest)", r.n)
        } else {
            format!("Round {}", r.n)
        }
    }));
    let picked = match &s.review.lens {
        Some(l) => rounds.iter().position(|r| r.n == l.round).map(|i| i + 1),
        None => Some(0),
    };
    let ns: Vec<u32> = rounds.iter().map(|r| r.n).collect();
    let control = SegmentedControl::new(&labels, picked)
        .label("Compare with")
        .fit()
        .on_pick(move |s: &mut App, i, tx: &mut Tx| s.set_lens(if i == 0 { None } else { ns.get(i - 1).copied() }, tx))
        .build(k);
    let mut r = row()
        .key("round-picker")
        .role("group")
        .label("Compare with a round")
        .w_full()
        .px(12.0)
        .py(4.0)
        .gap(10.0)
        .items_center()
        .fill(k.c.layer_01)
        .child(text("Compare:").font(k.ty.small).color(k.c.text_muted))
        .child(control);
    if let Some(l) = &s.review.lens {
        r = r
            .child(
                text(format!(
                    "{} hunks in {} files changed since round {}",
                    l.hunks,
                    l.files.len(),
                    l.round
                ))
                .font(k.ty.small)
                .color(k.c.text_muted)
                .truncate()
                .grow(1.0)
                .shrink(),
            )
            .child(
                Button::new("Show all")
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Small)
                    .on_press_tx(|s: &mut App, tx: &mut Tx| s.set_lens(None, tx))
                    .build(k)
                    .key("round-reset"),
            );
    }
    Some(r)
}

/// A file header's "changed since you viewed · n hunks", which jumps to the first of them.
pub(super) fn changed_chip(s: &App, g: usize, k: &Kit) -> Option<El> {
    let n = s.reloader.changed.get(&g)?.len();
    let label = format!("changed since you viewed · {n} hunk{}", if n == 1 { "" } else { "s" });
    Some(
        k.interactive(
            row()
                .key(format!("changed-{g}"))
                .role("button")
                .label(label.clone())
                .help("Jump to the first hunk that changed since you marked this file viewed")
                .h(18.0)
                .px(6.0)
                .gap(4.0)
                .items_center()
                .radius(crate::theme::radius::CHIP)
                .fill(k.c.warning.bg)
                .cursor(rune::Cursor::Pointer)
                .child(super::icons::draw(&super::icons::RELOAD, 12.0, k.c.warning.fg))
                .child(text(label).font(crate::theme::TYPE.micro).color(k.c.warning.fg).a11y_hidden()),
        )
        .on_press_tx(move |s: &mut App, tx: &mut Tx| s.goto_changed_hunk(g, tx)),
    )
}
