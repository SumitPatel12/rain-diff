//! The Keyboard shortcuts screen (F1, the `⋯` menu, the palette): the profile, every command
//! with its keys (record, remove, reset), the universal keys, and Save or Cancel.

use rain_core::command::Command;
use rain_core::keymap::{Chord, Profile, UNIVERSAL};
use rune::{El, Key, KeyEvent, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Kit, SegmentedControl, TextInput};

use super::icons;
use super::ui::{Ghost, kbd, outline, primary, sr};
use crate::app::App;
use crate::theme::{TYPE, radius};

fn search_key(s: &mut App, ev: &KeyEvent, _: &mut Tx) -> bool {
    ev.key == Key::Escape && s.close_shortcuts()
}

/// One key of a command as a cap with a remove button.
fn chip(cmd: Command, i: usize, chord: Chord, k: &Kit) -> El {
    let (surf, _) = sr(k);
    let label = chord.display();
    row()
        .key(format!("sc-{}-key-{i}", cmd.id()))
        .role("group")
        .label(format!("{} key {label}", cmd.label()))
        .h(22.0)
        .pl(6.0)
        .gap(2.0)
        .items_center()
        .radius(radius::KEY)
        .fill(surf.well)
        .border(1.0, surf.hairline)
        .child(text(label.clone()).font(TYPE.micro_mono).color(k.c.text_base).a11y_hidden())
        .child(
            Ghost::new(format!("sc-{}-remove-{i}", cmd.id()))
                .icon(&icons::CLOSE)
                .help(format!("Remove {label} from {}", cmd.label()))
                .height(18.0)
                .build(k)
                .on_press(move |s: &mut App| s.remove_shortcut(cmd, chord)),
        )
}

fn command_row(s: &App, cmd: Command, k: &Kit) -> El {
    let (surf, _) = sr(k);
    let sh = &s.shortcuts;
    let chords = sh.draft.chords(cmd);
    let keys: Vec<String> = chords.iter().map(|c| c.display()).collect();
    let said = if keys.is_empty() {
        format!("{}: no keys", cmd.label())
    } else {
        format!("{}: {}", cmd.label(), keys.join(", "))
    };
    let recording = sh.recording == Some(cmd);
    let mut line = row()
        .key(format!("sc-{}", cmd.id()))
        .role("group")
        .label(said)
        .w_full()
        .min_h(34.0)
        .px(10.0)
        .gap(8.0)
        .items_center()
        .radius(radius::BUTTON)
        .child(
            text(cmd.label())
                .font(TYPE.ui)
                .color(k.c.text_base)
                .grow(1.0)
                .shrink()
                .truncate()
                .a11y_hidden(),
        );
    if recording {
        line = line.fill(surf.selected_row).child(
            text("Press the keys…")
                .key("sc-recording")
                .role("status")
                .font(TYPE.small)
                .color(k.c.text_muted),
        );
    }
    for (i, c) in chords.iter().enumerate() {
        line = line.child(chip(cmd, i, *c, k));
    }
    line = line.child(
        Ghost::new(format!("sc-{}-record", cmd.id()))
            .icon(&icons::KBD)
            .help(format!("Record a key for {}", cmd.label()))
            .height(24.0)
            .on(recording)
            .build(k)
            .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                tx.blur();
                s.start_recording(cmd);
            }),
    );
    if sh.changed(cmd) {
        line = line.child(
            Ghost::new(format!("sc-{}-reset", cmd.id()))
                .icon(&icons::RELOAD)
                .help(format!("Reset {} to the {} keys", cmd.label(), sh.profile.label()))
                .height(24.0)
                .build(k)
                .on_press(move |s: &mut App| s.reset_shortcut(cmd)),
        );
    }
    line
}

fn conflict_bar(s: &App, k: &Kit) -> Option<El> {
    let c = s.shortcuts.conflict?;
    Some(
        row()
            .key("sc-conflict")
            .role("alert")
            .w_full()
            .px(14.0)
            .py(8.0)
            .gap(8.0)
            .items_center()
            .fill(k.c.warning.bg)
            .child(
                text(format!(
                    "{} is already {}. Give it to {}?",
                    c.chord.display(),
                    c.holder.label(),
                    c.cmd.label()
                ))
                .font(TYPE.small)
                .color(k.c.warning.fg)
                .wrap()
                .grow(1.0)
                .shrink(),
            )
            .child(outline("sc-conflict-cancel", "Cancel", None, 24.0, k).on_press(|s: &mut App| s.resolve_conflict(false)))
            .child(outline("sc-conflict-replace", "Replace", None, 24.0, k).on_press(|s: &mut App| s.resolve_conflict(true))),
    )
}

fn universal_rows(k: &Kit) -> El {
    let mut c = col()
        .key("sc-universal")
        .role("group")
        .label("Universal keys")
        .w_full()
        .px(6.0)
        .pb(8.0)
        .child(
            text("In every profile")
                .font(TYPE.micro)
                .color(k.c.text_faint)
                .h(26.0)
                .px(8.0)
                .pt(8.0),
        );
    for (i, (keys, what)) in UNIVERSAL.iter().enumerate() {
        c = c.child(
            row()
                .key(format!("sc-universal-{i}"))
                .label(format!("{keys}: {what}"))
                .w_full()
                .h(28.0)
                .px(10.0)
                .gap(8.0)
                .items_center()
                .child(
                    text(*what)
                        .font(TYPE.small)
                        .color(k.c.text_muted)
                        .grow(1.0)
                        .shrink()
                        .truncate()
                        .a11y_hidden(),
                )
                .child(kbd(*keys, k)),
        );
    }
    c
}

pub(super) fn shortcuts_overlay(s: &App, k: &Kit) -> El {
    let (surf, r) = sr(k);
    let sh = &s.shortcuts;
    let labels: Vec<&str> = Profile::ALL.iter().map(|p| p.label()).collect();
    let picked = Profile::ALL.iter().position(|p| *p == sh.profile);
    let head = row()
        .key("sc-head")
        .w_full()
        .h(52.0)
        .px(14.0)
        .gap(12.0)
        .items_center()
        .child(text("Keyboard shortcuts").font(TYPE.title).color(k.c.text_base).grow(1.0))
        .child(
            SegmentedControl::new(&labels, picked)
                .label("Shortcut profile")
                .fit()
                .on_pick(|s: &mut App, i, _: &mut Tx| {
                    if let Some(p) = Profile::ALL.get(i) {
                        s.shortcuts_profile(*p);
                    }
                })
                .build(k)
                .key("sc-profile"),
        );
    let search = row()
        .key("sc-search")
        .w_full()
        .h(40.0)
        .px(14.0)
        .gap(10.0)
        .items_center()
        .child(icons::draw(&icons::SEARCH, 16.0, k.c.text_muted))
        .child(
            col().grow(1.0).min_w(0.0).child(
                TextInput::new(&sh.search)
                    .placeholder("Search commands or keys")
                    .label("Search keyboard shortcuts")
                    .width(None)
                    .plain()
                    .on_edit(|s: &mut App| &mut s.shortcuts.search)
                    .on_key(search_key)
                    .build(k),
            ),
        );
    let mut list = col().key("sc-list").w_full().px(6.0).py(6.0);
    let matches = sh.matches();
    for cmd in &matches {
        let version = (sh.generation, sh.recording == Some(*cmd), s.seen_scheme.get());
        list = list.child(rune::memo(format!("sc-{}", cmd.id()), version, || command_row(s, *cmd, k)));
    }
    if matches.is_empty() {
        list = list.child(text("No command matches").font(TYPE.small).color(k.c.text_muted).padding(12.0));
    }
    list = list.child(universal_rows(k));
    let mut panel = col()
        .key("shortcuts")
        .role("dialog")
        .label("Keyboard shortcuts")
        .w(720.0)
        .h_full()
        .max_h(640.0)
        .min_h(0.0)
        .radius(radius::ISLAND)
        .fill(r.raised)
        .border(1.0, surf.card_border)
        .shadow(24.0, 64.0, r.shadow)
        .clip_rounded(radius::ISLAND)
        .on_press(|_: &mut App| {})
        .child(head)
        .child(search)
        .child(el().w_full().h(1.0).fill(surf.hairline));
    if let Some(bar) = conflict_bar(s, k) {
        panel = panel.child(bar);
    } else if !sh.note.is_empty() {
        panel = panel.child(
            text(sh.note.clone())
                .key("sc-note")
                .role("status")
                .font(TYPE.small)
                .color(k.c.text_muted)
                .w_full()
                .px(14.0)
                .py(6.0),
        );
    }
    let foot = row()
        .key("sc-foot")
        .w_full()
        .h(48.0)
        .px(14.0)
        .gap(8.0)
        .items_center()
        .fill(surf.card_head)
        .child(outline("sc-reset-all", "Reset all", None, 26.0, k).on_press(|s: &mut App| s.reset_all_shortcuts()))
        .child(el().grow(1.0).a11y_hidden())
        .child(outline("sc-cancel", "Cancel", Some("Esc"), 26.0, k).on_press(|s: &mut App| {
            s.shortcuts.recording = None;
            s.shortcuts.conflict = None;
            s.close_shortcuts();
        }))
        .child(primary("sc-save", "Save", k).on_press(|s: &mut App| s.save_shortcuts()));
    panel = panel
        .child(
            col()
                .key("sc-rows")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .scroll(ScrollSpec { stick_bottom: false })
                .child(list),
        )
        .child(el().w_full().h(1.0).fill(surf.hairline))
        .child(foot);
    col()
        .key("shortcuts-scrim")
        .w_full()
        .h_full()
        .items_center()
        .pt(60.0)
        .pb(40.0)
        .fill(k.c.scrim)
        .child(panel)
}
