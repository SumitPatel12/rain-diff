//! Data-model lens (PR Lens `layout/datamodel.ts` with difit's index patch): stores as lanes,
//! tables as cards with folded column and index rows, relations routed through the corridors
//! beside the lanes or over the top when they skip a lane, coloured by key family. Not ported:
//! trunks (four or more arrivals merging into one leg) and lifting labelled routes over the top.

use std::collections::{BTreeMap, HashMap, HashSet};

use rain_core::overview::{
    ColumnKey, GraphDelta, GraphLens, GraphRelation, GraphStore, GraphTable, GraphTableColumn, GraphTableIndex, OverviewGraph,
};

use crate::arch::{fitted_badges, pill_clear_of};
use crate::design::*;
use crate::layout::{Chip, ItemId, Layout, Placed, PlacedKind, Route, RouteKind, spread};
use crate::scope::Scoped;

pub struct ColumnFold<'g> {
    pub shown: Vec<&'g GraphTableColumn>,
    pub hidden: usize,
    pub unchanged_only: bool,
}

/// Keys, changed columns and relation ends always draw; unchanged ones up to a few; never past
/// the ceiling; a fold hiding fewer than `TABLE_FOLD_MIN_HIDDEN` is not worth its row.
pub fn fold_columns<'g>(table: &'g GraphTable, relations: &[&GraphRelation]) -> ColumnFold<'g> {
    let mut ends: HashSet<&str> = relations
        .iter()
        .flat_map(|r| [&r.from, &r.to])
        .filter(|e| e.table == table.id)
        .map(|e| e.column.as_str())
        .collect();
    for i in table.indexes.iter().filter(|i| i.delta != GraphDelta::Unchanged) {
        ends.extend(i.columns.iter().map(String::as_str));
    }
    let tier = |c: &GraphTableColumn| {
        if c.key.is_some() || ends.contains(c.name.as_str()) {
            0
        } else if c.delta != GraphDelta::Unchanged {
            1
        } else {
            2
        }
    };
    let mut keep = vec![false; table.columns.len()];
    let (mut kept, mut unchanged) = (0, 0);
    'tiers: for wanted in 0..3 {
        for (i, c) in table.columns.iter().enumerate() {
            if kept >= TABLE_ROWS_MAX {
                break 'tiers;
            }
            if tier(c) != wanted {
                continue;
            }
            if wanted == 2 {
                if unchanged >= TABLE_UNCHANGED_ROWS_MAX {
                    break 'tiers;
                }
                unchanged += 1;
            }
            keep[i] = true;
            kept += 1;
        }
    }
    let hidden = table.columns.len() - kept;
    if hidden < TABLE_FOLD_MIN_HIDDEN {
        return ColumnFold {
            shown: table.columns.iter().collect(),
            hidden: 0,
            unchanged_only: true,
        };
    }
    ColumnFold {
        shown: table.columns.iter().zip(&keep).filter(|(_, k)| **k).map(|(c, _)| c).collect(),
        hidden,
        unchanged_only: table.columns.iter().zip(&keep).all(|(c, k)| *k || c.delta == GraphDelta::Unchanged),
    }
}

/// Changed indexes always draw; unchanged ones up to a few, the rest counted on one fold row.
pub fn fold_indexes(table: &GraphTable) -> (Vec<&GraphTableIndex>, usize) {
    let mut unchanged = 0;
    let shown: Vec<&GraphTableIndex> = table
        .indexes
        .iter()
        .filter(|i| {
            if i.delta != GraphDelta::Unchanged {
                return true;
            }
            unchanged += 1;
            unchanged <= TABLE_INDEX_UNCHANGED_MAX
        })
        .collect();
    let hidden = table.indexes.len() - shown.len();
    (shown, hidden)
}

/// Relations pointing at the same target column share a family, numbered in the order the whole
/// document first names each target, so a relation keeps its hue in every view.
pub fn key_families(relations: &[GraphRelation]) -> HashMap<String, usize> {
    let mut by_target: HashMap<(&str, &str), usize> = HashMap::new();
    let mut out = HashMap::new();
    for r in relations {
        let n = by_target.len();
        let f = *by_target.entry((r.to.table.as_str(), r.to.column.as_str())).or_insert(n);
        out.insert(r.id.clone(), f);
    }
    out
}

/// `order` when any store sets it; else the store most keys point into sits in the middle.
fn order_stores<'g>(s: &Scoped<'g>) -> Vec<&'g GraphStore> {
    if s.stores.iter().any(|st| st.order.is_some()) {
        let mut v: Vec<(usize, &GraphStore)> = s.stores.iter().copied().enumerate().collect();
        v.sort_by_key(|(i, st)| (st.order.unwrap_or(u32::MAX), *i));
        return v.into_iter().map(|(_, st)| st).collect();
    }
    let store_of: HashMap<&str, &str> = s.tables.iter().map(|t| (t.id.as_str(), t.store.as_str())).collect();
    let mut incoming: HashMap<&str, usize> = HashMap::new();
    for r in &s.relations {
        let Some(&target) = store_of.get(r.to.table.as_str()) else {
            continue;
        };
        if Some(&target) == store_of.get(r.from.table.as_str()) {
            continue;
        }
        *incoming.entry(target).or_default() += 1;
    }
    let count = |st: &GraphStore| incoming.get(st.id.as_str()).copied().unwrap_or(0);
    let hub = s.stores.iter().copied().fold(None::<&GraphStore>, |best, st| match best {
        Some(b) if count(st) <= count(b) => Some(b),
        _ => Some(st),
    });
    let Some(hub) = hub.filter(|h| count(h) > 0) else {
        return s.stores.clone();
    };
    let mut rest: Vec<&GraphStore> = s.stores.iter().copied().filter(|st| st.id != hub.id).collect();
    let middle = (s.stores.len() - 1) / 2;
    rest.insert(middle, hub);
    rest
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Left,
    Right,
}

struct LanePlan {
    from_side: Side,
    to_side: Side,
    corridors: Vec<usize>,
    over: bool,
}

/// A neighbouring lane is reached through the corridor between; the same lane steps into the
/// corridor beside it and back; a lane further away is crossed above the lanes.
fn plan_lanes(la: usize, lb: usize, last: usize) -> LanePlan {
    if la == lb {
        let corridor = if la < last { la + 1 } else { la };
        let side = if corridor == la + 1 { Side::Right } else { Side::Left };
        return LanePlan {
            from_side: side,
            to_side: side,
            corridors: vec![corridor],
            over: false,
        };
    }
    let rightward = la < lb;
    let from_side = if rightward { Side::Right } else { Side::Left };
    let near = if rightward { Side::Left } else { Side::Right };
    if la.abs_diff(lb) == 1 {
        return LanePlan {
            from_side,
            to_side: near,
            corridors: vec![la.max(lb)],
            over: false,
        };
    }
    LanePlan {
        from_side,
        to_side: near,
        corridors: if rightward { vec![la + 1, lb] } else { vec![la, lb + 1] },
        over: true,
    }
}

const GUTTER: f32 = LANE_PADDING_X * 2.0 + LANE_GAP;

fn corridor_extra(legs: usize) -> f32 {
    (TRACK_CLEARANCE * 2.0 + TRACK_PITCH_MIN * legs.saturating_sub(1) as f32 - GUTTER).max(0.0)
}

fn key_chip(k: ColumnKey) -> Chip {
    match k {
        ColumnKey::Primary => Chip::Primary,
        ColumnKey::Foreign => Chip::Foreign,
        ColumnKey::Index => Chip::Index,
    }
}

struct PlacedTable {
    rect: [f32; 4],
    rows: HashMap<String, f32>,
}

/// Lays out the data-model lens of `scoped`; `graph` supplies the document-wide key families.
pub fn data_model(graph: &OverviewGraph, scoped: &Scoped, title: &str) -> Layout {
    let mut layout = Layout::new(GraphLens::DataModel, title);
    let all_relations: &[GraphRelation] = graph.data_model.as_ref().map_or(&[], |m| m.relations.as_slice());
    let families = key_families(all_relations);
    let stores = order_stores(scoped);
    let lane_of: HashMap<&str, usize> = {
        let idx: HashMap<&str, usize> = stores.iter().enumerate().map(|(i, s)| (s.id.as_str(), i)).collect();
        scoped
            .tables
            .iter()
            .map(|t| (t.id.as_str(), idx.get(t.store.as_str()).copied().unwrap_or(0)))
            .collect()
    };
    let last = stores.len().saturating_sub(1);
    let plans: Vec<(&GraphRelation, LanePlan)> = scoped
        .relations
        .iter()
        .map(|r| {
            (
                *r,
                plan_lanes(
                    lane_of.get(r.from.table.as_str()).copied().unwrap_or(0),
                    lane_of.get(r.to.table.as_str()).copied().unwrap_or(0),
                    last,
                ),
            )
        })
        .collect();
    let skips = plans.iter().filter(|(_, p)| p.over).count();
    let mut legs: BTreeMap<usize, usize> = BTreeMap::new();
    for (_, p) in &plans {
        for c in &p.corridors {
            *legs.entry(*c).or_default() += 1;
        }
    }
    let extra = |c: usize| corridor_extra(legs.get(&c).copied().unwrap_or(0));
    let over_band = if skips == 0 {
        0.0
    } else {
        skips as f32 * OVER_PITCH + OVER_CLEARANCE
    };
    let lane_top = LANE_TOP + over_band;
    let content_top = CONTENT_TOP + over_band;
    let lane_w = LANE_CONTENT_WIDTH + LANE_PADDING_X * 2.0;

    let mut lane_lefts = Vec::new();
    let mut lane_x = DIAGRAM_MARGIN;
    for i in 0..stores.len() {
        lane_x += extra(i);
        lane_lefts.push(lane_x);
        lane_x += lane_w + LANE_GAP;
    }
    let content_right = lane_x - LANE_GAP + extra(stores.len());

    // Hue per relation end: a target column takes its family's hue, a source column the hue of
    // the first family it points from.
    let mut hues: HashMap<(&str, &str), usize> = HashMap::new();
    let mut by_family: Vec<&&GraphRelation> = scoped.relations.iter().collect();
    by_family.sort_by_key(|r| families.get(&r.id).copied().unwrap_or(0));
    for r in &by_family {
        let f = families.get(&r.id).copied().unwrap_or(0);
        hues.entry((r.to.table.as_str(), r.to.column.as_str())).or_insert(f);
    }
    for r in &by_family {
        let f = families.get(&r.id).copied().unwrap_or(0);
        hues.entry((r.from.table.as_str(), r.from.column.as_str())).or_insert(f);
    }

    let mut tables: HashMap<&str, PlacedTable> = HashMap::new();
    let mut lane_bottom = content_top;
    for (li, store) in stores.iter().enumerate() {
        let x = lane_lefts[li] + LANE_PADDING_X;
        let mut y = content_top;
        for t in scoped.tables.iter().filter(|t| t.store == store.id) {
            let fold = fold_columns(t, &scoped.relations);
            let (indexes, idx_hidden) = fold_indexes(t);
            let rows_top = y + CARD_HEIGHT_WITH_SUBTITLE + TABLE_ROWS_PADDING_Y;
            let columns_bottom =
                rows_top + fold.shown.len() as f32 * TABLE_ROW_HEIGHT + if fold.hidden > 0 { TABLE_FOLD_HEIGHT } else { 0.0 };
            let has_index = !indexes.is_empty() || idx_hidden > 0;
            let index_section = if has_index {
                TABLE_INDEX_RULE_GAP
                    + indexes.len() as f32 * TABLE_INDEX_ROW_HEIGHT
                    + if idx_hidden > 0 { TABLE_INDEX_FOLD_HEIGHT } else { 0.0 }
            } else {
                0.0
            };
            let height = columns_bottom - y + TABLE_ROWS_PADDING_Y + index_section;
            let rect = [x, y, LANE_CONTENT_WIDTH, height];
            let tid = ItemId::Table(t.id.clone());
            let n = t.columns.len();
            let count = format!("{n} {}", if n == 1 { "column" } else { "columns" });
            let mut card = Placed::new(tid.clone(), PlacedKind::Table, rect, t.label.clone());
            card.subtitle = Some(match &t.name {
                Some(name) => format!("{name} · {count}"),
                None => count,
            });
            card.delta = t.delta;
            card.badges = fitted_badges(&t.badges, t.delta, LANE_CONTENT_WIDTH);
            card.glyph = Some("table".into());
            card.title_size = TITLE_SIZE;
            card.parent = Some(ItemId::Lane(store.id.clone()));
            layout.items.push(card);

            let mut rows = HashMap::new();
            for (i, c) in fold.shown.iter().enumerate() {
                let top = rows_top + i as f32 * TABLE_ROW_HEIGHT;
                rows.insert(c.name.clone(), top + TABLE_ROW_HEIGHT / 2.0);
                let mut p = Placed::new(
                    ItemId::Column {
                        table: t.id.clone(),
                        column: c.name.clone(),
                    },
                    PlacedKind::Column,
                    [x, top, LANE_CONTENT_WIDTH, TABLE_ROW_HEIGHT],
                    c.name.clone(),
                );
                p.delta = c.delta;
                p.chip = c.key.map(key_chip);
                p.family = hues.get(&(t.id.as_str(), c.name.as_str())).copied();
                p.aside = Some(c.ty.clone());
                p.previous = c.previous_type.clone();
                p.parent = Some(tid.clone());
                layout.items.push(p);
            }
            if fold.hidden > 0 {
                let top = rows_top + fold.shown.len() as f32 * TABLE_ROW_HEIGHT + TABLE_FOLD_GAP;
                let what = if fold.unchanged_only { "unchanged columns" } else { "more columns" };
                let mut p = Placed::new(
                    ItemId::Fold {
                        table: t.id.clone(),
                        indexes: false,
                    },
                    PlacedKind::Fold,
                    [x, top, LANE_CONTENT_WIDTH, TABLE_ROW_HEIGHT],
                    format!("+ {} {what}", fold.hidden),
                );
                p.parent = Some(tid.clone());
                layout.items.push(p);
            }
            let index_top = columns_bottom + TABLE_INDEX_RULE_GAP;
            for (i, ix) in indexes.iter().enumerate() {
                let top = index_top + i as f32 * TABLE_INDEX_ROW_HEIGHT;
                let mut p = Placed::new(
                    ItemId::Index {
                        table: t.id.clone(),
                        name: ix.name.clone(),
                    },
                    PlacedKind::Index,
                    [x, top, LANE_CONTENT_WIDTH, TABLE_INDEX_ROW_HEIGHT],
                    ix.name.clone(),
                );
                p.delta = ix.delta;
                p.chip = Some(if ix.unique { Chip::UniqueIndex } else { Chip::PlainIndex });
                p.aside = Some(ix.columns.join(", "));
                p.parent = Some(tid.clone());
                layout.items.push(p);
            }
            if idx_hidden > 0 {
                let top = index_top + indexes.len() as f32 * TABLE_INDEX_ROW_HEIGHT + TABLE_FOLD_GAP;
                let label = format!("+ {idx_hidden} unchanged {}", if idx_hidden == 1 { "index" } else { "indexes" });
                let mut p = Placed::new(
                    ItemId::Fold {
                        table: t.id.clone(),
                        indexes: true,
                    },
                    PlacedKind::Fold,
                    [x, top, LANE_CONTENT_WIDTH, TABLE_INDEX_ROW_HEIGHT],
                    label,
                );
                p.parent = Some(tid.clone());
                layout.items.push(p);
            }
            tables.insert(t.id.as_str(), PlacedTable { rect, rows });
            y += height + ROW_GAP;
        }
        lane_bottom = lane_bottom.max(y - ROW_GAP);
    }
    lane_bottom += LANE_BOTTOM_PADDING;
    for (i, s) in stores.iter().enumerate() {
        let label = match &s.platform {
            Some(p) => format!("{} · {}", s.label, p).to_uppercase(),
            None => s.label.to_uppercase(),
        };
        let mut p = Placed::new(
            ItemId::Lane(s.id.clone()),
            PlacedKind::Lane,
            [lane_lefts[i], lane_top, lane_w, lane_bottom - lane_top],
            label,
        );
        p.delta = s.delta.unwrap_or(GraphDelta::Unchanged);
        layout.items.insert(i, p);
    }

    let corridors: Vec<(f32, f32)> = (0..=stores.len())
        .map(|i| {
            if i < stores.len() {
                let right = lane_lefts[i] + LANE_PADDING_X;
                (right - GUTTER - extra(i), right)
            } else {
                let left = lane_lefts
                    .last()
                    .map_or(DIAGRAM_MARGIN, |l| l + LANE_PADDING_X + LANE_CONTENT_WIDTH);
                (left, left + GUTTER + extra(stores.len()))
            }
        })
        .collect();
    route(&mut layout, &plans, &tables, &corridors, lane_top, &families);
    layout.width = (content_right + DIAGRAM_MARGIN).ceil();
    layout.height = (lane_bottom + DIAGRAM_MARGIN).ceil();
    layout.fit_canvas();
    layout
}

fn route(
    layout: &mut Layout,
    plans: &[(&GraphRelation, LanePlan)],
    tables: &HashMap<&str, PlacedTable>,
    corridors: &[(f32, f32)],
    lane_top: f32,
    families: &HashMap<String, usize>,
) {
    struct End {
        x: f32,
        y: f32,
    }
    let face = |t: &PlacedTable, side: Side| if side == Side::Left { t.rect[0] } else { t.rect[0] + t.rect[2] };
    let mut routed: Vec<(usize, End, End)> = Vec::new();
    for (i, (r, p)) in plans.iter().enumerate() {
        let (Some(a), Some(b)) = (tables.get(r.from.table.as_str()), tables.get(r.to.table.as_str())) else {
            continue;
        };
        let (Some(&ya), Some(&yb)) = (a.rows.get(&r.from.column), b.rows.get(&r.to.column)) else {
            continue;
        };
        routed.push((
            i,
            End {
                x: face(a, p.from_side),
                y: ya,
            },
            End {
                x: face(b, p.to_side),
                y: yb,
            },
        ));
    }
    // Arrivals on one column fan out a few points so each keeps its own head.
    let mut landing: HashMap<usize, f32> = HashMap::new();
    let mut by_target: BTreeMap<(&str, &str), Vec<(usize, f32)>> = BTreeMap::new();
    for (i, s, _) in &routed {
        let r = plans[*i].0;
        by_target
            .entry((r.to.table.as_str(), r.to.column.as_str()))
            .or_default()
            .push((*i, s.y));
    }
    for v in by_target.values_mut() {
        v.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        let pitch = PORT_FAN_MAX.min((TABLE_ROW_HEIGHT - 2.0) / (v.len().max(2) - 1) as f32);
        for (k, (i, _)) in v.iter().enumerate() {
            landing.insert(*i, (k as f32 - (v.len() - 1) as f32 / 2.0) * pitch);
        }
    }
    // One track per leg a corridor carries, ordered by where the leg lands.
    let mut tracks: HashMap<(usize, usize), f32> = HashMap::new();
    for (c, gap) in corridors.iter().enumerate() {
        let mut members: Vec<(usize, usize, f32, f32)> = Vec::new();
        for (i, s, t) in &routed {
            for (leg, at) in plans[*i].1.corridors.iter().enumerate() {
                if *at == c {
                    members.push((*i, leg, t.y + landing.get(i).copied().unwrap_or(0.0), s.y));
                }
            }
        }
        members.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.3.total_cmp(&b.3)).then(a.0.cmp(&b.0)));
        for (m, x) in members.iter().zip(spread(gap.0, gap.1, members.len())) {
            tracks.insert((m.0, m.1), x);
        }
    }
    let obstacles: Vec<[f32; 4]> = layout
        .items
        .iter()
        .filter(|i| i.kind == PlacedKind::Table)
        .map(|i| i.rect)
        .collect();
    let mut over = 0;
    for (i, s, t) in routed {
        let (r, p) = (&plans[i].0, &plans[i].1);
        let land = t.y + landing.get(&i).copied().unwrap_or(0.0);
        let x1 = tracks.get(&(i, 0)).copied().unwrap_or(s.x);
        let points = if p.over {
            let top = lane_top - OVER_CLEARANCE - over as f32 * OVER_PITCH;
            over += 1;
            let x2 = tracks.get(&(i, 1)).copied().unwrap_or(t.x);
            vec![[s.x, s.y], [x1, s.y], [x1, top], [x2, top], [x2, land], [t.x, land]]
        } else {
            vec![[s.x, s.y], [x1, s.y], [x1, land], [t.x, land]]
        };
        let mut route = Route::new(ItemId::Relation(r.id.clone()), RouteKind::Relation { enforced: r.enforced }, points);
        route.delta = r.delta;
        route.family = families.get(&r.id).copied();
        route.ends = Some((
            ItemId::Column {
                table: r.from.table.clone(),
                column: r.from.column.clone(),
            },
            ItemId::Column {
                table: r.to.table.clone(),
                column: r.to.column.clone(),
            },
        ));
        route.label = r.label.as_ref().map(|l| (l.clone(), pill_clear_of(&route.points, l, &obstacles)));
        layout.routes.push(route);
    }
}
