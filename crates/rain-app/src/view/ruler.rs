//! The diff's scrollbar at its right edge: a plain track and thumb.

use rain_diffview::DiffView;
use rune::{El, Spring, el};
use rune_kit::Kit;

use crate::app::{App, RulerState};

const W: f32 = RulerState::WIDTH;

/// The strip beside the diff, the full height of its viewport; nothing when the diff has no rows.
pub(super) fn ruler(s: &App, k: &Kit) -> El {
    let d = s.active();
    if d.row_count() == 0 {
        return el();
    }
    let [offset, max, ..] = s.ruler.live.get();
    let pct = if max > 0.0 {
        (offset / max * 100.0).clamp(0.0, 100.0).round()
    } else {
        0.0
    };
    el().key("ruler")
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
