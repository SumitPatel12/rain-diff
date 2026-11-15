//! Architecture lens: lanes as columns, cards seated on a row grid shared by every lane (PR Lens
//! `layout/{architecture,seating,rank}.ts`), and orthogonal connectors that run in the gaps
//! between lanes and rows instead of PR Lens's congestion-aware router.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rain_core::overview::{GraphDelta, GraphEdge, GraphLane, GraphLens, GraphNode, OverviewGraph};

use crate::design::*;
use crate::layout::{ItemId, Layout, Placed, PlacedKind, Route, RouteKind, spread};
use crate::scope::Scoped;

pub fn delta_badge(delta: GraphDelta) -> Option<&'static str> {
    match delta {
        GraphDelta::Added => Some("NEW"),
        GraphDelta::Modified => Some("CHANGED"),
        GraphDelta::Removed => Some("REMOVED"),
        GraphDelta::Unchanged => None,
    }
}

pub fn badge_width(text: &str) -> f32 {
    tracked_width(text, BADGE_TEXT_SIZE, BADGE_TRACKING) + BADGE_PADDING_X * 2.0
}

pub(crate) fn badges_width(badges: &[String]) -> f32 {
    badges.iter().map(|b| badge_width(b)).sum::<f32>() + BADGE_GAP * badges.len().saturating_sub(1) as f32
}

/// The card's own badges then the delta badge, a duplicate of the delta dropped, trimmed from the
/// front until the strip fits the card.
pub fn fitted_badges(own: &[String], delta: GraphDelta, card_width: f32) -> Vec<String> {
    let mut all: Vec<String> = match delta_badge(delta) {
        Some(d) => own
            .iter()
            .filter(|b| !b.eq_ignore_ascii_case(d))
            .cloned()
            .chain([d.to_owned()])
            .collect(),
        None => own.to_vec(),
    };
    let room = card_width - CARD_PADDING_X;
    let mut used = 0.0;
    let mut keep = 0;
    for b in all.iter().rev() {
        let next = used + badge_width(b) + if keep > 0 { BADGE_GAP } else { 0.0 };
        if next > room {
            break;
        }
        used = next;
        keep += 1;
    }
    all.drain(..all.len() - keep);
    all
}

pub fn card_text_width(card_width: f32) -> f32 {
    card_width - CARD_PADDING_X * 2.0 - ICON_CHIP_SIZE - ICON_CHIP_GAP
}

pub fn fitted_title_size(label: &str, earned: f32, budget: f32) -> f32 {
    let mut size = earned;
    while size > TITLE_SIZE_MIN && measure(label, size) > budget {
        size = TITLE_SIZE_MIN.max(size - TITLE_SIZE_STEP);
    }
    size
}

fn card_height(n: &GraphNode) -> f32 {
    if n.subtitle.is_some() {
        CARD_HEIGHT_WITH_SUBTITLE
    } else {
        CARD_HEIGHT
    }
}

/// Lane header: `LABEL · SUBTITLE`, upper-cased.
pub fn lane_header(lane: &GraphLane) -> String {
    match &lane.subtitle {
        Some(s) => format!("{} · {}", lane.label, s).to_uppercase(),
        None => lane.label.to_uppercase(),
    }
}

/// `graph.layout.laneOrder` and `graph.layout.rank`, PR Lens's non-binding hints.
fn hints(graph: &OverviewGraph) -> (Vec<String>, HashMap<String, u32>) {
    let Some(v) = &graph.layout else { return Default::default() };
    let order = v["laneOrder"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect())
        .unwrap_or_default();
    let rank = v["rank"]
        .as_object()
        .map(|m| m.iter().filter_map(|(k, x)| Some((k.clone(), x.as_u64()? as u32))).collect())
        .unwrap_or_default();
    (order, rank)
}

fn order_lanes<'g>(lanes: &[&'g GraphLane], named: &[String]) -> Vec<&'g GraphLane> {
    let mut out: Vec<&GraphLane> = Vec::new();
    for id in named {
        if let Some(l) = lanes.iter().find(|l| &l.id == id)
            && !out.iter().any(|o| o.id == l.id)
        {
            out.push(l);
        }
    }
    let mut rest: Vec<(usize, &GraphLane)> = lanes
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, l)| !out.iter().any(|o| o.id == l.id))
        .collect();
    rest.sort_by_key(|(i, l)| (l.order.unwrap_or(u32::MAX), *i));
    out.extend(rest.into_iter().map(|(_, l)| l));
    out
}

/// Longest path from a source, cycle-closing edges dropped (DFS in document order); hints are a
/// floor, never an answer.
pub(crate) fn rank_nodes(nodes: &[&GraphNode], edges: &[&GraphEdge], hints: &HashMap<String, u32>) -> HashMap<String, u32> {
    let index: HashMap<&str, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let mut forward: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for e in edges {
        if let (Some(&a), Some(&b)) = (index.get(e.from.as_str()), index.get(e.to.as_str()))
            && a != b
        {
            forward[a].push(b);
        }
    }
    let mut backward: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let (mut open, mut closed) = (vec![false; nodes.len()], vec![false; nodes.len()]);
    for root in 0..nodes.len() {
        if closed[root] {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        open[root] = true;
        while let Some(&mut (id, ref mut next)) = stack.last_mut() {
            if *next >= forward[id].len() {
                open[id] = false;
                closed[id] = true;
                stack.pop();
                continue;
            }
            let succ = forward[id][*next];
            *next += 1;
            if open[succ] {
                continue;
            }
            backward[succ].push(id);
            if closed[succ] {
                continue;
            }
            open[succ] = true;
            stack.push((succ, 0));
        }
    }
    let mut ranks: Vec<Option<u32>> = vec![None; nodes.len()];
    fn settle(i: usize, back: &[Vec<usize>], nodes: &[&GraphNode], hints: &HashMap<String, u32>, ranks: &mut [Option<u32>]) -> u32 {
        if let Some(r) = ranks[i] {
            return r;
        }
        let mut r = hints.get(&nodes[i].id).copied().unwrap_or(0);
        for &p in &back[i] {
            r = r.max(settle(p, back, nodes, hints, ranks) + 1);
        }
        ranks[i] = Some(r);
        r
    }
    for i in 0..nodes.len() {
        settle(i, &backward, nodes, hints, &mut ranks);
    }
    nodes.iter().zip(ranks).map(|(n, r)| (n.id.clone(), r.unwrap_or(0))).collect()
}

struct SeatedRow<'g> {
    grid: usize,
    nodes: Vec<&'g GraphNode>,
}

struct Keys {
    fall: HashMap<String, f32>,
    side: HashMap<String, f32>,
}

fn mean(v: &[f32]) -> Option<f32> {
    (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
}

fn barycenters(nodes: &[&GraphNode], edges: &[&GraphEdge], ranks: &HashMap<String, u32>, lane_index: &HashMap<&str, usize>) -> Keys {
    let lane_of: HashMap<&str, usize> = nodes
        .iter()
        .map(|n| (n.id.as_str(), lane_index.get(n.lane.as_str()).copied().unwrap_or(0)))
        .collect();
    let mut partners: HashMap<&str, Vec<&str>> = nodes.iter().map(|n| (n.id.as_str(), Vec::new())).collect();
    for e in edges {
        if e.from == e.to || !lane_of.contains_key(e.from.as_str()) || !lane_of.contains_key(e.to.as_str()) {
            continue;
        }
        if let Some(v) = partners.get_mut(e.from.as_str()) {
            v.push(e.to.as_str());
        }
        if let Some(v) = partners.get_mut(e.to.as_str()) {
            v.push(e.from.as_str());
        }
    }
    let mut keys = Keys {
        fall: HashMap::new(),
        side: HashMap::new(),
    };
    for n in nodes {
        let linked = &partners[n.id.as_str()];
        let own_rank = ranks.get(&n.id).copied().unwrap_or(0) as f32;
        let ranks_of: Vec<f32> = linked.iter().map(|id| ranks.get(*id).copied().unwrap_or(0) as f32).collect();
        let lanes_of: Vec<f32> = linked.iter().map(|id| lane_of[id] as f32).collect();
        keys.fall.insert(n.id.clone(), mean(&ranks_of).unwrap_or(own_rank));
        keys.side
            .insert(n.id.clone(), mean(&lanes_of).unwrap_or(lane_of[n.id.as_str()] as f32));
    }
    keys
}

fn compress(nodes: &[&GraphNode], ranks: &HashMap<String, u32>) -> HashMap<u32, usize> {
    let used: BTreeSet<u32> = nodes.iter().map(|n| ranks.get(&n.id).copied().unwrap_or(0)).collect();
    used.into_iter().enumerate().map(|(i, r)| (r, i)).collect()
}

#[allow(clippy::too_many_arguments)]
fn seat_lane<'g>(
    members: &[&'g GraphNode],
    ranks: &HashMap<String, u32>,
    row_of_rank: &HashMap<u32, usize>,
    floor: i64,
    keys: &Keys,
    doc: &HashMap<&str, usize>,
) -> Vec<SeatedRow<'g>> {
    let row = |n: &GraphNode| row_of_rank.get(&ranks.get(&n.id).copied().unwrap_or(0)).copied().unwrap_or(0);
    let mut sorted = members.to_vec();
    sorted.sort_by(|a, b| {
        row(a)
            .cmp(&row(b))
            .then(keys.fall[&a.id].total_cmp(&keys.fall[&b.id]))
            .then(doc[a.id.as_str()].cmp(&doc[b.id.as_str()]))
    });
    let mut rows: Vec<(i64, u32, Vec<&GraphNode>)> = Vec::new();
    let mut previous = floor;
    for n in sorted {
        let rank = ranks.get(&n.id).copied().unwrap_or(0);
        if let Some((_, r, open)) = rows.last_mut()
            && open.len() == 1
            && *r == rank
            && open[0].group.as_deref().unwrap_or("") == n.group.as_deref().unwrap_or("")
        {
            let side = keys.side[&n.id] - keys.side[&open[0].id];
            let left = if side != 0.0 {
                side < 0.0
            } else {
                doc[n.id.as_str()] < doc[open[0].id.as_str()]
            };
            if left {
                open.insert(0, n);
            } else {
                open.push(n);
            }
            continue;
        }
        previous = (previous + 1).max(floor + 1 + row(n) as i64);
        rows.push((previous, rank, vec![n]));
    }
    rows.into_iter()
        .map(|(grid, _, nodes)| SeatedRow {
            grid: grid as usize,
            nodes,
        })
        .collect()
}

struct Seating<'g> {
    rows_by_lane: BTreeMap<usize, Vec<SeatedRow<'g>>>,
    row_count: usize,
}

fn seat<'g>(lanes: &[&GraphLane], nodes: &[&'g GraphNode], edges: &[&GraphEdge], hints: &HashMap<String, u32>) -> Seating<'g> {
    let lane_index: HashMap<&str, usize> = lanes.iter().enumerate().map(|(i, l)| (l.id.as_str(), i)).collect();
    let doc: HashMap<&str, usize> = nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let living: Vec<&GraphNode> = nodes.iter().copied().filter(|n| n.delta != GraphDelta::Removed).collect();
    let dead: Vec<&GraphNode> = nodes.iter().copied().filter(|n| n.delta == GraphDelta::Removed).collect();
    let living_edges: Vec<&GraphEdge> = edges.iter().copied().filter(|e| e.delta != GraphDelta::Removed).collect();
    let mut rows_by_lane: BTreeMap<usize, Vec<SeatedRow>> = BTreeMap::new();
    let mut last: i64 = -1;
    let mut place = |group: &[&'g GraphNode], group_edges: &[&GraphEdge], floor: i64, hints: &HashMap<String, u32>, last: &mut i64| {
        let ranks = rank_nodes(group, group_edges, hints);
        let row_of_rank = compress(group, &ranks);
        let keys = barycenters(group, group_edges, &ranks, &lane_index);
        for (li, lane) in lanes.iter().enumerate() {
            let members: Vec<&GraphNode> = group.iter().copied().filter(|n| n.lane == lane.id).collect();
            if members.is_empty() {
                continue;
            }
            let rows = seat_lane(&members, &ranks, &row_of_rank, floor, &keys, &doc);
            *last = (*last).max(rows.last().map_or(-1, |r| r.grid as i64));
            rows_by_lane.entry(li).or_default().extend(rows);
        }
    };
    place(&living, &living_edges, -1, hints, &mut last);
    if !dead.is_empty() {
        let floor = last;
        place(&dead, edges, floor, &HashMap::new(), &mut last);
    }
    Seating {
        rows_by_lane,
        row_count: (last + 1) as usize,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Face {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug)]
enum X {
    At(f32),
    Corridor(usize),
}

#[derive(Clone, Copy, Debug)]
enum Y {
    At(f32),
    Band(usize),
}

struct Card {
    rect: [f32; 4],
    lane: usize,
    row: usize,
}

struct Plan<'g> {
    edge: &'g GraphEdge,
    from: (usize, Face),
    to: (usize, Face),
    /// Interior corners between the two ports.
    via: Vec<(X, Y)>,
}

/// Lays out the architecture lens of `scoped`.
pub fn architecture(graph: &OverviewGraph, scoped: &Scoped, title: &str) -> Layout {
    let (named, rank_hints) = hints(graph);
    let lanes = order_lanes(&scoped.lanes, &named);
    let seating = seat(&lanes, &scoped.nodes, &scoped.edges, &rank_hints);
    let mut layout = Layout::new(GraphLens::Architecture, title);

    let mut grid_heights = vec![0.0f32; seating.row_count];
    for rows in seating.rows_by_lane.values() {
        for r in rows {
            let h = r.nodes.iter().map(|n| card_height(n)).fold(0.0, f32::max);
            grid_heights[r.grid] = grid_heights[r.grid].max(h);
        }
    }
    let mut rows_y = Vec::with_capacity(seating.row_count);
    let mut cursor = CONTENT_TOP;
    for h in &grid_heights {
        rows_y.push((cursor, *h));
        cursor += h + ROW_GAP;
    }
    let content_bottom = if rows_y.is_empty() { CONTENT_TOP } else { cursor - ROW_GAP };
    let lane_bottom = content_bottom + LANE_BOTTOM_PADDING;
    let lane_w = LANE_CONTENT_WIDTH + LANE_PADDING_X * 2.0;

    let mut cards: Vec<Card> = Vec::new();
    let mut card_of: HashMap<&str, usize> = HashMap::new();
    let mut lane_x = DIAGRAM_MARGIN;
    let mut content_lefts = Vec::new();
    for (li, lane) in lanes.iter().enumerate() {
        let Some(rows) = seating.rows_by_lane.get(&li) else { continue };
        let mut p = Placed::new(
            ItemId::Lane(lane.id.clone()),
            PlacedKind::Lane,
            [lane_x, LANE_TOP, lane_w, lane_bottom - LANE_TOP],
            lane_header(lane),
        );
        p.delta = lane.delta.unwrap_or(GraphDelta::Unchanged);
        layout.items.push(p);
        let content_x = lane_x + LANE_PADDING_X;
        content_lefts.push(content_x);
        let lane_slot = content_lefts.len() - 1;
        for r in rows {
            let top = rows_y[r.grid].0;
            let widths = if r.nodes.len() < 2 {
                vec![LANE_CONTENT_WIDTH]
            } else {
                let half = ((LANE_CONTENT_WIDTH - CARD_GAP_X) / 2.0).round();
                vec![half, LANE_CONTENT_WIDTH - CARD_GAP_X - half]
            };
            let mut x = content_x;
            for (n, w) in r.nodes.iter().zip(widths) {
                let rect = [x, top, w, card_height(n)];
                let earned = if w >= ICON_MIN_CARD_WIDTH { TITLE_SIZE } else { TITLE_SIZE_SMALL };
                let mut p = Placed::new(ItemId::Node(n.id.clone()), PlacedKind::Card, rect, n.label.clone());
                p.subtitle = n.subtitle.clone();
                p.delta = n.delta;
                p.badges = fitted_badges(&n.badges, n.delta, w);
                p.glyph = Some(n.kind.clone());
                p.title_size = fitted_title_size(&n.label, earned, card_text_width(w));
                p.parent = Some(ItemId::Lane(lane.id.clone()));
                layout.items.push(p);
                card_of.insert(n.id.as_str(), cards.len());
                cards.push(Card {
                    rect,
                    lane: lane_slot,
                    row: r.grid,
                });
                x += w + CARD_GAP_X;
            }
        }
        lane_x += lane_w + LANE_GAP;
    }
    layout.width = (lane_x - LANE_GAP + DIAGRAM_MARGIN).ceil().max(DIAGRAM_MARGIN * 2.0);
    layout.height = (lane_bottom + DIAGRAM_MARGIN).ceil();

    // Corridor `i` runs left of lane `i`; one more after the last lane.
    let corridors: Vec<(f32, f32)> = (0..=content_lefts.len())
        .map(|i| {
            let right = content_lefts
                .get(i)
                .copied()
                .unwrap_or_else(|| content_lefts.last().map_or(0.0, |l| l + LANE_CONTENT_WIDTH) + 52.0);
            let left = if i == 0 {
                right - 52.0
            } else {
                content_lefts[i - 1] + LANE_CONTENT_WIDTH
            };
            (left, if i == content_lefts.len() { left + 52.0 } else { right })
        })
        .collect();
    // Band `r` runs under row `r`; band `rows` sits under the last row, above the lane floor.
    let bands: Vec<(f32, f32)> = rows_y.iter().map(|(top, h)| (top + h, top + h + ROW_GAP)).collect();

    let plans: Vec<Plan> = scoped
        .edges
        .iter()
        .filter_map(|e| {
            let (&a, &b) = (card_of.get(e.from.as_str())?, card_of.get(e.to.as_str())?);
            Some(plan(e, a, b, &cards, bands.len()))
        })
        .collect();
    route_all(&mut layout, &plans, &cards, &corridors, &bands);
    layout.fit_canvas();
    layout
}

fn plan<'g>(edge: &'g GraphEdge, a: usize, b: usize, cards: &[Card], band_count: usize) -> Plan<'g> {
    let (ca, cb) = (&cards[a], &cards[b]);
    let band_under = |row: usize| Y::Band(row.min(band_count.saturating_sub(1)));
    if a == b {
        let c = ca.lane + 1;
        return Plan {
            edge,
            from: (a, Face::Right),
            to: (b, Face::Right),
            via: vec![(X::Corridor(c), Y::At(f32::NAN)), (X::Corridor(c), Y::At(f32::NAN))],
        };
    }
    if ca.lane == cb.lane {
        if ca.row == cb.row {
            let (fa, fb) = if ca.rect[0] < cb.rect[0] {
                (Face::Right, Face::Left)
            } else {
                (Face::Left, Face::Right)
            };
            return Plan {
                edge,
                from: (a, fa),
                to: (b, fb),
                via: Vec::new(),
            };
        }
        let between = cards
            .iter()
            .any(|c| c.lane == ca.lane && c.row > ca.row.min(cb.row) && c.row < ca.row.max(cb.row));
        if cb.row > ca.row && !between {
            let same_x = (ca.rect[0] - cb.rect[0]).abs() < 1.0 && (ca.rect[2] - cb.rect[2]).abs() < 1.0;
            let via = if same_x {
                Vec::new()
            } else {
                vec![(X::At(f32::NAN), band_under(ca.row)), (X::At(f32::NAN), band_under(ca.row))]
            };
            return Plan {
                edge,
                from: (a, Face::Bottom),
                to: (b, Face::Top),
                via,
            };
        }
        let c = ca.lane + 1;
        return Plan {
            edge,
            from: (a, Face::Right),
            to: (b, Face::Right),
            via: vec![(X::Corridor(c), Y::At(f32::NAN)), (X::Corridor(c), Y::At(f32::NAN))],
        };
    }
    let rightward = ca.lane < cb.lane;
    let (fa, fb) = if rightward {
        (Face::Right, Face::Left)
    } else {
        (Face::Left, Face::Right)
    };
    let near_a = if rightward { ca.lane + 1 } else { ca.lane };
    let near_b = if rightward { cb.lane } else { cb.lane + 1 };
    if near_a == near_b {
        return Plan {
            edge,
            from: (a, fa),
            to: (b, fb),
            via: vec![(X::Corridor(near_a), Y::At(f32::NAN)), (X::Corridor(near_a), Y::At(f32::NAN))],
        };
    }
    let band = if cb.row > 0 { Y::Band(cb.row - 1) } else { band_under(cb.row) };
    Plan {
        edge,
        from: (a, fa),
        to: (b, fb),
        via: vec![
            (X::Corridor(near_a), Y::At(f32::NAN)),
            (X::Corridor(near_a), band),
            (X::Corridor(near_b), band),
            (X::Corridor(near_b), Y::At(f32::NAN)),
        ],
    }
}

/// One route end on a card face: (plan index, is the start, where the other end sits along the face).
type PortEnd = (usize, bool, f32);

fn route_all(layout: &mut Layout, plans: &[Plan], cards: &[Card], corridors: &[(f32, f32)], bands: &[(f32, f32)]) {
    let centre = |c: &Card| [c.rect[0] + c.rect[2] / 2.0, c.rect[1] + c.rect[3] / 2.0];

    // Ports: what leaves or lands on one face spreads along it, ordered by where the other end is.
    let mut faces: BTreeMap<(usize, Face), Vec<PortEnd>> = BTreeMap::new();
    for (i, p) in plans.iter().enumerate() {
        let other_a = centre(&cards[p.to.0]);
        let other_b = centre(&cards[p.from.0]);
        let key = |f: Face, o: [f32; 2]| if matches!(f, Face::Left | Face::Right) { o[1] } else { o[0] };
        faces.entry(p.from).or_default().push((i, true, key(p.from.1, other_a)));
        faces.entry(p.to).or_default().push((i, false, key(p.to.1, other_b)));
    }
    let mut ports: HashMap<(usize, bool), [f32; 2]> = HashMap::new();
    for ((card, face), mut ends) in faces {
        ends.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)).then(a.1.cmp(&b.1)));
        let r = cards[card].rect;
        let along = if matches!(face, Face::Left | Face::Right) { r[3] } else { r[2] };
        let n = ends.len();
        let pitch = if n < 2 {
            0.0
        } else {
            PORT_PITCH.min((along - PORT_INSET * 2.0).max(0.0) / (n - 1) as f32)
        };
        for (k, (i, start, _)) in ends.into_iter().enumerate() {
            let off = (k as f32 - (n - 1) as f32 / 2.0) * pitch;
            let p = match face {
                Face::Left => [r[0], r[1] + r[3] / 2.0 + off],
                Face::Right => [r[0] + r[2], r[1] + r[3] / 2.0 + off],
                Face::Top => [r[0] + r[2] / 2.0 + off, r[1]],
                Face::Bottom => [r[0] + r[2] / 2.0 + off, r[1] + r[3]],
            };
            ports.insert((i, start), p);
        }
    }

    // Tracks: one per route per corridor or band, ordered so neighbours cross as little as they can.
    let mut corridor_use: BTreeMap<usize, Vec<(usize, f32)>> = BTreeMap::new();
    let mut band_use: BTreeMap<usize, Vec<(usize, f32)>> = BTreeMap::new();
    for (i, p) in plans.iter().enumerate() {
        let (s, t) = (ports[&(i, true)], ports[&(i, false)]);
        for (x, y) in &p.via {
            if let X::Corridor(c) = x {
                let v = corridor_use.entry(*c).or_default();
                if !v.iter().any(|(j, _)| *j == i) {
                    v.push((i, t[1]));
                }
            }
            if let Y::Band(b) = y {
                let v = band_use.entry(*b).or_default();
                if !v.iter().any(|(j, _)| *j == i) {
                    v.push((i, s[0].min(t[0])));
                }
            }
        }
    }
    let assign = |uses: BTreeMap<usize, Vec<(usize, f32)>>, gaps: &[(f32, f32)]| -> HashMap<(usize, usize), f32> {
        let mut out = HashMap::new();
        for (g, mut v) in uses {
            v.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let (lo, hi) = gaps.get(g).copied().unwrap_or((0.0, 52.0));
            for ((i, _), at) in v.iter().zip(spread(lo, hi, v.len())) {
                out.insert((g, *i), at);
            }
        }
        out
    };
    let cx = assign(corridor_use, corridors);
    let by = assign(band_use, bands);
    let obstacles: Vec<[f32; 4]> = layout
        .items
        .iter()
        .filter(|i| i.kind == PlacedKind::Card)
        .flat_map(|i| std::iter::once(i.rect).chain(i.badge_strip()))
        .collect();

    for (i, p) in plans.iter().enumerate() {
        let (s, t) = (ports[&(i, true)], ports[&(i, false)]);
        let mut pts = vec![s];
        let n = p.via.len();
        for (k, (x, y)) in p.via.iter().enumerate() {
            let prev = *pts.last().unwrap_or(&s);
            let x = match x {
                X::Corridor(c) => cx.get(&(*c, i)).copied().unwrap_or(prev[0]),
                X::At(v) if v.is_nan() => {
                    if k + 1 == n {
                        t[0]
                    } else {
                        prev[0]
                    }
                }
                X::At(v) => *v,
            };
            let y = match y {
                Y::Band(b) => by.get(&(*b, i)).copied().unwrap_or(prev[1]),
                Y::At(v) if v.is_nan() => {
                    if k + 1 == n {
                        t[1]
                    } else {
                        prev[1]
                    }
                }
                Y::At(v) => *v,
            };
            pts.push([x, y]);
        }
        if pts.len() == 1 && (s[0] - t[0]).abs() > 0.01 && (s[1] - t[1]).abs() > 0.01 {
            // Spread ports leave a direct link a little askew: jog it at the middle.
            if matches!(p.from.1, Face::Top | Face::Bottom) {
                let mid = (s[1] + t[1]) / 2.0;
                pts.extend([[s[0], mid], [t[0], mid]]);
            } else {
                let mid = (s[0] + t[0]) / 2.0;
                pts.extend([[mid, s[1]], [mid, t[1]]]);
            }
        }
        pts.push(t);
        pts.dedup_by(|a, b| (a[0] - b[0]).abs() < 0.01 && (a[1] - b[1]).abs() < 0.01);
        let e = p.edge;
        let mut r = Route::new(ItemId::Edge(e.id.clone()), RouteKind::Edge, pts);
        r.delta = e.delta;
        r.emphasis = e.emphasis;
        r.animated = e.animated;
        r.ends = Some((ItemId::Node(e.from.clone()), ItemId::Node(e.to.clone())));
        r.label = e.label.as_ref().map(|l| (l.clone(), pill_clear_of(&r.points, l, &obstacles)));
        layout.routes.push(r);
    }
}

pub(crate) fn pill_size(label: &str) -> [f32; 2] {
    [measure(label, PILL_TEXT_SIZE) + PILL_PADDING_X * 2.0, PILL_HEIGHT]
}

/// The pill where it keeps `PILL_CARD_CLEARANCE` from every obstacle: the middle of a level run
/// first, then along any run in 4-point steps; failing all, `pill_on`'s place.
pub(crate) fn pill_clear_of(points: &[[f32; 2]], label: &str, obstacles: &[[f32; 4]]) -> [f32; 4] {
    const CLEAR: f32 = 6.0;
    let [w, h] = pill_size(label);
    let free = |c: [f32; 2]| {
        let r = [c[0] - w / 2.0 - CLEAR, c[1] - h / 2.0 - CLEAR, w + CLEAR * 2.0, h + CLEAR * 2.0];
        !obstacles
            .iter()
            .any(|o| r[0] < o[0] + o[2] && o[0] < r[0] + r[2] && r[1] < o[1] + o[3] && o[1] < r[1] + r[3])
    };
    let mut segs: Vec<[[f32; 2]; 2]> = points.windows(2).map(|s| [s[0], s[1]]).collect();
    let level = |s: &[[f32; 2]; 2]| (s[0][1] - s[1][1]).abs() < 0.5;
    let len = |s: &[[f32; 2]; 2]| ((s[1][0] - s[0][0]).powi(2) + (s[1][1] - s[0][1]).powi(2)).sqrt();
    segs.sort_by(|a, b| level(b).cmp(&level(a)).then(len(b).total_cmp(&len(a))));
    for s in &segs {
        let n = (len(s) / 4.0).floor() as usize;
        let mut ts: Vec<f32> = (0..=n).map(|i| if n == 0 { 0.5 } else { i as f32 / n as f32 }).collect();
        ts.sort_by(|a, b| (a - 0.5).abs().total_cmp(&(b - 0.5).abs()));
        for t in ts {
            let c = [s[0][0] + (s[1][0] - s[0][0]) * t, s[0][1] + (s[1][1] - s[0][1]) * t];
            if free(c) {
                return [c[0] - w / 2.0, c[1] - h / 2.0, w, h];
            }
        }
    }
    pill_on(points, label)
}

/// The pill box on the route's longest level run, else on its longest run.
pub(crate) fn pill_on(points: &[[f32; 2]], label: &str) -> [f32; 4] {
    let [w, h] = pill_size(label);
    let len = |a: [f32; 2], b: [f32; 2]| ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
    let level = points
        .windows(2)
        .filter(|s| (s[0][1] - s[1][1]).abs() < 0.5)
        .max_by(|a, b| len(a[0], a[1]).total_cmp(&len(b[0], b[1])));
    let seg = level
        .or_else(|| points.windows(2).max_by(|a, b| len(a[0], a[1]).total_cmp(&len(b[0], b[1]))))
        .map(|s| [s[0], s[1]]);
    let mid = seg.map_or(points.first().copied().unwrap_or([0.0, 0.0]), |[a, b]| {
        [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]
    });
    [mid[0] - w / 2.0, mid[1] - h / 2.0, w, h]
}
