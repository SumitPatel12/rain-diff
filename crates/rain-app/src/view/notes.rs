//! Note cards: agent story, file and line notes and the overview's intent notes, all with a
//! purple rail so they never read as comment threads.

use rain_core::Note;
use rune::{Color, El, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, Markdown};

use crate::app::App;

/// Bodies longer than this many lines start clipped behind "Show all".
pub(super) const CLIP_LINES: usize = 8;

pub(super) fn note_purple() -> Color {
    rune::oklch(0.606, 0.219, 292.7)
}

/// The purple for label text: the rail's purple misses 4.5:1 on its own tint in both themes.
pub(super) fn note_ink(k: &Kit) -> Color {
    if k.dark {
        rune::oklch(0.72, 0.219, 292.7)
    } else {
        rune::oklch(0.52, 0.219, 292.7)
    }
}

/// "Intent note" for the overview's notes, else "Note from <author>".
pub(super) fn note_label(n: &Note) -> String {
    if n.is_overview() {
        return "Intent note".into();
    }
    match n.author.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
        Some(a) => format!("Note from {a}"),
        None => "Note".into(),
    }
}

fn shell(key: String, label: &str, k: &Kit, content: El) -> El {
    row()
        .key(key)
        .role("note")
        .label(label.to_owned())
        .w_full()
        .gap(8.0)
        .items_stretch()
        .padding(8.0)
        .radius(k.radius.md)
        .fill(k.alpha(note_purple(), 0.10))
        .child(el().w(3.0).radius(1.5).fill(note_purple()))
        .child(content)
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

/// One note, keyed `note-<id>`: label, optional title, Markdown body; long bodies clip with a
/// "Show all" toggle.
pub(super) fn note_view(s: &App, n: &Note, k: &Kit) -> El {
    let label = note_label(n);
    let mut head = row()
        .key("note-head")
        .w_full()
        .gap(8.0)
        .items_center()
        .child(text(label.clone()).font(k.ty.small_medium).color(note_ink(k)));
    if let Some(t) = n.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        head = head.child(
            text(t.to_owned())
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .grow(1.0)
                .shrink()
                .truncate(),
        );
    }
    let md = Markdown::new(n.body.clone()).selectable(true).build(k);
    let mut content = col().grow(1.0).shrink().gap(4.0).child(head);
    if long(&n.body) {
        let open = s.note_expanded(&n.id);
        let body = col().key("note-body").w_full().child(md);
        content = content.child(if open {
            body
        } else {
            body.max_h(k.ty.body.line_height * CLIP_LINES as f32).clip_fade(0.0, 0.0, 0.0, 24.0)
        });
        let id = n.id.clone();
        content = content.child(
            row().child(
                Button::new(if open { "Show less" } else { "Show all" })
                    .variant(ButtonVariant::Ghost)
                    .control_size(ControlSize::Small)
                    .on_press(move |s: &mut App| s.toggle_note_expanded(&id))
                    .build(k)
                    .key(format!("note-more-{}", n.id)),
            ),
        );
    } else {
        content = content.child(col().key("note-body").w_full().child(md));
    }
    shell(format!("note-{}", n.id), &label, k, content)
}

/// The story notes, at the top of the inspector; `None` when there are none.
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
        .py(4.0)
        .gap(6.0);
    for n in notes {
        c = c.child(note_view(s, n, k));
    }
    Some(c)
}
