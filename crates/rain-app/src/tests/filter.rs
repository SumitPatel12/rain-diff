use std::sync::Arc;
use std::time::Instant;

use rain_core::FileTag;
use rain_tree::TreeFilter;
use rune::{Key, Mods, Runtime};

use super::*;

fn m() -> Runtime<App> {
    runtime(app(workspace("fixtures/m.patch"), true))
}

fn press_at(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no element {key}"));
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

/// Taps `key`; a chip is pressed in the `⚲` popover, which is closed again after.
fn tap(rt: &mut Runtime<App>, key: &str) {
    if !key.starts_with("chip-") {
        return press_at(rt, key);
    }
    press_at(rt, "filter-chips-button");
    assert!(rt.state.menus.chips, "⚲ opens the chips");
    press_at(rt, key);
    assert!(rt.state.menus.chips, "the chips stay open while toggled");
    press_at(rt, "filter-chips-scrim");
    assert!(!rt.state.menus.chips, "a press outside closes them");
}

fn bar(rt: &Runtime<App>) -> Option<String> {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("filter-bar-text"))
        .and_then(|e| e.text)
}

fn repo_of(s: &App, g: usize) -> &str {
    &s.repos[s.diff.files()[g].repo as usize].id
}

fn apply(rt: &mut Runtime<App>, f: TreeFilter) {
    rt.update(|s, tx| {
        s.tree.apply(&f);
        s.on_filter_change(&f, tx);
    });
    settle(rt);
}

#[test]
fn a_repo_chip_narrows_the_diff_and_the_bar_says_so() {
    let mut rt = m();
    assert!(bar(&rt).is_none(), "no bar without a filter");
    tap(&mut rt, "chip-repo-1");
    let s = &rt.state;
    let n = s.active().files().len();
    assert!(n > 0 && n < 40, "{n}");
    assert!((0..n).all(|l| repo_of(s, s.global(l)) == "server"));
    assert_eq!(n, s.keys.iter().filter(|k| k.starts_with("server/")).count());
    assert_eq!(bar(&rt).as_deref(), Some(format!("{n} of 40 files — server").as_str()));
    let first = rt.state.global(0);
    assert_eq!(rt.state.current(), Some(first), "the reader lands on the subset's first file");
    for _ in 0..n + 2 {
        typed(&mut rt, "]");
        let g = rt.state.current().unwrap();
        assert_eq!(repo_of(&rt.state, g), "server", "] stays inside the subset");
    }
    assert_eq!(rt.state.current(), Some(rt.state.global(n - 1)));
}

#[test]
fn space_marks_viewed_and_walks_only_the_subset() {
    let mut rt = m();
    tap(&mut rt, "chip-repo-1");
    assert!(
        !rt.elements().iter().any(|e| e.focused),
        "a pressed chip hands Space back to the diff"
    );
    let n = rt.state.active().files().len();
    let first = rt.state.current().unwrap();
    press(&mut rt, Key::Text(" ".into()), Mods::default());
    assert!(rt.state.is_viewed(first));
    let key = rt.state.keys[first].clone();
    assert!(
        rt.state.tree.file(&key).unwrap().viewed,
        "the tree shows the mark made in the subset"
    );
    let next = rt.state.current().unwrap();
    assert_ne!(next, first);
    assert_eq!(repo_of(&rt.state, next), "server");
    for _ in 1..n {
        press(&mut rt, Key::Text(" ".into()), Mods::default());
    }
    assert!(rt.state.filter.subset.is_some(), "the walk never left the filter");
    assert!(rt.state.status.contains("Every filtered file is viewed"), "{}", rt.state.status);
    assert!((0..40).filter(|&g| repo_of(&rt.state, g) == "app").all(|g| !rt.state.is_viewed(g)));
}

#[test]
fn escape_clears_the_filter_and_keeps_the_file() {
    let mut rt = m();
    tap(&mut rt, "chip-repo-1");
    typed(&mut rt, "]");
    let at = rt.state.current();
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.filter.subset.is_none());
    assert!(rt.state.tree.filter.is_empty());
    assert!(bar(&rt).is_none(), "the bar is gone");
    assert_eq!(rt.state.active().files().len(), 40);
    assert_eq!(rt.state.current(), at, "the reader stays on the same file");
    assert_eq!(rt.state.status, "Filter cleared");
}

#[test]
fn ctrl_shift_l_and_the_clear_button_clear_the_filter() {
    let mut rt = m();
    tap(&mut rt, "chip-repo-0");
    assert!(rt.state.filter.subset.is_some());
    press(&mut rt, Key::Command('l'), Mods { shift: true, ..ctrl() });
    assert!(rt.state.filter.subset.is_none());
    tap(&mut rt, "chip-repo-0");
    assert!(rt.find("filter-clear").is_some());
    tap(&mut rt, "filter-clear");
    assert!(rt.state.filter.subset.is_none());
    assert!(rt.find("filter-bar").is_none());
}

#[test]
fn a_repo_and_a_tag_combine() {
    let mut rt = m();
    let tag = FileTag::MustReview;
    rt.update(|s, _| {
        s.tags = s
            .keys
            .iter()
            .enumerate()
            .map(|(i, k)| (k.clone(), if i % 3 == 0 { tag } else { FileTag::Skim }))
            .collect();
        let tags = s.tags.clone();
        s.tree.set_tags(&tags);
    });
    settle(&mut rt);
    let s = &rt.state;
    for repo in ["app/", "server/"] {
        let tagged = |t| s.keys.iter().any(|k| k.starts_with(repo) && s.tags[k] == t);
        assert!(tagged(tag) && tagged(FileTag::Skim), "{repo} has both tags");
    }
    tap(&mut rt, "chip-repo-1");
    tap(&mut rt, &format!("chip-{}", rain_tree::tag_slug(tag)));
    let s = &rt.state;
    let want: Vec<usize> = (0..40)
        .filter(|&g| repo_of(s, g) == "server" && s.tags[&s.keys[g]] == tag)
        .collect();
    assert!(!want.is_empty());
    assert_eq!(s.filter.subset.as_ref().unwrap().files, want);
    assert_eq!(
        bar(&rt).as_deref(),
        Some(format!("{} of 40 files — server, {}", want.len(), tag.label()).as_str())
    );
}

#[test]
fn a_text_filter_alone_builds_a_subset_in_reading_order() {
    let mut rt = m();
    let term = rt.state.keys[0].rsplit('/').next().unwrap().split('.').next().unwrap().to_owned();
    apply(
        &mut rt,
        TreeFilter {
            text: term.clone(),
            ..TreeFilter::default()
        },
    );
    let s = &rt.state;
    let files = &s.filter.subset.as_ref().expect("a subset").files;
    assert!(files.contains(&0));
    assert!(files.windows(2).all(|w| w[0] < w[1]), "reading order");
    assert!(bar(&rt).unwrap().ends_with(&format!("'{term}'")));
}

#[test]
fn apply_filter_sets_the_tree_and_the_diff_without_a_tx() {
    let mut rt = m();
    typed(&mut rt, "]");
    rt.update(|s, _| {
        s.apply_filter(&TreeFilter {
            repos: [0].into(),
            ..TreeFilter::default()
        })
    });
    settle(&mut rt);
    let s = &rt.state;
    assert_eq!(s.tree.filter.repos, [0].into());
    assert!((0..s.active().files().len()).all(|l| repo_of(s, s.global(l)) == "app"));
    assert!(s.current().is_some_and(|g| s.local(g).is_some()), "the reader is inside the subset");
    assert_eq!(rt.find("filter-chips-button").map(|_| ()), Some(()));
    rt.update(|s, _| s.menus.chips = true);
    settle(&mut rt);
    assert!(rt.find("chip-repo-0").is_some());
}

#[test]
fn jumping_to_a_file_the_filter_hides_clears_it() {
    let mut rt = m();
    tap(&mut rt, "chip-repo-1");
    let client = (0..40).find(|&g| repo_of(&rt.state, g) == "app").unwrap();
    rt.update(|s, tx| s.goto_file(client, tx));
    settle(&mut rt);
    assert!(rt.state.filter.subset.is_none());
    assert_eq!(rt.state.current(), Some(client));
    assert!(rt.state.status.starts_with("Filter cleared to show"), "{}", rt.state.status);
}

/// Whether full-list file `g` is folded in the diff on screen.
fn folded(rt: &Runtime<App>, g: usize) -> bool {
    rt.state.active().collapsed(rt.state.local(g).expect("the file is on screen"))
}

/// The server repo's files, narrowed to those whose path holds `text`.
fn server(text: &str) -> TreeFilter {
    TreeFilter {
        repos: [1].into(),
        text: text.into(),
        ..TreeFilter::default()
    }
}

/// A viewed file, folded under the server filter and opened again there; and a second server
/// file to mark viewed later.
fn viewed_and_reopened() -> (Runtime<App>, usize, usize) {
    let mut rt = m();
    let mut files = (0..40).filter(|&g| repo_of(&rt.state, g) == "server");
    let (open, other) = (files.nth(1).unwrap(), files.nth(1).unwrap());
    rt.update(move |s, _| s.set_viewed(open, true));
    apply(&mut rt, server(""));
    assert!(folded(&rt, open), "marked viewed, it is folded under the filter too");
    rt.update(move |s, _| {
        let l = s.local(open).unwrap();
        s.active_mut().set_collapsed(l, false);
    });
    settle(&mut rt);
    (rt, open, other)
}

#[test]
fn a_viewed_file_opened_under_a_filter_stays_open_as_the_filter_is_typed_and_cleared() {
    let (mut rt, open, other) = viewed_and_reopened();
    let before = rt.state.active().files().len();
    let name = rt.state.keys[open].rsplit('/').next().unwrap().to_owned();
    apply(&mut rt, server(&name));
    assert!(rt.state.active().files().len() < before, "the filter narrowed to another subset");
    assert!(!folded(&rt, open), "typing in the filter folds no file for being viewed");
    apply(&mut rt, server(""));
    assert!(!folded(&rt, open), "nor does widening it");

    rt.update(move |s, _| s.set_viewed(other, true));
    settle(&mut rt);
    assert!(folded(&rt, other), "marking a file viewed is what folds it");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.filter.subset.is_none());
    assert!(rt.state.is_viewed(open) && !folded(&rt, open), "clearing the filter keeps it open");
    assert!(folded(&rt, other), "and the file marked under the filter folded");
}

#[test]
fn a_viewed_file_opened_under_a_filter_stays_open_through_the_picker_and_a_jump_out_of_the_filter() {
    let (mut rt, open, other) = viewed_and_reopened();
    press(&mut rt, Key::Command('p'), ctrl());
    let name = rt.state.keys[other].rsplit('/').next().unwrap().to_lowercase();
    typed(&mut rt, &name[..1]);
    assert!(!folded(&rt, open), "searching in the picker folds nothing");
    let at = rt
        .state
        .picker_rows()
        .iter()
        .position(|e| e.target == crate::app::picker::Target::File(other));
    rt.update(move |s, tx| s.picker_select(at.expect("the file is listed"), tx));
    press(&mut rt, Key::Enter, Mods::default());
    assert_eq!(rt.state.current(), Some(other));
    assert!(!folded(&rt, open), "nor does picking another file of the filter");

    let client = (0..40).find(|&g| repo_of(&rt.state, g) == "app").unwrap();
    rt.update(move |s, tx| s.goto_file(client, tx));
    settle(&mut rt);
    assert!(rt.state.filter.subset.is_none(), "the jump left the filter");
    assert_eq!(rt.state.current(), Some(client));
    assert!(
        rt.state.is_viewed(open) && !folded(&rt, open),
        "the file opened under the filter is still open"
    );
}

#[test]
fn ctrl_l_opens_the_list_and_its_files_section_and_zoomed_an_overlay() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| {
        s.toggle_list();
        s.toggle_list_section(crate::app::board::FILES);
    });
    settle(&mut rt);
    assert!(!rt.state.sidebar.is_open());
    press(&mut rt, Key::Command('l'), ctrl());
    assert!(rt.state.sidebar.is_open());
    assert!(!rt.state.settings.section_collapsed(crate::app::board::FILES));
    assert!(rt.state.tree.filter_edit.focused);
    rt.update(|s, _| {
        s.tree.filter_edit.focused = false;
        s.toggle_zoom();
    });
    settle(&mut rt);
    press(&mut rt, Key::Command('l'), ctrl());
    assert!(rt.state.layout.is_open(Overlay::Files), "zoomed, the list comes as an overlay");
    assert!(!rt.state.sidebar.is_open(), "zoom stays");
    assert!(rt.find("overlay-files").is_some());
}

#[test]
fn a_subset_shares_the_files_and_costs_little() {
    let mut rt = m();
    tap(&mut rt, "chip-repo-1");
    let s = &rt.state;
    let sub = s.filter.subset.as_ref().unwrap();
    for (l, &g) in sub.files.iter().enumerate() {
        assert!(Arc::ptr_eq(&sub.diff.files()[l], &s.diff.files()[g]));
    }
    assert!(sub.diff.heap_bytes() <= 2 << 20, "{}", sub.diff.heap_bytes());
}

#[test]
fn threads_land_on_the_subset_rows() {
    let mut a = app(workspace("fixtures/m.patch"), true);
    let g = (0..40).find(|&g| a.repos[a.diff.files()[g].repo as usize].id == "server").unwrap();
    let f = a.diff.files()[g].clone();
    let line = f.lines.iter().find(|l| l.new != 0).unwrap().new;
    let path = f.path.clone();
    a.mutate(|s| {
        rain_comments::ops::add_thread(
            s,
            "server",
            &path,
            rain_core::Anchor::Line {
                side: rain_core::Side::New,
                line,
            },
            "on server",
            Some("claude"),
            None,
        )
    });
    let mut rt = runtime(a);
    tap(&mut rt, "chip-repo-1");
    let s = &rt.state;
    let l = s.local(g).unwrap();
    assert!(s.active().annotations().iter().any(|a| a.file == l && !a.id.starts_with("note:")));
}

/// Timings for docs/wu/filter.md: `cargo test -p rain-app --release subset_timings -- --ignored --nocapture`.
#[test]
#[ignore]
fn subset_timings() {
    for (name, fixture) in [("m.patch", workspace("fixtures/m.patch")), ("story.patch", story())] {
        let mut rt = runtime(app(fixture, true));
        let repos = rt.state.tree.repos().len();
        let f = TreeFilter {
            repos: [0].into(),
            ..TreeFilter::default()
        };
        let mut best = f64::MAX;
        let mut worst: f64 = 0.0;
        for _ in 0..20 {
            rt.update(|s, tx| {
                s.tree.apply(&TreeFilter::default());
                s.on_filter_change(&TreeFilter::default(), tx);
                s.tree.apply(&f);
                let t = Instant::now();
                s.on_filter_change(&f, tx);
                let ms = t.elapsed().as_secs_f64() * 1e3;
                best = best.min(ms);
                worst = worst.max(ms);
            });
        }
        let n = rt.state.filter.subset.as_ref().map_or(0, |s| s.files.len());
        println!(
            "{name}: {repos} repos, subset {n}/{} files, build best {best:.3} ms, worst {worst:.3} ms (20 runs)",
            rt.state.keys.len()
        );
    }
}
