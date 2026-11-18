//! Style 5, the differential icicle (after Brendan Gregg's differential flame graphs), for
//! checking performance on a big change or one query before and after: the call tree hangs from
//! the top, each frame as wide as the time under it. Play morphs every frame from its width
//! before the change to its width after, warming the ones that take longer to red and cooling
//! the ones that take less to blue. A sentence on top says where the time went, paired bars
//! below compare the biggest moves before and after, and tabs switch between the sample's call
//! trees. Pressing a frame zooms into it: its range fills the width and the rest rescales.

use std::cell::RefCell;
use std::collections::HashMap;

use rune::{Color, El, Live, Spring, el, row, text};

use super::beats;
use super::paint::{self, LABEL, MICRO, SMALL, STRONG};
use crate::motion::{self, Animate, FocusTrack, Lerp, Rig, Track, ease, window};
use crate::schema::calls::{CallFrame, CallTrees};
use crate::schema::{SchemaId, Status};
use crate::style::{Act, Cx, Style};

pub struct Icicle;

const MARGIN: f32 = 24.0;
/// Above the frames: the conclusion, the tabs, the zoom trail and the time axis.
const SAYS: f32 = 46.0;
const TABS: f32 = 30.0;
const TRAIL: f32 = 20.0;
const HEAD: f32 = SAYS + TABS + TRAIL + 30.0;
/// Below them: the legend and the paired bars of the biggest moves.
const FOOT: f32 = 190.0;
const MOVES: usize = 5;
const NAME_W: f32 = 230.0;
const DELTA_W: f32 = 80.0;
const BAR_H: f32 = 7.0;
const MORPH_AT: f32 = 0.6;
const MORPH: f32 = 1.6;
const FOCUS_AT: f32 = 2.6;
const END: f32 = 3.8;
/// How a zoom rescales the frames.
const ZOOM: Spring = Spring::new(0.45, 0.0);

thread_local! {
    /// Each frame's bound rect, kept across the rigs a zoom rebuilds, so the renderer springs
    /// the frames from where they were to where the new zoom puts them.
    static RECTS: RefCell<HashMap<String, Live<[f32; 4]>>> = RefCell::new(HashMap::new());
}

fn rect_live(id: &str, first: [f32; 4]) -> Live<[f32; 4]> {
    RECTS.with(|m| m.borrow_mut().entry(id.to_owned()).or_insert_with(|| Live::new(first)).clone())
}

/// One frame placed for a zoom.
pub(crate) struct Frame {
    pub id: String,
    pub name: String,
    pub parent: Option<usize>,
    pub depth: usize,
    /// Its rect before and after, on screen under the zoom.
    pub before: [f32; 4],
    pub after: [f32; 4],
    /// Its work and its callees', before and after.
    pub total: (f32, f32),
    pub status: Status,
    /// -1 (all of its work gone) to 1 (all of it new).
    pub heat: f32,
    pub rect: FrameRect,
    pub fill: Track<Color>,
}

/// A frame's rect morphing from before to after in the morph window.
#[derive(Clone)]
pub(crate) struct FrameRect {
    pub live: Live<[f32; 4]>,
    before: [f32; 4],
    after: [f32; 4],
}

impl FrameRect {
    pub fn at(&self, t: f32) -> [f32; 4] {
        <[f32; 4]>::lerp(self.before, self.after, ease(window(t, MORPH_AT, MORPH)))
    }
}

impl Animate for FrameRect {
    fn publish(&self, t: f32) {
        self.live.set(self.at(t));
    }
}

pub(crate) struct Tick {
    pub ms: f32,
    pub x: f32,
}

/// The icicle laid out for one canvas size and zoom.
pub(crate) struct Layout {
    pub frames: Vec<Frame>,
    pub focus: usize,
    pub row: f32,
    pub ticks: Vec<Tick>,
    /// The after labels' opacity (the deltas, the before mark).
    pub reveal: Track<f32>,
    /// Where the time went, worked out from the call tree.
    pub says: String,
    /// The call trees to switch between: a label and the pick that shows each.
    pub tabs: Vec<(String, Option<String>)>,
    pub tab: usize,
}

/// The call trees in `c`: the change's own first, then its profiles, with their tab labels.
pub(crate) fn trees(c: &CallTrees) -> Vec<(String, &CallFrame)> {
    c.calls
        .iter()
        .map(|f| (f.name.clone(), f))
        .chain(c.profiles.iter().map(|p| (p.title.clone(), &p.calls)))
        .collect()
}

/// The tree a pick is in (a frame id), and its index; the first when nothing is picked.
fn tree_of<'a>(trees: &[(String, &'a CallFrame)], pick: Option<&str>) -> Option<(usize, &'a CallFrame)> {
    let i = pick
        .and_then(|p| trees.iter().position(|(_, t)| t.walk().iter().any(|(_, f)| f.id == p)))
        .unwrap_or(0);
    trees.get(i).map(|(_, t)| (i, *t))
}

fn heat_of(total: (f32, f32)) -> f32 {
    let m = total.0.max(total.1);
    if m <= 0.0 { 0.0 } else { (total.1 - total.0) / m }
}

/// The colour a frame ends on: red for more work, blue for less, stronger the bigger the move.
fn heat_fill(ink: &rain_diagram::DiagramStyle, heat: f32) -> Color {
    if heat.abs() < 0.02 {
        return ink.neutral_fill;
    }
    let hue = if heat > 0.0 { ink.removed } else { ink.viewed };
    hue.alpha(0.16 + 0.42 * heat.abs())
}

fn nice_step(span: f32) -> f32 {
    let raw = (span / 6.0).max(0.1);
    let p = 10f32.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0].iter().map(|k| k * p).find(|s| *s >= raw).unwrap_or(10.0 * p)
}

pub(crate) fn layout(calls: &CallFrame, ink: &rain_diagram::DiagramStyle, size: [f32; 2], pick: Option<&str>) -> Layout {
    let walk = calls.walk();
    let totals = calls.total();
    let m = totals.0.max(totals.1).max(1e-3);
    // Unit positions: x and width in 0..1 of the larger total, before and after.
    let mut unit: Vec<([f32; 2], [f32; 2])> = vec![([0.0; 2], [0.0; 2]); walk.len()];
    let mut parents: Vec<Option<usize>> = vec![None; walk.len()];
    let mut stack: Vec<(usize, f32, f32)> = Vec::new();
    for (i, (d, f)) in walk.iter().enumerate() {
        stack.truncate(*d);
        let (xb, xa) = match stack.last_mut() {
            Some(top) => {
                parents[i] = Some(top.0);
                let at = (top.1, top.2);
                let t = f.total();
                top.1 += t.0 / m;
                top.2 += t.1 / m;
                at
            }
            None => (0.0, 0.0),
        };
        let t = f.total();
        unit[i] = ([xb, t.0 / m], [xa, t.1 / m]);
        stack.push((i, xb, xa));
    }
    let focus = pick.and_then(|p| walk.iter().position(|(_, f)| f.id == p)).unwrap_or(0);
    // The zoom spans the focused frame before and after, so a call that shrank keeps its old
    // width in view; a side where it doesn't exist is left out.
    let (fu_b, fu_a) = unit[focus];
    let (fx, fw) = match (fu_b[1] > 1e-4, fu_a[1] > 1e-4) {
        (true, true) => {
            let x0 = fu_b[0].min(fu_a[0]);
            (x0, (fu_b[0] + fu_b[1]).max(fu_a[0] + fu_a[1]) - x0)
        }
        (false, true) => (fu_a[0], fu_a[1]),
        _ => (fu_b[0], fu_b[1].max(1e-4)),
    };
    let depth = walk.iter().map(|(d, _)| *d).max().unwrap_or(0) + 1;
    let width = (size[0] - 2.0 * MARGIN).max(1.0);
    let row = ((size[1] - 2.0 * MARGIN - HEAD - FOOT) / depth as f32).floor().clamp(24.0, 52.0);
    let top = MARGIN + HEAD;
    let map = |u: [f32; 2], d: usize| -> [f32; 4] {
        let x0 = (MARGIN + (u[0] - fx) / fw * width).clamp(MARGIN, MARGIN + width);
        let x1 = (MARGIN + (u[0] + u[1] - fx) / fw * width).clamp(MARGIN, MARGIN + width);
        let w = (x1 - x0 - 1.0).max(0.0);
        [x0, top + d as f32 * row, w, row - 1.0]
    };
    let frames = walk
        .iter()
        .enumerate()
        .map(|(i, (d, f))| {
            let (before, after) = (map(unit[i].0, *d), map(unit[i].1, *d));
            let total = f.total();
            let heat = heat_of(total);
            let rect = FrameRect {
                live: rect_live(&f.id, before),
                before,
                after,
            };
            let start = paint::tone(ink, Status::Same).fill;
            Frame {
                id: f.id.clone(),
                name: f.name.clone(),
                parent: parents[i],
                depth: *d,
                before,
                after,
                total,
                status: f.status(),
                heat,
                rect,
                fill: Track::new(vec![(MORPH_AT, start), (MORPH_AT + MORPH, heat_fill(ink, heat))]),
            }
        })
        .collect();
    let span = fw * m;
    let step = nice_step(span);
    let mut ticks = Vec::new();
    let mut v = ((fx * m) / step).ceil() * step;
    while v <= (fx + fw) * m + 1e-3 {
        ticks.push(Tick {
            ms: v,
            x: MARGIN + (v / m - fx) / fw * width,
        });
        v += step;
    }
    let shown = MORPH_AT + MORPH * 0.7;
    Layout {
        frames,
        focus,
        row,
        ticks,
        reveal: Track::new(vec![(shown, 0.0), (MORPH_AT + MORPH, 1.0)]),
        says: conclusion(calls),
        tabs: Vec::new(),
        tab: 0,
    }
}

/// `RefreshTotal()` for `AccountService.RefreshTotal()`; other names as they are.
fn method(name: &str) -> &str {
    match name.split_once('.') {
        Some((_, m)) if m.ends_with(')') => m,
        _ => name,
    }
}

/// A call's change: `+24 ms`, `new, +24 ms` or `gone, −28 ms`.
fn moved(f: &CallFrame) -> String {
    let (b, a) = f.total();
    let tag = match f.status() {
        Status::Added => "new, ",
        Status::Removed => "gone, ",
        _ => "",
    };
    format!("{tag}{}", signed(a - b))
}

/// Where the time went: the totals, then the two calls under the root that moved most, each
/// with the calls under it that explain it (past any call that only passes its time through).
fn conclusion(root: &CallFrame) -> String {
    let d = |f: &CallFrame| {
        let (b, a) = f.total();
        a - b
    };
    let (b, a) = root.total();
    let mut out = format!("{}: {} → {} per request ({}).", root.name, ms(b), ms(a), signed(a - b));
    let mut entries: Vec<&CallFrame> = root.children.iter().filter(|f| d(f).abs() > 0.05).collect();
    entries.sort_by(|x, y| d(y).abs().total_cmp(&d(x).abs()));
    let mut parts = Vec::new();
    for e in entries.iter().take(2) {
        let mut at = *e;
        while let Some(c) = at.children.iter().max_by(|x, y| d(x).abs().total_cmp(&d(y).abs())) {
            if d(c).abs() < 0.8 * d(at).abs() || c.children.is_empty() {
                break;
            }
            at = c;
        }
        let mut why: Vec<&CallFrame> = at.children.iter().filter(|c| d(c).abs() >= 0.2 * d(e).abs()).collect();
        let along = |c: &CallFrame| d(c) * d(e) > 0.0;
        why.sort_by(|x, y| d(y).abs().total_cmp(&d(x).abs()).then(along(y).cmp(&along(x))));
        let head = format!("{} ({})", method(&e.name), moved(e));
        let said: Vec<String> = why
            .iter()
            .take(3)
            .map(|c| format!("{} {}", method(&c.name), signed(d(c))))
            .collect();
        parts.push(match said.first() {
            None => head,
            Some(one) if why.iter().all(|c| along(c)) && d(why[0]).abs() > 0.5 * d(e).abs() => {
                format!("{head}, mostly {one}")
            }
            Some(_) => format!("{head}: {}", said.join(", ")),
        });
    }
    if !parts.is_empty() {
        out.push(' ');
        out.push_str(&parts.join("; "));
        out.push('.');
    }
    out
}

/// The name, or just the method when the class and method don't fit in `w`.
fn fit_name(name: &str, w: f32) -> String {
    let fits = |s: &str| s.chars().count() as f32 * 6.6 + 14.0 <= w;
    match name.split_once('.') {
        Some((_, method)) if !fits(name) && method.ends_with(')') => method.to_owned(),
        _ => name.to_owned(),
    }
}

fn ms(v: f32) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{v:.0} ms")
    } else {
        format!("{v:.1} ms")
    }
}

fn signed(d: f32) -> String {
    if d >= 0.0 {
        format!("+{}", ms(d))
    } else {
        format!("−{}", ms(-d))
    }
}

impl Style for Icicle {
    fn id(&self) -> &'static str {
        "icicle"
    }
    fn name(&self) -> &'static str {
        "Differential icicle"
    }
    fn pitch(&self) -> &'static str {
        "Performance before and after: a big change's call tree, or one query's, red where time grew."
    }
    fn schemas(&self) -> &'static [SchemaId] {
        &[SchemaId::C]
    }
    fn how_to_read(&self) -> Option<&'static str> {
        Some(
            "For checking performance: on a big change, or one query before and after (the tabs \
             switch call trees). The sentence on top says where the time went. Each bar is a \
             call; the bars under it are what it calls, and its width is the time under it \
             (milliseconds per request), on one scale for before and after. Play morphs every \
             bar from its width before the change to its width after: red bars take longer than \
             before (solid red is new), blue ones take less (they shrink away when gone), grey \
             ones are unchanged. The paired bars below compare the biggest moves at a glance. \
             Press a bar to zoom into it; press the top bar to zoom back out.",
        )
    }

    fn rig(&self, cx: &Cx) -> Rig {
        let trees = trees(&cx.samples.c);
        let Some((tab, calls)) = tree_of(&trees, cx.pick) else {
            return Rig::new((), cx.size, 1.0);
        };
        let mut l = layout(calls, cx.ink, cx.size, cx.pick);
        l.tab = tab;
        l.tabs = trees
            .iter()
            .enumerate()
            .map(|(i, (label, t))| (label.clone(), (i > 0).then(|| t.id.clone())))
            .collect();
        let hot: Vec<String> = l.frames.iter().filter(|f| f.heat.abs() > 0.02).map(|f| f.id.clone()).collect();
        let mut tracks: Vec<Box<dyn Animate>> = Vec::new();
        for f in &l.frames {
            tracks.push(Box::new(f.rect.clone()));
            tracks.push(Box::new(f.fill.clone()));
        }
        tracks.push(Box::new(l.reveal.clone()));
        let mut rig = Rig::new(l, cx.size, END)
            .step(0.0, "Before the change")
            .step(MORPH_AT, "The work moves")
            .step(FOCUS_AT, "Where it moved");
        rig.add(FocusTrack::new(rig.focus.clone(), vec![(0.0, Vec::new()), (FOCUS_AT, hot)]));
        for t in tracks {
            rig.add(Boxed(t));
        }
        rig
    }

    fn view(&self, cx: &Cx, rig: &Rig) -> El {
        let Some(l) = rig.data::<Layout>() else {
            return paint::centred("This sample has no call tree", LABEL, cx.ink.muted, cx.size[0], cx.size[1]);
        };
        let ink = cx.ink;
        let mut world = el()
            .key("icicle")
            .size(cx.size[0], cx.size[1])
            .clip_rounded(0.0)
            .role("tree")
            .label("Differential icicle of the call tree");
        world = world
            .child(beats::conclusion(cx, "icicle", &l.says, cx.size[0] - 2.0 * MARGIN).absolute(MARGIN, MARGIN - 8.0))
            .child(tabs(cx, l).absolute(MARGIN, MARGIN + SAYS))
            .child(header(cx, l).absolute(MARGIN, MARGIN + SAYS + TABS));
        for t in &l.ticks {
            world = world
                .child(
                    el().key(format!("icicle-tick-{}", t.ms))
                        .absolute(t.x, MARGIN + HEAD - 10.0)
                        .size(1.0, 6.0)
                        .fill(ink.edge)
                        .motion(ZOOM)
                        .a11y_hidden(),
                )
                .child(
                    text(ms(t.ms))
                        .key(format!("icicle-tick-label-{}", t.ms))
                        .font(MICRO)
                        .color(ink.muted)
                        .absolute(
                            if t.x > cx.size[0] - MARGIN - 40.0 { t.x - 40.0 } else { t.x + 3.0 },
                            MARGIN + HEAD - 24.0,
                        )
                        .motion(ZOOM)
                        .a11y_hidden(),
                );
        }
        for (i, f) in l.frames.iter().enumerate() {
            world = world.child(frame_el(cx, rig, l, i, f));
        }
        let depth = l.frames.iter().map(|f| f.depth).max().unwrap_or(0) + 1;
        let bottom = MARGIN + HEAD + depth as f32 * l.row;
        if let Some(root) = l.frames.first() {
            let x = root.before[0] + root.before[2];
            let shrank = root.after[2] + 1.0 < root.before[2];
            if x > MARGIN + 40.0 && (shrank || x < cx.size[0] - MARGIN - 4.0) {
                let h = bottom - (MARGIN + HEAD) + 20.0;
                // Near the right edge the label sits left of the line.
                let left = x > cx.size[0] - MARGIN - 90.0;
                let (at, line, label) = if left { (x - 90.0, 88.5, 0.0) } else { (x, 0.0, 5.0) };
                world = world.child(
                    el().key("icicle-before-mark")
                        .absolute(at, MARGIN + HEAD - 4.0)
                        .size(90.0, h)
                        .motion(ZOOM)
                        .live_opacity(&l.reveal.live)
                        .a11y_hidden()
                        .child(el().absolute(line, 0.0).size(1.5, h).fill(ink.foreground.alpha(0.55)))
                        .child(
                            text(format!("before, {}", ms(root.total.0)))
                                .font(MICRO)
                                .color(ink.foreground)
                                .absolute(label, h - 13.0),
                        ),
                );
            }
        }
        world.child(footer(cx, l).absolute(MARGIN, bottom + 26.0))
    }
}

/// Puts a boxed track on the timeline.
struct Boxed(Box<dyn Animate>);

impl Animate for Boxed {
    fn publish(&self, t: f32) {
        self.0.publish(t);
    }
}

fn header(cx: &Cx, l: &Layout) -> El {
    let ink = cx.ink;
    let mut trail: Vec<&str> = Vec::new();
    let mut at = Some(l.focus);
    while let Some(i) = at {
        trail.push(&l.frames[i].name);
        at = l.frames[i].parent;
    }
    trail.reverse();
    row()
        .key("icicle-trail")
        .h(TRAIL)
        .gap(10.0)
        .items_center()
        .child(text(trail.join("  ›  ")).font(STRONG).color(ink.foreground))
        .child(
            text(if l.focus == 0 {
                "· bar width is time per request, callees under it · press a bar to zoom"
            } else {
                "· press the top bar to zoom back out"
            })
            .font(SMALL)
            .color(ink.muted),
        )
}

/// One tab per call tree in the sample; pressing one shows it.
fn tabs(cx: &Cx, l: &Layout) -> El {
    let ink = cx.ink;
    let mut r = row().key("icicle-tabs").h(TABS - 6.0).gap(6.0).items_center();
    for (i, (label, pick)) in l.tabs.iter().enumerate() {
        let on = i == l.tab;
        let tab = row()
            .key(format!("icicle-tab-{i}"))
            .role("tab")
            .label(label.clone())
            .h(TABS - 6.0)
            .px(10.0)
            .center()
            .radius(6.0)
            .fill(if on { ink.card } else { ink.neutral_fill.alpha(0.0) })
            .border(1.0, if on { ink.foreground.alpha(0.5) } else { ink.card_border })
            .child(
                text(label.clone())
                    .font(SMALL)
                    .color(if on { ink.foreground } else { ink.muted })
                    .a11y_hidden(),
            );
        r = r.child(if on { tab } else { cx.act(tab, Act::Pick(pick.clone())) });
    }
    r
}

fn frame_el(cx: &Cx, rig: &Rig, l: &Layout, i: usize, f: &Frame) -> El {
    let ink = cx.ink;
    let rest = if f.after[2] > 0.0 { f.after } else { f.before };
    let target = if i == l.focus {
        f.parent.map(|p| l.frames[p].id.clone())
    } else {
        Some(f.id.clone())
    };
    // The root of the first tree is no pick at all; another tree's root keeps that tree on.
    let home = l.tabs.get(l.tab).and_then(|t| t.1.clone());
    let target = match (target, l.frames.first()) {
        (Some(id), Some(r)) if r.id != id => Some(id),
        _ => home,
    };
    let d = f.total.1 - f.total.0;
    let says = match f.status {
        Status::Added => format!("{}: new, {}", f.name, ms(f.total.1)),
        Status::Removed => format!("{}: gone, was {}", f.name, ms(f.total.0)),
        _ => format!("{}: {} → {} ({})", f.name, ms(f.total.0), ms(f.total.1), signed(d)),
    };
    let delta_colour = if d > 0.0 { ink.removed_text } else { ink.viewed };
    let mut bar = row()
        .key(format!("icicle-frame-{}", f.id))
        .role("treeitem")
        .label(says.clone())
        .help(says)
        .absolute(rest[0], rest[1])
        .size(rest[2].max(1.0), rest[3])
        .px(6.0)
        .gap(6.0)
        .items_center()
        .radius(3.0)
        .clip_rounded(3.0)
        .fill(paint::tone(ink, Status::Same).fill)
        .live_fill(&f.fill.live)
        .child(
            text(fit_name(&f.name, rest[2]))
                .font(LABEL)
                .color(ink.foreground)
                .truncate()
                .shrink()
                .a11y_hidden(),
        );
    if f.status != Status::Same && rest[2] > 90.0 {
        bar = bar.child(
            text(signed(d))
                .font(MICRO)
                .color(delta_colour)
                .live_opacity(&l.reveal.live)
                .a11y_hidden(),
        );
    }
    bar = if cx.reduced {
        bar.live_rect(&f.rect.live)
    } else {
        bar.live_rect(f.rect.live.smooth(ZOOM))
    };
    if i == l.focus && i != 0 {
        bar = bar.border(1.5, ink.foreground.alpha(0.6));
    }
    let bar = motion::fade(bar, &rig.focus.opacity(&f.id), cx.reduced);
    cx.act(bar, Act::Pick(target))
}

/// The calls whose time moved most (time under each), biggest first; a call that only passes
/// one callee's change through gives its place to that callee.
pub(crate) fn biggest(l: &Layout) -> Vec<&Frame> {
    let d = |f: &Frame| f.total.1 - f.total.0;
    let passes = |i: usize| {
        let f = &l.frames[i];
        l.frames
            .iter()
            .any(|c| c.parent == Some(i) && (d(c) - d(f)).abs() <= 0.05 * d(f).abs())
    };
    let mut moves: Vec<&Frame> = l
        .frames
        .iter()
        .enumerate()
        .filter(|(i, f)| f.depth > 0 && d(f).abs() > 0.05 && !passes(*i))
        .map(|(_, f)| f)
        .collect();
    moves.sort_by(|a, b| d(b).abs().total_cmp(&d(a).abs()).then(a.depth.cmp(&b.depth)));
    moves.truncate(MOVES);
    moves
}

/// The legend, then the biggest moves as paired bars on one scale: before above, after below.
fn footer(cx: &Cx, l: &Layout) -> El {
    let ink = cx.ink;
    let grey = ink.muted.alpha(0.55);
    let legend = row()
        .gap(14.0)
        .items_center()
        .child(paint::swatch(
            "takes longer (solid: new)",
            heat_fill(ink, 0.8),
            ink.removed_border,
            ink,
        ))
        .child(paint::swatch(
            "takes less (gone: shrinks away)",
            heat_fill(ink, -0.8),
            ink.viewed,
            ink,
        ))
        .child(paint::swatch("unchanged", ink.neutral_fill, ink.card_border, ink));
    let moves = biggest(l);
    let scale = moves.iter().map(|f| f.total.0.max(f.total.1)).fold(1e-3, f32::max);
    let room = (cx.size[0] - 2.0 * MARGIN - NAME_W - DELTA_W - 20.0).max(60.0);
    let mut list = rune::col().key("icicle-moves").gap(6.0).child(
        row()
            .gap(8.0)
            .items_center()
            .child(text("Biggest moves, time under each call:").font(SMALL).color(ink.muted))
            .child(el().size(14.0, BAR_H).radius(2.0).fill(grey).a11y_hidden())
            .child(text("before").font(SMALL).color(ink.muted))
            .child(el().size(14.0, BAR_H).radius(2.0).fill(ink.removed).a11y_hidden())
            .child(el().size(14.0, BAR_H).radius(2.0).fill(ink.viewed).a11y_hidden())
            .child(text("after").font(SMALL).color(ink.muted)),
    );
    for f in moves {
        let d = f.total.1 - f.total.0;
        let hue = if d > 0.0 { ink.removed } else { ink.viewed };
        let bar = |v: f32, c| {
            el().w((v / scale * room).max(if v > 0.0 { 2.0 } else { 0.0 }))
                .h(BAR_H)
                .radius(2.0)
                .fill(c)
                .a11y_hidden()
        };
        let tag = match f.status {
            Status::Added => "new ",
            Status::Removed => "gone ",
            _ => "",
        };
        let said = format!("{}: {} → {}", f.name, ms(f.total.0), ms(f.total.1));
        list = list.child(
            row()
                .key(format!("icicle-move-{}", f.id))
                .role("img")
                .label(said.clone())
                .help(said)
                .gap(10.0)
                .items_center()
                .child(
                    text(f.name.clone())
                        .font(SMALL)
                        .color(ink.foreground)
                        .truncate()
                        .w(NAME_W)
                        .a11y_hidden(),
                )
                .child(
                    rune::col()
                        .w(room)
                        .gap(2.0)
                        .child(bar(f.total.0, grey))
                        .child(bar(f.total.1, hue))
                        .a11y_hidden(),
                )
                .child(
                    text(format!("{tag}{}", signed(d)))
                        .font(SMALL)
                        .color(if d > 0.0 { ink.removed_text } else { ink.viewed })
                        .w(DELTA_W)
                        .a11y_hidden(),
                ),
        );
    }
    rune::col().gap(12.0).child(legend).child(list)
}

#[cfg(test)]
mod tests {
    use rain_diagram::DiagramStyle;

    use super::*;
    use crate::schema::samples;

    fn calls() -> &'static CallFrame {
        samples().c.calls.as_ref().expect("the C sample has a call tree")
    }

    fn find<'a>(l: &'a Layout, id: &str) -> &'a Frame {
        l.frames.iter().find(|f| f.id == id).unwrap()
    }

    #[test]
    fn the_call_tree_ids_are_unique_and_it_adds_up() {
        let c = &samples().c;
        let mut ids = std::collections::BTreeSet::new();
        let mut n_frames = 0;
        for (_, tree) in trees(c) {
            for (_, f) in tree.walk() {
                ids.insert(f.id.clone());
                n_frames += 1;
            }
        }
        assert_eq!(ids.len(), n_frames, "frame ids are unique across the trees");
        let (b, a) = calls().total();
        assert!(a > b, "the change adds work: {b} → {a}");
    }

    #[test]
    fn every_frame_is_laid_out_inside_the_canvas_and_inside_its_parent() {
        let size = [830.0, 760.0];
        let l = layout(calls(), &DiagramStyle::dark(), size, None);
        assert_eq!(l.frames.len(), calls().walk().len());
        for f in &l.frames {
            for r in [f.before, f.after] {
                assert!(r[0] >= MARGIN - 0.01 && r[0] + r[2] <= size[0] - MARGIN + 0.01, "{}", f.id);
                assert!(r[1] + r[3] <= size[1], "{}", f.id);
            }
            if let Some(p) = f.parent {
                let p = &l.frames[p];
                assert!(
                    f.after[0] >= p.after[0] - 0.01 && f.after[0] + f.after[2] <= p.after[0] + p.after[2] + 1.01,
                    "{}",
                    f.id
                );
                assert!(f.after[1] > p.after[1]);
            }
        }
        let root = &l.frames[0];
        assert!(
            (root.after[2] - (830.0 - 2.0 * MARGIN - 1.0)).abs() < 0.5,
            "after is the larger total: full width"
        );
        assert!(root.before[2] < root.after[2] * 0.6, "before is about half of it");
    }

    #[test]
    fn play_morphs_from_before_to_after_and_warms_new_work() {
        let ink = DiagramStyle::dark();
        let l = layout(calls(), &ink, [830.0, 760.0], None);
        let new = find(&l, "record.insert");
        assert_eq!(new.status, Status::Added);
        assert_eq!(new.rect.at(0.0)[2], 0.0, "not there before");
        assert!(new.rect.at(END)[2] > 10.0, "there after");
        assert_eq!(new.fill.at(END), heat_fill(&ink, 1.0));
        let gone = find(&l, "pay.compute");
        assert_eq!(gone.status, Status::Removed);
        assert!(gone.rect.at(0.0)[2] > 10.0);
        assert_eq!(gone.rect.at(END)[2], 0.0);
        assert!(gone.heat < -0.99);
        let same = find(&l, "pay.capture");
        assert_eq!(same.status, Status::Same);
        assert_eq!(same.fill.at(END), ink.neutral_fill);
        assert!(
            (same.rect.at(0.0)[2] - same.rect.at(END)[2]).abs() < 0.01,
            "the same work is the same width"
        );
    }

    #[test]
    fn a_pick_zooms_its_frame_to_the_full_width_and_keeps_the_bound_values() {
        let ink = DiagramStyle::dark();
        let whole = layout(calls(), &ink, [830.0, 760.0], None);
        let zoomed = layout(calls(), &ink, [830.0, 760.0], Some("record"));
        let r = find(&zoomed, "record");
        assert!((r.after[2] - (830.0 - 2.0 * MARGIN - 1.0)).abs() < 0.5);
        assert!(find(&zoomed, "all").after[2] >= r.after[2], "an ancestor spans the zoom");
        assert_eq!(find(&zoomed, "page").after[2], 0.0, "a frame outside the zoom folds away");
        assert!(find(&zoomed, "record.insert").after[2] > find(&whole, "record.insert").after[2] * 2.0);
        find(&whole, "record").rect.live.set([1.0, 2.0, 3.0, 4.0]);
        assert_eq!(
            find(&zoomed, "record").rect.live.get(),
            [1.0, 2.0, 3.0, 4.0],
            "the same value, so the renderer springs it to the new zoom"
        );
    }

    #[test]
    fn the_conclusion_says_where_the_time_went_in_each_tree() {
        let ink = DiagramStyle::dark();
        let l = layout(calls(), &ink, [830.0, 760.0], None);
        assert_eq!(
            l.says,
            "All server work: 36 ms → 73 ms per request (+37 ms). RecordVoucher() (new, +24 ms), mostly \
             RefreshTotal() +15 ms; Charge() (+13 ms): Total() +10 ms, Compute() −10 ms, Redeem() +7 ms."
        );
        assert_eq!(biggest(&l)[0].id, "record");
        let trees = trees(&samples().c);
        assert_eq!(trees.len(), 2, "the change's call tree and the query's");
        let (i, q) = tree_of(&trees, Some("q.total")).unwrap();
        assert_eq!((i, q.id.as_str()), (1, "q"));
        assert_eq!(tree_of(&trees, None).unwrap().0, 0);
        let ql = layout(q, &ink, [830.0, 760.0], Some("q"));
        assert!(
            ql.says
                .starts_with("Account total, 2,400 charges: 28 ms → 10 ms per request (−18 ms)."),
            "{}",
            ql.says
        );
        assert!(
            ql.says
                .contains("Compute() (gone, −28 ms), mostly SELECT Charges (2,400 rows) −18 ms")
        );
        assert_eq!(ql.focus, 0, "a tree's root is its whole width");
    }

    #[test]
    fn ticks_follow_the_zoom() {
        let ink = DiagramStyle::dark();
        let l = layout(calls(), &ink, [830.0, 760.0], Some("pay.total"));
        assert!(l.ticks.len() >= 3);
        assert!(l.ticks.iter().all(|t| t.x >= MARGIN - 0.5 && t.x <= 830.0 - MARGIN + 0.5));
    }
}
