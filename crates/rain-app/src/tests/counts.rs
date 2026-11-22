//! What an action costs, in counts that repeat exactly from run to run (a clock does not), and
//! the baselines the suite holds today's code to. How to use it: `docs/perf-counts.md`.

use rune::{Key, Mods, Runtime};

use super::*;

/// The work done on this thread, between two readings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Counts {
    /// Runs of the whole view.
    pub commits: u64,
    /// Diff line rows built; a row the view reused is not counted.
    pub line_rows: u64,
    /// Gap and file-note rows built.
    pub fold_rows: u64,
    /// Times a diff laid all its rows out again.
    pub layouts: u64,
    /// Layouts that changed the rows, so the list keyed every row again.
    pub rekeys: u64,
    /// Heap allocations.
    pub allocs: u64,
    /// Passes of the fuzzy matcher over the picker's or the palette's entries.
    pub matches: u64,
}

impl Counts {
    fn now(a: &App) -> Self {
        Self {
            commits: rune::commit_generation(),
            line_rows: rain_diffview::line_builds(),
            fold_rows: rain_diffview::fold_builds(),
            layouts: rain_diffview::layout_builds(),
            rekeys: rain_diffview::rekeys(),
            allocs: allocations(),
            matches: u64::from(a.picker.cache.borrow().scans) + u64::from(a.palette.cache.borrow().scans),
        }
    }

    fn since(self, then: Self) -> Self {
        Self {
            commits: self.commits - then.commits,
            line_rows: self.line_rows - then.line_rows,
            fold_rows: self.fold_rows - then.fold_rows,
            layouts: self.layouts - then.layouts,
            rekeys: self.rekeys - then.rekeys,
            allocs: self.allocs - then.allocs,
            // Opening the picker or the palette starts its count again.
            matches: self.matches.saturating_sub(then.matches),
        }
    }

    /// Fails, naming `what`, when any count is over its bound in `max`.
    #[track_caller]
    pub(crate) fn at_most(self, max: Self, what: &str) {
        println!("{what}: {self:?}");
        for (name, got, max) in [
            ("commits", self.commits, max.commits),
            ("line_rows", self.line_rows, max.line_rows),
            ("fold_rows", self.fold_rows, max.fold_rows),
            ("layouts", self.layouts, max.layouts),
            ("rekeys", self.rekeys, max.rekeys),
            ("allocs", self.allocs, max.allocs),
            ("matches", self.matches, max.matches),
        ] {
            assert!(got <= max, "{what}: {name} is {got}, over the bound of {max}");
        }
    }
}

/// Runs `act` and gives what it cost.
pub(crate) fn counted(rt: &mut Runtime<App>, act: impl FnOnce(&mut Runtime<App>)) -> Counts {
    let then = Counts::now(&rt.state);
    act(rt);
    Counts::now(&rt.state).since(then)
}

/// One key and the frames after it, until the window is at rest.
pub(crate) fn key_counts(rt: &mut Runtime<App>, key: Key, mods: Mods) -> Counts {
    counted(rt, |rt| press(rt, key, mods))
}

/// `frames` frames with no input.
pub(crate) fn idle_counts(rt: &mut Runtime<App>, frames: usize) -> Counts {
    counted(rt, |rt| {
        for _ in 0..frames {
            rt.frame(1.0 / 60.0);
        }
    })
}

fn text(s: &str) -> Key {
    Key::Text(s.into())
}

// Budgets (docs/perf.md): a fix lowers one here; raising one needs the reason in the commit message.
const ZERO: Counts = Counts {
    commits: 0,
    line_rows: 0,
    fold_rows: 0,
    layouts: 0,
    rekeys: 0,
    allocs: 0,
    matches: 0,
};
const CURSOR_DOWN: Counts = Counts {
    commits: 1,
    line_rows: 2,
    allocs: 2_925,
    ..ZERO
};
const IDLE_FRAMES: usize = 60;
const IDLE: Counts = Counts { allocs: 246, ..ZERO };
const SPACE: Counts = Counts {
    commits: 2,
    line_rows: 19,
    fold_rows: 0,
    layouts: 1,
    rekeys: 1,
    allocs: 12_355,
    ..ZERO
};
const FIND_LETTER: Counts = Counts {
    commits: 1,
    line_rows: 14,
    allocs: 4_783,
    ..ZERO
};
const FIND_ENTER: Counts = Counts {
    commits: 1,
    line_rows: 2,
    allocs: 3_634,
    ..ZERO
};

const HELD_PRESSES: usize = 10;
const PICKER_LETTER: Counts = Counts {
    commits: 1,
    allocs: 4_566,
    matches: 1,
    ..ZERO
};
const PICKER_ARROW: Counts = Counts {
    commits: 1,
    allocs: 5_975,
    ..ZERO
};
const PALETTE_LETTER: Counts = Counts {
    commits: 1,
    allocs: 4_147,
    matches: 1,
    ..ZERO
};
const PALETTE_ARROW: Counts = Counts {
    commits: 1,
    allocs: 3_346,
    ..ZERO
};
const WHEEL_FRAMES: usize = 60;
const WHEEL_STEP: f32 = 60.0;
const WHEEL: Counts = Counts {
    commits: 15,
    line_rows: 180,
    allocs: 47_292,
    ..ZERO
};

fn story_rt() -> Runtime<App> {
    let mut rt = runtime(app(story(), true));
    typed(&mut rt, "jj");
    rt
}

#[test]
fn one_cursor_down_stays_within_its_counts() {
    let mut rt = story_rt();
    let before = rt.state.active().cursor();
    let c = key_counts(&mut rt, text("j"), Mods::default());
    assert_ne!(rt.state.active().cursor(), before, "the cursor moved");
    c.at_most(CURSOR_DOWN, "cursor down");
}

#[test]
fn a_cursor_move_builds_the_two_rows_it_touches_with_threads_on_screen() {
    let mut rt = story_rt();
    assert!(
        rt.elements().iter().any(|e| !e.exiting && e.key.as_deref() == Some("line")),
        "a line with a thread under it is built"
    );
    for k in ["j", "k"] {
        let c = key_counts(&mut rt, text(k), Mods::default());
        assert_eq!((c.commits, c.line_rows), (1, 2), "{k}: the row left and the row reached, {c:?}");
    }
}

#[test]
fn a_cursor_move_builds_no_file_list_row_and_marking_viewed_builds_its_own() {
    let mut rt = story_rt();
    let before = crate::view::file_row_builds();
    typed(&mut rt, "jjkk");
    assert_eq!(crate::view::file_row_builds(), before, "the list's file rows are reused");
    let g = rt.state.current().unwrap();
    rt.update(|s, tx| s.mark_viewed(g, true, tx));
    settle(&mut rt);
    assert!(rt.state.is_viewed(g));
    assert!(crate::view::file_row_builds() > before, "the row that changed builds again");
}

#[test]
fn an_idle_frame_rebuilds_nothing() {
    let mut rt = story_rt();
    idle_counts(&mut rt, IDLE_FRAMES).at_most(IDLE, "idle frames");
}

#[test]
fn space_marks_viewed_and_goes_next_within_its_counts() {
    let mut rt = story_rt();
    let first = rt.state.current().unwrap();
    let c = key_counts(&mut rt, text(" "), Mods::default());
    assert!(rt.state.is_viewed(first));
    assert_ne!(rt.state.current(), Some(first));
    c.at_most(SPACE, "space");
}

#[test]
fn a_find_letter_and_enter_stay_within_their_counts() {
    let mut rt = story_rt();
    typed(&mut rt, "/");
    typed(&mut rt, "vouche");
    let hits = rt.state.find.hits.len();
    let c = key_counts(&mut rt, text("r"), Mods::default());
    assert!(
        !rt.state.find.hits.is_empty() && rt.state.find.hits.len() <= hits,
        "voucher narrows vouche"
    );
    c.at_most(FIND_LETTER, "find letter");
    press(&mut rt, Key::Enter, Mods::default());
    let at = rt.state.find.at;
    let c = key_counts(&mut rt, Key::Enter, Mods::default());
    assert_ne!(rt.state.find.at, at, "Enter went to the next hit");
    c.at_most(FIND_ENTER, "find Enter");
}

#[test]
fn a_held_cursor_key_costs_no_more_on_its_tenth_press_than_on_its_first() {
    let mut rt = story_rt();
    let first = key_counts(&mut rt, text("j"), Mods::default());
    println!("held cursor down, press 1: {first:?}");
    let bound = Counts {
        allocs: first.allocs + first.allocs / 50,
        ..first
    };
    for n in 2..=HELD_PRESSES {
        let before = rt.state.active().cursor();
        let c = key_counts(&mut rt, text("j"), Mods::default());
        assert_ne!(rt.state.active().cursor(), before, "press {n} moved the cursor");
        c.at_most(bound, &format!("held cursor down, press {n}"));
    }
}

#[test]
fn a_picker_letter_matches_once_and_builds_no_diff_row() {
    let mut rt = story_rt();
    press(&mut rt, Key::Command('p'), ctrl());
    typed(&mut rt, "v");
    let c = key_counts(&mut rt, text("o"), Mods::default());
    assert!(rt.state.picker_rows().len() > 1, "vo still finds files");
    assert_eq!(c.matches, 1, "{c:?}");
    c.at_most(PICKER_LETTER, "picker letter");
    key_counts(&mut rt, Key::Down, Mods::default()).at_most(PICKER_ARROW, "picker arrow");
}

#[test]
fn a_palette_letter_matches_once_and_builds_no_diff_row() {
    let mut rt = story_rt();
    rt.update(|s, _| s.open_palette());
    settle(&mut rt);
    typed(&mut rt, "v");
    let c = key_counts(&mut rt, text("i"), Mods::default());
    assert!(crate::palette::filtered(&rt.state, "vi").len() > 1, "vi still finds commands");
    assert_eq!(c.matches, 1, "{c:?}");
    c.at_most(PALETTE_LETTER, "palette letter");
    key_counts(&mut rt, Key::Down, Mods::default()).at_most(PALETTE_ARROW, "palette arrow");
}

#[test]
fn wheel_frames_inside_one_file_lay_nothing_out_and_build_only_the_rows_that_come_into_view() {
    let mut rt = runtime(app(super::find::big_patch(2_000), false));
    let area = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("diff-col"))
        .unwrap()
        .rect;
    let (x, y) = (area[0] + area[2] / 2.0, area[1] + area[3] / 2.0);
    let wheel = |rt: &mut Runtime<App>, frames: usize| {
        for _ in 0..frames {
            rt.wheel(x, y, -WHEEL_STEP);
            rt.frame(1.0 / 60.0);
        }
    };
    wheel(&mut rt, 5);
    let from = rt.state.active().scroll_offset();
    let c = counted(&mut rt, |rt| wheel(rt, WHEEL_FRAMES));
    let moved = rt.state.active().scroll_offset() - from;
    assert!(moved >= WHEEL_STEP * WHEEL_FRAMES as f32 - 1.0, "scrolled {moved}");
    assert_eq!(rt.state.active().visible_file(), 0, "still inside the one file");
    c.at_most(WHEEL, "wheel frames in one file");
}

#[test]
fn a_find_letter_builds_only_the_rows_whose_marks_change() {
    let mut rt = story_rt();
    typed(&mut rt, "/");
    typed(&mut rt, "vouche");
    let c = key_counts(&mut rt, text("r"), Mods::default());
    assert_eq!(
        (c.commits, c.line_rows, c.fold_rows),
        (1, 14, 0),
        "the 14 built rows that hold vouche, {c:?}"
    );
    let c = key_counts(&mut rt, text("#"), Mods::default());
    assert!(rt.state.find.hits.is_empty(), "no line holds voucher#");
    assert!(
        (1..=14).contains(&c.line_rows) && c.fold_rows == 0,
        "the rows that lose their marks, {c:?}"
    );
    let c = key_counts(&mut rt, text("#"), Mods::default());
    assert_eq!(
        (c.commits, c.line_rows, c.fold_rows),
        (1, 0, 0),
        "no row had a mark or gets one, {c:?}"
    );
}

#[test]
fn moving_the_current_match_builds_the_row_it_leaves_and_the_row_it_reaches() {
    let mut rt = story_rt();
    typed(&mut rt, "/");
    typed(&mut rt, "voucher");
    press(&mut rt, Key::Enter, Mods::default());
    let find = &rt.state.find;
    let (here, next) = (find.hits[find.at], find.hits[find.at + 1]);
    assert_ne!(here, next);
    let c = counted(&mut rt, |rt| {
        rt.update(move |s, _| s.active_mut().set_find("voucher", Some(next)));
        settle(rt);
    });
    assert_eq!((c.commits, c.line_rows, c.fold_rows), (1, 2, 0), "{c:?}");
    let c = counted(&mut rt, |rt| {
        rt.update(move |s, _| s.active_mut().set_fuzzy_find("voucher", Some(next)));
        settle(rt);
    });
    assert!(
        c.commits == 1 && c.line_rows > 0 && c.fold_rows == 0,
        "fuzzy marks its own rows, {c:?}"
    );
}
