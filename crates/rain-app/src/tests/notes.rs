use rain_comments::{add_notes, remove_note};
use rain_core::{Anchor, Note, NoteKind, Side};
use rain_diffview::LineRef;
use rune::{ElementInfo, Runtime};

use super::*;

const VOUCHER: &str = "server/src/Data/Voucher.cs";

fn note(kind: NoteKind, path: &str, anchor: Option<Anchor>, body: &str) -> Note {
    Note {
        id: String::new(),
        kind,
        repo: String::new(),
        file_path: path.into(),
        anchor,
        title: None,
        body: body.into(),
        author: Some("claude".into()),
        created_at: String::new(),
        updated_at: String::new(),
        ..Note::default()
    }
}

/// Adds notes through the shared session as the HTTP routes do; returns their ids.
fn push(rt: &mut Runtime<App>, notes: Vec<Note>) -> Vec<String> {
    let ids = {
        let mut s = rt.state.shared.lock();
        add_notes(&mut s, notes, "", false).ids
    };
    rt.state.shared.notify();
    settle(rt);
    ids
}

fn live(rt: &Runtime<App>, key: &str) -> Option<ElementInfo> {
    rt.elements().into_iter().find(|e| !e.exiting && e.key.as_deref() == Some(key))
}

fn goto(rt: &mut Runtime<App>, key: &str, line: Option<u32>) {
    let k = key.to_owned();
    rt.update(move |s, tx| {
        let g = s.index_of(&k).unwrap();
        match line {
            Some(n) => s.goto_line(g, LineRef::new(Side::New, n), tx),
            None => s.goto_file(g, tx),
        }
    });
    settle(rt);
}

fn three(rt: &mut Runtime<App>) -> Vec<String> {
    push(
        rt,
        vec![
            note(NoteKind::Story, "", None, "Adds **vouchers** to accounts."),
            note(NoteKind::File, VOUCHER, None, "New entity for one voucher row."),
            note(
                NoteKind::Line,
                VOUCHER,
                Some(Anchor::Line { side: Side::New, line: 9 }),
                "Amount is stored in cents.",
            ),
        ],
    )
}

#[test]
fn story_file_and_line_notes_render_where_they_belong() {
    let mut rt = runtime(app(story(), true));
    let ids = three(&mut rt);
    assert_eq!(ids.len(), 3);
    assert_eq!(rt.state.status, "Note added");

    rt.update(|s, _| s.set_reader_tab(crate::app::ReaderTab::Overview));
    settle(&mut rt);
    let story = live(&rt, &format!("note-{}", ids[0])).expect("story card on the Overview");
    assert!(story.path.contains("overview"), "{}", story.path);
    assert_eq!(story.label.as_deref(), Some("Note"));

    goto(&mut rt, VOUCHER, None);
    let file = live(&rt, &format!("note-{}", ids[1])).expect("file card");
    let g = rt.state.index_of(VOUCHER).unwrap();
    assert!(file.path.contains(&format!("file-notes-{g}")), "{}", file.path);

    assert!(rt.state.diff.annotations().iter().any(|a| a.id == format!("note:{}", ids[2])));
    goto(&mut rt, VOUCHER, Some(9));
    let line = live(&rt, &format!("note-{}", ids[2])).expect("line note row");
    assert!(!line.path.contains("overview"), "{}", line.path);
}

#[test]
fn overview_intent_notes_still_render_inline_with_their_label() {
    let mut rt = runtime(app(story(), true));
    goto(&mut rt, "server/src/Api/VouchersController.cs", Some(22));
    let n = live(&rt, "note-overview:record-before-refresh").expect("intent note row");
    assert_eq!(n.label.as_deref(), Some("Intent"));
    assert!(rt.state.mirror.notes.is_empty(), "overview notes are never stored");
}

#[test]
fn a_long_note_clips_and_show_all_expands_it() {
    let mut rt = runtime(app(story(), true));
    let soft = numbered(12, |i| format!("Line {i} of one paragraph."));
    let list = numbered(12, |i| format!("- Step {i} of the reasoning."));
    let ids = push(
        &mut rt,
        vec![
            note(NoteKind::File, VOUCHER, None, &soft),
            note(NoteKind::File, VOUCHER, None, &list),
        ],
    );
    goto(&mut rt, VOUCHER, None);
    assert!(live(&rt, &format!("note-{}", ids[0])).is_some());
    assert!(
        live(&rt, &format!("note-more-{}", ids[0])).is_none(),
        "a soft-wrapped paragraph draws a few lines and needs no toggle"
    );
    let ids = [ids[1].clone()];
    let more = format!("note-more-{}", ids[0]);
    let height = |rt: &Runtime<App>| {
        let card = live(rt, &format!("note-{}", ids[0])).unwrap();
        rt.elements()
            .into_iter()
            .find(|e| !e.exiting && e.key.as_deref() == Some("note-body") && e.path.starts_with(&card.path))
            .unwrap()
            .rect[3]
    };
    let clipped = height(&rt);
    let button = live(&rt, &more).expect("Show all");
    assert_eq!(button.label.as_deref(), Some("Show all"));
    let (x, y) = rt.find(&more).unwrap();
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(&mut rt);
    assert!(rt.state.note_expanded(&ids[0]));
    assert_eq!(live(&rt, &more).unwrap().label.as_deref(), Some("Show less"));
    assert!(height(&rt) > clipped + 20.0, "{} vs {clipped}", height(&rt));
}

#[test]
fn removing_a_note_removes_its_card_without_reload() {
    let mut rt = runtime(app(story(), true));
    let ids = three(&mut rt);
    goto(&mut rt, VOUCHER, Some(9));
    assert!(live(&rt, &format!("note-{}", ids[2])).is_some());
    for id in &ids {
        assert!(remove_note(&mut rt.state.shared.lock(), id));
    }
    rt.state.shared.notify();
    settle(&mut rt);
    assert_eq!(rt.state.status, "Note removed");
    for id in &ids {
        assert!(live(&rt, &format!("note-{id}")).is_none(), "{id}");
    }
    assert!(
        !rt.state
            .diff
            .annotations()
            .iter()
            .any(|a| ids.iter().any(|id| a.id == format!("note:{id}")))
    );
}

#[test]
fn notes_stay_out_of_the_comment_export() {
    let mut rt = runtime(app(story(), true));
    {
        let mut s = rt.state.shared.lock();
        rain_comments::ops::add_thread(
            &mut s,
            "server",
            "src/Data/Voucher.cs",
            Anchor::Line { side: Side::New, line: 9 },
            "A thread",
            Some("claude"),
            None,
        );
    }
    settle(&mut rt);
    let json = rain_comments::comments_json_string(&rt.state.mirror, None);
    let text = rain_comments::comments_output(&rt.state.mirror, None);
    three(&mut rt);
    assert_eq!(rt.state.mirror.notes.len(), 3);
    assert_eq!(rain_comments::comments_json_string(&rt.state.mirror, None), json);
    let out = rain_comments::comments_output(&rt.state.mirror, None);
    assert_eq!(out, text);
    assert!(!out.contains("cents") && !out.contains("voucher row"), "{out}");
}

#[test]
fn note_bodies_render_as_markdown() {
    let mut rt = runtime(app(story(), true));
    let ids = push(&mut rt, vec![note(NoteKind::File, VOUCHER, None, "Adds a **bold** claim.")]);
    goto(&mut rt, VOUCHER, None);
    let card = live(&rt, &format!("note-{}", ids[0])).unwrap();
    let texts: Vec<String> = rt
        .elements()
        .into_iter()
        .filter(|e| !e.exiting && e.path.starts_with(&card.path))
        .filter_map(|e| e.text)
        .collect();
    assert!(texts.iter().any(|t| t == "Adds a bold claim."), "{texts:?}");
    assert!(!texts.iter().any(|t| t.contains("**")), "{texts:?}");
}
