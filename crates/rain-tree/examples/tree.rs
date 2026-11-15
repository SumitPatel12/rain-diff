//! The file tree over a synthetic 300-file, two-repo change.
//!
//! `cargo run -p rain-tree --example tree -- --snap tree.png --scene must-only --theme light`

use rain_core::FileTag;
use rain_tree::sample::sample;
use rain_tree::{TreeFilter, TreeState, TreeView};
use rune::{AppSpec, Cx, El, col, row, text};
use rune_kit::Kit;

struct App {
    tree: TreeState,
    status: String,
}

fn init() -> App {
    let s = sample(300);
    let tree = TreeState::build(&s.files, &s.repos, &s.tags, |k| s.viewed.contains(k), &s.comments);
    App {
        tree,
        status: "Click a file, tick a checkbox, or pick a chip".into(),
    }
}

fn view(s: &App, cx: &mut Cx<App>) -> El {
    let k = Kit::neutral(cx.system_dark());
    let tree = TreeView::new(|s: &App| &s.tree, |s: &mut App| &mut s.tree)
        .on_open(|s: &mut App, key, _| s.status = format!("open {key}"))
        .on_viewed(|s: &mut App, key, on, _| s.status = format!("{} {key}", if on { "viewed" } else { "unviewed" }))
        .on_mark_group_viewed(|s: &mut App, tag, _| s.status = format!("marked {} viewed", tag.label()))
        .build(s, &k);
    let side = col().key("side").w(380.0).h_full().fill(k.c.layer_01).child(tree);
    let main = col()
        .key("main")
        .grow(1.0)
        .h_full()
        .padding(16.0)
        .child(text(s.status.as_str()).font(k.ty.body).color(k.c.text_muted));
    row().w_full().h_full().fill(k.c.bg_base).child(side).child(main)
}

fn filtered(s: &mut App, f: TreeFilter) {
    s.tree.apply(&f);
}

fn main() {
    rune::run(AppSpec {
        scenes: &[
            ("all", |_| {}),
            ("must-only", |s| {
                filtered(
                    s,
                    TreeFilter {
                        tags: [FileTag::MustReview].into(),
                        ..TreeFilter::default()
                    },
                )
            }),
            ("unviewed", |s| {
                filtered(
                    s,
                    TreeFilter {
                        unviewed_only: true,
                        ..TreeFilter::default()
                    },
                )
            }),
            ("repo-filter", |s| {
                filtered(
                    s,
                    TreeFilter {
                        repos: [1].into(),
                        ..TreeFilter::default()
                    },
                )
            }),
            ("filtered-text", |s| {
                filtered(
                    s,
                    TreeFilter {
                        text: "vouchers".into(),
                        ..TreeFilter::default()
                    },
                )
            }),
        ],
        ..AppSpec::new("Rain tree", (1100.0, 800.0), init, view)
    });
}
