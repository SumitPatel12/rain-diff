//! The tree panel in a headless runtime: checkbox clicks, keys, chips, the group bar and the
//! accessibility tree.

use rain_core::FileTag;
use rain_tree::sample::sample;
use rain_tree::{TREE_KEY, TreeFilter, TreeState, TreeView};
use rune::{Cx, El, Env, Key, KeyEvent, Mods, Runtime, col};
use rune_kit::Kit;
use serde_json::Value;

struct App {
    tree: TreeState,
    opened: Vec<String>,
    viewed: Vec<(String, bool)>,
    groups: Vec<FileTag>,
    filters: Vec<TreeFilter>,
}

fn view(s: &App, _: &mut Cx<App>) -> El {
    let k = Kit::neutral(true);
    col().size(420.0, 600.0).child(
        TreeView::new(|s: &App| &s.tree, |s: &mut App| &mut s.tree)
            .on_open(|s: &mut App, key, _| s.opened.push(key.to_owned()))
            .on_viewed(|s: &mut App, key, on, _| s.viewed.push((key.to_owned(), on)))
            .on_mark_group_viewed(|s: &mut App, tag, _| s.groups.push(tag))
            .on_filter_change(|s: &mut App, f, _| s.filters.push(f.clone()))
            .build(s, &k),
    )
}

fn runtime(n: usize) -> Runtime<App> {
    let s = sample(n);
    let tree = TreeState::build(&s.files, &s.repos, &s.tags, |k| s.viewed.contains(k), &s.comments);
    runtime_with(tree)
}

fn runtime_with(tree: TreeState) -> Runtime<App> {
    let app = App {
        tree,
        opened: Vec::new(),
        viewed: Vec::new(),
        groups: Vec::new(),
        filters: Vec::new(),
    };
    let mut rt = Runtime::new(app, view, (420.0, 600.0), Env::new(true, 1.0));
    settle(&mut rt);
    rt
}

fn settle(rt: &mut Runtime<App>) {
    rt.frame(0.0);
    rt.settle_all();
    rt.frame(1.0 / 60.0);
}

fn walk<'a>(v: &'a Value, f: &mut dyn FnMut(&'a Value)) {
    f(v);
    for c in v["children"].as_array().into_iter().flatten() {
        walk(c, f);
    }
}

fn with_role<'a>(v: &'a Value, role: &str) -> Vec<&'a Value> {
    let mut out = Vec::new();
    walk(v, &mut |n| {
        if n["role"] == role {
            out.push(n)
        }
    });
    out
}

fn named<'a>(v: &'a Value, name: &str) -> Option<&'a Value> {
    let mut out = None;
    walk(v, &mut |n| {
        if out.is_none() && n["name"] == name {
            out = Some(n)
        }
    });
    out
}

fn tap_at(rt: &mut Runtime<App>, node: &Value) {
    let b: Vec<f32> = node["bounds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap() as f32)
        .collect();
    let (x, y) = (b[0] + b[2] / 2.0, b[1] + b[3] / 2.0);
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.pointer_up(x, y);
    settle(rt);
}

fn tap_named(rt: &mut Runtime<App>, name: &str) {
    let tree = rt.accessibility_json();
    let node = named(&tree, name).unwrap_or_else(|| panic!("no node named {name:?}")).clone();
    tap_at(rt, &node);
}

fn press(rt: &mut Runtime<App>, key: Key) {
    rt.key(KeyEvent {
        key,
        mods: Mods::default(),
    });
    settle(rt);
}

fn focused(rt: &Runtime<App>) -> Option<String> {
    rt.elements().into_iter().find(|e| e.focused).and_then(|e| e.key)
}

const VOUCHERS: &str = "server/src/Acme.Api/Controllers/VouchersController.cs";

#[test]
fn clicking_a_checkbox_reports_the_file_and_flips_it() {
    let mut rt = runtime(40);
    rt.update(|s, tx| {
        s.tree.reveal(VOUCHERS, tx);
    });
    settle(&mut rt);
    tap_named(&mut rt, "Viewed VouchersController.cs");
    assert_eq!(rt.state.viewed, [(VOUCHERS.to_owned(), true)]);
    assert!(rt.state.tree.file(VOUCHERS).unwrap().viewed);
    assert!(rt.state.opened.is_empty(), "the checkbox doesn't open the file");
    tap_named(&mut rt, "Viewed VouchersController.cs");
    assert_eq!(rt.state.viewed.last(), Some(&(VOUCHERS.to_owned(), false)));
}

#[test]
fn clicking_a_row_opens_the_file() {
    let mut rt = runtime(40);
    let tree = rt.accessibility_json();
    let row = with_role(&tree, "tree-item")
        .into_iter()
        .find(|n| n["name"] == "VouchersController.cs")
        .unwrap()
        .clone();
    tap_at(&mut rt, &row);
    assert_eq!(rt.state.opened, [VOUCHERS]);
    assert_eq!(rt.state.tree.model.cursor(), Some(VOUCHERS));
}

#[test]
fn keys_move_open_toggle_viewed_and_focus_the_filter() {
    let mut rt = runtime(40);
    press(&mut rt, Key::Tab);
    // Tab may land on the filter or a chip first; walk until the tree has focus.
    for _ in 0..20 {
        if focused(&rt).as_deref() == Some(TREE_KEY) {
            break;
        }
        press(&mut rt, Key::Tab);
    }
    assert_eq!(focused(&rt).as_deref(), Some(TREE_KEY));
    press(&mut rt, Key::Down);
    assert_eq!(rt.state.tree.model.cursor(), Some("server"));
    press(&mut rt, Key::Down);
    press(&mut rt, Key::Down);
    press(&mut rt, Key::Down);
    let cursor = rt.state.tree.model.cursor().unwrap().to_owned();
    let file = rt.state.tree.cursor_file().expect("the cursor is on a file").key.clone();
    assert_eq!(cursor, file);
    let before = rt.state.tree.file(&file).unwrap().viewed;
    press(&mut rt, Key::Text(" ".into()));
    assert_eq!(rt.state.tree.file(&file).unwrap().viewed, !before, "Space toggles viewed");
    assert_eq!(rt.state.viewed.last(), Some(&(file.clone(), !before)));
    assert!(rt.state.opened.is_empty(), "Space doesn't open");
    press(&mut rt, Key::Enter);
    assert_eq!(rt.state.opened, [file.as_str()]);
    press(&mut rt, Key::Text("j".into()));
    assert_ne!(rt.state.tree.model.cursor(), Some(file.as_str()), "j moves down");
    press(&mut rt, Key::Text("k".into()));
    assert_eq!(rt.state.tree.model.cursor(), Some(file.as_str()), "k moves back up");
    press(&mut rt, Key::Text("f".into()));
    assert!(rt.state.tree.filter_edit.focused, "f focuses the filter box");
}

#[test]
fn space_on_a_folder_marks_everything_under_it() {
    let mut rt = runtime(40);
    rt.update(|s, _| s.tree.model.select("server"));
    settle(&mut rt);
    rt.update(|s, tx| {
        s.tree.reveal("server", tx);
    });
    for _ in 0..30 {
        press(&mut rt, Key::Tab);
        if focused(&rt).as_deref() == Some(TREE_KEY) {
            break;
        }
    }
    press(&mut rt, Key::Text(" ".into()));
    let (v, t) = rt.state.tree.folder_counts("server");
    assert_eq!(v, t);
    assert!(rt.state.viewed.len() > 1 && rt.state.viewed.iter().all(|(k, on)| *on && k.starts_with("server/")));
}

#[test]
fn chips_filter_and_the_group_bar_marks_the_tag() {
    let mut rt = runtime(60);
    let must = rt.state.tree.tag_count(FileTag::MustReview);
    assert!(must > 0);
    tap_named(&mut rt, &format!("Must review, {must} files"));
    assert_eq!(rt.state.tree.filter.tags, [FileTag::MustReview].into());
    assert_eq!(rt.state.filters.len(), 1);
    assert_eq!(rt.state.tree.shown_keys().len(), must as usize);
    let tree = rt.accessibility_json();
    let chip = named(&tree, &format!("Must review, {must} files")).unwrap();
    assert!(
        chip["states"].as_array().unwrap().iter().any(|s| s == "checked" || s == "pressed"),
        "{chip}"
    );
    let (v, t) = rt.state.tree.tag_progress(FileTag::MustReview);
    tap_named(&mut rt, &format!("Mark all {t} viewed"));
    assert_eq!(rt.state.groups, [FileTag::MustReview]);
    assert_eq!(rt.state.viewed.len(), (t - v) as usize);
    assert_eq!(rt.state.tree.tag_progress(FileTag::MustReview), (t, t));
    tap_named(&mut rt, "Unviewed only");
    assert!(rt.state.tree.filter.unviewed_only);
    assert!(rt.state.tree.shown_keys().is_empty(), "every must-review file is viewed now");
}

/// The repo chips as (name, checked), found by their repo-name prefix.
fn repo_chips(rt: &Runtime<App>) -> Vec<(String, bool)> {
    let tree = rt.accessibility_json();
    with_role(&tree, "button")
        .into_iter()
        .filter_map(|n| {
            let name = n["name"].as_str()?;
            (name.starts_with("server, ") || name.starts_with("app, ")).then(|| {
                let on = n["states"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|s| s == "checked" || s == "pressed");
                (name.to_owned(), on)
            })
        })
        .collect()
}

#[test]
fn repo_chips_filter_by_repo_and_toggle_off() {
    let mut rt = runtime(60);
    let (server, client) = (rt.state.tree.repo_count(0), rt.state.tree.repo_count(1));
    assert!(server > 0 && client > 0);
    let toggles = with_role(&rt.accessibility_json(), "button")
        .into_iter()
        .filter_map(|n| n["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    assert!(toggles.contains(&format!("server, {server} files")), "{toggles:?}");
    assert!(toggles.contains(&format!("app, {client} files")), "{toggles:?}");
    let keys: Vec<String> = rt
        .elements()
        .into_iter()
        .filter_map(|e| e.key)
        .filter(|k| k.starts_with("chip-repo-"))
        .collect();
    assert_eq!(keys, ["chip-repo-0", "chip-repo-1"]);

    tap_named(&mut rt, &format!("app, {client} files"));
    assert_eq!(rt.state.filters.len(), 1);
    assert_eq!(rt.state.filters[0].repos, [1].into());
    assert_eq!(rt.state.tree.filter.repos, [1].into());
    let shown = rt.state.tree.shown_keys().iter().map(|k| k.to_string()).collect::<Vec<_>>();
    assert_eq!(shown.len(), client as usize);
    assert!(shown.iter().all(|k| k.starts_with("app/")));
    let chip = named(&rt.accessibility_json(), &format!("app, {client} files")).unwrap().clone();
    assert!(
        chip["states"].as_array().unwrap().iter().any(|s| s == "checked" || s == "pressed"),
        "{chip}"
    );

    let must = rt.state.tree.tag_count(FileTag::MustReview);
    tap_named(&mut rt, &format!("Must review, {must} files"));
    assert!(rt.state.tree.shown_keys().iter().all(|k| k.starts_with("app/")));
    assert_eq!(rt.state.tree.shown_keys().len(), must as usize, "repo and tag combine");
    let client_must = rt.state.tree.repo_count(1);
    assert_eq!(client_must, must);

    tap_named(&mut rt, &format!("app, {client_must} files"));
    assert!(rt.state.filters.last().unwrap().repos.is_empty(), "pressing again clears the repo");
    assert!(rt.state.tree.shown_keys().iter().any(|k| k.starts_with("server/")));
}

#[test]
fn no_repo_chips_with_a_single_repo() {
    let s = sample(40);
    let files: Vec<_> = s.files.iter().filter(|f| f.repo == 0).cloned().collect();
    let tree = TreeState::build(&files, &s.repos, &s.tags, |_| false, &s.comments);
    let rt = runtime_with(tree);
    assert!(rt.elements().iter().all(|e| e.key.as_deref() != Some("repo-chips")));
    assert!(repo_chips(&rt).is_empty());
    let rt = runtime(40);
    assert!(rt.elements().iter().any(|e| e.key.as_deref() == Some("repo-chips")));
    let chips = repo_chips(&rt);
    assert_eq!(chips.len(), 2);
    assert!(chips.iter().all(|(name, on)| name.ends_with(" files") && !on), "{chips:?}");
}

#[test]
fn every_row_chip_and_checkbox_has_a_label() {
    let rt = runtime(300);
    let tree = rt.accessibility_json();
    let mut checked = 0;
    for role in ["tree-item", "check-box", "button", "text-input", "search-input"] {
        for n in with_role(&tree, role) {
            let name = n["name"].as_str().unwrap_or("");
            assert!(!name.is_empty(), "unlabelled {role}: {n}");
            checked += 1;
        }
    }
    let rows = with_role(&tree, "tree-item");
    assert!(rows.len() >= 10, "{} rows built", rows.len());
    assert_eq!(with_role(&tree, "check-box").len(), rows.len(), "one checkbox per row");
    assert!(checked > rows.len() * 2);
    let vouchers = rows.iter().find(|n| n["name"] == "server").unwrap();
    assert!(
        vouchers["description"].as_str().unwrap_or("").contains("files viewed"),
        "{vouchers}"
    );
}
