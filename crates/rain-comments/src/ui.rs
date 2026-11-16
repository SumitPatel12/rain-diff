//! Thread cards and the comment composer, generic over the app state `S` like rune-kit's
//! components: build them in a view with `.build(&kit)`, and get what the user did back
//! through one action handler.

use std::rc::Rc;

use rain_core::{Anchor, AnchorState, AnswerStatus, CodeSnapshot, DismissReason, Disposition, Side, Thread, ThreadKind, ThreadState};
use rune::{Color, El, Key, KeyEvent, TextEdit, TextStyle, Tx, col, row, text};
use rune_kit::{Kit, Markdown, TextArea};

use crate::export::suggestions;
use crate::label;
use crate::ops;

/// What the user asked a thread card to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CardAction {
    Toggle,
    OpenReply,
    /// Reply with a question for the agent (`@claude`): Ask on the user's own thread.
    AskReply,
    CancelReply,
    Reply(String),
    /// Set this disposition, or clear it when the thread already has it.
    Disposition(Disposition),
    /// Show the dismiss reasons; [`CardAction::Dismiss`] picks one.
    OpenDismiss,
    Dismiss(DismissReason),
    /// The user agrees the thread was addressed.
    Confirm,
    Reopen,
    Resolve,
    /// Ask again after a failed, timed-out or cancelled answer.
    Retry,
    CancelAnswer,
    /// Re-anchor an orphan where the cursor is.
    Attach,
    /// Close an orphan; its snapshot stays.
    CloseOrphan,
    EditMessage(String),
    /// Removes the user's own thread; agents' threads are resolved or dismissed instead.
    Delete,
    /// The pointer entered (`true`) or left the card.
    Hover(bool),
}

/// The one-key responses (`a x ? d`), `r` to reply and Enter or Space to open or close: what a
/// focused card does with a key. The shell calls it too when its own cursor is on a card.
pub fn card_key(ev: &KeyEvent) -> Option<CardAction> {
    if ev.mods.cmd || ev.mods.ctrl || ev.mods.alt {
        return None;
    }
    match &ev.key {
        Key::Enter => Some(CardAction::Toggle),
        Key::Text(t) => match t.as_str() {
            " " => Some(CardAction::Toggle),
            "a" => Some(CardAction::Disposition(Disposition::Agree)),
            "x" => Some(CardAction::OpenDismiss),
            "?" => Some(CardAction::Disposition(Disposition::Ask)),
            "d" => Some(CardAction::Disposition(Disposition::Defer)),
            "r" => Some(CardAction::OpenReply),
            _ => None,
        },
        _ => None,
    }
}

/// [`card_key`] for `t`: `a` confirms a thread that reads as addressed; the user's own threads
/// take no dispositions.
pub fn card_key_for(t: &Thread, ev: &KeyEvent) -> Option<CardAction> {
    match card_key(ev)? {
        CardAction::Disposition(Disposition::Agree) if confirmable(t) => Some(CardAction::Confirm),
        CardAction::Disposition(_) | CardAction::OpenDismiss if !takes_dispositions(t) => None,
        a => Some(a),
    }
}

/// Agree · Dismiss · Ask · Defer answer someone else's finding, so only those threads take them.
pub fn takes_dispositions(t: &Thread) -> bool {
    !is_mine(t.messages.first().and_then(|m| m.author.as_deref()))
}

/// The ladder suggests the thread was addressed, or the agent said so and the user has not
/// confirmed it yet.
pub fn confirmable(t: &Thread) -> bool {
    ops::addressed_suggested(t) || (t.state == ThreadState::Addressed && t.disposition.is_none())
}

pub const DISPOSITIONS: [(Disposition, &str, &str); 4] = [
    (Disposition::Agree, "a", "Agree"),
    (Disposition::Dismiss, "x", "Dismiss"),
    (Disposition::Ask, "?", "Ask"),
    (Disposition::Defer, "d", "Defer"),
];

/// `server/src/X.cs:L12`, `…:L12-L14`, `…:L12C5-L14C20` (1-based columns), ` (old)` on the old side.
pub fn anchor_label(path: &str, anchor: &Anchor) -> String {
    let lines = match *anchor {
        Anchor::Line { line, .. } => format!("L{line}"),
        Anchor::Range { start, end, .. } => format!("L{start}-L{end}"),
        Anchor::Span {
            start_line,
            start_col,
            end_line,
            end_col,
            ..
        } => {
            format!("L{start_line}C{}-L{end_line}C{}", u64::from(start_col) + 1, u64::from(end_col) + 1)
        }
    };
    let old = if anchor.side() == Side::Old { " (old)" } else { "" };
    format!("{path}:{lines}{old}")
}

/// A body as Markdown to draw: each ```` ```suggestion ```` block becomes a `diff` fence of
/// the snapshot's lines (`-`) and the suggestion's (`+`).
pub fn display_body(body: &str, snapshot: Option<&CodeSnapshot>) -> String {
    let blocks = suggestions(body);
    if blocks.is_empty() {
        return body.to_owned();
    }
    let mut out = String::with_capacity(body.len() + 64);
    let mut last = 0;
    for b in &blocks {
        out.push_str(&body[last..b.start]);
        out.push_str("```diff\n");
        if let Some(s) = snapshot.filter(|s| !s.content.is_empty()) {
            for l in s.content.lines() {
                out.push('-');
                out.push_str(l);
                out.push('\n');
            }
        }
        for l in b.code.lines() {
            out.push('+');
            out.push_str(l);
            out.push('\n');
        }
        out.push_str("```");
        last = b.end;
    }
    out.push_str(&body[last..]);
    out
}

/// A ```` ```suggestion ```` block holding `code`, for the composer's `+ suggestion`.
pub fn suggestion_block(code: &str) -> String {
    format!("```suggestion\n{}\n```", code.trim_end_matches('\n'))
}

/// The composer's two modes: a review comment, or a question the agent answers right away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ComposerMode {
    #[default]
    Comment,
    Question,
}

/// The types Comment mode offers; Question is the other mode.
pub const COMMENT_KINDS: [ThreadKind; 5] = [
    ThreadKind::Comment,
    ThreadKind::Issue,
    ThreadKind::Suggestion,
    ThreadKind::Nit,
    ThreadKind::Praise,
];

/// Whether `text` addresses Claude (`@claude`), which asks for an answer.
pub fn mentions_claude(text: &str) -> bool {
    text.to_ascii_lowercase().contains("@claude")
}

/// The mode a send uses: `@claude` in a comment makes it a question.
pub fn effective_mode(mode: ComposerMode, text: &str) -> ComposerMode {
    if mode == ComposerMode::Comment && mentions_claude(text) {
        ComposerMode::Question
    } else {
        mode
    }
}

fn kind_tone(k: &Kit, kind: ThreadKind, blocking: bool) -> Color {
    match kind {
        ThreadKind::Issue if blocking => k.c.danger.icon,
        ThreadKind::Issue => k.c.warning.icon,
        ThreadKind::Question => k.c.info.icon,
        ThreadKind::Suggestion | ThreadKind::Praise => k.c.success.icon,
        _ if blocking => k.c.danger.icon,
        _ => k.c.icon_muted,
    }
}

fn short_time(iso: &str) -> String {
    iso.get(..16).map_or_else(|| iso.to_owned(), |s| s.replacen('T', " ", 1))
}

/// Milliseconds since the epoch of a `2026-11-06T10:00:00.000Z` timestamp.
pub fn iso_millis(iso: &str) -> Option<i64> {
    let b = iso.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let num = |r: std::ops::Range<usize>| iso.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hh, mm, ss) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let ms = if b.get(19) == Some(&b'.') { num(20..23).unwrap_or(0) } else { 0 };
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 86_400 + hh * 3600 + mm * 60 + ss) * 1000) + ms)
}

/// Whole seconds from `from` to `to`, both ISO timestamps; `None` when either doesn't parse.
pub fn secs_between(from: &str, to: &str) -> Option<u64> {
    Some(((iso_millis(to)? - iso_millis(from)?).max(0) / 1000) as u64)
}

/// What a question thread's answer chip reads.
pub fn answer_text(t: &Thread, now: Option<&str>) -> Option<String> {
    let a = t.answer.as_ref()?;
    let by = a.by.as_deref().map_or(String::new(), |b| format!(" ({b})"));
    Some(match a.status {
        AnswerStatus::Queued => match a.error.as_deref().filter(|e| !e.is_empty()) {
            Some(note) => format!("queued · {note}"),
            None => "Sent · waiting for an answer".to_owned(),
        },
        AnswerStatus::Answering => {
            let since = a.started_at.as_deref().unwrap_or(&t.created_at);
            match now.and_then(|n| secs_between(since, n)) {
                Some(s) => format!("Claude is answering… {s} s"),
                None => "Claude is answering…".to_owned(),
            }
        }
        AnswerStatus::Answered => format!("answered by claude{by}"),
        AnswerStatus::Failed => match a.error.as_deref().filter(|e| !e.is_empty()) {
            Some(e) => format!("failed · {}", label::first_sentence(e, 60)),
            None => "failed".to_owned(),
        },
        AnswerStatus::Timeout => "timed out".to_owned(),
        AnswerStatus::Cancelled => "cancelled".to_owned(),
    })
}

/// The anchor chip: `moved (exact)`, `outdated`, `orphaned · line deleted`; `None` when placed.
pub fn anchor_text(t: &Thread) -> Option<String> {
    match t.placement() {
        AnchorState::Placed => None,
        AnchorState::Moved => Some(match t.anchor_how {
            Some(h) => format!("moved ({})", h.label()),
            None => "moved".to_owned(),
        }),
        AnchorState::Outdated => Some("outdated".to_owned()),
        AnchorState::Orphaned => Some(match &t.orphan {
            Some(o) => format!("orphaned · {}", o.reason.label()),
            None => "orphaned".to_owned(),
        }),
    }
}

/// The state chip for a thread that is no longer open.
pub fn state_text(t: &Thread) -> Option<String> {
    match t.state {
        ThreadState::Open => None,
        ThreadState::Addressed => Some(match t.state_sha.as_deref() {
            Some(sha) => format!("addressed in {}", &sha[..sha.len().min(7)]),
            None => "addressed".to_owned(),
        }),
        ThreadState::Resolved => Some("resolved".to_owned()),
        ThreadState::Dismissed => Some(match t.dismiss_reason {
            Some(r) => format!("dismissed · {}", r.label()),
            None => "dismissed".to_owned(),
        }),
    }
}

fn is_mine(author: Option<&str>) -> bool {
    author.map(str::trim).is_none_or(str::is_empty)
}

type ActionFn<S> = Rc<dyn Fn(&mut S, &str, CardAction, &mut Tx)>;
type EditAccess<S> = Rc<dyn for<'a> Fn(&'a mut S) -> &'a mut TextEdit>;

/// The colours a card paints beyond the kit's: its fill and ring, the focus outline, the drop
/// shadow, the hairline between replies, and the avatars' (fill, letter) for the agent and you.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardLook {
    pub fill: Color,
    pub border: Color,
    pub focus: Color,
    pub shadow: Color,
    pub hairline: Color,
    pub agent: (Color, Color),
    pub you: (Color, Color),
}

impl CardLook {
    /// From the kit alone: the card head layer, its muted border, the focus ring.
    pub fn from_kit(k: &Kit) -> CardLook {
        CardLook {
            fill: k.c.layer_01,
            border: k.c.border_muted,
            focus: k.c.border_focus,
            shadow: Color::TRANSPARENT,
            hairline: k.c.border_weak,
            agent: (k.alpha(k.c.info.icon, 0.2), k.c.info.fg),
            you: (k.alpha(k.c.success.icon, 0.2), k.c.success.fg),
        }
    }
}

/// One thread. Expanded (the diff's default): a head with the author, kind, chips, time and the
/// fold button; the messages; then Reply · Ask · Resolve and, quieter, the dispositions.
/// Collapsed (in hosts without a gutter): one line, `● issue · Claude · first sentence`. Orphans
/// keep their last-seen code in view either way.
pub struct ThreadCard<'t, S> {
    thread: &'t Thread,
    collapsed: bool,
    focused: bool,
    hovered: bool,
    dismissing: bool,
    settled_dim: bool,
    now: Option<String>,
    location: Option<String>,
    lines: Option<String>,
    look: Option<CardLook>,
    reply: Option<(TextEdit, EditAccess<S>)>,
    on_action: Option<ActionFn<S>>,
}

impl<'t, S: 'static> ThreadCard<'t, S> {
    pub fn new(thread: &'t Thread) -> Self {
        ThreadCard {
            thread,
            collapsed: true,
            focused: false,
            hovered: false,
            dismissing: false,
            settled_dim: true,
            now: None,
            location: None,
            lines: None,
            look: None,
            reply: None,
            on_action: None,
        }
    }

    pub fn collapsed(mut self, c: bool) -> Self {
        self.collapsed = c;
        self
    }

    /// The shell's cursor is on this card: its response buttons show without a hover.
    pub fn focused(mut self, f: bool) -> Self {
        self.focused = f;
        self
    }

    /// The pointer is over the card: a collapsed card shows its actions.
    pub fn hovered(mut self, h: bool) -> Self {
        self.hovered = h;
        self
    }

    /// Shows the dismiss reasons.
    pub fn dismissing(mut self, d: bool) -> Self {
        self.dismissing = d;
        self
    }

    /// The time now (ISO), for the seconds an answer has been running.
    pub fn now(mut self, iso: impl Into<String>) -> Self {
        self.now = Some(iso.into());
        self
    }

    /// Where the thread sits, e.g. [`anchor_label`]'s: the collapsed line's tooltip and the
    /// expanded head's location when no short `lines` label is given.
    pub fn location(mut self, l: impl Into<String>) -> Self {
        self.location = Some(l.into());
        self
    }

    /// The short location the expanded head shows (`L23`), the full one in its tooltip.
    pub fn lines(mut self, l: impl Into<String>) -> Self {
        self.lines = Some(l.into());
        self
    }

    pub fn look(mut self, l: CardLook) -> Self {
        self.look = Some(l);
        self
    }

    /// Shows the inline reply composer bound to `edit`; its Ctrl+Enter sends
    /// [`CardAction::Reply`] and Esc [`CardAction::CancelReply`].
    pub fn reply_composer(mut self, edit: &TextEdit, access: impl for<'a> Fn(&'a mut S) -> &'a mut TextEdit + 'static) -> Self {
        self.reply = Some((edit.clone(), Rc::new(access)));
        self
    }

    pub fn on_action(mut self, f: impl Fn(&mut S, &str, CardAction, &mut Tx) + 'static) -> Self {
        self.on_action = Some(Rc::new(f));
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let t = self.thread;
        let look = self.look.unwrap_or_else(|| CardLook::from_kit(k));
        let id: Rc<str> = Rc::from(t.id.as_str());
        let first = t.messages.first().map_or("", |m| m.body.as_str());
        let parsed = label::parse(first);
        let kind = t.kind();
        let blocking = t.blocking();
        let ink = kind_tone(k, kind, blocking);
        let author = t
            .messages
            .first()
            .and_then(|m| m.author.clone())
            .filter(|a| !a.trim().is_empty())
            .unwrap_or_else(|| "you".into());
        let summary = label::first_sentence(parsed.as_ref().map_or(first, |l| l.rest), 120);
        let act = self.on_action.clone();
        let fire = move |a: CardAction| {
            let act = act.clone();
            let id = id.clone();
            move |s: &mut S, tx: &mut Tx| {
                if let Some(f) = &act {
                    f(s, &id, a.clone(), tx);
                }
            }
        };
        let mut tag = String::new();
        if let Some(f) = parsed.as_ref().and_then(|l| l.finding) {
            tag.push_str(f);
            tag.push(' ');
        }
        if kind != ThreadKind::Comment || parsed.is_some() {
            tag.push_str(
                parsed
                    .as_ref()
                    .filter(|_| t.kind.is_none())
                    .map_or(kind.label(), |l| l.kind.label()),
            );
        } else {
            tag.push_str("comment");
        }
        let tag = tag.trim().to_owned();

        let mut card = col()
            .key(format!("thread-{}", t.id))
            .role("article")
            .label(format!("Comment by {author}"))
            .w_full()
            .radius(8.0)
            .fill(look.fill)
            .border(1.0, look.border)
            .focusable(true);
        if look.shadow.a > 0.0 {
            card = card.shadow(6.0, 16.0, look.shadow);
        }
        if self.settled_dim && matches!(t.state, ThreadState::Resolved | ThreadState::Dismissed) {
            card = card.opacity(0.75);
        }
        if let Some(f) = self.on_action.clone() {
            let hover = f.clone();
            let hid: Rc<str> = Rc::from(t.id.as_str());
            card = card.on_hover_change(move |s: &mut S, inside: bool, tx: &mut Tx| hover(s, &hid, CardAction::Hover(inside), tx));
            let id: Rc<str> = Rc::from(t.id.as_str());
            let own = format!("thread-{}", t.id);
            let thread = t.clone();
            card = card.on_key(move |s: &mut S, ev: &KeyEvent, tx: &mut Tx| match card_key_for(&thread, ev) {
                // Enter and Space still press a focused chip or button inside the card.
                Some(CardAction::Toggle) if tx.focused().is_some_and(|k| k != own && k != "head") => false,
                Some(a) => {
                    f(s, &id, a, tx);
                    true
                }
                None => false,
            });
        }

        if self.collapsed {
            let mut head = row()
                .key("head")
                .w_full()
                .h(28.0)
                .px(8.0)
                .gap(8.0)
                .center_y()
                .role("button")
                .label("Expand thread")
                .expanded(false)
                .child(text(label::kind_glyph(kind)).font(k.ty.ui_medium).color(ink).a11y_hidden())
                .child(text(author.clone()).key("author").font(STRONG).color(k.c.text_base))
                .child(text(tag).key("tag").font(k.ty.small).color(ink))
                .child(
                    text(summary)
                        .key("summary")
                        .font(k.ty.ui)
                        .color(k.c.text_muted)
                        .truncate()
                        .grow(1.0)
                        .shrink(),
                );
            if let Some(l) = &self.location {
                head = head.help(l.clone());
            }
            if t.messages.len() > 1 {
                head = head.child(
                    text(format!("{}↩", t.messages.len() - 1))
                        .key("replies")
                        .font(k.ty.small)
                        .color(k.c.text_faint)
                        .label(format!("{} replies", t.messages.len() - 1)),
                );
            }
            head = self.chips(head, k, &fire);
            if (self.focused || self.hovered) && (confirmable(t) || takes_dispositions(t)) {
                head = head.child(self.dispositions(k, &fire));
            }
            card = card.child(k.interactive(head.radius(8.0)).on_press_tx(fire(CardAction::Toggle)));
            if self.dismissing {
                card = card.child(dismiss_reasons(k, &fire));
            }
            if t.placement() == AnchorState::Orphaned {
                card = card.child(orphan_block(k, t, &fire));
            }
            return card;
        }

        // Expanded: avatar · who · kind · chips · where · · · time · fold.
        let mut head = row()
            .key("head")
            .w_full()
            .pl(12.0)
            .pr(8.0)
            .pt(8.0)
            .pb(2.0)
            .gap(8.0)
            .center_y()
            .child(avatar(&author, &look))
            .child(text(author.clone()).key("author").font(STRONG_SMALL).color(k.c.text_base))
            .child(text(tag).key("tag").font(k.ty.small).color(ink));
        if blocking {
            head = head.child(tone_chip("blocking", "blocking", k.c.danger.fg, k.c.danger.bg));
        }
        let place = self.lines.clone().or(self.location.clone());
        if let Some(p) = place {
            let mut l = text(p).key("summary").font(MONO_MICRO).color(k.c.text_faint).truncate().shrink();
            if let Some(full) = &self.location {
                l = l.help(full.clone());
            }
            head = head.child(l);
        }
        if ops::waiting_on_user(t, "") {
            head = head.child(tone_chip("waiting-chip", "waiting on you", k.c.warning.fg, k.c.warning.bg));
        }
        if let Some(st) = state_text(t) {
            let (fg, bg) = match t.state {
                ThreadState::Addressed => (k.c.success.fg, k.c.success.bg),
                _ => (k.c.text_muted, k.c.layer_02),
            };
            head = head.child(tone_chip("state", &st, fg, bg));
        }
        if ops::addressed_suggested(t) {
            head = head.child(tone_chip("addressed", "addressed?", k.c.success.fg, k.c.success.bg));
        }
        if let Some(a) = anchor_text(t) {
            let (fg, bg) = if t.placement() == AnchorState::Orphaned {
                (k.c.danger.fg, k.c.danger.bg)
            } else {
                (k.c.text_muted, k.c.layer_02)
            };
            head = head.child(tone_chip("anchor-state", &a, fg, bg));
        }
        let answer = t.answer.as_ref().map(|a| a.status);
        if let Some(ans) = answer_text(t, self.now.as_deref()).filter(|_| !answer.is_some_and(|a| a.pending())) {
            let (fg, bg) = match answer {
                Some(AnswerStatus::Answered) => (k.c.success.fg, k.c.success.bg),
                Some(AnswerStatus::Failed | AnswerStatus::Timeout) => (k.c.danger.fg, k.c.danger.bg),
                _ => (k.c.text_muted, k.c.layer_02),
            };
            head = head.child(row().key("answer").child(tone_chip("answer-text", &ans, fg, bg)));
        }
        head = head.child(rune::el().grow(1.0).a11y_hidden());
        if let Some(m) = t.messages.first() {
            head = head.child(
                text(clock(&m.created_at))
                    .key("time")
                    .font(MICRO)
                    .color(k.c.text_faint)
                    .help(short_time(&m.created_at)),
            );
        }
        head = head.child(
            k.interactive(
                row()
                    .key("fold")
                    .role("button")
                    .label("Fold into the gutter")
                    .help("Fold into the gutter  Enter")
                    .size(22.0, 22.0)
                    .center()
                    .radius(6.0)
                    .focusable(true)
                    .child(rune::icon(&FOLD, 14.0).color(k.c.text_muted)),
            )
            .on_press_tx(fire(CardAction::Toggle)),
        );
        card = card.child(head.role("group").label("Thread head"));
        if self.dismissing {
            card = card.child(dismiss_reasons(k, &fire));
        }
        if t.placement() == AnchorState::Orphaned {
            card = card.child(orphan_block(k, t, &fire));
        }

        let mut body = col().key("messages").w_full();
        for (i, m) in t.messages.iter().enumerate() {
            let who = m.author.clone().filter(|a| !a.trim().is_empty()).unwrap_or_else(|| "you".into());
            let md = Markdown::new(display_body(&m.body, t.code_snapshot.as_ref()))
                .selectable(true)
                .build(k);
            if i == 0 {
                body = body.child(
                    col()
                        .key(format!("m-{}", m.id))
                        .role("listitem")
                        .w_full()
                        .pl(38.0)
                        .pr(12.0)
                        .pt(2.0)
                        .pb(8.0)
                        .child(col().key("msg").w_full().child(md)),
                );
                continue;
            }
            let mut meta = row()
                .key("meta")
                .gap(8.0)
                .center_y()
                .child(avatar(&who, &look))
                .child(text(who).font(STRONG_SMALL).color(k.c.text_base))
                .child(
                    text(clock(&m.created_at))
                        .font(k.ty.small)
                        .color(k.c.text_muted)
                        .help(short_time(&m.created_at)),
                );
            if is_mine(m.author.as_deref()) {
                meta = meta.child(
                    k.interactive(
                        row()
                            .key(format!("edit-{}", m.id))
                            .role("button")
                            .label("Edit")
                            .px(4.0)
                            .radius(4.0)
                            .focusable(true)
                            .child(text("· Edit").font(k.ty.small).color(k.c.text_faint).a11y_hidden()),
                    )
                    .on_press_tx(fire(CardAction::EditMessage(m.id.clone()))),
                );
            }
            let line = rune::el()
                .pin(Some(38.0), Some(0.0), Some(12.0), None)
                .h(1.0)
                .fill(look.hairline)
                .a11y_hidden();
            body = body.child(
                col()
                    .key(format!("m-{}", m.id))
                    .role("listitem")
                    .w_full()
                    .pl(38.0)
                    .pr(12.0)
                    .py(6.0)
                    .gap(2.0)
                    .child(meta)
                    .child(col().key("msg").w_full().pl(26.0).child(md))
                    .child(line),
            );
        }
        card = card.child(body);
        if t.answer.as_ref().is_some_and(|a| a.status.pending()) {
            let label = answer_text(t, self.now.as_deref()).unwrap_or_default();
            card = card.child(
                row()
                    .key("answer")
                    .ml(38.0)
                    .mr(12.0)
                    .mb(8.0)
                    .px(10.0)
                    .py(6.0)
                    .gap(8.0)
                    .center_y()
                    .radius(6.0)
                    .fill(k.c.info.bg)
                    .child(
                        rune::el()
                            .size(12.0, 12.0)
                            .radius(6.0)
                            .border(2.0, k.alpha(k.c.info.fg, 0.35))
                            .a11y_hidden(),
                    )
                    .child(text(label).key("answer-text").font(k.ty.small).color(k.c.info.fg).grow(1.0))
                    .child(
                        k.interactive(
                            row()
                                .key("cancel-answer")
                                .role("button")
                                .label("Cancel the answer")
                                .px(4.0)
                                .radius(4.0)
                                .focusable(true)
                                .child(text("Cancel the answer").font(k.ty.small).color(k.c.info.fg).a11y_hidden()),
                        )
                        .on_press_tx(fire(CardAction::CancelAnswer)),
                    ),
            );
        }
        if let Some((edit, access)) = self.reply {
            let submit = self.on_action.clone();
            let cancel = self.on_action.clone();
            let (sid, cid): (Rc<str>, Rc<str>) = (Rc::from(t.id.as_str()), Rc::from(t.id.as_str()));
            let placeholder = if kind == ThreadKind::Question {
                "Follow-up question… Claude answers here (Ctrl+Enter to send, Esc to cancel)"
            } else {
                "Reply… @claude asks Claude (Ctrl+Enter to send, Esc to cancel)"
            };
            let composer = Composer::new_rc(&edit, access)
                .key("reply")
                .placeholder(placeholder)
                .on_submit(move |s: &mut S, body: String, tx: &mut Tx| {
                    if let Some(f) = &submit {
                        f(s, &sid, CardAction::Reply(body), tx);
                    }
                })
                .on_cancel(move |s: &mut S, tx: &mut Tx| {
                    if let Some(f) = &cancel {
                        f(s, &cid, CardAction::CancelReply, tx);
                    }
                })
                .build(k);
            return card.child(col().w_full().pl(38.0).pr(12.0).pb(10.0).child(composer));
        }
        let mut actions = row()
            .key("actions")
            .role("toolbar")
            .label("Thread actions")
            .w_full()
            .pl(38.0)
            .pr(12.0)
            .pt(6.0)
            .pb(10.0)
            .gap(6.0)
            .center_y();
        if t.answer
            .as_ref()
            .is_some_and(|a| matches!(a.status, AnswerStatus::Failed | AnswerStatus::Timeout | AnswerStatus::Cancelled))
        {
            actions = actions.child(small_button(k, "retry-answer", "Retry", None, true).on_press_tx(fire(CardAction::Retry)));
        }
        actions = actions.child(small_button(k, "reply-btn", "Reply", Some("r"), true).on_press_tx(fire(CardAction::OpenReply)));
        let ask = if takes_dispositions(t) {
            CardAction::Disposition(Disposition::Ask)
        } else {
            CardAction::AskReply
        };
        actions = actions.child(small_button(k, "ask-btn", "Ask", Some("?"), true).on_press_tx(fire(ask)));
        actions = if t.is_open() {
            actions.child(
                small_button(k, "resolve-btn", "Resolve", None, true)
                    .min_w(STATE_BUTTON_W)
                    .on_press_tx(fire(CardAction::Resolve)),
            )
        } else {
            actions.child(
                small_button(k, "reopen-btn", "Reopen", None, true)
                    .min_w(STATE_BUTTON_W)
                    .on_press_tx(fire(CardAction::Reopen)),
            )
        };
        if t.messages.iter().all(|m| is_mine(m.author.as_deref())) {
            actions = actions.child(small_button(k, "delete-btn", "Delete", None, true).on_press_tx(fire(CardAction::Delete)));
        }
        actions = actions.child(rune::el().grow(1.0).a11y_hidden());
        if confirmable(t) || takes_dispositions(t) {
            actions = actions.child(self.dispositions(k, &fire));
        }
        card.child(actions)
    }

    /// State, addressed, anchor and answer chips, in that order.
    fn chips<F, H>(&self, mut head: El, k: &Kit, fire: &F) -> El
    where
        F: Fn(CardAction) -> H,
        H: Fn(&mut S, &mut Tx) + 'static,
    {
        let t = self.thread;
        if let Some(st) = state_text(t) {
            head = head.child(chip(k, "state", st, k.c.text_muted));
        }
        if ops::addressed_suggested(t) {
            head = head.child(chip(k, "addressed", "addressed?".into(), k.c.success.fg));
        }
        if let Some(a) = anchor_text(t) {
            let tone = if t.placement() == AnchorState::Orphaned {
                k.c.warning.fg
            } else {
                k.c.text_muted
            };
            head = head.child(chip(k, "anchor-state", a, tone));
        }
        if let Some(ans) = answer_text(t, self.now.as_deref()) {
            head = head.child(self.answer_chip(k, ans, fire));
        }
        head
    }

    fn answer_chip<F, H>(&self, k: &Kit, label: String, fire: &F) -> El
    where
        F: Fn(CardAction) -> H,
        H: Fn(&mut S, &mut Tx) + 'static,
    {
        let status = self.thread.answer.as_ref().map(|a| a.status);
        let tone = match status {
            Some(AnswerStatus::Answered) => k.c.success.fg,
            Some(AnswerStatus::Failed | AnswerStatus::Timeout) => k.c.danger.fg,
            _ => k.c.info.fg,
        };
        let mut r = row().key("answer").gap(4.0).center_y().child(chip(k, "answer-text", label, tone));
        match status {
            Some(s) if s.pending() => {
                r = r.child(
                    small_button(k, "cancel-answer", "✕", None, false)
                        .label("Cancel the answer")
                        .on_press_tx(fire(CardAction::CancelAnswer)),
                );
            }
            Some(AnswerStatus::Failed | AnswerStatus::Timeout | AnswerStatus::Cancelled) => {
                r = r.child(small_button(k, "retry-answer", "Retry", None, true).on_press_tx(fire(CardAction::Retry)));
            }
            _ => {}
        }
        r
    }

    /// The quiet responses: Confirm/Reopen for a thread that reads as addressed, else
    /// Agree · Dismiss · Defer (Ask sits with Reply).
    fn dispositions<F, H>(&self, k: &Kit, fire: &F) -> El
    where
        F: Fn(CardAction) -> H,
        H: Fn(&mut S, &mut Tx) + 'static,
    {
        let t = self.thread;
        if confirmable(t) {
            return row()
                .key("dispositions")
                .role("group")
                .label("Addressed?")
                .gap(4.0)
                .child(small_button(k, "confirm-btn", "Confirm addressed", Some("a"), true).on_press_tx(fire(CardAction::Confirm)))
                .child(small_button(k, "reopen-btn", "Reopen", None, false).on_press_tx(fire(CardAction::Reopen)));
        }
        let current = t.disposition;
        row().key("dispositions").role("group").label("Disposition").gap(2.0).children(
            DISPOSITIONS
                .iter()
                .filter(|(d, ..)| *d != Disposition::Ask || current == Some(Disposition::Ask))
                .map(|&(d, key, name)| {
                    let on = current == Some(d);
                    let chip = row()
                        .key(format!("disp-{}", d.label()))
                        .role("button")
                        .label(name)
                        .checked(on)
                        .help(format!("{name}  {key}"))
                        .h(24.0)
                        .px(8.0)
                        .gap(6.0)
                        .center()
                        .radius(5.0)
                        .fill(if on { k.c.pressed } else { Color::TRANSPARENT })
                        .child(
                            text(name)
                                .font(k.ty.small)
                                .color(if on { k.c.text_base } else { k.c.text_muted })
                                .a11y_hidden(),
                        );
                    let action = if d == Disposition::Dismiss && !on {
                        CardAction::OpenDismiss
                    } else {
                        CardAction::Disposition(d)
                    };
                    k.interactive(chip).focusable(true).on_press_tx(fire(action))
                }),
        )
    }
}

const STRONG: TextStyle = TextStyle::new(13.0, 600.0).lh(20.0);
const STRONG_SMALL: TextStyle = TextStyle::new(12.0, 600.0).lh(18.0);
const MICRO: TextStyle = TextStyle::new(11.0, 400.0).lh(16.0);
const MONO_MICRO: TextStyle = TextStyle::new(11.0, 400.0).mono().lh(16.0);

static FOLD: rune::Icon = rune::Icon {
    name: "rain-fold",
    view: 16.0,
    body: r#"<path d="m4.5 3.5 3.5 3 3.5-3M4.5 12.5l3.5-3 3.5 3" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// `16:12` from an ISO timestamp.
fn clock(iso: &str) -> String {
    iso.get(11..16).map_or_else(|| short_time(iso), str::to_owned)
}

/// An 18 point round avatar with the author's initial.
fn avatar(who: &str, look: &CardLook) -> El {
    let (bg, fg) = if who == "you" { look.you } else { look.agent };
    let letter = who.chars().next().map_or_else(String::new, |c| c.to_uppercase().collect());
    row()
        .key("avatar")
        .size(18.0, 18.0)
        .center()
        .radius(9.0)
        .fill(bg)
        .a11y_hidden()
        .child(text(letter).font(TextStyle::new(10.0, 700.0).lh(12.0)).color(fg))
}

fn tone_chip(key: &str, label: &str, fg: Color, bg: Color) -> El {
    let label = label.to_owned();
    row()
        .key(key.to_owned())
        .h(18.0)
        .px(6.0)
        .center()
        .radius(4.0)
        .fill(bg)
        .child(text(label).font(MICRO).color(fg).truncate())
}

fn chip(k: &Kit, key: &str, label: String, ink: Color) -> El {
    row()
        .key(key.to_owned())
        .h(18.0)
        .px(6.0)
        .center()
        .radius(k.radius.sm)
        .fill(k.c.layer_02)
        .border(1.0, k.c.border_muted)
        .child(text(label).font(k.ty.small_medium).color(ink).truncate())
}

fn dismiss_reasons<S: 'static, F, H>(k: &Kit, fire: &F) -> El
where
    F: Fn(CardAction) -> H,
    H: Fn(&mut S, &mut Tx) + 'static,
{
    let mut r = row()
        .key("dismiss-reasons")
        .role("group")
        .label("Dismiss because")
        .w_full()
        .px(12.0)
        .pb(8.0)
        .gap(6.0)
        .center_y()
        .child(text("Dismiss because").font(k.ty.small_medium).color(k.c.text_muted));
    for reason in DismissReason::ALL {
        let key = format!("dismiss-{}", reason.label().replace(' ', "-"));
        r = r.child(small_button(k, &key, reason.label(), None, true).on_press_tx(fire(CardAction::Dismiss(reason))));
    }
    r
}

/// The code (or artifact text) as it was when the thread lost its anchor, with where and when.
fn orphan_block<S: 'static, F, H>(k: &Kit, t: &Thread, fire: &F) -> El
where
    F: Fn(CardAction) -> H,
    H: Fn(&mut S, &mut Tx) + 'static,
{
    let (snap, sha) = match &t.orphan {
        Some(o) => (Some(&o.snapshot), o.last_seen_sha.as_deref()),
        None => (t.code_snapshot.as_ref(), t.snapshot_sha()),
    };
    let mut head = String::from("last seen");
    if let Some(sha) = sha {
        head.push_str(&format!(" at {}", &sha[..sha.len().min(7)]));
    }
    if let Some(s) = snap {
        let path = s.path.clone().unwrap_or_else(|| t.file_path.clone());
        match s.lines {
            Some((a, b)) if a == b => head.push_str(&format!(" · {path}:{a}")),
            Some((a, b)) => head.push_str(&format!(" · {path}:{a}-{b}")),
            None => head.push_str(&format!(" · {path}")),
        }
    }
    let content = snap.map_or("", |s| s.content.as_str());
    let first = snap.and_then(|s| s.lines).map_or(0, |l| l.0);
    let mut code = col().key("orphan-code").w_full().gap(0.0);
    for (i, l) in content.lines().take(8).enumerate() {
        let n = if first > 0 {
            format!("{:>4} │ ", first as usize + i)
        } else {
            "  │ ".to_owned()
        };
        code = code.child(text(format!("{n}{l}")).font(k.ty.mono).color(k.c.text_base).truncate());
    }
    if content.lines().count() > 8 {
        code = code.child(text("  │ …").font(k.ty.mono).color(k.c.text_faint));
    }
    col()
        .key("orphan")
        .role("group")
        .label("Last seen code")
        .w_full()
        .px(12.0)
        .pb(8.0)
        .gap(4.0)
        .child(text(head).key("orphan-where").font(k.ty.small).color(k.c.text_muted).truncate())
        .child(
            row()
                .w_full()
                .items_stretch()
                .gap(8.0)
                .padding(6.0)
                .radius(k.radius.sm)
                .fill(k.c.layer_02)
                .child(rail(k.c.warning.icon))
                .child(code.grow(1.0).shrink()),
        )
        .child(
            row()
                .gap(6.0)
                .child(small_button(k, "attach-btn", "Attach here", None, true).on_press_tx(fire(CardAction::Attach)))
                .child(small_button(k, "close-orphan-btn", "Close…", None, true).on_press_tx(fire(CardAction::CloseOrphan))),
        )
}

fn rail(c: Color) -> El {
    rune::el().key("rail").w(2.0).radius(1.0).fill(c).self_stretch().a11y_hidden()
}

/// A 24 point button: outlined (thread actions) or quiet, with its key after the name.
/// Resolve and Reopen take each other's place; both are this wide so nothing beside them moves.
const STATE_BUTTON_W: f32 = 68.0;

fn small_button(k: &Kit, key: &str, name: &str, chord: Option<&str>, outlined: bool) -> El {
    let mut b = row()
        .key(key.to_owned())
        .role("button")
        .label(name.to_owned())
        .h(24.0)
        .px(8.0)
        .gap(6.0)
        .center()
        .radius(5.0)
        .child(
            text(name.to_owned())
                .font(k.ty.small)
                .color(if outlined { k.c.text_base } else { k.c.text_muted })
                .a11y_hidden(),
        );
    if outlined {
        b = b.border(1.0, k.c.border_base);
    }
    // The key lives in the tooltip, never on the face.
    if let Some(c) = chord {
        b = b.help(format!("{name}  {c}"));
    }
    k.interactive(b).focusable(true).press_scale(0.97)
}

type SubmitFn<S> = Rc<dyn Fn(&mut S, String, &mut Tx)>;
type CancelFn<S> = Rc<dyn Fn(&mut S, &mut Tx)>;
type ModeFn<S> = Rc<dyn Fn(&mut S, ComposerMode, &mut Tx)>;
type KindFn<S> = Rc<dyn Fn(&mut S, ThreadKind, &mut Tx)>;

/// A comment box: the anchor it comments on, the selected text, and a growing text area.
/// Enter adds a line, Ctrl+Enter submits the trimmed text (and clears the box), Esc cancels.
/// Unfocused, it leaves every key to the window (`j`/`k` move the diff); focused, it keeps them.
/// Given a [`ComposerMode`] it shows the Comment | Question tabs (Ctrl+1/2), the type picker and
/// blocking toggle in Comment mode, `+ suggestion`, `⤢` and Send.
pub struct Composer<S> {
    edit: TextEdit,
    access: EditAccess<S>,
    key: String,
    anchor_label: Option<String>,
    preview: Option<String>,
    placeholder: Option<String>,
    mode: Option<ComposerMode>,
    kind: ThreadKind,
    blocking: bool,
    rows: (usize, usize),
    on_submit: Option<SubmitFn<S>>,
    on_cancel: Option<CancelFn<S>>,
    on_mode: Option<ModeFn<S>>,
    on_kind: Option<KindFn<S>>,
    on_blocking: Option<CancelFn<S>>,
    on_suggest: Option<CancelFn<S>>,
    on_expand: Option<CancelFn<S>>,
    kind_open: bool,
    on_kind_menu: Option<CancelFn<S>>,
    look: Option<CardLook>,
    bare: bool,
}

impl<S: 'static> Composer<S> {
    pub fn new(edit: &TextEdit, access: impl for<'a> Fn(&'a mut S) -> &'a mut TextEdit + 'static) -> Self {
        Self::new_rc(edit, Rc::new(access))
    }

    fn new_rc(edit: &TextEdit, access: EditAccess<S>) -> Self {
        Composer {
            edit: edit.clone(),
            access,
            key: "composer".into(),
            anchor_label: None,
            preview: None,
            placeholder: None,
            mode: None,
            kind: ThreadKind::Comment,
            blocking: false,
            rows: (3, 12),
            on_submit: None,
            on_cancel: None,
            on_mode: None,
            on_kind: None,
            on_blocking: None,
            on_suggest: None,
            on_expand: None,
            kind_open: false,
            on_kind_menu: None,
            look: None,
            bare: false,
        }
    }

    /// For a host surface that already frames it and names the mode and the place: no box of its
    /// own and no title row (the anchor still labels the text area; Ctrl+1/2 still switch).
    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }

    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = key.into();
        self
    }

    /// The title row: the anchor (`VoucherService.cs:L27-L31`) or the artifact (`Brief › How 2`).
    pub fn anchor_label(mut self, l: impl Into<String>) -> Self {
        self.anchor_label = Some(l.into());
        self
    }

    /// The selected text a span comment is about.
    pub fn preview(mut self, p: impl Into<String>) -> Self {
        self.preview = Some(p.into()).filter(|p: &String| !p.is_empty());
        self
    }

    pub fn placeholder(mut self, p: impl Into<String>) -> Self {
        self.placeholder = Some(p.into());
        self
    }

    /// Shows the mode tabs with `m` selected (or Question while the text says `@claude`).
    pub fn mode(mut self, m: ComposerMode) -> Self {
        self.mode = Some(m);
        self
    }

    /// The type Comment mode sends, and whether an issue blocks.
    pub fn kind(mut self, kind: ThreadKind, blocking: bool) -> Self {
        self.kind = kind;
        self.blocking = blocking;
        self
    }

    /// The text area's height in rows, growing from `min` to `max`.
    pub fn rows(mut self, min: usize, max: usize) -> Self {
        self.rows = (min, max);
        self
    }

    pub fn on_submit(mut self, f: impl Fn(&mut S, String, &mut Tx) + 'static) -> Self {
        self.on_submit = Some(Rc::new(f));
        self
    }

    pub fn on_cancel(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.on_cancel = Some(Rc::new(f));
        self
    }

    pub fn on_mode(mut self, f: impl Fn(&mut S, ComposerMode, &mut Tx) + 'static) -> Self {
        self.on_mode = Some(Rc::new(f));
        self
    }

    pub fn on_kind(mut self, f: impl Fn(&mut S, ThreadKind, &mut Tx) + 'static) -> Self {
        self.on_kind = Some(Rc::new(f));
        self
    }

    pub fn on_blocking(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.on_blocking = Some(Rc::new(f));
        self
    }

    /// `+ suggestion`: the app inserts a suggestion block prefilled with the selection.
    pub fn on_suggest(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.on_suggest = Some(Rc::new(f));
        self
    }

    /// The kind dropdown: whether its list is open, and what its button does (open or close it).
    pub fn kind_menu(mut self, open: bool, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.kind_open = open;
        self.on_kind_menu = Some(Rc::new(f));
        self
    }

    /// The card's fill, ring and shadow.
    pub fn look(mut self, l: CardLook) -> Self {
        self.look = Some(l);
        self
    }

    /// `⤢`: the app opens (or closes) the long editor.
    pub fn on_expand(mut self, f: impl Fn(&mut S, &mut Tx) + 'static) -> Self {
        self.on_expand = Some(Rc::new(f));
        self
    }

    pub fn build(self, k: &Kit) -> El {
        let shown_mode = self.mode.map(|m| effective_mode(m, &self.edit.text));
        let access = self.access.clone();
        let key_access = self.access.clone();
        let send = {
            let access = self.access.clone();
            let submit = self.on_submit.clone();
            Rc::new(move |s: &mut S, tx: &mut Tx| {
                let edit = access(s);
                let body = edit.text.trim().to_owned();
                if body.is_empty() {
                    return;
                }
                edit.clear();
                if let Some(f) = &submit {
                    f(s, body, tx);
                }
            })
        };
        let key_send = send.clone();
        let cancel = self.on_cancel.clone();
        let mode_fn = self.on_mode.clone();
        let has_mode = self.mode.is_some();
        let placeholder = self.placeholder.clone().unwrap_or_else(|| match shown_mode {
            Some(ComposerMode::Question) => "Ask a question… Claude answers here (Ctrl+Enter to send, Esc keeps a draft)".to_owned(),
            Some(ComposerMode::Comment) => "Comment… @claude asks Claude (Ctrl+Enter to send, Esc keeps a draft)".to_owned(),
            None => "Comment… (Ctrl+Enter to send, Esc to cancel)".to_owned(),
        });
        let area = TextArea::new(&self.edit)
            .label(
                self.anchor_label
                    .as_ref()
                    .map_or_else(|| "Comment".to_owned(), |l| format!("Comment on {l}")),
            )
            .placeholder(placeholder)
            .width(None)
            .auto_grow(self.rows.0, self.rows.1)
            .on_edit(move |s: &mut S| access(s))
            .on_key(move |s: &mut S, ev: &KeyEvent, tx: &mut Tx| {
                let chord = ev.mods.ctrl || ev.mods.cmd;
                match &ev.key {
                    Key::Enter if chord => {
                        key_send(s, tx);
                        true
                    }
                    Key::Enter if !ev.mods.shift && !ev.mods.alt => {
                        let _ = key_access(s).paste("\n");
                        true
                    }
                    Key::Text(t) if chord && has_mode && (t == "1" || t == "2") => {
                        if let Some(f) = &mode_fn {
                            let m = if t == "1" { ComposerMode::Comment } else { ComposerMode::Question };
                            f(s, m, tx);
                        }
                        true
                    }
                    Key::Escape => match &cancel {
                        Some(f) => {
                            f(s, tx);
                            true
                        }
                        None => false,
                    },
                    _ => false,
                }
            })
            .build(k)
            .key("field");
        let look = self.look.unwrap_or_else(|| CardLook::from_kit(k));
        let mut e = col().key(self.key).role("group").label("Comment composer").w_full().gap(8.0);
        if !self.bare {
            e = e
                .pl(12.0)
                .pr(12.0)
                .pt(10.0)
                .pb(10.0)
                .radius(8.0)
                .fill(look.fill)
                .border(1.0, look.border);
            if look.shadow.a > 0.0 {
                e = e.shadow(6.0, 16.0, look.shadow);
            }
        }
        // Comment | Question · where · the kind (and Blocking for an issue) · ⤢
        let mut top = row().key("title").w_full().gap(10.0).center_y();
        let mut has_top = false;
        if let Some(m) = shown_mode {
            top = top.child(mode_switch(k, m, self.on_mode.clone()));
            has_top = true;
        }
        if let Some(l) = self.anchor_label.clone().filter(|_| !self.bare) {
            top = top.child(text(l).key("anchor").font(k.ty.small).color(k.c.text_muted).truncate().shrink());
            has_top = true;
        }
        match shown_mode {
            Some(ComposerMode::Comment) => {
                top = top.child(kind_menu(
                    k,
                    self.kind,
                    self.blocking,
                    self.kind_open,
                    self.on_kind.clone(),
                    self.on_kind_menu.clone(),
                ));
                if self.kind == ThreadKind::Issue {
                    let mut sw = row()
                        .key("blocking")
                        .role("checkbox")
                        .label("Blocking")
                        .checked(self.blocking)
                        .gap(6.0)
                        .center_y()
                        .focusable(true)
                        .child({
                            let track = row()
                                .w(26.0)
                                .h(14.0)
                                .radius(7.0)
                                .px(2.0)
                                .center_y()
                                .fill(if self.blocking { k.c.danger.icon } else { k.c.layer_03 })
                                .a11y_hidden()
                                .child(rune::el().size(10.0, 10.0).radius(5.0).fill(k.c.text_base));
                            if self.blocking { track.justify_end() } else { track }
                        })
                        .child(text("Blocking").font(k.ty.small).color(k.c.text_base).a11y_hidden());
                    if let Some(f) = self.on_blocking.clone() {
                        sw = sw.on_press_tx(move |s: &mut S, tx: &mut Tx| f(s, tx));
                    }
                    top = top.child(sw);
                }
            }
            Some(ComposerMode::Question) => {
                top = top.child(
                    text("Claude answers here, as a reply")
                        .key("question-hint")
                        .font(k.ty.small)
                        .color(k.c.info.fg),
                );
            }
            None => {}
        }
        top = top.child(rune::el().grow(1.0));
        if let Some(f) = self.on_expand.clone() {
            top = top.child(
                k.interactive(
                    row()
                        .key("expand-btn")
                        .role("button")
                        .label("Long editor")
                        .help("Long editor with a preview")
                        .size(24.0, 24.0)
                        .center()
                        .radius(6.0)
                        .focusable(true)
                        .child(rune::icon(&EXPAND, 14.0).color(k.c.text_muted)),
                )
                .on_press_tx(move |s: &mut S, tx: &mut Tx| f(s, tx)),
            );
            has_top = true;
        }
        if has_top {
            e = e.child(top);
        }
        if let Some(p) = self.preview {
            e = e.child(
                row()
                    .key("preview")
                    .w_full()
                    .items_stretch()
                    .gap(8.0)
                    .padding(6.0)
                    .radius(6.0)
                    .fill(k.c.layer_02)
                    .child(rail(k.c.border_focus))
                    .child(
                        text(p)
                            .key("text")
                            .font(k.ty.mono)
                            .color(k.c.text_base)
                            .max_lines(4)
                            .grow(1.0)
                            .shrink(),
                    ),
            );
        }
        e = e.child(area);
        if self.mode.is_some() {
            let mut foot = row().key("footer").w_full().gap(8.0).center_y();
            if let Some(f) = self.on_suggest.clone() {
                foot = foot
                    .child(small_button(k, "suggest-btn", "+ suggestion", None, false).on_press_tx(move |s: &mut S, tx: &mut Tx| f(s, tx)));
            }
            if shown_mode == Some(ComposerMode::Comment) {
                foot = foot.child(text("Type @claude to send it as a question").font(k.ty.small).color(k.c.text_faint));
            }
            foot = foot.child(rune::el().grow(1.0));
            if let Some(f) = self.on_cancel.clone() {
                foot = foot.child(small_button(k, "cancel-btn", "Cancel", None, false).on_press_tx(move |s: &mut S, tx: &mut Tx| f(s, tx)));
            }
            let label = if shown_mode == Some(ComposerMode::Question) {
                "Ask"
            } else {
                "Send"
            };
            foot = foot.child(
                k.interactive_with(
                    row()
                        .key("send-btn")
                        .role("button")
                        .label(label)
                        .h(24.0)
                        .px(8.0)
                        .gap(6.0)
                        .center()
                        .radius(5.0)
                        .fill(k.c.bg_accent)
                        .focusable(true)
                        .help(format!("{label}  Ctrl+Enter"))
                        .child(text(label).font(STRONG_SMALL).color(k.c.submit_fg).a11y_hidden()),
                    k.c.contrast_hover,
                    k.c.contrast_pressed,
                )
                .press_scale(0.97)
                .on_press_tx(move |s: &mut S, tx: &mut Tx| send(s, tx)),
            );
            e = e.child(foot);
        }
        e
    }
}

static EXPAND: rune::Icon = rune::Icon {
    name: "rain-expand",
    view: 16.0,
    body: r#"<path d="M9.5 2.5h4v4M13.5 2.5 9 7M6.5 13.5h-4v-4M2.5 13.5 7 9" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>"#,
};

/// `Comment Ctrl+1 | Question Ctrl+2` on the well.
fn mode_switch<S: 'static>(k: &Kit, m: ComposerMode, on_mode: Option<ModeFn<S>>) -> El {
    let mut seg = row()
        .key("mode-tabs")
        .role("tablist")
        .label("Mode")
        .padding(2.0)
        .radius(6.0)
        .fill(k.c.page)
        .border(1.0, k.c.border_weak);
    for (mode, key, name, chord) in [
        (ComposerMode::Comment, "mode-comment", "Comment", "Ctrl+1"),
        (ComposerMode::Question, "mode-question", "Question", "Ctrl+2"),
    ] {
        let on = mode == m;
        let mut t = row()
            .key(key)
            .role("tab")
            .label(name)
            .checked(on)
            .h(22.0)
            .px(8.0)
            .gap(6.0)
            .center_y()
            .radius(4.0)
            .help(format!("{name}  {chord}"))
            .child(
                text(name)
                    .font(k.ty.small)
                    .color(if on { k.c.text_base } else { k.c.text_muted })
                    .a11y_hidden(),
            );
        t = if on {
            t.fill(k.c.layer_01).border(1.0, k.c.border_base)
        } else {
            k.interactive(t)
        };
        if let Some(f) = on_mode.clone() {
            t = t.on_press_tx(move |s: &mut S, tx: &mut Tx| f(s, mode, tx));
        }
        seg = seg.child(t);
    }
    seg
}

/// The one kind dropdown: `Issue ▾`, its list under it while open.
fn kind_menu<S: 'static>(
    k: &Kit,
    kind: ThreadKind,
    blocking: bool,
    open: bool,
    on_kind: Option<KindFn<S>>,
    on_menu: Option<CancelFn<S>>,
) -> El {
    let ink = kind_tone(k, kind, blocking);
    let mut b = k.interactive(
        row()
            .key("kind-menu")
            .role("button")
            .label(format!("Kind: {}", kind.label()))
            .expanded(open)
            .h(22.0)
            .px(8.0)
            .gap(4.0)
            .center_y()
            .radius(4.0)
            .border(1.0, k.c.border_base)
            .focusable(true)
            .child(text(capitalised(kind.label())).font(k.ty.small).color(ink).a11y_hidden())
            .child(text("▾").font(k.ty.small).color(k.c.text_muted).a11y_hidden()),
    );
    if let Some(f) = on_menu.clone() {
        b = b.on_press_tx(move |s: &mut S, tx: &mut Tx| f(s, tx));
    }
    if !open {
        return b;
    }
    let mut list = col()
        .key("kind-list")
        .role("menu")
        .w(160.0)
        .padding(4.0)
        .radius(8.0)
        .fill(k.c.layer_02)
        .border(1.0, k.c.border_base)
        .shadow(12.0, 32.0, Color::BLACK.alpha(0.45));
    for kd in COMMENT_KINDS {
        let mut item = k.interactive(
            row()
                .key(format!("kind-{}", kd.label()))
                .role("menuitemradio")
                .label(kd.label())
                .checked(kd == kind)
                .w_full()
                .h(26.0)
                .px(8.0)
                .gap(8.0)
                .center_y()
                .radius(5.0)
                .child(
                    text(if kd == kind { "✓" } else { " " })
                        .font(k.ty.small)
                        .color(k.c.bg_accent)
                        .w(12.0)
                        .a11y_hidden(),
                )
                .child(
                    text(capitalised(kd.label()))
                        .font(k.ty.ui)
                        .color(kind_tone(k, kd, false))
                        .a11y_hidden(),
                ),
        );
        if let Some(f) = on_kind.clone() {
            item = item.on_press_tx(move |s: &mut S, tx: &mut Tx| f(s, kd, tx));
        }
        list = list.child(item);
    }
    b.child(list.absolute(0.0, 26.0).raise())
}

fn capitalised(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().chain(c).collect())
}
