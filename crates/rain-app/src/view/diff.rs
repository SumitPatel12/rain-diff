//! The diff: file headers with their actions and notes, threads, the composer, drafts and inline
//! notes.

use rain_comments::{ThreadCard, anchor_label, ops};
use rain_core::{AnchorState, FileTag, Thread};
use rain_diffview::{Annotation, DiffView};
use rune::{El, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, Checkbox, ControlSize, Kit, Placement, popover};

use super::{composer, notes, ruler, wholefile};
use crate::app::App;

pub(super) fn diff_view(s: &App, k: &Kit) -> El {
    let ka = k.clone();
    let kh = k.clone();
    let kw = k.clone();
    let frame = crate::theme::shape(s.chrome(), s.settings.density).frame(super::window_width(s));
    // An island already encloses the files: cards keep their head band and gap, no outline.
    let mut kd = k.clone();
    if s.chrome().islands() {
        kd.c.border_base = kd.c.bg_base;
    }
    let v = DiffView::new(|s: &App| s.active(), |s: &mut App| s.active_mut())
        .frame(frame)
        .calm_added_files(true)
        .annotation(move |s: &App, a: &Annotation| annotation(s, a, &ka))
        .header_actions(move |s: &App, local: usize| header_actions(s, local, &kh))
        .header_annotation(move |s: &App, local: usize| why_line(s, local, &kw))
        .on_comment(|s: &mut App, lines, tx: &mut Tx| s.comment_on(lines, tx))
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
        .child(ruler::wire(v, s).build(s, &kd))
}

/// Thread cards read at most this wide, however wide the reader.
const CARD_MAX_W: f32 = 860.0;

/// `Must` (only that tag), the changed-since-round chip, Viewed and the file's `⋯`.
fn header_actions(s: &App, local: usize, k: &Kit) -> El {
    let g = s.global(local);
    let viewed = g < s.keys.len() && s.is_viewed(g);
    let tag = s.keys.get(g).and_then(|key| s.tags.get(key)).copied();
    let mut r = row().gap(8.0).items_center();
    if tag == Some(FileTag::MustReview) {
        r = r.child(text("Must").key("must").font(k.ty.small_medium).color(k.c.danger.fg));
    }
    if let Some(c) = super::finish::changed_chip(s, g, k) {
        r = r.child(c);
    }
    r.child(
        Checkbox::new(viewed)
            .label("Viewed")
            .on_toggle(move |s: &mut App| {
                if g < s.keys.len() {
                    let on = !s.is_viewed(g);
                    s.set_viewed(g, on);
                }
            })
            .build(k)
            .key(format!("viewed-{g}")),
    )
    .child(
        Button::new("⋯")
            .variant(ButtonVariant::GhostMuted)
            .control_size(ControlSize::Small)
            .help("Whole file, open in editor, copy path")
            .on_press(move |s: &mut App| {
                let open = s.menus.file != Some(g);
                s.menus.close();
                s.menus.file = open.then_some(g);
            })
            .build(k)
            .key(format!("file-more-{g}"))
            .expanded(s.menus.file == Some(g)),
    )
}

/// The open file `⋯` menu: Whole file, Open in editor, Copy path, Copy location.
pub(super) fn file_menu(s: &App, k: &Kit, layer: &mut Vec<El>) {
    let Some(g) = s.menus.file.filter(|&g| g < s.keys.len()) else {
        return;
    };
    layer.push(rune_kit::scrim("file-menu-scrim", |s: &mut App| s.menus.file = None));
    let mut c = col().key("file-menu").role("menu").w(240.0).padding(6.0).gap(2.0).items_stretch();
    if let Some(w) = wholefile::header_button(s, g, k) {
        c = c.child(row().h(28.0).px(8.0).items_center().child(w));
    }
    let blocked = s.disk_path(g).err();
    c = c.child(
        Button::new("Open in editor")
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Small)
            .disabled(blocked.is_some())
            .help(blocked.unwrap_or("Open in editor at the cursor line"))
            .shortcut("Ctrl+O")
            .on_press(move |s: &mut App| {
                s.menus.close();
                s.open_file_in_editor(g);
            })
            .build(k)
            .key(format!("open-{g}")),
    );
    let path = s.keys[g].clone();
    c = c.child(
        Button::new("Copy path")
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Small)
            .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                s.menus.close();
                tx.copy(path.clone());
                s.status = format!("Copied {path}");
            })
            .build(k)
            .key(format!("copy-path-{g}")),
    );
    if let Some(loc) = s.location(g) {
        c = c.child(
            Button::new("Copy location")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .shortcut("Y")
                .on_press_tx(move |s: &mut App, tx: &mut Tx| {
                    s.menus.close();
                    tx.copy(loc.clone());
                    s.status = format!("Copied {loc}");
                })
                .build(k)
                .key(format!("copy-location-{g}")),
        );
    }
    let surface = popover::raised(k, c, k.radius.xl);
    layer.push(popover::anchored(k, surface, &format!("file-more-{g}"), Placement::BottomEnd, 4.0));
}

/// Under a file's header: the overview's "why", the agent's file notes, then the re-anchor
/// strip with the threads that can't sit on a line.
fn why_line(s: &App, local: usize, k: &Kit) -> Option<El> {
    let g = s.global(local);
    let why = s.keys.get(g).and_then(|key| s.why.get(key));
    let file_note = notes::file_card(s, g, k);
    let strip = reanchor_strip(s, g, k);
    if why.is_none() && file_note.is_none() && strip.is_none() {
        return None;
    }
    let mut c = col().w_full().gap(4.0);
    if let Some(why) = why {
        c = c.child(
            row()
                .w_full()
                .px(16.0)
                .py(4.0)
                .child(text(format!("Why: {why}")).font(k.ty.small).color(k.c.text_muted).wrap()),
        );
    }
    if let Some(n) = file_note {
        c = c.child(n);
    }
    if let Some(strip) = strip {
        c = c.child(strip);
    }
    Some(c)
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
    let mut head = row().w_full().gap(8.0).center_y().child(
        text(parts.join(" · "))
            .key("strip-summary")
            .font(k.ty.small_medium)
            .color(k.c.text_muted),
    );
    if ids.is_some() {
        head = head.child(
            Button::new(if folded { "Show" } else { "Hide" })
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut App| {
                    if !s.composer_ui.folded_strips.remove(&g) {
                        s.composer_ui.folded_strips.insert(g);
                    }
                })
                .build(k)
                .key(format!("strip-toggle-{g}")),
        );
    }
    let mut strip = col()
        .key("reanchor-strip")
        .role("group")
        .label("Threads that moved or lost their line")
        .w_full()
        .px(12.0)
        .py(4.0)
        .gap(4.0)
        .child(head);
    if let Some(ids) = ids.filter(|_| !folded) {
        for t in ids.iter().filter_map(|id| ops::thread(&s.mirror, id)) {
            strip = strip.child(card(s, t, k));
        }
    }
    Some(strip)
}

fn annotation(s: &App, a: &Annotation, k: &Kit) -> El {
    let wrap = |e: El| col().w_full().px(12.0).py(4.0).child(e);
    if a.id == "composer" {
        if s.composer.is_none() || (s.composer_ui.long && s.composer_ui.card) {
            return el();
        }
        if s.composer_ui.long {
            return wrap(composer::long_note(k));
        }
        return wrap(composer::inline(s, k));
    }
    if let Some(i) = a.id.strip_prefix(App::DRAFT_PREFIX) {
        return i
            .parse::<usize>()
            .ok()
            .and_then(|i| s.mirror.drafts.get(i))
            .map_or_else(el, |d| wrap(composer::draft_mark(d, k)));
    }
    if let Some(id) = a.id.strip_prefix("note:") {
        return s.note(id).map_or_else(el, |n| wrap(notes::note_view(s, &n, k)));
    }
    match ops::thread(&s.mirror, &a.id) {
        Some(t) => wrap(card(s, t, k).max_w(CARD_MAX_W)),
        None => el(),
    }
}

/// A thread's card as every surface draws it: the diff, the strip, the Brief and the Map.
pub(super) fn card(s: &App, t: &Thread, k: &Kit) -> El {
    let location = match s.file_of_thread(t) {
        Some(i) => anchor_label(&s.keys[i], &t.anchor),
        None if t.is_artifact() => App::thread_title(t),
        None => anchor_label(&rain_core::paths::join(&t.repo, &t.file_path), &t.anchor),
    };
    let mut c = ThreadCard::new(t)
        .collapsed(!s.expanded.contains(&t.id))
        .focused(s.focused_thread.as_deref() == Some(t.id.as_str()))
        .hovered(s.composer_ui.hovered.as_deref() == Some(t.id.as_str()))
        .dismissing(s.composer_ui.dismissing.as_deref() == Some(t.id.as_str()))
        .location(location)
        .on_action(|s: &mut App, id: &str, a, tx: &mut Tx| s.card_action(id, a, tx));
    if t.answer.as_ref().is_some_and(|a| a.status.pending()) {
        c = c.now(rain_comments::clock::now_iso());
    }
    if s.reply_to.as_deref() == Some(t.id.as_str()) {
        c = c.reply_composer(&s.reply_edit, |s: &mut App| &mut s.reply_edit);
    }
    c.build(k)
}
