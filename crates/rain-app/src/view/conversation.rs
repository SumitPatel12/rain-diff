//! The Conversation overlay (waiting, addressed, yours, orphaned, settled, then the rounds), the
//! toolbar's presence dot and the agent's toasts.

use rain_comments::anchor_label;
use rain_comments::label::first_sentence;
use rain_core::{AnchorState, AnswerStatus, PresenceState, TargetKind, Thread, ThreadKind, ThreadState, Verdict};
use rune::{Color, El, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, Toast, ToastRegion};

use super::muted;
use crate::app::conversation::{Section, by_agent, short_name};
use crate::app::{App, Overlay};

const WIDTH: f32 = 420.0;
const CLOSE_REASONS: [&str; 3] = ["fixed elsewhere", "no longer applies", "duplicate"];

fn presence_color(p: PresenceState, k: &Kit) -> Color {
    match p {
        PresenceState::Idle | PresenceState::Done => k.c.success.icon,
        PresenceState::Working => k.c.info.icon,
        PresenceState::Blocked => k.c.warning.icon,
        PresenceState::Failed => k.c.danger.icon,
        PresenceState::Offline => k.c.icon_faint,
    }
}

fn dot(c: Color, size: f32) -> El {
    el().size(size, size).radius(size / 2.0).fill(c)
}

/// The toolbar's presence: a dot and the state; opens the Conversation.
pub(super) fn presence_dot(s: &App, k: &Kit) -> El {
    let state = s.presence_state();
    let full = s.presence_text();
    let short = match state {
        PresenceState::Working => s
            .conversation
            .presence
            .on
            .as_deref()
            .and_then(|on| rain_comments::ops::thread(&s.mirror, on))
            .map_or_else(|| "working".to_owned(), |t| format!("working · {}", short_name(t))),
        st => st.label().to_owned(),
    };
    k.interactive(
        row()
            .key("presence")
            .role("button")
            .label(full.clone())
            .description(format!("{full} · Conversation (C)"))
            .px(8.0)
            .py(4.0)
            .gap(6.0)
            .items_center()
            .radius(k.radius.sm),
    )
    .on_press_tx(|s: &mut App, tx: &mut Tx| s.toggle_conversation(tx))
    .child(dot(presence_color(state, k), 8.0).key("presence-dot"))
    .child(text(short).font(k.ty.small).color(k.c.text_muted).a11y_hidden())
}

/// The agent's toasts in the bottom-right stack; `None` when there are none.
pub(super) fn toasts(s: &App, k: &Kit) -> Option<El> {
    if s.conversation.toasts.is_empty() {
        return None;
    }
    let mut region = ToastRegion::new();
    for t in &s.conversation.toasts {
        let (close, act, hover) = (t.key.clone(), t.key.clone(), t.key.clone());
        let mut toast = Toast::new(format!("toast-{}", t.key), t.title.clone()).on_close(move |s: &mut App| s.dismiss_toast(&close));
        if let Some(a) = &t.action {
            let label = match a {
                crate::app::conversation::ToastAction::Thread(_) => "Show",
                crate::app::conversation::ToastAction::Orphaned => "Show orphaned",
                crate::app::conversation::ToastAction::AllowAsk => "Allow",
            };
            toast = toast.action(k, label, true, move |s: &mut App, tx: &mut Tx| s.toast_action(&act, tx));
        }
        region = region.toast(
            toast
                .build(k)
                .on_hover_change(move |s: &mut App, inside: bool, _: &mut Tx| s.hover_toast(&hover, inside)),
        );
    }
    Some(region.build(k))
}

/// Where a thread sits, in words.
fn location(s: &App, t: &Thread) -> String {
    let target = t.target.clone().unwrap_or_default();
    match t.target_kind() {
        TargetKind::Brief => format!("Brief › {}", target.section.as_deref().unwrap_or("section")),
        TargetKind::Node => format!("Map › node {}", target.node.as_deref().unwrap_or("?")),
        TargetKind::Note => format!("Note {}", target.note.as_deref().unwrap_or("?")),
        TargetKind::Pr => "Review (whole change)".into(),
        TargetKind::File => s
            .file_of_thread(t)
            .map_or_else(|| t.file_path.clone(), |i| format!("{} (file)", s.keys[i])),
        TargetKind::Line => {
            let key = s
                .file_of_thread(t)
                .map_or_else(|| rain_core::paths::join(&t.repo, &t.file_path), |i| s.keys[i].clone());
            anchor_label(&key, &t.anchor)
        }
    }
}

/// The chip on the right of a row: answer progress, the state, or where the anchor went.
fn chip(t: &Thread) -> String {
    if t.kind() == ThreadKind::Question
        && let Some(a) = &t.answer
    {
        return match a.status {
            AnswerStatus::Answering => "Claude is answering…".into(),
            AnswerStatus::Answered => "answered".into(),
            AnswerStatus::Failed => "failed · retry".into(),
            AnswerStatus::Timeout => "timed out · retry".into(),
            s => s.label().to_owned(),
        };
    }
    match (t.state, t.placement()) {
        (ThreadState::Addressed, _) => match &t.state_sha {
            Some(sha) => format!("addressed in {}", sha.chars().take(7).collect::<String>()),
            None => "addressed".into(),
        },
        (ThreadState::Dismissed, _) => match t.dismiss_reason {
            Some(r) => format!("dismissed · {}", r.label()),
            None => "dismissed".into(),
        },
        (ThreadState::Resolved, _) => "resolved".into(),
        (_, AnchorState::Orphaned) => match &t.orphan {
            Some(o) => format!("orphaned · {}", o.reason.label()),
            None => "orphaned".into(),
        },
        (_, AnchorState::Outdated) => "code changed".into(),
        (_, AnchorState::Moved) => "moved".into(),
        _ => t.disposition.map_or_else(|| "open".into(), |d| d.label().to_owned()),
    }
}

fn glyph(t: &Thread) -> &'static str {
    match t.kind() {
        ThreadKind::Issue => "●",
        ThreadKind::Question => "?",
        ThreadKind::Suggestion => "◆",
        ThreadKind::Nit => "·",
        ThreadKind::Praise => "★",
        ThreadKind::Comment => "○",
    }
}

fn small_button(key: String, label: &str, variant: ButtonVariant, f: impl Fn(&mut App, &mut Tx) + 'static, k: &Kit) -> El {
    Button::new(label)
        .variant(variant)
        .control_size(ControlSize::Small)
        .on_press_tx(f)
        .build(k)
        .key(key)
}

/// A quoted excerpt in mono: the post-time snapshot, first `max` lines.
fn excerpt(key: String, content: &str, max: usize, k: &Kit) -> El {
    let mut lines: Vec<&str> = content.lines().take(max).collect();
    if lines.is_empty() {
        lines.push("(empty)");
    }
    col()
        .key(key)
        .w_full()
        .px(8.0)
        .py(4.0)
        .radius(k.radius.sm)
        .fill(k.c.layer_02)
        .border(1.0, k.c.border_base)
        .children(
            lines
                .into_iter()
                .map(|l| text(l.to_owned()).font(k.ty.mono).color(k.c.text_base).truncate()),
        )
}

fn thread_row(s: &App, t: &Thread, section: Section, k: &Kit) -> El {
    let id = t.id.clone();
    let focused = s.focused_thread.as_deref() == Some(t.id.as_str());
    let in_reader = !t.is_artifact() && s.file_of_thread(t).is_some_and(|g| s.current() == Some(g));
    let who = if by_agent(t) { "Claude" } else { "you" };
    let head = format!("{} {} · {} · {who}", glyph(t), short_name(t), t.kind().label());
    let last = t.messages.last();
    let said = last.map_or(String::new(), |m| {
        let by = m.author.as_deref().filter(|a| !a.trim().is_empty()).unwrap_or("you");
        format!("{by}: {}", first_sentence(&m.body, 90))
    });
    let chip_text = chip(t);
    let chip_color = match section {
        Section::Waiting | Section::Addressed => k.c.warning.fg,
        Section::Orphaned => k.c.danger.fg,
        _ => k.c.text_muted,
    };
    let mut c = col()
        .key(format!("conv-{}", t.id))
        .role("button")
        .label(format!("{head} · {}", location(s, t)))
        .w_full()
        .px(10.0)
        .py(6.0)
        .gap(4.0)
        .radius(k.radius.sm)
        .on_press_tx(move |s: &mut App, tx: &mut Tx| s.show_thread(&id, tx))
        .child(
            row()
                .w_full()
                .gap(8.0)
                .items_center()
                .child(
                    text(head)
                        .font(k.ty.small_medium)
                        .color(k.c.text_base)
                        .truncate()
                        .grow(1.0)
                        .shrink(),
                )
                .child(text(chip_text).key("chip").font(k.ty.small).color(chip_color)),
        )
        .child(text(location(s, t)).font(k.ty.small).color(k.c.text_muted).truncate())
        .child(text(said).font(k.ty.small).color(k.c.text_base).truncate());
    c = if focused {
        c.fill(k.c.layer_03).border(1.0, k.c.border_focus)
    } else {
        k.interactive(c)
    };
    if section == Section::Orphaned {
        let snap = t.orphan.as_ref().map(|o| &o.snapshot).or(t.code_snapshot.as_ref());
        let sha = t
            .orphan
            .as_ref()
            .and_then(|o| o.last_seen_sha.clone())
            .or_else(|| t.commit.clone())
            .map_or_else(
                || "last seen:".to_owned(),
                |s| format!("last seen at {}:", s.chars().take(7).collect::<String>()),
            );
        c = c.child(muted(sha, k));
        if let Some(sn) = snap {
            c = c.child(excerpt(format!("conv-snapshot-{}", t.id), &sn.content, 4, k));
        }
        c = c.child(orphan_actions(s, t, k));
    } else if !in_reader && let Some(sn) = &t.code_snapshot {
        c = c.child(excerpt(format!("conv-excerpt-{}", t.id), &sn.content, 2, k));
    }
    if section == Section::Addressed {
        let (a, b) = (t.id.clone(), t.id.clone());
        let mut r = row().gap(6.0).child(small_button(
            format!("conv-confirm-{}", t.id),
            "Confirm (a)",
            ButtonVariant::Neutral,
            move |s: &mut App, _| s.confirm_addressed(&a),
            k,
        ));
        if t.state == ThreadState::Addressed {
            r = r.child(small_button(
                format!("conv-reopen-{}", t.id),
                "Reopen",
                ButtonVariant::Ghost,
                move |s: &mut App, _| s.reopen_thread(&b),
                k,
            ));
        }
        c = c.child(r);
    }
    c
}

fn orphan_actions(s: &App, t: &Thread, k: &Kit) -> El {
    let (a, b) = (t.id.clone(), t.id.clone());
    if s.conversation.closing.as_deref() == Some(t.id.as_str()) {
        let mut r = row().gap(6.0).flex_wrap().items_center().child(muted("Close because:", k));
        for reason in CLOSE_REASONS {
            let id = t.id.clone();
            r = r.child(small_button(
                format!("conv-close-{}-{}", t.id, reason.replace(' ', "-")),
                reason,
                ButtonVariant::Ghost,
                move |s: &mut App, _| s.close_orphan(&id, reason),
                k,
            ));
        }
        return r;
    }
    row()
        .gap(6.0)
        .child(small_button(
            format!("conv-attach-{}", t.id),
            "Attach here",
            ButtonVariant::Neutral,
            move |s: &mut App, _| s.attach_orphan_here(&a),
            k,
        ))
        .child(small_button(
            format!("conv-close-{}", t.id),
            "Close…",
            ButtonVariant::Ghost,
            move |s: &mut App, _| s.conversation.closing = Some(b.clone()),
            k,
        ))
}

fn section_header(s: &App, section: Section, count: usize, k: &Kit) -> El {
    let mut r = row()
        .key(format!("conv-section-{}", section.key()))
        .w_full()
        .px(10.0)
        .pt(10.0)
        .pb(4.0)
        .gap(8.0)
        .items_center()
        .child(text(section.title()).font(k.ty.small_medium).color(k.c.text_muted))
        .child(text(count.to_string()).font(k.ty.small).color(k.c.text_muted));
    if section == Section::Settled {
        let open = s.conversation.settled_open;
        r = r
            .role("button")
            .label(if open { "Fold settled" } else { "Show settled" })
            .on_press(|s: &mut App| s.conversation.settled_open = !s.conversation.settled_open)
            .child(text(if open { "▾" } else { "▸" }).font(k.ty.small).color(k.c.text_muted));
    }
    r
}

fn verdict_color(v: Verdict, k: &Kit) -> Color {
    match v {
        Verdict::Approve => k.c.success.icon,
        Verdict::RequestChanges => k.c.danger.icon,
        Verdict::Comment => k.c.info.icon,
    }
}

fn verdict_label(v: Verdict) -> &'static str {
    match v {
        Verdict::Approve => "approved",
        Verdict::RequestChanges => "changes requested",
        Verdict::Comment => "commented",
    }
}

fn rounds(s: &App, k: &Kit) -> Vec<El> {
    s.mirror
        .rounds
        .iter()
        .rev()
        .map(|r| {
            let line = format!("Round {} · {} · {}", r.n, verdict_label(r.verdict), first_sentence(&r.summary, 70));
            row()
                .key(format!("conv-round-{}", r.n))
                .label(line.clone())
                .w_full()
                .px(10.0)
                .py(4.0)
                .gap(8.0)
                .items_center()
                .child(el().w(3.0).h(28.0).radius(1.5).fill(verdict_color(r.verdict, k)))
                .child(text(line).font(k.ty.small).color(k.c.text_base).truncate().grow(1.0).shrink())
        })
        .collect()
}

fn filter_chip(key: &'static str, label: String, on: bool, k: &Kit) -> El {
    Button::new(label)
        .variant(if on { ButtonVariant::Outline } else { ButtonVariant::GhostMuted })
        .control_size(ControlSize::Small)
        .on_press(move |s: &mut App| s.toggle_conversation_filter(key))
        .build(k)
        .key(format!("conv-filter-{key}"))
}

fn body(s: &App, k: &Kit) -> El {
    let mut list = col().w_full().pb(12.0).gap(2.0);
    let mut shown = 0;
    for section in Section::ALL {
        let threads = s.conversation_section(section);
        if threads.is_empty() && section != Section::Waiting {
            continue;
        }
        list = list.child(section_header(s, section, threads.len(), k));
        if section == Section::Waiting && threads.is_empty() {
            list = list.child(muted("Nothing waiting on you.", k).px(10.0));
        }
        if section == Section::Settled && !s.conversation.settled_open {
            continue;
        }
        shown += threads.len();
        for t in threads {
            list = list.child(thread_row(s, t, section, k));
        }
    }
    let rounds = rounds(s, k);
    if !rounds.is_empty() {
        list = list
            .child(
                row()
                    .key("conv-section-rounds")
                    .px(10.0)
                    .pt(10.0)
                    .pb(4.0)
                    .child(text("ROUNDS").font(k.ty.small_medium).color(k.c.text_muted)),
            )
            .children(rounds);
    }
    if shown == 0 && !s.conversation.filter.is_empty() {
        list = list.child(muted("No threads match the filters.", k).px(10.0).pt(8.0));
    }
    col()
        .key("conv-list")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(ScrollSpec { stick_bottom: false })
        .child(list)
}

/// The overlay: a 420 pt panel on the reader's right edge.
pub(super) fn conversation(s: &App, k: &Kit) -> Option<El> {
    let state = s.presence_state();
    let f = &s.conversation.filter;
    let state_label = f
        .state
        .map_or_else(|| "state: any".to_owned(), |st| format!("state: {}", st.label()));
    let header = row()
        .key("conv-header")
        .w_full()
        .px(10.0)
        .pt(8.0)
        .gap(8.0)
        .items_center()
        .child(text("Conversation").font(k.ty.ui_medium).color(k.c.text_base))
        .child(
            row()
                .key("conv-presence")
                .grow(1.0)
                .shrink()
                .gap(6.0)
                .items_center()
                .child(dot(presence_color(state, k), 8.0))
                .child(text(s.presence_text()).font(k.ty.small).color(k.c.text_muted).truncate()),
        )
        .child(
            Button::new("Close (Esc)")
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .on_press(|s: &mut App| {
                    s.close_overlay(Overlay::Conversation);
                })
                .build(k)
                .key("overlay-conversation-close"),
        );
    let filters = row()
        .key("conv-filters")
        .w_full()
        .px(10.0)
        .pb(6.0)
        .gap(4.0)
        .flex_wrap()
        .child(filter_chip("waiting", "waiting".into(), f.waiting, k))
        .child(filter_chip("agent", "agent".into(), f.agent, k))
        .child(filter_chip("mine", "mine".into(), f.mine, k))
        .child(filter_chip("state", state_label, f.state.is_some(), k));
    let panel = col()
        .key("overlay-conversation")
        .role("dialog")
        .label("Conversation")
        .w(WIDTH)
        .h_full()
        .gap(2.0)
        .fill(k.c.layer_01)
        .border(1.0, k.c.border_base)
        .on_press(|_: &mut App| {})
        .child(header)
        .child(filters)
        .child(el().w_full().h(1.0).fill(k.c.border_base))
        .child(body(s, k));
    Some(row().w_full().h_full().justify_end().child(k.elevate(panel, &k.elevation.overlay)))
}
