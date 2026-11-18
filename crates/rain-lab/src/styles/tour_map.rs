//! The tour's figure, a line map: every request path is a coloured line on a 45°/90° grid, the
//! components it calls are its stations, and a station two lines share is an interchange: code
//! both flows run through. Stations sit in their tier's band (frontend, backend, database), a
//! page is a compact block holding its components, and packets labelled with what each call
//! carries ride the stretches.

use std::collections::BTreeMap;

use rain_diagram::DiagramStyle;
use rune::tok::TextStyle;
use rune::{Color, El, LineCap, Shape, canvas, col, el, row, text};

use super::paint::{self, LABEL, MICRO, MONO, SMALL};
use crate::motion::{self, Highlight, Pulse, Rig, Track, WriteOn};
use crate::schema::Status;
use crate::schema::trace::{Fragment, Trace, TracePath, TraceStep};

const LINE_W: f32 = 6.0;
/// How far apart two lines sharing a stretch run.
const LINE_GAP: f32 = 7.0;
const CORNER: f32 = 18.0;
const STATION: f32 = 14.0;
const INTERCHANGE: f32 = 22.0;
const PACKET: f32 = 12.0;
/// A page block's padding and name line, and the height of a component inside it.
const BOX_PAD: f32 = 6.0;
const BOX_TITLE: f32 = 14.0;
const CHIP_H: f32 = 24.0;
/// Space between two districts, and the band their names take.
const DISTRICT_GAP: f32 = 14.0;
const DISTRICT_LABEL: f32 = 24.0;

pub(crate) struct Line {
    pub id: String,
    pub name: String,
    pub colour: Color,
}

/// How a station is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Mark {
    /// A dot with its name beside it.
    Dot,
    /// A component inside a page box, its rect; the station sits on its exit side.
    Chip([f32; 4]),
    /// A page box's own station, on the box's exit side.
    Port,
}

pub(crate) struct Station {
    pub id: String,
    pub label: String,
    pub at: [f32; 2],
    /// Indexes into [`Map::lines`].
    pub lines: Vec<usize>,
    /// Every step through it is new.
    pub new: bool,
    pub label_at: [f32; 2],
    /// What hovering it lights: itself, its lines and their stations.
    pub lights: Vec<String>,
    pub mark: Mark,
}

impl Station {
    pub fn interchange(&self) -> bool {
        self.lines.len() > 1
    }

    pub fn diameter(&self) -> f32 {
        match self.mark {
            Mark::Dot if self.interchange() => INTERCHANGE,
            Mark::Dot => STATION,
            _ => 10.0,
        }
    }
}

/// A layer's band behind its stations.
pub(crate) struct District {
    pub label: String,
    pub rect: [f32; 4],
    /// Where its name goes: top left, or top right when the bands are stacked.
    pub label_at: [f32; 2],
}

/// A page drawn as a window holding its components.
pub(crate) struct PageBox {
    pub id: String,
    pub label: String,
    pub rect: [f32; 4],
    pub new: bool,
}

/// One step of one path: the stretch of line between two stations.
pub(crate) struct Seg {
    pub line: usize,
    pub from: String,
    pub to: String,
    pub call: String,
    pub status: Status,
    pub fragment: Option<Fragment>,
    pub points: Vec<[f32; 2]>,
    /// What the call carries, as a packet's label.
    pub packet: Option<String>,
}

impl Seg {
    pub fn mid(&self) -> [f32; 2] {
        motion::along(&self.points, 0.5)
    }
}

/// The map laid out in a box `size` big.
pub(crate) struct Map {
    pub size: [f32; 2],
    pub lines: Vec<Line>,
    pub stations: Vec<Station>,
    pub segs: Vec<Seg>,
    pub districts: Vec<District>,
    pub boxes: Vec<PageBox>,
    pub font: TextStyle,
}

impl Map {
    pub fn station(&self, id: &str) -> Option<&Station> {
        self.stations.iter().find(|s| s.id == id)
    }

    /// What a station takes up: its dot and name, its component, or its page box.
    fn extent(&self, s: &Station) -> [f32; 4] {
        match s.mark {
            Mark::Dot => {
                let w = label_w(&s.label, self.font);
                let lo = [s.at[0].min(s.label_at[0]), s.at[1].min(s.label_at[1])];
                let hi = [s.at[0].max(s.label_at[0] + w), s.at[1].max(s.label_at[1] + 16.0)];
                [lo[0], lo[1], hi[0] - lo[0], hi[1] - lo[1]]
            }
            Mark::Chip(r) => r,
            Mark::Port => self
                .boxes
                .iter()
                .find(|b| b.id == s.id)
                .map_or([s.at[0], s.at[1], 0.0, 0.0], |b| b.rect),
        }
    }

    /// The box around `ids`' stations and their names.
    pub fn bounds(&self, ids: &[String]) -> Option<[f32; 4]> {
        let mut b: Option<[f32; 4]> = None;
        for s in self.stations.iter().filter(|s| ids.contains(&s.id)) {
            let r = self.extent(s);
            b = Some(match b {
                None => [r[0], r[1], r[0] + r[2], r[1] + r[3]],
                Some(o) => [o[0].min(r[0]), o[1].min(r[1]), o[2].max(r[0] + r[2]), o[3].max(r[1] + r[3])],
            });
        }
        b.map(|r| [r[0], r[1], r[2] - r[0], r[3] - r[1]])
    }
}

pub(crate) fn seg_id(i: usize) -> String {
    format!("seg:{i}")
}

pub(crate) fn line_id(l: &Line) -> String {
    format!("line:{}", l.id)
}

fn label_w(s: &str, font: TextStyle) -> f32 {
    rain_diagram::design::measure(s, font.size) * 0.92
}

fn mono_w(s: &str, font: TextStyle) -> f32 {
    s.chars().count() as f32 * font.size * 0.6
}

/// A line's colour: the theme's key hues, so it reads in both themes and never means a status.
fn line_colour(ink: &DiagramStyle, i: usize) -> Color {
    ink.keys[[2, 1, 0, 3, 4][i % 5]]
}

/// A tier's name as its district shows it.
fn district_name(tier: &str) -> String {
    tier.to_owned()
}

/// The ways from `p` to `q` on the 45°/90° grid: straight, diagonal, straight with the
/// straight split evenly, all at the start, or all at the end.
fn octilinear(p: [f32; 2], q: [f32; 2]) -> Vec<Vec<[f32; 2]>> {
    let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
    let (ax, ay) = (dx.abs(), dy.abs());
    if ax < 0.5 || ay < 0.5 || (ax - ay).abs() < 0.5 {
        return vec![vec![p, q]];
    }
    let corner = |a: f32| -> Vec<[f32; 2]> {
        if ax > ay {
            let (l1, l2) = ((ax - ay) * a * dx.signum(), (ax - ay) * (1.0 - a) * dx.signum());
            vec![p, [p[0] + l1, p[1]], [q[0] - l2, q[1]], q]
        } else {
            let (l1, l2) = ((ay - ax) * a * dy.signum(), (ay - ax) * (1.0 - a) * dy.signum());
            vec![p, [p[0], p[1] + l1], [q[0], q[1] - l2], q]
        }
    };
    [0.5, 0.0, 1.0]
        .into_iter()
        .map(|a| {
            let mut v = corner(a);
            v.dedup_by(|x, y| (x[0] - y[0]).abs() < 0.5 && (x[1] - y[1]).abs() < 0.5);
            v
        })
        .collect()
}

/// Points every few points along `pts`.
fn sample(pts: &[[f32; 2]], every: f32) -> Vec<[f32; 2]> {
    let len: f32 = pts.windows(2).map(|w| dist(w[0], w[1])).sum();
    let n = (len / every).ceil().max(1.0) as usize;
    (0..=n).map(|k| motion::along(pts, k as f32 / n as f32)).collect()
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn inside(p: [f32; 2], r: [f32; 4]) -> bool {
    p[0] > r[0] && p[0] < r[0] + r[2] && p[1] > r[1] && p[1] < r[1] + r[3]
}

/// `pts` moved `d` points to the left of its direction, corners mitred.
fn offset(pts: &[[f32; 2]], d: f32) -> Vec<[f32; 2]> {
    if d == 0.0 || pts.len() < 2 {
        return pts.to_vec();
    }
    let normal = |a: [f32; 2], b: [f32; 2]| {
        let (x, y) = (b[0] - a[0], b[1] - a[1]);
        let l = (x * x + y * y).sqrt().max(1e-4);
        [-y / l, x / l]
    };
    (0..pts.len())
        .map(|i| {
            let before = (i > 0).then(|| normal(pts[i - 1], pts[i]));
            let after = (i + 1 < pts.len()).then(|| normal(pts[i], pts[i + 1]));
            let n = match (before, after) {
                (Some(a), Some(b)) => {
                    let m = [a[0] + b[0], a[1] + b[1]];
                    let l = (m[0] * m[0] + m[1] * m[1]).sqrt().max(1e-4);
                    let m = [m[0] / l, m[1] / l];
                    let k = 1.0 / (m[0] * a[0] + m[1] * a[1]).max(0.5);
                    [m[0] * k, m[1] * k]
                }
                (Some(a), None) | (None, Some(a)) => a,
                (None, None) => [0.0; 2],
            };
            [pts[i][0] + n[0] * d, pts[i][1] + n[1] * d]
        })
        .collect()
}

/// A path the change adds whole: none of its calls was made before.
pub(crate) fn fresh(p: &TracePath) -> bool {
    p.steps.iter().all(|s| matches!(s.status, Status::Added | Status::Changed)) && p.steps.iter().any(|s| s.status == Status::Added)
}

/// What a call carries, as a packet's label: `VoucherDto {accountId, amount}`.
pub(crate) fn packet_label(a: &Trace, carries: &[String]) -> Option<String> {
    let parts: Vec<String> = carries
        .iter()
        .map(|id| match a.entity(id) {
            Some(e) if !e.fields.is_empty() => format!("{} {{{}}}", e.label, e.fields.join(", ")),
            Some(e) => e.label.clone(),
            None => id.clone(),
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(" + "))
}

/// Lays `a`'s paths out as a metro map in a `size` box. Layers are districts along the box's
/// longer side, in calling order; inside a district, stations step along by the calls between
/// them; a component with a `parent` sits in its page's box.
pub(crate) fn map(a: &Trace, ink: &DiagramStyle, size: [f32; 2], font: TextStyle) -> Map {
    let lines: Vec<Line> = a
        .paths
        .iter()
        .enumerate()
        .map(|(i, p)| Line {
            id: p.id.clone(),
            name: p.name.clone(),
            colour: line_colour(ink, i),
        })
        .collect();

    // Stations in the order the paths first reach them, with the path that reached them first.
    let mut order: Vec<(String, usize)> = Vec::new();
    for (pi, p) in a.paths.iter().enumerate() {
        for s in &p.steps {
            for id in [&s.from, &s.to] {
                if !order.iter().any(|(o, _)| o == id) {
                    order.push((id.clone(), pi));
                }
            }
        }
    }
    let n = order.len();
    let idx = |id: &str| order.iter().position(|(o, _)| o == id).unwrap_or(0);
    let steps: Vec<(usize, &crate::schema::trace::TraceStep)> = a
        .paths
        .iter()
        .enumerate()
        .flat_map(|(pi, p)| p.steps.iter().map(move |s| (pi, s)))
        .collect();
    let label = |i: usize| a.entity(&order[i].0).map_or_else(|| order[i].0.clone(), |e| e.label.clone());

    // Nesting: a component whose parent is on the map sits in the parent's page box.
    let parent: Vec<Option<usize>> = (0..n)
        .map(|i| {
            a.entity(&order[i].0)
                .and_then(|e| e.parent.as_deref())
                .and_then(|p| order.iter().position(|(o, _)| o == p))
                .filter(|&p| p != i)
        })
        .collect();
    let anchor = |i: usize| parent[i].unwrap_or(i);
    let container: Vec<bool> = (0..n).map(|i| parent.contains(&Some(i))).collect();

    let mut rank = vec![0usize; n];
    for _ in 0..n {
        for (_, s) in &steps {
            let (f, t) = (idx(&s.from), idx(&s.to));
            if f != t && rank[t] < rank[f] + 1 {
                rank[t] = rank[f] + 1;
            }
        }
    }

    // Districts: one per tier (frontend, backend, database), in the order their stations are
    // called.
    let layer: Vec<String> = (0..n)
        .map(|i| a.entity(&order[anchor(i)].0).map_or("Other", |e| paint::tier(&e.layer)).to_owned())
        .collect();
    let mut layers: Vec<(String, f32, usize)> = Vec::new();
    for i in 0..n {
        if !layers.iter().any(|l| l.0 == layer[i]) {
            let members: Vec<usize> = (0..n).filter(|&j| layer[j] == layer[i]).collect();
            let mean = members.iter().map(|&j| rank[j] as f32).sum::<f32>() / members.len() as f32;
            layers.push((layer[i].clone(), mean, i));
        }
    }
    layers.sort_by(|x, y| x.1.total_cmp(&y.1).then(x.2.cmp(&y.2)));
    let district = |i: usize| layers.iter().position(|l| l.0 == layer[i]).unwrap_or(0);

    // Columns: inside a district, a station steps along by the calls from its neighbours there.
    let mut local = vec![0usize; n];
    for _ in 0..n {
        for (_, s) in &steps {
            let (f, t) = (anchor(idx(&s.from)), anchor(idx(&s.to)));
            if f != t && layer[f] == layer[t] && local[t] < local[f] + 1 {
                local[t] = local[f] + 1;
            }
        }
    }
    for i in 0..n {
        local[i] = local[anchor(i)];
    }
    let mut cols: Vec<(usize, usize)> = (0..n).map(|i| (district(i), local[i])).collect();
    cols.sort_unstable();
    cols.dedup();
    let column: Vec<usize> = (0..n)
        .map(|i| cols.iter().position(|c| *c == (district(i), local[i])).unwrap_or(0))
        .collect();

    // Across: each station sits at the mean of the stations calling it, entry points by path;
    // then a column's stations are kept a track apart.
    let mut across = vec![0.0f32; n];
    for c in 0..cols.len() {
        let mut here: Vec<usize> = (0..n).filter(|&i| column[i] == c).collect();
        for &i in &here {
            let preds: Vec<f32> = steps
                .iter()
                .filter(|(_, s)| idx(&s.to) == i && column[idx(&s.from)] < c)
                .map(|(_, s)| across[idx(&s.from)])
                .collect();
            across[i] = if preds.is_empty() {
                order[i].1 as f32
            } else {
                preds.iter().sum::<f32>() / preds.len() as f32
            };
        }
        here.sort_by(|x, y| across[*x].total_cmp(&across[*y]).then(x.cmp(y)));
        for w in 1..here.len() {
            let (p, q) = (here[w - 1], here[w]);
            if across[q] < across[p] + 1.0 {
                across[q] = across[p] + 1.0;
            }
        }
    }
    let lo = across.iter().copied().fold(f32::INFINITY, f32::min);
    for v in &mut across {
        *v -= lo;
    }
    let tracks = across.iter().copied().fold(0.0, f32::max);

    // Along: a column holding a page box is as long as the box; the others share the rest.
    let tall = size[1] > size[0] * 1.1;
    let chip_w = |i: usize| label_w(&label(i), font) + 22.0;
    let has_box = container.iter().any(|c| *c);
    let box_along: Vec<f32> = (0..cols.len())
        .map(|c| {
            let kids: Vec<usize> = (0..n).filter(|&i| column[i] == c && parent[i].is_some()).collect();
            if kids.is_empty() {
                0.0
            } else if tall {
                2.0 * BOX_PAD + BOX_TITLE + CHIP_H
            } else {
                kids.iter().map(|&i| chip_w(i)).fold(80.0, f32::max) + 2.0 * BOX_PAD
            }
        })
        .collect();
    let pad = 16.0;
    let along_total = if tall { size[1] } else { size[0] } - 2.0 * pad;
    let gaps = DISTRICT_GAP * layers.len().saturating_sub(1) as f32;
    let fixed: f32 = box_along.iter().sum();
    let normal = box_along.iter().filter(|b| **b == 0.0).count().max(1) as f32;
    let per = ((along_total - fixed - gaps) / normal).clamp(40.0, if tall { 120.0 } else { 170.0 });
    let slot_w: Vec<f32> = box_along.iter().map(|b| if *b > 0.0 { *b } else { per }).collect();
    let used = slot_w.iter().sum::<f32>() + gaps;
    let mut cursor = pad + ((along_total - used) / 2.0).max(0.0);
    let mut slot_start = vec![0.0f32; cols.len()];
    for c in 0..cols.len() {
        if c > 0 && cols[c].0 != cols[c - 1].0 {
            cursor += DISTRICT_GAP;
        }
        slot_start[c] = cursor;
        cursor += slot_w[c];
    }
    let along = |i: usize| {
        let c = column[i];
        if box_along[c] > 0.0 {
            slot_start[c] + slot_w[c] - if container[i] { 0.0 } else { BOX_PAD }
        } else {
            slot_start[c] + slot_w[c] / 2.0
        }
    };

    // Across positions: tracks centred in the room the district names and box titles leave.
    let (across_start, across_room) = if tall {
        // Room on the left for half a component, on the right for a station's name.
        let half = (0..n)
            .filter(|&i| parent[i].is_some())
            .map(|i| chip_w(i) / 2.0 + BOX_PAD)
            .fold(12.0, f32::max);
        (pad + half, size[0] - 2.0 * pad - half - 120.0)
    } else {
        let top = pad + DISTRICT_LABEL + if has_box { BOX_TITLE + 6.0 } else { 0.0 };
        (top, size[1] - top - pad - 20.0)
    };
    let step_across = if tracks > 0.0 {
        (across_room / tracks).min(if tall { 150.0 } else { 100.0 })
    } else {
        0.0
    };
    let spread = step_across * tracks;
    let first_track = across_start + (across_room - spread).max(0.0) / 2.0;
    let across_at = |i: usize| first_track + across[i] * step_across;
    let at = |i: usize| -> [f32; 2] { if tall { [across_at(i), along(i)] } else { [along(i), across_at(i)] } };

    // Page boxes and the components inside them.
    let chip_rect = |i: usize| -> [f32; 4] {
        let [x, y] = at(i);
        let w = chip_w(i);
        if tall {
            [x - w / 2.0, y - CHIP_H, w, CHIP_H]
        } else {
            [x - w, y - CHIP_H / 2.0, w, CHIP_H]
        }
    };
    let mut boxes: Vec<PageBox> = Vec::new();
    for i in (0..n).filter(|&i| container[i]) {
        let c = column[i];
        let (s0, s1) = (slot_start[c], slot_start[c] + slot_w[c]);
        let (mut lo, mut hi) = (across_at(i) - 12.0, across_at(i) + 12.0);
        for j in (0..n).filter(|&j| parent[j] == Some(i)) {
            let r = chip_rect(j);
            let (a0, a1) = if tall { (r[0], r[0] + r[2]) } else { (r[1], r[1] + r[3]) };
            lo = lo.min(a0);
            hi = hi.max(a1);
        }
        let rect = if tall {
            [lo - BOX_PAD, s0, hi - lo + 2.0 * BOX_PAD, s1 - s0]
        } else {
            [s0, lo - BOX_PAD - BOX_TITLE, s1 - s0, hi - lo + 2.0 * BOX_PAD + BOX_TITLE]
        };
        let id = &order[i].0;
        boxes.push(PageBox {
            id: id.clone(),
            label: label(i),
            rect,
            new: steps
                .iter()
                .filter(|(_, s)| s.from == *id || s.to == *id)
                .all(|(_, s)| s.status == Status::Added),
        });
    }
    let districts: Vec<District> = layers
        .iter()
        .enumerate()
        .filter_map(|(d, l)| {
            let cs: Vec<usize> = (0..cols.len()).filter(|&c| cols[c].0 == d).collect();
            let (first, last) = (*cs.first()?, *cs.last()?);
            let a0 = slot_start[first] - DISTRICT_GAP / 2.0 + 2.0;
            let a1 = slot_start[last] + slot_w[last] + DISTRICT_GAP / 2.0 - 2.0;
            let rect = if tall {
                [pad / 2.0, a0, size[0] - pad, a1 - a0]
            } else {
                let top = (first_track - DISTRICT_LABEL - 14.0 - if has_box { BOX_TITLE + 6.0 } else { 0.0 }).max(pad / 2.0);
                let bottom = (first_track + spread + 48.0).min(size[1] - pad / 2.0);
                [a0, top, a1 - a0, bottom - top]
            };
            let label = district_name(&l.0);
            let label_at = if tall {
                [rect[0] + rect[2] - label_w(&label, MICRO) - 12.0, rect[1] + 6.0]
            } else {
                [rect[0] + 10.0, rect[1] + 6.0]
            };
            Some(District { label, rect, label_at })
        })
        .collect();

    // What a line must keep out of: other stations' components and page boxes it isn't from.
    let mut obstacles: Vec<([f32; 4], Vec<usize>)> = Vec::new();
    for b in &boxes {
        let owner = idx(&b.id);
        let mut own: Vec<usize> = (0..n).filter(|&j| parent[j] == Some(owner)).collect();
        own.push(owner);
        obstacles.push((b.rect, own));
    }
    for j in (0..n).filter(|&j| parent[j].is_some()) {
        obstacles.push((chip_rect(j), vec![j]));
    }

    // Each stretch takes the grid route that keeps clear of other stations and of the
    // stretches already routed; removed stretches are routed last, around the rest.
    let mut routed: BTreeMap<(usize, usize), Vec<[f32; 2]>> = BTreeMap::new();
    let mut taken: Vec<[f32; 2]> = Vec::new();
    let mut by_status: Vec<usize> = (0..steps.len()).collect();
    by_status.sort_by_key(|&i| steps[i].1.status == Status::Removed);
    for i in by_status {
        let s = steps[i].1;
        let (f, t) = (idx(&s.from), idx(&s.to));
        let key = (f.min(t), f.max(t));
        if routed.contains_key(&key) {
            continue;
        }
        let mut best = (f32::INFINITY, Vec::new());
        for (k, cand) in octilinear(at(f), at(t)).into_iter().enumerate() {
            let pts = sample(&cand, 4.0);
            let mut score = k as f32;
            for j in (0..n).filter(|&j| j != f && j != t) {
                let c = at(j);
                let near = pts.iter().map(|p| dist(*p, c)).fold(f32::INFINITY, f32::min);
                if near < 26.0 {
                    score += (26.0 - near) * 40.0;
                }
            }
            for (r, own) in &obstacles {
                if !own.contains(&f) && !own.contains(&t) {
                    score += pts.iter().filter(|p| inside(**p, *r)).count() as f32 * 30.0;
                }
            }
            let (a0, b0) = (at(f), at(t));
            score += pts
                .iter()
                .filter(|p| dist(**p, a0) > 16.0 && dist(**p, b0) > 16.0)
                .filter(|p| taken.iter().any(|q| dist(**p, *q) < 9.0))
                .count() as f32
                * 3.0;
            if score < best.0 {
                best = (score, cand);
            }
        }
        taken.extend(sample(&best.1, 4.0));
        let mut route = best.1;
        if f > t {
            route.reverse();
        }
        routed.insert(key, route);
    }
    let mut segs: Vec<Seg> = steps
        .iter()
        .map(|(pi, s)| {
            let (f, t) = (idx(&s.from), idx(&s.to));
            let mut points = routed.get(&(f.min(t), f.max(t))).cloned().unwrap_or_default();
            if f > t {
                points.reverse();
            }
            Seg {
                line: *pi,
                from: s.from.clone(),
                to: s.to.clone(),
                call: s.call.clone(),
                status: s.status,
                fragment: s.fragment,
                points,
                packet: packet_label(a, &s.carries),
            }
        })
        .collect();
    let mut shared: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
    for (i, s) in segs.iter().enumerate() {
        let key = if s.from < s.to {
            (s.from.clone(), s.to.clone())
        } else {
            (s.to.clone(), s.from.clone())
        };
        shared.entry(key).or_default().push(i);
    }
    for group in shared.values().filter(|g| g.len() > 1) {
        let k = group.len() as f32;
        for (j, &i) in group.iter().enumerate() {
            let sign = if segs[i].from < segs[i].to { 1.0 } else { -1.0 };
            let d = (j as f32 - (k - 1.0) / 2.0) * LINE_GAP * sign;
            segs[i].points = offset(&segs[i].points, d);
        }
    }
    for s in &mut segs {
        s.points = rain_diagram::rounded(&s.points, CORNER);
    }

    let mut stations: Vec<Station> = order
        .iter()
        .enumerate()
        .map(|(i, (id, _))| {
            let touching: Vec<&Seg> = segs.iter().filter(|s| s.from == *id || s.to == *id).collect();
            let mut on: Vec<usize> = touching.iter().map(|s| s.line).collect();
            on.sort_unstable();
            on.dedup();
            let mark = if container[i] {
                Mark::Port
            } else if parent[i].is_some() {
                Mark::Chip(chip_rect(i))
            } else {
                Mark::Dot
            };
            Station {
                id: id.clone(),
                label: label(i),
                at: at(i),
                new: touching.iter().all(|s| s.status == Status::Added),
                lines: on,
                label_at: [0.0; 2],
                lights: Vec::new(),
                mark,
            }
        })
        .collect();
    for i in 0..stations.len() {
        let mut lights = vec![stations[i].id.clone()];
        for &l in &stations[i].lines {
            lights.push(line_id(&lines[l]));
            for s in &stations {
                if s.lines.contains(&l) && !lights.contains(&s.id) {
                    lights.push(s.id.clone());
                }
            }
        }
        stations[i].lights = lights;
    }
    let mut keep_clear: Vec<[f32; 4]> = obstacles.iter().map(|(r, _)| *r).collect();
    for d in &districts {
        let w = label_w(&d.label, MICRO) + 12.0;
        keep_clear.push([d.label_at[0] - 4.0, d.label_at[1] - 2.0, w, 18.0]);
    }
    place_labels(&mut stations, &segs, size, font, &keep_clear);
    Map {
        size,
        lines,
        stations,
        segs,
        districts,
        boxes,
        font,
    }
}

/// Puts each dot station's name where it covers the fewest lines, stations, boxes and other
/// names.
fn place_labels(stations: &mut [Station], segs: &[Seg], size: [f32; 2], font: TextStyle, keep_clear: &[[f32; 4]]) {
    let tall = size[1] > size[0] * 1.1;
    let h = font.line_height.max(font.size + 4.0);
    let samples: Vec<[f32; 2]> = segs.iter().flat_map(|s| sample(&s.points, 4.0)).collect();
    let overlap = |a: [f32; 4], b: [f32; 4]| {
        let w = (a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0]);
        let hh = (a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1]);
        if w > 0.0 && hh > 0.0 { w * hh } else { 0.0 }
    };
    let mut placed: Vec<[f32; 4]> = Vec::new();
    for i in 0..stations.len() {
        if stations[i].mark != Mark::Dot {
            continue;
        }
        let (p, r) = (stations[i].at, stations[i].diameter() / 2.0 + 5.0);
        let w = label_w(&stations[i].label, font);
        let (below, above, right) = (
            [p[0] - w / 2.0, p[1] + r],
            [p[0] - w / 2.0, p[1] - r - h],
            [p[0] + r, p[1] - h / 2.0],
        );
        // Lines run along the longer side, so a name goes across them first.
        let first = if tall { [right, below, above] } else { [below, above, right] };
        let cands = [
            first[0],
            first[1],
            first[2],
            [p[0] + r * 0.7, p[1] + r * 0.7],
            [p[0] + r * 0.7, p[1] - r * 0.7 - h],
            [p[0] - r - w, p[1] - h / 2.0],
            [p[0] - r * 0.7 - w, p[1] + r * 0.7],
            [p[0] - r * 0.7 - w, p[1] - r * 0.7 - h],
        ];
        let mut best = (f32::INFINITY, cands[0]);
        for (k, c) in cands.iter().enumerate() {
            let rect = [c[0] - 2.0, c[1] - 1.0, w + 4.0, h + 2.0];
            let mut score = k as f32 * 2.0;
            score += samples.iter().filter(|q| inside(**q, rect)).count() as f32 * 40.0;
            for s in stations.iter() {
                let d = s.diameter() / 2.0 + 2.0;
                score += overlap(rect, [s.at[0] - d, s.at[1] - d, 2.0 * d, 2.0 * d]) * 4.0;
            }
            for o in placed.iter().chain(keep_clear) {
                score += overlap(rect, *o) * 8.0;
            }
            if rect[0] < 4.0 || rect[1] < 4.0 || rect[0] + rect[2] > size[0] - 4.0 || rect[1] + rect[3] > size[1] - 4.0 {
                score += 5000.0;
            }
            if score < best.0 {
                best = (score, *c);
            }
        }
        stations[i].label_at = best.1;
        placed.push([best.1[0], best.1[1], w, h]);
    }
}

/// A dot running one stretch, and a label saying what the call carries, held still beside the
/// stretch while the dot runs so it can be read.
#[derive(Clone)]
pub(crate) struct Packet {
    pub line: usize,
    pub pulse: Pulse,
    pub label: Option<String>,
    /// The label's top-left on the map: beside the stretch's middle, clear of names, stations
    /// and components.
    pub label_at: [f32; 2],
    pub label_alpha: Option<Track<f32>>,
}

/// Where the dot stations' names sit, with their plates.
pub(crate) fn names(m: &Map) -> Vec<[f32; 4]> {
    m.stations
        .iter()
        .filter(|s| s.mark == Mark::Dot)
        .map(|s| [s.label_at[0] - 3.0, s.label_at[1] - 1.0, label_w(&s.label, m.font) + 6.0, 18.0])
        .collect()
}

/// A packet's label: its width and height.
pub(crate) fn label_size(l: &str) -> [f32; 2] {
    [mono_w(l, MONO) + 12.0, 18.0]
}

impl Packet {
    /// A packet running stretch `seg` from `start` for `dur` seconds, labelled with what the
    /// call carries when `labelled`.
    pub fn new(m: &Map, seg: usize, start: f32, dur: f32, labelled: bool) -> Packet {
        let s = &m.segs[seg];
        let label = if labelled { s.packet.clone() } else { None };
        let label_at = label.as_deref().map_or([0.0; 2], |l| label_spot(m, s, label_size(l)));
        let label_alpha = label.as_ref().map(|_| {
            let fade = (dur * 0.15).min(0.15);
            Track::new(vec![
                (start, 0.0),
                (start + fade, 1.0),
                (start + dur - fade, 1.0),
                (start + dur, 0.0),
            ])
        });
        Packet {
            line: s.line,
            pulse: Pulse::new(s.points.clone(), start, dur, false, PACKET),
            label,
            label_at,
            label_alpha,
        }
    }
}

/// Beside the stretch near its middle, on whichever side covers the fewest station and tier
/// names, stations, components, page blocks and lines, inside the map.
fn label_spot(m: &Map, s: &Seg, [w, h]: [f32; 2]) -> [f32; 2] {
    let g = PACKET / 2.0 + 4.0;
    let sides = [
        [-w / 2.0, -h - g],
        [-w / 2.0, g],
        [g, -h / 2.0],
        [-w - g, -h / 2.0],
        [g * 0.7, -h - g * 0.7],
        [-w - g * 0.7, -h - g * 0.7],
        [g * 0.7, g * 0.7],
        [-w - g * 0.7, g * 0.7],
        [-w / 2.0, -h - 3.0 * g],
        [-w / 2.0, 3.0 * g],
        [-w * 0.2, -h - 2.0 * g],
        [-w * 0.8, 2.0 * g],
    ];
    let mut named = names(m);
    named.extend(
        m.districts
            .iter()
            .map(|d| [d.label_at[0] - 4.0, d.label_at[1] - 2.0, label_w(&d.label, MICRO) + 12.0, 18.0]),
    );
    let lines: Vec<[f32; 2]> = m.segs.iter().flat_map(|x| sample(&x.points, 6.0)).collect();
    let (mut dots, mut busy) = (Vec::new(), Vec::new());
    for st in &m.stations {
        let d = st.diameter() / 2.0 + 3.0;
        dots.push([st.at[0] - d, st.at[1] - d, 2.0 * d, 2.0 * d]);
        if st.mark != Mark::Dot {
            busy.push(m.extent(st));
        }
    }
    let mut best = (f32::INFINITY, [0.0; 2]);
    for (j, frac) in [0.5, 0.4, 0.6, 0.3, 0.7, 0.2, 0.8].into_iter().enumerate() {
        let p = motion::along(&s.points, frac);
        for (k, off) in sides.iter().enumerate() {
            let r = [p[0] + off[0], p[1] + off[1], w, h];
            let mut score = (j * 2 + k) as f32;
            score += named.iter().map(|b| overlap(r, *b) * 10.0).sum::<f32>();
            score += dots.iter().map(|b| overlap(r, *b) * 40.0).sum::<f32>();
            score += busy.iter().map(|b| overlap(r, *b)).sum::<f32>();
            score += lines.iter().filter(|p| inside(**p, r)).count() as f32 * 40.0;
            if r[0] < 2.0 || r[1] < 2.0 || r[0] + w > m.size[0] - 2.0 || r[1] + h > m.size[1] - 2.0 {
                score += 20000.0;
            }
            if score < best.0 {
                best = (score, [r[0], r[1]]);
            }
        }
    }
    best.1
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> f32 {
    let w = (a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0]);
    let h = (a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1]);
    if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
}

/// How the map moves: per stretch and station, `None` where it stays as drawn.
#[derive(Clone, Default)]
pub(crate) struct Anim {
    /// New stretches drawing on.
    pub trim: Vec<Option<WriteOn>>,
    /// A new stretch's fragment tag, landing with it.
    pub tag: Vec<Option<Track<f32>>>,
    /// The solid line of a removed stretch.
    pub solid: Vec<Option<Track<f32>>>,
    /// The dashed ghost a removed stretch leaves.
    pub ghost: Vec<Option<Track<f32>>>,
    /// The halo of a changed stretch.
    pub halo: Vec<Option<Track<f32>>>,
    /// New stations popping in, scale and opacity.
    pub pop: Vec<Option<Track<f32>>>,
    /// Interchange rings.
    pub ring: Vec<Option<Track<f32>>>,
    pub packets: Vec<Packet>,
}

impl Anim {
    /// The after state, held: removed stretches as ghosts, changed ones haloed.
    pub fn still(m: &Map) -> Anim {
        let held = |on: bool, v: f32| on.then(|| Track::new(vec![(0.0, v)]));
        Anim {
            trim: m.segs.iter().map(|_| None).collect(),
            tag: m.segs.iter().map(|_| None).collect(),
            solid: m.segs.iter().map(|s| held(s.status == Status::Removed, 0.0)).collect(),
            ghost: m.segs.iter().map(|s| held(s.status == Status::Removed, 1.0)).collect(),
            halo: m.segs.iter().map(|s| held(s.status == Status::Changed, 0.35)).collect(),
            pop: m.stations.iter().map(|_| None).collect(),
            ring: m.stations.iter().map(|_| None).collect(),
            packets: Vec::new(),
        }
    }

    pub fn add_to(&self, rig: &mut Rig) {
        for w in self.trim.iter().flatten() {
            rig.add(w.clone());
        }
        for t in self
            .tag
            .iter()
            .chain(&self.solid)
            .chain(&self.ghost)
            .chain(&self.halo)
            .chain(&self.pop)
            .chain(&self.ring)
            .flatten()
        {
            rig.add(t.clone());
        }
        for p in &self.packets {
            rig.add(p.pulse.clone());
            if let Some(a) = &p.label_alpha {
                rig.add(a.clone());
            }
        }
    }
}

/// `a`, `a and b`, `a, b and c`.
fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [a] => a.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// One plain sentence per path the change touches, from the data: what an existing path gains
/// and loses, what a new path is and what it shares. Each comes with its path's index.
pub(crate) fn conclusion(a: &Trace) -> Vec<(usize, String)> {
    let name = |id: &str| a.entity(id).map_or_else(|| id.to_owned(), |e| e.label.clone());
    let call = |c: &str| c.split(['(', ' ']).next().unwrap_or(c).to_owned();
    let touches = |p: &TracePath, id: &str| p.steps.iter().any(|s| s.status != Status::Removed && (s.from == id || s.to == id));
    let hop = |s: &TraceStep| format!("{} → {} ({})", name(&s.from), name(&s.to), call(&s.call));
    let mut changed = Vec::new();
    let mut new = Vec::new();
    for (pi, p) in a.paths.iter().enumerate() {
        if fresh(p) {
            let mut stations: Vec<String> = Vec::new();
            for s in &p.steps {
                for id in [&s.from, &s.to] {
                    if !stations.contains(id) {
                        stations.push(id.clone());
                    }
                }
            }
            let others: Vec<&TracePath> = a.paths.iter().enumerate().filter(|(j, _)| *j != pi).map(|(_, q)| q).collect();
            let shared: Vec<String> = stations
                .iter()
                .filter(|id| others.iter().any(|q| touches(q, id)))
                .map(|id| name(id))
                .collect();
            let mut s = format!("{} is a new path of {}", p.name, plural(p.steps.len(), "call", "calls"));
            if !shared.is_empty() {
                let with: Vec<String> = others
                    .iter()
                    .filter(|q| stations.iter().any(|id| touches(q, id)))
                    .map(|q| q.name.clone())
                    .collect();
                s += &format!(" and shares {} with {}", list(&shared), list(&with));
            }
            new.push((pi, s + "."));
        } else {
            let gains: Vec<String> = p.steps.iter().filter(|s| s.status == Status::Added).map(hop).collect();
            let loses: Vec<String> = p.steps.iter().filter(|s| s.status == Status::Removed).map(hop).collect();
            let mut parts = Vec::new();
            if !gains.is_empty() {
                parts.push(format!("gains {}", list(&gains)));
            }
            if !loses.is_empty() {
                parts.push(format!("loses {}", list(&loses)));
            }
            if !parts.is_empty() {
                changed.push((pi, format!("{} {}.", p.name, parts.join(" and "))));
            }
        }
    }
    changed.extend(new);
    changed
}

fn status_word(s: Status) -> &'static str {
    match s {
        Status::Added => "new",
        Status::Changed => "changed",
        Status::Removed => "removed",
        Status::Same => "unchanged",
    }
}

fn fragment_word(f: Fragment) -> &'static str {
    match f {
        Fragment::Alt => "alt",
        Fragment::Loop => "↻ loop",
        Fragment::Opt => "opt",
    }
}

/// A full-size layer of the map, its opacity bound to `live`.
fn layer(m: &Map, live: &rune::Live<f32>, reduced: bool) -> El {
    motion::fade(el().absolute(0.0, 0.0).size(m.size[0], m.size[1]), live, reduced)
}

/// The districts and page boxes, behind the lines.
fn ground(key: &str, m: &Map, ink: &DiagramStyle) -> El {
    let mut g = el().absolute(0.0, 0.0).size(m.size[0], m.size[1]).a11y_hidden();
    for (i, d) in m.districts.iter().enumerate() {
        let [x, y, w, h] = d.rect;
        g = g.child(
            el().key(format!("{key}-district-{i}"))
                .absolute(x, y)
                .size(w, h)
                .radius(10.0)
                .fill(ink.foreground.alpha(if i % 2 == 0 { 0.035 } else { 0.02 }))
                .border(1.0, ink.foreground.alpha(0.06)),
        );
        g = g.child(
            text(d.label.clone())
                .font(MICRO)
                .color(ink.muted)
                .absolute(d.label_at[0], d.label_at[1]),
        );
    }
    for b in &m.boxes {
        let [x, y, w, h] = b.rect;
        let rim = if b.new { ink.added_border } else { ink.card_border };
        g = g.child(
            el().key(format!("{key}-page-{}", b.id))
                .absolute(x, y)
                .size(w, h)
                .radius(6.0)
                .fill(ink.card)
                .border(1.0, rim)
                .child(
                    text(b.label.clone())
                        .font(MICRO)
                        .color(ink.muted)
                        .truncate()
                        .absolute(BOX_PAD, 2.0)
                        .w(w - 2.0 * BOX_PAD),
                ),
        );
    }
    g
}

/// The map drawn in its own box, `key` prefixing every key. `hl` is hover (lines and
/// stations), `focus` the timeline's (`seg:i`, `line:id` and station ids).
pub(crate) fn draw(key: &str, m: &Map, ink: &DiagramStyle, reduced: bool, hl: &Highlight, focus: &Highlight, anim: &Anim) -> El {
    let mut world = el().key(key.to_owned()).size(m.size[0], m.size[1]).child(ground(key, m, ink));

    // Removed stretches leave a dashed ghost and a cross.
    for (i, s) in m.segs.iter().enumerate() {
        let Some(g) = anim.ghost.get(i).and_then(|g| g.as_ref()) else {
            continue;
        };
        let mut shape = Shape::new();
        for d in rain_diagram::dashes(&s.points, 7.0, 6.0) {
            shape = shape.stroke_with(d, false, 2.5, LineCap::Round, ink.removed.alpha(0.85));
        }
        let mid = s.mid();
        world = world.child(
            canvas(shape)
                .key(format!("{key}-ghost-{i}"))
                .absolute(0.0, 0.0)
                .size(m.size[0], m.size[1])
                .live_opacity(&g.live)
                .a11y_hidden(),
        );
        world = world.child(
            paint::centred("✕", SMALL, ink.removed_text, 16.0, 16.0)
                .absolute(mid[0] - 8.0, mid[1] - 8.0)
                .radius(8.0)
                .fill(ink.removed_fill)
                .live_opacity(&g.live)
                .a11y_hidden(),
        );
    }

    // The lines: one canvas per stretch, so each draws on by itself.
    for (i, s) in m.segs.iter().enumerate() {
        let colour = m.lines[s.line].colour;
        let mut stretch = layer(m, &focus.opacity(&seg_id(i)), reduced);
        let mut inner = layer(m, &hl.opacity(&line_id(&m.lines[s.line])), reduced);
        if let Some(h) = anim.halo.get(i).and_then(|h| h.as_ref()) {
            inner = inner.child(
                canvas(Shape::new().stroke(s.points.clone(), LINE_W + 8.0, ink.modified.alpha(0.55)))
                    .absolute(0.0, 0.0)
                    .size(m.size[0], m.size[1])
                    .live_opacity(&h.live)
                    .a11y_hidden(),
            );
        }
        let mut line = canvas(Shape::new().stroke(s.points.clone(), LINE_W, colour))
            .key(format!("{key}-seg-{i}"))
            .absolute(0.0, 0.0)
            .size(m.size[0], m.size[1])
            .a11y_hidden();
        if let Some(w) = anim.trim.get(i).and_then(|w| w.as_ref()) {
            line = w.bind(line);
        }
        if let Some(a) = anim.solid.get(i).and_then(|a| a.as_ref()) {
            line = line.live_opacity(&a.live);
        }
        inner = inner.child(line);
        stretch = stretch.child(inner);
        world = world.child(stretch);
    }

    // Each stretch's call, as a tooltip over its middle; fragments as a tag.
    for (i, s) in m.segs.iter().enumerate() {
        let mid = s.mid();
        let said = format!("{} · {} ({})", m.lines[s.line].name, s.call, status_word(s.status));
        let mut tip = el()
            .key(format!("{key}-call-{i}"))
            .absolute(mid[0] - 12.0, mid[1] - 12.0)
            .size(24.0, 24.0)
            .role("note")
            .label(said.clone())
            .help(said);
        if let Some(f) = s.fragment {
            let word = fragment_word(f);
            let w = rain_diagram::design::measure(word, 10.0) + 10.0;
            let mut tag = paint::centred(word, MICRO, ink.foreground, w, 15.0)
                .absolute(12.0 - w / 2.0, -10.0)
                .radius(7.5)
                .fill(ink.card)
                .border(1.0, m.lines[s.line].colour);
            if let Some(t) = anim.tag.get(i).and_then(|t| t.as_ref()) {
                tag = tag.live_opacity(&t.live);
            }
            tip = tip.child(tag);
        }
        world = world.child(tip);
    }

    // Stations: dots with names, components in their page, and the page's own port.
    for (i, st) in m.stations.iter().enumerate() {
        let d = st.diameter();
        let [x, y] = st.at;
        let colour = m.lines[st.lines.first().copied().unwrap_or(0)].colour;
        let said = format!(
            "{}{}{}",
            st.label,
            if st.interchange() { ", interchange" } else { "" },
            if st.new { ", new" } else { "" }
        );
        let ring = |live: &rune::Live<f32>, c: Color, k: &str, r: [f32; 4]| {
            el().key(format!("{key}-{k}-{}", st.id))
                .absolute(r[0], r[1])
                .size(r[2], r[3])
                .radius(r[3].min(r[2]) / 2.0)
                .border(2.0, c)
                .live_opacity(live)
                .a11y_hidden()
        };
        let mut group = el().absolute(0.0, 0.0).size(m.size[0], m.size[1]);
        let mark: El = match st.mark {
            Mark::Dot => {
                let mut dot = el()
                    .key(format!("{key}-station-{}", st.id))
                    .absolute(x - d / 2.0, y - d / 2.0)
                    .size(d, d)
                    .radius(d / 2.0)
                    .fill(ink.background)
                    .role("img")
                    .label(said)
                    .help(st.label.clone());
                dot = if st.interchange() {
                    dot.border(3.5, ink.foreground)
                } else {
                    dot.border(4.0, colour)
                };
                if st.new {
                    dot = dot.child(
                        el().absolute(d / 2.0 - 3.0, d / 2.0 - 3.0)
                            .size(6.0, 6.0)
                            .radius(3.0)
                            .fill(ink.added)
                            .a11y_hidden(),
                    );
                }
                let r = d + 12.0;
                let around = [x - r / 2.0, y - r / 2.0, r, r];
                if let Some(rg) = anim.ring.get(i).and_then(|r| r.as_ref()) {
                    group = group.child(ring(&rg.live, ink.foreground, "ring", around));
                }
                group = group
                    .child(ring(&focus.lit(&st.id), ink.foreground.alpha(0.7), "focus", around))
                    .child(ring(&hl.lit(&st.id), ink.foreground, "hover", around));
                // A soft plate under the name keeps it legible where a line runs behind it.
                group = group.child(
                    row()
                        .absolute(st.label_at[0] - 3.0, st.label_at[1] - 1.0)
                        .px(3.0)
                        .py(1.0)
                        .radius(4.0)
                        .fill(ink.background.alpha(0.9))
                        .child(
                            text(st.label.clone())
                                .font(m.font)
                                .color(if st.new { ink.added_text } else { ink.foreground }),
                        )
                        .a11y_hidden(),
                );
                dot
            }
            Mark::Chip(r) => {
                let tone = if st.new {
                    paint::tone(ink, Status::Added)
                } else {
                    paint::tone(ink, Status::Same)
                };
                let around = [r[0] - 4.0, r[1] - 4.0, r[2] + 8.0, r[3] + 8.0];
                group = group
                    .child(ring(&focus.lit(&st.id), ink.foreground.alpha(0.7), "focus", around))
                    .child(ring(&hl.lit(&st.id), ink.foreground, "hover", around));
                let port = [x - r[0] - 5.0, y - r[1] - 5.0];
                el().key(format!("{key}-station-{}", st.id))
                    .absolute(r[0], r[1])
                    .size(r[2], r[3])
                    .radius(6.0)
                    .fill(tone.fill)
                    .border(1.5, colour)
                    .role("img")
                    .label(said)
                    .help(st.label.clone())
                    .child(paint::centred(st.label.clone(), m.font, tone.text, r[2] - 8.0, r[3]).absolute(4.0, 0.0))
                    .child(
                        el().absolute(port[0], port[1])
                            .size(10.0, 10.0)
                            .radius(5.0)
                            .fill(ink.background)
                            .border(3.0, colour)
                            .a11y_hidden(),
                    )
            }
            Mark::Port => {
                let around = m.boxes.iter().find(|b| b.id == st.id).map_or([x - 8.0, y - 8.0, 16.0, 16.0], |b| {
                    [b.rect[0] - 3.0, b.rect[1] - 3.0, b.rect[2] + 6.0, b.rect[3] + 6.0]
                });
                group = group
                    .child(ring(&focus.lit(&st.id), ink.foreground.alpha(0.5), "focus", around).radius(10.0))
                    .child(ring(&hl.lit(&st.id), ink.foreground, "hover", around).radius(10.0));
                el().key(format!("{key}-station-{}", st.id))
                    .absolute(x - 6.0, y - 6.0)
                    .size(12.0, 12.0)
                    .radius(6.0)
                    .fill(ink.background)
                    .border(3.5, colour)
                    .role("img")
                    .label(said)
                    .help(st.label.clone())
            }
        };
        group = group.child(hl.hover(mark, st.lights.clone()));
        if let Some(p) = anim.pop.get(i).and_then(|p| p.as_ref()) {
            group = group.live_opacity(&p.live);
        }
        let group = layer(m, &focus.opacity(&st.id), reduced).child(layer(m, &hl.opacity(&st.id), reduced).child(group));
        world = world.child(group);
    }

    // Packets, over everything: a dot, and what the call carries held beside its stretch.
    for (k, p) in anim.packets.iter().enumerate() {
        let colour = m.lines[p.line].colour;
        let dot = el()
            .key(format!("{key}-packet-{k}"))
            .radius(PACKET / 2.0)
            .fill(ink.foreground)
            .border(2.5, colour)
            .a11y_hidden();
        world = world.child(p.pulse.bind(dot));
        if let Some(l) = &p.label {
            let [w, h] = label_size(l);
            let mut tag = paint::centred(l.clone(), MONO, ink.foreground, w, h)
                .key(format!("{key}-packet-label-{k}"))
                .absolute(p.label_at[0], p.label_at[1])
                .radius(h / 2.0)
                .fill(ink.card)
                .border(1.0, colour)
                .a11y_hidden();
            if let Some(a) = &p.label_alpha {
                tag = tag.live_opacity(&a.live);
            }
            world = world.child(tag);
        }
    }
    world
}

/// The conclusion, one sentence per path, each marked with its line's colour.
pub(crate) fn upshot(key: &str, said: &[(usize, String)], m: &Map, ink: &DiagramStyle, w: f32) -> El {
    let mut c = col().key(key.to_owned()).w(w).gap(3.0);
    for (l, s) in said {
        let colour = m.lines.get(*l).map_or(ink.muted, |x| x.colour);
        c = c.child(
            row()
                .w(w)
                .gap(8.0)
                .items_start()
                .child(el().size(14.0, 4.0).mt(6.0).radius(2.0).fill(colour).a11y_hidden())
                .child(
                    text(s.clone())
                        .font(LABEL)
                        .color(ink.foreground)
                        .wrap()
                        .shrink()
                        .role("note")
                        .label(s.clone()),
                ),
        );
    }
    c
}

#[cfg(test)]
mod tests {
    use rain_diagram::DiagramStyle;

    use super::*;
    use crate::schema::samples;

    fn within(p: [f32; 2], r: [f32; 4]) -> bool {
        p[0] >= r[0] - 0.5 && p[0] <= r[0] + r[2] + 0.5 && p[1] >= r[1] - 0.5 && p[1] <= r[1] + r[3] + 0.5
    }

    #[test]
    fn every_call_is_a_stretch_and_shared_stations_are_interchanges() {
        let a = &samples().a;
        for size in [[1200.0, 560.0], [800.0, 540.0], [450.0, 700.0]] {
            let m = map(a, &DiagramStyle::dark(), size, LABEL);
            assert_eq!(m.segs.len(), a.paths.iter().map(|p| p.steps.len()).sum::<usize>());
            assert_eq!(m.stations.len(), 11);
            for s in &m.stations {
                assert!(within(s.at, [0.0, 0.0, size[0], size[1]]), "{} in {size:?}", s.id);
                if s.mark == Mark::Dot {
                    let w = label_w(&s.label, LABEL);
                    assert!(
                        within(s.label_at, [0.0, 0.0, size[0], size[1]])
                            && within([s.label_at[0] + w, s.label_at[1]], [0.0, 0.0, size[0], size[1]]),
                        "{} named inside {size:?}",
                        s.id
                    );
                }
            }
            for seg in &m.segs {
                let (f, t) = (m.station(&seg.from).unwrap().at, m.station(&seg.to).unwrap().at);
                let (p, q) = (seg.points[0], *seg.points.last().unwrap());
                assert!(
                    dist(p, f) <= LINE_GAP && dist(q, t) <= LINE_GAP,
                    "{} runs station to station: {p:?} {f:?} {q:?} {t:?} {size:?}",
                    seg.call
                );
            }
            let mut inter: Vec<&str> = m.stations.iter().filter(|s| s.interchange()).map(|s| s.id.as_str()).collect();
            inter.sort_unstable();
            assert_eq!(inter, ["account-service", "voucher-service", "voucher-table"]);
            assert!(m.station("voucher-form").unwrap().new);
            assert!(!m.station("account-service").unwrap().new);
        }
    }

    #[test]
    fn stations_sit_in_their_tier_and_components_in_their_page() {
        let a = &samples().a;
        for size in [[800.0, 540.0], [450.0, 700.0]] {
            let m = map(a, &DiagramStyle::dark(), size, LABEL);
            let names: Vec<&str> = m.districts.iter().map(|d| d.label.as_str()).collect();
            assert_eq!(names, ["Frontend", "Backend", "Database"], "{size:?}");
            for s in &m.stations {
                let layer = district_name(paint::tier(&a.entity(&s.id).unwrap().layer));
                let d = m.districts.iter().find(|d| d.label == layer).unwrap();
                assert!(within(s.at, d.rect), "{} in {layer} {size:?}", s.id);
            }
            assert_eq!(m.boxes.len(), 1);
            let page = &m.boxes[0];
            assert_eq!((page.id.as_str(), page.label.as_str()), ("account-page", "Account page"));
            let Mark::Chip(chip) = m.station("voucher-form").unwrap().mark else {
                panic!("the form is drawn inside its page")
            };
            assert!(
                within([chip[0], chip[1]], page.rect) && within([chip[0] + chip[2], chip[1] + chip[3]], page.rect),
                "{chip:?} in {:?}",
                page.rect
            );
            assert_eq!(m.station("account-page").unwrap().mark, Mark::Port);
            for (i, x) in m.districts.iter().enumerate() {
                for y in &m.districts[i + 1..] {
                    let tall = size[1] > size[0] * 1.1;
                    let (ax, bx, ay) = if tall {
                        (x.rect[1], x.rect[1] + x.rect[3], y.rect[1])
                    } else {
                        (x.rect[0], x.rect[0] + x.rect[2], y.rect[0])
                    };
                    assert!(ax < ay && bx <= ay, "districts {} and {} don't overlap", x.label, y.label);
                }
            }
        }
    }

    #[test]
    fn packets_say_what_each_call_carries() {
        let a = &samples().a;
        let m = map(a, &DiagramStyle::dark(), [800.0, 540.0], LABEL);
        let first = m.segs.iter().find(|s| s.from == "voucher-form").unwrap();
        assert_eq!(first.packet.as_deref(), Some("VoucherDto {accountId, amount}"));
        let sum = m.segs.iter().find(|s| s.call == "SUM(Amount)").unwrap();
        assert_eq!(sum.packet.as_deref(), Some("Voucher row {amount} + Account total"));
    }

    #[test]
    fn the_conclusion_comes_from_the_data() {
        let c = conclusion(&samples().a);
        assert_eq!(
            c,
            [
                (
                    1,
                    "Pay at checkout gains PaymentService → VoucherService (Redeem) and AccountService → \
                     Vouchers table (SUM) and loses PaymentsController → AccountService (Total)."
                        .to_owned()
                ),
                (
                    0,
                    "Record a voucher is a new path of 8 calls and shares VoucherService, Vouchers table \
                     and AccountService with Pay at checkout."
                        .to_owned()
                ),
            ]
        );
    }

    #[test]
    fn the_routes_keep_to_the_grid() {
        for (p, q) in [([0.0, 0.0], [100.0, 40.0]), ([0.0, 0.0], [30.0, -90.0]), ([5.0, 5.0], [5.0, 80.0])] {
            for route in octilinear(p, q) {
                assert_eq!((route[0], *route.last().unwrap()), (p, q));
                for w in route.windows(2) {
                    let (dx, dy) = ((w[1][0] - w[0][0]).abs(), (w[1][1] - w[0][1]).abs());
                    assert!(dx < 1e-3 || dy < 1e-3 || (dx - dy).abs() < 1e-3, "{w:?} is 0, 45 or 90 degrees");
                }
            }
        }
    }
}
