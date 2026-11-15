//! The laid-out picture every lens produces: placed boxes, routed lines and the atlas difit reports.

use std::collections::BTreeSet;

use rain_core::overview::{AtlasBox, EdgeEmphasis, GraphDelta, GraphLens, RenderAtlas};

use crate::design::{BADGE_HEIGHT, BADGE_RISE, DIAGRAM_MARGIN};

/// What a placed box or route stands for. Ids are the document's own.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemId {
    /// An architecture lane, or a data-model store (drawn as a lane).
    Lane(String),
    Node(String),
    Edge(String),
    Participant {
        flow: String,
        node: String,
    },
    Message {
        flow: String,
        id: String,
    },
    Table(String),
    Column {
        table: String,
        column: String,
    },
    Index {
        table: String,
        name: String,
    },
    Relation(String),
    /// The closing "+ N more" row of a folded table; `indexes` for the index section's.
    Fold {
        table: String,
        indexes: bool,
    },
    /// A sync call's activation bar on a participant's lifeline.
    Activation {
        flow: String,
        node: String,
        at: usize,
    },
    /// A note beside a message, or the band around a repeated one.
    Note {
        flow: String,
        id: String,
    },
    Repeat {
        flow: String,
        id: String,
    },
    Lifeline {
        flow: String,
        node: String,
    },
}

impl ItemId {
    /// A stable element key (`node:voucher-form`, `col:vouchers:AccountId`, ...). No `/`: Rune reads a
    /// key holding one as a path.
    pub fn key(&self) -> String {
        let k = match self {
            ItemId::Lane(id) => format!("lane:{id}"),
            ItemId::Node(id) => format!("node:{id}"),
            ItemId::Edge(id) => format!("edge:{id}"),
            ItemId::Participant { flow, node } => format!("part:{flow}:{node}"),
            ItemId::Message { flow, id } => format!("msg:{flow}:{id}"),
            ItemId::Table(id) => format!("table:{id}"),
            ItemId::Column { table, column } => format!("col:{table}:{column}"),
            ItemId::Index { table, name } => format!("idx:{table}:{name}"),
            ItemId::Relation(id) => format!("rel:{id}"),
            ItemId::Fold { table, indexes } => format!("fold:{table}:{}", if *indexes { "indexes" } else { "columns" }),
            ItemId::Activation { flow, node, at } => format!("act:{flow}:{node}:{at}"),
            ItemId::Note { flow, id } => format!("note:{flow}:{id}"),
            ItemId::Repeat { flow, id } => format!("repeat:{flow}:{id}"),
            ItemId::Lifeline { flow, node } => format!("life:{flow}:{node}"),
        };
        k.replace('/', "\u{2215}")
    }

    /// Whether a pointer can press or hover it (the decorations cannot).
    pub fn interactive(&self) -> bool {
        !matches!(
            self,
            ItemId::Fold { .. } | ItemId::Activation { .. } | ItemId::Note { .. } | ItemId::Repeat { .. } | ItemId::Lifeline { .. }
        )
    }
}

/// How a placed box is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PlacedKind {
    Lane,
    Card,
    Participant,
    Table,
    Column,
    Index,
    Fold,
    Activation,
    Note,
    Repeat,
}

/// A key chip at the start of a table row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Chip {
    Primary,
    Foreign,
    Index,
    UniqueIndex,
    PlainIndex,
}

impl Chip {
    pub fn text(self) -> &'static str {
        match self {
            Chip::Primary => "PK",
            Chip::Foreign => "FK",
            Chip::Index => "IX",
            Chip::UniqueIndex => "UX",
            Chip::PlainIndex => "IX",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub id: ItemId,
    pub kind: PlacedKind,
    /// `[x, y, width, height]` in diagram points.
    pub rect: [f32; 4],
    pub label: String,
    pub subtitle: Option<String>,
    pub delta: GraphDelta,
    /// Badges as shown above a card, the delta badge last (already fitted to the card).
    pub badges: Vec<String>,
    pub emphasis: EdgeEmphasis,
    /// The node kind for a card's glyph, or nothing.
    pub glyph: Option<String>,
    pub chip: Option<Chip>,
    /// Hue index of a key chip (data model), when its column is a relation end.
    pub family: Option<usize>,
    /// Right-aligned secondary text: a column's type, an index's columns.
    pub aside: Option<String>,
    /// A changed column's old type, drawn struck before `aside`.
    pub previous: Option<String>,
    pub title_size: f32,
    /// The box this one sits in (node → lane, column → table, ...).
    pub parent: Option<ItemId>,
}

impl Placed {
    pub fn new(id: ItemId, kind: PlacedKind, rect: [f32; 4], label: impl Into<String>) -> Placed {
        Placed {
            id,
            kind,
            rect,
            label: label.into(),
            subtitle: None,
            delta: GraphDelta::Unchanged,
            badges: Vec::new(),
            emphasis: EdgeEmphasis::Normal,
            glyph: None,
            chip: None,
            family: None,
            aside: None,
            previous: None,
            title_size: 0.0,
            parent: None,
        }
    }

    pub fn badge_strip(&self) -> Option<[f32; 4]> {
        if self.badges.is_empty() {
            return None;
        }
        let used = crate::arch::badges_width(&self.badges);
        let right = self.rect[0] + self.rect[2] - crate::design::CARD_PADDING_X / 2.0;
        Some([right - used, self.rect[1] - BADGE_RISE, used, BADGE_HEIGHT])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RouteKind {
    Edge,
    Sync,
    Async,
    Return,
    SelfCall,
    Relation { enforced: bool },
    Lifeline,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub id: ItemId,
    pub kind: RouteKind,
    /// The corners the line turns at, start to end; the head (if any) points at the last.
    pub points: Vec<[f32; 2]>,
    /// Label text and its pill box.
    pub label: Option<(String, [f32; 4])>,
    pub delta: GraphDelta,
    pub emphasis: EdgeEmphasis,
    pub animated: bool,
    pub head: bool,
    /// Key family hue (data model).
    pub family: Option<usize>,
    /// What the route joins, for the hover veil.
    pub ends: Option<(ItemId, ItemId)>,
}

impl Route {
    pub fn new(id: ItemId, kind: RouteKind, points: Vec<[f32; 2]>) -> Route {
        Route {
            id,
            kind,
            points,
            label: None,
            delta: GraphDelta::Unchanged,
            emphasis: EdgeEmphasis::Normal,
            animated: false,
            head: true,
            family: None,
            ends: None,
        }
    }

    /// Bounding box of the line and its label.
    pub fn bounds(&self) -> [f32; 4] {
        let mut b = Bounds::default();
        for p in &self.points {
            b.point(*p);
        }
        if let Some((_, r)) = &self.label {
            b.rect(*r);
        }
        b.rect_of()
    }
}

/// One lens laid out: the atlas plus everything the painter needs.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    pub lens: GraphLens,
    pub title: String,
    pub width: f32,
    pub height: f32,
    pub items: Vec<Placed>,
    pub routes: Vec<Route>,
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    min: [f32; 2],
    max: [f32; 2],
}

impl Default for Bounds {
    fn default() -> Self {
        Bounds {
            min: [f32::MAX; 2],
            max: [f32::MIN; 2],
        }
    }
}

impl Bounds {
    fn point(&mut self, p: [f32; 2]) {
        self.min = [self.min[0].min(p[0]), self.min[1].min(p[1])];
        self.max = [self.max[0].max(p[0]), self.max[1].max(p[1])];
    }
    fn rect(&mut self, r: [f32; 4]) {
        self.point([r[0], r[1]]);
        self.point([r[0] + r[2], r[1] + r[3]]);
    }
    fn rect_of(&self) -> [f32; 4] {
        if self.min[0] > self.max[0] {
            return [0.0; 4];
        }
        [self.min[0], self.min[1], self.max[0] - self.min[0], self.max[1] - self.min[1]]
    }
}

fn atlas_box(r: [f32; 4]) -> AtlasBox {
    AtlasBox {
        x: r[0],
        y: r[1],
        width: r[2],
        height: r[3],
    }
}

impl Layout {
    pub(crate) fn new(lens: GraphLens, title: impl Into<String>) -> Layout {
        Layout {
            lens,
            title: title.into(),
            width: 0.0,
            height: 0.0,
            items: Vec::new(),
            routes: Vec::new(),
        }
    }

    /// Moves everything so the drawing starts `DIAGRAM_MARGIN` in from the top-left and sizes the
    /// canvas to what is drawn, badges, labels and routes that leave the lanes included.
    pub(crate) fn fit_canvas(&mut self) {
        let mut b = Bounds::default();
        for it in &self.items {
            b.rect(it.rect);
            if let Some(s) = it.badge_strip() {
                b.rect(s);
            }
        }
        for r in &self.routes {
            b.rect(r.bounds());
        }
        let [x, y, w, h] = b.rect_of();
        let (dx, dy) = ((DIAGRAM_MARGIN - x).max(0.0), (DIAGRAM_MARGIN - y).max(0.0));
        if dx > 0.0 || dy > 0.0 {
            for it in &mut self.items {
                it.rect[0] += dx;
                it.rect[1] += dy;
            }
            for r in &mut self.routes {
                for p in &mut r.points {
                    p[0] += dx;
                    p[1] += dy;
                }
                if let Some((_, l)) = &mut r.label {
                    l[0] += dx;
                    l[1] += dy;
                }
            }
        }
        self.width = (x + dx + w + DIAGRAM_MARGIN).ceil().max(self.width);
        self.height = (y + dy + h + DIAGRAM_MARGIN).ceil().max(self.height);
    }

    pub fn item(&self, id: &ItemId) -> Option<&Placed> {
        self.items.iter().find(|p| &p.id == id)
    }

    pub fn route(&self, id: &ItemId) -> Option<&Route> {
        self.routes.iter().find(|r| &r.id == id)
    }

    /// difit's `RenderAtlas`: every lane, node, edge, message, table, column and index box.
    pub fn atlas(&self) -> RenderAtlas {
        let mut a = RenderAtlas::default();
        for it in &self.items {
            let b = atlas_box(it.rect);
            match &it.id {
                ItemId::Lane(id) => {
                    a.lanes.insert(id.clone(), b);
                }
                ItemId::Node(id) => {
                    a.nodes.insert(id.clone(), b);
                }
                ItemId::Participant { node, .. } => {
                    a.nodes.insert(node.clone(), b);
                }
                ItemId::Table(id) => {
                    a.tables.insert(id.clone(), b);
                }
                ItemId::Column { table, column } => {
                    a.columns.entry(table.clone()).or_default().insert(column.clone(), b);
                }
                ItemId::Index { table, name } => {
                    a.indexes.entry(table.clone()).or_default().insert(name.clone(), b);
                }
                _ => {}
            }
        }
        for r in &self.routes {
            let b = atlas_box(r.bounds());
            match &r.id {
                ItemId::Edge(id) | ItemId::Relation(id) => {
                    a.edges.insert(id.clone(), b);
                }
                ItemId::Message { flow, id } => {
                    a.messages.entry(flow.clone()).or_default().insert(id.clone(), b);
                }
                _ => {}
            }
        }
        a
    }

    /// What stays lit while `id` is hovered: itself, what it joins or holds, and what joins it.
    pub fn related(&self, id: &ItemId) -> BTreeSet<ItemId> {
        let mut lit = BTreeSet::from([id.clone()]);
        let owner = |x: &ItemId| -> Option<ItemId> {
            match x {
                ItemId::Column { table, .. } | ItemId::Index { table, .. } | ItemId::Fold { table, .. } => {
                    Some(ItemId::Table(table.clone()))
                }
                _ => None,
            }
        };
        let touches = |end: &ItemId| end == id || owner(end).as_ref() == Some(id);
        for r in &self.routes {
            let Some((a, b)) = &r.ends else { continue };
            if &r.id == id {
                lit.extend([a.clone(), b.clone()]);
                lit.extend(owner(a));
                lit.extend(owner(b));
            } else if touches(a) || touches(b) {
                lit.insert(r.id.clone());
                for end in [a, b] {
                    lit.insert(end.clone());
                    lit.extend(owner(end));
                }
            }
        }
        for it in &self.items {
            if it.parent.as_ref() == Some(id) || it.parent.as_ref().is_some_and(|p| lit.contains(p) && !matches!(p, ItemId::Lane(_))) {
                lit.insert(it.id.clone());
            }
        }
        let parents: Vec<ItemId> = self
            .items
            .iter()
            .filter(|it| lit.contains(&it.id))
            .filter_map(|it| it.parent.clone())
            .collect();
        lit.extend(parents);
        lit
    }

    /// The topmost interactive item or route at `p` (diagram points); cards win over lines,
    /// rows over their table, and anything over its lane.
    pub fn hit(&self, p: [f32; 2]) -> Option<ItemId> {
        let inside = |r: [f32; 4]| p[0] >= r[0] && p[0] <= r[0] + r[2] && p[1] >= r[1] && p[1] <= r[1] + r[3];
        let rank = |k: PlacedKind| match k {
            PlacedKind::Column | PlacedKind::Index => 3,
            PlacedKind::Card | PlacedKind::Participant | PlacedKind::Table => 2,
            PlacedKind::Lane => 0,
            _ => 1,
        };
        let best = self
            .items
            .iter()
            .filter(|it| it.id.interactive() && inside(it.rect))
            .max_by_key(|it| rank(it.kind))
            .filter(|it| it.kind != PlacedKind::Lane);
        if let Some(it) = best {
            return Some(it.id.clone());
        }
        for r in &self.routes {
            if !r.id.interactive() {
                continue;
            }
            if r.label.as_ref().is_some_and(|(_, l)| inside(*l)) || hit_segments(&r.points).iter().any(|s| inside(*s)) {
                return Some(r.id.clone());
            }
        }
        self.items
            .iter()
            .find(|it| it.kind == PlacedKind::Lane && inside(it.rect))
            .map(|it| it.id.clone())
    }

    /// Lanes one under another, for viewports too narrow for them side by side: every box moves
    /// with its lane, lines inside a lane move along, and lines between lanes are dropped (their
    /// count is returned). `None` when there are fewer than two lanes.
    pub fn stacked(&self) -> Option<(Layout, usize)> {
        use crate::design::LANE_GAP;
        let mut lanes: Vec<&Placed> = self.items.iter().filter(|i| i.kind == PlacedKind::Lane).collect();
        if lanes.len() < 2 {
            return None;
        }
        lanes.sort_by(|a, b| a.rect[0].total_cmp(&b.rect[0]));
        let mut shift: Vec<(ItemId, [f32; 2])> = Vec::new();
        let mut y = lanes.iter().map(|l| l.rect[1]).fold(f32::MAX, f32::min);
        for l in &lanes {
            shift.push((l.id.clone(), [DIAGRAM_MARGIN - l.rect[0], y - l.rect[1]]));
            y += l.rect[3] + LANE_GAP;
        }
        let lane_of = |id: &ItemId| -> Option<usize> {
            let mut at = self.item(id)?;
            for _ in 0..8 {
                if at.kind == PlacedKind::Lane {
                    return shift.iter().position(|(l, _)| *l == at.id);
                }
                at = self.item(at.parent.as_ref()?)?;
            }
            None
        };
        let mut out = Layout::new(self.lens, self.title.clone());
        for it in &self.items {
            let Some(i) = lane_of(&it.id) else { continue };
            let [dx, dy] = shift[i].1;
            let mut moved = it.clone();
            moved.rect[0] += dx;
            moved.rect[1] += dy;
            out.items.push(moved);
        }
        let mut hidden = 0;
        for r in &self.routes {
            let same = r.ends.as_ref().and_then(|(a, b)| {
                let (la, lb) = (lane_of(a)?, lane_of(b)?);
                (la == lb).then_some(la)
            });
            let Some(i) = same else {
                hidden += 1;
                continue;
            };
            let [dx, dy] = shift[i].1;
            let mut moved = r.clone();
            for p in &mut moved.points {
                p[0] += dx;
                p[1] += dy;
            }
            if let Some((_, b)) = &mut moved.label {
                b[0] += dx;
                b[1] += dy;
            }
            out.routes.push(moved);
        }
        out.fit_canvas();
        Some((out, hidden))
    }
}

/// Grab boxes along a route: one thin rect per segment, `HIT` points either side of the line.
pub fn hit_segments(points: &[[f32; 2]]) -> Vec<[f32; 4]> {
    const HIT: f32 = 4.0;
    points
        .windows(2)
        .map(|w| {
            let (x0, x1) = (w[0][0].min(w[1][0]), w[0][0].max(w[1][0]));
            let (y0, y1) = (w[0][1].min(w[1][1]), w[0][1].max(w[1][1]));
            [x0 - HIT, y0 - HIT, x1 - x0 + HIT * 2.0, y1 - y0 + HIT * 2.0]
        })
        .collect()
}

/// Spreads `count` tracks across a gap from `lo` to `hi`, centred, at most `TRACK_PITCH_MAX` apart.
pub(crate) fn spread(lo: f32, hi: f32, count: usize) -> Vec<f32> {
    use crate::design::{TRACK_CLEARANCE, TRACK_PITCH_MAX};
    let room = (hi - lo - TRACK_CLEARANCE * 2.0).max(0.0);
    let pitch = if count < 2 {
        0.0
    } else {
        TRACK_PITCH_MAX.min(room / (count - 1) as f32)
    };
    let start = (lo + hi) / 2.0 - pitch * (count.saturating_sub(1)) as f32 / 2.0;
    (0..count).map(|i| start + i as f32 * pitch).collect()
}
