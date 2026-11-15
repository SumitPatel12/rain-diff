//! Paints a [`Layout`] with Rune elements: cards and rows as boxes with text, routes as one
//! `Shape` of polylines and arrowheads, press targets on every node, row and line, hover from the
//! pointer's position (so it is exact at any zoom), and a veil that dims what the hovered item
//! does not touch.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use rain_core::overview::{EdgeEmphasis, GraphDelta};
use rune::tok::TextStyle;
use rune::{Color, Cursor, El, LineCap, MagnifyEvent, PointerEvent, Shape, Tx, canvas, col, el, row, text};
use rune_kit::Kit;

use crate::design::*;
use crate::layout::{Chip, ItemId, Layout, Placed, PlacedKind, Route, RouteKind, hit_segments};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Zoom {
    /// Shrinks to fit the viewport's width (never past 100 %).
    Fit,
    P100,
    P150,
    Custom(f32),
}

impl Zoom {
    pub const MIN: f32 = 0.25;
    pub const MAX: f32 = 3.0;

    /// The scale this zoom draws `width` at in a viewport `viewport_w` wide.
    pub fn factor(self, width: f32, viewport_w: f32) -> f32 {
        self.factor_in((width, 0.0), (viewport_w, 0.0), 1.0)
    }

    /// [`Zoom::factor`] where `Fit` may also enlarge a diagram smaller than the viewport, up to
    /// `grow`, until it meets the viewport's width or height.
    pub fn factor_in(self, size: (f32, f32), viewport: (f32, f32), grow: f32) -> f32 {
        let (width, viewport_w) = (size.0, viewport.0);
        let k = match self {
            Zoom::Fit if width > 0.0 && viewport_w > 0.0 && viewport_w < width => viewport_w / width,
            Zoom::Fit if width > 0.0 && viewport_w > 0.0 && size.1 > 0.0 && viewport.1 > 0.0 => {
                (viewport_w / width).min(viewport.1 / size.1).clamp(1.0, grow.max(1.0))
            }
            Zoom::Fit | Zoom::P100 => 1.0,
            Zoom::P150 => 1.5,
            Zoom::Custom(k) => k,
        };
        k.clamp(Self::MIN, Self::MAX)
    }
}

/// What a press, hover or pinch on the diagram reports.
#[derive(Clone, Debug, PartialEq)]
pub enum DiagramEvent {
    Node(String),
    Edge(String),
    Table(String),
    Column {
        table: String,
        column: String,
    },
    Index {
        table: String,
        name: String,
    },
    Message {
        flow: String,
        id: String,
    },
    Lane(String),
    Relation(String),
    /// The item under the pointer changed; `None` once it leaves every item.
    Hover(Option<ItemId>),
    /// Ctrl + wheel or a pinch asked for this scale.
    Zoom(f32),
}

impl DiagramEvent {
    /// The press event an item reports, if it takes presses.
    pub fn press(id: &ItemId) -> Option<DiagramEvent> {
        Some(match id {
            ItemId::Lane(x) => DiagramEvent::Lane(x.clone()),
            ItemId::Node(x) => DiagramEvent::Node(x.clone()),
            ItemId::Participant { node, .. } => DiagramEvent::Node(node.clone()),
            ItemId::Edge(x) => DiagramEvent::Edge(x.clone()),
            ItemId::Message { flow, id } => DiagramEvent::Message {
                flow: flow.clone(),
                id: id.clone(),
            },
            ItemId::Table(x) => DiagramEvent::Table(x.clone()),
            ItemId::Column { table, column } => DiagramEvent::Column {
                table: table.clone(),
                column: column.clone(),
            },
            ItemId::Index { table, name } => DiagramEvent::Index {
                table: table.clone(),
                name: name.clone(),
            },
            ItemId::Relation(x) => DiagramEvent::Relation(x.clone()),
            _ => return None,
        })
    }
}

type Handler<S> = Rc<dyn Fn(&mut S, DiagramEvent, &mut Tx)>;

/// Review progress drawn on a node: the share of its files marked viewed and its open threads.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NodeMark {
    /// 0.0 ..= 1.0.
    pub viewed: f32,
    pub threads: usize,
}

/// The node a card, participant or table stands for.
pub fn node_of(id: &ItemId) -> Option<&str> {
    match id {
        ItemId::Node(n) | ItemId::Participant { node: n, .. } | ItemId::Table(n) => Some(n),
        _ => None,
    }
}

/// A laid-out lens as an element tree.
pub struct Diagram<'l, S> {
    layout: &'l Layout,
    style: DiagramStyle,
    hover: Option<ItemId>,
    zoom: Zoom,
    viewport: (f32, f32),
    fit_grow: f32,
    key: String,
    on_event: Option<Handler<S>>,
    marks: BTreeMap<String, NodeMark>,
    selected: Option<String>,
}

/// One frame's paint settings: the scale and the veil.
struct Paint<'a> {
    k: f32,
    st: &'a DiagramStyle,
    lit: Option<BTreeSet<ItemId>>,
    hovered: Option<&'a ItemId>,
}

/// What a hover leaves of the items it doesn't relate: faded, still readable for context.
pub(crate) const DIMMED: f32 = 0.6;

impl Paint<'_> {
    fn dim(&self, id: &ItemId) -> f32 {
        match &self.lit {
            Some(lit) if !lit.contains(id) => DIMMED,
            _ => 1.0,
        }
    }
    fn at(&self, r: [f32; 4]) -> El {
        el().absolute(r[0] * self.k, r[1] * self.k).size(r[2] * self.k, r[3] * self.k)
    }
    fn font(&self, size: f32, weight: f32) -> TextStyle {
        TextStyle::new(size * self.k, weight).mono()
    }
}

impl<'l, S: 'static> Diagram<'l, S> {
    pub fn new(layout: &'l Layout) -> Self {
        Diagram {
            layout,
            style: DiagramStyle::dark(),
            hover: None,
            zoom: Zoom::Fit,
            viewport: (0.0, 0.0),
            fit_grow: 1.0,
            key: "diagram".into(),
            on_event: None,
            marks: BTreeMap::new(),
            selected: None,
        }
    }
    /// Viewed share and thread count per node id.
    pub fn marks(mut self, marks: BTreeMap<String, NodeMark>) -> Self {
        self.marks = marks;
        self
    }
    /// The node whose card carries the selection ring.
    pub fn selected(mut self, node: Option<String>) -> Self {
        self.selected = node;
        self
    }
    pub fn style(mut self, style: DiagramStyle) -> Self {
        self.style = style;
        self
    }
    /// The hovered item, as the last `DiagramEvent::Hover` reported it.
    pub fn hover(mut self, hover: Option<ItemId>) -> Self {
        self.hover = hover;
        self
    }
    pub fn zoom(mut self, zoom: Zoom) -> Self {
        self.zoom = zoom;
        self
    }
    /// The viewport size `Zoom::Fit` fits into.
    pub fn viewport(mut self, w: f32, h: f32) -> Self {
        self.viewport = (w, h);
        self
    }
    /// Lets `Zoom::Fit` enlarge a diagram smaller than the viewport, up to `max`.
    pub fn fit_grow(mut self, max: f32) -> Self {
        self.fit_grow = max;
        self
    }
    /// Element key of the scroll viewport (`diagram` by default); the canvas is `<key>-canvas`.
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = key.into();
        self
    }
    pub fn on_event(mut self, f: impl Fn(&mut S, DiagramEvent, &mut Tx) + 'static) -> Self {
        self.on_event = Some(Rc::new(f));
        self
    }

    /// The scale the diagram draws at.
    pub fn scale(&self) -> f32 {
        self.zoom
            .factor_in((self.layout.width, self.layout.height), self.viewport, self.fit_grow)
    }

    pub fn build(self, _kit: &Kit) -> El {
        let k = self.scale();
        let l = self.layout;
        let lit = self
            .hover
            .as_ref()
            .filter(|h| l.item(h).is_some() || l.route(h).is_some())
            .map(|h| l.related(h));
        let p = Paint {
            k,
            st: &self.style,
            lit,
            hovered: self.hover.as_ref(),
        };
        let mut kids: Vec<El> = Vec::new();

        for it in l.items.iter().filter(|i| i.kind == PlacedKind::Lane) {
            kids.push(self.wire(lane(it, &p), &it.id));
        }
        let lifelines = |r: &Route| r.kind == RouteKind::Lifeline;
        if l.routes.iter().any(lifelines) {
            kids.push(
                canvas(routes_shape(l, &p, lifelines))
                    .absolute(0.0, 0.0)
                    .size(l.width * k, l.height * k)
                    .key(format!("{}-lifelines", self.key)),
            );
        }
        for it in l
            .items
            .iter()
            .filter(|i| matches!(i.kind, PlacedKind::Repeat | PlacedKind::Activation))
        {
            kids.push(decoration(it, &p));
        }
        let lines = canvas(routes_shape(l, &p, |r: &Route| r.kind != RouteKind::Lifeline));
        kids.push(
            lines
                .absolute(0.0, 0.0)
                .size(l.width * k, l.height * k)
                .key(format!("{}-routes", self.key)),
        );
        for r in l.routes.iter().filter(|r| r.id.interactive()) {
            for (i, s) in hit_segments(&r.points).into_iter().enumerate() {
                kids.push(self.wire_as(p.at(s), &r.id, format!("{}#{i}", r.id.key())));
            }
        }
        for it in l
            .items
            .iter()
            .filter(|i| matches!(i.kind, PlacedKind::Card | PlacedKind::Participant | PlacedKind::Table))
        {
            kids.push(self.wire(card(it, l, &p), &it.id));
            if let Some(strip) = badges(it, &p) {
                kids.push(strip);
            }
            if let Some(node) = node_of(&it.id) {
                let selected = self.selected.as_deref() == Some(node);
                if let Some(m) = self.marks.get(node) {
                    for (i, e) in mark(it, node, m, &p).into_iter().enumerate() {
                        kids.push(self.wire_as(e, &it.id, format!("mark:{}#{i}", node.replace('/', "\u{2215}"))));
                    }
                }
                if selected {
                    kids.push(
                        p.at(it.rect)
                            .key(format!("selected:{}", node.replace('/', "\u{2215}")))
                            .radius(CARD_RADIUS * k)
                            .outline(2.0, 2.0, self.style.thread),
                    );
                }
            }
        }
        for it in l
            .items
            .iter()
            .filter(|i| matches!(i.kind, PlacedKind::Column | PlacedKind::Index | PlacedKind::Fold))
        {
            kids.push(self.wire(table_row(it, &p), &it.id));
        }
        for it in l.items.iter().filter(|i| i.kind == PlacedKind::Note) {
            kids.push(decoration(it, &p));
        }
        for r in &l.routes {
            if let Some((t, b)) = &r.label {
                kids.push(self.wire_as(label(r, t, *b, &p), &r.id, format!("{}#label", r.id.key())));
            }
        }

        let mut canvas_el = el()
            .key(format!("{}-canvas", self.key))
            .size(l.width * k, l.height * k)
            .fill(self.style.background)
            .children(kids);
        if let Some(f) = self.on_event.clone() {
            let (layout_hover, held) = (Rc::new(l.clone()), self.hover.clone());
            let g = f.clone();
            let held2 = held.clone();
            canvas_el = canvas_el
                .on_pointer_move_local(move |s: &mut S, e: PointerEvent, tx: &mut Tx| {
                    let now = layout_hover.hit([e.x / k, e.y / k]);
                    if now != held {
                        f(s, DiagramEvent::Hover(now), tx);
                    }
                })
                .on_hover_change(move |s: &mut S, inside, tx| {
                    if !inside && held2.is_some() {
                        g(s, DiagramEvent::Hover(None), tx);
                    }
                });
        }
        let mut viewport = el().key(self.key.clone()).w_full().h_full().scroll_xy().child(canvas_el);
        if let Some(f) = self.on_event {
            viewport = viewport.on_magnify(move |s: &mut S, e: MagnifyEvent, tx: &mut Tx| {
                f(s, DiagramEvent::Zoom((k * (1.0 + e.delta)).clamp(Zoom::MIN, Zoom::MAX)), tx);
                true
            });
        }
        viewport
    }

    fn wire(&self, e: El, id: &ItemId) -> El {
        self.wire_as(e, id, id.key())
    }

    fn wire_as(&self, e: El, id: &ItemId, key: String) -> El {
        let e = e.key(key);
        match (&self.on_event, DiagramEvent::press(id)) {
            (Some(f), Some(ev)) => {
                let f = f.clone();
                e.cursor(Cursor::Pointer)
                    .on_press_tx(move |s: &mut S, tx: &mut Tx| f(s, ev.clone(), tx))
            }
            _ => e,
        }
    }
}

pub(crate) fn tone(st: &DiagramStyle, d: GraphDelta) -> Color {
    match d {
        GraphDelta::Added => st.added,
        GraphDelta::Modified => st.modified,
        GraphDelta::Removed => st.removed,
        GraphDelta::Unchanged => st.edge,
    }
}

/// (fill, border, text) of a badge or pill in a delta's tone.
fn badge_colours(st: &DiagramStyle, d: Option<GraphDelta>) -> (Color, Color, Color) {
    match d {
        Some(GraphDelta::Added) => (st.added_fill, st.added_border, st.added_text),
        Some(GraphDelta::Modified) => (st.modified_fill, st.modified_border, st.modified_text),
        Some(GraphDelta::Removed) => (st.removed_fill, st.removed_border, st.removed_text),
        _ => (st.neutral_fill, st.card_border, st.muted),
    }
}

/// A short glyph for a node kind; the chip reserves its room on every card.
pub fn kind_glyph(kind: &str) -> String {
    let k = kind.to_ascii_lowercase();
    let known = [
        ("ui", "UI"),
        ("component", "UI"),
        ("page", "UI"),
        ("service", "SV"),
        ("route", "API"),
        ("endpoint", "API"),
        ("api", "API"),
        ("controller", "API"),
        ("datastore", "DB"),
        ("database", "DB"),
        ("table", "TB"),
        ("queue", "Q"),
        ("job", "JOB"),
        ("worker", "JOB"),
        ("event", "EV"),
        ("external", "EXT"),
        ("config", "CFG"),
        ("test", "T"),
        ("library", "LIB"),
        ("model", "M"),
    ];
    known.iter().find(|(name, _)| *name == k).map_or_else(
        || {
            kind.chars()
                .filter(|c| c.is_alphanumeric())
                .take(2)
                .collect::<String>()
                .to_uppercase()
        },
        |(_, g)| (*g).to_owned(),
    )
}

fn lane(it: &Placed, p: &Paint) -> El {
    let st = p.st;
    let header = text(it.label.clone())
        .font(p.font(LANE_LABEL_SIZE, 700.0).tracking(LANE_LABEL_TRACKING * LANE_LABEL_SIZE * p.k))
        .color(st.muted)
        .w(LANE_CONTENT_WIDTH * p.k)
        .truncate()
        .absolute(LANE_PADDING_X * p.k, (LANE_HEADER_BASELINE - LANE_TOP - LANE_LABEL_SIZE) * p.k);
    p.at(it.rect)
        .radius(LANE_RADIUS * p.k)
        .fill(st.lane)
        .child(header)
        .opacity(p.dim(&it.id).max(0.6))
}

fn decoration(it: &Placed, p: &Paint) -> El {
    let st = p.st;
    let a = p.dim(it.parent.as_ref().unwrap_or(&it.id));
    match it.kind {
        PlacedKind::Activation => p
            .at(it.rect)
            .radius(2.0 * p.k)
            .fill(st.card)
            .child(
                el().w_full()
                    .h_full()
                    .radius(2.0 * p.k)
                    .fill(st.added_fill)
                    .border(1.0, st.added_border),
            )
            .opacity(a),
        PlacedKind::Repeat => {
            let [w, h] = [it.rect[2] * p.k, it.rect[3] * p.k];
            let r = rect_points(w, h);
            let mut shape = Shape::new();
            for seg in dashes(&r, 4.0 * p.k, 3.0 * p.k) {
                shape = shape.stroke_with(seg, false, 1.0, LineCap::Butt, st.muted.alpha(0.7));
            }
            p.at(it.rect)
                .shape(shape)
                .child(
                    text(it.label.clone())
                        .font(p.font(8.5, 700.0))
                        .color(st.muted)
                        .absolute(6.0 * p.k, 2.0 * p.k),
                )
                .opacity(a)
        }
        _ => p
            .at(it.rect)
            .radius(4.0 * p.k)
            .fill(st.pill)
            .border(1.0, st.pill_border)
            .px(6.0 * p.k)
            .center_y()
            .child(
                text(it.label.clone())
                    .font(p.font(9.0, 400.0))
                    .color(st.muted)
                    .w((it.rect[2] - 12.0) * p.k)
                    .truncate(),
            )
            .opacity(a),
    }
}

fn card_border(st: &DiagramStyle, d: GraphDelta) -> Color {
    match d {
        GraphDelta::Added => st.added.alpha(0.55),
        GraphDelta::Modified => st.modified.alpha(0.5),
        GraphDelta::Removed => st.removed.alpha(0.6),
        GraphDelta::Unchanged => st.card_border,
    }
}

fn card_opacity(d: GraphDelta) -> f32 {
    match d {
        GraphDelta::Removed => 0.55,
        GraphDelta::Unchanged => 0.82,
        _ => 1.0,
    }
}

fn rect_points(w: f32, h: f32) -> Vec<[f32; 2]> {
    vec![[0.5, 0.5], [w - 0.5, 0.5], [w - 0.5, h - 0.5], [0.5, h - 0.5], [0.5, 0.5]]
}

fn card(it: &Placed, l: &Layout, p: &Paint) -> El {
    let st = p.st;
    let k = p.k;
    let [w, _] = [it.rect[2] * k, it.rect[3] * k];
    let head_h = if it.kind == PlacedKind::Table {
        CARD_HEIGHT_WITH_SUBTITLE
    } else {
        it.rect[3]
    };
    let text_w = (it.rect[2] - CARD_PADDING_X * 2.0 - ICON_CHIP_SIZE - ICON_CHIP_GAP) * k;
    let glyph = it.glyph.as_deref().map(kind_glyph).unwrap_or_default();
    let chip = el()
        .size(ICON_CHIP_SIZE * k, ICON_CHIP_SIZE * k)
        .radius(ICON_CHIP_RADIUS * k)
        .fill(st.chip)
        .center()
        .child(
            text(glyph)
                .font(p.font(
                    if it.glyph.as_deref().map(kind_glyph).is_some_and(|g| g.len() > 2) {
                        7.5
                    } else {
                        9.0
                    },
                    700.0,
                ))
                .color(st.muted),
        );
    let size = if it.title_size > 0.0 { it.title_size } else { TITLE_SIZE };
    let mut title = text(it.label.clone())
        .font(p.font(size, 600.0))
        .color(st.foreground)
        .w(text_w)
        .truncate();
    if it.delta == GraphDelta::Removed {
        title = title.strike(0, it.label.len(), None);
    }
    let mut words = col().gap(2.0 * k).child(title);
    if let Some(sub) = &it.subtitle {
        words = words.child(
            text(sub.clone())
                .font(p.font(SUBTITLE_SIZE, 400.0))
                .color(st.muted)
                .w(text_w)
                .truncate(),
        );
    }
    let head = row()
        .w(w)
        .h(head_h * k)
        .px(CARD_PADDING_X * k)
        .gap(ICON_CHIP_GAP * k)
        .center_y()
        .child(chip)
        .child(words);

    let mut shape = Shape::new();
    if it.delta == GraphDelta::Removed {
        for seg in dashes(&rect_points(w, it.rect[3] * k), 4.0 * k, 3.0 * k) {
            shape = shape.stroke_with(seg, false, 1.0, LineCap::Butt, st.removed.alpha(0.6));
        }
    }
    if it.kind == PlacedKind::Table {
        let rule = head_h * k;
        shape = shape.line([0.0, rule], [w, rule], 1.0, LineCap::Butt, st.card_border);
        let pad = CARD_PADDING_X * k;
        let mut first_index = true;
        for row in l.items.iter().filter(|r| r.parent.as_ref() == Some(&it.id)) {
            let y = (row.rect[1] - it.rect[1]) * k;
            match (row.kind, &row.id) {
                (PlacedKind::Index, _) | (PlacedKind::Fold, ItemId::Fold { indexes: true, .. }) if first_index => {
                    first_index = false;
                    let ry = y - TABLE_INDEX_RULE_GAP / 2.0 * k - if row.kind == PlacedKind::Fold { TABLE_FOLD_GAP * k } else { 0.0 };
                    shape = shape.line([pad, ry], [w - pad, ry], 1.0, LineCap::Butt, st.card_border);
                }
                _ => {}
            }
            if row.kind == PlacedKind::Fold {
                let ry = y - TABLE_FOLD_GAP / 2.0 * k;
                for seg in dashes(&[[pad, ry], [w - pad, ry]], 3.0 * k, 3.0 * k) {
                    shape = shape.stroke_with(seg, false, 1.0, LineCap::Butt, st.card_border);
                }
            }
        }
    }
    let border = if it.delta == GraphDelta::Removed {
        Color::TRANSPARENT
    } else {
        card_border(st, it.delta)
    };
    let mut out = p.at(it.rect);
    if p.hovered == Some(&it.id) {
        out = out.outline(1.5, 2.0, tone(st, it.delta).alpha(0.6));
    }
    out.radius(CARD_RADIUS * k)
        .fill(st.card)
        .border(1.0, border)
        .shape(shape)
        .child(head)
        .opacity(card_opacity(it.delta) * p.dim(&it.id))
}

/// Progress over a card: a tint once every file is viewed, a bar along its foot for the viewed
/// share, and a thread-count badge on its top-left corner.
fn mark(it: &Placed, node: &str, m: &NodeMark, p: &Paint) -> Vec<El> {
    let (st, k) = (p.st, p.k);
    let [x, y, w, h] = it.rect;
    let foot = if it.kind == PlacedKind::Table {
        CARD_HEIGHT_WITH_SUBTITLE
    } else {
        h
    };
    let mut out = Vec::new();
    let viewed = m.viewed.clamp(0.0, 1.0);
    if viewed >= 1.0 {
        out.push(
            p.at([x, y, w, foot])
                .radius(CARD_RADIUS * k)
                .fill(st.viewed_fill)
                .label(format!("{node}: viewed")),
        );
    }
    let track = [x + CARD_PADDING_X, y + foot - 6.0, w - CARD_PADDING_X * 2.0, 3.0];
    out.push(
        p.at(track)
            .radius(1.5 * k)
            .fill(st.card_border.alpha(0.6))
            .child(el().w(track[2] * viewed * k).h_full().radius(1.5 * k).fill(st.viewed))
            .label(format!("{node}: {:.0} % viewed", viewed * 100.0)),
    );
    if m.threads > 0 {
        let n = m.threads.to_string();
        let d = 18.0_f32.max(measure(&n, 10.0) + 10.0);
        out.push(
            p.at([x - 7.0, y - 7.0, d, 18.0])
                .radius(9.0 * k)
                .fill(st.thread)
                .border(1.5, st.background)
                .center()
                .label(format!("{node}: {n} thread(s)"))
                .child(text(n).font(p.font(10.0, 700.0)).color(st.thread_text)),
        );
    }
    out.into_iter().map(|e| e.opacity(p.dim(&it.id))).collect()
}

fn badges(it: &Placed, p: &Paint) -> Option<El> {
    let strip = it.badge_strip()?;
    let k = p.k;
    let delta_badge = crate::arch::delta_badge(it.delta);
    let mut r = row().absolute(strip[0] * k, strip[1] * k).h(BADGE_HEIGHT * k).gap(BADGE_GAP * k);
    for b in &it.badges {
        let tone = (Some(b.as_str()) == delta_badge).then_some(it.delta);
        let (fill, border, ink) = badge_colours(p.st, tone);
        r = r.child(
            el().h(BADGE_HEIGHT * k)
                .w(crate::arch::badge_width(b) * k)
                .radius(BADGE_RADIUS * k)
                .fill(fill)
                .border(1.0, border)
                .center()
                .child(
                    text(b.clone())
                        .font(p.font(BADGE_TEXT_SIZE, 700.0).tracking(BADGE_TRACKING * BADGE_TEXT_SIZE * k))
                        .color(ink),
                ),
        );
    }
    Some(r.opacity(p.dim(&it.id)))
}

fn ink(st: &DiagramStyle, d: GraphDelta) -> Color {
    match d {
        GraphDelta::Added => st.added_text,
        GraphDelta::Removed => st.removed_text,
        _ => st.foreground,
    }
}

fn table_row(it: &Placed, p: &Paint) -> El {
    let st = p.st;
    let k = p.k;
    let chip_el = |c: Chip| {
        let (fill, border, ink) = match (c, it.family) {
            (Chip::UniqueIndex, _) => (st.muted, Color::TRANSPARENT, st.card),
            (Chip::PlainIndex, _) => (Color::TRANSPARENT, st.muted, st.muted),
            (_, Some(f)) => (st.key_fills[f % 5], st.keys[f % 5], st.keys[f % 5]),
            _ => (st.chip, Color::TRANSPARENT, st.muted),
        };
        el().size(KEY_CHIP_WIDTH * k, KEY_CHIP_HEIGHT * k)
            .radius(KEY_CHIP_HEIGHT / 2.0 * k)
            .fill(fill)
            .border(1.0, border)
            .center()
            .child(text(c.text()).font(p.font(8.0, 700.0)).color(ink))
    };
    let lead = match it.chip {
        Some(c) => chip_el(c),
        None => el().size(KEY_CHIP_WIDTH * k, KEY_CHIP_HEIGHT * k),
    };
    let name_ink = if it.kind == PlacedKind::Fold { st.muted } else { ink(st, it.delta) };
    let mut name = text(it.label.clone())
        .font(p.font(TABLE_ROW_TEXT_SIZE, 400.0))
        .color(name_ink)
        .truncate()
        .grow(1.0);
    if it.delta == GraphDelta::Removed {
        name = name.strike(0, it.label.len(), None);
    }
    let mut r = row()
        .w_full()
        .h_full()
        .px(CARD_PADDING_X * k)
        .gap(KEY_CHIP_GAP * k)
        .center_y()
        .child(lead)
        .child(name);
    if let Some(aside) = &it.aside {
        let type_ink = match it.delta {
            GraphDelta::Modified => st.modified_text,
            GraphDelta::Added => st.added_text,
            _ => st.muted,
        };
        if let Some(prev) = &it.previous {
            r = r.child(
                text(prev.clone())
                    .font(p.font(TABLE_ROW_TYPE_SIZE, 400.0))
                    .color(st.muted)
                    .strike(0, prev.len(), None),
            );
            r = r.child(text("→").font(p.font(TABLE_ROW_TYPE_SIZE, 400.0)).color(st.muted));
        }
        r = r.child(text(aside.clone()).font(p.font(TABLE_ROW_TYPE_SIZE, 400.0)).color(type_ink));
    }
    let a = p.dim(&it.id).max(p.dim(it.parent.as_ref().unwrap_or(&it.id)));
    let lit = p.lit.as_ref().is_some_and(|l| l.contains(&it.id)) && it.kind != PlacedKind::Fold;
    let mut out = p.at(it.rect).child(r).opacity(card_opacity_row(it) * a);
    if lit {
        out = out.fill(st.chip);
    }
    out
}

fn card_opacity_row(it: &Placed) -> f32 {
    if it.delta == GraphDelta::Removed { 0.7 } else { 1.0 }
}

fn label(r: &Route, t: &str, b: [f32; 4], p: &Paint) -> El {
    let st = p.st;
    let message = matches!(r.kind, RouteKind::Sync | RouteKind::Async | RouteKind::Return | RouteKind::SelfCall);
    let (_, _, ink) = badge_colours(st, (r.delta != GraphDelta::Unchanged).then_some(r.delta));
    let ink = if message { st.foreground } else { ink };
    let mut e = p
        .at(b)
        .radius(b[3] / 2.0 * p.k)
        .center()
        .child(text(t.to_owned()).font(p.font(PILL_TEXT_SIZE, 600.0)).color(ink).single_line());
    e = if message {
        e.fill(st.background.alpha(0.85))
    } else {
        e.fill(st.pill).border(1.0, st.pill_border)
    };
    e.opacity(p.dim(&r.id))
}

/// The route's corners rounded by up to `radius` (never past half the shorter leg).
pub fn rounded(points: &[[f32; 2]], radius: f32) -> Vec<[f32; 2]> {
    if points.len() < 3 || radius <= 0.0 {
        return points.to_vec();
    }
    let len = |a: [f32; 2], b: [f32; 2]| ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    let toward = |a: [f32; 2], b: [f32; 2], d: f32| {
        let l = len(a, b).max(1e-6);
        [a[0] + (b[0] - a[0]) * d / l, a[1] + (b[1] - a[1]) * d / l]
    };
    let mut out = vec![points[0]];
    for w in points.windows(3) {
        let (a, c, b) = (w[0], w[1], w[2]);
        let r = radius.min(len(a, c) / 2.0).min(len(c, b) / 2.0);
        let (p0, p1) = (toward(c, a, r), toward(c, b, r));
        out.push(p0);
        for i in 1..6 {
            let t = i as f32 / 6.0;
            let u = 1.0 - t;
            out.push([
                u * u * p0[0] + 2.0 * u * t * c[0] + t * t * p1[0],
                u * u * p0[1] + 2.0 * u * t * c[1] + t * t * p1[1],
            ]);
        }
        out.push(p1);
    }
    out.push(points[points.len() - 1]);
    out
}

/// `points` cut into dashes `on` long with `off` gaps, the pattern running on across corners.
pub fn dashes(points: &[[f32; 2]], on: f32, off: f32) -> Vec<Vec<[f32; 2]>> {
    let mut out: Vec<Vec<[f32; 2]>> = Vec::new();
    let mut cur: Vec<[f32; 2]> = Vec::new();
    let (mut drawing, mut left) = (true, on);
    for w in points.windows(2) {
        let (mut a, b) = (w[0], w[1]);
        let mut seg = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        if drawing && cur.is_empty() {
            cur.push(a);
        }
        while seg > 1e-4 {
            let step = left.min(seg);
            let n = [a[0] + (b[0] - a[0]) * step / seg, a[1] + (b[1] - a[1]) * step / seg];
            if drawing {
                cur.push(n);
            }
            seg -= step;
            left -= step;
            a = n;
            if left <= 1e-4 {
                drawing = !drawing;
                left = if drawing { on } else { off };
                if drawing {
                    cur.push(a);
                } else if cur.len() >= 2 {
                    out.push(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
            }
        }
    }
    if cur.len() >= 2 {
        out.push(cur);
    }
    out
}

struct Stroke {
    color: Color,
    width: f32,
    dash: Option<(f32, f32)>,
    glow: bool,
    open_head: bool,
}

fn stroke_for(r: &Route, st: &DiagramStyle) -> Stroke {
    let base = match r.kind {
        RouteKind::Relation { .. } => r.family.map_or(st.edge, |f| st.keys[f % 5]),
        RouteKind::Lifeline => st.lifeline,
        _ => tone(st, r.delta),
    };
    let mut s = Stroke {
        color: base,
        width: 1.5,
        dash: None,
        glow: false,
        open_head: false,
    };
    match r.delta {
        GraphDelta::Removed => {
            s.dash = Some((5.0, 4.0));
            s.color = s.color.alpha(0.7);
        }
        GraphDelta::Unchanged => s.color = s.color.alpha(0.82),
        _ => {}
    }
    match r.kind {
        RouteKind::Edge => match r.emphasis {
            EdgeEmphasis::Hero => {
                s.width = 2.25;
                s.glow = true;
            }
            EdgeEmphasis::Muted => s.color = s.color.alpha(0.45),
            EdgeEmphasis::Normal => {}
        },
        RouteKind::Sync | RouteKind::SelfCall | RouteKind::Async | RouteKind::Return => {
            if r.delta == GraphDelta::Added {
                s.width = 2.25;
            }
            if r.kind == RouteKind::Async {
                s.dash = Some((6.0, 4.0));
                s.open_head = true;
            }
            if r.kind == RouteKind::Return {
                s.dash = Some((2.0, 3.0));
                s.open_head = true;
                s.color = s.color.alpha(s.color.a * 0.8);
            }
        }
        RouteKind::Relation { enforced } => {
            if r.delta == GraphDelta::Added {
                s.width = KEY_ADDED_STROKE_WIDTH;
            }
            if !enforced {
                s.dash = Some((2.0, 4.0));
            }
        }
        RouteKind::Lifeline => {
            s.width = 1.0;
            s.dash = Some((3.0, 4.0));
        }
    }
    s
}

/// Every route in one shape: glow, line (dashed where its delta or kind says), arrowhead.
fn routes_shape(l: &Layout, p: &Paint, which: impl Fn(&Route) -> bool) -> Shape {
    let k = p.k;
    let mut shape = Shape::new();
    for r in l.routes.iter().filter(|r| which(r)) {
        let mut s = stroke_for(r, p.st);
        let veil = p.dim(&r.id);
        s.color = s.color.alpha(s.color.a * veil);
        let pts: Vec<[f32; 2]> = r.points.iter().map(|q| [q[0] * k, q[1] * k]).collect();
        if pts.len() < 2 {
            continue;
        }
        let head_len = 8.0 * k;
        let body = if r.head { trim_end(&pts, head_len * 0.8) } else { pts.clone() };
        let smooth = if r.kind == RouteKind::SelfCall {
            rounded(&body, SELF_LOOP_CORNER * k)
        } else {
            rounded(&body, 8.0 * k)
        };
        if s.glow {
            shape = shape.stroke_with(smooth.clone(), false, 7.0 * k, LineCap::Round, s.color.alpha(0.14 * veil));
        }
        let w = s.width * k.max(0.75);
        match s.dash {
            Some((on, off)) => {
                for seg in dashes(&smooth, on * k, off * k) {
                    shape = shape.stroke_with(seg, false, w, LineCap::Butt, s.color);
                }
            }
            None => shape = shape.stroke_with(smooth, false, w, LineCap::Round, s.color),
        }
        if r.head {
            let (tip, from) = (pts[pts.len() - 1], pts[pts.len() - 2]);
            let (dx, dy) = (tip[0] - from[0], tip[1] - from[1]);
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            let (ux, uy) = (dx / len, dy / len);
            let half = 3.75 * k;
            let base = [tip[0] - ux * head_len, tip[1] - uy * head_len];
            let left = [base[0] - uy * half, base[1] + ux * half];
            let right = [base[0] + uy * half, base[1] - ux * half];
            shape = if s.open_head {
                shape.stroke_with(vec![left, tip, right], false, w, LineCap::Round, s.color)
            } else {
                shape.fill(vec![tip, left, right], s.color)
            };
        }
    }
    shape
}

/// `pts` with the last `d` points of its length removed, so a line ends under its arrowhead.
fn trim_end(pts: &[[f32; 2]], d: f32) -> Vec<[f32; 2]> {
    let mut out = pts.to_vec();
    let mut left = d;
    while out.len() >= 2 && left > 0.0 {
        let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
        let l = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        if l > left {
            let t = (l - left) / l;
            let n = out.len();
            out[n - 1] = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            break;
        }
        left -= l;
        out.pop();
    }
    out
}
