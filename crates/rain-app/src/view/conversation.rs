//! The Conversation overlay (waiting, addressed, yours, orphaned, settled, then the rounds), the
//! toolbar's presence dot and the agent's toasts.

use rain_comments::anchor_label;
use rain_comments::label::first_sentence;
use rain_core::{AnchorState, AnswerStatus, PresenceState, TargetKind, Thread, ThreadKind, ThreadState, Verdict};
use rune::{Color, El, ScrollSpec, Tx, col, el, row, text};
use rune_kit::{ButtonVariant, Kit};

use super::ui::{Ghost, outline, sr};
use super::{icons, muted};
use crate::app::conversation::{Section, by_agent, short_name};
use crate::app::{App, Overlay};
use crate::theme::{TYPE, radius};

const WIDTH: f32 = 380.0;
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

/// The agent's toasts, stacked at the reader island's bottom right; `None` when there are none.
pub(super) fn toasts(s: &App, k: &Kit) -> Option<El> {
    if s.conversation.toasts.is_empty() {
        return None;
    }
    let (surf, r) = sr(k);
    let mut stack = col().key("toasts").role("log").label("Messages").w(380.0).gap(8.0);
    for t in &s.conversation.toasts {
        let (close, act, hover) = (t.key.clone(), t.key.clone(), t.key.clone());
        let allow = matches!(t.action, Some(crate::app::conversation::ToastAction::AllowAsk));
        let mut card = row()
            .key(format!("toast-{}", t.key))
            .role("status")
            .label(t.title.clone())
            .w_full()
            .pl(12.0)
            .pr(8.0)
            .py(10.0)
            .gap(10.0)
            .items_start()
            .radius(radius::CARD)
            .fill(r.raised)
            .border(1.0, surf.card_border)
            .shadow(12.0, 30.0, r.shadow)
            .on_hover_change(move |s: &mut App, inside: bool, _: &mut Tx| s.hover_toast(&hover, inside))
            .child(icons::draw(
                if allow { &icons::PIN } else { &icons::SPARK },
                16.0,
                if allow { k.c.text_muted } else { r.note },
            ))
            .child(
                text(t.title.clone())
                    .font(TYPE.small)
                    .color(k.c.text_base)
                    .wrap()
                    .grow(1.0)
                    .shrink(),
            );
        if let Some(a) = &t.action {
            let label = match a {
                crate::app::conversation::ToastAction::Thread(_) => "Show",
                crate::app::conversation::ToastAction::Orphaned => "Show orphaned",
                crate::app::conversation::ToastAction::AllowAsk => "Allow",
            };
            let b = if allow {
                k.interactive_with(
                    row()
                        .key(format!("toast-{}-action", t.key))
                        .role("button")
                        .label(label)
                        .h(24.0)
                        .px(8.0)
                        .center()
                        .radius(radius::SMALL)
                        .fill(r.accent)
                        .cursor(rune::Cursor::Pointer)
                        .child(text(label).font(TYPE.small).color(r.on_accent).a11y_hidden()),
                    k.c.contrast_hover,
                    k.c.contrast_pressed,
                )
            } else {
                outline(format!("toast-{}-action", t.key), label, None, 24.0, k)
            };
            card = card.child(b.on_press_tx(move |s: &mut App, tx: &mut Tx| s.toast_action(&act, tx)));
        }
        card = card.child(
            Ghost::new(format!("toast-{}-close", t.key))
                .icon(&icons::CLOSE)
                .help("Dismiss")
                .height(24.0)
                .build(k)
                .on_press(move |s: &mut App| s.dismiss_toast(&close)),
        );
        stack = stack.child(card);
    }
    Some(stack)
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
    let b = if variant == ButtonVariant::Ghost {
        k.interactive(
            row()
                .key(key)
                .role("button")
                .label(label.to_owned())
                .h(24.0)
                .px(8.0)
                .items_center()
                .radius(radius::SMALL)
                .cursor(rune::Cursor::Pointer)
                .child(text(label.to_owned()).font(TYPE.small).color(k.c.text_muted).a11y_hidden()),
        )
    } else {
        outline(key, label.to_owned(), None, 24.0, k)
    };
    b.on_press_tx(f)
}

/// A quoted excerpt in mono on the well: the post-time snapshot, first `max` lines.
fn excerpt(key: String, content: &str, max: usize, k: &Kit) -> El {
    let (surf, _) = sr(k);
    let mut lines: Vec<&str> = content.lines().take(max).collect();
    if lines.is_empty() {
        lines.push("(empty)");
    }
    col()
        .key(key)
        .w_full()
        .px(10.0)
        .py(4.0)
        .radius(radius::BUTTON)
        .fill(surf.well)
        .children(
            lines
                .into_iter()
                .map(|l| text(l.to_owned()).font(TYPE.micro_mono.lh(18.0)).color(k.c.text_muted).truncate()),
        )
}

/// `VouchersController.cs:23`, or the artifact's title.
fn short_place(s: &App, t: &Thread) -> String {
    match s.file_of_thread(t) {
        Some(g) if !t.is_artifact() => {
            let key = &s.keys[g];
            format!("{}:{}", key.rsplit('/').next().unwrap_or(key), t.anchor.lines().0)
        }
        _ => location(s, t),
    }
}

fn kind_ink(t: &Thread, k: &Kit) -> Color {
    match t.kind() {
        ThreadKind::Issue => k.c.danger.fg,
        ThreadKind::Question => k.c.info.fg,
        ThreadKind::Suggestion | ThreadKind::Praise => k.c.success.fg,
        _ => k.c.text_muted,
    }
}

fn thread_row(s: &App, t: &Thread, section: Section, k: &Kit) -> El {
    let (surf, _) = sr(k);
    let id = t.id.clone();
    let focused = s.focused_thread.as_deref() == Some(t.id.as_str());
    let in_reader = !t.is_artifact() && s.file_of_thread(t).is_some_and(|g| s.current() == Some(g));
    let who = if by_agent(t) { "claude" } else { "you" };
    let head = format!("{} {} · {} · {who}", glyph(t), short_name(t), t.kind().label());
    let last = t.messages.last();
    let said = last.map_or(String::new(), |m| first_sentence(&m.body, 90));
    let chip_text = chip(t);
    let (chip_fg, chip_bg) = match section {
        Section::Waiting => (k.c.danger.fg, k.c.danger.bg),
        Section::Addressed => (k.c.success.fg, k.c.success.bg),
        Section::Orphaned => (k.c.danger.fg, k.c.danger.bg),
        _ if chip_text.contains("failed") || chip_text.contains("timed out") => (k.c.danger.fg, k.c.danger.bg),
        _ => (k.c.text_muted, k.c.layer_02),
    };
    let show_chip = !matches!(chip_text.as_str(), "open" | "");
    let mut top = row()
        .w_full()
        .gap(6.0)
        .items_center()
        .child(text(glyph(t)).font(TYPE.small).color(kind_ink(t, k)))
        .child(text(who).font(TYPE.label).color(k.c.text_base))
        .child(text(t.kind().label()).font(TYPE.small).color(kind_ink(t, k)));
    if show_chip {
        top = top.child(
            row()
                .key("chip")
                .h(18.0)
                .px(6.0)
                .items_center()
                .radius(radius::CHIP)
                .fill(chip_bg)
                .child(text(chip_text).font(TYPE.micro).color(chip_fg)),
        );
    }
    top = top.child(el().grow(1.0).a11y_hidden()).child(
        text(short_place(s, t))
            .font(TYPE.micro_mono)
            .color(k.c.text_accent)
            .truncate()
            .shrink(),
    );
    let mut c = col()
        .key(format!("conv-{}", t.id))
        .role("button")
        .label(format!("{head} · {}", location(s, t)))
        .w_full()
        .px(12.0)
        .py(10.0)
        .gap(6.0)
        .radius(radius::CARD)
        // Each thread is its own card on the island, so neighbours never run together.
        .fill(surf.card_head)
        .border(1.0, surf.card_border)
        .on_press_tx(move |s: &mut App, tx: &mut Tx| s.show_thread(&id, tx))
        .child(top)
        .child(text(said).font(TYPE.small).color(k.c.text_base).truncate());
    c = if focused {
        c.fill(k.c.pressed).outline(2.0, 0.0, k.c.border_focus)
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
        let mut r = row().gap(6.0).pt(2.0).child(small_button(
            format!("conv-confirm-{}", t.id),
            "Confirm  a",
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
        .pt(2.0)
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

/// `WAITING ON YOU` as `Waiting on you`.
fn sentence(t: &str) -> String {
    let lower = t.to_lowercase();
    let mut c = lower.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}

fn section_header(s: &App, section: Section, count: usize, k: &Kit) -> El {
    let folded = section == Section::Settled && !s.conversation.settled_open;
    let ink = if section == Section::Orphaned {
        k.c.danger.fg
    } else {
        k.c.text_base
    };
    let mut r = row()
        .key(format!("conv-section-{}", section.key()))
        .w_full()
        .h(30.0)
        .px(8.0)
        .pt(4.0)
        .gap(6.0)
        .items_center()
        .child(icons::draw(
            if folded { &icons::CHEV_R } else { &icons::CHEV_D },
            14.0,
            k.c.text_muted,
        ))
        .child(text(sentence(section.title())).font(TYPE.label).color(ink))
        .child(text(count.to_string()).font(TYPE.small).color(k.c.text_faint));
    if section == Section::Settled {
        let open = s.conversation.settled_open;
        r = r
            .role("button")
            .label(if open { "Fold settled" } else { "Show settled" })
            .cursor(rune::Cursor::Pointer)
            .on_press(|s: &mut App| s.conversation.settled_open = !s.conversation.settled_open);
    }
    r
}

fn verdict_colors(v: Verdict, k: &Kit) -> (Color, Color) {
    match v {
        Verdict::Approve => (k.c.success.fg, k.c.success.bg),
        Verdict::RequestChanges => (k.c.warning.fg, k.c.warning.bg),
        Verdict::Comment => (k.c.info.fg, k.c.info.bg),
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
    let (surf, _) = sr(k);
    s.mirror
        .rounds
        .iter()
        .rev()
        .map(|r| {
            let line = format!("Round {} · {} · {}", r.n, verdict_label(r.verdict), first_sentence(&r.summary, 70));
            let (fg, bg) = verdict_colors(r.verdict, k);
            col()
                .key(format!("conv-round-{}", r.n))
                .label(line)
                .w_full()
                .px(12.0)
                .py(10.0)
                .gap(6.0)
                .radius(radius::CARD)
                .fill(surf.card_head)
                .border(1.0, surf.card_border)
                .child(
                    row()
                        .gap(8.0)
                        .items_center()
                        .child(text(format!("Round {}", r.n)).font(TYPE.label).color(k.c.text_base))
                        .child(
                            row()
                                .h(18.0)
                                .px(6.0)
                                .items_center()
                                .radius(radius::CHIP)
                                .fill(bg)
                                .child(text(verdict_label(r.verdict)).font(TYPE.micro).color(fg)),
                        ),
                )
                .child(
                    text(first_sentence(&r.summary, 90))
                        .font(TYPE.small)
                        .color(k.c.text_base)
                        .truncate(),
                )
        })
        .collect()
}

fn filter_chip(key: &'static str, label: String, on: bool, k: &Kit) -> El {
    let (surf, _) = sr(k);
    let mut c = row()
        .key(format!("conv-filter-{key}"))
        .role("button")
        .label(label.clone())
        .checked(on)
        .h(24.0)
        .px(10.0)
        .items_center()
        .radius(12.0)
        .border(1.0, if on { surf.strong_line } else { surf.card_border })
        .cursor(rune::Cursor::Pointer)
        .child(
            text(label)
                .font(TYPE.small)
                .color(if on { k.c.text_base } else { k.c.text_muted })
                .a11y_hidden(),
        )
        .on_press(move |s: &mut App| s.toggle_conversation_filter(key));
    if on {
        c = c.fill(k.c.pressed);
    }
    k.interactive(c)
}

fn body(s: &App, k: &Kit) -> El {
    let mut list = col().w_full().px(10.0).pb(12.0).gap(8.0);
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
                    .w_full()
                    .h(30.0)
                    .px(8.0)
                    .pt(4.0)
                    .gap(6.0)
                    .items_center()
                    .child(icons::draw(&icons::CHEV_D, 14.0, k.c.text_muted))
                    .child(text("Rounds").font(TYPE.label).color(k.c.text_base))
                    .child(text(rounds.len().to_string()).font(TYPE.small).color(k.c.text_faint)),
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

/// The Conversation island at the window's right: its title and waiting count, the agent's
/// presence, the filters, then every thread by section and the rounds.
pub(super) fn conversation(s: &App, k: &Kit) -> Option<El> {
    let (surf, _) = sr(k);
    let state = s.presence_state();
    let f = &s.conversation.filter;
    let state_label = f
        .state
        .map_or_else(|| "State ▾".to_owned(), |st| format!("State: {} ▾", st.label()));
    let waiting = s.waiting_count();
    let mut header = row()
        .key("conv-header")
        .w_full()
        .h(42.0)
        .pl(14.0)
        .pr(8.0)
        .gap(8.0)
        .items_center()
        .child(text("Conversation").font(TYPE.title).color(k.c.text_base));
    if waiting > 0 {
        header = header.child(
            row()
                .h(18.0)
                .px(6.0)
                .items_center()
                .radius(radius::CHIP)
                .fill(k.c.warning.bg)
                .child(text(format!("{waiting} waiting")).font(TYPE.micro).color(k.c.warning.fg)),
        );
    }
    header = header.child(el().grow(1.0).a11y_hidden()).child(
        Ghost::new("overlay-conversation-close")
            .icon(&icons::CLOSE)
            .help("Close  Esc")
            .height(24.0)
            .build(k)
            .on_press(|s: &mut App| {
                s.close_overlay(Overlay::Conversation);
            }),
    );
    let presence = row()
        .key("conv-presence")
        .w_full()
        .px(14.0)
        .pb(8.0)
        .gap(6.0)
        .items_center()
        .child(dot(presence_color(state, k), 8.0))
        .child(text(s.presence_text()).font(TYPE.small).color(k.c.text_muted).truncate().shrink());
    let all = f.is_empty();
    let filters = row()
        .key("conv-filters")
        .w_full()
        .px(14.0)
        .pb(8.0)
        .gap(6.0)
        .flex_wrap()
        .child(
            k.interactive(
                row()
                    .key("conv-filter-all")
                    .role("button")
                    .label("All")
                    .checked(all)
                    .h(24.0)
                    .px(10.0)
                    .items_center()
                    .radius(12.0)
                    .border(1.0, if all { surf.strong_line } else { surf.card_border })
                    .fill(if all { k.c.pressed } else { Color::TRANSPARENT })
                    .child(
                        text("All")
                            .font(TYPE.small)
                            .color(if all { k.c.text_base } else { k.c.text_muted })
                            .a11y_hidden(),
                    ),
            )
            .on_press(|s: &mut App| s.conversation.filter = Default::default()),
        )
        .child(filter_chip("waiting", "Waiting".into(), f.waiting, k))
        .child(filter_chip("agent", "Agent".into(), f.agent, k))
        .child(filter_chip("mine", "Mine".into(), f.mine, k))
        .child(filter_chip("state", state_label, f.state.is_some(), k));
    Some(
        col()
            .key("overlay-conversation")
            .role("complementary")
            .label("Conversation")
            .w(WIDTH)
            .h_full()
            .fill(surf.panel)
            .on_press(|_: &mut App| {})
            .child(header)
            .child(presence)
            .child(filters)
            .child(el().w_full().h(1.0).fill(surf.hairline))
            .child(body(s, k)),
    )
}
