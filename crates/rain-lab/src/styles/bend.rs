//! Curves whose ends move without a rebuild: each is drawn once in a unit box (`Shape::view_box`)
//! that stretches to the element's rect, so binding a live rect carries the curve with its ends.
//! Stroke widths stay in points however the box stretches.

use rune::{Color, LineCap, Path, Shape};

/// The unit box's side in shape coordinates (curves flatten at 0.1 of these).
const U: f32 = 1000.0;

/// A cubic from one point to another: the rect it spans and its start, controls and end as
/// fractions of that rect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Span {
    pub rect: [f32; 4],
    pub unit: [[f32; 2]; 4],
}

fn corner(p: [f32; 2], r: [f32; 4]) -> [f32; 2] {
    [
        if p[0] > r[0] + r[2] / 2.0 { 1.0 } else { 0.0 },
        if p[1] > r[1] + r[3] / 2.0 { 1.0 } else { 0.0 },
    ]
}

/// From `a` to `b` in another column: leaves and lands level, an S between them.
pub(crate) fn across(a: [f32; 2], b: [f32; 2]) -> Span {
    let rect = [
        a[0].min(b[0]),
        a[1].min(b[1]),
        (a[0] - b[0]).abs().max(1.0),
        (a[1] - b[1]).abs().max(1.0),
    ];
    let (s, e) = (corner(a, rect), corner(b, rect));
    Span {
        rect,
        unit: [s, [0.5, s[1]], [0.5, e[1]], e],
    }
}

/// From `a` to `b` on one vertical line: bows `depth` out to the right (`right`) or left.
pub(crate) fn bow(a: [f32; 2], b: [f32; 2], right: bool, depth: f32) -> Span {
    let x = if right { a[0] } else { a[0] - depth };
    let rect = [x, a[1].min(b[1]), depth, (a[1] - b[1]).abs().max(1.0)];
    let side = if right { 0.0 } else { 1.0 };
    let (sy, ey) = (corner(a, rect)[1], corner(b, rect)[1]);
    Span {
        rect,
        unit: [[side, sy], [1.0 - side, sy], [1.0 - side, ey], [side, ey]],
    }
}

impl Span {
    /// This span drawn with `other`'s corners when it is level (one point tall, so either way
    /// up looks the same) and they run the same way across: two states of one line then share
    /// a shape and one can glide into the other.
    pub fn matching(self, other: &Span) -> Span {
        let level = self.rect[3] <= 1.0;
        let same_x = self.unit[0][0] == other.unit[0][0] && self.unit[3][0] == other.unit[3][0];
        if level && same_x { Span { unit: other.unit, ..self } } else { self }
    }

    /// The point at unit `u` inside `rect`.
    pub fn at(rect: [f32; 4], u: [f32; 2]) -> [f32; 2] {
        [rect[0] + u[0] * rect[2], rect[1] + u[1] * rect[3]]
    }

    /// The curve's point at `k` (0 to 1), in the canvas.
    pub fn point(&self, k: f32) -> [f32; 2] {
        let [p0, p1, p2, p3] = self.unit;
        let m = 1.0 - k;
        let w = [m * m * m, 3.0 * m * m * k, 3.0 * m * k * k, k * k * k];
        let u = [0, 1].map(|i| w[0] * p0[i] + w[1] * p1[i] + w[2] * p2[i] + w[3] * p3[i]);
        Span::at(self.rect, u)
    }

    /// `n` + 1 points along it, for a pulse.
    pub fn points(&self, n: usize) -> Vec<[f32; 2]> {
        (0..=n).map(|i| self.point(i as f32 / n as f32)).collect()
    }
}

/// A curve as a stroke in a unit box, for a canvas sized (or live-rected) to its rect.
pub(crate) fn unit_shape(unit: [[f32; 2]; 4], width: f32, color: Color) -> Shape {
    let s = |p: [f32; 2]| [p[0] * U, p[1] * U];
    let path = Path::new().move_to(s(unit[0])).cubic_to(s(unit[1]), s(unit[2]), s(unit[3]));
    Shape::new().stroke_path(&path, width, LineCap::Round, color).view_box(U, U)
}

/// About how wide `chars` characters of 12-point UI text set.
pub(crate) fn text_w(s: &str, per: f32) -> f32 {
    s.chars().count() as f32 * per
}
