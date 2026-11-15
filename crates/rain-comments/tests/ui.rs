use rain_comments::ui::{answer_text, display_body, effective_mode, secs_between, suggestion_block};
use rain_comments::{CardAction, Composer, ComposerMode, ThreadCard, anchor_label, ops};
use rain_core::{
    Anchor, AnchorState, Answer, AnswerStatus, CodeSnapshot, DismissReason, Disposition, Orphan, OrphanReason, Session, Side, ThreadKind,
    ThreadState,
};
use rune::{Cx, El, Env, Key, KeyEvent, Mods, Runtime, TextEdit, Tx, col};
use rune_kit::Kit;

struct App {
    s: Session,
    edit: TextEdit,
    reply: TextEdit,
    collapsed: bool,
    replying: bool,
    focused: bool,
    dismissing: bool,
    cancelled: usize,
    window_keys: Vec<String>,
    mode: ComposerMode,
    kind: ThreadKind,
    kind_open: bool,
    blocking: bool,
}

const ANCHOR: Anchor = Anchor::Range {
    side: Side::New,
    start: 12,
    end: 14,
};

fn act(a: &mut App, id: &str, action: CardAction, _: &mut Tx) {
    match action {
        CardAction::Toggle => a.collapsed = !a.collapsed,
        CardAction::OpenReply | CardAction::AskReply => a.replying = true,
        CardAction::CancelReply => a.replying = false,
        CardAction::Reply(body) => {
            ops::reply(&mut a.s, id, &body, None);
            a.replying = false;
        }
        CardAction::Disposition(d) => {
            ops::toggle_disposition(&mut a.s, id, d);
        }
        CardAction::OpenDismiss => a.dismissing = true,
        CardAction::Dismiss(r) => {
            ops::set_disposition(&mut a.s, id, Some(Disposition::Dismiss));
            ops::dismiss(&mut a.s, id, r);
            a.dismissing = false;
        }
        CardAction::Confirm => {
            ops::confirm_addressed(&mut a.s, id);
        }
        CardAction::Reopen => {
            ops::reopen(&mut a.s, id);
        }
        CardAction::Resolve | CardAction::CloseOrphan => {
            ops::resolve(&mut a.s, id);
        }
        CardAction::EditMessage(_)
        | CardAction::Delete
        | CardAction::Retry
        | CardAction::CancelAnswer
        | CardAction::Attach
        | CardAction::Hover(_) => {}
    }
}

fn view(a: &App, _: &mut Cx<App>) -> El {
    let k = Kit::neutral(true);
    let mut root = col().w_full().h_full().gap(12.0).padding(12.0).child(
        Composer::new(&a.edit, |a: &mut App| &mut a.edit)
            .anchor_label(anchor_label("server/src/X.cs", &ANCHOR))
            .on_submit(|a: &mut App, body: String, _: &mut Tx| {
                ops::add_thread(&mut a.s, "server", "src/X.cs", ANCHOR, &body, None, None);
            })
            .on_cancel(|a: &mut App, _: &mut Tx| a.cancelled += 1)
            .mode(a.mode)
            .kind(a.kind, a.blocking)
            .on_mode(|a: &mut App, m, _: &mut Tx| a.mode = m)
            .on_kind(|a: &mut App, kd, _: &mut Tx| {
                a.kind = kd;
                a.kind_open = false;
            })
            .kind_menu(a.kind_open, |a: &mut App, _: &mut Tx| a.kind_open = !a.kind_open)
            .on_blocking(|a: &mut App, _: &mut Tx| a.blocking = !a.blocking)
            .build(&k),
    );
    for t in &a.s.threads {
        let mut card = ThreadCard::new(t)
            .collapsed(a.collapsed)
            .focused(a.focused)
            .dismissing(a.dismissing)
            .now("2026-01-01T00:00:12.000Z")
            .on_action(act);
        if a.replying {
            card = card.reply_composer(&a.reply, |a: &mut App| &mut a.reply);
        }
        root = root.child(card.build(&k));
    }
    root
}

fn window_key(a: &mut App, ev: &KeyEvent, _: &mut Tx) -> bool {
    a.window_keys.push(match &ev.key {
        Key::Text(t) => t.clone(),
        other => format!("{other:?}"),
    });
    true
}

fn runtime(threads: usize) -> Runtime<App> {
    let mut s = Session::default();
    for i in 0..threads {
        ops::add_thread(
            &mut s,
            "server",
            "src/X.cs",
            ANCHOR,
            &format!("F{i} issue (blocking): broken. Details."),
            Some("claude"),
            None,
        );
    }
    let app = App {
        s,
        edit: TextEdit::default(),
        reply: TextEdit::default(),
        collapsed: true,
        replying: false,
        focused: true,
        dismissing: false,
        cancelled: 0,
        window_keys: Vec::new(),
        mode: ComposerMode::Comment,
        kind: ThreadKind::Comment,
        kind_open: false,
        blocking: false,
    };
    let mut rt = Runtime::new(
        app,
        view,
        (900.0, 700.0),
        Env {
            system_dark: true,
            scale: 1.0,
        },
    );
    rt.on_key = Some(window_key);
    settle(&mut rt);
    rt
}

fn settle(rt: &mut Runtime<App>) {
    rt.frame(0.0);
    for _ in 0..20 {
        rt.frame(1.0 / 60.0);
    }
}

fn press(rt: &mut Runtime<App>, key: Key, mods: Mods) {
    rt.key(KeyEvent { key, mods });
    rt.frame(1.0 / 60.0);
}

fn text(rt: &mut Runtime<App>, s: &str) {
    press(rt, Key::Text(s.into()), Mods::default());
}

fn click(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

#[test]
fn unfocused_composer_leaves_keys_to_the_window_and_focused_one_keeps_them() {
    let mut rt = runtime(0);
    text(&mut rt, "j");
    text(&mut rt, "k");
    assert_eq!(
        rt.state.window_keys,
        ["j", "k"],
        "j/k reach the diff while the composer is not focused"
    );

    click(&mut rt, "field");
    assert!(rt.state.edit.focused, "a click focuses the composer");
    for ch in ["j", "k", "]", "n"] {
        text(&mut rt, ch);
    }
    assert_eq!(rt.state.edit.text, "jk]n");
    assert_eq!(rt.state.window_keys, ["j", "k"], "typed keys stay in the focused composer");

    press(&mut rt, Key::Enter, Mods::default());
    text(&mut rt, "x");
    assert_eq!(rt.state.edit.text, "jk]n\nx", "plain Enter adds a line");

    press(
        &mut rt,
        Key::Enter,
        Mods {
            ctrl: true,
            cmd: true,
            ..Mods::default()
        },
    );
    assert_eq!(rt.state.s.threads.len(), 1, "Ctrl+Enter submits");
    assert_eq!(rt.state.s.threads[0].messages[0].body, "jk]n\nx");
    assert_eq!(rt.state.s.version, 1);
    assert!(rt.state.edit.text.is_empty(), "the box clears after sending");

    press(
        &mut rt,
        Key::Enter,
        Mods {
            ctrl: true,
            cmd: true,
            ..Mods::default()
        },
    );
    assert_eq!(rt.state.s.threads.len(), 1, "an empty box sends nothing");

    press(&mut rt, Key::Escape, Mods::default());
    assert_eq!(rt.state.cancelled, 1, "Esc cancels the focused composer");
    assert_eq!(rt.state.window_keys, ["j", "k"], "and does not reach the window");
}

#[test]
fn focused_card_takes_one_key_dispositions_and_reply() {
    let mut rt = runtime(1);
    let id = rt.state.s.threads[0].id.clone();
    let card = format!("thread-{id}");
    assert!(rt.find(&card).is_some());

    click(&mut rt, "disp-agree");
    assert_eq!(
        rt.state.s.threads[0].disposition,
        Some(Disposition::Agree),
        "pressing the chip agrees"
    );
    click(&mut rt, "disp-agree");
    assert_eq!(rt.state.s.threads[0].disposition, None, "pressing it again clears it");

    let focused = rt
        .elements()
        .into_iter()
        .any(|e| e.focused && (e.key.as_deref() == Some(card.as_str()) || e.path.contains(&card)));
    assert!(focused, "the press left focus inside the card");
    text(&mut rt, "x");
    settle(&mut rt);
    assert!(rt.state.dismissing, "x asks for a reason instead of dismissing at once");
    assert_eq!(rt.state.s.threads[0].disposition, None);
    for (k, want) in [
        ("?", Some(Disposition::Ask)),
        ("d", Some(Disposition::Defer)),
        ("a", Some(Disposition::Agree)),
        ("a", None),
    ] {
        text(&mut rt, k);
        assert_eq!(rt.state.s.threads[0].disposition, want, "key {k}");
    }
    assert!(rt.state.window_keys.is_empty(), "the card consumed its keys");

    text(&mut rt, " ");
    settle(&mut rt);
    assert!(rt.state.collapsed, "Space on a focused chip presses the chip, not the card");
    assert_eq!(rt.state.s.threads[0].disposition, Some(Disposition::Agree));
    text(&mut rt, "a");

    click(&mut rt, "head");
    assert!(!rt.state.collapsed, "pressing the header expands");
    press(&mut rt, Key::Enter, Mods::default());
    settle(&mut rt);
    assert!(rt.state.collapsed, "Enter on the header collapses");
    press(&mut rt, Key::Enter, Mods::default());
    settle(&mut rt);
    assert!(!rt.state.collapsed, "and expands again");
    text(&mut rt, "r");
    settle(&mut rt);
    assert!(rt.state.replying, "r opens the reply composer");
    click(&mut rt, "reply");
    assert!(rt.state.reply.focused);
    for ch in ["a", "x", "d"] {
        text(&mut rt, ch);
    }
    assert_eq!(rt.state.s.threads[0].disposition, None, "keys typed into the reply box are text");
    press(
        &mut rt,
        Key::Enter,
        Mods {
            ctrl: true,
            cmd: true,
            ..Mods::default()
        },
    );
    settle(&mut rt);
    assert_eq!(rt.state.s.threads[0].messages.len(), 2);
    assert_eq!(rt.state.s.threads[0].messages[1].body, "axd");
    assert!(!rt.state.replying);
}

#[test]
fn collapsed_card_is_one_line_and_expanded_shows_every_message() {
    let mut rt = runtime(1);
    let id = rt.state.s.threads[0].id.clone();
    ops::reply(&mut rt.state.s, &id, "Agreed.", Some("sumit"));
    ops::reply(&mut rt.state.s, &id, "```suggestion\nfixed();\n```", Some("claude"));
    ops::reply(&mut rt.state.s, &id, "Thanks", None);
    rt.invalidate();
    settle(&mut rt);
    let els = rt.elements();
    let summary = els.iter().find(|e| e.key.as_deref() == Some("summary")).unwrap();
    assert_eq!(summary.text.as_deref(), Some("broken."));
    assert!(
        els.iter()
            .any(|e| e.key.as_deref() == Some("replies") && e.text.as_deref() == Some("3↩"))
    );
    assert!(
        !els.iter().any(|e| e.key.as_deref().is_some_and(|k| k.starts_with("m-"))),
        "collapsed shows no messages"
    );

    rt.state.collapsed = false;
    rt.invalidate();
    settle(&mut rt);
    let messages = rt
        .elements()
        .into_iter()
        .filter(|e| e.key.as_deref().is_some_and(|k| k.starts_with("m-")) && !e.exiting)
        .count();
    assert_eq!(messages, 4);
    for name in ["Agree", "Dismiss", "Ask", "Defer", "Reply", "Resolve"] {
        assert!(
            rt.elements().iter().any(|e| e.role == "button" && e.label.as_deref() == Some(name)),
            "button {name}"
        );
    }
    let edits: Vec<String> = rt
        .elements()
        .into_iter()
        .filter(|e| !e.exiting && e.key.as_deref().is_some_and(|k| k.starts_with("edit-")))
        .filter_map(|e| e.key)
        .collect();
    let mine = rt.state.s.threads[0].messages[3].id.clone();
    assert_eq!(edits, [format!("edit-{mine}")], "Edit only on the user's own message");
    assert!(rt.find("delete-btn").is_none(), "never Delete on the agent's thread");
}

fn live(rt: &Runtime<App>, key: &str) -> bool {
    rt.elements().iter().any(|e| !e.exiting && e.key.as_deref() == Some(key))
}

fn text_of(rt: &Runtime<App>, key: &str) -> Option<String> {
    let els = rt.elements();
    let at = els.iter().position(|e| !e.exiting && e.key.as_deref() == Some(key))?;
    let path = els[at].path.clone();
    els.iter()
        .skip(at)
        .filter(|e| e.path.starts_with(&path))
        .find_map(|e| e.text.clone())
}

#[test]
fn actions_show_only_on_the_focused_or_hovered_card() {
    let mut rt = runtime(1);
    assert!(live(&rt, "disp-agree"));
    rt.state.focused = false;
    rt.invalidate();
    settle(&mut rt);
    assert!(!live(&rt, "disp-agree"), "an idle card shows no actions");
    assert!(!live(&rt, "dispositions"));
}

#[test]
fn dismiss_asks_for_a_reason_and_stores_it() {
    let mut rt = runtime(1);
    click(&mut rt, "disp-dismiss");
    assert!(rt.state.dismissing);
    assert_eq!(rt.state.s.threads[0].state, ThreadState::Open, "no dismissal without a reason");
    assert!(live(&rt, "dismiss-reasons"));
    click(&mut rt, "dismiss-out-of-scope");
    let t = &rt.state.s.threads[0];
    assert_eq!(t.state, ThreadState::Dismissed);
    assert_eq!(t.dismiss_reason, Some(DismissReason::OutOfScope));
    assert_eq!(t.disposition, Some(Disposition::Dismiss));
    assert!(!rt.state.dismissing);
}

#[test]
fn orphan_card_keeps_the_last_seen_code_and_its_sha() {
    let mut rt = runtime(1);
    {
        let t = &mut rt.state.s.threads[0];
        let snap = CodeSnapshot {
            content: "var total = calc.Sum(payments);".into(),
            path: Some("server/src/AccountService.cs".into()),
            lines: Some((50, 50)),
            side: Some(Side::New),
            ..CodeSnapshot::default()
        };
        t.code_snapshot = Some(snap.clone());
        t.set_anchor_state(AnchorState::Orphaned, None);
        t.orphan = Some(Orphan {
            reason: OrphanReason::ContentRewritten,
            last_seen_sha: Some("a1b2c3d4e5f6".into()),
            snapshot: snap,
            at: "2026-01-01T00:00:00.000Z".into(),
        });
    }
    rt.state.focused = false;
    rt.invalidate();
    settle(&mut rt);
    assert_eq!(text_of(&rt, "anchor-state").as_deref(), Some("orphaned · content rewritten"));
    let where_ = text_of(&rt, "orphan-where").unwrap();
    assert!(where_.contains("a1b2c3d") && where_.contains("AccountService.cs:50"), "{where_}");
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref().is_some_and(|t| t.contains("var total = calc.Sum(payments);"))),
        "the snapshot text is drawn"
    );
    assert!(live(&rt, "attach-btn") && live(&rt, "close-orphan-btn"));
    click(&mut rt, "close-orphan-btn");
    let t = &rt.state.s.threads[0];
    assert_eq!(t.state, ThreadState::Resolved);
    assert!(t.orphan.is_some(), "closing keeps the snapshot");
}

#[test]
fn addressed_threads_offer_confirm_and_a_confirms() {
    let mut rt = runtime(1);
    let id = rt.state.s.threads[0].id.clone();
    ops::set_state(&mut rt.state.s, &id, ThreadState::Addressed, Some("d4e5f6a0"), None);
    rt.invalidate();
    settle(&mut rt);
    assert_eq!(text_of(&rt, "state").as_deref(), Some("addressed in d4e5f6a"));
    assert!(live(&rt, "confirm-btn"));
    assert!(!live(&rt, "disp-agree"));
    click(&mut rt, "confirm-btn");
    let t = &rt.state.s.threads[0];
    assert_eq!(t.disposition, Some(Disposition::Agree), "Confirm agrees");
    assert_eq!(t.state, ThreadState::Addressed);
    assert!(!rain_comments::ui::confirmable(t));
}

#[test]
fn question_threads_show_their_answer_state() {
    let mut rt = runtime(1);
    {
        let t = &mut rt.state.s.threads[0];
        t.kind = Some(ThreadKind::Question);
        t.answer = Some(Answer::default());
    }
    rt.invalidate();
    settle(&mut rt);
    assert_eq!(text_of(&rt, "answer-text").as_deref(), Some("Sent · waiting for an answer"));
    assert!(live(&rt, "cancel-answer"));
    {
        let a = rt.state.s.threads[0].answer.as_mut().unwrap();
        a.status = AnswerStatus::Answering;
        a.started_at = Some("2026-01-01T00:00:00.000Z".into());
    }
    rt.invalidate();
    settle(&mut rt);
    assert_eq!(text_of(&rt, "answer-text").as_deref(), Some("Claude is answering… 12 s"));
    rt.state.s.threads[0].answer.as_mut().unwrap().status = AnswerStatus::Timeout;
    rt.invalidate();
    settle(&mut rt);
    assert_eq!(text_of(&rt, "answer-text").as_deref(), Some("timed out"));
    assert!(live(&rt, "retry-answer"));
}

fn ctrl_digit(rt: &mut Runtime<App>, d: &str) {
    press(
        rt,
        Key::Text(d.into()),
        Mods {
            ctrl: true,
            cmd: true,
            ..Mods::default()
        },
    );
    settle(rt);
}

#[test]
fn composer_modes_types_and_ctrl_digits() {
    let mut rt = runtime(0);
    assert!(live(&rt, "mode-tabs"));
    assert!(live(&rt, "kind-menu") && !live(&rt, "kind-issue"), "one dropdown for the kind");
    click(&mut rt, "kind-menu");
    assert!(live(&rt, "kind-comment") && live(&rt, "kind-issue"));
    click(&mut rt, "kind-issue");
    assert!(!live(&rt, "kind-list"), "a pick closes the list");
    assert_eq!(rt.state.kind, ThreadKind::Issue);
    assert!(live(&rt, "blocking"), "issues offer blocking");
    click(&mut rt, "field");
    ctrl_digit(&mut rt, "2");
    assert_eq!(rt.state.mode, ComposerMode::Question);
    assert!(!live(&rt, "kind-menu"), "Question mode has no type picker");
    assert!(live(&rt, "question-hint"));
    ctrl_digit(&mut rt, "1");
    assert_eq!(rt.state.mode, ComposerMode::Comment);
    assert!(rt.state.edit.text.is_empty(), "Ctrl+digits switch modes, not type");
    for ch in "@claude why".chars() {
        text(&mut rt, &ch.to_string());
    }
    settle(&mut rt);
    assert!(live(&rt, "question-hint"), "@claude reads as a question");
    assert_eq!(effective_mode(ComposerMode::Comment, "hey @Claude"), ComposerMode::Question);
    assert_eq!(effective_mode(ComposerMode::Comment, "hey"), ComposerMode::Comment);
}

#[test]
fn suggestion_blocks_read_back_as_diff_fences() {
    let block = suggestion_block("return total + vouchers;\n");
    assert_eq!(block, "```suggestion\nreturn total + vouchers;\n```");
    let snap = CodeSnapshot {
        content: "return total;".into(),
        ..CodeSnapshot::default()
    };
    let shown = display_body(&format!("Add vouchers:\n{block}"), Some(&snap));
    assert_eq!(shown, "Add vouchers:\n```diff\n-return total;\n+return total + vouchers;\n```");
}

#[test]
fn elapsed_seconds_between_iso_times() {
    assert_eq!(secs_between("2026-01-01T00:00:00.000Z", "2026-01-01T00:01:05.900Z"), Some(65));
    assert_eq!(secs_between("2026-02-28T23:59:59.000Z", "2026-03-01T00:00:01.000Z"), Some(2));
    assert_eq!(secs_between("bad", "2026-03-01T00:00:01.000Z"), None);
    let mut s = Session::default();
    let id = ops::add_thread_with(
        &mut s,
        "",
        "a.cs",
        ANCHOR,
        "why?",
        None,
        None,
        ops::ThreadExtra {
            kind: Some(ThreadKind::Question),
            ..ops::ThreadExtra::default()
        },
    );
    let t = ops::thread(&s, &id).unwrap();
    assert_eq!(answer_text(t, None).as_deref(), Some("Sent · waiting for an answer"));
}
