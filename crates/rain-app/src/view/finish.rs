//! The Finish review dialog, the strips above the reader that rounds and reloads add (new
//! commits banner, reload message, round picker) and a file header's "changed since you viewed".

use rune::{El, Tx, col, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, RadioGroup, RadioItem, SegmentedControl, TextArea};

use super::messages::{self, Tone};
use crate::app::{App, VERDICTS, verdict_label};

/// The dialog, centred under the toolbar.
pub(super) fn finish(s: &App, k: &Kit) -> Option<El> {
    let round = s.mirror.round();
    let mut radios = RadioGroup::new().label("Verdict");
    for v in VERDICTS {
        radios = radios.item(RadioItem::new(verdict_label(v), s.review.verdict == Some(v)).on_select(move |s: &mut App| s.set_verdict(v)));
    }
    let mut body = col()
        .w_full()
        .gap(12.0)
        .child(
            row()
                .w_full()
                .items_center()
                .child(
                    text(format!("Finish review · round {round}"))
                        .font(k.ty.ui_medium)
                        .color(k.c.text_strong)
                        .grow(1.0),
                )
                .child(
                    Button::new("Close (Esc)")
                        .variant(ButtonVariant::Ghost)
                        .on_press(|s: &mut App| s.close_finish())
                        .build(k)
                        .key("finish-close"),
                ),
        )
        .child(radios.build(k).key("finish-verdict"))
        .child(
            TextArea::new(&s.review.summary)
                .label("Summary")
                .rows(3)
                .on_edit(|s: &mut App| &mut s.review.summary)
                .build(k)
                .key("finish-summary"),
        );
    let warnings = s.finish_warnings();
    let mut list = col().key("finish-warnings").w_full().gap(4.0);
    if warnings.is_empty() {
        list = list.child(text("Nothing left on your list.").font(k.ty.small).color(k.c.success.fg));
    }
    for (i, w) in warnings.iter().enumerate() {
        let mut r = row()
            .key(format!("finish-warning-{i}"))
            .w_full()
            .gap(8.0)
            .items_center()
            .child(text(w.text.clone()).font(k.ty.small).color(k.c.text_base).wrap().grow(1.0).shrink());
        if w.go_to_must {
            r = r.child(
                Button::new("Go to first")
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Small)
                    .on_press_tx(|s: &mut App, tx: &mut Tx| s.goto_first_must(tx))
                    .build(k)
                    .key("finish-go-must"),
            );
        }
        list = list.child(r);
    }
    body = body.child(list);
    if let Some(why) = &s.review.refused {
        body = body.child(
            text(why.clone())
                .key("finish-refused")
                .role("alert")
                .font(k.ty.small)
                .color(k.c.danger.fg)
                .wrap(),
        );
    }
    let submit = Button::new("Submit review")
        .variant(ButtonVariant::Submit)
        .disabled(s.review.verdict.is_none())
        .help(if s.review.verdict.is_none() {
            "Pick a verdict first"
        } else {
            "Record this round"
        })
        .on_press_tx(|s: &mut App, tx: &mut Tx| {
            s.submit_review(tx);
        })
        .build(k)
        .key("finish-submit");
    body = body.child(
        row()
            .w_full()
            .gap(8.0)
            .items_center()
            .child(row().grow(1.0))
            .child(
                Button::new("Cancel")
                    .variant(ButtonVariant::Ghost)
                    .on_press(|s: &mut App| s.close_finish())
                    .build(k)
                    .key("finish-cancel"),
            )
            .child(submit),
    );
    let panel = col()
        .key("overlay-finish-review")
        .role("dialog")
        .label("Finish review")
        .w(560.0)
        .padding(16.0)
        .radius(k.radius.lg)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_base)
        .on_press(|_: &mut App| {})
        .child(body);
    Some(
        row()
            .w_full()
            .pt(40.0)
            .justify_center()
            .child(k.elevate(panel, &k.elevation.overlay)),
    )
}

/// Above the reader: new commits, what the last reload changed, and the round picker.
pub(super) fn reader_strips(s: &App, k: &Kit) -> Vec<El> {
    let mut out = Vec::new();
    if let Some(st) = &s.reloader.banner {
        out.push(messages::banner(
            "reload-banner",
            format!("{} — Reload (Shift+R)", st.message()),
            Tone::Warning,
            Some(("Reload", |s: &mut App, tx: &mut Tx| s.reload_now(tx))),
            k,
        ));
    }
    if let Some((msg, _)) = &s.reloader.toast {
        let show = s.reloader.counts.is_some_and(|c| c.moved + c.addressed + c.orphaned > 0);
        let action: messages::Action = if show {
            ("Show", |s: &mut App, tx: &mut Tx| s.show_reload_changes(tx))
        } else {
            ("Dismiss", |s: &mut App, tx: &mut Tx| s.dismiss_reload_toast(tx))
        };
        out.push(messages::banner("reload-toast", msg.clone(), Tone::Info, Some(action), k));
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
        Button::new(label)
            .variant(ButtonVariant::Warning)
            .control_size(ControlSize::Small)
            .help("Jump to the first hunk that changed since you marked this file viewed")
            .on_press_tx(move |s: &mut App, tx: &mut Tx| s.goto_changed_hunk(g, tx))
            .build(k)
            .key(format!("changed-{g}")),
    )
}
