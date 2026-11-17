use std::sync::Arc;

use rain_comments::ComposerMode;
use rain_core::{AnchorState, AnswerStatus, DismissReason, EventKind, Launch, Shared, Side, Target, TargetKind, ThreadKind, ThreadState};
use rain_diffview::LineRef;
use rune::{Key, Mods, Runtime};

use super::*;

fn live(rt: &Runtime<App>, key: &str) -> bool {
    rt.elements().iter().any(|e| !e.exiting && e.key.as_deref() == Some(key))
}

fn click(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// The story fixture with the cursor on an added line of the first file that has no thread.
fn on_a_line() -> Runtime<App> {
    let mut rt = runtime(app(story(), true));
    let n = rt.state.diff.files()[0].lines.iter().filter(|l| l.new != 0).nth(2).unwrap().new;
    rt.update(move |s, _| s.active_mut().set_cursor(Some((0, LineRef::new(Side::New, n)))));
    settle(&mut rt);
    rt
}

fn asks(rt: &Runtime<App>) -> usize {
    rt.state.shared.events_since(0, &[EventKind::Ask]).len()
}

#[test]
fn question_mark_opens_question_mode_without_a_type_picker() {
    let mut rt = on_a_line();
    typed(&mut rt, "?");
    assert!(rt.state.composer.is_some());
    assert_eq!(rt.state.composer_ui.mode, ComposerMode::Question);
    assert!(live(&rt, "question-hint"));
    assert!(!live(&rt, "kind-menu"), "no type picker in Question mode");
    assert!(live(&rt, "mode-tabs"));
}

#[test]
fn c_opens_comment_mode_defaulting_to_comment() {
    let mut rt = on_a_line();
    typed(&mut rt, "c");
    assert_eq!(rt.state.composer_ui.mode, ComposerMode::Comment);
    assert_eq!(rt.state.composer_ui.kind, ThreadKind::Comment);
    assert!(live(&rt, "kind-menu"), "the type picker");
    click(&mut rt, "kind-menu");
    click(&mut rt, "kind-issue");
    click(&mut rt, "blocking");
    rt.update(|s, _| s.composer_edit.focused = true);
    typed(&mut rt, "Unchecked amount");
    press(&mut rt, Key::Enter, ctrl());
    let s = rt.state.shared.lock();
    let t = s.threads.last().unwrap();
    assert_eq!(t.kind, Some(ThreadKind::Issue));
    assert_eq!(t.blocking, Some(true));
    assert!(t.answer.is_none(), "a comment expects no answer");
    assert!(
        t.code_snapshot.as_ref().is_some_and(|c| c.lines.is_some()),
        "snapshot taken at post time"
    );
    drop(s);
    assert_eq!(asks(&rt), 0);
}

#[test]
fn sending_a_question_queues_an_answer_and_one_ask_event() {
    let mut rt = on_a_line();
    typed(&mut rt, "?");
    typed(&mut rt, "Why here?");
    press(&mut rt, Key::Enter, ctrl());
    let t = rt.state.shared.lock().threads.last().cloned().unwrap();
    assert_eq!(t.kind, Some(ThreadKind::Question));
    assert_eq!(t.answer.as_ref().map(|a| a.status), Some(AnswerStatus::Queued));
    let events = rt.state.shared.events_since(0, &[EventKind::Ask]);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].thread.as_deref(), Some(t.id.as_str()));
    assert_eq!(events[0].data["question"], "Why here?");
    assert!(live(&rt, "answer-text"), "the card shows the answer chip");
    assert!(rt.state.status.contains("waiting for an answer"), "{}", rt.state.status);
}

#[test]
fn at_claude_in_a_comment_makes_it_a_question() {
    let mut rt = on_a_line();
    typed(&mut rt, "c");
    typed(&mut rt, "@claude is this safe?");
    assert!(live(&rt, "question-hint"), "the composer reads as Question");
    press(&mut rt, Key::Enter, ctrl());
    let t = rt.state.shared.lock().threads.last().cloned().unwrap();
    assert_eq!(t.kind, Some(ThreadKind::Question));
    assert_eq!(asks(&rt), 1);
}

#[test]
fn ctrl_digits_switch_the_mode() {
    let mut rt = on_a_line();
    typed(&mut rt, "c");
    press(&mut rt, Key::Text("2".into()), ctrl());
    assert_eq!(rt.state.composer_ui.mode, ComposerMode::Question);
    press(&mut rt, Key::Text("1".into()), ctrl());
    assert_eq!(rt.state.composer_ui.mode, ComposerMode::Comment);
    assert!(rt.state.composer_edit.text.is_empty());
}

#[test]
fn drafts_survive_a_restart_with_their_mode_and_a_mark() {
    let dir = temp_dir();
    let launch = Launch {
        session: "drafts".into(),
        fixture: Some(story()),
        overview: Some(overview()),
        ..Launch::default()
    };
    let n = {
        let a = App::new(launch.clone(), Arc::new(Shared::default()), Some(dir.clone()), None);
        let mut rt = runtime({
            let mut a = a;
            a.load_now();
            a
        });
        let n = rt.state.diff.files()[0].lines.iter().filter(|l| l.new != 0).nth(2).unwrap().new;
        rt.update(move |s, _| s.active_mut().set_cursor(Some((0, LineRef::new(Side::New, n)))));
        settle(&mut rt);
        typed(&mut rt, "?");
        typed(&mut rt, "half a question");
        press(&mut rt, Key::Escape, Mods::default());
        assert!(rt.state.composer.is_none());
        assert!(live(&rt, "draft"), "a draft keeps a mark on its line");
        rt.state.autosave.take();
        n
    };
    let mut b = App::new(launch, Arc::new(Shared::default()), Some(dir), None);
    b.load_now();
    assert_eq!(b.mirror.drafts.len(), 1);
    assert_eq!(b.mirror.drafts[0].kind, Some(ThreadKind::Question));
    let mut rt = runtime(b);
    rt.update(move |s, _| s.active_mut().set_cursor(Some((0, LineRef::new(Side::New, n)))));
    settle(&mut rt);
    typed(&mut rt, "c");
    assert_eq!(rt.state.composer_edit.text, "half a question");
    assert_eq!(rt.state.composer_ui.mode, ComposerMode::Question, "the draft keeps its mode");
}

#[test]
fn the_long_editor_keeps_the_text_there_and_back() {
    let mut rt = on_a_line();
    typed(&mut rt, "c");
    typed(&mut rt, "long **thought**");
    click(&mut rt, "expand-btn");
    assert!(rt.state.composer_ui.long);
    assert!(live(&rt, "overlay-composer") && live(&rt, "long-composer"));
    assert!(live(&rt, "composer-preview"));
    assert!(!live(&rt, "scrim-overlay-composer"), "the long editor is not modal");
    assert_eq!(rt.state.composer_edit.text, "long **thought**");
    let before = rt.state.composer_ui.long_position();
    rt.update(|s, _| {
        s.drag_long_editor(rune::Phase::Down, 0.0, 0.0);
        s.drag_long_editor(rune::Phase::Up, 30.0, -500.0);
    });
    let after = rt.state.composer_ui.long_position();
    assert_eq!(after.0, before.0 + 30.0);
    assert_eq!(after.1, 0.0, "never above the reader");
    click(&mut rt, "long-back");
    assert!(!rt.state.composer_ui.long);
    assert!(!live(&rt, "overlay-composer"));
    assert_eq!(rt.state.composer_edit.text, "long **thought**");
    rt.update(|s, _| s.composer_edit.focused = true);
    press(&mut rt, Key::Enter, ctrl());
    assert_eq!(rt.state.shared.lock().threads.last().unwrap().messages[0].body, "long **thought**");
}

#[test]
fn suggestion_prefills_the_anchored_code_and_reads_back_as_a_diff() {
    let mut rt = on_a_line();
    let code = {
        let f = &rt.state.diff.files()[0];
        let n = f.lines.iter().filter(|l| l.new != 0).nth(2).unwrap().new;
        rain_comments::ops::anchor_text(f, &rain_core::Anchor::Line { side: Side::New, line: n }).unwrap()
    };
    typed(&mut rt, "c");
    click(&mut rt, "suggest-btn");
    assert!(rt.state.composer_edit.text.contains(&format!("```suggestion\n{code}\n```")));
    press(&mut rt, Key::Enter, ctrl());
    let t = rt.state.shared.lock().threads.last().cloned().unwrap();
    let shown = rain_comments::ui::display_body(&t.messages[0].body, t.code_snapshot.as_ref());
    assert!(shown.contains(&format!("```diff\n-{code}\n+{code}\n```")), "{shown}");
    let out = rain_comments::comments_output(&rt.state.shared.lock(), None);
    assert!(out.contains("SUGGESTED"), "{out}");
}

#[test]
fn an_open_card_shows_its_actions_and_folds_into_the_gutter() {
    let mut a = app(story(), true);
    seed_threads(&mut a);
    let id = a.mirror.threads[0].id.clone();
    let mut rt = runtime(a);
    rt.update({
        let id = id.clone();
        move |s, tx| s.goto_thread(&id, tx)
    });
    rt.update(|s, _| s.focused_thread = None);
    settle(&mut rt);
    for key in ["reply-btn", "ask-btn", "resolve-btn", "disp-agree", "disp-dismiss", "disp-defer"] {
        assert!(live(&rt, key), "an open card shows {key}");
    }
    rt.update({
        let id = id.clone();
        move |s, tx| s.card_action(&id, rain_comments::CardAction::Toggle, tx)
    });
    settle(&mut rt);
    assert!(rt.state.is_folded(&id), "Enter folds a diff thread into the gutter");
    assert!(!live(&rt, &format!("thread-{id}")), "its card leaves the diff");
    assert!(live(&rt, "gm"), "a badge marks it in the gutter");
    let (x, y) = rt.find("gm").unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(!rt.state.is_folded(&id), "pressing the badge unfolds it");
    assert!(live(&rt, &format!("thread-{id}")));
}

#[test]
fn your_own_question_card_offers_no_dispositions() {
    let mut rt = on_a_line();
    typed(&mut rt, "?");
    typed(&mut rt, "Why here?");
    press(&mut rt, Key::Enter, ctrl());
    let id = rt.state.shared.lock().threads.last().unwrap().id.clone();
    rt.update({
        let id = id.clone();
        move |s, _| s.focused_thread = Some(id)
    });
    settle(&mut rt);
    assert!(live(&rt, &format!("thread-{id}")));
    assert!(
        !live(&rt, "disp-agree") && !live(&rt, "disp-dismiss"),
        "Agree/Dismiss/Ask/Defer are for findings"
    );
    let hints = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("status-hints"))
        .and_then(|e| e.label)
        .unwrap_or_default();
    assert!(hints.contains("reply") && !hints.contains("agree"), "{hints}");
    for key in ["a", "x", "d"] {
        typed(&mut rt, key);
    }
    let t = rain_comments::ops::thread(&rt.state.mirror, &id).unwrap().clone();
    assert_eq!(t.disposition, None);
    assert_eq!(t.state, ThreadState::Open);
    assert!(rt.state.composer_ui.dismissing.is_none());
}

#[test]
fn dismiss_requires_a_reason_and_stores_it() {
    let mut a = app(story(), true);
    seed_threads(&mut a);
    let id = a.mirror.threads[0].id.clone();
    a.focused_thread = Some(id.clone());
    let mut rt = runtime(a);
    rt.update({
        let id = id.clone();
        move |s, tx| s.goto_thread(&id, tx)
    });
    settle(&mut rt);
    typed(&mut rt, "x");
    assert_eq!(rt.state.composer_ui.dismissing.as_deref(), Some(id.as_str()));
    assert_eq!(rain_comments::ops::thread(&rt.state.mirror, &id).unwrap().state, ThreadState::Open);
    click(&mut rt, "dismiss-false-positive");
    let t = rain_comments::ops::thread(&rt.state.mirror, &id).unwrap().clone();
    assert_eq!(t.state, ThreadState::Dismissed);
    assert_eq!(t.dismiss_reason, Some(DismissReason::FalsePositive));
    assert_eq!(t.disposition, Some(rain_core::Disposition::Dismiss));
    assert!(rt.state.composer_ui.dismissing.is_none());
}

#[test]
fn a_on_an_addressed_suggestion_confirms_it() {
    let mut a = app(story(), true);
    seed_threads(&mut a);
    let id = a.mirror.threads[0].id.clone();
    a.mutate(|s| {
        let t = s.threads.iter_mut().find(|t| t.id == id).unwrap();
        t.set_anchor_state(AnchorState::Outdated, None);
        s.bump_comments();
    });
    assert!(rain_comments::ops::addressed_suggested(&a.mirror.threads[0]));
    a.focused_thread = Some(a.mirror.threads[0].id.clone());
    let id = a.mirror.threads[0].id.clone();
    let mut rt = runtime(a);
    typed(&mut rt, "a");
    let t = rain_comments::ops::thread(&rt.state.mirror, &id).unwrap();
    assert_eq!(t.state, ThreadState::Addressed);
    assert_eq!(t.disposition, Some(rain_core::Disposition::Agree));
}

#[test]
fn orphan_scene_shows_the_last_seen_code_and_sha() {
    let mut a = app(story(), true);
    crate::tests::shell_scene("orphan")(&mut a);
    let orphan = a
        .mirror
        .threads
        .iter()
        .find(|t| t.placement() == AnchorState::Orphaned)
        .cloned()
        .expect("an orphan");
    let rt = runtime(a);
    assert!(live(&rt, "orphan"), "the strip lists the orphan card");
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref().is_some_and(|t| t.contains("var total = calc.Sum(payments);"))),
        "snapshot text"
    );
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.text.as_deref().is_some_and(|t| t.contains("a1b2c3d"))),
        "last-seen sha"
    );
    assert!(live(&rt, "strip-summary"));
    assert!(orphan.orphan.is_some());
}

#[test]
fn attach_here_reanchors_an_orphan_and_quotes_the_old_code() {
    let mut a = app(story(), true);
    crate::tests::shell_scene("orphan")(&mut a);
    let id = a
        .mirror
        .threads
        .iter()
        .find(|t| t.placement() == AnchorState::Orphaned)
        .unwrap()
        .id
        .clone();
    let mut rt = runtime(a);
    let n = rt.state.diff.files()[0].lines.iter().filter(|l| l.new != 0).nth(1).unwrap().new;
    rt.update(move |s, _| s.active_mut().set_cursor(Some((0, LineRef::new(Side::New, n)))));
    rt.update({
        let id = id.clone();
        move |s, tx| s.card_action(&id, rain_comments::CardAction::Attach, tx)
    });
    let t = rain_comments::ops::thread(&rt.state.mirror, &id).unwrap();
    assert_eq!(t.placement(), AnchorState::Placed);
    assert_eq!(t.anchor, rain_core::Anchor::Line { side: Side::New, line: n });
    assert!(t.messages.last().unwrap().body.contains("var total = calc.Sum(payments);"));
}

#[test]
fn replies_on_questions_and_at_claude_ask_again() {
    let mut rt = on_a_line();
    typed(&mut rt, "?");
    typed(&mut rt, "Why?");
    press(&mut rt, Key::Enter, ctrl());
    let id = rt.state.shared.lock().threads.last().unwrap().id.clone();
    rt.update({
        let id = id.clone();
        move |s, _| {
            s.mutate(|ss| {
                rain_comments::ops::reply(ss, &id, "Because.", Some("claude"));
            });
        }
    });
    assert_eq!(
        rain_comments::ops::thread(&rt.state.mirror, &id)
            .unwrap()
            .answer
            .as_ref()
            .unwrap()
            .status,
        AnswerStatus::Answered
    );
    rt.update({
        let id = id.clone();
        move |s, tx| s.card_action(&id, rain_comments::CardAction::Reply("And then?".into()), tx)
    });
    let t = rain_comments::ops::thread(&rt.state.mirror, &id).unwrap();
    assert_eq!(t.answer.as_ref().unwrap().status, AnswerStatus::Queued, "a follow-up re-asks");
    assert_eq!(asks(&rt), 2);

    let mut a = app(story(), true);
    seed_threads(&mut a);
    let c = a.mirror.threads[0].id.clone();
    let mut rt = runtime(a);
    rt.update({
        let c = c.clone();
        move |s, tx| s.card_action(&c, rain_comments::CardAction::Reply("plain reply".into()), tx)
    });
    assert_eq!(asks(&rt), 0);
    rt.update(move |s, tx| s.card_action(&c, rain_comments::CardAction::Reply("@claude explain".into()), tx));
    assert_eq!(asks(&rt), 1);
}

#[test]
fn comments_on_the_brief_and_map_nodes_use_the_same_composer() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| {
        s.comment_on_artifact(
            Target {
                kind: TargetKind::Brief,
                section: Some("how.2".into()),
                quote: Some("same unit of work".into()),
                ..Target::default()
            },
            ComposerMode::Question,
        )
    });
    settle(&mut rt);
    assert!(live(&rt, "overlay-composer"));
    assert!(
        rt.elements()
            .iter()
            .any(|e| e.key.as_deref() == Some("anchor") && e.text.as_deref() == Some("Brief › How 2")),
        "the artifact is the title"
    );
    typed(&mut rt, "why not after?");
    press(&mut rt, Key::Enter, ctrl());
    let t = rt.state.shared.lock().threads.last().cloned().unwrap();
    assert_eq!(t.file_path, "@brief");
    assert_eq!(t.target.as_ref().and_then(|t| t.section.as_deref()), Some("how.2"));
    assert_eq!(t.kind, Some(ThreadKind::Question));
    assert!(!live(&rt, "overlay-composer"));
    assert_eq!(asks(&rt), 1);

    rt.update(|s, _| {
        s.comment_on_artifact(
            Target {
                kind: TargetKind::Node,
                node: Some("voucher-service".into()),
                view: Some("architecture".into()),
                ..Target::default()
            },
            ComposerMode::Comment,
        )
    });
    settle(&mut rt);
    typed(&mut rt, "odd edge");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!live(&rt, "overlay-composer"));
    let d = rt.state.shared.lock().drafts.last().cloned().unwrap();
    assert_eq!(d.file_path, "@map");
    assert_eq!(d.text, "odd edge");
    rt.update(|s, _| {
        s.comment_on_artifact(
            Target {
                kind: TargetKind::Node,
                node: Some("voucher-service".into()),
                view: Some("architecture".into()),
                ..Target::default()
            },
            ComposerMode::Comment,
        )
    });
    settle(&mut rt);
    assert_eq!(rt.state.composer_edit.text, "odd edge", "the artifact draft comes back");
    press(&mut rt, Key::Enter, ctrl());
    let t = rt.state.shared.lock().threads.last().cloned().unwrap();
    assert_eq!(t.file_path, "@map");
    assert_eq!(t.target.as_ref().and_then(|t| t.node.as_deref()), Some("voucher-service"));
    assert!(rt.state.shared.lock().drafts.is_empty());
    assert_eq!(App::thread_title(&t), "Map › node voucher-service");
}

#[test]
fn composer_scenes_seed_what_they_show() {
    for (name, want) in [
        ("composer", "kind-menu"),
        ("composer-question", "question-hint"),
        ("thread-states", "answer-text"),
        ("orphan", "orphan"),
    ] {
        for size in [(1366.0, 768.0), (1920.0, 1080.0)] {
            let mut a = app(story(), true);
            crate::tests::shell_scene(name)(&mut a);
            let rt = sized_runtime(a, size);
            assert!(live(&rt, want), "{name} at {size:?} shows {want}");
        }
    }
}

fn act(rt: &mut Runtime<App>, id: &str, a: rain_comments::CardAction) {
    let id = id.to_owned();
    rt.update(move |s, tx| s.card_action(&id, a, tx));
    settle(rt);
}

#[test]
fn deleting_your_thread_offers_undo_that_puts_it_back_exactly() {
    let mut rt = on_a_line();
    rt.update(|s, _| seed_threads(s));
    typed(&mut rt, "c");
    typed(&mut rt, "Mine to delete");
    press(&mut rt, Key::Enter, ctrl());
    rt.update(|s, _| {
        s.mutate(|ss| {
            let t = ss.threads.pop().unwrap();
            ss.threads.insert(1, t);
        })
    });
    let before = rt.state.shared.lock().threads.clone();
    let id = before[1].id.clone();
    act(&mut rt, &id, rain_comments::CardAction::Delete);
    assert!(
        rain_comments::ops::thread(&rt.state.shared.lock(), &id).is_none(),
        "deleted at once"
    );
    assert!(live(&rt, &format!("toast-deleted-{id}-action")), "the toast offers Undo");
    typed(&mut rt, "u");
    assert_eq!(rt.state.shared.lock().threads, before, "u restores it, in place");
    assert_eq!(rt.state.focused_thread.as_deref(), Some(id.as_str()));
    assert!(rt.state.conversation.toasts.is_empty());

    act(&mut rt, &id, rain_comments::CardAction::Delete);
    let later = std::time::Instant::now() + crate::app::conversation::UNDO_DELETE_FOR;
    rt.state.expire_toasts(later);
    assert!(rt.state.conversation.toasts.is_empty(), "Undo lasts six seconds");
    typed(&mut rt, "u");
    assert!(
        rain_comments::ops::thread(&rt.state.shared.lock(), &id).is_none(),
        "then u leaves it deleted"
    );
}

#[test]
fn a_reply_draft_survives_esc_and_another_card_and_returns_with_its_thread() {
    let mut a = app(story(), true);
    seed_threads(&mut a);
    let (one, two) = (a.mirror.threads[0].id.clone(), a.mirror.threads[1].id.clone());
    let mut rt = runtime(a);
    act(&mut rt, &one, rain_comments::CardAction::OpenReply);
    rt.update(|s, _| {
        let _ = s.reply_edit.paste("half a thought");
    });
    act(&mut rt, &two, rain_comments::CardAction::OpenReply);
    assert_eq!(rt.state.reply_to.as_deref(), Some(two.as_str()));
    assert!(rt.state.reply_edit.text.is_empty(), "the other card's box starts empty");
    act(&mut rt, &one, rain_comments::CardAction::OpenReply);
    assert_eq!(rt.state.reply_edit.text, "half a thought", "Reply on another card kept it");

    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.reply_to.is_none(), "Esc closes the box");
    act(&mut rt, &one, rain_comments::CardAction::AskReply);
    assert_eq!(
        rt.state.reply_edit.text, "@claude half a thought",
        "Esc kept it; Ask leads with @claude"
    );

    act(&mut rt, &one, rain_comments::CardAction::CancelReply);
    act(&mut rt, &one, rain_comments::CardAction::OpenReply);
    assert_eq!(rt.state.reply_edit.text, "@claude half a thought");
    act(&mut rt, &one, rain_comments::CardAction::Reply("@claude half a thought".into()));
    assert!(rt.state.reply_drafts.is_empty(), "sending drops the draft");
}

#[test]
fn a_reload_keeps_an_open_composer_as_a_draft_and_frees_the_keys() {
    let mut rt = on_a_line();
    typed(&mut rt, "c");
    typed(&mut rt, "typed before the reload");
    rt.update(|s, _| s.reload());
    settle(&mut rt);
    assert!(rt.state.composer.is_none());
    assert!(!rt.state.typing(), "the keys reach the reader again");
    let drafts = &rt.state.mirror.drafts;
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].text, "typed before the reload");
}

/// Your own comment on the cursor's line, with its reply box open and some text in it.
fn replying_on_your_own() -> (Runtime<App>, String) {
    let mut rt = on_a_line();
    typed(&mut rt, "c");
    typed(&mut rt, "Mine");
    press(&mut rt, Key::Enter, ctrl());
    let id = rt.state.mirror.threads.last().unwrap().id.clone();
    act(&mut rt, &id, rain_comments::CardAction::OpenReply);
    rt.update(|s, _| {
        let _ = s.reply_edit.paste("half a reply");
    });
    assert!(rt.state.typing());
    (rt, id)
}

#[test]
fn a_reply_box_lets_go_of_the_keys_when_its_card_goes() {
    let (mut rt, id) = replying_on_your_own();
    act(&mut rt, &id, rain_comments::CardAction::Delete);
    assert!(rt.state.reply_to.is_none() && !rt.state.typing(), "delete");
    assert_eq!(rt.state.reply_drafts.get(&id).map(String::as_str), Some("half a reply"));
    typed(&mut rt, "u");
    act(&mut rt, &id, rain_comments::CardAction::OpenReply);
    assert_eq!(rt.state.reply_edit.text, "half a reply", "the draft comes back with the thread");

    act(&mut rt, &id, rain_comments::CardAction::Toggle);
    assert!(rt.state.is_folded(&id));
    assert!(rt.state.reply_to.is_none() && !rt.state.typing(), "fold");

    let (mut rt, id) = replying_on_your_own();
    {
        let mut s = rt.state.shared.lock();
        rain_comments::ops::delete_thread(&mut s, &id);
    }
    rt.state.shared.notify();
    settle(&mut rt);
    assert!(rt.state.reply_to.is_none() && !rt.state.typing(), "removed by an agent");
}

#[test]
fn a_reply_to_a_thread_gone_meanwhile_says_so_and_keeps_the_text() {
    let (mut rt, id) = replying_on_your_own();
    rt.update({
        let id = id.clone();
        move |s, tx| {
            {
                let mut ss = s.shared.lock();
                rain_comments::ops::delete_thread(&mut ss, &id);
            }
            s.card_action(&id, rain_comments::CardAction::Reply("half a reply".into()), tx);
        }
    });
    assert!(rt.state.status.contains("not sent"), "{}", rt.state.status);
    assert_eq!(rt.state.reply_drafts.get(&id).map(String::as_str), Some("half a reply"));
    assert!(!rt.state.typing());
}

#[test]
fn u_undoes_whichever_came_last_and_newer_toasts_keep_the_undo() {
    let (mut rt, id) = replying_on_your_own();
    rt.update(|s, _| s.keep_reply_draft());
    act(&mut rt, &id, rain_comments::CardAction::Delete);
    for i in 0..4 {
        rt.update(move |s, _| s.push_toast(format!("note-{i}"), "Claude replied".into(), None));
    }
    assert!(
        rt.state.conversation.toasts.iter().any(|t| t.key == format!("deleted-{id}")),
        "newer toasts don't push the Undo out"
    );
    rt.update(|s, _| {
        s.mutate(|ss| {
            ss.accepted.push(rain_core::AcceptedGroup {
                group: rain_core::Group::Must,
                reason: "accepted".into(),
                at: rain_comments::clock::now_iso(),
                ..Default::default()
            });
            ss.version += 1;
        })
    });
    typed(&mut rt, "u");
    assert!(
        rt.state.mirror.accepted.last().unwrap().undone,
        "the accept came last, so u undoes it"
    );
    assert!(rain_comments::ops::thread(&rt.state.mirror, &id).is_none());
    typed(&mut rt, "u");
    assert!(rain_comments::ops::thread(&rt.state.mirror, &id).is_some(), "then the delete");
    let states: Vec<String> = rt
        .state
        .shared
        .events_since(0, &[EventKind::State])
        .iter()
        .filter(|e| e.thread.as_deref() == Some(id.as_str()))
        .map(|e| e.data["state"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(states, ["deleted", "restored"], "agents hear both");
}

#[test]
fn send_on_an_empty_box_says_why_and_keeps_the_box() {
    let mut rt = on_a_line();
    typed(&mut rt, "c");
    click(&mut rt, "send-btn");
    assert!(rt.state.composer.is_some(), "nothing sent, the composer stays");
    assert!(rt.state.composer_edit.focused, "the box takes focus");
    assert_eq!(rt.state.status, "Nothing to send: type a comment first");
    assert!(rt.state.shared.lock().threads.is_empty());
}

#[test]
fn folded_thread_badges_reach_assistive_tech() {
    fn walk<'a>(v: &'a serde_json::Value, out: &mut Vec<&'a serde_json::Value>) {
        out.push(v);
        for k in v["children"].as_array().into_iter().flatten() {
            walk(k, out);
        }
    }
    let mut a = app(story(), true);
    seed_threads(&mut a);
    let id = a.mirror.threads[0].id.clone();
    let mut rt = runtime(a);
    rt.update({
        let id = id.clone();
        move |s, tx| s.goto_thread(&id, tx)
    });
    act(&mut rt, &id, rain_comments::CardAction::Toggle);
    assert!(rt.state.is_folded(&id));
    let tree = rt.accessibility_json();
    let mut all = Vec::new();
    walk(&tree, &mut all);
    let badge = all
        .iter()
        .find(|n| n["name"].as_str().is_some_and(|s| s.starts_with("Folded issue")))
        .unwrap_or_else(|| panic!("no folded badge in the tree"));
    assert_eq!(badge["role"], "button");
}
