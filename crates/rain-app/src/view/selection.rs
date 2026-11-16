//! What floats over a selection in the diff or the Overview: the toolbar with Comment, Ask and
//! Copy, and the right-click menu on diff rows and Overview sections.

use rain_comments::ComposerMode;
use rain_core::Side;
use rain_core::command::Command;
use rain_diffview::{Layout, LineRef};
use rune::{Cx, El, PointerPress, Tx, el, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, Kit, MenuItem, context_menu, popover};

use super::composer::{CARD_H, CARD_W};
use crate::app::{App, ComposeMode};

/// Between the selection and what floats beside it.
const GAP: f32 = 6.0;
/// The toolbar's size before it has been laid out.
const BAR: [f32; 2] = [239.0, 28.0];
/// A diff row's line, without the widgets under it.
const LINE_H: f32 = 20.0;
/// The top of the diff column a file header (sticky or not) may cover.
const HEAD_H: f32 = 44.0;

/// The toolbar over the selection and the right-click menu, for the view's floating tail.
pub(super) fn toolbar(s: &App, k: &Kit, cx: &mut Cx<App>) -> Vec<El> {
    let mut layer = Vec::new();
    if let Some((id, at)) = s.brief.menu.clone() {
        let trigger = section_menu(s, k, id, at, &mut layer);
        layer.insert(0, trigger);
        return layer;
    }
    if s.overview_toolbar_shown() {
        layer.extend(overview_bar(s, k, cx));
        return layer;
    }
    match s.composer_ui.sel.menu {
        Some(at) => {
            let trigger = menu(s, k, cx, at, &mut layer);
            layer.insert(0, trigger);
        }
        None if s.selection_toolbar_shown() => layer.extend(bar(s, k, cx)),
        None => {}
    }
    layer
}

/// What the toolbar floats by, in window points: the first and last line boxes of the text
/// selected, or for selected lines the right end of the last line's row; and the diff column.
/// `None` when nothing selected is on screen.
struct Boxes {
    first: [f32; 4],
    last: [f32; 4],
    column: [f32; 4],
    lines: bool,
}

fn selection_boxes(s: &App, cx: &Cx<App>, width: f32) -> Option<Boxes> {
    let column = cx.window_layout("diff-col")?;
    let a = s.active();
    let (first, last, lines) = if a.span_selection().is_some() {
        let rects = cx.selection_rects();
        (*rects.first()?, *rects.last()?, false)
    } else {
        let sel = a.selection()?;
        let rows = [sel.start, sel.end].map(|at| a.row_of_line(sel.file, at));
        let last = rows.iter().flatten().max()?;
        let r = cx.window_layout(&a.row_key(*last as usize)?)?;
        let right = [r[0] + r[2] - width - 16.0, r[1], width, LINE_H];
        (right, right, true)
    };
    let shown = last[1] + last[3] > column[1] + HEAD_H && first[1] < column[1] + column[3];
    shown.then_some(Boxes {
        first,
        last,
        column,
        lines,
    })
}

/// Where the toolbar of `size` goes: just above the first box (selected lines: just below the
/// last), on the other side when the column has no room there, kept inside the column.
fn place_bar(b: &Boxes, size: [f32; 2]) -> (f32, f32) {
    let [cx0, cy0, cw, ch] = b.column;
    let (first, last) = (b.first, b.last);
    let above = first[1] - size[1] - GAP;
    let under = last[1] + last[3] + GAP;
    let y = match b.lines {
        false if above >= cy0 + HEAD_H => above,
        false => under,
        true if under + size[1] <= cy0 + ch - 4.0 => under,
        true => above,
    };
    let y = y.min(cy0 + ch - size[1] - 4.0).max(cy0 + 4.0);
    let x = first[0].min(cx0 + cw - size[0] - 8.0).max(cx0 + 8.0);
    (x, y)
}

/// Where a composer card opened from the selection goes: under the last box, above the first
/// when the window has no room below.
fn place_card(first: [f32; 4], last: [f32; 4], window: [f32; 2]) -> (f32, f32) {
    let below = last[1] + last[3] + GAP;
    let y = if below + CARD_H <= window[1] - 34.0 {
        below
    } else {
        (first[1] - CARD_H - GAP).max(super::TOOLBAR_HEIGHT + 4.0)
    };
    let x = (first[0] - 16.0).min(window[0] - CARD_W - 8.0).max(8.0);
    (x, y)
}

fn bar(s: &App, k: &Kit, cx: &Cx<App>) -> Option<El> {
    let size = cx.layout_size("sel-toolbar").unwrap_or(BAR);
    let boxes = selection_boxes(s, cx, size[0])?;
    let (x, y) = place_bar(&boxes, size);
    let window = cx.layout_size("app").unwrap_or([s.window_width, 1080.0]);
    let card_at = place_card(boxes.first, boxes.last, window);
    let last = boxes.last;
    s.composer_ui
        .sel
        .seen
        .set(Some([card_at.0, card_at.1, last[0] + last[2] / 2.0, last[1] + last[3] / 2.0]));
    let button = |label: &'static str, hint: String| {
        let glyph = glyph_of(label);
        Button::new(label)
            .help(format!("{label}  {hint}"))
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Small)
            .label_with(move |font, ink| {
                row()
                    .gap(6.0)
                    .items_center()
                    .child(super::icons::draw(glyph, 14.0, ink))
                    .child(text(label).font(font).color(ink))
            })
    };
    let comment = button("Comment", chord(Command::Comment))
        .on_press(move |s: &mut App| s.compose_on_selection(ComposerMode::Comment, card_at))
        .build(k)
        .key("sel-comment");
    let ask = button("Ask", chord(Command::Ask))
        .on_press(move |s: &mut App| s.compose_on_selection(ComposerMode::Question, card_at))
        .build(k)
        .key("sel-ask");
    let copy = button("Copy", "Ctrl+C".into())
        .on_press_tx(|s: &mut App, tx: &mut Tx| s.copy_selection(tx))
        .build(k)
        .key("sel-copy");
    let pill = row().gap(2.0).padding(4.0).items_center().child(comment).child(ask).child(copy);
    Some(
        popover::raised(k, pill, k.radius.xl)
            .key("sel-toolbar")
            .role("toolbar")
            .label("Selection")
            .keeps_text_selection()
            .absolute(x, y),
    )
}

/// The toolbar over text selected in an Overview section: Comment and Ask open the section's
/// composer quoting it, Copy copies it.
fn overview_bar(s: &App, k: &Kit, cx: &Cx<App>) -> Option<El> {
    let (id, _) = s.brief.selection.clone()?;
    let column = cx.window_layout("overview")?;
    let rects = cx.selection_rects();
    let (first, last) = (*rects.first()?, *rects.last()?);
    if last[1] + last[3] <= column[1] || first[1] >= column[1] + column[3] {
        return None;
    }
    let size = cx.layout_size("sel-toolbar").unwrap_or(BAR);
    let boxes = Boxes {
        first,
        last,
        column,
        lines: false,
    };
    let (x, y) = place_bar(&boxes, size);
    let button = |label: &'static str, hint: &'static str| {
        let glyph = glyph_of(label);
        Button::new(label)
            .help(format!("{label}  {hint}"))
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Small)
            .label_with(move |font, ink| {
                row()
                    .gap(6.0)
                    .items_center()
                    .child(super::icons::draw(glyph, 14.0, ink))
                    .child(text(label).font(font).color(ink))
            })
    };
    let (c_id, a_id, y_id) = (id.clone(), id.clone(), id);
    let pill = row()
        .gap(2.0)
        .padding(4.0)
        .items_center()
        .child(
            button("Comment", "c")
                .on_press(move |s: &mut App| s.compose_on_section_selection(&c_id, ComposeMode::Comment))
                .build(k)
                .key("sel-comment"),
        )
        .child(
            button("Ask", "?")
                .on_press(move |s: &mut App| s.compose_on_section_selection(&a_id, ComposeMode::Question))
                .build(k)
                .key("sel-ask"),
        )
        .child(
            button("Copy", "Ctrl+C")
                .on_press_tx(move |s: &mut App, tx: &mut Tx| s.copy_section_selection(&y_id, tx))
                .build(k)
                .key("sel-copy"),
        );
    Some(
        popover::raised(k, pill, k.radius.xl)
            .key("sel-toolbar")
            .role("toolbar")
            .label("Selection")
            .keeps_text_selection()
            .absolute(x, y),
    )
}

/// The right-click menu on Overview section `id`: Comment, Ask and Copy on the text selected in
/// it, else on the section.
fn section_menu(s: &App, k: &Kit, id: String, at: (f32, f32), layer: &mut Vec<El>) -> El {
    let selected = s.brief.selection.as_ref().is_some_and(|(x, _)| *x == id);
    let what = if selected { "selection" } else { "section" };
    let (c_id, a_id, y_id) = (id.clone(), id.clone(), id);
    context_menu("section-menu", el(), Some(PointerPress { x: at.0, y: at.1 }))
        .min_width(220.0)
        .on_close(|s: &mut App| s.close_section_menu())
        .item(
            MenuItem::new(format!("Comment on {what}"))
                .shortcut("c")
                .on_select(move |s: &mut App| s.compose_on_section_selection(&c_id, ComposeMode::Comment)),
        )
        .item(
            MenuItem::new(format!("Ask about {what}"))
                .shortcut("?")
                .on_select(move |s: &mut App| s.compose_on_section_selection(&a_id, ComposeMode::Question)),
        )
        .item(
            MenuItem::new(format!("Copy {what}"))
                .shortcut("Ctrl+C")
                .on_select_tx(move |s: &mut App, tx: &mut Tx| s.copy_section_selection(&y_id, tx)),
        )
        .build(k, layer)
}

/// The selection toolbar's icon for each of its buttons.
fn glyph_of(label: &str) -> &'static rune::Icon {
    match label {
        "Comment" => &super::icons::CHAT,
        "Ask" => &super::icons::QUESTION,
        _ => &super::icons::COPY,
    }
}

fn chord(c: Command) -> String {
    c.chord().map(|k| k.display()).unwrap_or_default()
}

/// The right-click menu at `at`: Comment, Ask and Copy on the selection (else the clicked line),
/// then the file's keys. Every item runs what its key runs.
fn menu(s: &App, k: &Kit, cx: &Cx<App>, at: (f32, f32), layer: &mut Vec<El>) -> El {
    let line = clicked_line(s, cx, at);
    let a = s.active();
    let selected = a.span_selection().is_some() || a.selection().is_some();
    let what = match (selected, line) {
        (true, _) => "selection".to_owned(),
        (false, Some((_, l))) => format!("line {}", l.line),
        (false, None) => "line".to_owned(),
    };
    let run = |label: String, cmd: Command| {
        MenuItem::new(label)
            .shortcut(chord(cmd))
            .on_select_tx(move |s: &mut App, tx: &mut Tx| s.diff_menu_run(line, cmd, tx))
    };
    context_menu("diff-menu", el(), Some(PointerPress { x: at.0, y: at.1 }))
        .min_width(220.0)
        .on_close(|s: &mut App| s.close_diff_menu())
        .item(run(format!("Comment on {what}"), Command::Comment))
        .item(run(format!("Ask about {what}"), Command::Ask))
        .item(
            MenuItem::new(if selected {
                "Copy selection".to_owned()
            } else {
                format!("Copy {what}")
            })
            .shortcut("Ctrl+C")
            .on_select_tx(move |s: &mut App, tx: &mut Tx| s.diff_menu_copy(line, tx)),
        )
        .separator()
        .item(run("Copy location".into(), Command::CopyLocation))
        .item(run("Open in editor".into(), Command::OpenInEditor))
        .item(run("Toggle viewed".into(), Command::ToggleViewed))
        .item(run("Whole file".into(), Command::ToggleWholeFile))
        .build(k, layer)
}

/// The line under window point `at`: the row it falls in among those on screen, on the side
/// of a split row it is over.
fn clicked_line(s: &App, cx: &Cx<App>, at: (f32, f32)) -> Option<(usize, LineRef)> {
    let a = s.active();
    let column = cx.window_layout("diff-col")?;
    let (i, key) = a.visible_rows().find_map(|i| {
        let key = a.row_key(i)?;
        let r = cx.window_layout(&key)?;
        (at.1 >= r[1] && at.1 < r[1] + r[3]).then_some((i, key))
    })?;
    let (f, kind, n) = parse_row_key(&key)?;
    let right = a.layout(f) == Layout::Split && at.0 > column[0] + column[2] / 2.0;
    let unified = a.layout(f) != Layout::Split;
    let lines = &a.files().get(f)?.lines;
    let on_new = |pick: &dyn Fn(&rain_core::LineRec) -> bool| lines.iter().find(|l| pick(l)).map(|l| (f, LineRef::new(Side::New, l.new)));
    match kind {
        'n' => Some((f, LineRef::new(Side::New, n))),
        'c' if right || unified => on_new(&|l| l.kind == rain_core::LineKind::Context && l.old == n),
        'o' if right => {
            on_new(&|l| l.kind == rain_core::LineKind::Added && a.row_of_line(f, LineRef::new(Side::New, l.new)) == Some(i as u32))
                .or(Some((f, LineRef::new(Side::Old, n))))
        }
        _ => Some((f, LineRef::new(Side::Old, n))),
    }
}

/// `df3.n12` → file 3, an added line, 12: the diff's row keys for lines (`c` context by its old
/// number, `o` removed, `n` added).
fn parse_row_key(key: &str) -> Option<(usize, char, u32)> {
    let (f, line) = key.strip_prefix("df")?.split_once('.')?;
    let kind = line.chars().next().filter(|c| matches!(c, 'c' | 'o' | 'n'))?;
    Some((f.parse().ok()?, kind, line[1..].parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_keys_parse_to_file_kind_and_line() {
        assert_eq!(parse_row_key("df3.n12"), Some((3, 'n', 12)));
        assert_eq!(parse_row_key("df0.c7"), Some((0, 'c', 7)));
        assert_eq!(parse_row_key("df0.g1"), None);
        assert_eq!(parse_row_key("df0.note"), None);
        assert_eq!(parse_row_key("df2"), None);
    }

    #[test]
    fn the_toolbar_sits_above_the_selection_unless_the_column_has_no_room() {
        let column = [300.0, 100.0, 1000.0, 600.0];
        let size = [280.0, 36.0];
        let at = |first, last, lines| Boxes {
            first,
            last,
            column,
            lines,
        };
        let (x, y) = place_bar(&at([400.0, 300.0, 200.0, 20.0], [400.0, 340.0, 80.0, 20.0], false), size);
        assert_eq!((x, y), (400.0, 300.0 - 36.0 - GAP));
        let (_, y) = place_bar(&at([400.0, 110.0, 200.0, 20.0], [400.0, 150.0, 80.0, 20.0], false), size);
        assert_eq!(y, 150.0 + 20.0 + GAP, "below the last line at the column's top");
        let (x, _) = place_bar(&at([1250.0, 300.0, 40.0, 20.0], [1250.0, 300.0, 40.0, 20.0], false), size);
        assert_eq!(x, 300.0 + 1000.0 - 280.0 - 8.0, "kept inside the column's right edge");
        let row = [1004.0, 300.0, 280.0, 20.0];
        assert_eq!(
            place_bar(&at(row, row, true), size).1,
            320.0 + GAP,
            "selected lines: under the last"
        );
        let low = [1004.0, 680.0, 280.0, 20.0];
        assert_eq!(place_bar(&at(low, low, true), size).1, 680.0 - 36.0 - GAP, "above it at the bottom");
    }

    #[test]
    fn the_card_goes_under_the_selection_and_flips_above_near_the_bottom() {
        let window = [1366.0, 768.0];
        let (x, y) = place_card([500.0, 200.0, 100.0, 20.0], [500.0, 240.0, 60.0, 20.0], window);
        assert_eq!((x, y), (484.0, 260.0 + GAP));
        let (_, y) = place_card([500.0, 600.0, 100.0, 20.0], [500.0, 640.0, 60.0, 20.0], window);
        assert_eq!(y, 600.0 - CARD_H - GAP);
        let (x, _) = place_card([1300.0, 200.0, 40.0, 20.0], [1300.0, 200.0, 40.0, 20.0], window);
        assert_eq!(x, 1366.0 - CARD_W - 8.0, "inside the window at 1366");
    }
}
