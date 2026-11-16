//! Note cards: agent story, file and line notes and the overview's intent notes, on their own
//! tint with a note rail, so they never read as comment threads.

use rain_core::{Anchor, Note};
use rune::{Color, Cursor, El, col, el, row, text};
use rune_kit::{Kit, Markdown};

use super::icons;
use super::ui::{Ghost, sr};
use crate::app::App;
use crate::theme::{TYPE, radius};

/// Bodies longer than this many lines start clipped behind "Show all".
pub(super) const CLIP_LINES: usize = 8;

pub(super) fn note_purple(k: &Kit) -> Color {
    sr(k).1.note
}

/// "Intent" for the overview's notes, else "Note".
pub(super) fn note_label(n: &Note) -> String {
    if n.is_overview() { "Intent".into() } else { "Note".into() }
}

/// ` · claude · L23–24`: who wrote it and where it sits.
fn note_meta(n: &Note) -> String {
    let mut parts = Vec::new();
    if let Some(a) = n.author.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
        parts.push(a.to_owned());
    }
    match n.anchor {
        Some(Anchor::Line { line, .. }) => parts.push(format!("L{line}")),
        Some(Anchor::Range { start, end, .. }) => parts.push(format!("L{start}–{end}")),
        Some(Anchor::Span { start_line, end_line, .. }) if start_line == end_line => parts.push(format!("L{start_line}")),
        Some(Anchor::Span { start_line, end_line, .. }) => parts.push(format!("L{start_line}–{end_line}")),
        None => {}
    }
    parts.iter().map(|p| format!("· {p}")).collect::<Vec<_>>().join(" ")
}

/// Characters of body text per drawn line, for the clip estimate.
const LINE_CHARS: usize = 90;

/// About how many lines `body` draws as Markdown: soft-broken lines join their paragraph;
/// list items, headings, table rows and code lines stand alone; blank lines add a gap.
pub(super) fn drawn_lines(body: &str) -> usize {
    let wrap = |chars: usize| chars.div_ceil(LINE_CHARS).max(1);
    let (mut total, mut para, mut fenced) = (0, 0, false);
    for line in body.lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            fenced = !fenced;
        }
        let alone = fenced
            || t.starts_with("```")
            || t.starts_with(['-', '*', '+', '#', '|', '>'])
            || t.split_once(". ")
                .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if t.is_empty() || alone {
            if para > 0 {
                total += wrap(para);
                para = 0;
            }
            total += 1;
        } else {
            para += t.chars().count() + 1;
        }
    }
    if para > 0 {
        total += wrap(para);
    }
    total
}

fn long(body: &str) -> bool {
    drawn_lines(body) > CLIP_LINES
}

/// A small header button on a note: hover overlay, muted text.
fn note_button(key: String, icon: &'static rune::Icon, label: Option<&str>, help: &str, k: &Kit) -> El {
    let mut g = Ghost::new(key).icon(icon).help(help).height(22.0);
    if let Some(l) = label {
        g = g.label(l);
    }
    g.build(k)
}

/// One note, keyed `note-<id>`: `✦ Intent · claude · L23–24` with Pop out and Fold, an optional
/// title, then the Markdown body; long bodies clip with a "Show all" toggle. Popped out, it
/// collapses to "Open in a window · Bring back".
pub(super) fn note_view(s: &App, n: &Note, k: &Kit) -> El {
    let (_, r) = sr(k);
    let label = note_label(n);
    let id = n.id.clone();
    let fold_key = format!("note:{}", n.id);
    let popped = s.note_popped(&n.id);
    let mut head = row()
        .key("note-head")
        .w_full()
        .h(24.0)
        .gap(6.0)
        .items_center()
        .child(icons::draw(&icons::SPARK, 14.0, r.note))
        .child(text(label.clone()).font(TYPE.label).color(r.note))
        .child(text(note_meta(n)).font(TYPE.small).color(k.c.text_faint).truncate().shrink());
    if let Some(t) = n.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        head = head.child(text(t.to_owned()).font(TYPE.strong).color(k.c.text_base).truncate().shrink());
    }
    head = head.child(el().grow(1.0).a11y_hidden());
    if !popped {
        let pop = id.clone();
        head = head.child(
            note_button(
                format!("note-popout-{}", n.id),
                &icons::POPOUT,
                Some("Pop out"),
                "Open in its own window",
                k,
            )
            .on_press(move |s: &mut App| s.pop_out_note(&pop)),
        );
    }
    if n.anchor.is_some() {
        let fk = fold_key.clone();
        head = head.child(
            note_button(format!("note-fold-{}", n.id), &icons::FOLD, None, "Fold into the gutter", k)
                .on_press(move |s: &mut App| s.toggle_fold(&fk)),
        );
    }
    let mut content = col().grow(1.0).shrink().min_w(0.0).gap(2.0).child(head);
    if popped {
        let back = id.clone();
        content = content.child(
            row()
                .key("note-popped")
                .gap(8.0)
                .items_center()
                .child(text("Open in a window").font(TYPE.small).color(k.c.text_muted))
                .child(
                    k.interactive(
                        row()
                            .key(format!("note-bring-back-{}", n.id))
                            .role("button")
                            .label("Bring back")
                            .px(6.0)
                            .h(22.0)
                            .items_center()
                            .radius(radius::CHIP)
                            .cursor(Cursor::Pointer)
                            .child(text("Bring back").font(TYPE.small).color(k.c.text_accent).a11y_hidden()),
                    )
                    .on_press(move |s: &mut App| s.bring_back_note(&back)),
                ),
        );
        return shell(format!("note-{}", n.id), &label, k, content);
    }
    let md = Markdown::new(n.body.clone()).selectable(true).build(k);
    if long(&n.body) {
        let open = s.note_expanded(&n.id);
        let body = col().key("note-body").w_full().max_w(1000.0).child(md);
        content = content.child(if open {
            body
        } else {
            body.max_h(TYPE.ui.line_height * CLIP_LINES as f32).clip_fade(0.0, 0.0, 0.0, 24.0)
        });
        let more = id.clone();
        content = content.child(
            row().child(
                note_button(
                    format!("note-more-{}", n.id),
                    if open { &icons::CHEV_U } else { &icons::CHEV_D },
                    Some(if open { "Show less" } else { "Show all" }),
                    if open { "Show less" } else { "Show all" },
                    k,
                )
                .min_w(96.0)
                .on_press(move |s: &mut App| s.toggle_note_expanded(&more)),
            ),
        );
    } else {
        content = content.child(col().key("note-body").w_full().max_w(1000.0).child(md));
    }
    shell(format!("note-{}", n.id), &label, k, content)
}

/// The note's card: its tint, ring and 4 point corners, with the rail at the left edge.
fn shell(key: String, label: &str, k: &Kit, content: El) -> El {
    let (_, r) = sr(k);
    row()
        .key(key)
        .role("note")
        .label(label.to_owned())
        .w_full()
        .pl(14.0)
        .pr(8.0)
        .pt(6.0)
        .pb(9.0)
        .radius(radius::CHIP)
        .fill(r.note_bg)
        .border(1.0, r.note_border)
        .child(
            el().pin(Some(0.0), Some(0.0), None, Some(0.0))
                .w(3.0)
                .corners([radius::CHIP, 0.0, 0.0, radius::CHIP])
                .fill(r.note)
                .a11y_hidden(),
        )
        .child(content)
}

/// The story notes, at the top of the Overview; `None` when there are none.
pub(super) fn story_card(s: &App, k: &Kit) -> Option<El> {
    let notes = s.story_notes();
    if notes.is_empty() {
        return None;
    }
    let mut c = col().key("story-notes").role("group").label("Story notes").w_full().gap(6.0);
    for n in notes {
        c = c.child(note_view(s, n, k));
    }
    Some(c)
}

/// Full-list file `g`'s file notes, under its header; `None` when it has none.
pub(super) fn file_card(s: &App, g: usize, k: &Kit) -> Option<El> {
    let key = s.keys.get(g)?;
    let notes = s.file_notes_for(key);
    if notes.is_empty() {
        return None;
    }
    let mut c = col()
        .key(format!("file-notes-{g}"))
        .role("group")
        .label("File notes")
        .w_full()
        .px(12.0)
        .pt(6.0)
        .pb(2.0)
        .gap(6.0);
    for n in notes {
        c = c.child(note_view(s, n, k));
    }
    Some(c)
}
