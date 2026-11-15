use rain_core::{LineKind, Side, UiRequest};
use rain_diffview::LineRef;

use super::*;

/// The last file of the story and the new-side number of its last added line.
fn last_added(a: &App) -> (usize, String, u32) {
    let g = a.keys.len() - 1;
    let f = &a.diff.files()[g];
    let line = f.lines.iter().rev().find(|l| l.kind == LineKind::Added).map(|l| l.new).unwrap();
    (g, a.keys[g].clone(), line)
}

#[test]
fn a_goto_request_moves_the_window_to_the_file_and_line() {
    let mut rt = runtime(app(story(), true));
    let (g, key, line) = last_added(&rt.state);
    assert_ne!(rt.state.current(), Some(g), "starts elsewhere");
    rt.state.shared.push_request(UiRequest::Goto {
        path: key.clone(),
        line: Some(line),
        side: Side::New,
    });
    settle(&mut rt);
    assert_eq!(rt.state.current(), Some(g));
    let local = rt.state.local(g).unwrap();
    assert_eq!(rt.state.active().cursor(), Some((local, LineRef::new(Side::New, line))));
    assert_eq!(rt.state.status, format!("Claude: look at {key}:{line}"));
    assert!(rt.state.shared.take_requests().is_empty(), "drained");
}

#[test]
fn a_goto_request_for_a_file_or_thread_and_unknown_targets() {
    let mut a = app(story(), true);
    seed_threads(&mut a);
    let mut rt = runtime(a);
    let (g, key, _) = last_added(&rt.state);
    rt.state.shared.push_request(UiRequest::Goto {
        path: key.clone(),
        line: None,
        side: Side::New,
    });
    settle(&mut rt);
    assert_eq!(rt.state.current(), Some(g));
    assert_eq!(rt.state.status, format!("Claude: look at {key}"));

    let t = rt.state.mirror.threads[0].clone();
    rt.state.shared.push_request(UiRequest::GotoThread(t.id.clone()));
    settle(&mut rt);
    assert_eq!(rt.state.focused_thread.as_deref(), Some(t.id.as_str()));
    assert_eq!(rt.state.current(), rt.state.file_of_thread(&t));
    assert!(rt.state.status.starts_with("Claude: look at "), "{}", rt.state.status);

    rt.state.shared.push_request(UiRequest::Goto {
        path: "nowhere/x.cs".into(),
        line: Some(1),
        side: Side::New,
    });
    settle(&mut rt);
    assert_eq!(rt.state.status, "Claude pointed at nowhere/x.cs, which is not in this diff");
    rt.state.shared.push_request(UiRequest::GotoThread("gone".into()));
    settle(&mut rt);
    assert_eq!(rt.state.status, "Claude pointed at thread gone, which no longer exists");
}

#[test]
fn a_goto_path_never_lands_in_another_repo_or_on_a_guess() {
    let mut rt = runtime(app(story(), true));
    let (g, key, _) = last_added(&rt.state);
    let f = rt.state.diff.files()[g].clone();
    let own = rt.state.repos[f.repo as usize].id.clone();
    let other = rt.state.repos.iter().map(|r| r.id.clone()).find(|id| !id.is_empty() && *id != own);
    let other = other.expect("the story has two repos");
    let wrong = format!("{other}/{}", f.path);
    assert!(rt.state.index_of(&wrong).is_none());
    rt.state.shared.push_request(UiRequest::Goto {
        path: wrong.clone(),
        line: None,
        side: Side::New,
    });
    settle(&mut rt);
    assert_eq!(rt.state.status, format!("Claude pointed at {wrong}, which is not in this diff"));

    rt.state.shared.push_request(UiRequest::Goto {
        path: format!("checkout/{key}"),
        line: None,
        side: Side::New,
    });
    settle(&mut rt);
    assert_eq!(rt.state.current(), Some(g), "a unique tail still matches");
}
