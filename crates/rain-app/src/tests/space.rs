//! What marking a file viewed costs, by cause: the rows it builds, the layouts it runs and the
//! parts of the session it copies.

use rune::{Key, Mods, Tx};

use super::counts::{Counts, counted};
use super::*;

// Today's counts on the story fixture, as upper bounds (allocations 2% over what was counted:
// 9,595, 12,876 and 4,272).
const MARK: Counts = Counts {
    commits: 1,
    line_rows: 21,
    fold_rows: 0,
    layouts: 1,
    rekeys: 1,
    allocs: 9_787,
    matches: 0,
};
const MARK_AND_TOP: Counts = Counts {
    commits: 2,
    line_rows: 23,
    allocs: 13_134,
    ..MARK
};
const MARK_BESIDE_THREADS: Counts = Counts {
    line_rows: 4,
    allocs: 4_357,
    ..MARK
};

fn story_rt() -> Runtime<App> {
    let mut rt = runtime(app(story(), true));
    typed(&mut rt, "jj");
    rt
}

/// The cost of marking the current file viewed through `mark`, to the window's rest.
fn marked(mark: impl FnOnce(&mut App, usize, &mut Tx)) -> Counts {
    let mut rt = story_rt();
    let cur = rt.state.current().unwrap();
    let c = counted(&mut rt, |rt| {
        rt.update(|s, tx| mark(s, cur, tx));
        settle(rt);
    });
    assert!(rt.state.is_viewed(cur) && rt.state.active().collapsed(cur));
    c
}

#[test]
fn a_viewed_mark_builds_only_the_rows_it_brings_on_screen() {
    marked(|s, g, _| s.set_viewed(g, true)).at_most(MARK, "the mark alone");
    marked(|s, g, tx| s.mark_viewed(g, true, tx)).at_most(MARK_AND_TOP, "the header's button and the toggle key");
    marked(|s, g, tx| {
        let key = s.keys[g].clone();
        s.tree.set_viewed(&key, true);
        s.persist_viewed_key(&key, true);
        s.top_next_open(g, tx);
    })
    .at_most(MARK_AND_TOP, "the tree's tick");
}

#[test]
fn a_viewed_mark_copies_no_thread_and_draws_none_again() {
    let mut a = app(story(), true);
    crate::seed_threads(&mut a);
    let mut rt = runtime(a);
    let with = rt.state.file_of_thread(&rt.state.mirror.threads[0]).unwrap();
    let mut others = (0..rt.state.keys.len()).rev().filter(|&g| g != with);
    let (first, second) = (others.next().unwrap(), others.next().unwrap());
    rt.update(move |s, _| s.set_viewed(first, true));
    settle(&mut rt);

    let kept = |a: &App| {
        let m = &a.mirror;
        (
            m.threads[0].id.as_ptr(),
            m.threads[0].messages[1].body.as_ptr(),
            m.threads[1].id.as_ptr(),
            m.viewed[0].file_path.as_ptr(),
            m.comments_version,
        )
    };
    let before = kept(&rt.state);
    let c = counted(&mut rt, |rt| {
        rt.update(move |s, _| s.set_viewed(second, true));
        settle(rt);
    });
    assert_eq!(rt.state.mirror.viewed.len(), 2);
    assert_eq!(*rt.state.shared.lock(), rt.state.mirror, "the window's copy follows the session");
    assert_eq!(kept(&rt.state), before, "the threads and the earlier mark were not copied again");
    c.at_most(MARK_BESIDE_THREADS, "a mark beside threads");
}

#[test]
fn mark_all_viewed_lays_the_diff_out_once() {
    let mut rt = story_rt();
    let c = counted(&mut rt, |rt| {
        rt.update(|s, _| s.mark_all_viewed());
        settle(rt);
    });
    assert!((0..rt.state.keys.len()).all(|g| rt.state.is_viewed(g) && rt.state.active().collapsed(g)));
    println!("mark all viewed: {c:?}");
    assert_eq!((c.layouts, c.rekeys), (1, 1), "{c:?}");
}

/// Whether file `g`'s header row is drawn whole inside the diff's viewport, with no other
/// file's header stuck over it.
fn header_shows(rt: &Runtime<App>, g: usize) -> bool {
    let els = rt.elements();
    let rect = |key: &str| els.iter().find(|e| !e.exiting && e.key.as_deref() == Some(key)).map(|e| e.rect);
    let (Some(view), Some(header)) = (rect("scroll"), rect(&format!("df{g}"))) else {
        return false;
    };
    let inside = header[1] >= view[1] - 0.5 && header[1] + header[3] <= view[1] + view[3] + 0.5;
    inside && rect("sticky").is_none_or(|s| s[1] + s[3] <= header[1] + 0.5)
}

/// `files` added files of `lines` lines each, then a last one: of `last` lines, or one line too
/// wide to draw whole for `None`.
fn walk_rt(files: usize, lines: usize, last: Option<usize>) -> Runtime<App> {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let file = |p: String, body: Vec<String>| {
        let n = body.len();
        let text: String = body.iter().map(|l| format!("+{l}\n")).collect();
        format!("diff --git a/{p} b/{p}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{p}\n@@ -0,0 +1,{n} @@\n{text}")
    };
    let mut patch: String = (0..files)
        .map(|f| file(format!("src/f{f:03}.cs"), (0..lines).map(|i| format!("l{i}")).collect()))
        .collect();
    patch.push_str(&file(
        "src/zlast.cs".into(),
        match last {
            Some(n) => (0..n).map(|i| format!("l{i}")).collect(),
            None => vec!["x".repeat(12_000)],
        },
    ));
    let path = dir.join("walk.patch");
    std::fs::write(&path, patch).unwrap();
    runtime(app(path, false))
}

#[test]
fn space_reaches_and_marks_the_last_files_the_end_of_the_diff_stops_short_of_the_top() {
    for (last_lines, what) in [
        (None, "a folded last file"),
        (Some(3), "a short last file"),
        (Some(200), "a long last file"),
    ] {
        let mut rt = walk_rt(120, 5, last_lines);
        let last = rt.state.keys.len() - 1;
        rt.update(move |s, _| (1..last - 1).for_each(|g| s.set_viewed(g, true)));
        settle(&mut rt);
        assert_eq!(rt.state.current(), Some(0));

        press(&mut rt, Key::Text(" ".into()), Mods::default());
        assert_eq!(rt.state.current(), Some(last - 1), "{what}: Space lands on the file before it");
        assert!(header_shows(&rt, last - 1), "{what}: with that file's header on screen");

        press(&mut rt, Key::Text(" ".into()), Mods::default());
        assert!(rt.state.is_viewed(last - 1), "{what}: the next Space marks the file it landed on");
        assert_eq!(rt.state.current(), Some(last), "{what}: and lands on the last file");
        assert!(header_shows(&rt, last), "{what}: its header is on screen");
        assert_eq!(rt.state.active().collapsed(last), last_lines.is_none(), "{what}");

        press(&mut rt, Key::Text(" ".into()), Mods::default());
        assert!(rt.state.is_viewed(last), "{what}: Space marks the last file");
        assert_eq!(rt.state.status, "Everything is viewed", "{what}");
        assert!(header_shows(&rt, last), "{what}: folded, its header is on screen");
    }
}

#[test]
fn a_jump_the_end_of_the_diff_stops_short_shows_the_files_own_header() {
    let mut rt = walk_rt(30, 40, None);
    let near_end = rt.state.keys.len() - 2;
    rt.update(move |s, tx| s.goto_file(near_end, tx));
    settle(&mut rt);
    let (view, header) = {
        let els = rt.elements();
        let y = |key: &str| els.iter().find(|e| !e.exiting && e.key.as_deref() == Some(key)).map(|e| e.rect[1]);
        (y("scroll").unwrap(), y(&format!("df{near_end}")).unwrap())
    };
    assert!(
        header > view && header < view + 38.0,
        "the end stops it a few points short: {header} {view}"
    );
    assert!(header_shows(&rt, near_end), "the header of the file above is not stuck over it");
    assert_eq!(rt.state.current(), Some(near_end), "and it is the reader's file");
}

#[test]
fn space_puts_the_last_file_at_the_top_as_a_jump_to_it_does() {
    let mut rt = story_rt();
    let last = rt.state.keys.len() - 1;
    for _ in 0..last {
        press(&mut rt, Key::Text(" ".into()), Mods::default());
    }
    assert_eq!(rt.state.current(), Some(last));
    let by_space = rt.state.active().scroll_offset();
    rt.update(move |s, tx| s.goto_file(last, tx));
    settle(&mut rt);
    assert_eq!(rt.state.active().scroll_offset(), by_space);
}
