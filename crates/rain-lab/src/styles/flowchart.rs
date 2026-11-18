//! The flowchart: a hand-laid scene (schema S) played beat by beat, in the manner of Manim. Boxes
//! are components, arrows are calls; the script creates them, draws the arrows on, carries a
//! dot from box to box, focuses the camera, runs a pulse and relabels what the change rewrites.
//! Each beat's sentence sits in a card under the figure, bright while it plays. Status shows in
//! colour and in words: a new, changed or removed box wears a tag, a removed one is dashed and
//! struck through, a removed call is dashed and crossed. A sentence on top, worked out from the
//! script, sums the change up.

use rain_diagram::DiagramStyle;
use rune::tok::TextStyle;
use rune::{Color, Cursor, El, LineCap, Shape, canvas, col, el, row, text};

use super::beats::{self, Tracks, plural};
use super::paint::{self, MICRO, SMALL};
use super::tour::{self, Chip, Para};
use crate::motion::{self, Camera, FocusTrack, Pulse, Rig, Track, WriteOn};
use crate::schema::scene::{Action, Scene, ShapeKind, Verb};
use crate::schema::trace::{Span, spans};
use crate::schema::{SchemaId, Status};
use crate::style::{Act, Cx, Style};

pub struct Flowchart;

const MARGIN: f32 = 24.0;
const GAP: f32 = 10.0;
const LEGEND_H: f32 = 16.0;
const CARD_COLS: usize = 3;
const CARD_GAP: f32 = 8.0;
const CARD_PAD: f32 = 8.0;
/// The step number's pill, and the room between it and the sentence.
const PILL: f32 = 18.0;
const PILL_GAP: f32 = 8.0;
/// One line of a card's sentence, chips included, and the space between lines.
const LINE_H: f32 = 20.0;
const LINE_GAP: f32 = 4.0;
/// A scene is drawn in its own points and scaled to fit, so its type starts larger: at the
/// usual three-quarter scale it lands at 11 points or more.
const SCENE_BOX: TextStyle = TextStyle::new(15.0, 600.0).lh(20.0);
const SCENE_NOTE: TextStyle = TextStyle::new(15.0, 400.0).lh(19.0);
const SCENE_CAPTION: TextStyle = TextStyle::new(18.0, 600.0).lh(23.0);
const SCENE_MONO: TextStyle = TextStyle::new(15.0, 400.0).mono().lh(18.0);
const SCENE_TAG: TextStyle = TextStyle::new(15.0, 600.0).lh(18.0);
const TAG_H: f32 = 22.0;

/// Ids lit from a time on, and camera keys `(t, centre, zoom)`.
pub(crate) type FocusKeys = Vec<(f32, Vec<String>)>;
pub(crate) type CameraKeys = Vec<(f32, [f32; 2], f32)>;

/// One scene shape in scene points, with the values its beats move.
pub(crate) struct Placed {
    pub id: String,
    pub kind: ShapeKind,
    /// Box, text: top-left and size; dot: centre box; path, arrow: their bounds.
    pub rect: [f32; 4],
    pub points: Vec<[f32; 2]>,
    pub text: Option<String>,
    pub morph: Option<String>,
    pub tone: Status,
    /// A box's status tag, in the box's own points: its word and rect.
    pub tag: Option<(&'static str, [f32; 4])>,
    /// Where a path's label sits, clear of boxes, lines, tags and other labels.
    pub label_at: Option<[f32; 2]>,
    pub alpha: Track<f32>,
    pub scale: Track<f32>,
    pub offset: Track<[f32; 2]>,
    /// Every offset its moves take it to.
    pub travel: Vec<[f32; 2]>,
    pub reveal: Option<Track<[f32; 4]>>,
    pub draw: Option<WriteOn>,
    pub label_alpha: Track<f32>,
    /// The old label fading out and the new one in.
    pub swap: Option<(Track<f32>, Track<f32>)>,
    pub pulses: Vec<Pulse>,
}

/// A scene ready to play: its shapes in scene points, and the region of them the figure frames.
pub(crate) struct Played {
    pub shapes: Vec<Placed>,
    pub region: [f32; 4],
}

/// Where the takeaway, the figure, the legend and each beat's card sit.
pub(crate) struct Frame {
    pub says: [f32; 4],
    pub figure: [f32; 4],
    pub legend_y: f32,
    pub cards: Vec<[f32; 4]>,
}

pub(crate) struct Chart {
    pub frame: Frame,
    pub paras: Vec<Para>,
    pub scene: Played,
    pub says: String,
    /// The statuses the scene shows, for the legend.
    pub tones: Vec<Status>,
}

fn tone_of(s: Option<Status>) -> Status {
    s.unwrap_or(Status::Same)
}

fn tag_word(s: Status) -> Option<&'static str> {
    match s {
        Status::Added => Some("new"),
        Status::Changed => Some("changed"),
        Status::Removed => Some("removed"),
        Status::Same => None,
    }
}

fn verb_word(v: Verb) -> &'static str {
    match v {
        Verb::Create => "create",
        Verb::Write => "write",
        Verb::Transform => "transform",
        Verb::Move => "move",
        Verb::Fade => "fade",
        Verb::Focus => "focus",
        Verb::Pulse => "pulse",
        Verb::Morph => "morph",
    }
}

fn measure(t: &str, font: TextStyle) -> f32 {
    rain_diagram::design::measure(t, font.size)
}

/// A text shape's type: a plain one is a caption, a toned one a note.
fn text_font(tone: Status) -> TextStyle {
    if tone == Status::Same { SCENE_CAPTION } else { SCENE_NOTE }
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

fn grow(r: [f32; 4], d: f32) -> [f32; 4] {
    [r[0] - d, r[1] - d, r[2] + 2.0 * d, r[3] + 2.0 * d]
}

/// `a`, `a and b`, `a, b and c`.
fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [a] => a.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// How the scene's `region` (what it draws, in its own points) sits centred in a figure `fsize`
/// big: its scale, and where the scene's origin lands.
pub(crate) fn fit(region: [f32; 4], fsize: [f32; 2]) -> (f32, [f32; 2]) {
    let k = (fsize[0] / region[2]).min(fsize[1] / region[3]);
    (
        k,
        [
            (fsize[0] - region[2] * k) / 2.0 - region[0] * k,
            (fsize[1] - region[3] * k) / 2.0 - region[1] * k,
        ],
    )
}

/// What the scene draws, in its own points: every shape, tag and label, and everywhere a dot
/// travels, with a margin.
pub(crate) fn bounds(shapes: &[Placed]) -> [f32; 4] {
    let mut rects: Vec<[f32; 4]> = Vec::new();
    for sh in shapes {
        rects.push(sh.rect);
        if let Some((_, t)) = sh.tag {
            rects.push([sh.rect[0] + t[0], sh.rect[1] + t[1], t[2], t[3]]);
        }
        if let (Some(t), Some(at)) = (&sh.text, sh.label_at) {
            rects.push([at[0], at[1], measure(t, SCENE_MONO), SCENE_MONO.line_height]);
        }
        for o in &sh.travel {
            rects.push([sh.rect[0] + o[0], sh.rect[1] + o[1], sh.rect[2], sh.rect[3]]);
        }
    }
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for r in rects {
        lo = [lo[0].min(r[0]), lo[1].min(r[1])];
        hi = [hi[0].max(r[0] + r[2]), hi[1].max(r[1] + r[3])];
    }
    if lo[0] > hi[0] {
        return [0.0, 0.0, 1.0, 1.0];
    }
    let pad = 20.0;
    [lo[0] - pad, lo[1] - pad, hi[0] - lo[0] + 2.0 * pad, hi[1] - lo[1] + 2.0 * pad]
}

fn shape_name(s: &Scene, id: &str) -> String {
    if id.is_empty() {
        return "nothing".into();
    }
    s.shape(id).and_then(|x| x.props.text.clone()).unwrap_or_else(|| id.to_owned())
}

/// A beat's sentence: its own, else what its actions do.
fn beat_text(s: &Scene, k: usize) -> String {
    let b = &s.beats[k];
    b.say.clone().unwrap_or_else(|| {
        b.actions
            .iter()
            .map(|a| {
                if a.target.is_empty() {
                    format!("{} off", verb_word(a.verb))
                } else {
                    format!("{} [[{}]]", verb_word(a.verb), a.target)
                }
            })
            .collect::<Vec<_>>()
            .join(" · ")
    })
}

/// A beat's sentence with its links as plain names, for the step label.
fn plain(s: &Scene, body: &str) -> String {
    spans(body)
        .into_iter()
        .map(|x| match x {
            Span::Text(t) => t.to_owned(),
            Span::Link(id) => shape_name(s, id),
        })
        .collect()
}

/// A morph's new label as a name and, after a ` · `, what it now does.
fn morph_parts(into: &str) -> (String, Option<String>) {
    match into.split_once(" · ") {
        Some((a, b)) => (a.to_owned(), Some(b.to_owned())),
        None => (into.to_owned(), None),
    }
}

/// One plain sentence from the script: how many calls it adds, changes and removes, and the
/// boxes it adds, changes and removes.
pub(crate) fn conclusion(s: &Scene) -> String {
    let calls: Vec<Status> = s
        .shapes
        .iter()
        .filter(|x| matches!(x.kind, ShapeKind::Arrow | ShapeKind::Path))
        .map(|x| tone_of(x.props.tone))
        .collect();
    let count = |t: Status| calls.iter().filter(|x| **x == t).count();
    let mut parts = Vec::new();
    for (t, one, many) in [
        (Status::Added, "new call", "new calls"),
        (Status::Changed, "changed", "changed"),
        (Status::Removed, "removed", "removed"),
    ] {
        if count(t) > 0 {
            parts.push(plural(count(t), one, many));
        }
    }
    let mut said = if parts.is_empty() {
        "No call changes.".to_owned()
    } else {
        format!("{}.", list(&parts))
    };
    let boxes = |t: Status| -> Vec<&crate::schema::scene::SceneShape> {
        s.shapes
            .iter()
            .filter(|x| x.kind == ShapeKind::Box && tone_of(x.props.tone) == t)
            .collect()
    };
    let names = |t: Status| -> Vec<String> { boxes(t).iter().map(|x| shape_name(s, &x.id)).collect() };
    if !names(Status::Added).is_empty() {
        said += &format!(" New: {}.", list(&names(Status::Added)));
    }
    let changed: Vec<String> = boxes(Status::Changed)
        .iter()
        .map(|x| {
            let into = s
                .beats
                .iter()
                .flat_map(|b| &b.actions)
                .find(|a| a.verb == Verb::Morph && a.target == x.id)
                .and_then(|a| a.into.as_deref())
                .and_then(|i| morph_parts(i).1);
            match into {
                Some(d) => format!("{} ({d})", shape_name(s, &x.id)),
                None => shape_name(s, &x.id),
            }
        })
        .collect();
    if !changed.is_empty() {
        said += &format!(" Changed: {}.", list(&changed));
    }
    if !names(Status::Removed).is_empty() {
        said += &format!(" Removed: {}.", list(&names(Status::Removed)));
    }
    if let Some(c) = said.get(..1) {
        said = c.to_uppercase() + &said[1..];
    }
    said
}

/// Where each path's label goes: beside the middle of its longest stretch, on whichever side
/// is clear of boxes, text, tags, every line and the labels placed before it.
fn place_labels(shapes: &mut [Placed], size: [f32; 2]) {
    let mut taken: Vec<[f32; 4]> = Vec::new();
    let mut lines: Vec<[f32; 4]> = Vec::new();
    for sh in shapes.iter() {
        match sh.kind {
            ShapeKind::Box => {
                taken.push(grow(sh.rect, 4.0));
                if let Some((_, t)) = sh.tag {
                    taken.push(grow([sh.rect[0] + t[0], sh.rect[1] + t[1], t[2], t[3]], 2.0));
                }
            }
            ShapeKind::Text => taken.push(grow(sh.rect, 2.0)),
            ShapeKind::Path | ShapeKind::Arrow => {
                for w in sh.points.windows(2) {
                    let r = [
                        w[0][0].min(w[1][0]),
                        w[0][1].min(w[1][1]),
                        (w[0][0] - w[1][0]).abs(),
                        (w[0][1] - w[1][1]).abs(),
                    ];
                    lines.push(grow(r, 4.0));
                }
            }
            ShapeKind::Dot => {}
        }
    }
    for sh in shapes.iter_mut() {
        let Some(t) = sh.text.as_ref().filter(|_| matches!(sh.kind, ShapeKind::Path | ShapeKind::Arrow)) else {
            continue;
        };
        let (tw, th) = (measure(t, SCENE_MONO), SCENE_MONO.line_height);
        let mut segs: Vec<([f32; 2], [f32; 2])> = sh.points.windows(2).map(|w| (w[0], w[1])).collect();
        let len = |s: &([f32; 2], [f32; 2])| ((s.1[0] - s.0[0]).powi(2) + (s.1[1] - s.0[1]).powi(2)).sqrt();
        segs.sort_by(|a, b| len(b).total_cmp(&len(a)));
        let mut cands = Vec::new();
        for (a, b) in &segs {
            for f in [0.5, 0.35, 0.65] {
                let m = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f];
                if (b[1] - a[1]).abs() > (b[0] - a[0]).abs() {
                    cands.push([m[0] + 14.0, m[1] - th / 2.0]);
                    cands.push([m[0] - tw - 14.0, m[1] - th / 2.0]);
                } else {
                    cands.push([m[0] - tw / 2.0, m[1] - th - 6.0]);
                    cands.push([m[0] - tw / 2.0, m[1] + 6.0]);
                }
            }
        }
        let rect = |c: [f32; 2]| [c[0], c[1], tw, th];
        let inside = |r: [f32; 4]| r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= size[0] && r[1] + r[3] <= size[1];
        let at = cands
            .iter()
            .find(|c| {
                let r = rect(**c);
                inside(r) && !taken.iter().chain(&lines).any(|o| overlap(r, *o))
            })
            .or(cands.first())
            .copied();
        if let Some(c) = at {
            taken.push(rect(c));
        }
        sh.label_at = at;
    }
}

/// The script's actions in time order, each with its beat's time.
fn actions(s: &Scene) -> Vec<(f32, &Action)> {
    let mut actions: Vec<(f32, &Action)> = s.beats.iter().flat_map(|b| b.actions.iter().map(move |a| (b.t, a))).collect();
    actions.sort_by(|a, b| a.0.total_cmp(&b.0));
    actions
}

/// Builds the scene's shapes (in scene points) and the tracks its beats make, with focus and
/// camera keys for a figure `fsize` big that frames what the scene draws.
pub(crate) fn play(s: &Scene, fsize: [f32; 2]) -> (Played, FocusKeys, CameraKeys) {
    let shapes = shapes(s);
    let region = bounds(&shapes);
    let (fk, fo) = fit(region, fsize);
    let to_fig = |p: [f32; 2]| [fo[0] + p[0] * fk, fo[1] + p[1] * fk];
    let actions = actions(s);
    // A focus lights its target and whatever else its beat acts on, so a note the beat fades in
    // is not dimmed by the focus it explains.
    let mut focus = vec![(0.0, Vec::new())];
    for b in &s.beats {
        for a in b.actions.iter().filter(|a| a.verb == Verb::Focus) {
            let mut ids = Vec::new();
            if !a.target.is_empty() {
                ids.push(a.target.clone());
                ids.extend(
                    b.actions
                        .iter()
                        .filter(|o| o.verb != Verb::Focus && !o.target.is_empty())
                        .map(|o| o.target.clone()),
                );
            }
            focus.push((b.t, ids));
        }
    }
    focus.sort_by(|x, y| x.0.total_cmp(&y.0));
    let centre = [fsize[0] / 2.0, fsize[1] / 2.0];
    // An authored key frames the whole scene at zoom 1; the figure frames what it draws, so a
    // key that does not zoom in shows all of it.
    let mut cam: CameraKeys = s
        .camera
        .iter()
        .map(|c| {
            if c.zoom <= 1.0 {
                (c.t, centre, 1.0)
            } else {
                (c.t, to_fig(c.center), c.zoom)
            }
        })
        .collect();
    if cam.is_empty() {
        let view = Camera::new(fsize);
        cam.push((0.0, centre, 1.0));
        for (t, a) in actions.iter().filter(|(_, a)| a.verb == Verb::Focus) {
            let last = *cam.last().unwrap_or(&(0.0, [0.0; 2], 1.0));
            cam.push((*t, last.1, last.2));
            match shapes.iter().find(|p| p.id == a.target) {
                Some(p) => {
                    let (lo, hi) = (
                        to_fig([p.rect[0], p.rect[1]]),
                        to_fig([p.rect[0] + p.rect[2], p.rect[1] + p.rect[3]]),
                    );
                    let r = [lo[0], lo[1], hi[0] - lo[0], hi[1] - lo[1]];
                    cam.push((t + a.dur, [r[0] + r[2] / 2.0, r[1] + r[3] / 2.0], view.fit(r, 80.0).min(1.6)));
                }
                None => cam.push((t + a.dur, centre, 1.0)),
            }
        }
    }
    (Played { shapes, region }, focus, cam)
}

/// The scene's shapes in its own points, with the tracks its beats make and their labels placed.
fn shapes(s: &Scene) -> Vec<Placed> {
    let actions = actions(s);
    let mut shapes = Vec::new();
    for sh in &s.shapes {
        let mine: Vec<(f32, &Action)> = actions.iter().filter(|(_, a)| a.target == sh.id).copied().collect();
        let p = &sh.props;
        let points = p.points.clone();
        let tone = tone_of(p.tone);
        let rect = match sh.kind {
            ShapeKind::Box => [sh.at[0], sh.at[1], p.w.unwrap_or(160.0), p.h.unwrap_or(48.0)],
            ShapeKind::Text => {
                let font = text_font(tone);
                let w = p.text.as_ref().map_or(0.0, |t| measure(t, font) * 0.95 + 8.0);
                [sh.at[0], sh.at[1], w, font.line_height + 4.0]
            }
            ShapeKind::Dot => {
                let r = p.r.unwrap_or(6.0);
                [sh.at[0] - r, sh.at[1] - r, 2.0 * r, 2.0 * r]
            }
            ShapeKind::Path | ShapeKind::Arrow => {
                let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
                for q in &points {
                    lo = [lo[0].min(q[0]), lo[1].min(q[1])];
                    hi = [hi[0].max(q[0]), hi[1].max(q[1])];
                }
                [lo[0], lo[1], (hi[0] - lo[0]).max(1.0), (hi[1] - lo[1]).max(1.0)]
            }
        };
        let tag = (sh.kind == ShapeKind::Box).then(|| tag_word(tone)).flatten().map(|word| {
            let tw = measure(word, SCENE_TAG) + 16.0;
            (word, [rect[2] - tw - 10.0, -TAG_H / 2.0, tw, TAG_H])
        });
        let stroke = matches!(sh.kind, ShapeKind::Path | ShapeKind::Arrow);
        let first = mine.first().map(|(_, a)| a);
        let start = match first.map(|a| a.verb) {
            Some(Verb::Create) => 0.0,
            Some(Verb::Write) if !stroke => 0.0,
            Some(Verb::Fade) => first.and_then(|a| a.to).map_or(1.0, |v| v[0]),
            _ => 1.0,
        };
        let mut alpha = vec![(0.0, start)];
        let mut scale = vec![(0.0, 1.0)];
        let mut offset = vec![(0.0, [0.0, 0.0])];
        let mut travel = Vec::new();
        let mut reveal: Option<Vec<(f32, [f32; 4])>> = None;
        let mut draw = None;
        let mut label_alpha = vec![(0.0, 1.0)];
        let mut swap = None;
        let mut pulses = Vec::new();
        let mut here = [rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0];
        for (t, a) in &mine {
            let (t, d) = (*t, a.dur.max(0.01));
            match a.verb {
                Verb::Create => {
                    alpha.extend([(t, 0.0), (t + d, 1.0)]);
                    scale.extend([(t, 0.7), (t + d, 1.0)]);
                }
                Verb::Write if stroke => {
                    draw = Some(WriteOn::draw(t, d));
                    label_alpha = vec![(0.0, 0.0), (t + d * 0.6, 0.0), (t + d, 1.0)];
                }
                Verb::Write => {
                    alpha.extend([(t, 0.0), (t + 0.08, 1.0)]);
                    let r = rect;
                    reveal = Some(vec![(0.0, [r[0], r[1], 0.0, r[3]]), (t, [r[0], r[1], 0.0, r[3]]), (t + d, r)]);
                }
                Verb::Fade => {
                    let v = a.to.unwrap_or([0.0, 1.0]);
                    alpha.extend([(t, v[0]), (t + d, v[1])]);
                }
                Verb::Move => {
                    let to = a.to.unwrap_or(here);
                    let last = offset.last().map_or([0.0; 2], |x| x.1);
                    let next = [last[0] + to[0] - here[0], last[1] + to[1] - here[1]];
                    offset.extend([(t, last), (t + d, next)]);
                    travel.push(next);
                    here = to;
                }
                Verb::Transform => {
                    if let Some(v) = a.to {
                        let r = rect;
                        let to = [r[0], r[1], v[0], v[1]];
                        let mut keys = reveal.take().unwrap_or_else(|| vec![(0.0, r)]);
                        keys.extend([(t, keys.last().map_or(r, |x| x.1)), (t + d, to)]);
                        reveal = Some(keys);
                    }
                }
                Verb::Pulse => {
                    if points.len() >= 2 {
                        pulses.push(Pulse::new(points.clone(), t, d, false, 12.0));
                    }
                }
                Verb::Morph => {
                    swap = Some((Track::new(vec![(t, 1.0), (t + d, 0.0)]), Track::new(vec![(t, 0.0), (t + d, 1.0)])));
                }
                Verb::Focus => {}
            }
        }
        let fix = |mut v: Vec<(f32, f32)>| {
            v.sort_by(|a, b| a.0.total_cmp(&b.0));
            Track::new(v)
        };
        shapes.push(Placed {
            id: sh.id.clone(),
            kind: sh.kind,
            rect,
            points,
            text: p.text.clone(),
            morph: mine.iter().find_map(|(_, a)| a.into.clone()),
            tone,
            tag,
            label_at: None,
            alpha: fix(alpha),
            scale: fix(scale),
            offset: Track::new(offset),
            travel,
            reveal: reveal.map(Track::new),
            draw,
            label_alpha: fix(label_alpha),
            swap,
            pulses,
        });
    }
    place_labels(&mut shapes, s.size);
    shapes
}

/// How many lines `body` takes in a card whose sentence is `room` wide.
fn lines(s: &Scene, body: &str, room: f32) -> usize {
    tour::prose_lines(body, room, &|id| shape_name(s, id))
}

/// The takeaway on top, the figure as big as the room under it allows, the legend under the
/// figure, then a card per beat, three to a row. The figure is `aspect` wide for its height at
/// most.
pub(crate) fn frame(size: [f32; 2], s: &Scene, says: &str, aspect: f32) -> Frame {
    let w = size[0] - 2.0 * MARGIN;
    let says_lines = (measure(says, paint::STRONG) / (w - 24.0)).ceil().max(1.0);
    let says_h = says_lines * paint::STRONG.line_height + 12.0;
    let cw = (w - CARD_GAP * (CARD_COLS - 1) as f32) / CARD_COLS as f32;
    let room = cw - 2.0 * CARD_PAD - PILL - PILL_GAP;
    let heights: Vec<f32> = (0..s.beats.len())
        .map(|k| {
            let n = lines(s, &beat_text(s, k), room) as f32;
            2.0 * CARD_PAD + (n * (LINE_H + LINE_GAP) - LINE_GAP).max(PILL)
        })
        .collect();
    let row_h: Vec<f32> = heights.chunks(CARD_COLS).map(|c| c.iter().copied().fold(0.0, f32::max)).collect();
    let cards_h = row_h.iter().sum::<f32>() + CARD_GAP * row_h.len().saturating_sub(1) as f32;
    let top = MARGIN - 8.0;
    let fig_y = top + says_h + GAP;
    let room_h = size[1] - fig_y - GAP - LEGEND_H - GAP - cards_h - MARGIN * 0.5;
    let fh = (w / aspect.max(0.1)).min(room_h).max(120.0);
    let legend_y = fig_y + fh + GAP;
    let mut y = legend_y + LEGEND_H + GAP;
    let mut cards = Vec::new();
    for (r, h) in row_h.iter().enumerate() {
        for c in 0..CARD_COLS {
            if r * CARD_COLS + c < heights.len() {
                cards.push([MARGIN + c as f32 * (cw + CARD_GAP), y, cw, *h]);
            }
        }
        y += h + CARD_GAP;
    }
    Frame {
        says: [MARGIN, top, w, says_h],
        figure: [MARGIN, fig_y, w, fh],
        legend_y,
        cards,
    }
}

fn scene_chip(s: &Scene, ink: &DiagramStyle, id: &str) -> Chip {
    let tone = s.shape(id).map_or(Status::Same, |x| tone_of(x.props.tone));
    Chip {
        label: shape_name(s, id),
        colour: if tone == Status::Same {
            ink.muted
        } else {
            paint::tone(ink, tone).stroke
        },
        lights: vec![id.to_owned()],
    }
}

/// A dashed outline round a `w` × `h` box.
fn dashed_box(w: f32, h: f32, colour: Color) -> Shape {
    let ring = vec![[1.0, 1.0], [w - 1.0, 1.0], [w - 1.0, h - 1.0], [1.0, h - 1.0], [1.0, 1.0]];
    let mut sh = Shape::new();
    for d in rain_diagram::dashes(&ring, 7.0, 5.0) {
        sh = sh.stroke_with(d, false, 1.8, LineCap::Butt, colour);
    }
    sh
}

/// A box's label: its name, or after a morph the old name fading into the new name and what
/// it now does.
fn box_label(sh: &Placed, colour: Color, w: f32, h: f32) -> El {
    let name = sh.text.clone().unwrap_or_default();
    let mut label = paint::centred(name.clone(), SCENE_BOX, colour, w, h).absolute(0.0, 0.0);
    if sh.tone == Status::Removed {
        let tw = (measure(&name, SCENE_BOX) * 0.92).min(w - 16.0);
        label = label.child(
            el().absolute((w - tw) / 2.0, h / 2.0 - 0.75)
                .size(tw, 1.5)
                .fill(colour)
                .a11y_hidden(),
        );
    }
    let (Some((out, inn)), Some(into)) = (&sh.swap, &sh.morph) else {
        return label;
    };
    let (head, rest) = morph_parts(into);
    let mut next = col()
        .absolute(0.0, 0.0)
        .size(w, h)
        .center()
        .gap(1.0)
        .child(text(head).font(SCENE_BOX).color(colour).a11y_hidden());
    if let Some(r) = rest {
        next = next.child(text(r).font(SCENE_NOTE).color(colour).a11y_hidden());
    }
    el().absolute(0.0, 0.0)
        .size(w, h)
        .child(label.live_opacity(&out.live))
        .child(next.live_opacity(&inn.live))
        .a11y_hidden()
}

/// The scene drawn in scene points.
fn draw_scene(p: &Played, cx: &Cx, rig: &Rig, size: [f32; 2]) -> El {
    let ink = cx.ink;
    let mut world = el().key("flowchart-scene").size(size[0], size[1]);
    let full = |e: El| e.absolute(0.0, 0.0).size(size[0], size[1]);
    for sh in &p.shapes {
        let tone = paint::tone(ink, sh.tone);
        let [x, y, w, h] = sh.rect;
        let body: El = match sh.kind {
            ShapeKind::Box => {
                let said = match tag_word(sh.tone) {
                    Some(t) => format!("{}, {t}", sh.text.clone().unwrap_or_default()),
                    None => sh.text.clone().unwrap_or_default(),
                };
                let mut b = el()
                    .key(format!("flowchart-shape-{}", sh.id))
                    .absolute(x, y)
                    .size(w, h)
                    .radius(8.0)
                    .fill(tone.fill)
                    .live_scale(&sh.scale.live)
                    .live_opacity(&sh.alpha.live)
                    .role("img")
                    .label(said.clone())
                    .help(said);
                b = if sh.tone == Status::Removed {
                    b.child(canvas(dashed_box(w, h, tone.border)).absolute(0.0, 0.0).size(w, h).a11y_hidden())
                } else {
                    b.border(1.5, tone.border)
                };
                b = b.child(box_label(sh, tone.text, w, h));
                if let Some((word, [tx, ty, tw, th])) = sh.tag {
                    b = b.child(
                        paint::centred(word, SCENE_TAG, tone.text, tw, th)
                            .absolute(tx, ty)
                            .radius(th / 2.0)
                            .fill(ink.card)
                            .border(1.5, tone.border)
                            .a11y_hidden(),
                    );
                }
                if let Some(r) = &sh.reveal {
                    b = b.live_rect(&r.live);
                }
                rig.highlight.hover(b, vec![sh.id.clone()])
            }
            ShapeKind::Text => {
                let colour = if sh.tone == Status::Same { ink.foreground } else { tone.text };
                let mut t = el()
                    .key(format!("flowchart-shape-{}", sh.id))
                    .absolute(x, y)
                    .size(w, h)
                    .clip_rounded(0.0)
                    .live_opacity(&sh.alpha.live)
                    .child(
                        text(sh.text.clone().unwrap_or_default())
                            .font(text_font(sh.tone))
                            .color(colour)
                            .absolute(0.0, 0.0),
                    );
                if let Some(r) = &sh.reveal {
                    t = t.live_rect(&r.live);
                }
                t
            }
            ShapeKind::Dot => el()
                .key(format!("flowchart-shape-{}", sh.id))
                .absolute(x, y)
                .size(w, h)
                .radius(w / 2.0)
                .fill(tone.stroke)
                .border(2.0, ink.background)
                .live_offset(&sh.offset.live)
                .live_scale(&sh.scale.live)
                .live_opacity(&sh.alpha.live)
                .a11y_hidden(),
            ShapeKind::Path | ShapeKind::Arrow => draw_path(sh, ink, size),
        };
        let hovered = motion::fade(full(el()).child(body), &rig.highlight.opacity(&sh.id), cx.reduced);
        world = world.child(motion::fade(full(el()).child(hovered), &rig.focus.opacity(&sh.id), cx.reduced));
    }
    world
}

/// A call: a line in its status's colour (grey as before, a halo when changed, dashed and
/// crossed when removed), its head, its label and its pulses.
fn draw_path(sh: &Placed, ink: &DiagramStyle, size: [f32; 2]) -> El {
    let full = |e: El| e.absolute(0.0, 0.0).size(size[0], size[1]);
    let tone = paint::tone(ink, sh.tone);
    let colour = if sh.tone == Status::Same { ink.edge } else { tone.stroke };
    let width = if sh.tone == Status::Same { 2.0 } else { 2.5 };
    let mut shape = Shape::new();
    if sh.tone == Status::Changed {
        shape = shape.stroke_with(sh.points.clone(), false, 8.0, LineCap::Round, colour.alpha(0.28));
    }
    if sh.tone == Status::Removed {
        for d in rain_diagram::dashes(&sh.points, 8.0, 6.0) {
            shape = shape.stroke_with(d, false, width, LineCap::Round, colour);
        }
    } else {
        shape = shape.stroke_with(sh.points.clone(), false, width, LineCap::Round, colour);
    }
    if sh.kind == ShapeKind::Arrow && sh.points.len() >= 2 {
        let (a, b) = (sh.points[sh.points.len() - 2], sh.points[sh.points.len() - 1]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = (dx * dx + dy * dy).sqrt().max(1e-3);
        let (ux, uy) = (dx / l, dy / l);
        shape = shape.fill(
            vec![
                b,
                [b[0] - ux * 11.0 - uy * 5.5, b[1] - uy * 11.0 + ux * 5.5],
                [b[0] - ux * 11.0 + uy * 5.5, b[1] - uy * 11.0 - ux * 5.5],
            ],
            colour,
        );
    }
    let mut c = full(canvas(shape))
        .key(format!("flowchart-shape-{}", sh.id))
        .live_opacity(&sh.alpha.live)
        .a11y_hidden();
    if let Some(d) = &sh.draw {
        c = d.bind(c);
    }
    let mut g = full(el()).child(c);
    if sh.tone == Status::Removed {
        let mid = motion::along(&sh.points, 0.5);
        let cross = Shape::new()
            .line([6.0, 6.0], [14.0, 14.0], 2.0, LineCap::Round, ink.removed_text)
            .line([14.0, 6.0], [6.0, 14.0], 2.0, LineCap::Round, ink.removed_text);
        g = g.child(
            el().size(20.0, 20.0)
                .child(canvas(cross).absolute(0.0, 0.0).size(20.0, 20.0))
                .absolute(mid[0] - 10.0, mid[1] - 10.0)
                .radius(10.0)
                .fill(ink.removed_fill)
                .border(1.5, ink.removed_border)
                .live_opacity(&sh.label_alpha.live)
                .a11y_hidden(),
        );
    }
    if let (Some(t), Some(at)) = (&sh.text, sh.label_at) {
        let colour = if sh.tone == Status::Same { ink.muted } else { tone.text };
        g = g.child(
            text(t.clone())
                .font(SCENE_MONO)
                .color(colour)
                .absolute(at[0], at[1])
                .live_opacity(&sh.label_alpha.live)
                .a11y_hidden(),
        );
    }
    for pu in &sh.pulses {
        g = g.child(pu.bind(el().radius(6.0).fill(ink.foreground).border(2.0, colour).a11y_hidden()));
    }
    g
}

/// The key: each status the scene shows, in colour and in its non-colour mark.
fn legend(tones: &[Status], ink: &DiagramStyle) -> El {
    let mut r = row().gap(14.0).items_center();
    for &t in tones {
        let tone = paint::tone(ink, t);
        let (word, mark) = match t {
            Status::Added => ("new", tone.stroke),
            Status::Changed => ("changed", tone.stroke),
            Status::Removed => ("removed", tone.stroke),
            Status::Same => ("as before", ink.edge),
        };
        let line = if t == Status::Removed {
            row()
                .gap(3.0)
                .child(el().size(6.0, 2.5).radius(1.0).fill(mark))
                .child(el().size(6.0, 2.5).radius(1.0).fill(mark))
        } else if t == Status::Changed {
            row().child(
                el().size(15.0, 7.0)
                    .radius(3.5)
                    .fill(mark.alpha(0.28))
                    .child(el().absolute(0.0, 2.25).size(15.0, 2.5).fill(mark)),
            )
        } else {
            row().child(el().size(15.0, 2.5).radius(1.0).fill(mark))
        };
        let swatch = if t == Status::Removed {
            el().size(14.0, 12.0)
                .radius(3.0)
                .fill(tone.fill)
                .child(canvas(dashed_box(14.0, 12.0, tone.border)).absolute(0.0, 0.0).size(14.0, 12.0))
        } else {
            el().size(14.0, 12.0).radius(3.0).fill(tone.fill).border(1.0, tone.border)
        };
        r = r.child(
            row()
                .gap(6.0)
                .items_center()
                .child(swatch)
                .child(line)
                .child(text(word).font(SMALL).color(ink.muted))
                .a11y_hidden(),
        );
    }
    r
}

impl Style for Flowchart {
    fn id(&self) -> &'static str {
        "flowchart"
    }
    fn name(&self) -> &'static str {
        "Flowchart"
    }
    fn pitch(&self) -> &'static str {
        "A scene script played beat by beat: boxes appear, calls draw on, one value travels the path."
    }
    fn schemas(&self) -> &'static [SchemaId] {
        &[SchemaId::S]
    }
    fn how_to_read(&self) -> Option<&'static str> {
        Some(
            "The sentence on top is the takeaway. The figure is the change's path: boxes are \
             components, arrows are calls, each labelled with the call. A tag on a box says \
             whether it is new, changed or removed; green is new, amber changed, red removed, \
             and a removed box or call is also dashed and struck through. Play builds the path \
             one beat at a time: boxes appear, calls draw on, a dot carries the value along and \
             the camera moves in where it matters. Each beat's sentence is a card under the \
             figure, bright while it plays; press a card to jump to it, hover a name to find it.",
        )
    }

    fn rig(&self, cx: &Cx) -> Rig {
        let s = &cx.samples.s;
        let says = conclusion(s);
        let region = bounds(&shapes(s));
        let fr = frame(cx.size, s, &says, region[2] / region[3]);
        let fsize = [fr.figure[2], fr.figure[3]];
        let (scene, focus, cam) = play(s, fsize);
        let times: Vec<f32> = s.beats.iter().map(|b| b.t).collect();
        let paras: Vec<Para> = (0..s.beats.len())
            .zip(tour::para_alphas(&times))
            .map(|(k, alpha)| Para {
                t: times[k],
                title: plain(s, &beat_text(s, k)),
                alpha,
            })
            .collect();
        let mut tracks = Tracks::default();
        for p in &scene.shapes {
            for t in [&p.alpha, &p.scale, &p.label_alpha] {
                tracks.push(t.clone());
            }
            tracks.push(p.offset.clone());
            if let Some(t) = &p.reveal {
                tracks.push(t.clone());
            }
            if let Some(w) = &p.draw {
                tracks.push(w.clone());
            }
            if let Some((a, b)) = &p.swap {
                tracks.push(a.clone());
                tracks.push(b.clone());
            }
            for pu in &p.pulses {
                tracks.push(pu.clone());
            }
        }
        for p in &paras {
            tracks.push(p.alpha.clone());
        }
        let titles: Vec<(f32, String)> = paras.iter().map(|p| (p.t, p.title.clone())).collect();
        let mut tones = Vec::new();
        for t in [Status::Added, Status::Changed, Status::Removed] {
            if s.shapes.iter().any(|x| tone_of(x.props.tone) == t) {
                tones.push(t);
            }
        }
        let chart = Chart {
            frame: fr,
            paras,
            scene,
            says,
            tones,
        };
        let mut rig = Rig::new(chart, cx.size, s.duration() + 1.0);
        for (t, title) in titles {
            rig = rig.step(t, title);
        }
        rig.camera = Camera::keyed(fsize, cam);
        tracks.onto(&mut rig);
        rig.add(FocusTrack::new(rig.focus.clone(), focus));
        rig
    }

    fn view(&self, cx: &Cx, rig: &Rig) -> El {
        let Some(c) = rig.data::<Chart>() else { return el() };
        let ink = cx.ink;
        let s = &cx.samples.s;
        let fr = &c.frame;
        let [fx, fy, fw, fh] = fr.figure;

        let (k, o) = fit(c.scene.region, [fw, fh]);
        let picture = draw_scene(&c.scene, cx, rig, s.size).absolute(o[0], o[1]).origin(0.0, 0.0).scale(k);
        let world = rig.camera.bind(el().key("flowchart-world").size(fw, fh).child(picture), cx.reduced);
        let figure = el()
            .key("flowchart-figure")
            .absolute(fx, fy)
            .size(fw, fh)
            .radius(10.0)
            .fill(ink.card)
            .border(1.0, ink.card_border)
            .clip_rounded(10.0)
            .role("img")
            .label(format!("Flowchart: {}", s.title))
            .child(world);

        let mut stage = el()
            .key("flowchart")
            .size(cx.size[0], cx.size[1])
            .child(beats::conclusion(cx, "flowchart", &c.says, fr.says[2]).absolute(fr.says[0], fr.says[1]))
            .child(figure)
            .child(
                row()
                    .absolute(fx, fr.legend_y)
                    .size(fw, LEGEND_H)
                    .items_center()
                    .justify_end()
                    .child(legend(&c.tones, ink))
                    .a11y_hidden(),
            );
        for (i, para) in c.paras.iter().enumerate() {
            let [x, y, w, h] = fr.cards[i];
            let room = w - 2.0 * CARD_PAD - PILL - PILL_GAP;
            let body = tour::prose(&beat_text(s, i), rig, ink, &format!("flowchart-step-{i}"), room, &|id| {
                scene_chip(s, ink, id)
            });
            let card = row()
                .key(format!("flowchart-step-{i}"))
                .absolute(x, y)
                .size(w, h)
                .gap(PILL_GAP)
                .padding(CARD_PAD)
                .radius(8.0)
                .fill(ink.foreground.alpha(0.04))
                .border(1.0, ink.card_border)
                .cursor(Cursor::Pointer)
                .role("button")
                .label(format!("Go to step {}: {}", i + 1, para.title))
                .child(
                    paint::centred((i + 1).to_string(), MICRO, ink.background, PILL, PILL)
                        .radius(PILL / 2.0)
                        .fill(ink.foreground),
                )
                .child(col().w(room).child(body))
                .live_opacity(&para.alpha.live);
            stage = stage.child(cx.act(card, Act::Seek(para.t)));
        }
        stage
    }
}

#[cfg(test)]
mod tests {
    use rune::El;

    use super::*;
    use crate::Lab;
    use crate::schema::samples;
    use crate::style::registry;

    fn no_act(e: El, _: Act) -> El {
        e
    }

    fn lab(size: [f32; 2]) -> Lab {
        let mut lab = Lab::new();
        lab.select(registry().iter().position(|s| s.id() == "flowchart").unwrap());
        let _ = lab.canvas(&rune_kit::Kit::neutral(true), &DiagramStyle::dark(), size, &no_act);
        lab
    }

    #[test]
    fn the_scene_script_plays_as_written() {
        let s = &samples().s;
        let (p, focus, cam) = play(s, [960.0, 540.0]);
        assert_eq!(p.shapes.len(), s.shapes.len());
        let shape = |id: &str| p.shapes.iter().find(|x| x.id == id).unwrap();
        let end = s.duration();
        let form = shape("voucher-form");
        assert_eq!((form.alpha.at(0.0), form.alpha.at(end)), (0.0, 1.0), "created");
        let post = shape("post");
        let w = post.draw.as_ref().unwrap();
        assert_eq!((w.at(1.5), w.at(end)), ([0.0, 0.0], [0.0, 1.0]), "written on at 1.6 s");
        let dot = shape("amount");
        let off = dot.offset.at(end);
        let centre = [dot.rect[0] + dot.rect[2] / 2.0 + off[0], dot.rect[1] + dot.rect[3] / 2.0 + off[1]];
        assert_eq!(centre, [500.0, 420.0], "the amount ends where its last move sends it");
        assert_eq!(shape("rule").alpha.at(0.0), 0.0);
        assert!(!shape("sum").pulses.is_empty());
        let morph = shape("account-service").swap.as_ref().unwrap();
        assert_eq!((morph.0.at(end), morph.1.at(end)), (0.0, 1.0), "the new label replaces the old");
        let gone = shape("total-calculator");
        assert!(
            gone.alpha.at(end) < 0.5 && gone.alpha.at(11.0) == 1.0,
            "what goes steps back at the end"
        );
        assert!(
            focus
                .iter()
                .any(|(_, ids)| ids == &vec!["vouchers-controller".to_owned(), "rule".to_owned()]),
            "a focus keeps what its beat shows lit: {focus:?}"
        );
        assert_eq!(focus.last().map(|f| f.1.is_empty()), Some(true), "the last focus ends it");
        assert_eq!(cam.len(), s.camera.len());
        assert_eq!(
            (cam[0].1, cam[0].2),
            ([480.0, 270.0], 1.0),
            "the overview frames all the scene draws"
        );
        let (k, o) = fit(p.region, [960.0, 540.0]);
        let zoomed = cam.iter().find(|c| c.2 > 1.0).unwrap();
        assert_eq!(
            zoomed.1,
            [o[0] + 420.0 * k, o[1] + 160.0 * k],
            "a zoom key lands where it was written"
        );
        let r = p.region;
        assert!(
            r[0] > 0.0 && r[1] >= 0.0 && r[0] + r[2] < s.size[0] && r[1] + r[3] < s.size[1],
            "the figure frames what is drawn: {r:?}"
        );
    }

    #[test]
    fn labels_and_tags_keep_clear_of_boxes_lines_and_each_other() {
        let s = &samples().s;
        let (p, _, _) = play(s, [750.0, 420.0]);
        let mut marks: Vec<(String, [f32; 4])> = Vec::new();
        let mut lines: Vec<(String, [f32; 4])> = Vec::new();
        for sh in &p.shapes {
            match sh.kind {
                ShapeKind::Box => {
                    marks.push((sh.id.clone(), sh.rect));
                    if let Some((word, t)) = sh.tag {
                        assert_eq!(Some(word), tag_word(sh.tone));
                        marks.push((format!("{}-tag", sh.id), [sh.rect[0] + t[0], sh.rect[1] + t[1], t[2], t[3]]));
                    }
                }
                ShapeKind::Text => marks.push((sh.id.clone(), sh.rect)),
                ShapeKind::Path | ShapeKind::Arrow => {
                    for w in sh.points.windows(2) {
                        let r = [
                            w[0][0].min(w[1][0]) - 1.0,
                            w[0][1].min(w[1][1]) - 1.0,
                            (w[0][0] - w[1][0]).abs() + 2.0,
                            (w[0][1] - w[1][1]).abs() + 2.0,
                        ];
                        lines.push((sh.id.clone(), r));
                    }
                    let t = sh.text.as_deref().unwrap();
                    let at = sh.label_at.unwrap();
                    marks.push((
                        format!("{}-label", sh.id),
                        [at[0], at[1], measure(t, SCENE_MONO), SCENE_MONO.line_height],
                    ));
                }
                ShapeKind::Dot => {}
            }
        }
        for (i, (a, ra)) in marks.iter().enumerate() {
            for (b, rb) in &marks[i + 1..] {
                let own_tag = b.starts_with(a.as_str()) && b.ends_with("-tag");
                assert!(own_tag || !overlap(*ra, *rb), "{a} and {b} overlap");
            }
            if a.ends_with("-label") {
                for (l, rl) in &lines {
                    assert!(!overlap(*ra, *rl), "{a} sits on {l}");
                }
            }
        }
        let tags = p.shapes.iter().filter(|x| x.tag.is_some()).count();
        assert_eq!(
            tags,
            s.shapes.iter().filter(|x| x.kind == ShapeKind::Box).count(),
            "every box here is new, changed or removed"
        );
    }

    #[test]
    fn the_frame_fits_the_canvas_and_nothing_in_it_overlaps() {
        let s = &samples().s;
        let says = conclusion(s);
        let region = bounds(&shapes(s));
        let aspect = region[2] / region[3];
        for size in [[798.0, 800.0], [800.0, 760.0], [1100.0, 700.0], [1000.0, 680.0]] {
            let f = frame(size, s, &says, aspect);
            assert_eq!(f.cards.len(), s.beats.len());
            let legend = [f.figure[0], f.legend_y, f.figure[2], LEGEND_H];
            let mut all = vec![f.says, f.figure, legend];
            all.extend(f.cards.iter().copied());
            for (i, r) in all.iter().enumerate() {
                assert!(
                    r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= size[0] + 0.5 && r[1] + r[3] <= size[1],
                    "{i} fits in {size:?}: {r:?}"
                );
                for q in &all[i + 1..] {
                    assert!(!overlap(*r, *q), "{r:?} and {q:?} overlap in {size:?}");
                }
            }
            let (k, _) = fit(region, [f.figure[2], f.figure[3]]);
            assert!(k * SCENE_NOTE.size >= 9.5, "readable at {size:?}: scale {k}");
        }
        let f = frame([798.0, 800.0], s, &says, aspect);
        let (k, _) = fit(region, [f.figure[2], f.figure[3]]);
        assert!(
            k * SCENE_NOTE.size >= 11.0,
            "at 1440 × 900 the scene's smallest type is 11 points or more"
        );
    }

    #[test]
    fn the_takeaway_comes_from_the_script() {
        assert_eq!(
            conclusion(&samples().s),
            "5 new calls and 1 removed. New: VoucherFormComponent, VouchersController, VoucherService \
             and Vouchers table. Changed: AccountService (total counts vouchers). Removed: TotalCalculator."
        );
    }

    #[test]
    fn one_card_is_bright_at_a_time_and_steps_follow_the_beats() {
        let lab = lab([800.0, 760.0]);
        let rig = lab.rig().unwrap();
        let c = rig.data::<Chart>().unwrap();
        let s = &samples().s;
        assert_eq!(rig.steps.len(), s.beats.len());
        assert_eq!(c.tones, [Status::Added, Status::Changed, Status::Removed]);
        assert!(!c.paras[2].title.contains("[["), "step labels name the shapes");
        for (k, p) in c.paras.iter().enumerate() {
            let t = p.t + 0.1;
            assert_eq!(p.alpha.at(t), 1.0, "beat {k} is the one being read");
            for (j, q) in c.paras.iter().enumerate().filter(|(j, _)| *j != k) {
                assert!(q.alpha.at(t) < 0.5, "beat {j} steps back while {k} plays");
            }
        }
        let ats: Vec<f32> = rig.steps.iter().map(|s| s.at).collect();
        assert!(ats.windows(2).all(|w| w[0] < w[1]) && ats.iter().all(|t| *t < rig.duration));
    }

    #[test]
    fn the_scene_holds_its_values_for_a_time() {
        let mut lab = lab([800.0, 760.0]);
        let rig = lab.rig().unwrap();
        let c = rig.data::<Chart>().unwrap();
        let dot = &c.scene.shapes.iter().find(|x| x.id == "amount").unwrap().offset;
        lab.seek_frac(0.35);
        let a = (dot.live.get(), rig.camera.scale.get(), rig.focus.current());
        lab.seek_frac(0.95);
        lab.seek_frac(0.35);
        assert_eq!((dot.live.get(), rig.camera.scale.get(), rig.focus.current()), a);
        assert!(a.1 > 1.0, "the camera is in on the controller a third of the way in");
    }
}
