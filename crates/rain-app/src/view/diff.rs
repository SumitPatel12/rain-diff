//! The diff: file headers with their actions, the Why line and notes, threads, the composer,
//! drafts, inline notes and the gutter marks of what is folded.

use rain_comments::{CardLook, ThreadCard, anchor_label, ops};
use rain_core::{AnchorState, Thread, ThreadKind};
use rain_diffview::{Annotation, DiffColors, DiffView};
use rune::{Cursor, El, Tx, col, el, row, text};
use rune_kit::{Kit, Placement, popover};

use super::icons;
use super::ui::{Ghost, group_tag, sr};
use super::{composer, notes, ruler, wholefile};
use crate::app::{App, ME};
use crate::theme::{TYPE, radius};

/// The diff's colours from the scheme's roles.
pub(super) fn colors(k: &Kit) -> DiffColors {
    let (s, r) = sr(k);
    DiffColors {
        add_line: r.add_bg,
        del_line: r.del_bg,
        add_word: r.add_word,
        del_word: r.del_word,
        filler: r.filler,
        gap: s.well,
        header: s.card_head,
        hairline: s.hairline,
        cursor: r.accent,
        cursor_line: r.cursor_line,
        selection: r.selection,
        comment: r.accent,
        ask: k.c.info.fg,
        on_button: r.on_accent,
        find_hit: r.find_hit,
        find_current: r.find_current,
    }
}

pub(super) fn diff_view(s: &App, k: &Kit) -> El {
    let ka = k.clone();
    let kh = k.clone();
    let kw = k.clone();
    let km = k.clone();
    let kl = k.clone();
    let v = DiffView::new(|s: &App| s.active(), |s: &mut App| s.active_mut())
        .calm_added_files(true)
        .colors(colors(k))
        .annotation(move |s: &App, a: &Annotation| annotation(s, a, &ka))
        .gutter_mark(move |s: &App, here: &[&Annotation]| gutter_mark(s, here, &km))
        .header_lead(move |s: &App, local: usize| header_lead(s, local, &kl))
        .header_actions(move |s: &App, local: usize| header_actions(s, local, &kh))
        .header_annotation(move |s: &App, local: usize| why_line(s, local, &kw))
        .on_comment(|s: &mut App, lines, tx: &mut Tx| s.comment_on(lines, tx))
        .on_ask(|s: &mut App, lines, tx: &mut Tx| s.ask_on(lines, tx))
        .on_span_select(|s: &mut App, span, _: &mut Tx| s.span_selected(span.as_ref()))
        .on_select(|s: &mut App, sel, _: &mut Tx| s.lines_selected(sel))
        .on_visible_file(|_: &mut App, _| {})
        .on_need_texts(|s: &mut App, local| s.fetch_texts(local));
    col()
        .key("diff-menu-area")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .on_secondary_press(|s: &mut App, p: rune::PointerPress, _: &mut Tx| s.open_diff_menu((p.x, p.y)))
        .child(ruler::wire(v, s).build(s, k))
}

/// The Viewed toggle: a ring and box at rest, the success tint and a check when on.
fn viewed_toggle(g: usize, on: bool, k: &Kit) -> El {
    let (s, _) = sr(k);
    let mut b = row()
        .key(format!("viewed-{g}"))
        .role("checkbox")
        .label("Viewed")
        .checked(on)
        .help("Viewed  v · Space marks viewed and goes to the next file")
        .h(26.0)
        .pl(8.0)
        .pr(10.0)
        .gap(6.0)
        .items_center()
        .radius(radius::BUTTON)
        .cursor(Cursor::Pointer)
        .focusable(true);
    // The same parts either way, so the button never changes width: the box, the word, `v`.
    if on {
        b = b
            .fill(k.c.success.bg)
            .border(1.0, k.c.success.border)
            .child(icons::draw(&icons::CHECK, 14.0, k.c.success.fg))
            .child(text("Viewed").font(TYPE.small).color(k.c.success.fg).a11y_hidden());
    } else {
        b = b
            .border(1.0, s.strong_line)
            .child(el().size(14.0, 14.0).radius(radius::KEY).border(1.5, k.c.text_muted).a11y_hidden())
            .child(text("Viewed").font(TYPE.small).color(k.c.text_base).a11y_hidden());
    }
    k.interactive(b).press_scale(0.97).on_press(move |s: &mut App| {
        if g < s.keys.len() {
            let on = !s.is_viewed(g);
            s.set_viewed(g, on);
        }
    })
}

/// Right after the name: the copy-path button, then the file's group as its letter tag.
fn header_lead(s: &App, local: usize, k: &Kit) -> Option<El> {
    let g = s.global(local);
    let path = s.keys.get(g)?.clone();
    let copy = Ghost::new(format!("copy-path-{g}"))
        .icon(&icons::COPY)
        .help(format!("Copy path  {path}"))
        .height(22.0)
        .build(k)
        .label("Copy path")
        .on_press_tx(move |s: &mut App, tx: &mut Tx| {
            tx.copy(path.clone());
            s.status = format!("Copied {path}");
        });
    Some(row().gap(6.0).items_center().child(copy).child(group_tag(s.group_of(g), k)))
}

/// After the stats: the changed-since chip, then at the end `Whole file e` · Viewed `v` · `⋯`
/// (Open in editor, Copy location).
fn header_actions(s: &App, local: usize, k: &Kit) -> El {
    let (surf, _) = sr(k);
    let g = s.global(local);
    let viewed = g < s.keys.len() && s.is_viewed(g);
    let mut r = row().gap(8.0).items_center();
    if let Some(c) = super::finish::changed_chip(s, g, k) {
        r = r.child(c);
    }
    r = r.child(row().grow(1.0).a11y_hidden());
    if let Some(w) = wholefile::header_button(s, g, k) {
        r = r.child(w);
    }
    r.child(el().w(1.0).h(16.0).fill(surf.hairline).a11y_hidden())
        .child(viewed_toggle(g, viewed, k))
        .child(
            Ghost::new(format!("file-more-{g}"))
                .icon(&icons::MORE)
                .help("Open in editor  Ctrl+O · Copy location  y")
                .on(s.menus.file == Some(g))
                .build(k)
                .expanded(s.menus.file == Some(g))
                .on_press(move |s: &mut App| {
                    let open = s.menus.file != Some(g);
                    s.menus.close();
                    s.menus.file = open.then_some(g);
                }),
        )
}

/// The open file `⋯` menu: Open in editor and Copy location.
pub(super) fn file_menu(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(g) = s.menus.file.filter(|&g| g < s.keys.len()) else {
        return;
    };
    layer.push(rune_kit::scrim("file-menu-scrim", |s: &mut App| s.menus.file = None));
    let mut c = col().key("file-menu").role("menu").w(240.0).padding(4.0).gap(0.0).items_stretch();
    let blocked = s.disk_path(g).err();
    let item = |key: String, label: &str, kbd: &str, disabled: bool| {
        let mut r = row()
            .key(key)
            .role("menuitem")
            .label(label.to_owned())
            .h(28.0)
            .px(8.0)
            .gap(8.0)
            .items_center()
            .radius(radius::SMALL)
            .child(
                text(label.to_owned())
                    .font(TYPE.ui)
                    .color(if disabled { k.c.text_faint } else { k.c.text_base })
                    .grow(1.0),
            )
            .child(super::ui::kbd_bare(kbd, k));
        if !disabled {
            r = k.interactive(r.cursor(Cursor::Pointer));
        }
        r
    };
    let mut open = item(format!("open-{g}"), "Open in editor", "Ctrl+O", blocked.is_some());
    if let Some(why) = blocked {
        open = open.help(why);
    } else {
        open = open.on_press(move |s: &mut App| {
            s.menus.close();
            s.open_file_in_editor(g);
        });
    }
    c = c.child(open);
    if let Some(loc) = s.location(g) {
        c = c.child(
            item(format!("copy-location-{g}"), "Copy location", "y", false).on_press_tx(move |s: &mut App, tx: &mut Tx| {
                s.menus.close();
                tx.copy(loc.clone());
                s.status = format!("Copied {loc}");
            }),
        );
    }
    let surface = popover::raised(k, c, radius::CARD);
    layer.push(popover::anchored(k, surface, &format!("file-more-{g}"), Placement::BottomEnd, 4.0));
}

/// Under a file's header: the triage reason (`Why …`), the agent's file notes, then the
/// re-anchor strip with the threads that can't sit on a line.
fn why_line(s: &App, local: usize, k: &Kit) -> Option<El> {
    let (surf, _) = sr(k);
    let g = s.global(local);
    let why = s.keys.get(g).and_then(|key| s.why.get(key));
    let file_note = notes::file_card(s, g, k);
    let strip = reanchor_strip(s, g, k);
    if why.is_none() && file_note.is_none() && strip.is_none() {
        return None;
    }
    let mut c = col().w_full();
    if let Some(why) = why {
        c = c.child(
            row()
                .key("why")
                .w_full()
                .pl(38.0)
                .pr(12.0)
                .pt(5.0)
                .pb(6.0)
                .gap(6.0)
                .fill(surf.card_head)
                .child(text("Why").font(TYPE.label).color(k.c.text_base))
                .child(text(why.clone()).font(TYPE.small).color(k.c.text_muted).wrap().grow(1.0).shrink())
                .child(hairline_bottom(surf.hairline)),
        );
    }
    if let Some(strip) = strip {
        c = c.child(strip);
    }
    if let Some(n) = file_note {
        c = c.child(n);
    }
    Some(c)
}

fn hairline_bottom(c: rune::Color) -> El {
    el().pin(Some(0.0), None, Some(0.0), Some(0.0)).h(1.0).fill(c).a11y_hidden()
}

/// What the last load did to file `g`'s threads: `2 threads moved · 1 addressed? · 1 orphaned`,
/// with the outdated and orphaned ones listed under it unless folded.
pub(super) fn strip_counts(s: &App, g: usize) -> (usize, usize, usize, usize) {
    let (mut moved, mut addressed, mut orphaned, mut outdated) = (0, 0, 0, 0);
    for t in s.mirror.threads.iter().filter(|t| t.is_open() && s.file_of_thread(t) == Some(g)) {
        match t.placement() {
            AnchorState::Moved => moved += 1,
            AnchorState::Orphaned => orphaned += 1,
            AnchorState::Outdated => outdated += 1,
            AnchorState::Placed => {}
        }
        if ops::addressed_suggested(t) {
            addressed += 1;
        }
    }
    (moved, addressed, orphaned, outdated)
}

fn reanchor_strip(s: &App, g: usize, k: &Kit) -> Option<El> {
    let (surf, _) = sr(k);
    let (moved, addressed, orphaned, outdated) = strip_counts(s, g);
    let ids = s.unplaced.get(&g).filter(|v| !v.is_empty());
    if moved + addressed + orphaned + outdated == 0 && ids.is_none() {
        return None;
    }
    let plural = |n: usize, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
    let mut parts = Vec::new();
    if moved > 0 {
        parts.push(plural(moved, "thread moved", "threads moved"));
    }
    if addressed > 0 {
        parts.push(format!("{addressed} addressed?"));
    }
    if outdated > 0 {
        parts.push(format!("{outdated} outdated"));
    }
    if orphaned > 0 {
        parts.push(format!("{orphaned} orphaned"));
    }
    let off_diff = ids.map_or(0, Vec::len).saturating_sub(orphaned + outdated);
    if off_diff > 0 {
        parts.push(format!("{off_diff} outside the diff"));
    }
    let folded = s.composer_ui.folded_strips.contains(&g);
    let mut head = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(text(parts.join(" · ")).key("strip-summary").font(TYPE.small).color(k.c.text_muted));
    if ids.is_some() {
        head = head.child(
            Ghost::new(format!("strip-toggle-{g}"))
                .label(if folded { "Show" } else { "Hide" })
                .height(22.0)
                .build(k)
                .on_press(move |s: &mut App| {
                    if !s.composer_ui.folded_strips.remove(&g) {
                        s.composer_ui.folded_strips.insert(g);
                    }
                }),
        );
    }
    let mut strip = col()
        .key("reanchor-strip")
        .role("group")
        .label("Threads that moved or lost their line")
        .w_full()
        .pl(38.0)
        .pr(12.0)
        .pt(5.0)
        .pb(6.0)
        .gap(6.0)
        .fill(surf.card_head)
        .child(head)
        .child(hairline_bottom(surf.hairline));
    if let Some(ids) = ids.filter(|_| !folded) {
        for t in ids.iter().filter_map(|id| ops::thread(&s.mirror, id)) {
            strip = strip.child(card(s, t, k));
        }
    }
    Some(strip)
}

/// The full-width row under a line: a card inset 12 on each side.
fn inset(e: El) -> El {
    col().w_full().px(12.0).pt(6.0).pb(8.0).child(e)
}

fn annotation(s: &App, a: &Annotation, k: &Kit) -> El {
    if a.id == "composer" {
        if s.composer.is_none() || (s.composer_ui.long && s.composer_ui.card) {
            return el();
        }
        if s.composer_ui.long {
            return inset(composer::long_note(k));
        }
        return inset(composer::inline(s, k));
    }
    if let Some(i) = a.id.strip_prefix(App::DRAFT_PREFIX) {
        return i
            .parse::<usize>()
            .ok()
            .and_then(|i| s.mirror.drafts.get(i))
            .map_or_else(el, |d| inset(composer::draft_mark(d, k)));
    }
    if let Some(id) = a.id.strip_prefix("note:") {
        if s.is_folded(&a.id) {
            return el();
        }
        return s.note(id).map_or_else(el, |n| inset(notes::note_view(s, &n, k)));
    }
    match ops::thread(&s.mirror, &a.id) {
        Some(_) if s.is_folded(&a.id) => el(),
        Some(t) => inset(card(s, t, k)),
        None => el(),
    }
}

/// The colours a thread card paints, from the scheme's roles.
pub(super) fn card_look(k: &Kit) -> CardLook {
    let (surf, r) = sr(k);
    CardLook {
        fill: surf.panel,
        border: surf.card_border,
        focus: r.focus,
        shadow: r.shadow,
        hairline: surf.hairline,
        agent: (r.note_bg, r.note),
        you: (surf.selected_row, r.accent),
    }
}

/// `L23`, `L20–24`.
fn short_lines(t: &Thread) -> String {
    match t.anchor.lines() {
        (a, b) if a == b => format!("L{a}"),
        (a, b) => format!("L{a}–{b}"),
    }
}

/// A thread's card as every surface draws it: the diff, the strip, the Brief and the Map. In
/// the diff it is always open (folding sends it to the gutter); elsewhere it collapses to a line.
pub(super) fn card(s: &App, t: &Thread, k: &Kit) -> El {
    let location = match s.file_of_thread(t) {
        Some(i) => anchor_label(&s.keys[i], &t.anchor),
        None if t.is_artifact() => App::thread_title(t),
        None => anchor_label(&rain_core::paths::join(&t.repo, &t.file_path), &t.anchor),
    };
    let open = s.folds_to_gutter(t) || s.expanded.contains(&t.id);
    let mut c = ThreadCard::new(t)
        .collapsed(!open)
        .focused(s.focused_thread.as_deref() == Some(t.id.as_str()))
        .hovered(s.composer_ui.hovered.as_deref() == Some(t.id.as_str()))
        .dismissing(s.composer_ui.dismissing.as_deref() == Some(t.id.as_str()))
        .location(location)
        .look(card_look(k))
        .on_action(|s: &mut App, id: &str, a, tx: &mut Tx| s.card_action(id, a, tx));
    if !t.is_artifact() {
        c = c.lines(short_lines(t));
    }
    if t.answer.as_ref().is_some_and(|a| a.status.pending()) {
        c = c.now(rain_comments::clock::now_iso());
    }
    if s.reply_to.as_deref() == Some(t.id.as_str()) {
        c = c.reply_composer(&s.reply_edit, |s: &mut App| &mut s.reply_edit);
    }
    c.build(k)
}

/// The marks column by a line: a folded thread's badge (its message count, tinted by state),
/// a folded note's ✦, or an unsent draft's dashed ✎. A press unfolds; hovering previews.
fn gutter_mark(s: &App, here: &[&Annotation], k: &Kit) -> Option<El> {
    let (surf, r) = sr(k);
    for a in here {
        if let Some(t) = ops::thread(&s.mirror, &a.id).filter(|_| s.is_folded(&a.id)) {
            let (fg, bg, ring) = if ops::waiting_on_user(t, ME) {
                (k.c.warning.fg, k.c.warning.bg, k.c.warning.border)
            } else if t.kind() == ThreadKind::Issue || t.blocking() {
                (k.c.danger.fg, k.c.danger.bg, k.c.danger.border)
            } else {
                (k.c.info.fg, k.c.info.bg, k.c.info.border)
            };
            let who = t
                .messages
                .first()
                .and_then(|m| m.author.clone())
                .filter(|a| !a.trim().is_empty())
                .unwrap_or_else(|| "you".into());
            let n = t.messages.len();
            let first = t
                .messages
                .first()
                .map(|m| rain_comments::label::first_sentence(&m.body, 140))
                .unwrap_or_default();
            let said = format!(
                "Folded {} · {who} · {n} message{} — Enter or click to unfold\n{first}",
                t.kind().label(),
                if n == 1 { "" } else { "s" }
            );
            let id = a.id.clone();
            let face = mark_box(bg, Some(ring), false)
                .child(icons::draw(&icons::CHAT, 12.0, fg))
                .child(text(n.to_string()).font(TYPE.mark).color(fg).a11y_hidden());
            return Some(mark_hit(face, said).on_press(move |s: &mut App| s.unfold(&id)));
        }
    }
    for a in here {
        if a.id.starts_with("note:") && s.is_folded(&a.id) {
            let id = a.id.clone();
            let said = "Folded note — click to unfold".to_owned();
            let face = mark_box(r.note_bg, Some(r.note_border), false).child(icons::draw(&icons::SPARK, 12.0, r.note));
            return Some(mark_hit(face, said).on_press(move |s: &mut App| s.unfold(&id)));
        }
    }
    for a in here {
        if a.id.starts_with(App::DRAFT_PREFIX) {
            let said = "Unsent draft — c to continue".to_owned();
            return Some(
                mark_box(rune::Color::TRANSPARENT, Some(surf.strong_line), true)
                    .key("gm")
                    .role("note")
                    .label(said.clone())
                    .help(said)
                    .child(icons::draw(&icons::EDIT, 11.0, k.c.text_muted)),
            );
        }
    }
    None
}

/// A pressable gutter mark: `face` as drawn, pressed anywhere in the marks cell's height.
fn mark_hit(face: El, said: String) -> El {
    row()
        .key("gm")
        .role("button")
        .label(said.clone())
        .help(said)
        .focusable(true)
        .min_w(22.0)
        .h_full()
        .center()
        .cursor(Cursor::Pointer)
        .child(face)
}

fn mark_box(bg: rune::Color, ring: Option<rune::Color>, dashed: bool) -> El {
    let mut b = row().h(16.0).min_w(18.0).px(3.0).gap(2.0).center().radius(radius::CHIP).fill(bg);
    if let Some(r) = ring {
        b = b.border(1.0, r);
        if dashed {
            b = b.border_dash(2.0, 2.0);
        }
    }
    b
}
