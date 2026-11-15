//! The overview ruler at the diff's right edge: the diff's scrollbar, with marks for hunks,
//! file starts, threads, notes, find hits and the cursor line.

use rain_diffview::DiffView;
use rune::{El, LineCap, Shape, Spring, canvas, el, memo};
use rune_kit::Kit;

use super::notes::note_purple;
use crate::app::{App, RulerState};

const W: f32 = RulerState::WIDTH;

/// The strip beside the diff, the full height of its viewport; nothing when the diff has no rows.
pub(super) fn ruler(s: &App, k: &Kit) -> El {
    let d = s.active();
    if d.row_count() == 0 {
        return el();
    }
    let track = s.ruler.track.get();
    let [offset, max, ..] = s.ruler.live.get();
    let pct = if max > 0.0 {
        (offset / max * 100.0).clamp(0.0, 100.0).round()
    } else {
        0.0
    };
    let marks = s.ruler_marks(track);
    let kc = k.clone();
    let purple = note_purple(k);
    let painted = memo(
        "ruler-marks",
        (s.ruler.builds.get(), s.seen_scheme.get(), track.round() as i32),
        move || {
            let mut shape = Shape::new();
            for m in marks.iter() {
                let [x0, y0, x1, y1] = m.rect();
                let c = m.color(&kc, purple);
                shape = if m.outlined() {
                    let [x0, y0, x1, y1] = [x0 + 0.5, y0 + 0.5, x1 - 0.5, y1 - 0.5];
                    shape.stroke_with(vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]], true, 1.0, LineCap::Butt, c)
                } else {
                    shape.fill(vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]], c)
                };
            }
            canvas(shape).w(W).h(track.max(0.0)).a11y_hidden()
        },
    );
    let mut strip = el()
        .key("ruler")
        .role("scrollbar")
        .label("Diff position")
        .range_value(pct as f64, 0.0, 100.0)
        .w(W)
        .h_full()
        .min_h(0.0)
        .fill(k.c.bg_base)
        .on_layout_tx(|s: &mut App, r: [f32; 4], tx| s.set_ruler_track(r[3], tx))
        .on_drag(|s: &mut App, d, tx| s.ruler_drag(d, tx))
        .on_pointer_move_local(|s: &mut App, p, tx| s.ruler_hover(Some(p.y), tx))
        .on_hover_change(|s: &mut App, inside, tx| {
            if !inside {
                s.ruler_hover(None, tx);
            }
        })
        .child(painted.absolute(0.0, 0.0));
    if let Some((f, at)) = d.cursor()
        && let Some(row) = d.row_of_line(f, at)
        && track > 2.0
    {
        let y = (d.row_y(row as usize) / d.rows_height().max(1.0) * track).floor().min(track - 2.0);
        strip = strip.child(
            el().key("ruler-cursor")
                .absolute(0.0, y)
                .w(W)
                .h(2.0)
                .fill(k.c.text_base)
                .a11y_hidden(),
        );
    }
    strip
        .child(
            el().key("ruler-thumb")
                .absolute(0.0, 0.0)
                .live_rect(&s.ruler.thumb)
                .fill(k.alpha(k.c.text_base, 0.09))
                .radius(3.0)
                .a11y_hidden()
                .child(
                    el().w_full()
                        .h_full()
                        .radius(3.0)
                        .fill(k.alpha(k.c.text_base, 0.07))
                        .live_opacity(s.ruler.hot.smooth(Spring::new(0.12, 0.0))),
                ),
        )
        .child(
            el().key("ruler-line")
                .pin(Some(0.0), Some(0.0), None, Some(0.0))
                .w(1.0)
                .fill(k.c.border_weak)
                .a11y_hidden(),
        )
}

/// Hooks the diff view's scroll offset to the ruler's thumb, with no view run per frame.
pub(super) fn wire(v: DiffView<App>, s: &App) -> DiffView<App> {
    v.scroll_live(s.ruler.live.clone()).on_scroll_offset(s.ruler.follower())
}
