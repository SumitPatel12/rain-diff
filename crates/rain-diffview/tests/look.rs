//! How files and changed lines look: a full-width header bar per file, and a theme's own fills
//! for added and removed lines.

mod common;

use common::patch::from_texts;
use rain_diffview::{DiffState, DiffView};
use rune::runtime::ElementInfo;
use rune::{Color, Cx, El, Env, Runtime, col};
use rune_kit::Kit;

const ADD: Color = Color {
    r: 0.05,
    g: 0.08,
    b: 0.06,
    a: 1.0,
};
const DEL: Color = Color {
    r: 0.08,
    g: 0.05,
    b: 0.06,
    a: 1.0,
};

struct App {
    diff: DiffState,
    fills: bool,
}

fn view(s: &App, _: &mut Cx<App>) -> El {
    let k = Kit::neutral(true);
    let mut diff = DiffView::new(|s: &App| &s.diff, |s: &mut App| &mut s.diff);
    if s.fills {
        diff = diff.change_fills(ADD, DEL);
    }
    col().w_full().h_full().child(diff.build(s, &k))
}

fn runtime(fills: bool) -> Runtime<App> {
    let old: String = (1..=40).map(|i| format!("line {i}\n")).collect();
    let a = from_texts("a.rs", &old, &old.replace("line 5\n", "line five\n"));
    let b = from_texts("b.rs", &old, &old.replace("line 30\n", "line thirty\n"));
    let mut rt = Runtime::new(
        App {
            diff: DiffState::new(vec![a, b]),
            fills,
        },
        view,
        (900.0, 1200.0),
        Env::new(true, 1.0),
    );
    rt.frame(0.0);
    for _ in 0..90 {
        rt.frame(1.0 / 60.0);
    }
    rt
}

fn find(rt: &Runtime<App>, end: &str) -> Option<ElementInfo> {
    let suffix = format!("/{end}");
    rt.elements().into_iter().find(|e| e.path.ends_with(&suffix) && !e.exiting)
}

#[test]
fn a_file_header_is_a_bar_across_the_view() {
    let rt = runtime(false);
    let head = find(&rt, "df1").expect("the second file's header");
    assert_eq!(head.rect[0], 0.0);
    assert_eq!(head.rect[2], 900.0, "full width");
}

#[test]
fn change_fills_paint_added_and_removed_lines() {
    let painted = |rt: &Runtime<App>, c: Color| rt.elements().iter().filter(|e| !e.exiting && e.fill == c).count();
    let plain = runtime(false);
    assert_eq!((painted(&plain, ADD), painted(&plain, DEL)), (0, 0), "the kit's tints by default");
    let themed = runtime(true);
    assert!(painted(&themed, ADD) > 0, "added lines take the theme's fill");
    assert!(painted(&themed, DEL) > 0, "removed lines take the theme's fill");
}
