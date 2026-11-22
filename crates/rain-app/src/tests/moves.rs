//! Moved blocks: found by the loader, grouped by triage, and one command between their two sides.

use rain_core::command::Command;
use rain_core::diff::Side;
use rain_diffview::LineRef;
use rain_triage::Group;

use super::*;

const PATCH: &str = r#"diff --git a/src/orders/total.rs b/src/orders/total.rs
--- a/src/orders/total.rs
+++ b/src/orders/total.rs
@@ -1,7 +1,2 @@
 use crate::shop::Order;
-fn tax_for(order: &Order, region: &Region) -> Money {
-    let rate = region.tax_rate(order.placed_on);
-
-    order.subtotal() * rate
-}
 // totals
diff --git a/src/shop/rates.rs b/src/shop/rates.rs
--- a/src/shop/rates.rs
+++ b/src/shop/rates.rs
@@ -1,2 +1,9 @@
 pub struct Rates;
 impl Rates {
+    fn tax_for(order: &Order, region: &Region) -> Money {
+        let rate = region.tax_rate(order.placed_on);
+
+        order.subtotal() * rate
+    }
+    fn none() {}
+}
"#;

const TOTAL: &str = "src/orders/total.rs";
const RATES: &str = "src/shop/rates.rs";

fn moved_app() -> App {
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("moved.patch");
    std::fs::write(&path, PATCH).unwrap();
    app(path, false)
}

#[test]
fn a_file_that_only_lost_a_moved_block_is_grouped_with_the_moves() {
    let a = moved_app();
    let item = |key: &str| a.board.items.iter().find(|i| i.key == key).unwrap();
    let total = item(TOTAL);
    assert_eq!(
        (total.group, total.reason.text.as_str(), total.reason.trusted),
        (Group::Moves, "moved blocks (same lines elsewhere)", false)
    );
    assert_eq!(item(RATES).group, Group::Review, "the file it moved into has new lines too");
    assert_eq!(a.keys, [RATES, TOTAL], "the list's order, which the moves follow");
    assert_eq!(a.moves.moves.len(), 1);
    assert!(
        !a.is_viewed(a.index_of(TOTAL).unwrap()),
        "grouped, never marked viewed for the reader"
    );
}

#[test]
fn one_command_goes_between_the_two_sides_of_a_move() {
    let mut rt = runtime(moved_app());
    let (total, rates) = (rt.state.index_of(TOTAL).unwrap(), rt.state.index_of(RATES).unwrap());
    let rows = crate::palette::filtered(&rt.state, "other side of a moved block");
    assert_eq!(rows[0].item, crate::palette::Item::Cmd(Command::OtherSideOfMove), "{rows:?}");
    assert_eq!(rows[0].hint, "", "no key until the user binds one");
    assert_eq!(Command::from_id("other-side-of-move"), Some(Command::OtherSideOfMove));

    rt.update(|s, tx| s.goto_line(total, LineRef::new(Side::Old, 3), tx));
    settle(&mut rt);
    rt.update(|s, tx| {
        s.run(Command::OtherSideOfMove, tx);
    });
    settle(&mut rt);
    let cursor = |rt: &Runtime<App>| rt.state.active().cursor().map(|(l, at)| (rt.state.global(l), at));
    assert_eq!(
        cursor(&rt),
        Some((rates, LineRef::new(Side::New, 4))),
        "the same line, where it went"
    );
    assert_eq!(rt.state.status, format!("The other side of the move: {RATES}:4"));
    rt.update(|s, tx| {
        s.run(Command::OtherSideOfMove, tx);
    });
    settle(&mut rt);
    assert_eq!(cursor(&rt), Some((total, LineRef::new(Side::Old, 3))), "and back");

    rt.update(|s, tx| s.goto_line(rates, LineRef::new(Side::New, 8), tx));
    settle(&mut rt);
    rt.update(|s, tx| {
        s.run(Command::OtherSideOfMove, tx);
    });
    assert_eq!(cursor(&rt), Some((rates, LineRef::new(Side::New, 8))), "a new line stays put");
    assert_eq!(rt.state.status, "This line is not part of a moved block");
}

#[test]
fn the_moved_label_is_a_pill_in_its_own_colours_and_a_press_follows_it() {
    use crate::theme::{Scheme, look};
    for (scheme, theme) in [
        (Scheme::Dark, Theme::Dark),
        (Scheme::Light, Theme::Light),
        (Scheme::Everforest, Theme::Everforest),
        (Scheme::GruvboxHard, Theme::GruvboxHard),
        (Scheme::GruvboxMedium, Theme::GruvboxMedium),
    ] {
        let mut a = moved_app();
        a.settings.theme = theme;
        let mut rt = Runtime::new(a, view, (1600.0, 1000.0), Env::new(scheme.is_dark(), 1.0));
        rt.frame(0.0);
        settle(&mut rt);
        let l = look(scheme);
        let (r, s) = (l.r, l.s);
        let lum = |c: rune::Color| c.luminance() + 0.05;
        let reads = lum(r.moved).max(lum(r.moved_bg)) / lum(r.moved).min(lum(r.moved_bg));
        assert!(reads >= 4.5, "{scheme:?}: the label on its pill: {reads:.2}");
        assert_eq!(r.moved_bg.a, 1.0, "{scheme:?}: the pill hides the code under it");
        assert_ne!(r.moved_bg, s.well, "{scheme:?}: the pill stands off the moved wash");

        let all = rt.elements();
        let chips: Vec<_> = all.iter().filter(|e| e.key.as_deref() == Some("moved") && !e.exiting).collect();
        assert_eq!(chips.len(), 2, "{scheme:?}: one label on each side of the move");
        for chip in &chips {
            assert_eq!((chip.fill, chip.radius, chip.role), (r.moved_bg, 4.0, "button"), "{scheme:?}");
            let line = &all[chip.parent.unwrap()];
            assert!(
                chip.rect[1] > line.rect[1] && chip.rect[1] + chip.rect[3] < line.rect[1] + line.rect[3],
                "{scheme:?}: inside its line, which it does not grow: {:?} in {:?}",
                chip.rect,
                line.rect
            );
            let inside = format!("{}/", chip.path);
            let said: Vec<_> = all.iter().filter(|e| e.path.starts_with(&inside) && e.text.is_some()).collect();
            assert_eq!(said.len(), 1, "{scheme:?}");
            assert!(said[0].text.as_deref().unwrap().starts_with("moved "), "{scheme:?}");
            assert_eq!(said[0].text_color, Some(r.moved), "{scheme:?}");
        }

        let to = chips
            .iter()
            .find(|c| c.label.as_deref().is_some_and(|l| l.contains("moved to")))
            .unwrap();
        let (x, y) = (to.rect[0] + to.rect[2] / 2.0, to.rect[1] + to.rect[3] / 2.0);
        let path = to.path.clone();
        rt.pointer_move(x, y);
        settle(&mut rt);
        let hovered = rt.elements().into_iter().find(|e| e.path == path).unwrap();
        assert!(hovered.hover > 0.0, "{scheme:?}: the pill answers the pointer");
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        settle(&mut rt);
        let rates = rt.state.index_of(RATES).unwrap();
        assert_eq!(
            rt.state.active().cursor().map(|(l, at)| (rt.state.global(l), at)),
            Some((rates, LineRef::new(Side::New, 3))),
            "{scheme:?}: a press goes to where the block went"
        );
    }
}

#[test]
fn a_filtered_reader_still_marks_the_block_and_follows_it_out() {
    let a = moved_app();
    let (total, rates) = (a.index_of(TOTAL).unwrap(), a.index_of(RATES).unwrap());
    let sub = crate::app::Subset::new(&a, vec![total], None);
    assert_eq!(sub.diff.files().len(), 1);
    assert_eq!(
        sub.diff.move_target(0, LineRef::new(Side::Old, 2)),
        Some((rates, LineRef::new(Side::New, 3))),
        "the other file by its place in the whole diff"
    );
}
