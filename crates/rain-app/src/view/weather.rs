//! Rain's weather, drawn like the app icon: one cloud with a scalloped underside, red and green
//! drops falling from it and splashing where they land. The raining loader, and the flood that
//! asks for a break.
//!
//! Drops are streaks: a drop moving `v` points a second draws as a line about `v / 30` long,
//! clear at the top and solid at the head, as a camera or the eye smears it. A streak at least
//! as long as one frame's step, and few enough drops that a lane holds one at a time, keeps the
//! eye from pairing a drop with the next one up (the wagon-wheel effect that reads as rising).

use std::time::Instant;

use rune::{Color, El, Shape, Tx, canvas, col, el, row, text};
use rune_kit::Kit;

use super::ui::{outline, primary};
use crate::app::App;
use crate::app::rest::{DRAIN, FLOOD_IN, Weather, flood_level};
use crate::theme::{Look, TYPE, brand, radius};

/// A fixed pseudo-random number in 0..1 for drop `i`, so each drop keeps its lane and pace.
fn hash(i: u32, salt: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9E37_79B9) ^ salt.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

/// The drop colours mix across the sky: red on the left, green on the right, both in between.
fn drop_color(i: u32, at: f32) -> Color {
    let green = ((at - 0.25) / 0.5).clamp(0.0, 1.0);
    if hash(i, 77) < green { brand::green() } else { brand::red() }
}

/// One cloud band `w` wide, `depth` deep from the top, its underside a row of scallops that
/// billow slowly: a dark layer under a light one, as on the icon. Opaque, so nothing doubles.
fn cloud_band(w: f32, depth: f32, t: f32, alpha: f32) -> El {
    let scallop = 64.0;
    let n = (w / (scallop * 0.82)).ceil() as u32 + 2;
    let mut band = el().size(w, depth + scallop).a11y_hidden();
    for (layer, drop, ink) in [(0u32, 14.0, brand::cloud_back()), (1, 0.0, brand::cloud_front())] {
        let ink = ink.alpha(alpha);
        band = band.child(el().size(w, depth + drop).fill(ink).absolute(0.0, 0.0));
        for i in 0..n {
            let billow = (t * 0.8 + i as f32 * 1.3 + layer as f32).sin() * 4.0;
            let d = scallop * (0.9 + 0.25 * hash(i, 11 + layer));
            let x = i as f32 * scallop * 0.82 - scallop * 0.4 + layer as f32 * scallop * 0.35;
            band = band.child(
                el().size(d, d)
                    .radius(d / 2.0)
                    .fill(ink)
                    .absolute(x, depth + drop - d * 0.55 + billow),
            );
        }
    }
    band
}

/// Drops in the box from `left, top`, `w` wide, falling `fall` points to a surface; each lands
/// with a ring that spreads and fades. `start` holds the drops back until the cloud is in.
struct Shower {
    /// Prefix for the drops' keys. Their count changes every frame, and unkeyed siblings are
    /// matched by position, so whatever came after them would re-enter (fade in) each frame.
    key: &'static str,
    left: f32,
    top: f32,
    w: f32,
    fall: f32,
    count: u32,
    speed: f32,
    width: f32,
    alpha: f32,
}

impl Shower {
    fn draw(&self, t: f32, out: &mut Vec<El>) {
        for i in 0..self.count {
            let at = (i as f32 + hash(i, 1) * 0.8) / self.count as f32;
            let x = self.left + at * self.w;
            let v = self.speed * (0.85 + 0.3 * hash(i, 2));
            let len = (v / 30.0).max(12.0);
            let period = (self.fall + len) / v + 0.25 * hash(i, 3);
            let phase = hash(i, 4) * period;
            let since = (t + phase) % period;
            let ink = drop_color(i, at);
            // The streak: its head travels from the cloud to the surface, its tail behind it.
            let head = self.top + since * v;
            let surface = self.top + self.fall;
            if head - len < surface {
                let y0 = (head - len).max(self.top);
                let y1 = head.min(surface);
                if y1 - y0 > 1.0 {
                    out.push(
                        el().key(format!("{}drop-{i}", self.key))
                            .size(self.width, y1 - y0)
                            .radius(self.width / 2.0)
                            .gradient(ink.alpha(0.0), ink.alpha(self.alpha))
                            .absolute(x, y0)
                            .a11y_hidden(),
                    );
                }
            }
            // The splash: a flat ring where it landed, spreading and fading.
            let landed = since - self.fall / v;
            if (0.0..0.45).contains(&landed) {
                let p = landed / 0.45;
                let rw = 6.0 + 26.0 * p;
                out.push(
                    el().key(format!("{}splash-{i}", self.key))
                        .size(rw, rw * 0.3)
                        .radius(rw * 0.15)
                        .border(1.5, ink.alpha(self.alpha * (1.0 - p)))
                        .absolute(x - rw / 2.0, surface - rw * 0.15)
                        .a11y_hidden(),
                );
            }
        }
    }
}

/// The loader: one small cloud raining red and green into a puddle, over the loading message.
pub(super) fn loader(s: &App, msg: &str, look: &Look) -> El {
    let k = &look.kit;
    let calm = s.rest.calm.get();
    let t = s.rest.clock;
    let (w, h) = (200.0, 120.0);
    let (cx, cw) = (34.0, 132.0);
    let puff = |d: f32, x: f32, y: f32, ink: Color| el().size(d, d).radius(d / 2.0).fill(ink).absolute(x, y);
    let mut drops = Vec::new();
    if !calm {
        Shower {
            key: "loader-",
            left: cx + 12.0,
            top: 40.0,
            w: cw - 24.0,
            fall: 66.0,
            count: 14,
            speed: 70.0,
            width: 3.5,
            alpha: 0.95,
        }
        .draw(t, &mut drops);
    }
    let bob = if calm { 0.0 } else { (t * 1.1).sin() * 1.5 };
    let (back, front) = (brand::cloud_back(), brand::cloud_front());
    let scene = el()
        .key("loader-rain")
        .size(w, h)
        // Every frame redraws the weather from the clock; springing a drop from where it was
        // last frame (the top, after it lands) would send it flying back up.
        .instant_changes()
        .a11y_hidden()
        .child(
            el().key("loader-puddle")
                .size(150.0, 3.0)
                .radius(1.5)
                .fill(k.alpha(k.c.icon_faint, 0.35))
                .absolute(25.0, 107.0),
        )
        .children(drops)
        .child(
            el().key("loader-cloud")
                .size(w, h)
                .absolute(0.0, bob)
                .child(el().size(cw, 22.0).radius(11.0).fill(back).absolute(cx, 26.0))
                .child(puff(46.0, cx + 14.0, 6.0, back))
                .child(puff(58.0, cx + 44.0, -4.0, back))
                .child(puff(42.0, cx + 84.0, 6.0, back))
                .child(el().size(cw, 20.0).radius(10.0).fill(front).absolute(cx, 20.0))
                .child(puff(42.0, cx + 16.0, 2.0, front))
                .child(puff(54.0, cx + 46.0, -8.0, front))
                .child(puff(38.0, cx + 86.0, 2.0, front)),
        );
    col()
        .key("loading")
        .role("status")
        .label(format!("Loading. {msg}"))
        .items_center()
        .gap(4.0)
        .child(scene)
        .child(text("Loading").font(TYPE.h2).color(k.c.text_base).pt(10.0).a11y_hidden())
        .child(text(msg.to_owned()).font(k.ty.small).color(k.c.text_muted).a11y_hidden())
}

/// The flood over the whole window while it rains or drains; `None` when clear.
pub(super) fn flood(s: &App, look: &Look) -> Option<El> {
    let now = Instant::now();
    // Reduced motion: the flood as it stands once risen, still.
    let calm = s.rest.calm.get();
    let (since, level, drain) = match s.rest.weather {
        Weather::Clear => return None,
        Weather::Flood(_) if calm => (FLOOD_IN, 1.0, 0.0),
        Weather::Flood(at) => {
            let since = now.saturating_duration_since(at);
            (since, flood_level(since), 0.0)
        }
        Weather::Drain(at, from) => {
            let d = (now.saturating_duration_since(at).as_secs_f32() / DRAIN.as_secs_f32()).clamp(0.0, 1.0);
            (FLOOD_IN, from as f32 / 1000.0 * (1.0 - d).powi(2), d)
        }
    };
    let (w, h) = rune::memo::layout_size("app").map_or((1440.0, 900.0), |sz| (sz[0], sz[1]));
    let t = if calm { 0.0 } else { s.rest.clock };
    let secs = since.as_secs_f32();
    // The cloud slides down over the first 2.5 s and lifts away as the water drains.
    let gather = 1.0 - (1.0 - (secs / 2.5).clamp(0.0, 1.0)).powi(3);
    let fade = 1.0 - drain;
    let depth = 70.0;
    let cloud_y = -(depth + 70.0) * (1.0 - gather) - (depth + 90.0) * drain;
    let cloud_bottom = cloud_y + depth + 14.0;

    let mut sky = el()
        .key("flood")
        .size(w, h)
        .instant_changes()
        .fill(brand::sky_top().alpha(0.45 * gather * fade))
        .clip_rounded(0.0);
    // Draining, it lets clicks through to the app as the water falls away.
    if matches!(s.rest.weather, Weather::Flood(_)) {
        sky = sky.on_press(|_: &mut App| {});
    }

    let water_h = level * h * 0.55;
    let surface = h - water_h;
    // Rain from the cloud's underside once it is in, onto the water (or the window's floor).
    if !calm && gather > 0.98 && drain < 0.4 {
        let mut drops = Vec::new();
        Shower {
            key: "flood-",
            left: 0.0,
            top: cloud_bottom - 10.0,
            w,
            fall: surface - cloud_bottom + 10.0,
            count: (w / 18.0) as u32,
            speed: 170.0,
            width: 3.5,
            alpha: fade,
        }
        .draw(t, &mut drops);
        sky = sky.children(drops);
    }

    // The water: a deep-sky body rising from the bottom, its surface a row of humps that meet it
    // edge to edge, so the translucent water never doubles up.
    if water_h > 1.0 {
        let top_ink = brand::sky_top().alpha(0.9);
        let lip = 6.0;
        sky = sky.child(
            el().key("flood-water")
                .size(w, water_h - lip)
                .fill(top_ink)
                .absolute(0.0, surface + lip)
                .a11y_hidden(),
        );
        let hump = 60.0;
        for i in 0..(w / hump).ceil() as u32 {
            let hh = hump * 0.22 + lip;
            sky = sky.child(
                el().key(format!("flood-hump-{i}"))
                    .size(hump, hh)
                    .corners([hump * 0.5, hump * 0.5, 0.0, 0.0])
                    .fill(top_ink)
                    .absolute(i as f32 * hump, surface + lip - hh)
                    .a11y_hidden(),
            );
        }
    }

    // The card floats on the water; lightning may strike it.
    let card_at = (drain < 0.3).then(|| {
        let float = (t * 1.5).sin() * 4.0;
        let top = (h * 0.42 - 90.0).min(surface - 220.0).max(cloud_bottom + 40.0) + float;
        let ch = rune::memo::layout_size("break-card").map_or(170.0, |sz| sz[1]);
        [((w - CARD_W) / 2.0).max(0.0), top, CARD_W, ch]
    });
    let bolt = (!calm && gather > 0.98 && drain < 0.2)
        .then(|| strike(t, cloud_bottom, w, card_at))
        .flatten();

    if let Some(b) = &bolt {
        // The sky lights up a little with each flash.
        sky = sky.child(
            el().key("flood-flash")
                .size(w, h)
                .fill(b.glow.alpha(0.07 * b.bright))
                .absolute(0.0, 0.0)
                .a11y_hidden(),
        );
        sky = sky.child(canvas(b.shape()).key("flood-bolt").size(w, h).absolute(0.0, 0.0).a11y_hidden());
    }

    sky = sky.child(cloud_band(w, depth, t, 1.0).key("flood-cloud").absolute(0.0, cloud_y));

    if let Some(b) = &bolt {
        // The cloud glows from inside where the bolt leaves it.
        let gh = depth + 140.0;
        let gy = cloud_y + 10.0;
        sky = sky.child(
            el().key("flood-cloud-glow")
                .size(w, gh)
                .radial_gradient(
                    [b.from[0] / w, (cloud_bottom - gy) / gh],
                    [300.0, 130.0],
                    [
                        (0.0, b.glow.alpha(0.6 * b.bright)),
                        (0.5, b.glow.alpha(0.2 * b.bright)),
                        (1.0, b.glow.alpha(0.0)),
                    ],
                )
                .absolute(0.0, gy)
                .a11y_hidden(),
        );
    }

    if let Some([cx, cy, cw, ch]) = card_at {
        let hit = bolt.as_ref().filter(|b| b.hits_card);
        if let Some(b) = hit {
            // A halo around the card, fading with the flash.
            let pad = 140.0;
            sky = sky.child(
                el().key("break-halo")
                    .size(cw + pad * 2.0, ch + pad * 2.0)
                    .radial_gradient(
                        [0.5, 0.5],
                        [cw * 0.5 + pad, ch * 0.5 + pad],
                        [
                            (0.0, b.glow.alpha(0.5 * b.bright)),
                            (0.55, b.glow.alpha(0.3 * b.bright)),
                            (1.0, b.glow.alpha(0.0)),
                        ],
                    )
                    .absolute(cx - pad, cy - pad)
                    .a11y_hidden(),
            );
        }
        let shown = ((secs - 1.5) / 1.0).clamp(0.0, 1.0) * fade;
        let light = hit.map(|b| (b.bright, b.core, b.glow));
        let mut c = card(s, &look.kit, look, light).absolute(cx, cy);
        if shown < 0.999 {
            c = c.opacity(shown);
        }
        sky = sky.child(c);
    }
    Some(sky)
}

/// A strike every few seconds: a bright double flash, then a fade.
const STRIKE_EVERY: f32 = 3.4;
const STRIKE_FOR: f32 = 0.8;

/// How bright a strike is `local` seconds in: on, dim, on again, then a long fade.
fn strike_brightness(local: f32) -> f32 {
    match local {
        l if !(0.0..STRIKE_FOR).contains(&l) => 0.0,
        l if l < 0.07 => 1.0,
        l if l < 0.13 => 0.3,
        l if l < 0.22 => 0.95,
        l => 0.95 * (1.0 - (l - 0.22) / (STRIKE_FOR - 0.22)).powi(2),
    }
}

/// The lightning in the air now, if any.
struct Strike {
    bright: f32,
    core: Color,
    glow: Color,
    hits_card: bool,
    from: [f32; 2],
    main: Vec<[f32; 2]>,
    forks: Vec<Vec<[f32; 2]>>,
    /// How much of the bolt has drawn yet (it grows down from the cloud in a few frames).
    grown: f32,
}

impl Strike {
    /// The bolt with its halo: wide faint glow strokes under a bright core.
    fn shape(&self) -> Shape {
        let b = self.bright;
        let mut sh = Shape::new();
        for (width, ink) in [
            (26.0, self.glow.alpha(0.08 * b)),
            (14.0, self.glow.alpha(0.18 * b)),
            (6.0, self.glow.alpha(0.6 * b)),
            (3.0, self.core.alpha(b)),
            (1.2, Color::WHITE.alpha(0.9 * b)),
        ] {
            sh = sh.stroke(self.main.clone(), width, ink);
            for f in &self.forks {
                sh = sh.stroke(f.clone(), width * 0.6, ink);
            }
        }
        sh.drawn(self.grown)
    }
}

/// The strike at clock `t`: from under the cloud at `top`, to the card on every third strike,
/// else into the rain. Purple and green take turns.
fn strike(t: f32, top: f32, w: f32, card: Option<[f32; 4]>) -> Option<Strike> {
    let n = (t / STRIKE_EVERY).floor().max(0.0) as u32;
    let local = t - n as f32 * STRIKE_EVERY - 0.9 * hash(n, 41);
    let bright = strike_brightness(local);
    if bright <= 0.0 {
        return None;
    }
    let (core, glow) = if n.is_multiple_of(2) {
        (brand::purple_core(), brand::purple_glow())
    } else {
        (brand::green_core(), brand::green_glow())
    };
    let target = card
        .filter(|_| n % 3 == 1)
        .map(|[x, y, cw, _]| [x + cw * (0.2 + 0.6 * hash(n, 42)), y]);
    let (from, to) = match target {
        Some(to) => ([to[0] + (hash(n, 43) - 0.5) * 260.0, top - 16.0], to),
        None => {
            let x = w * (0.08 + 0.84 * hash(n, 44));
            (
                [x, top - 16.0],
                [x + (hash(n, 45) - 0.5) * 180.0, top + 170.0 + 220.0 * hash(n, 46)],
            )
        }
    };
    let steps = 9;
    let mut main = vec![from];
    for k in 1..steps {
        let f = k as f32 / steps as f32;
        let jitter = (hash(n * 32 + k, 47) - 0.5) * 46.0;
        main.push([from[0] + (to[0] - from[0]) * f + jitter, from[1] + (to[1] - from[1]) * f]);
    }
    main.push(to);
    let forks = [3usize, 6]
        .iter()
        .map(|&at| {
            let side = if hash(n * 8 + at as u32, 48) < 0.5 { -1.0 } else { 1.0 };
            let mut p = main[at];
            let mut fork = vec![p];
            for k in 0..3 {
                p = [
                    p[0] + side * (16.0 + 14.0 * hash(n * 8 + at as u32 + k, 49)),
                    p[1] + 18.0 + 10.0 * hash(n * 8 + k, 50),
                ];
                fork.push(p);
            }
            fork
        })
        .collect();
    Some(Strike {
        bright,
        core,
        glow,
        hits_card: target.is_some(),
        from,
        main,
        forks,
        grown: (local / 0.05).clamp(0.0, 1.0),
    })
}

const CARD_W: f32 = 420.0;

/// The card; `light` is a strike on it: (brightness, core, glow).
fn card(s: &App, k: &Kit, look: &Look, light: Option<(f32, Color, Color)>) -> El {
    let minutes = s.settings.break_reminder.minutes;
    let snooze = s.settings.break_reminder.snooze_minutes;
    let mins = |m: f64| {
        if m >= 1.0 {
            format!("{} min", m.round() as i64)
        } else {
            format!("{} s", (m * 60.0).round() as i64)
        }
    };
    let card = col()
        .key("break-card")
        .role("alertdialog")
        .label("Time for a break")
        .w(CARD_W)
        .padding(24.0)
        .gap(8.0)
        .radius(radius::ISLAND)
        .fill(look.r.raised)
        .border(1.0, look.s.card_border)
        .shadow(24.0, 64.0, look.r.shadow);
    // Struck: the edge burns in the bolt's colour, a glow spills out and the face lights from above.
    let card = match light {
        Some((b, core, glow)) => card
            .border(1.5, core.alpha(0.35 + 0.65 * b))
            .shadow(0.0, 56.0, glow.alpha(0.75 * b))
            .gradient(glow.alpha(0.22 * b), glow.alpha(0.0)),
        None => card,
    };
    card.child(
        text("It's raining. Time for a break.")
            .font(TYPE.h2)
            .color(k.c.text_base)
            .wrap()
            .w_full(),
    )
    .child(
        text(format!(
            "You've been reviewing for {} straight. Stand up, look outside, get some water. Rain keeps your place.",
            mins(minutes)
        ))
        .font(TYPE.body)
        .color(k.c.text_muted)
        .wrap()
        .w_full(),
    )
    .child(
        row()
            .pt(12.0)
            .gap(8.0)
            .items_center()
            .child(primary("break-taken", "I took a break", k).on_press_tx(|s: &mut App, _: &mut Tx| s.rest_taken()))
            .child(el().grow(1.0).a11y_hidden())
            .child(
                outline("break-snooze", format!("Snooze {}", mins(snooze)), Some("Esc"), 28.0, k)
                    .on_press_tx(|s: &mut App, _: &mut Tx| s.rest_snooze()),
            ),
    )
}
