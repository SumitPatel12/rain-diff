//! The linked-text tour (Red Blob Games, Distill, Code Hike): prose is the primary surface and
//! the figure beside it follows the reading. The prose is schema A's tour steps and the figure
//! is the line map: each step glides the camera to the parts it names, dims the rest and runs a
//! labelled packet along each call between them, one call after another. Every coloured name in
//! the prose lights its mark on hover.

use rain_diagram::DiagramStyle;
use rune::{Color, Cursor, El, col, el, row, text};

use super::paint::{self, LABEL, MICRO, SMALL, STRONG};
use super::tour_map::{self, Anim, Map, Packet};
use crate::motion::{Camera, FocusTrack, Rig, Track};
use crate::schema::trace::{Span, Trace, spans};
use crate::schema::{SchemaId, Status};
use crate::style::{Act, Cx, Style};

pub struct Tour;

const PAD: f32 = 20.0;
const GAP: f32 = 10.0;
/// The title line over the figure.
const HEAD_H: f32 = 20.0;
/// A step card's padding, the gap between cards, and its heading line.
const CARD_PAD: f32 = 10.0;
const CARD_GAP: f32 = 8.0;
const CARD_HEAD: f32 = 18.0;
/// One line of a card's prose, chips included, and the space between lines.
const LINE_H: f32 = 20.0;
const LINE_GAP: f32 = 4.0;
/// "In short" over the summary, and one line of it.
const UPSHOT_HEAD: f32 = 20.0;
const UPSHOT_LINE: f32 = 16.0;
/// The figure is at least this much wider than tall, so the tiers run left to right.
const ASPECT: f32 = 1.45;
/// When the first step starts, how long a step lasts, and how long the camera takes to get
/// there.
const FIRST: f32 = 1.2;
const STEP_T: f32 = 4.0;
const GLIDE: f32 = 0.9;
/// The longest a packet takes over one call.
const PULSE_T: f32 = 1.0;
/// Opacity of a step that is not the one being read.
const ASIDE: f32 = 0.42;
const MAX_ZOOM: f32 = 1.5;

/// Where the parts sit: the title, the figure across the top, a card per step under it (two to a
/// row, four on a wide canvas), and the summary under the cards.
pub(crate) struct Frame {
    pub head: [f32; 4],
    pub figure: [f32; 4],
    pub cards: Vec<[f32; 4]>,
    pub upshot: [f32; 4],
}

/// How many lines `body` takes in a column `room` wide: words wrap, and a link is a chip named
/// by `name`.
pub(crate) fn prose_lines(body: &str, room: f32, name: &dyn Fn(&str) -> String) -> usize {
    let measure = |t: &str, font: rune::tok::TextStyle| rain_diagram::design::measure(t, font.size);
    let (mut n, mut used) = (1, 0.0);
    for sp in spans(body) {
        let widths: Vec<f32> = match sp {
            Span::Text(t) => t.split_whitespace().map(|w| measure(w, LABEL) * 0.8).collect(),
            Span::Link(id) => vec![(measure(&name(id), STRONG) * 0.85 + 24.0).min(room)],
        };
        for w in widths {
            let need = if used > 0.0 { used + 4.0 + w } else { w };
            if need > room && used > 0.0 {
                n += 1;
                used = w;
            } else {
                used = need;
            }
        }
    }
    n
}

/// A link's name in the prose: its entity's or path's label.
fn link_name(a: &Trace, id: &str) -> String {
    a.entity(id)
        .map(|e| e.label.clone())
        .or_else(|| a.path(id).map(|p| p.name.clone()))
        .unwrap_or_else(|| id.to_owned())
}

pub(crate) fn frame(size: [f32; 2], a: &Trace, said: &[(usize, String)]) -> Frame {
    let w = size[0] - 2.0 * PAD;
    let cols = if w >= 1100.0 { 4 } else { 2 };
    let cw = (w - CARD_GAP * (cols - 1) as f32) / cols as f32;
    let room = cw - 2.0 * CARD_PAD;
    let heights: Vec<f32> = a
        .tour
        .iter()
        .map(|s| {
            let n = prose_lines(&s.text, room, &|id| link_name(a, id)) as f32;
            2.0 * CARD_PAD + CARD_HEAD + 6.0 + n * (LINE_H + LINE_GAP) - LINE_GAP
        })
        .collect();
    let row_h: Vec<f32> = heights.chunks(cols).map(|c| c.iter().copied().fold(0.0, f32::max)).collect();
    let cards_h = row_h.iter().sum::<f32>() + CARD_GAP * row_h.len().saturating_sub(1) as f32;
    let text_w = w - 22.0;
    let upshot_h = UPSHOT_HEAD
        + said
            .iter()
            .map(|(_, s)| (rain_diagram::design::measure(s, LABEL.size) * 0.9 / text_w).ceil().max(1.0) * UPSHOT_LINE + 3.0)
            .sum::<f32>();
    let fig_y = PAD + HEAD_H + GAP;
    let room_h = size[1] - fig_y - GAP - cards_h - GAP - upshot_h - PAD;
    let fh = room_h.min(w / ASPECT).max(160.0);
    let mut y = fig_y + fh + GAP;
    let mut cards = Vec::new();
    for (r, h) in row_h.iter().enumerate() {
        for c in 0..cols {
            if r * cols + c < heights.len() {
                cards.push([PAD + c as f32 * (cw + CARD_GAP), y, cw, *h]);
            }
        }
        y += h + CARD_GAP;
    }
    Frame {
        head: [PAD, PAD, w, HEAD_H],
        figure: [PAD, fig_y, w, fh],
        cards,
        upshot: [PAD, y - CARD_GAP + GAP, w, upshot_h],
    }
}

/// One tour step: the time it starts and how bright it is.
pub(crate) struct Para {
    pub t: f32,
    pub title: String,
    pub alpha: Track<f32>,
}

/// A link in the prose: what it says, its colour and what hovering it lights.
pub(crate) struct Chip {
    pub label: String,
    pub colour: Color,
    pub lights: Vec<String>,
}

pub(crate) struct Data {
    pub frame: Frame,
    pub paras: Vec<Para>,
    pub map: Map,
    pub anim: Anim,
    /// The summary, one sentence per path.
    pub said: Vec<(usize, String)>,
}

/// Each paragraph bright while it is the one being read, dimmer before the first.
pub(crate) fn para_alphas(times: &[f32]) -> Vec<Track<f32>> {
    let first = times.first().copied().unwrap_or(0.0);
    (0..times.len())
        .map(|k| {
            let mut keys = Vec::new();
            if first > 0.2 {
                keys.extend([(0.0, 0.8), (first - 0.2, 0.8)]);
            }
            if k == 0 {
                keys.push((first, 1.0));
            } else {
                keys.extend([(first, ASIDE), (times[k] - 0.2, ASIDE), (times[k], 1.0)]);
            }
            if let Some(n) = times.get(k + 1) {
                keys.extend([(n - 0.2, 1.0), (*n, ASIDE)]);
            }
            keys.sort_by(|a, b| a.0.total_cmp(&b.0));
            Track::new(keys)
        })
        .collect()
}

fn map_rig(cx: &Cx) -> Rig {
    let a = &cx.samples.a;
    let said = tour_map::conclusion(a);
    let fr = frame(cx.size, a, &said);
    let fsize = [fr.figure[2], fr.figure[3]];
    let map = tour_map::map(a, cx.ink, fsize, SMALL);
    let mut anim = Anim::still(&map);
    let times: Vec<f32> = (0..a.tour.len()).map(|k| FIRST + k as f32 * STEP_T).collect();
    let duration = times.last().map_or(FIRST, |t| t + STEP_T);

    let cam = Camera::new(fsize);
    let mut keys = vec![(0.0, [fsize[0] / 2.0, fsize[1] / 2.0], 1.0)];
    let mut focus = vec![(0.0, Vec::new())];
    for (k, step) in a.tour.iter().enumerate() {
        let t = times[k];
        // The map is laid out here, not by the author, so the camera frames what the step
        // names; an authored camera key only caps the zoom.
        let most = step.camera.map_or(MAX_ZOOM, |c| c.zoom.min(MAX_ZOOM));
        let (c, z) = match map.bounds(&step.focus) {
            Some(r) => ([r[0] + r[2] / 2.0, r[1] + r[3] / 2.0], cam.fit(r, 48.0).min(most)),
            None => ([fsize[0] / 2.0, fsize[1] / 2.0], 1.0),
        };
        let last = *keys.last().unwrap_or(&(0.0, [0.0; 2], 1.0));
        keys.push((t, last.1, last.2));
        keys.push((t + GLIDE, c, z));
        let mut ids = step.focus.clone();
        let calls: Vec<usize> = (0..map.segs.len())
            .filter(|&i| step.focus.contains(&map.segs[i].from) && step.focus.contains(&map.segs[i].to))
            .collect();
        ids.extend(calls.iter().map(|&i| tour_map::seg_id(i)));
        // One packet at a time, in calling order, so no two labels share the figure.
        let moving: Vec<usize> = calls.into_iter().filter(|&i| map.segs[i].status != Status::Removed).collect();
        let room = STEP_T - GLIDE - 0.3;
        let each = (room / moving.len().max(1) as f32).min(PULSE_T);
        for (j, &i) in moving.iter().enumerate() {
            anim.packets
                .push(Packet::new(&map, i, t + GLIDE + j as f32 * each, each * 0.95, true));
        }
        focus.push((t, ids));
    }
    let paras = a
        .tour
        .iter()
        .zip(para_alphas(&times))
        .zip(&times)
        .map(|((s, alpha), t)| Para {
            t: *t,
            title: s.title.clone(),
            alpha,
        })
        .collect::<Vec<_>>();
    let tracks: Vec<Track<f32>> = paras.iter().map(|p| p.alpha.clone()).collect();
    let anim_c = anim.clone();
    let titles: Vec<(f32, String)> = paras.iter().map(|p| (p.t, p.title.clone())).collect();
    let data = Data {
        frame: fr,
        paras,
        map,
        anim,
        said,
    };
    let overview = match cx.samples.a.paths.len() {
        1 => "Overview: the path".to_owned(),
        2 => "Overview: both paths".to_owned(),
        n => format!("Overview: all {n} paths"),
    };
    let mut rig = Rig::new(data, cx.size, duration).step(0.0, overview);
    for (t, title) in titles {
        rig = rig.step(t, title);
    }
    rig.camera = Camera::keyed(fsize, keys);
    anim_c.add_to(&mut rig);
    for t in tracks {
        rig.add(t);
    }
    let ft = FocusTrack::new(rig.focus.clone(), focus);
    rig.add(ft);
    rig
}

/// What a `[[id]]` link looks like and lights on the map.
fn map_chip(a: &Trace, m: &Map, ink: &DiagramStyle, id: &str) -> Chip {
    if let Some(l) = m.lines.iter().find(|l| l.id == id) {
        let li = m.lines.iter().position(|x| x.id == id).unwrap_or(0);
        let mut lights = vec![tour_map::line_id(l)];
        lights.extend(m.stations.iter().filter(|s| s.lines.contains(&li)).map(|s| s.id.clone()));
        return Chip {
            label: l.name.clone(),
            colour: l.colour,
            lights,
        };
    }
    let label = a.entity(id).map_or_else(|| id.to_owned(), |e| e.label.clone());
    match m.station(id) {
        Some(s) => {
            let mut lights = vec![s.id.clone()];
            lights.extend(s.lines.iter().map(|l| tour_map::line_id(&m.lines[*l])));
            Chip {
                label,
                colour: if s.interchange() {
                    ink.foreground
                } else {
                    m.lines[s.lines[0]].colour
                },
                lights,
            }
        }
        None => Chip {
            label,
            colour: ink.muted,
            lights: vec![id.to_owned()],
        },
    }
}

/// A paragraph's words with its links as chips; hovering a chip lights its marks.
pub(crate) fn prose(body: &str, rig: &Rig, ink: &DiagramStyle, key: &str, room: f32, chip: &dyn Fn(&str) -> Chip) -> El {
    let mut flow = row().w_full().flex_wrap().gaps(4.0, 4.0).items_center();
    for (i, s) in spans(body).into_iter().enumerate() {
        match s {
            Span::Text(t) => {
                for w in t.split_whitespace() {
                    flow = flow.child(text(w.to_owned()).font(LABEL).color(ink.foreground));
                }
            }
            Span::Link(id) => {
                let c = chip(id);
                let e = row()
                    .key(format!("{key}-link-{i}"))
                    .gap(5.0)
                    .items_center()
                    .px(5.0)
                    .py(1.0)
                    .radius(5.0)
                    .fill(c.colour.alpha(0.16))
                    .border(1.0, c.colour.alpha(0.5))
                    .max_w(room)
                    .role("link")
                    .label(c.label.clone())
                    .help(c.label.clone())
                    .child(el().size(7.0, 7.0).radius(3.5).fill(c.colour).a11y_hidden())
                    .child(
                        text(c.label.clone())
                            .font(STRONG)
                            .color(ink.foreground)
                            .truncate()
                            .shrink()
                            .a11y_hidden(),
                    );
                flow = flow.child(rig.highlight.hover(e, c.lights));
            }
        }
    }
    flow
}

impl Style for Tour {
    fn id(&self) -> &'static str {
        "tour"
    }
    fn name(&self) -> &'static str {
        "Linked-text tour"
    }
    fn pitch(&self) -> &'static str {
        "Prose beside a line map; each step focuses, moves the camera and runs what each call carries."
    }
    fn schemas(&self) -> &'static [SchemaId] {
        &[SchemaId::A]
    }
    fn how_to_read(&self) -> Option<&'static str> {
        Some(
            "Read the steps under the figure; the figure follows. Each request path is a \
             coloured line and each component it calls a station, grouped left to right into \
             frontend, backend and database; a bold station is shared by both paths, and the page is a small block \
             holding its form. Each step glides the camera to the parts it names, dims the \
             rest and runs a packet along each call, labelled with what the call carries. \
             Every coloured name in the text is a link: hover it to light its mark; press a \
             step to jump to it. \"In short\" sums the change up.",
        )
    }

    fn rig(&self, cx: &Cx) -> Rig {
        map_rig(cx)
    }

    fn view(&self, cx: &Cx, rig: &Rig) -> El {
        let Some(d) = rig.data::<Data>() else { return el() };
        let ink = cx.ink;
        let [fx, fy, fw, fh] = d.frame.figure;

        // The figure, under the camera.
        let picture = tour_map::draw("tour-map", &d.map, ink, cx.reduced, &rig.highlight, &rig.focus, &d.anim);
        let world = rig.camera.bind(el().key("tour-world").size(fw, fh).child(picture), cx.reduced);
        let figure = el()
            .key("tour-figure")
            .absolute(fx, fy)
            .size(fw, fh)
            .radius(10.0)
            .fill(ink.card)
            .border(1.0, ink.card_border)
            .clip_rounded(10.0)
            .role("img")
            .label("The figure the tour walks through")
            .child(world);

        // The title, the step cards and the summary.
        let a = &cx.samples.a;
        let [hx, hy, hw, hh] = d.frame.head;
        let mut stage = el().key("tour").size(cx.size[0], cx.size[1]).child(
            row()
                .absolute(hx, hy)
                .size(hw, hh)
                .gap(10.0)
                .items_center()
                .child(text(a.title.clone()).font(paint::TITLE).color(ink.foreground))
                .child(text("A tour of the change").font(SMALL).color(ink.muted)),
        );
        stage = stage.child(figure);
        for (k, para) in d.paras.iter().enumerate() {
            let [x, y, w, h] = d.frame.cards[k];
            let room = w - 2.0 * CARD_PAD;
            let body = prose(&a.tour[k].text, rig, ink, &format!("tour-step-{k}"), room, &|id| {
                map_chip(a, &d.map, ink, id)
            });
            let head = row()
                .gap(8.0)
                .items_center()
                .child(
                    paint::centred((k + 1).to_string(), MICRO, ink.background, 18.0, 18.0)
                        .radius(9.0)
                        .fill(ink.foreground),
                )
                .child(text(para.title.clone()).font(STRONG).color(ink.foreground).truncate().shrink());
            let card = col()
                .key(format!("tour-step-{k}"))
                .absolute(x, y)
                .size(w, h)
                .gap(6.0)
                .padding(CARD_PAD)
                .radius(8.0)
                .fill(ink.foreground.alpha(0.04))
                .border(1.0, ink.card_border)
                .cursor(Cursor::Pointer)
                .role("button")
                .label(format!("Go to step {}: {}", k + 1, para.title))
                .child(head)
                .child(body)
                .live_opacity(&para.alpha.live);
            stage = stage.child(cx.act(card, Act::Seek(para.t)));
        }
        let [ux, uy, uw, _] = d.frame.upshot;
        stage.child(
            col()
                .absolute(ux, uy)
                .w(uw)
                .gap(6.0)
                .child(text("In short").font(MICRO).color(ink.muted))
                .child(tour_map::upshot("tour-conclusion", &d.said, &d.map, ink, uw)),
        )
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

    fn lab() -> Lab {
        let mut lab = Lab::new();
        lab.select(registry().iter().position(|s| s.id() == "tour").unwrap());
        let _ = lab.canvas(&rune_kit::Kit::neutral(true), &DiagramStyle::dark(), [1000.0, 760.0], &no_act);
        lab
    }

    #[test]
    fn each_step_lights_what_its_prose_names() {
        let a = &samples().a;
        let lab = lab();
        let rig = lab.rig().unwrap();
        let d = rig.data::<Data>().unwrap();
        let (map, anim) = (&d.map, &d.anim);
        assert_eq!(d.paras.len(), a.tour.len());
        assert!(
            anim.packets
                .iter()
                .any(|p| p.label.as_deref() == Some("VoucherDto {accountId, amount}")),
            "the pulses say what they carry"
        );
        assert_eq!(map.boxes.len(), 1, "the page holds its form");
        for (k, step) in a.tour.iter().enumerate() {
            let t = d.paras[k].t + GLIDE + 0.1;
            rig.publish(t);
            let lit = rig.focus.current();
            for id in &step.focus {
                assert!(lit.contains(id), "step {k} lights {id}");
            }
            assert!(lit.iter().any(|id| id.starts_with("seg:")), "step {k} lights a call");
            assert_eq!(d.paras[k].alpha.at(t), 1.0, "step {k} is the one being read");
            for (j, p) in d.paras.iter().enumerate().filter(|(j, _)| *j != k) {
                assert!(p.alpha.at(t) < 0.5, "step {j} steps back while {k} is read");
            }
            for s in spans(&step.text) {
                if let Span::Link(id) = s {
                    let c = map_chip(a, map, &DiagramStyle::dark(), id);
                    assert!(c.colour != DiagramStyle::dark().muted, "{id} is on the map");
                    assert!(!c.lights.is_empty());
                }
            }
        }
        let ats: Vec<f32> = rig.steps.iter().map(|s| s.at).collect();
        assert!(ats.windows(2).all(|w| w[0] < w[1]) && ats.iter().all(|t| *t < rig.duration));
        rig.publish(d.paras[1].t + GLIDE);
        let pose = (rig.camera.offset.get(), rig.camera.scale.get());
        rig.publish(rig.duration);
        rig.publish(d.paras[1].t + GLIDE);
        assert_eq!(
            (rig.camera.offset.get(), rig.camera.scale.get()),
            pose,
            "the camera is a function of time"
        );
        assert!(pose.1 > 1.0 && pose.1 <= MAX_ZOOM, "it moves in on the step");
    }

    #[test]
    fn the_figure_is_landscape_with_tiers_left_to_right_and_the_cards_under_it() {
        let a = &samples().a;
        for size in [[1000.0, 760.0], [800.0, 760.0], [1400.0, 820.0]] {
            let said = tour_map::conclusion(a);
            let f = frame(size, a, &said);
            let [fx, fy, fw, fh] = f.figure;
            assert!(fw > fh * 1.1, "{size:?}: the figure is wider than tall ({fw} × {fh})");
            assert!(fx >= 0.0 && fy >= f.head[1] + f.head[3]);
            let inside = |r: [f32; 4]| r[0] >= 0.0 && r[1] >= 0.0 && r[0] + r[2] <= size[0] && r[1] + r[3] <= size[1];
            assert!(inside(f.figure) && inside(f.upshot), "{size:?}");
            assert_eq!(f.cards.len(), a.tour.len());
            for (i, c) in f.cards.iter().enumerate() {
                assert!(inside(*c) && c[1] >= fy + fh, "{size:?}: card {i} under the figure");
                assert!(f.upshot[1] >= c[1] + c[3], "{size:?}: the summary under card {i}");
                for d in &f.cards[i + 1..] {
                    let apart = c[0] + c[2] <= d[0] || d[0] + d[2] <= c[0] || c[1] + c[3] <= d[1] || d[1] + d[3] <= c[1];
                    assert!(apart, "{size:?}: cards overlap");
                }
            }
            let m = tour_map::map(a, &DiagramStyle::dark(), [fw, fh], SMALL);
            for w in m.districts.windows(2) {
                assert!(w[0].rect[0] + w[0].rect[2] <= w[1].rect[0], "{size:?}: tiers run left to right");
            }
        }
    }

    #[test]
    fn one_packet_label_shows_at_a_time_and_none_covers_a_station_name() {
        let lab = lab();
        let rig = lab.rig().unwrap();
        let d = rig.data::<Data>().unwrap();
        let names = tour_map::names(&d.map);
        let mut covered = 0;
        let mut t = 0.0;
        while t < rig.duration {
            let shown: Vec<&Packet> = d
                .anim
                .packets
                .iter()
                .filter(|p| p.label_alpha.as_ref().is_some_and(|a| a.at(t) > 0.05))
                .collect();
            assert!(shown.len() <= 1, "{} labels at {t}", shown.len());
            for p in shown {
                let [w, h] = tour_map::label_size(p.label.as_deref().unwrap_or(""));
                let r = [p.label_at[0], p.label_at[1], w, h];
                let hit = |n: &[f32; 4]| !(r[0] + r[2] <= n[0] || n[0] + n[2] <= r[0] || r[1] + r[3] <= n[1] || n[1] + n[3] <= r[1]);
                covered += names.iter().filter(|n| hit(n)).count();
                for s in &d.map.stations {
                    let h = s.diameter() / 2.0;
                    covered += usize::from(hit(&[s.at[0] - h, s.at[1] - h, 2.0 * h, 2.0 * h]));
                }
                assert!(r[0] >= 0.0 && r[1] >= 0.0 && r[0] + w <= d.map.size[0] && r[1] + h <= d.map.size[1]);
            }
            t += 0.05;
        }
        assert_eq!(covered, 0, "a shown label never covers a station or its name");
    }
}
