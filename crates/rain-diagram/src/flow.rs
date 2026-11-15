//! Data-flow lens: one flow as a sequence diagram (PR Lens `layout/dataflow.ts`): participants as
//! columns headed by node cards, messages as rows top-down, activation bars for sync calls,
//! self-loops, and notes and repeat bands the PR Lens SVG leaves to hover.

use std::collections::HashMap;

use rain_core::overview::{GraphDelta, GraphFlow, GraphFlowMessage, GraphLens, GraphNode, MessageKind, OverviewGraph};

use crate::arch::{card_text_width, fitted_badges, fitted_title_size};
use crate::design::*;
use crate::layout::{ItemId, Layout, Placed, PlacedKind, Route, RouteKind};

struct Column<'g> {
    node: &'g str,
    label: String,
    subtitle: Option<String>,
    kind: String,
    delta: GraphDelta,
    badges: Vec<String>,
}

fn columns<'g>(flow: &'g GraphFlow, nodes: &HashMap<&str, &'g GraphNode>) -> Vec<Column<'g>> {
    let mut ids: Vec<&str> = flow.participants.iter().map(|p| p.node.as_str()).collect();
    for m in &flow.messages {
        for id in [m.from.as_str(), m.to.as_str()] {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids.into_iter()
        .map(|id| {
            let own = flow.participants.iter().find(|p| p.node == id).and_then(|p| p.label.clone());
            match nodes.get(id) {
                Some(n) => Column {
                    node: id,
                    label: own.unwrap_or_else(|| n.label.clone()),
                    subtitle: n.subtitle.clone(),
                    kind: n.kind.clone(),
                    delta: n.delta,
                    badges: n.badges.clone(),
                },
                None => Column {
                    node: id,
                    label: own.unwrap_or_else(|| id.to_owned()),
                    subtitle: None,
                    kind: "other".into(),
                    delta: GraphDelta::Unchanged,
                    badges: Vec::new(),
                },
            }
        })
        .collect()
}

fn card_height(c: &Column) -> f32 {
    if c.subtitle.is_some() {
        CARD_HEIGHT_WITH_SUBTITLE
    } else {
        CARD_HEIGHT
    }
}

fn card_content_width(c: &Column) -> f32 {
    CARD_PADDING_X * 2.0
        + ICON_CHIP_SIZE
        + ICON_CHIP_GAP
        + measure(&c.label, TITLE_SIZE).max(c.subtitle.as_deref().map_or(0.0, |s| measure(s, SUBTITLE_SIZE)))
}

pub fn message_pitch(m: &GraphFlowMessage) -> f32 {
    if m.kind == MessageKind::SelfCall {
        SELF_MESSAGE_PITCH
    } else {
        MESSAGE_PITCH
    }
}

/// The label as drawn: a repeat count folded in.
pub fn message_label(m: &GraphFlowMessage) -> String {
    match m.repeat {
        Some(n) if n != 1 => format!("{} ×{n}", m.label),
        _ => m.label.clone(),
    }
}

/// A sync call activates its receiver from where it lands to the first unclaimed return back to
/// the caller, or through the receiver's last involvement when nothing answers it.
fn activations(node: &str, messages: &[(&GraphFlowMessage, f32)]) -> Vec<(f32, f32)> {
    let mut claimed = vec![false; messages.len()];
    let mut bars: Vec<(f32, f32)> = Vec::new();
    for (i, (m, y)) in messages.iter().enumerate() {
        if m.kind != MessageKind::Sync || m.to != node {
            continue;
        }
        let answer = (i + 1..messages.len()).find(|&j| {
            !claimed[j] && messages[j].0.kind == MessageKind::Return && messages[j].0.from == node && messages[j].0.to == m.from
        });
        if let Some(j) = answer {
            claimed[j] = true;
            bars.push((*y, messages[j].1));
            continue;
        }
        let last = messages[i..]
            .iter()
            .filter(|(c, _)| c.from == node || c.to == node)
            .map(|(_, y)| *y)
            .fold(*y, f32::max);
        bars.push((*y, last + MESSAGE_PITCH / 3.0));
    }
    let mut merged: Vec<(f32, f32)> = Vec::new();
    for b in bars {
        match merged.last_mut() {
            Some(cur) if b.0 <= cur.1 => cur.1 = cur.1.max(b.1),
            _ => merged.push(b),
        }
    }
    merged
}

/// Lays out flow `flow_id` of `graph`, or `None` when the graph has no such flow.
pub fn data_flow(graph: &OverviewGraph, flow_id: &str) -> Option<Layout> {
    let flow = graph.flows.iter().find(|f| f.id == flow_id)?;
    let nodes: HashMap<&str, &GraphNode> = graph.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let cols = columns(flow, &nodes);
    let mut layout = Layout::new(GraphLens::DataFlow, flow.title.clone());
    let fid = flow.id.clone();

    let col_w = cols.iter().map(card_content_width).fold(COLUMN_MIN_WIDTH, f32::max).ceil();
    let centre_of: HashMap<&str, f32> = cols
        .iter()
        .enumerate()
        .map(|(i, c)| (c.node, DIAGRAM_MARGIN + col_w / 2.0 + i as f32 * (col_w + COLUMN_GAP)))
        .collect();
    let header_h = cols.iter().map(card_height).fold(0.0, f32::max);
    let lifeline_top = PARTICIPANT_TOP + header_h + LIFELINE_GAP;

    let mut y = lifeline_top + FIRST_MESSAGE_DROP;
    let placed: Vec<(&GraphFlowMessage, f32)> = flow
        .messages
        .iter()
        .map(|m| {
            let at = y;
            y += message_pitch(m);
            (m, at)
        })
        .collect();
    let drawn = placed
        .iter()
        .map(|(m, y)| y + if m.kind == MessageKind::SelfCall { SELF_LOOP_EXTENT } else { 0.0 })
        .fold(lifeline_top + FIRST_MESSAGE_DROP, f32::max);
    let bottom = drawn + FLOW_BOTTOM_PADDING;
    let last_right = DIAGRAM_MARGIN + cols.len() as f32 * (col_w + COLUMN_GAP) - COLUMN_GAP;

    let mut bars_of: HashMap<&str, Vec<(f32, f32)>> = HashMap::new();
    for c in &cols {
        let cx = centre_of[c.node];
        let mut life = Route::new(
            ItemId::Lifeline {
                flow: fid.clone(),
                node: c.node.to_owned(),
            },
            RouteKind::Lifeline,
            vec![[cx, lifeline_top], [cx, bottom]],
        );
        life.head = false;
        layout.routes.push(life);
        let rect = [cx - col_w / 2.0, PARTICIPANT_TOP, col_w, card_height(c)];
        let mut p = Placed::new(
            ItemId::Participant {
                flow: fid.clone(),
                node: c.node.to_owned(),
            },
            PlacedKind::Participant,
            rect,
            c.label.clone(),
        );
        p.subtitle = c.subtitle.clone();
        p.delta = c.delta;
        p.glyph = Some(c.kind.clone());
        p.badges = fitted_badges(&c.badges, c.delta, col_w);
        p.title_size = fitted_title_size(&c.label, TITLE_SIZE, card_text_width(col_w));
        layout.items.push(p);
        let bars = activations(c.node, &placed);
        for (at, (top, bot)) in bars.iter().enumerate() {
            let mut a = Placed::new(
                ItemId::Activation {
                    flow: fid.clone(),
                    node: c.node.to_owned(),
                    at,
                },
                PlacedKind::Activation,
                [cx - ACTIVATION_HALF_WIDTH, *top, ACTIVATION_HALF_WIDTH * 2.0, bot - top],
                "",
            );
            a.parent = Some(ItemId::Participant {
                flow: fid.clone(),
                node: c.node.to_owned(),
            });
            layout.items.push(a);
        }
        bars_of.insert(c.node, bars);
    }
    let active = |node: &str, y: f32| {
        bars_of
            .get(node)
            .is_some_and(|b| b.iter().any(|(t, bt)| y >= *t - 0.5 && y <= *bt + 0.5))
    };

    for (m, y) in &placed {
        let (fx, tx) = (centre_of[m.from.as_str()], centre_of[m.to.as_str()]);
        let id = ItemId::Message {
            flow: fid.clone(),
            id: m.id.clone(),
        };
        let label = message_label(m);
        let self_call = m.kind == MessageKind::SelfCall || m.from == m.to;
        let (kind, points, label_box) = if self_call {
            let x0 = fx + if active(&m.from, *y) { ACTIVATION_HALF_WIDTH } else { 0.0 };
            let low = y + SELF_LOOP_DROP + SELF_LOOP_CORNER * 2.0;
            let reach = fx + SELF_LOOP_REACH;
            let w = measure(&label, PILL_TEXT_SIZE) + 8.0;
            (
                RouteKind::SelfCall,
                vec![[x0, *y], [reach, *y], [reach, low], [x0, low]],
                [reach + 6.0, y - 7.0, w, 14.0],
            )
        } else {
            let dir = if tx >= fx { 1.0 } else { -1.0 };
            let s = fx + if active(&m.from, *y) { ACTIVATION_HALF_WIDTH * dir } else { 0.0 };
            let t = tx - if active(&m.to, *y) { ACTIVATION_HALF_WIDTH * dir } else { 0.0 };
            let kind = match m.kind {
                MessageKind::Async => RouteKind::Async,
                MessageKind::Return => RouteKind::Return,
                _ => RouteKind::Sync,
            };
            let w = measure(&label, PILL_TEXT_SIZE) + 8.0;
            (kind, vec![[s, *y], [t, *y]], [(fx + tx) / 2.0 - w / 2.0, y - 18.0, w, 14.0])
        };
        let mut r = Route::new(id.clone(), kind, points);
        r.delta = m.delta;
        r.animated = m.animated;
        r.label = Some((label, label_box));
        r.ends = Some((
            ItemId::Participant {
                flow: fid.clone(),
                node: m.from.clone(),
            },
            ItemId::Participant {
                flow: fid.clone(),
                node: m.to.clone(),
            },
        ));
        layout.routes.push(r);

        if let Some(n) = m.repeat.filter(|n| *n > 1) {
            let (lo, hi) = (
                fx.min(tx) - 24.0,
                fx.max(tx) + if self_call { SELF_LOOP_REACH + 12.0 } else { 24.0 },
            );
            let mut p = Placed::new(
                ItemId::Repeat {
                    flow: fid.clone(),
                    id: m.id.clone(),
                },
                PlacedKind::Repeat,
                [lo, y - 26.0, hi - lo, 36.0],
                format!("loop ×{n}"),
            );
            p.parent = Some(id.clone());
            layout.items.push(p);
        }
        if let Some(note) = &m.note {
            let mut p = Placed::new(
                ItemId::Note {
                    flow: fid.clone(),
                    id: m.id.clone(),
                },
                PlacedKind::Note,
                [last_right + NOTE_GAP, y - 10.0, NOTE_WIDTH, 20.0],
                note.clone(),
            );
            p.parent = Some(id);
            layout.items.push(p);
        }
    }
    layout.width = (last_right + DIAGRAM_MARGIN).ceil();
    layout.height = (bottom + DIAGRAM_MARGIN).ceil();
    layout.fit_canvas();
    Some(layout)
}
