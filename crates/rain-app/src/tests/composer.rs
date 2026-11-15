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
    assert!(!live(&rt, "kind-issue"), "no type picker in Question mode");
    assert!(live(&rt, "mode-tabs"));
}

#[test]
fn c_opens_comment_mode_defaulting_to_comment() {
    let mut rt = on_a_line();
    typed(&mut rt, "c");
    assert_eq!(rt.state.composer_ui.mode, ComposerMode::Comment);
    assert_eq!(rt.state.composer_ui.kind, ThreadKind::Comment);
    assert!(live(&rt, "kind-comment"), "the type picker");
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
fn card_actions_show_only_on_the_focused_card() {
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
    assert!(!live(&rt, "disp-agree"), "idle cards show no actions");
    rt.update(move |s, _| s.focused_thread = Some(id.clone()));
    settle(&mut rt);
    assert!(live(&rt, "disp-agree"));
    assert!(live(&rt, "disp-dismiss"));
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
        .and_then(|e| e.text)
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
        ("composer", "kind-issue"),
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
