//! Style 3, field lineage and ER morph: four columns of cards (database tables, entities,
//! DTOs, UI) whose rows are fields, each card with a line on why it is there, and thin strands
//! for each value's way from column to UI. The data model morphs first, as an ER diff does:
//! dropped columns turn red and collapse, retyped ones swap their type, kept cards glide to
//! make room. Then each new value draws its strand left to right, cards appearing as it reaches
//! them, and a dot runs every strand that now reaches the screen. A strand that reaches the API
//! but never the UI is flagged a dead end. Key fields wear a PK or FK tag, and a foreign key
//! within the database column bows to the key it references. A sentence on top says what
//! changed for the data.

use std::collections::HashMap;

use rune::{El, Shape, canvas, col, el, row, text};

use super::beats::{self, Caption, Tracks, layered, plural};
use super::bend::{self, Span};
use super::paint::{self, MICRO, MONO, SMALL, STRONG};
use crate::motion::{FocusTrack, Pulse, Rig, Track, WriteOn};
use crate::schema::state::{Field, FieldState, Stage, States};
use crate::schema::{SchemaId, Status};
use crate::style::{Cx, Style};

pub struct Lineage;

const MARGIN: f32 = 24.0;
/// The conclusion's room above the columns: two lines.
const SAYS: f32 = 44.0;
const HEAD: f32 = 30.0;
const FOOT: f32 = 104.0;
const CARD_HEAD: f32 = 26.0;
/// A note's line height and about how wide a character of it sets.
const NOTE_LH: f32 = 14.0;
const NOTE_CH: f32 = 5.4;
const ROW_H: f32 = 22.0;
const CARD_GAP: f32 = 18.0;
const RIBBON: f32 = 3.0;
/// A PK or FK tag before a field's name, and the room it takes.
const TAG_W: f32 = 22.0;
const TAG_ROOM: f32 = 26.0;
/// Room left of the columns for a foreign key's bow, and how far the bow reaches into it.
const GUTTER: f32 = 20.0;
const BOW_MAX: f32 = 36.0;

const HOLD: f32 = 0.9;
const TINT: f32 = 0.45;
const UNDRAW: f32 = 0.6;
const GLIDE: f32 = 1.0;
const ENTER: f32 = 0.4;
const HOP: f32 = 0.5;
const TRIP: f32 = 2.2;

pub(crate) struct Card {
    pub key: String,
    pub title: String,
    /// Why it is there, wrapped under the title.
    pub note: Option<String>,
    pub status: Status,
    pub rest: [f32; 4],
    pub rect: Track<[f32; 4]>,
    pub alpha: Track<f32>,
    pub head: Track<[f32; 2]>,
}

pub(crate) struct Row {
    pub id: String,
    pub name: String,
    pub why: Option<String>,
    pub status: Status,
    pub was_type: Option<String>,
    pub now_type: Option<String>,
    pub rest: [f32; 4],
    pub offset: Track<[f32; 2]>,
    pub alpha: Track<f32>,
    pub tint: Track<f32>,
    /// A retyped field's old and new types' opacity.
    pub retype: Option<(Track<f32>, Track<f32>)>,
    /// It reached the API before the change but never the UI.
    pub dead_end: bool,
    pub key: Option<KeyMark>,
}

/// A key field's tag: its owner's primary key, or a foreign key naming the field it points at.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum KeyMark {
    Primary,
    /// The referenced field's id and its `Owner.name`.
    Foreign(String, String),
}

pub(crate) struct Hop {
    pub key: String,
    pub from: String,
    pub to: String,
    pub status: Status,
    pub unit: [[f32; 2]; 4],
    pub rest: [f32; 4],
    pub rect: Track<[f32; 4]>,
    pub tint: Track<f32>,
    pub trim: Option<WriteOn>,
}

/// A foreign key between two rows of one column: a bow down the column's left side, its head
/// at the key it points to.
pub(crate) struct Key {
    pub from: String,
    pub to: String,
    pub status: Status,
    pub span: Span,
    pub trim: WriteOn,
    /// The head lands as the bow finishes drawing.
    pub head: Track<f32>,
}

pub(crate) struct Flow {
    pub columns: Vec<(String, f32, f32)>,
    pub cards: Vec<Card>,
    pub rows: Vec<Row>,
    pub hops: Vec<Hop>,
    pub keys: Vec<Key>,
    /// Ids of the fields on each lineage, and whether it reaches the UI after the change.
    pub strands: Vec<(Vec<String>, bool)>,
    pub pulses: Vec<(Pulse, Pulse)>,
    pub captions: Vec<Caption>,
    /// What the change does to the data, worked out from the sample.
    pub says: String,
    /// When the model changes, glides, the strands draw, the trips run, and after.
    pub beats: [f32; 5],
    pub duration: f32,
}

fn stage_index(s: Stage) -> usize {
    Stage::ALL.iter().position(|x| *x == s).unwrap_or(0)
}

/// The prefix of the field ids at stage `s`.
fn stage_id(s: Stage) -> &'static str {
    match s {
        Stage::Db => "db",
        Stage::Entity => "entity",
        Stage::Dto => "dto",
        Stage::Ui => "ui",
    }
}

fn stage_title(s: Stage) -> &'static str {
    match s {
        Stage::Db => "Database",
        Stage::Entity => "Entity",
        Stage::Dto => "DTO",
        Stage::Ui => "UI",
    }
}

/// `Accounts` from `db:Accounts.Total`.
fn owner(f: &Field) -> String {
    let rest = f.id.split_once(':').map_or(f.id.as_str(), |x| x.1);
    rest.split_once('.').map_or(rest, |x| x.0).to_owned()
}

/// What a row shows on its right: a foreign key the table it points to, any other field its
/// type.
fn right_text(f: &Field, s: &FieldState) -> String {
    match f.references.as_deref().and_then(|r| r.split_once(':')) {
        Some((_, rest)) => format!("→ {}", rest.split_once('.').map_or(rest, |x| x.0)),
        None => type_text(s),
    }
}

/// A foreign key whose bow runs down its own column: both ends are in one stage.
fn bowed(s: &States, f: &Field) -> bool {
    f.references
        .as_deref()
        .and_then(|r| s.field(r))
        .is_some_and(|t| t.stage == f.stage && f.after.is_some() && t.after.is_some())
}

fn type_text(s: &FieldState) -> String {
    if s.nullable && !s.ty.ends_with('?') {
        format!("{}?", s.ty)
    } else {
        s.ty.clone()
    }
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

/// The lineage hops between neighbouring columns that exist after the change.
fn neighbour_hops(s: &States) -> Vec<(&Field, &Field)> {
    let mut out: Vec<(&Field, &Field)> = Vec::new();
    for l in &s.lineage {
        let hops: Vec<&str> = l.hops().collect();
        for w in hops.windows(2) {
            let (Some(a), Some(b)) = (s.field(w[0]), s.field(w[1])) else {
                continue;
            };
            let next = stage_index(b.stage) == stage_index(a.stage) + 1;
            if next && a.after.is_some() && b.after.is_some() && !out.iter().any(|(x, y)| x.id == a.id && y.id == b.id) {
                out.push((a, b));
            }
        }
    }
    out
}

/// How many pairs of strands between the same two columns cross, given each row's rect.
pub(crate) fn crossings(hops: &[(&str, &str)], rows: &HashMap<String, [f32; 4]>) -> usize {
    let y = |id: &str| rows.get(id).map(|r| r[1] + r[3] / 2.0);
    let ends: Vec<(f32, f32, f32, f32)> = hops
        .iter()
        .filter_map(|(a, b)| {
            let (ra, rb) = (rows.get(*a)?, rows.get(*b)?);
            Some((ra[0], rb[0], y(a)?, y(b)?))
        })
        .collect();
    let mut n = 0;
    for (i, p) in ends.iter().enumerate() {
        for q in &ends[i + 1..] {
            let same_gap = (p.0 - q.0).abs() < 0.5 && (p.1 - q.1).abs() < 0.5;
            if same_gap && (p.2 - q.2) * (p.3 - q.3) < 0.0 {
                n += 1;
            }
        }
    }
    n
}

/// One state laid out: each card's rect, by `column:Owner`, and each row's, by field id.
type Placed = (HashMap<String, [f32; 4]>, HashMap<String, [f32; 4]>);

/// A card: its column's stage and its owner.
type CardKey = (Stage, String);

/// The card order with the fewest strand crossings: barycenter sweeps against the
/// neighbouring column, left to right then back, twice; a card goes where the rows its strands
/// come from sit, and ties keep the listed order.
fn untangled(s: &States, mut order: Vec<CardKey>, n_cols: usize, layout: &dyn Fn(&[CardKey]) -> Placed) -> Vec<CardKey> {
    let hops = neighbour_hops(s);
    let ids: Vec<(&str, &str)> = hops.iter().map(|(a, b)| (a.id.as_str(), b.id.as_str())).collect();
    let mut best = (crossings(&ids, &layout(&order).1), order.clone());
    for sweep in 0..4 {
        let right = sweep % 2 == 0;
        let cols: Vec<usize> = if right {
            (1..n_cols).collect()
        } else {
            (0..n_cols - 1).rev().collect()
        };
        for c in cols {
            let (cards, rows) = layout(&order);
            let mut here: Vec<(f32, (Stage, String))> = Vec::new();
            for k in order.iter().filter(|k| stage_index(k.0) == c) {
                let key = format!("{c}:{}", k.1);
                let top = cards.get(&key).map_or(f32::MAX, |r| r[1]);
                let mut wants = Vec::new();
                for (a, b) in &hops {
                    let (mine, other) = if right { (b, a) } else { (a, b) };
                    if stage_index(mine.stage) != c || owner(mine) != k.1 {
                        continue;
                    }
                    if let (Some(m), Some(o)) = (rows.get(&mine.id), rows.get(&other.id)) {
                        wants.push(o[1] - (m[1] - top));
                    }
                }
                let bary = if wants.is_empty() {
                    top
                } else {
                    wants.iter().sum::<f32>() / wants.len() as f32
                };
                here.push((bary, k.clone()));
            }
            here.sort_by(|x, y| x.0.total_cmp(&y.0));
            let mut next = here.into_iter().map(|x| x.1);
            for slot in order.iter_mut().filter(|k| stage_index(k.0) == c) {
                if let Some(k) = next.next() {
                    *slot = k;
                }
            }
            let n = crossings(&ids, &layout(&order).1);
            if n < best.0 {
                best = (n, order.clone());
            }
        }
    }
    best.1
}

pub(crate) fn flow(s: &States, size: [f32; 2]) -> Flow {
    flow_ordered(s, size, true)
}

/// [`flow`], its cards in listed order unless `untangle`.
pub(crate) fn flow_ordered(s: &States, size: [f32; 2], untangle: bool) -> Flow {
    // Cards: one per (stage, owner), in the order their fields are listed.
    let mut card_keys: Vec<(Stage, String)> = Vec::new();
    for f in &s.fields {
        let k = (f.stage, owner(f));
        if !card_keys.contains(&k) {
            card_keys.push(k);
        }
    }
    // Each column as wide as its widest name and type, the gaps sharing what is left.
    let n_cols = Stage::ALL.len();
    // A foreign key down the first column bows into a gutter left of it.
    let gutter = if s.fields.iter().any(|f| f.stage == Stage::ALL[0] && bowed(s, f)) {
        GUTTER
    } else {
        0.0
    };
    let room = size[0] - 2.0 * MARGIN - gutter;
    let mut widths: Vec<f32> = Stage::ALL
        .iter()
        .map(|st| {
            let here = || s.fields.iter().filter(move |f| f.stage == *st);
            here()
                .flat_map(|f| [f.before.as_ref(), f.after.as_ref()].into_iter().flatten().map(move |x| (f, x)))
                .map(|(f, x)| {
                    let tag = if f.primary_key || f.references.is_some() { TAG_ROOM } else { 0.0 };
                    bend::text_w(&x.name, 6.2) + bend::text_w(&right_text(f, x), 6.7) + 34.0 + tag
                })
                .chain(here().map(|f| bend::text_w(&owner(f), 7.0) + 24.0))
                .fold(110.0f32, f32::max)
                .min(240.0)
        })
        .collect();
    let min_gap = 44.0;
    let used: f32 = widths.iter().sum();
    if used + min_gap * (n_cols - 1) as f32 > room {
        let k = (room - min_gap * (n_cols - 1) as f32) / used;
        widths.iter_mut().for_each(|w| *w *= k);
    }
    let used: f32 = widths.iter().sum();
    let gap = ((room - used) / (n_cols - 1) as f32).clamp(min_gap, 120.0);
    let mut xs = Vec::new();
    let mut x = MARGIN + gutter + ((room - used - gap * (n_cols - 1) as f32) / 2.0).max(0.0);
    for w in &widths {
        xs.push(x);
        x += w + gap;
    }
    let col_x = |c: usize| xs[c];
    let col_w = |c: usize| widths[c];
    let columns = Stage::ALL
        .iter()
        .enumerate()
        .map(|(i, s)| (stage_title(*s).to_owned(), col_x(i), col_w(i)))
        .collect();

    let note_of = |stage: Stage, own: &str| s.note(&format!("{}:{own}", stage_id(stage)));
    let note_h = |stage: Stage, own: &str| {
        note_of(stage, own).map_or(0.0, |n| {
            let room = (col_w(stage_index(stage)) - 20.0).max(40.0);
            (bend::text_w(n, NOTE_CH) / room).ceil() * NOTE_LH + 4.0
        })
    };

    // Lays out one state: cards stacked from the top of each column in `order`, rows in list
    // order.
    let layout = |after: bool, order: &[(Stage, String)]| -> Placed {
        let (mut cards, mut rows) = (HashMap::new(), HashMap::new());
        let mut y = [MARGIN + SAYS + HEAD; 4];
        for (stage, own) in order {
            let c = stage_index(*stage);
            let fields: Vec<&Field> = s
                .fields
                .iter()
                .filter(|f| f.stage == *stage && owner(f) == *own && if after { f.after.is_some() } else { f.before.is_some() })
                .collect();
            if fields.is_empty() {
                continue;
            }
            let (x, cw) = (col_x(c), col_w(c));
            let top = CARD_HEAD + note_h(*stage, own);
            let h = top + fields.len() as f32 * ROW_H + 6.0;
            cards.insert(format!("{}:{own}", c), [x, y[c], cw, h]);
            for (i, f) in fields.iter().enumerate() {
                rows.insert(f.id.clone(), [x, y[c] + top + i as f32 * ROW_H, cw, ROW_H]);
            }
            y[c] += h + CARD_GAP;
        }
        (cards, rows)
    };
    let card_keys = if untangle {
        untangled(s, card_keys, n_cols, &|order| layout(true, order))
    } else {
        card_keys
    };
    let (was_cards, was_rows) = layout(false, &card_keys);
    let (now_cards, now_rows) = layout(true, &card_keys);

    let removed_rows = s.fields.iter().filter(|f| f.status() == Status::Removed).count();
    let retyped = s.fields.iter().filter(|f| f.status() == Status::Changed).count();
    let go = HOLD;
    let changes = removed_rows + retyped > 0;
    let room_at = if changes { go + TINT + UNDRAW + 0.2 } else { go };
    let new_at = room_at + GLIDE + 0.2;

    // Lineage hops, each once, and when the strands draw.
    let field = |id: &str| s.field(id);
    let mut hop_keys: Vec<(String, String)> = Vec::new();
    let mut strands = Vec::new();
    for l in &s.lineage {
        let hops: Vec<&str> = l.hops().collect();
        for w in hops.windows(2) {
            let k = (w[0].to_owned(), w[1].to_owned());
            if !hop_keys.contains(&k) {
                hop_keys.push(k);
            }
        }
        let reaches = hops
            .last()
            .and_then(|id| field(id))
            .is_some_and(|f| f.stage == Stage::Ui && f.after.is_some())
            && hops.iter().all(|id| field(id).is_some_and(|f| f.after.is_some()));
        strands.push((hops.iter().map(|h| (*h).to_owned()).collect::<Vec<_>>(), reaches));
    }
    let exists = |a: &str, b: &str, after: bool| {
        let has = |id: &str| field(id).is_some_and(|f| if after { f.after.is_some() } else { f.before.is_some() });
        has(a) && has(b)
    };

    // New cards appear when the first strand reaches them, else with the database's new tables.
    let mut card_at: HashMap<String, f32> = HashMap::new();
    let card_of = |f: &Field| format!("{}:{}", stage_index(f.stage), owner(f));
    for f in s.fields.iter().filter(|f| f.stage == Stage::Db && f.before.is_none()) {
        card_at.entry(card_of(f)).or_insert(new_at);
    }
    let keys_at = new_at + ENTER + 0.1;
    let has_keys = s.fields.iter().any(|f| bowed(s, f));
    let mut cursor = if has_keys { keys_at + HOP + 0.2 } else { new_at + ENTER + 0.1 };
    let mut hop_at: HashMap<(String, String), f32> = HashMap::new();
    for (ids, _) in &strands {
        let mut drew = false;
        for w in ids.windows(2) {
            let k = (w[0].clone(), w[1].clone());
            if hop_at.contains_key(&k) || !exists(&w[0], &w[1], true) || exists(&w[0], &w[1], false) {
                continue;
            }
            hop_at.insert(k, cursor);
            if let Some(t) = field(&w[1]) {
                card_at.entry(card_of(t)).or_insert(cursor + 0.1);
            }
            cursor += HOP;
            drew = true;
        }
        if drew {
            cursor += 0.25;
        }
    }
    for f in s.fields.iter().filter(|f| f.before.is_none()) {
        card_at.entry(card_of(f)).or_insert(new_at);
    }
    let trip_at = cursor + 0.2;

    let mut cards = Vec::new();
    for (stage, own) in &card_keys {
        let key = format!("{}:{own}", stage_index(*stage));
        let (a, b) = (was_cards.get(&key).copied(), now_cards.get(&key).copied());
        let rest = b.or(a).unwrap_or([0.0; 4]);
        let still = |v: f32| Track::new(vec![(0.0, v)]);
        let (status, rect, alpha, head) = match (a, b) {
            (Some(a), Some(b)) => (
                Status::Same,
                Track::new(vec![(room_at, a), (room_at + GLIDE, b)]),
                still(1.0),
                Track::new(vec![(room_at, sub([a[0], a[1]], [b[0], b[1]])), (room_at + GLIDE, [0.0; 2])]),
            ),
            (None, Some(b)) => {
                let t = card_at.get(&key).copied().unwrap_or(new_at);
                (
                    Status::Added,
                    Track::new(vec![(0.0, b)]),
                    Track::new(vec![(t, 0.0), (t + ENTER, 1.0)]),
                    still2(),
                )
            }
            (Some(a), None) => (
                Status::Removed,
                Track::new(vec![(0.0, a)]),
                Track::new(vec![(room_at, 1.0), (room_at + GLIDE * 0.6, 0.0)]),
                still2(),
            ),
            (None, None) => continue,
        };
        cards.push(Card {
            key,
            title: own.clone(),
            note: note_of(*stage, own).map(str::to_owned),
            status,
            rest,
            rect,
            alpha,
            head,
        });
    }

    let dead: Vec<&str> = s
        .lineage
        .iter()
        .filter(|l| {
            let last = field(&l.to);
            l.hops().any(|h| field(h).is_some_and(|f| f.stage == Stage::Dto)) && last.is_some_and(|f| f.stage != Stage::Ui)
        })
        .map(|l| l.to.as_str())
        .collect();
    let mut rows = Vec::new();
    for f in &s.fields {
        let (a, b) = (was_rows.get(&f.id).copied(), now_rows.get(&f.id).copied());
        let rest = b.or(a).unwrap_or([0.0; 4]);
        let st = f.status();
        let still = |v: f32| Track::new(vec![(0.0, v)]);
        let mut row = Row {
            id: f.id.clone(),
            name: f.after.as_ref().or(f.before.as_ref()).map_or(f.id.clone(), |x| x.name.clone()),
            why: f.why.clone(),
            status: st,
            was_type: f.before.as_ref().map(type_text),
            now_type: f.after.as_ref().map(type_text),
            rest,
            offset: Track::new(vec![(0.0, [0.0; 2])]),
            alpha: still(1.0),
            tint: still(0.0),
            retype: None,
            dead_end: dead.contains(&f.id.as_str()),
            key: match &f.references {
                Some(to) => Some(KeyMark::Foreign(to.clone(), short(s, to))),
                None => f.primary_key.then_some(KeyMark::Primary),
            },
        };
        match (a, b) {
            (Some(a), Some(b)) => {
                row.offset = Track::new(vec![(room_at, sub([a[0], a[1]], [b[0], b[1]])), (room_at + GLIDE, [0.0; 2])]);
                if st == Status::Changed {
                    row.tint = Track::new(vec![(go, 0.0), (go + TINT, 1.0)]);
                    if row.was_type != row.now_type {
                        let (t0, t1) = (go + 0.2, go + TINT + UNDRAW);
                        row.retype = Some((Track::new(vec![(t0, 1.0), (t1, 0.0)]), Track::new(vec![(t0, 0.0), (t1, 1.0)])));
                    }
                }
            }
            (Some(_), None) => {
                row.tint = Track::new(vec![(go, 0.0), (go + TINT, 1.0)]);
                row.alpha = Track::new(vec![(room_at, 1.0), (room_at + GLIDE * 0.6, 0.0)]);
            }
            (None, Some(_)) => {
                let card = card_of(f);
                let i = s
                    .fields
                    .iter()
                    .filter(|g| card_of(g) == card && g.after.is_some())
                    .position(|g| g.id == f.id)
                    .unwrap_or(0);
                let t = card_at.get(&card).copied().unwrap_or(new_at) + 0.15 + i as f32 * 0.06;
                row.alpha = Track::new(vec![(t, 0.0), (t + ENTER, 1.0)]);
            }
            (None, None) => continue,
        }
        rows.push(row);
    }

    let side = |rect: [f32; 4], right: bool| [if right { rect[0] + rect[2] } else { rect[0] }, rect[1] + rect[3] / 2.0];
    let span_of = |a: [f32; 4], b: [f32; 4]| bend::across(side(a, a[0] < b[0]), side(b, b[0] < a[0]));
    let mut hops = Vec::new();
    for (from, to) in &hop_keys {
        let (Some(fa), Some(fb)) = (field(from), field(to)) else { continue };
        if fa.stage == fb.stage {
            continue;
        }
        let s0 = was_rows.get(from).zip(was_rows.get(to)).map(|(a, b)| span_of(*a, *b));
        let s1 = now_rows.get(from).zip(now_rows.get(to)).map(|(a, b)| span_of(*a, *b));
        let key = format!("lineage-hop-{from}-{to}");
        let mk = |sp: Span, status| Hop {
            key: key.clone(),
            from: from.clone(),
            to: to.clone(),
            status,
            unit: sp.unit,
            rest: sp.rect,
            rect: Track::new(vec![(0.0, sp.rect)]),
            tint: Track::new(vec![(0.0, 0.0)]),
            trim: None,
        };
        let hop = match (s0, s1) {
            (Some(a), Some(b)) => {
                let a = a.matching(&b);
                let mut h = mk(b, Status::Same);
                if a.unit == b.unit {
                    h.rect = Track::new(vec![(room_at, a.rect), (room_at + GLIDE, b.rect)]);
                }
                h
            }
            (Some(a), None) => {
                let mut h = mk(a, Status::Removed);
                h.tint = Track::new(vec![(go, 0.0), (go + TINT, 1.0)]);
                h.trim = Some(WriteOn::undraw(go + TINT, UNDRAW));
                h
            }
            (None, Some(b)) => {
                let t = hop_at.get(&(from.clone(), to.clone())).copied().unwrap_or(new_at);
                let mut h = mk(b, Status::Added);
                h.trim = Some(WriteOn::draw(t, HOP));
                h
            }
            (None, None) => continue,
        };
        hops.push(hop);
    }

    let keys: Vec<Key> = s
        .fields
        .iter()
        .filter(|f| bowed(s, f))
        .filter_map(|f| {
            let to = f.references.as_ref()?;
            let (a, b) = (now_rows.get(&f.id)?, now_rows.get(to)?);
            let (pa, pb) = ([a[0], a[1] + a[3] / 2.0], [b[0], b[1] + b[3] / 2.0]);
            Some(Key {
                from: f.id.clone(),
                to: to.clone(),
                status: if f.before.is_none() { Status::Added } else { Status::Same },
                span: bend::bow(pa, pb, false, (22.0 + (pa[1] - pb[1]).abs() * 0.08).min(BOW_MAX)),
                trim: if f.before.is_none() {
                    WriteOn::draw(keys_at, HOP)
                } else {
                    WriteOn::draw(0.0, 0.0)
                },
                head: if f.before.is_none() {
                    Track::new(vec![(keys_at + HOP - 0.1, 0.0), (keys_at + HOP, 1.0)])
                } else {
                    Track::new(vec![(0.0, 1.0)])
                },
            })
        })
        .collect();

    // Trips: a dot runs every new strand that now reaches the UI.
    let mut pulses = Vec::new();
    for (ids, reaches) in &strands {
        let new = ids.windows(2).any(|w| !exists(&w[0], &w[1], false));
        if !*reaches || !new {
            continue;
        }
        let mut pts = Vec::new();
        for w in ids.windows(2) {
            if let Some(h) = hops.iter().find(|h| h.from == w[0] && h.to == w[1]) {
                pts.extend(
                    Span {
                        rect: h.rest,
                        unit: h.unit,
                    }
                    .points(20),
                );
            }
        }
        if pts.len() >= 2 {
            let t = trip_at + 0.3 + pulses.len() as f32 * 0.4;
            pulses.push((Pulse::new(pts.clone(), t, TRIP, false, 10.0), Pulse::new(pts, t, TRIP, false, 22.0)));
        }
    }
    let after_at = if pulses.is_empty() {
        trip_at
    } else {
        trip_at + 0.3 + TRIP + 0.4 * (pulses.len() - 1) as f32 + 0.3
    };
    let duration = after_at + 0.8;

    let name_of = |id: &str| {
        field(id).map_or(id.to_owned(), |f| {
            format!(
                "{}.{}",
                owner(f),
                f.after.as_ref().or(f.before.as_ref()).map_or("", |x| x.name.as_str())
            )
        })
    };
    let mut beats = vec![(
        0.0,
        "Before".to_owned(),
        format!(
            "{} across {}; strands follow each value from its column to the screen.",
            plural(s.fields.iter().filter(|f| f.before.is_some()).count(), "field", "fields"),
            plural(was_cards.len(), "card", "cards")
        ),
    )];
    if changes {
        let mut t = Vec::new();
        let gone: Vec<String> = s
            .fields
            .iter()
            .filter(|f| f.status() == Status::Removed)
            .map(|f| name_of(&f.id))
            .collect();
        if !gone.is_empty() {
            t.push(format!("{} dropped", gone.join(", ")));
        }
        if retyped > 0 {
            t.push(format!("{} retyped (amber)", plural(retyped, "field", "fields")));
        }
        beats.push((go, "The model changes".to_owned(), format!("{}.", t.join("; "))));
    }
    beats.push((
        room_at,
        "Kept cards make room".to_owned(),
        "What stays glides to its place in the new model.".to_owned(),
    ));
    let new_tables: Vec<String> = cards
        .iter()
        .filter(|c| c.status == Status::Added && c.key.starts_with("0:"))
        .map(|c| c.title.clone())
        .collect();
    let mut drawn = format!(
        "{} draw left to right",
        plural(
            hops.iter().filter(|h| h.status == Status::Added).count(),
            "new strand",
            "new strands"
        )
    );
    if !new_tables.is_empty() {
        drawn = format!("New table {}; {}", new_tables.join(", "), drawn);
    }
    beats.push((
        new_at,
        "New values flow".to_owned(),
        format!("{drawn}, cards appearing as they are reached."),
    ));
    if !pulses.is_empty() {
        let reach: Vec<String> = strands
            .iter()
            .filter(|(ids, r)| *r && ids.windows(2).any(|w| !exists(&w[0], &w[1], false)))
            .map(|(ids, _)| format!("{} → {}", name_of(&ids[0]), name_of(ids.last().map_or("", |x| x.as_str()))))
            .collect();
        let mut t = format!("End to end: {}", reach.join(", "));
        if !dead.is_empty() {
            t.push_str(&format!(
                "; the old dead end at {} is gone",
                dead.iter().map(|d| name_of(d)).collect::<Vec<_>>().join(", ")
            ));
        }
        beats.push((trip_at, "Does it reach the screen?".to_owned(), format!("{t}.")));
    }
    beats.push((
        after_at,
        "After".to_owned(),
        format!(
            "{} across {}. Hover a field to follow its strands.",
            plural(s.fields.iter().filter(|f| f.after.is_some()).count(), "field", "fields"),
            plural(now_cards.len(), "card", "cards")
        ),
    ));
    let says = conclusion(s, &strands, &dead, &new_tables);
    Flow {
        columns,
        cards,
        rows,
        hops,
        keys,
        strands,
        pulses,
        captions: beats::captions(beats),
        says,
        beats: [go, room_at, new_at, trip_at, after_at],
        duration,
    }
}

/// `Accounts.Total` for `db:Accounts.Total`.
fn short(s: &States, id: &str) -> String {
    s.field(id).map_or(id.to_owned(), |f| {
        let name = f.after.as_ref().or(f.before.as_ref()).map_or("", |x| x.name.as_str());
        format!("{}.{name}", owner(f))
    })
}

/// What a retype does: `becomes required` when only nullability went, else the new type.
fn retype(b: &FieldState, a: &FieldState) -> String {
    let null = |x: &FieldState| x.nullable || x.ty.ends_with('?');
    let base = |x: &FieldState| x.ty.trim_end_matches('?').to_owned();
    match (base(b) == base(a), null(b), null(a)) {
        (true, true, false) => "becomes required".to_owned(),
        (true, false, true) => "becomes optional".to_owned(),
        _ => format!("becomes {}", type_text(a)),
    }
}

/// One or two sentences on what the change does to the data: what newly reaches a screen,
/// what is retyped, and what is dropped, each strand named by the field it starts from.
fn conclusion(s: &States, strands: &[(Vec<String>, bool)], dead: &[&str], new_tables: &[String]) -> String {
    let was = |id: &str| s.field(id).is_some_and(|f| f.before.is_some());
    let mut reach: Vec<String> = Vec::new();
    for (ids, reaches) in strands {
        let (Some(first), Some(last)) = (ids.first(), ids.last()) else {
            continue;
        };
        if *reaches && !ids.iter().all(|i| was(i)) {
            reach.push(format!("{} now reaches the screen in {}", short(s, first), short(s, last)));
        }
    }
    let lead = match (new_tables.is_empty(), reach.is_empty()) {
        (true, true) => "No new value reaches a screen.".to_owned(),
        (true, false) => format!("{}.", reach.join("; ")),
        (false, true) => format!("New table {}, not yet on any screen.", new_tables.join(", ")),
        (false, false) => format!("New table {}: {}.", new_tables.join(", "), reach.join("; ")),
    };
    let mut also: Vec<String> = Vec::new();
    let mut told: Vec<&str> = Vec::new();
    for (ids, _) in strands {
        let fields: Vec<&Field> = ids.iter().filter_map(|i| s.field(i)).collect();
        if !fields.is_empty() && fields.iter().all(|f| f.status() == Status::Removed) {
            let end = if dead.contains(&ids.last().map_or("", |x| x.as_str())) {
                " (it never reached a screen)"
            } else {
                ""
            };
            also.push(format!("{} is dropped end to end{end}", short(s, &fields[0].id)));
            told.extend(fields.iter().map(|f| f.id.as_str()));
        } else if let Some(f) = fields
            .iter()
            .find(|f| f.status() == Status::Changed && !told.contains(&f.id.as_str()))
        {
            if let (Some(b), Some(a)) = (&f.before, &f.after) {
                also.push(format!("{} {}", short(s, &f.id), retype(b, a)));
            }
            told.extend(fields.iter().filter(|f| f.status() == Status::Changed).map(|f| f.id.as_str()));
        }
    }
    for f in s
        .fields
        .iter()
        .filter(|f| f.status() == Status::Removed && !told.contains(&f.id.as_str()))
    {
        also.push(format!("{} is dropped", short(s, &f.id)));
    }
    if also.is_empty() {
        return lead;
    }
    let mut rest = also.join("; ");
    if let Some(c) = rest.get(..1) {
        rest = c.to_uppercase() + &rest[1..];
    }
    format!("{lead} {rest}.")
}

fn still2() -> Track<[f32; 2]> {
    Track::new(vec![(0.0, [0.0; 2])])
}

impl Style for Lineage {
    fn id(&self) -> &'static str {
        "lineage"
    }
    fn name(&self) -> &'static str {
        "Field lineage and ER"
    }
    fn pitch(&self) -> &'static str {
        "For schema changes: each field's way from table to screen, and why each table is there."
    }
    fn schemas(&self) -> &'static [SchemaId] {
        &[SchemaId::B]
    }
    fn how_to_read(&self) -> Option<&'static str> {
        Some(
            "The sentence on top is the takeaway. Below it, four columns of cards: database \
             tables, entities, DTOs and UI, each with a line on why it is there and a row per \
             field with its type. A strand is one value moving to the next stage. The model changes \
             first, like an ER diff: dropped fields turn red and collapse, a retyped field swaps \
             its type in amber, kept cards glide. Then new values draw their strands left to \
             right (green), cards appearing as they are reached, and a dot runs each strand \
             that now reaches the screen. A field marked dead end reached the API but no \
             screen. PK marks a primary key; FK a foreign key, with the table it points to on \
             its right, and in the database column an arrowed bow runs from each foreign key \
             to the key it references. Hover a field to follow its strands and keys.",
        )
    }

    fn rig(&self, cx: &Cx) -> Rig {
        let m = flow(&cx.samples.b, cx.size);
        let [go, room, new_at, trip, after] = m.beats;
        let mut focus: Vec<(f32, Vec<String>)> = vec![(0.0, Vec::new())];
        if room > go {
            let mut ids: Vec<String> = m
                .rows
                .iter()
                .filter(|r| matches!(r.status, Status::Removed | Status::Changed))
                .map(|r| format!("f:{}", r.id))
                .collect();
            ids.extend(m.hops.iter().filter(|h| h.status == Status::Removed).map(|h| h.key.clone()));
            focus.push((go, ids));
        }
        focus.push((room, Vec::new()));
        if !m.pulses.is_empty() {
            let mut ids = Vec::new();
            for (strand, reaches) in &m.strands {
                if *reaches {
                    ids.extend(strand.iter().map(|f| format!("f:{f}")));
                    for w in strand.windows(2) {
                        ids.extend(m.hops.iter().filter(|h| h.from == w[0] && h.to == w[1]).map(|h| h.key.clone()));
                    }
                }
            }
            focus.push((trip, ids));
        }
        focus.push((after, Vec::new()));
        let mut tracks = Tracks::default();
        for c in &m.cards {
            tracks.push(c.rect.clone());
            tracks.push(c.alpha.clone());
            tracks.push(c.head.clone());
        }
        for r in &m.rows {
            tracks.push(r.offset.clone());
            tracks.push(r.alpha.clone());
            tracks.push(r.tint.clone());
            if let Some((a, b)) = &r.retype {
                tracks.push(a.clone());
                tracks.push(b.clone());
            }
        }
        for h in &m.hops {
            tracks.push(h.rect.clone());
            tracks.push(h.tint.clone());
            if let Some(w) = &h.trim {
                tracks.push(w.clone());
            }
        }
        for k in &m.keys {
            tracks.push(k.trim.clone());
            tracks.push(k.head.clone());
        }
        for (a, b) in &m.pulses {
            tracks.push(a.clone());
            tracks.push(b.clone());
        }
        for c in &m.captions {
            tracks.push(c.alpha.clone());
        }
        let (duration, changes, trips) = (m.duration, room > go, !m.pulses.is_empty());
        let mut rig = Rig::new(m, cx.size, duration).step(0.0, "Before");
        if changes {
            rig = rig.step(go, "The model changes");
        }
        rig = rig.step(room, "Kept cards make room").step(new_at, "New values flow");
        if trips {
            rig = rig.step(trip, "Does it reach the screen?");
        }
        rig = rig.step(after, "After");
        rig.add(FocusTrack::new(rig.focus.clone(), focus));
        tracks.onto(&mut rig);
        rig
    }

    fn view(&self, cx: &Cx, rig: &Rig) -> El {
        let Some(m) = rig.data::<Flow>() else { return el() };
        let ink = cx.ink;
        let [w, h] = cx.size;
        let mut stage = el()
            .key("lineage")
            .size(w, h)
            .role("img")
            .label(format!("Field lineage across {} cards", m.cards.len()))
            .child(beats::conclusion(cx, "lineage", &m.says, w - 2.0 * MARGIN).absolute(MARGIN, MARGIN - 8.0));
        for (i, (title, x, cw)) in m.columns.iter().enumerate() {
            let colour = ink.keys[i % ink.keys.len()];
            stage = stage.child(
                row()
                    .absolute(*x, MARGIN + SAYS)
                    .size(*cw, 16.0)
                    .gap(6.0)
                    .items_center()
                    .child(el().size(8.0, 8.0).radius(4.0).fill(colour).a11y_hidden())
                    .child(text(title.clone()).font(MICRO).color(colour).a11y_hidden())
                    .a11y_hidden(),
            );
        }
        for hop in &m.hops {
            stage = stage.child(layered(cx, rig, &hop.key, hop_el(cx, hop)));
        }
        for c in &m.cards {
            stage = stage.child(card_el(cx, c));
        }
        for k in &m.keys {
            let r = k.span.rect;
            let colour = if k.status == Status::Added { ink.added } else { ink.edge };
            let end = Span::at(r, k.span.unit[3]);
            let head = Shape::new().fill(vec![[8.0, 4.5], [0.0, 0.0], [0.0, 9.0]], colour);
            let g = el()
                .absolute(0.0, 0.0)
                .size(w, h)
                .a11y_hidden()
                .child(
                    k.trim.bind(
                        canvas(bend::unit_shape(k.span.unit, 1.5, colour))
                            .key(format!("lineage-fk-{}", k.from))
                            .absolute(r[0], r[1])
                            .size(r[2], r[3])
                            .a11y_hidden(),
                    ),
                )
                .child(
                    canvas(head)
                        .absolute(end[0] - 7.0, end[1] - 4.5)
                        .size(8.0, 9.0)
                        .live_opacity(&k.head.live)
                        .a11y_hidden(),
                );
            stage = stage.child(layered(cx, rig, &format!("fk:{}", k.from), g));
        }
        for r in &m.rows {
            stage = stage.child(layered(cx, rig, &format!("f:{}", r.id), row_el(cx, rig, m, r)));
        }
        for (i, (dot, halo)) in m.pulses.iter().enumerate() {
            stage = stage
                .child(
                    halo.bind(
                        el().key(format!("lineage-halo-{i}"))
                            .radius(11.0)
                            .fill(ink.added.alpha(0.25))
                            .a11y_hidden(),
                    ),
                )
                .child(
                    dot.bind(
                        el().key(format!("lineage-dot-{i}"))
                            .radius(5.0)
                            .fill(ink.added)
                            .border(1.5, ink.background)
                            .a11y_hidden(),
                    ),
                );
        }
        // The caption sits under the tallest column, before or after the change, not at the
        // canvas's foot.
        let bottom = m
            .cards
            .iter()
            .map(|c| (c.rest[1] + c.rest[3]).max(c.rect.at(0.0)[1] + c.rect.at(0.0)[3]))
            .fold(0.0, f32::max);
        let cap_y = (bottom + 28.0).min(h - FOOT + 18.0);
        for c in beats::caption_els(cx, "lineage", &m.captions, [MARGIN, cap_y], w - 2.0 * MARGIN - 280.0) {
            stage = stage.child(c);
        }
        stage.child(
            col()
                .absolute(w - MARGIN - 260.0, cap_y + 4.0)
                .size(260.0, 50.0)
                .gap(8.0)
                .items_end()
                .child(
                    row()
                        .gap(12.0)
                        .child(paint::swatch("new", ink.added_fill, ink.added_border, ink))
                        .child(paint::swatch("retyped", ink.modified_fill, ink.modified_border, ink))
                        .child(paint::swatch("dropped", ink.removed_fill, ink.removed_border, ink)),
                )
                .a11y_hidden(),
        )
    }
}

fn hop_el(cx: &Cx, h: &Hop) -> El {
    let ink = cx.ink;
    let soft = |c: rune::Color| c.mix(ink.background, 0.35);
    let base = if h.status == Status::Added {
        soft(ink.added)
    } else {
        soft(ink.edge)
    };
    let r = h.rest;
    let ribbon = |c, key: String| {
        let mut e = canvas(bend::unit_shape(h.unit, RIBBON, c))
            .key(key)
            .absolute(r[0], r[1])
            .size(r[2], r[3])
            .live_rect(&h.rect.live)
            .a11y_hidden();
        if let Some(t) = &h.trim {
            e = t.bind(e);
        }
        e
    };
    let mut g = el()
        .absolute(0.0, 0.0)
        .size(cx.size[0], cx.size[1])
        .a11y_hidden()
        .child(ribbon(base, h.key.clone()));
    if h.status == Status::Removed {
        g = g.child(ribbon(soft(ink.removed), format!("{}-wash", h.key)).live_opacity(&h.tint.live));
    }
    g
}

fn card_el(cx: &Cx, c: &Card) -> El {
    let ink = cx.ink;
    let tone = paint::tone(ink, c.status);
    let r = c.rest;
    let border = if c.status == Status::Added { tone.border } else { ink.card_border };
    let note = c.note.as_ref().map(|n| {
        text(n.clone())
            .font(SMALL)
            .color(ink.muted)
            .wrap()
            .absolute(r[0] + 10.0, r[1] + CARD_HEAD - 3.0)
            .w(r[2] - 20.0)
            .live_offset(&c.head.live)
            .a11y_hidden()
    });
    el().absolute(0.0, 0.0)
        .size(cx.size[0], cx.size[1])
        .live_opacity(&c.alpha.live)
        .a11y_hidden()
        .child(
            el().key(format!("lineage-card-{}", c.key))
                .absolute(r[0], r[1])
                .size(r[2], r[3])
                .radius(8.0)
                .fill(ink.card)
                .border(1.0, border)
                .live_rect(&c.rect.live)
                .a11y_hidden(),
        )
        .child(
            row()
                .absolute(r[0] + 10.0, r[1])
                .size(r[2] - 20.0, CARD_HEAD)
                .items_center()
                .child(
                    text(c.title.clone())
                        .font(STRONG)
                        .color(if c.status == Status::Added { tone.text } else { ink.foreground })
                        .truncate()
                        .shrink()
                        .a11y_hidden(),
                )
                .live_offset(&c.head.live)
                .a11y_hidden(),
        )
        .children(note)
}

fn row_el(cx: &Cx, rig: &Rig, m: &Flow, r: &Row) -> El {
    let ink = cx.ink;
    let tone = paint::tone(ink, r.status);
    let [x, y, w, h] = r.rest;
    let name_colour = if r.status == Status::Added { tone.text } else { ink.foreground };
    let ty = |t: &str, colour| {
        row()
            .absolute(0.0, 0.0)
            .size(w - 10.0, h)
            .justify_end()
            .items_center()
            .child(text(t.to_owned()).font(MONO).color(colour).truncate().shrink().a11y_hidden())
            .a11y_hidden()
    };
    let mut e = el()
        .key(format!("lineage-row-{}", r.id))
        .absolute(x, y)
        .size(w, h)
        .role("img")
        .label(format!(
            "{}: {}",
            r.name,
            r.now_type.as_deref().or(r.was_type.as_deref()).unwrap_or("")
        ))
        .help(r.why.as_ref().map_or_else(|| r.id.clone(), |w| format!("{}: {w}", r.name)))
        .child(
            el().absolute(4.0, 1.0)
                .size(w - 8.0, h - 2.0)
                .radius(4.0)
                .fill(tone.fill)
                .border(1.0, tone.border)
                .live_opacity(&r.tint.live)
                .a11y_hidden(),
        );
    if r.status == Status::Added {
        e = e.child(
            el().absolute(4.0, 1.0)
                .size(w - 8.0, h - 2.0)
                .radius(4.0)
                .fill(tone.fill)
                .a11y_hidden(),
        );
    }
    let tag = r.key.as_ref().map(|k| {
        let (word, help) = match k {
            KeyMark::Primary => ("PK", "Primary key".to_owned()),
            KeyMark::Foreign(_, to) => ("FK", format!("Foreign key to {to}")),
        };
        row()
            .size(TAG_W, 14.0)
            .center()
            .radius(3.0)
            .border(1.0, ink.muted.alpha(0.6))
            .child(text(word).font(MICRO).color(ink.muted).a11y_hidden())
            .help(help)
            .a11y_hidden()
    });
    e = e.child(
        row()
            .absolute(10.0, 0.0)
            .size(w - 20.0, h)
            .gap(TAG_ROOM - TAG_W)
            .items_center()
            .children(tag)
            .child(
                text(r.name.clone())
                    .font(SMALL)
                    .color(name_colour)
                    .truncate()
                    .shrink()
                    .a11y_hidden(),
            )
            .a11y_hidden(),
    );
    let muted = ink.muted;
    match (&r.retype, &r.key) {
        (Some((old, new)), _) => {
            e = e
                .child(ty(r.was_type.as_deref().unwrap_or(""), muted).live_opacity(&old.live))
                .child(ty(r.now_type.as_deref().unwrap_or(""), tone.text).live_opacity(&new.live));
        }
        (None, Some(KeyMark::Foreign(_, to))) => {
            let table = to.split_once('.').map_or(to.as_str(), |x| x.0);
            e = e.child(ty(&format!("→ {table}"), muted));
        }
        (None, _) => e = e.child(ty(r.now_type.as_deref().or(r.was_type.as_deref()).unwrap_or(""), muted)),
    }
    if r.status == Status::Removed {
        e = e.child(
            el().absolute(8.0, h / 2.0)
                .size(w - 16.0, 1.2)
                .fill(tone.text)
                .live_opacity(&r.tint.live)
                .a11y_hidden(),
        );
    }
    if r.dead_end {
        e = e.child(
            row()
                .absolute(w + 6.0, 3.0)
                .size(62.0, h - 6.0)
                .center()
                .radius(8.0)
                .border(1.0, ink.removed_border)
                .child(text("dead end").font(MICRO).color(ink.removed_text).a11y_hidden())
                .help("Reaches the API, never a screen")
                .a11y_hidden(),
        );
    }
    let mut lit: Vec<String> = Vec::new();
    for (ids, _) in m.strands.iter().filter(|(ids, _)| ids.contains(&r.id)) {
        lit.extend(ids.iter().map(|f| format!("f:{f}")));
        for w in ids.windows(2) {
            lit.extend(m.hops.iter().filter(|h| h.from == w[0] && h.to == w[1]).map(|h| h.key.clone()));
        }
    }
    for k in m.keys.iter().filter(|k| k.from == r.id || k.to == r.id) {
        lit.extend([format!("fk:{}", k.from), format!("f:{}", k.from), format!("f:{}", k.to)]);
    }
    if let Some(KeyMark::Foreign(to, _)) = &r.key {
        lit.push(format!("f:{to}"));
    }
    if !lit.contains(&format!("f:{}", r.id)) {
        lit.push(format!("f:{}", r.id));
    }
    rig.highlight.hover(e.live_offset(&r.offset.live).live_opacity(&r.alpha.live), lit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::samples;

    fn b() -> Flow {
        flow(&samples().b, [1000.0, 800.0])
    }

    #[test]
    fn every_field_has_a_row_in_its_card_inside_the_canvas() {
        let m = b();
        assert_eq!(m.rows.len(), samples().b.fields.len());
        for r in &m.rows {
            assert!(
                r.rest[0] >= 0.0 && r.rest[0] + r.rest[2] <= 1000.0 && r.rest[1] + r.rest[3] <= 800.0 - FOOT,
                "{}",
                r.id
            );
        }
        let titles: Vec<_> = m.cards.iter().map(|c| c.title.as_str()).collect();
        for t in [
            "Accounts",
            "Vouchers",
            "Account",
            "Voucher",
            "VoucherDto",
            "PaymentVm",
            "VoucherForm",
            "AccountPage",
        ] {
            assert!(titles.contains(&t), "{t}");
        }
        let card = |t: &str| m.cards.iter().find(|c| c.title == t).unwrap();
        assert_eq!(card("Vouchers").status, Status::Added);
        assert_eq!(card("Accounts").status, Status::Same);
        assert!(m.columns.windows(2).all(|w| w[0].1 + w[0].2 + 40.0 < w[1].1));
    }

    #[test]
    fn the_conclusion_and_the_notes_come_from_the_sample() {
        let m = b();
        for part in [
            "New table Vouchers: Vouchers.Amount now reaches the screen in VoucherForm.amount.",
            "Accounts.Total becomes required",
            "Accounts.LegacyTotal is dropped end to end (it never reached a screen)",
        ] {
            assert!(m.says.contains(part), "{part} in {}", m.says);
        }
        for c in &m.cards {
            let note = c.note.as_deref().unwrap_or_else(|| panic!("{} has a note", c.title));
            if c.status == Status::Added {
                assert!(note.starts_with("New"), "{note}");
            }
        }
        for r in m.rows.iter().filter(|r| r.status != Status::Removed) {
            let card = m
                .cards
                .iter()
                .find(|c| r.id.contains(&format!(":{}.", c.title)) && c.rest[0] == r.rest[0])
                .unwrap();
            assert!(
                r.rest[1] >= card.rest[1] + CARD_HEAD + NOTE_LH,
                "{} sits under its card's note",
                r.id
            );
        }
    }

    #[test]
    fn the_model_morphs_from_before_to_after() {
        let m = b();
        let end = m.duration;
        let row = |id: &str| m.rows.iter().find(|r| r.id == id).unwrap();
        let legacy = row("db:Accounts.LegacyTotal");
        assert_eq!((legacy.alpha.at(0.0), legacy.alpha.at(end)), (1.0, 0.0));
        assert!(row("dto:PaymentVm.LegacyTotal").dead_end, "reached the API, never the UI");
        assert!(
            !row("entity:Voucher.AccountId").dead_end,
            "a key that stops at the entity is no dead end"
        );
        let total = row("db:Accounts.Total");
        let (old, new) = total.retype.as_ref().unwrap();
        assert_eq!((old.at(0.0), new.at(0.0), old.at(end), new.at(end)), (1.0, 0.0, 0.0, 1.0));
        assert_eq!(total.now_type.as_deref(), Some("decimal(18,2)"));
        assert_eq!(total.was_type.as_deref(), Some("decimal(18,2)?"));
        let amount = row("ui:VoucherForm.amount");
        assert_eq!((amount.alpha.at(0.0), amount.alpha.at(end)), (0.0, 1.0));
        let page = row("ui:AccountPage.total");
        assert_eq!(page.offset.at(end), [0.0, 0.0]);
    }

    #[test]
    fn keys_are_tagged_and_the_foreign_key_bows_inside_the_canvas() {
        for size in [[1000.0, 800.0], [758.0, 780.0]] {
            let m = flow(&samples().b, size);
            let key = |id: &str| m.rows.iter().find(|r| r.id == id).unwrap().key.clone();
            assert_eq!(key("db:Accounts.Id"), Some(KeyMark::Primary));
            assert_eq!(key("db:Vouchers.Id"), Some(KeyMark::Primary));
            assert_eq!(
                key("db:Vouchers.AccountId"),
                Some(KeyMark::Foreign("db:Accounts.Id".into(), "Accounts.Id".into()))
            );
            assert_eq!(
                key("entity:Voucher.AccountId"),
                Some(KeyMark::Foreign("db:Accounts.Id".into(), "Accounts.Id".into())),
                "the entity's key names the table it points to"
            );
            assert_eq!(key("db:Vouchers.Amount"), None);
            assert_eq!(m.keys.len(), 1, "only a key within one column bows");
            let r = m.keys[0].span.rect;
            assert!(r[0] >= 0.0, "the bow stays on the canvas at {size:?}: {r:?}");
        }
    }

    /// The strands between neighbouring columns that cross after the change.
    fn crossed(m: &Flow) -> usize {
        let rows: HashMap<String, [f32; 4]> = m
            .rows
            .iter()
            .filter(|r| r.status != Status::Removed)
            .map(|r| (r.id.clone(), r.rest))
            .collect();
        let hops: Vec<(&str, &str)> = m
            .hops
            .iter()
            .filter(|h| h.status != Status::Removed)
            .map(|h| (h.from.as_str(), h.to.as_str()))
            .collect();
        crossings(&hops, &rows)
    }

    #[test]
    fn cards_are_ordered_so_fewer_strands_cross() {
        let listed = flow_ordered(&samples().b, [1000.0, 800.0], false);
        let m = b();
        let (before, after) = (crossed(&listed), crossed(&m));
        assert!(before > 0, "the listed order tangles");
        assert!(after < before, "{after} crossings, down from {before}");
        let top = |t: &str| m.cards.iter().find(|c| c.title == t).unwrap().rest[1];
        assert!(
            top("PaymentVm") < top("VoucherDto"),
            "the account's total stays level with its entity"
        );
        assert!(top("AccountPage") < top("VoucherForm"));
        assert_eq!(crossed(&flow(&samples().b, [1000.0, 800.0])), after, "the same every time");
    }

    #[test]
    fn new_strands_draw_left_to_right_and_one_reaches_the_screen() {
        let m = b();
        let hop = |a: &str, b: &str| m.hops.iter().find(|h| h.from == a && h.to == b).unwrap();
        let first = hop("db:Vouchers.Amount", "entity:Voucher.Amount");
        let last = hop("dto:VoucherDto.Amount", "ui:VoucherForm.amount");
        let starts = |h: &Hop| {
            (0..400)
                .map(|i| i as f32 * 0.05)
                .find(|t| h.trim.as_ref().unwrap().at(*t)[1] > 0.0)
                .unwrap()
        };
        assert!(starts(first) < starts(last), "left to right");
        let gone = hop("entity:Account.LegacyTotal", "dto:PaymentVm.LegacyTotal");
        assert_eq!(gone.trim.as_ref().unwrap().at(m.duration), [0.0, 0.0]);
        assert_eq!(m.pulses.len(), 1, "only the voucher amount newly reaches a screen");
        assert_eq!(m.keys.len(), 1, "the voucher's account key");
        let k = &m.keys[0];
        assert_eq!((k.from.as_str(), k.to.as_str()), ("db:Vouchers.AccountId", "db:Accounts.Id"));
        assert_eq!((k.head.at(0.0), k.head.at(m.duration)), (0.0, 1.0), "a new key draws on");
        let mut last = 0.0;
        for t in m.beats {
            assert!(t > last && t < m.duration, "{:?}", m.beats);
            last = t;
        }
    }
}
