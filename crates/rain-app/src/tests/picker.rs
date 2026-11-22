use std::sync::Arc;

use rain_core::keymap::Profile;
use rain_core::{Launch, Shared};
use rune::{Key, Mods, Runtime};

use super::*;
use crate::app::layout::Overlay;
use crate::settings::{PICKER_MIN, Settings};

fn app_at(dir: &std::path::Path) -> App {
    let mut a = App::new(
        Launch {
            session: "t".into(),
            fixture: Some(story()),
            overview: Some(overview()),
            ..Launch::default()
        },
        Arc::new(Shared::default()),
        Some(dir.join("sessions")),
        Some(dir.join("settings.json")),
    );
    a.load_now();
    a
}

fn rect(rt: &Runtime<App>, key: &str) -> [f32; 4] {
    rt.presented(key).unwrap_or_else(|| panic!("{key} is drawn")).0
}

/// Whether `row` is drawn wholly inside the viewport `list`.
fn shown_in(rt: &Runtime<App>, list: &str, row: &str) -> bool {
    let (l, r) = (rect(rt, list), rect(rt, row));
    r[1] >= l[1] - 0.5 && r[1] + r[3] <= l[1] + l[3] + 0.5
}

fn drag(rt: &mut Runtime<App>, key: &str, dx: f32, dy: f32) {
    let (x, y) = rt.find(key).unwrap_or_else(|| panic!("{key}"));
    rt.pointer_down(x, y);
    rt.frame(1.0 / 60.0);
    for i in 1..=4 {
        let f = i as f32 / 4.0;
        rt.pointer_move(x + dx * f, y + dy * f);
        rt.frame(1.0 / 60.0);
    }
    rt.pointer_up(x + dx, y + dy);
    settle(rt);
}

fn open(rt: &mut Runtime<App>) {
    press(rt, Key::Command('p'), ctrl());
    assert!(rt.state.layout.is_open(Overlay::Picker));
}

#[test]
fn the_picker_resizes_by_its_edges_and_reopens_at_the_saved_size() {
    let dir = temp_dir();
    let mut rt = runtime(app_at(&dir));
    open(&mut rt);
    let r = rect(&rt, "picker");
    assert_eq!((r[2], r[3]), (940.0, 520.0), "the default size");
    let list = rect(&rt, "picker-list")[2];

    drag(&mut rt, "picker-grip-right", 50.0, 30.0);
    let r = rect(&rt, "picker");
    assert_eq!(
        (r[2], r[3]),
        (1040.0, 520.0),
        "the right edge: width only, both sides as it stays centred"
    );
    assert_eq!(
        rect(&rt, "picker-list")[2],
        list,
        "the list keeps its width; the preview takes the rest"
    );
    drag(&mut rt, "picker-grip-bottom", 40.0, 60.0);
    assert_eq!(rect(&rt, "picker")[3], 580.0, "the bottom edge: height only");
    drag(&mut rt, "picker-grip-corner", -100.0, -100.0);
    let r = rect(&rt, "picker");
    assert_eq!((r[2], r[3]), (840.0, 480.0), "the corner: both");
    assert_eq!(rect(&rt, "picker-list")[2], 378.0, "a narrow panel gives the list 45%");

    drag(&mut rt, "picker-grip-corner", -2000.0, -2000.0);
    let r = rect(&rt, "picker");
    assert_eq!((r[2], r[3]), PICKER_MIN, "never below the minimum");
    drag(&mut rt, "picker-grip-corner", 3000.0, 3000.0);
    let r = rect(&rt, "picker");
    assert_eq!(r[2], 1600.0 - 40.0, "never wider than the window");
    assert!(r[1] + r[3] <= 1000.0 - 30.0, "never past the status bar: {r:?}");
    drag(&mut rt, "picker-grip-corner", -500.0, -200.0);
    let saved = rt.state.settings.picker_size;
    assert_eq!(saved, (rect(&rt, "picker")[2], rect(&rt, "picker")[3]));

    let text = std::fs::read_to_string(dir.join("settings.json")).unwrap();
    assert!(text.contains("\"pickerSize\""), "{text}");
    assert_eq!(Settings::load(&dir.join("settings.json")).picker_size, saved);
    press(&mut rt, Key::Enter, Mods::default());

    let mut again = runtime(app_at(&dir));
    open(&mut again);
    let r = rect(&again, "picker");
    assert_eq!((r[2], r[3]), saved, "a new window reopens it at the saved size");
}

#[test]
fn an_old_or_odd_settings_file_gives_the_default_picker_size() {
    let default = Settings::default().picker_size;
    assert_eq!(default, (940.0, 520.0));
    let old = Settings::parse(br#"{"theme": "dark", "sidebarWidth": 300}"#).expect("an old file loads");
    assert_eq!((old.picker_size, old.sidebar_width), (default, 300.0));
    for odd in [r#""big""#, "[1, 2]", "[-5, 10]", "null", "[1e300, 600]"] {
        let s = Settings::parse(format!(r#"{{"pickerSize": {odd}, "sidebarWidth": 300}}"#).as_bytes())
            .unwrap_or_else(|e| panic!("{odd} still loads: {e}"));
        assert_eq!(s.sidebar_width, 300.0, "{odd}: the rest is kept");
        assert!(s.picker_size == default || odd.contains("1e300"), "{odd}: {:?}", s.picker_size);
    }
    let s = Settings::parse(br#"{"pickerSize": [700, 400]}"#).unwrap();
    assert_eq!(s.picker_size, (700.0, 400.0));
}

#[test]
fn moving_the_picker_selection_keeps_its_row_in_view() {
    let mut rt = runtime(app(story(), true));
    open(&mut rt);
    let n = rt.state.picker_rows().len();
    assert!(n as f32 * 32.0 > rect(&rt, "picker-list")[3] + 64.0, "enough rows to scroll: {n}");
    for _ in 0..n + 3 {
        press(&mut rt, Key::Down, Mods::default());
        let sel = rt.state.picker.sel;
        assert!(shown_in(&rt, "picker-list", &format!("pick-{sel}")), "row {sel} after Down");
    }
    assert_eq!(rt.state.picker.sel, n - 1);
    assert!(!shown_in(&rt, "picker-list", "pick-0"), "the list scrolled");
    for _ in 0..n {
        press(&mut rt, Key::Up, Mods::default());
        let sel = rt.state.picker.sel;
        assert!(shown_in(&rt, "picker-list", &format!("pick-{sel}")), "row {sel} after Up");
    }
    assert_eq!(rt.state.picker.sel, 0);
}

#[test]
fn moving_the_palette_selection_keeps_its_row_in_view() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| s.open_palette());
    settle(&mut rt);
    let n = crate::palette::filtered(&rt.state, "").len();
    assert!(n > 20, "enough rows to scroll: {n}");
    for _ in 0..n + 3 {
        press(&mut rt, Key::Down, Mods::default());
        let sel = rt.state.palette.sel;
        assert!(shown_in(&rt, "palette-rows", &format!("pal-{sel}")), "row {sel} after Down");
    }
    assert_eq!(rt.state.palette.sel, n - 1);
    for _ in 0..n {
        press(&mut rt, Key::Up, Mods::default());
        let sel = rt.state.palette.sel;
        assert!(shown_in(&rt, "palette-rows", &format!("pal-{sel}")), "row {sel} after Up");
    }
    assert_eq!(rt.state.palette.sel, 0);
    assert!(shown_in(&rt, "palette-rows", "pal-group-0"), "the top shows its group's title");
}

fn file_rows(rt: &Runtime<App>) -> Vec<String> {
    let rows = rt.state.picker_rows().into_iter();
    rows.filter(|e| e.kind == "File").map(|e| e.label).collect()
}

/// The picker's file rows for `q`, as a fresh open on it lists them.
fn picked(rt: &mut Runtime<App>, q: &'static str) -> Vec<String> {
    rt.update(move |s, _| s.show_picker(q));
    settle(rt);
    file_rows(rt)
}

fn scans(rt: &Runtime<App>) -> u32 {
    rt.state.picker.cache.borrow().scans
}

#[test]
fn the_picker_ranks_names_over_folders_and_takes_words_in_any_order() {
    let mut rt = runtime(app(story(), true));
    let rows = picked(&mut rt, "serv");
    let voucher = rows.iter().position(|l| l == "Voucher.cs").expect("its folder matches");
    for name in ["AccountService.cs", "VoucherService.cs", "voucher.service.ts"] {
        let at = rows.iter().position(|l| l == name).unwrap_or_else(|| panic!("{name}: {rows:?}"));
        assert!(at < voucher, "{name} before a file only under server/: {rows:?}");
    }
    let rows = picked(&mut rt, "service voucher");
    assert_eq!(rows[..2], ["voucher.service.ts", "VoucherService.cs"], "{rows:?}");
    assert!(!rows.contains(&"AccountService.cs".to_owned()), "every word must match: {rows:?}");
    assert_eq!(picked(&mut rt, "voucher service"), rows, "in any order");
    assert_eq!(
        picked(&mut rt, "api controller")[0],
        "VouchersController.cs",
        "a word may be a folder's"
    );
    assert_eq!(picked(&mut rt, "voucher")[0], "Voucher.cs", "the whole name first");
    let rows = picked(&mut rt, "designer");
    assert_eq!(rows, ["20260101000000_AddVoucher.Designer.cs"], "no scattered-letter rows after it");
    let rows = picked(&mut rt, "vou");
    let real = rows.iter().take_while(|l| l.to_lowercase().contains("vou")).count();
    assert!(
        real >= 6 && rows.len() - real <= 1,
        "a stray v and an ou rank after every vou: {rows:?}"
    );
    assert!(picked(&mut rt, "zzzz").is_empty() && rt.state.picker_rows().is_empty());
    assert!(rt.elements().iter().any(|e| e.text.as_deref() == Some("Nothing matches")));
}

#[test]
fn a_mistyped_letter_is_forgiven_only_when_nothing_fits_as_typed() {
    let mut rt = runtime(app(story(), true));
    assert_eq!(picked(&mut rt, "contorller"), ["VouchersController.cs"]);
    let at = rt.state.picker_rows().iter().position(|e| e.kind == "File").unwrap();
    assert_eq!(pieces("VouchersController.cs", &rt.state.picker_marks()[at]), "Contr|ller");
    assert_eq!(scans(&rt), 2, "as typed, then forgiving");
    assert!(picked(&mut rt, "vsx").is_empty(), "a short word must be right");
    assert_eq!(picked(&mut rt, "voucher")[0], "Voucher.cs");
    assert_eq!(scans(&rt), 1, "what fits as typed is not widened");

    let rows = crate::palette::filtered(&rt.state, "toggel viewed");
    assert_eq!(rows[0].label, "Toggle viewed", "{rows:?}");
    assert_eq!(
        pieces("Toggle viewed", &crate::palette::marks(&rt.state, "toggel viewed", &rows)[0]),
        "Toggl|viewed"
    );
    assert!(crate::palette::filtered(&rt.state, "qzj").is_empty());
}

#[test]
fn the_file_you_are_on_gives_way_in_the_picker() {
    let mut rt = runtime(app(story(), true));
    let file = |rt: &Runtime<App>, end: &str| rt.state.keys.iter().position(|k| k.ends_with(end)).unwrap();
    let go = |rt: &mut Runtime<App>, g: usize| {
        rt.update(move |s, tx| {
            s.close_picker();
            s.goto_file(g, tx);
        });
        settle(rt);
        assert_eq!(rt.state.current(), Some(g));
    };
    let (account, voucher, other) = (
        file(&rt, "AccountService.cs"),
        file(&rt, "VoucherService.cs"),
        file(&rt, "VouchersController.cs"),
    );
    go(&mut rt, other);
    let order = rt.state.list_order();
    let at = |g: usize| order.iter().position(|&x| x == g).unwrap();
    let changed = |g: usize| {
        let f = &rt.state.diff.files()[g];
        f.additions + f.deletions
    };
    assert!(changed(account) > changed(voucher) && at(voucher) < at(account));
    let more_changed_first = ["AccountService.cs", "VoucherService.cs"];
    let pair = |rt: &mut Runtime<App>| -> Vec<String> {
        let rows = picked(rt, "service").into_iter();
        rows.filter(|l| l == "AccountService.cs" || l == "VoucherService.cs").collect()
    };
    assert_eq!(
        pair(&mut rt),
        more_changed_first,
        "equal fits: the more changed first, over list order"
    );
    go(&mut rt, account);
    assert_eq!(
        pair(&mut rt),
        ["VoucherService.cs", "AccountService.cs"],
        "the current file steps down, and stays listed"
    );
    go(&mut rt, voucher);
    assert_eq!(pair(&mut rt), ["AccountService.cs", "VoucherService.cs"]);
    go(&mut rt, account);
    let rows = picked(&mut rt, "accountservice");
    assert_eq!(rows[0], "AccountService.cs", "a clearly better fit still leads");
    let all = picked(&mut rt, "");
    let order = rt.state.list_order();
    let name = |g: usize| rt.state.keys[g].rsplit('/').next().unwrap().to_owned();
    let rest = order.iter().filter(|&&g| g != voucher).map(|&g| name(g));
    let want: Vec<String> = std::iter::once(name(voucher)).chain(rest).collect();
    assert_eq!(all, want, "no query: the file just left, then the list's order");
}

/// The keys of the picker's file rows with no query, as a fresh open lists them.
fn blank(rt: &mut Runtime<App>) -> Vec<String> {
    rt.update(|s, _| s.show_picker(""));
    settle(rt);
    let rows = rt.state.picker_rows();
    let files = rows.iter().filter_map(|e| match e.target {
        crate::app::picker::Target::File(g) => Some(rt.state.keys[g].clone()),
        _ => None,
    });
    let files: Vec<String> = files.collect();
    assert!(rows[..files.len()].iter().all(|e| e.kind == "File"), "files before the rest");
    files
}

#[test]
fn with_no_query_the_picker_leads_with_the_file_left_then_what_is_not_viewed() {
    let mut rt = runtime(app(story(), true));
    let order = rt.state.list_order();
    let keys = |rt: &Runtime<App>, gs: &[usize]| -> Vec<String> { gs.iter().map(|&g| rt.state.keys[g].clone()).collect() };
    let says_previous = |rt: &Runtime<App>, i: usize| row_texts(rt, i).0.iter().any(|t| t.0 == "Previous");
    assert_eq!(blank(&mut rt), keys(&rt, &order), "nothing viewed, nowhere left: the list's order");
    assert!(!says_previous(&rt, 0));

    let (a, b, c) = (order[0], order[2], order[4]);
    let path = rt.state.diff.files()[b].path.clone();
    let at = rain_core::Anchor::Line {
        side: rain_core::Side::New,
        line: 1,
    };
    rt.state
        .mutate(|s| rain_comments::ops::add_thread(s, "", &path, at, "Still open", None, None));
    rt.update(move |s, _| {
        s.set_viewed(a, true);
        s.set_viewed(b, true);
    });
    settle(&mut rt);
    let unviewed: Vec<usize> = order.iter().copied().filter(|&g| g != a && g != b).collect();
    let want = [&unviewed[..], &[b, a]].concat();
    assert_eq!(
        blank(&mut rt),
        keys(&rt, &want),
        "not viewed first in list order, then the viewed file with an open thread, then the viewed"
    );
    assert_eq!(scans(&rt), 0, "no query matches nothing");

    for g in [a, c] {
        rt.update(move |s, tx| {
            s.close_picker();
            s.goto_file(g, tx);
        });
        settle(&mut rt);
    }
    assert_eq!(rt.state.current(), Some(c));
    let rest: Vec<usize> = want.iter().copied().filter(|&g| g != a).collect();
    assert_eq!(
        blank(&mut rt),
        keys(&rt, &[&[a], &rest[..]].concat()),
        "the file the jump left leads, though viewed"
    );
    assert!(says_previous(&rt, 0) && !says_previous(&rt, 1), "and says what it is");
    typed(&mut rt, "v");
    assert!(!says_previous(&rt, 0), "a query ranks it like any file");
    press(&mut rt, Key::Backspace, Mods::default());
    assert!(says_previous(&rt, 0));
    press(&mut rt, Key::Enter, Mods::default());
    assert_eq!(rt.state.current(), Some(a), "Enter on the fresh picker goes back to it");
}

#[test]
fn the_picker_matches_once_per_query_not_per_key_or_frame() {
    let mut rt = runtime(app(story(), true));
    open(&mut rt);
    assert_eq!(scans(&rt), 0, "no query, nothing to match");
    typed(&mut rt, "vo");
    assert_eq!(scans(&rt), 2, "once per letter");
    let rows = rt.state.picker_rows();
    for key in [Key::Down, Key::Down, Key::Up] {
        press(&mut rt, key, Mods::default());
    }
    assert_eq!(rt.state.picker.sel, 1);
    assert_eq!(rt.state.picker_rows(), rows);
    assert_eq!(scans(&rt), 2, "moving the selection and redrawing match nothing");
    typed(&mut rt, "uc");
    assert_eq!(scans(&rt), 4);
    let narrowed = rt.state.picker_rows();
    rt.update(|s, _| s.show_picker("vouc"));
    assert_eq!(rt.state.picker_rows(), narrowed, "typing on finds what a fresh search finds");
    settle(&mut rt);
    typed(&mut rt, "h s");
    let narrowed = rt.state.picker_rows();
    rt.update(|s, _| s.show_picker("vouch s"));
    assert_eq!(rt.state.picker_rows(), narrowed, "a second word too");
    settle(&mut rt);

    let before = rt.state.picker_rows().len();
    rt.update(|s, _| {
        s.apply_filter(&rain_tree::TreeFilter {
            text: "voucher.service".into(),
            ..Default::default()
        })
    });
    settle(&mut rt);
    assert!(rt.state.picker_rows().len() < before, "a change to the list rebuilds the rows");
}

/// A row and the label in it, as laid out (where a move still easing ends).
struct RowBox {
    label: String,
    row: [f32; 4],
    /// The label's place inside its row, and its size.
    inner: [f32; 4],
}

/// The rows keyed `<row><i>`, each with its label: its first text.
fn row_boxes(rt: &Runtime<App>, row: &str) -> Vec<RowBox> {
    let all = rt.elements();
    (0..)
        .map_while(|i| {
            let key = format!("{row}{i}");
            let at = all.iter().position(|e| !e.exiting && e.key.as_deref() == Some(key.as_str()))?;
            let label = all
                .iter()
                .find(|e| e.parent == Some(at) && e.text.is_some())
                .expect("the row's label");
            let (r, l) = (all[at].target, label.target);
            Some(RowBox {
                label: label.text.clone().unwrap_or_default(),
                row: r,
                inner: [l[0] - r[0], l[1] - r[1], l[2], l[3]],
            })
        })
        .collect()
}

/// Checks that every row now sits where `plain` had its row (or, when headings sit between
/// rows, is its size), and that `marked` has its label at the same place in its row and at
/// the same size as without marks.
fn assert_nothing_moved(now: &[RowBox], plain: &[RowBox], aligned: bool, marked: &str, q: &str) {
    assert!(!now.is_empty(), "{q}: rows");
    let mut compared = Vec::new();
    for (i, b) in now.iter().enumerate() {
        if aligned {
            assert_eq!(b.row, plain[i].row, "{q}: row {i} keeps its slot");
        } else {
            assert_eq!(b.row[2..], plain[0].row[2..], "{q}: row {i} keeps its size");
        }
        if let Some(was) = plain.iter().find(|p| p.label == b.label) {
            assert_eq!(b.inner, was.inner, "{q}: {} is the same size at the same place in its row", b.label);
            compared.push(b.label.as_str());
        }
    }
    assert!(compared.contains(&marked), "{q}: {marked} was compared, {compared:?}");
}

/// The marked parts of `label`, `|` between them.
fn pieces(label: &str, marks: &[std::ops::Range<usize>]) -> String {
    marks.iter().map(|r| &label[r.clone()]).collect::<Vec<_>>().join("|")
}

#[test]
fn the_picker_colours_matched_letters_and_no_row_moves() {
    let mut rt = runtime(app(story(), true));
    open(&mut rt);
    assert!(rt.state.picker_marks().is_empty(), "no query, no marks");
    let plain = row_boxes(&rt, "pick-");
    assert!(plain.len() > 12, "{}", plain.len());
    for (q, label, want) in [
        ("vou", "Voucher.cs", "Vou"),
        ("ser", "AccountService.cs", "Ser"),
        ("vsc", "VouchersController.cs", "V|sC"),
        ("api controller", "VouchersController.cs", "Controller"),
        ("cs acc", "AccountService.cs", "Acc|cs"),
    ] {
        rt.update(move |s, _| s.show_picker(q));
        settle(&mut rt);
        let (rows, marks) = (rt.state.picker_rows(), rt.state.picker_marks());
        assert_eq!(rows.len(), marks.len(), "{q}");
        let at = rows.iter().position(|e| e.label == label).unwrap_or_else(|| panic!("{q}: {label}"));
        assert_eq!(pieces(label, &marks[at]), want, "{q}: {label}");
        assert_nothing_moved(&row_boxes(&rt, "pick-"), &plain, true, label, q);
    }
}

#[test]
fn the_palette_colours_matched_letters_and_no_row_moves() {
    let mut rt = runtime(app(story(), true));
    rt.update(|s, _| s.open_palette());
    settle(&mut rt);
    let plain = row_boxes(&rt, "pal-");
    assert!(plain.len() > 20, "{}", plain.len());
    for (q, label, want) in [
        ("line", "Next line", "line"),
        ("hunk", "Previous hunk", "hunk"),
        ("next file", "Next file", "Next|file"),
        ("nls", "Next list section", "N|l|s"),
    ] {
        typed(&mut rt, q);
        let rows = crate::palette::filtered(&rt.state, q);
        let marks = crate::palette::marks(&rt.state, q, &rows);
        let at = rows.iter().position(|e| e.label == label).unwrap_or_else(|| panic!("{q}: {label}"));
        assert_eq!(pieces(label, &marks[at]), want, "{q}: {label}");
        assert_nothing_moved(&row_boxes(&rt, "pal-"), &plain, false, label, q);
        rt.update(|s, _| s.open_palette());
        settle(&mut rt);
    }
    let rows = crate::palette::filtered(&rt.state, "");
    assert!(crate::palette::marks(&rt.state, "", &rows).iter().all(Vec::is_empty));
}

#[test]
fn the_palette_matches_and_marks_once_per_query_not_per_key_or_frame() {
    let mut rt = runtime(app(story(), true));
    let counts = |rt: &Runtime<App>| {
        let c = rt.state.palette.cache.borrow();
        (c.scans, c.marks_made)
    };
    let rest = |rt: &mut Runtime<App>| {
        for key in [Key::Down, Key::Down, Key::Up] {
            press(rt, key, Mods::default());
        }
        rt.update(|_, _| {});
        settle(rt);
    };
    rt.update(|s, _| s.open_palette());
    settle(&mut rt);
    assert_eq!(counts(&rt), (0, 1), "no query: nothing to match, one empty set of marks");
    typed(&mut rt, "vie");
    assert_eq!(counts(&rt), (3, 4), "once per letter");
    let rows = crate::palette::filtered(&rt.state, "vie");
    rest(&mut rt);
    assert_eq!(rt.state.palette.sel, 1);
    assert_eq!(crate::palette::filtered(&rt.state, "vie"), rows);
    assert_eq!(counts(&rt), (3, 4), "moving the selection and redrawing match and mark nothing");

    for (q, forgiven) in [("toggel viewed", true), ("zqxj", false)] {
        rt.update(|s, _| s.open_palette());
        settle(&mut rt);
        typed(&mut rt, q);
        let rows = crate::palette::filtered(&rt.state, q);
        assert_eq!(rows.iter().any(|e| e.label.starts_with("Toggle")), forgiven, "{q}: {rows:?}");
        let typed_once = counts(&rt);
        let letters = q.chars().filter(|c| !c.is_whitespace()).count() as u32;
        assert!(typed_once.0 > letters, "{q}: a forgiving pass ran, {typed_once:?}");
        rest(&mut rt);
        assert_eq!(counts(&rt), typed_once, "{q}: the forgiving pass is kept too");
        assert_eq!(crate::palette::filtered(&rt.state, q), rows, "{q}");
    }
}

/// A change adding a file of so many lines at each path.
fn added(files: &[(&str, usize)]) -> App {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let patch: String = files
        .iter()
        .map(|(p, n)| {
            let lines: String = (0..*n).map(|i| format!("+l{i}\n")).collect();
            format!(
                "diff --git a/{p} b/{p}\nnew file mode 100644\nindex 0000000..1111111\n--- /dev/null\n+++ b/{p}\n@@ -0,0 +1,{n} @@\n{lines}"
            )
        })
        .collect();
    let path = dir.join("added.patch");
    std::fs::write(&path, patch).unwrap();
    app_with(Launch {
        session: "t".into(),
        fixture: Some(path),
        ..Launch::default()
    })
}

/// The texts drawn straight in picker row `i`, each with its box: the name, the folder when
/// the file has one, then the counts; and the box of the row's group tag.
fn row_texts(rt: &Runtime<App>, i: usize) -> (Vec<(String, [f32; 4])>, [f32; 4]) {
    let all = rt.elements();
    let key = format!("pick-{i}");
    let at = all
        .iter()
        .position(|e| !e.exiting && e.key.as_deref() == Some(key.as_str()))
        .unwrap_or_else(|| panic!("{key}"));
    let inside = |e: &&rune::ElementInfo| e.parent == Some(at);
    let texts = all.iter().filter(inside).filter(|e| e.role == "text");
    let tag = all.iter().filter(inside).find(|e| e.key.as_deref() == Some("group-tag"));
    (
        texts.map(|e| (e.text.clone().unwrap_or_default(), e.target)).collect(),
        tag.expect("a file row's tag").target,
    )
}

#[test]
fn a_file_row_shows_its_folder_cut_at_the_far_end_with_its_matched_letters() {
    const DEEP: &str = "server/src/Acme.Shop.Api/Features/Settings/Vouchers/Handlers";
    let mut rt = runtime(added(&[
        ("web/src/app/icons/index.ts", 3),
        ("web/src/app/invoices/index.ts", 3),
        (
            "server/src/Acme.Shop.Api/Features/Settings/Vouchers/Handlers/VouchersController.cs",
            3,
        ),
        ("README.md", 3),
    ]));
    open(&mut rt);
    let row = |rt: &Runtime<App>, folder: &str| {
        let rows = rt.state.picker_rows();
        rows.iter()
            .position(|e| e.kind == "File" && e.detail == folder)
            .unwrap_or_else(|| panic!("{folder}: {rows:?}"))
    };
    let drawn = |rt: &Runtime<App>, i: usize| -> Vec<String> { row_texts(rt, i).0.into_iter().map(|t| t.0).collect() };
    for folder in ["web/src/app/icons", "web/src/app/invoices"] {
        let texts = drawn(&rt, row(&rt, folder));
        assert_eq!(texts[..2], ["index.ts", folder], "the same name, told apart by its folder");
    }
    let texts = drawn(&rt, row(&rt, ""));
    assert_eq!(texts[..2], ["README.md", "+3"], "no folder, nothing in its place");

    let boxes = |rt: &Runtime<App>| {
        let (texts, tag) = row_texts(rt, row(rt, DEEP));
        assert_eq!((texts[0].0.as_str(), texts[1].0.as_str()), ("VouchersController.cs", DEEP));
        (texts[0].1, texts[1].1, tag)
    };
    let (name, folder, tag) = boxes(&rt);
    let whole = rune::text::advance("VouchersController.cs", crate::theme::TYPE.ui);
    assert!(name[2] >= whole - 1.0, "the name keeps its room: {} of {whole}", name[2]);
    assert!(folder[0] >= name[0] + name[2], "the folder comes after the name");
    assert!(
        folder[0] + folder[2] <= tag[0] + 0.5,
        "and stops before the tag: {folder:?} {tag:?}"
    );
    assert!(
        rune::text::advance(DEEP, crate::theme::TYPE.small) > folder[2] + 1.0,
        "this one is too long for its room, so it is cut"
    );
    let plain = row_boxes(&rt, "pick-");

    for (q, at, in_folder, in_name) in [
        ("icons index", "web/src/app/icons", "icons", "index"),
        ("handlers controller", DEEP, "Handlers", "Controller"),
        ("settings vouch", DEEP, "Settings", "Vouch"),
        ("readme", "", "", "README"),
    ] {
        rt.update(move |s, _| s.show_picker(q));
        settle(&mut rt);
        let i = row(&rt, at);
        let rows = rt.state.picker_rows();
        let (marks, folders) = (rt.state.picker_marks(), rt.state.picker_folder_marks());
        assert_eq!((marks.len(), folders.len()), (rows.len(), rows.len()), "{q}");
        assert_eq!(pieces(&rows[i].label, &marks[i]), in_name, "{q}: the name");
        assert_eq!(pieces(&rows[i].detail, &folders[i]), in_folder, "{q}: the folder");
        assert_nothing_moved(&row_boxes(&rt, "pick-"), &plain, true, &rows[i].label, q);
        if at == DEEP {
            assert_eq!(boxes(&rt), (name, folder, tag), "{q}: marks move nothing in the row");
        }
    }
    rt.update(|s, _| s.show_picker(""));
    settle(&mut rt);
    assert!(rt.state.picker_folder_marks().is_empty(), "no query, no marks");
}

#[test]
fn a_keystroke_matches_once_and_marks_each_row_once_with_folders_shown() {
    let mut rt = runtime(app(story(), true));
    let marked = |rt: &Runtime<App>| rt.state.picker.cache.borrow().marked;
    open(&mut rt);
    assert_eq!((scans(&rt), marked(&rt)), (0, 0), "no query: nothing matched or marked");
    typed(&mut rt, "s");
    let first = rt.state.picker_len() as u32;
    assert!(first > 4, "{first}");
    assert_eq!(
        (scans(&rt), marked(&rt)),
        (1, first),
        "one pass, and one look per row for name and folder both"
    );
    typed(&mut rt, "e");
    let second = rt.state.picker_len() as u32;
    assert_eq!((scans(&rt), marked(&rt)), (2, first + second));
    for key in [Key::Down, Key::Down, Key::Up] {
        press(&mut rt, key, Mods::default());
    }
    rt.update(|_, _| {});
    settle(&mut rt);
    assert_eq!((scans(&rt), marked(&rt)), (2, first + second), "moving and redrawing do neither");
}

/// The folders of the picker's file rows for `q`, as a fresh open on it lists them.
fn picked_in(rt: &mut Runtime<App>, q: &'static str) -> Vec<String> {
    rt.update(move |s, _| s.show_picker(q));
    settle(rt);
    let rows = rt.state.picker_rows().into_iter();
    rows.filter(|e| e.kind == "File").map(|e| e.detail).collect()
}

#[test]
fn equal_fits_go_unviewed_first_then_open_threads_then_more_changed_lines() {
    let mut rt = runtime(added(&[
        ("one/Widget.cs", 3),
        ("two/Widget.cs", 9),
        ("three/Widget.cs", 3),
        ("four/Widget.cs", 3),
        ("big/WidgetFactory.cs", 40),
    ]));
    let file = |rt: &Runtime<App>, dir: &str| rt.state.keys.iter().position(|k| k.starts_with(dir)).unwrap();
    assert_eq!(
        picked_in(&mut rt, "widget"),
        ["two", "four", "one", "three", "big"],
        "more changed lines first, then list order (by path); a whole name still beats a bigger file"
    );
    assert_eq!(scans(&rt), 1, "ordering ties is no extra pass");

    let four = file(&rt, "four");
    let path = rt.state.diff.files()[four].path.clone();
    let at = rain_core::Anchor::Line {
        side: rain_core::Side::New,
        line: 1,
    };
    let id = rt
        .state
        .mutate(|s| rain_comments::ops::add_thread(s, "", &path, at, "Why a widget?", None, None));
    settle(&mut rt);
    assert_eq!(
        picked_in(&mut rt, "widget")[..2],
        ["four", "two"],
        "an open thread before more lines"
    );

    let (two, big) = (file(&rt, "two"), file(&rt, "big"));
    rt.update(move |s, _| {
        s.set_viewed(two, true);
        s.set_viewed(four, true);
    });
    settle(&mut rt);
    assert_eq!(
        picked_in(&mut rt, "widget"),
        ["one", "three", "four", "two", "big"],
        "what is not viewed first; among the viewed, the open thread"
    );
    rt.state.mutate(|s| rain_comments::ops::resolve(s, &id));
    settle(&mut rt);
    assert_eq!(
        picked_in(&mut rt, "widget")[..4],
        ["one", "three", "two", "four"],
        "a resolved thread raises nothing"
    );

    rt.update(move |s, _| {
        for g in 0..s.keys.len() {
            s.set_viewed(g, g == big);
        }
    });
    settle(&mut rt);
    assert_eq!(picked_in(&mut rt, "widgetf"), ["big"], "{:?}", rt.state.picker_rows());
    assert_eq!(picked_in(&mut rt, "widget").last().unwrap(), "big");
    assert_eq!(picked_in(&mut rt, "wf")[0], "big", "a better fit leads though it alone is viewed");
}

#[test]
fn the_picker_folds_no_open_file_for_being_viewed() {
    let mut rt = runtime(app(story(), true));
    let viewed = 1;
    rt.update(move |s, _| s.set_viewed(viewed, true));
    settle(&mut rt);
    assert!(rt.state.active().collapsed(viewed), "marking it viewed folds it");
    rt.update(move |s, _| s.active_mut().set_collapsed(viewed, false));
    settle(&mut rt);

    open(&mut rt);
    typed(&mut rt, "vou");
    assert!(!rt.state.active().collapsed(viewed), "opening the picker and typing fold nothing");
    press(&mut rt, Key::Enter, Mods::default());
    assert!(!rt.state.active().collapsed(viewed), "nor does going to the first match");
    for g in [3, viewed, 0] {
        open(&mut rt);
        let at = rt
            .state
            .picker_rows()
            .iter()
            .position(|e| e.target == crate::app::picker::Target::File(g));
        rt.update(move |s, tx| s.picker_select(at.expect("the file is listed"), tx));
        press(&mut rt, Key::Enter, Mods::default());
        assert_eq!(rt.state.current(), Some(g));
        assert!(
            rt.state.is_viewed(viewed) && !rt.state.active().collapsed(viewed),
            "picking file {g}"
        );
    }
}

fn mode(rt: &Runtime<App>) -> Option<String> {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("picker-mode"))
        .and_then(|e| e.text)
}

#[test]
fn vim_mode_opens_the_picker_in_insert_and_esc_gives_normal_mode() {
    let mut rt = runtime(app(story(), true));
    assert!(rt.state.keymap_profile().vim_motions(), "Vim is the default profile");
    open(&mut rt);
    assert!(!rt.state.picker_normal());
    assert_eq!(mode(&rt).as_deref(), Some("INSERT"));
    typed(&mut rt, "jk");
    assert_eq!(rt.state.picker.edit.text, "jk", "j and k type in insert mode");
    let at = rt.state.active().cursor();
    for _ in 0..2 {
        press(&mut rt, Key::Backspace, Mods::default());
    }

    press(&mut rt, Key::Down, Mods::default());
    assert_eq!(rt.state.picker.sel, 1, "↓ moves in insert mode");
    press(&mut rt, Key::Escape, Mods::default());
    assert!(rt.state.layout.is_open(Overlay::Picker), "Esc leaves insert mode, not the picker");
    assert!(rt.state.picker_normal());
    assert_eq!(mode(&rt).as_deref(), Some("NORMAL"));
    assert!(!rt.state.picker.edit.focused, "the query box lets go");
    assert!(rt.elements().iter().all(|e| e.caret.is_none()), "no caret in normal mode");

    let n = rt.state.picker_rows().len();
    typed(&mut rt, "jj");
    assert_eq!(rt.state.picker.sel, 3, "j moves down");
    typed(&mut rt, "k");
    assert_eq!(rt.state.picker.sel, 2, "k moves up");
    press(&mut rt, Key::Down, Mods::default());
    press(&mut rt, Key::Up, Mods::default());
    press(&mut rt, Key::Up, Mods::default());
    assert_eq!(rt.state.picker.sel, 1, "the arrows too");
    press(
        &mut rt,
        Key::Text("G".into()),
        Mods {
            shift: true,
            ..Mods::default()
        },
    );
    assert_eq!(rt.state.picker.sel, n - 1, "G: the bottom");
    assert!(shown_in(&rt, "picker-list", &format!("pick-{}", n - 1)));
    typed(&mut rt, "g");
    assert_eq!(rt.state.picker.sel, n - 1, "one g waits");
    typed(&mut rt, "g");
    assert_eq!(rt.state.picker.sel, 0, "gg: the top");
    assert!(shown_in(&rt, "picker-list", "pick-0"));
    typed(&mut rt, "xzc");
    assert_eq!(rt.state.picker.edit.text, "", "normal mode types nothing");
    assert!(
        rt.state.layout.is_open(Overlay::Picker) && rt.state.composer.is_none(),
        "nor runs the reader's keys"
    );
    assert_eq!(rt.state.active().cursor(), at, "the diff never moved");

    typed(&mut rt, "i");
    assert!(!rt.state.picker_normal() && rt.state.picker.edit.focused, "i: insert again");
    typed(&mut rt, "vsc");
    assert_eq!(rt.state.picker.edit.text, "vsc");
    press(&mut rt, Key::Escape, Mods::default());
    assert_eq!(rt.state.picker.edit.text, "vsc", "the query stays");
    typed(&mut rt, "/");
    assert!(rt.state.picker.edit.focused, "/: insert again");
    assert_eq!(rt.state.picker.edit.text, "vsc", "the / isn't typed");
    press(&mut rt, Key::Escape, Mods::default());
    press(&mut rt, Key::Enter, Mods::default());
    assert!(!rt.state.layout.is_open(Overlay::Picker), "Enter goes from normal mode");
    assert!(rt.state.keys[rt.state.current().unwrap()].ends_with("VouchersController.cs"));

    open(&mut rt);
    press(&mut rt, Key::Escape, Mods::default());
    press(&mut rt, Key::Escape, Mods::default());
    assert!(!rt.state.layout.is_open(Overlay::Picker), "Esc in normal mode closes");
    assert!(!rt.state.picker_normal());
}

#[test]
fn without_vim_the_picker_has_no_modes_and_esc_closes() {
    for p in [Profile::VsCode, Profile::JetBrains] {
        let mut rt = runtime(app(story(), true));
        rt.update(|s, _| s.set_profile(p));
        settle(&mut rt);
        rt.update(|s, tx| s.open_picker(tx));
        settle(&mut rt);
        assert_eq!(mode(&rt), None, "{p:?}: no mode tag");
        typed(&mut rt, "jk");
        assert_eq!(rt.state.picker.edit.text, "jk", "{p:?}");
        press(&mut rt, Key::Escape, Mods::default());
        assert!(!rt.state.layout.is_open(Overlay::Picker), "{p:?}: Esc closes");
        assert!(!rt.state.picker_normal());
    }
}
