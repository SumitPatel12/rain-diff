//! Rain's weather, drawn like the app icon: one cloud with a scalloped underside, red and green
//! drops falling from it and splashing where they land. The raining loader, and the flood that
//! asks for a break.
//!
//! Drops are streaks: a drop moving `v` points a second draws as a line about `v / 30` long,
//! clear at the top and solid at the head, as a camera or the eye smears it. A streak at least
//! as long as one frame's step, and few enough drops that a lane holds one at a time, keeps the
//! eye from pairing a drop with the next one up (the wagon-wheel effect that reads as rising).

use std::cell::{Cell, RefCell};
use std::time::Instant;

use rune::{Color, El, Live, Shape, Tx, canvas, col, el, row, text};
use rune_kit::Kit;

use super::ui::{Activate, outline, primary};
use crate::app::rest::{DRAIN, FLOOD_IN, Weather, flood_level};
use crate::app::{App, Phase};
use crate::theme::{Look, TYPE, brand, radius};

/// The values one drop's streak and splash are bound to.
#[derive(Clone)]
struct DropLive {
    streak: Live<[f32; 4]>,
    alpha: Live<f32>,
    splash: Live<[f32; 4]>,
    splash_alpha: Live<f32>,
}

impl DropLive {
    fn new() -> DropLive {
        DropLive {
            streak: Live::new([0.0; 4]),
            alpha: Live::new(0.0),
            splash: Live::new([0.0; 4]),
            splash_alpha: Live::new(0.0),
        }
    }
}

/// What a flood build mounted; a frame that needs other parts rebuilds the view.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Built {
    size: [f32; 2],
    drops: bool,
    water: bool,
    card: bool,
}

/// The weather's moving parts, bound to its elements, so a frame of rain moves them in the
/// renderer instead of rebuilding the window's view.
#[derive(Clone)]
pub(crate) struct Motion {
    loader_drops: RefCell<Vec<DropLive>>,
    flood_drops: RefCell<Vec<DropLive>>,
    bob: Live<[f32; 2]>,
    sky: Live<Color>,
    cloud: Live<[f32; 2]>,
    billows: RefCell<Vec<Live<[f32; 2]>>>,
    water: Live<[f32; 2]>,
    card: Live<[f32; 2]>,
    card_alpha: Live<f32>,
    built: Cell<Option<Built>>,
}

impl Default for Motion {
    fn default() -> Motion {
        Motion {
            loader_drops: RefCell::default(),
            flood_drops: RefCell::default(),
            bob: Live::new([0.0; 2]),
            sky: Live::new(Color::TRANSPARENT),
            cloud: Live::new([0.0; 2]),
            billows: RefCell::default(),
            water: Live::new([0.0; 2]),
            card: Live::new([0.0; 2]),
            card_alpha: Live::new(0.0),
            built: Cell::new(None),
        }
    }
}

impl std::fmt::Debug for Motion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Motion").field("built", &self.built.get()).finish_non_exhaustive()
    }
}

/// Moves the weather to the clock through its bound values; true when the frame needs a new
/// view instead (the flood mounts or drops a part, or lightning is in the air).
pub(crate) fn moved(s: &App) -> bool {
    let m = &s.rest.motion;
    let t = s.rest.clock;
    if matches!(s.phase, Phase::Loading(_)) && !s.rest.calm.get() {
        LOADER.publish(&m.loader_drops, t);
        m.bob.set([0.0, bob(t)]);
    }
    let Some(built) = m.built.get() else {
        return s.rest.weather != Weather::Clear;
    };
    let Some(f) = Flood::at(s, built.size) else { return true };
    if f.strike(None).is_some() || f.built() != built {
        return true;
    }
    f.publish(m);
    false
}

fn bob(t: f32) -> f32 {
    (t * 1.1).sin() * 1.5
}

fn billow(t: f32, i: u32, layer: u32) -> f32 {
    (t * 0.8 + i as f32 * 1.3 + layer as f32).sin() * 4.0
}

const SCALLOP: f32 = 64.0;

fn scallops(w: f32) -> u32 {
    (w / (SCALLOP * 0.82)).ceil() as u32 + 2
}

fn publish_billows(billows: &RefCell<Vec<Live<[f32; 2]>>>, w: f32, t: f32) {
    let n = scallops(w);
    let mut b = billows.borrow_mut();
    while b.len() < 2 * n as usize {
        b.push(Live::new([0.0; 2]));
    }
    for layer in 0..2 {
        for i in 0..n {
            b[(layer * n + i) as usize].set([0.0, billow(t, i, layer)]);
        }
    }
}

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
fn cloud_band(w: f32, depth: f32, t: f32, alpha: f32, billows: &RefCell<Vec<Live<[f32; 2]>>>) -> El {
    let scallop = SCALLOP;
    let n = scallops(w);
    publish_billows(billows, w, t);
    let billows = billows.borrow();
    let mut band = el().size(w, depth + scallop).a11y_hidden();
    for (layer, drop, ink) in [(0u32, 14.0, brand::cloud_back()), (1, 0.0, brand::cloud_front())] {
        let ink = ink.alpha(alpha);
        band = band.child(el().size(w, depth + drop).fill(ink).absolute(0.0, 0.0));
        for i in 0..n {
            let d = scallop * (0.9 + 0.25 * hash(i, 11 + layer));
            let x = i as f32 * scallop * 0.82 - scallop * 0.4 + layer as f32 * scallop * 0.35;
            band = band.child(
                el().size(d, d)
                    .radius(d / 2.0)
                    .fill(ink)
                    .absolute(x, depth + drop - d * 0.55)
                    .live_offset(&billows[(layer * n + i) as usize]),
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

/// The spring Rune gives a colour change in a rebuild, which bound fades keep.
const COLOUR_SPRING: rune::Spring = rune::Spring::new(0.3, 0.0);

/// A splash's corner radius: more than half its height, so it rounds to a pill at every size.
const SPLASH_ROUND: f32 = 16.0;

impl Shower {
    fn lane(&self, i: u32) -> f32 {
        (i as f32 + hash(i, 1) * 0.8) / self.count as f32
    }

    /// Drop `i` at clock `t`: its streak's rect (empty while out of sight), and its splash's
    /// rect and fade (0 while it has none).
    fn drop_at(&self, i: u32, t: f32) -> ([f32; 4], [f32; 4], f32) {
        let x = self.left + self.lane(i) * self.w;
        let v = self.speed * (0.85 + 0.3 * hash(i, 2));
        let len = (v / 30.0).max(12.0);
        let period = (self.fall + len) / v + 0.25 * hash(i, 3);
        let phase = hash(i, 4) * period;
        let since = (t + phase) % period;
        // The streak: its head travels from the cloud to the surface, its tail behind it.
        let head = self.top + since * v;
        let surface = self.top + self.fall;
        let mut streak = [x, self.top, 0.0, 0.0];
        if head - len < surface {
            let y0 = (head - len).max(self.top);
            let y1 = head.min(surface);
            if y1 - y0 > 1.0 {
                streak = [x, y0, self.width, y1 - y0];
            }
        }
        // The splash: a flat ring where it landed, spreading and fading.
        let landed = since - self.fall / v;
        let (mut splash, mut fade) = ([x, surface, 0.0, 0.0], 0.0);
        if (0.0..0.45).contains(&landed) {
            let p = landed / 0.45;
            let rw = 6.0 + 26.0 * p;
            splash = [x - rw / 2.0, surface - rw * 0.15, rw, rw * 0.3];
            fade = 1.0 - p;
        }
        (streak, splash, fade)
    }

    fn publish(&self, pool: &RefCell<Vec<DropLive>>, t: f32) {
        let mut pool = pool.borrow_mut();
        while pool.len() < self.count as usize {
            pool.push(DropLive::new());
        }
        for (i, d) in pool.iter().take(self.count as usize).enumerate() {
            let (streak, splash, fade) = self.drop_at(i as u32, t);
            d.streak.set(streak);
            d.alpha.set(self.alpha);
            d.splash.set(splash);
            // A ring lands and goes at once; between, its fade springs as a border colour does.
            let a = self.alpha * fade;
            if a == 0.0 || d.splash_alpha.get() == 0.0 {
                d.splash_alpha.snap(a);
            } else {
                d.splash_alpha.set(a);
            }
        }
    }

    /// Every drop mounted once, bound to `pool`, which later frames move.
    fn draw(&self, pool: &RefCell<Vec<DropLive>>, t: f32, out: &mut Vec<El>) {
        self.publish(pool, t);
        let pool = pool.borrow();
        for (i, d) in pool.iter().take(self.count as usize).enumerate() {
            let i = i as u32;
            let (streak, splash, _) = self.drop_at(i, t);
            let ink = drop_color(i, self.lane(i));
            out.push(
                el().key(format!("{}drop-{i}", self.key))
                    .size(streak[2], streak[3])
                    .radius(self.width / 2.0)
                    .gradient(ink.alpha(0.0), ink)
                    .absolute(streak[0], streak[1])
                    .live_rect(&d.streak)
                    .live_opacity(&d.alpha)
                    .a11y_hidden(),
            );
            out.push(
                el().key(format!("{}splash-{i}", self.key))
                    .size(splash[2], splash[3])
                    .radius(SPLASH_ROUND)
                    .border(1.5, ink)
                    .absolute(splash[0], splash[1])
                    .live_rect(&d.splash)
                    .live_opacity(d.splash_alpha.smooth(COLOUR_SPRING))
                    .a11y_hidden(),
            );
        }
    }
}

const LOADER: Shower = Shower {
    key: "loader-",
    left: 34.0 + 12.0,
    top: 40.0,
    w: 132.0 - 24.0,
    fall: 66.0,
    count: 14,
    speed: 70.0,
    width: 3.5,
    alpha: 0.95,
};

/// The loader: one small cloud raining red and green into a puddle, over the loading message.
pub(super) fn loader(s: &App, msg: &str, look: &Look) -> El {
    let k = &look.kit;
    let m = &s.rest.motion;
    let calm = s.rest.calm.get();
    let t = s.rest.clock;
    let (w, h) = (200.0, 120.0);
    let (cx, cw) = (34.0, 132.0);
    let puff = |d: f32, x: f32, y: f32, ink: Color| el().size(d, d).radius(d / 2.0).fill(ink).absolute(x, y);
    let mut drops = Vec::new();
    if !calm {
        LOADER.draw(&m.loader_drops, t, &mut drops);
    }
    let bob = if calm { 0.0 } else { bob(t) };
    m.bob.set([0.0, bob]);
    let (back, front) = (brand::cloud_back(), brand::cloud_front());
    let scene = el()
        .key("loader-rain")
        .size(w, h)
        // A rebuild redraws the weather from the clock; springing a drop from where it was
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
                .absolute(0.0, 0.0)
                .live_offset(&m.bob)
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

/// The flood at the clock, in a window `w` by `h`.
struct Flood {
    secs: f32,
    drain: f32,
    gather: f32,
    fade: f32,
    cloud_y: f32,
    cloud_bottom: f32,
    surface: f32,
    water_h: f32,
    t: f32,
    calm: bool,
    w: f32,
    h: f32,
}

const DEPTH: f32 = 70.0;

impl Flood {
    /// `None` when clear.
    fn at(s: &App, [w, h]: [f32; 2]) -> Option<Flood> {
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
        let secs = since.as_secs_f32();
        // The cloud slides down over the first 2.5 s and lifts away as the water drains.
        let gather = 1.0 - (1.0 - (secs / 2.5).clamp(0.0, 1.0)).powi(3);
        let cloud_y = -(DEPTH + 70.0) * (1.0 - gather) - (DEPTH + 90.0) * drain;
        let water_h = level * h * 0.55;
        Some(Flood {
            secs,
            drain,
            gather,
            fade: 1.0 - drain,
            cloud_y,
            cloud_bottom: cloud_y + DEPTH + 14.0,
            surface: h - water_h,
            water_h,
            t: if calm { 0.0 } else { s.rest.clock },
            calm,
            w,
            h,
        })
    }

    /// Rain from the cloud's underside once it is in, onto the water (or the window's floor).
    fn shower(&self) -> Option<Shower> {
        (!self.calm && self.gather > 0.98 && self.drain < 0.4).then(|| Shower {
            key: "flood-",
            left: 0.0,
            top: self.cloud_bottom - 10.0,
            w: self.w,
            fall: self.surface - self.cloud_bottom + 10.0,
            count: (self.w / 18.0) as u32,
            speed: 170.0,
            width: 3.5,
            alpha: self.fade,
        })
    }

    /// The card's left and top as it floats on the water; `None` once it has gone.
    fn card_at(&self) -> Option<[f32; 2]> {
        (self.drain < 0.3).then(|| {
            let float = (self.t * 1.5).sin() * 4.0;
            let top = (self.h * 0.42 - 90.0).min(self.surface - 220.0).max(self.cloud_bottom + 40.0) + float;
            [((self.w - CARD_W) / 2.0).max(0.0), top]
        })
    }

    fn card_alpha(&self) -> f32 {
        let shown = ((self.secs - 1.5) / 1.0).clamp(0.0, 1.0) * self.fade;
        if shown < 0.999 { shown } else { 1.0 }
    }

    fn strike(&self, card: Option<[f32; 4]>) -> Option<Strike> {
        (!self.calm && self.gather > 0.98 && self.drain < 0.2)
            .then(|| strike(self.t, self.cloud_bottom, self.w, card))
            .flatten()
    }

    fn built(&self) -> Built {
        Built {
            size: [self.w, self.h],
            drops: self.shower().is_some(),
            water: self.water_h > 1.0,
            card: self.card_at().is_some(),
        }
    }

    fn publish(&self, m: &Motion) {
        m.sky.set(brand::sky_top().alpha(0.45 * self.gather * self.fade));
        m.cloud.set([0.0, self.cloud_y]);
        publish_billows(&m.billows, self.w, self.t);
        m.water.set([0.0, self.surface]);
        if let Some([_, top]) = self.card_at() {
            m.card.set([0.0, top]);
        }
        m.card_alpha.set(self.card_alpha());
        if let Some(shower) = self.shower() {
            shower.publish(&m.flood_drops, self.t);
        }
    }
}

/// The flood over the whole window while it rains or drains; `None` when clear.
pub(super) fn flood(s: &App, look: &Look) -> Option<El> {
    let m = &s.rest.motion;
    let size = rune::memo::layout_size("app").unwrap_or([1440.0, 900.0]);
    let Some(f) = Flood::at(s, size) else {
        m.built.set(None);
        return None;
    };
    m.built.set(Some(f.built()));
    f.publish(m);
    let Flood {
        w,
        h,
        t,
        cloud_y,
        cloud_bottom,
        ..
    } = f;

    let mut sky = el()
        .key("flood")
        .size(w, h)
        .instant_changes()
        .fill(brand::sky_top().alpha(0.45 * f.gather * f.fade))
        // Colours spring under `instant_changes`, as the sky's did when each frame rebuilt it.
        .live_fill(m.sky.smooth(COLOUR_SPRING))
        .clip_rounded(0.0);
    // Draining, it lets clicks through to the app as the water falls away.
    if matches!(s.rest.weather, Weather::Flood(_)) {
        sky = sky.on_press(|_: &mut App| {});
    }

    if let Some(shower) = f.shower() {
        let mut drops = Vec::new();
        shower.draw(&m.flood_drops, t, &mut drops);
        sky = sky.children(drops);
    }

    // The water: a deep-sky body rising from the bottom, its surface a row of humps that meet it
    // edge to edge, so the translucent water never doubles up. It rises as one, from its surface.
    if f.water_h > 1.0 {
        let top_ink = brand::sky_top().alpha(0.9);
        let lip = 6.0;
        let mut sea = el()
            .key("flood-water")
            .size(w, h + lip)
            .absolute(0.0, 0.0)
            .live_offset(&m.water)
            .a11y_hidden()
            .child(el().size(w, h).fill(top_ink).absolute(0.0, lip));
        let hump = 60.0;
        for i in 0..(w / hump).ceil() as u32 {
            let hh = hump * 0.22 + lip;
            sea = sea.child(
                el().key(format!("flood-hump-{i}"))
                    .size(hump, hh)
                    .corners([hump * 0.5, hump * 0.5, 0.0, 0.0])
                    .fill(top_ink)
                    .absolute(i as f32 * hump, lip - hh),
            );
        }
        sky = sky.child(sea);
    }

    // The card floats on the water; lightning may strike it.
    let card_at = f.card_at().map(|[x, top]| {
        let ch = rune::memo::layout_size("break-card").map_or(170.0, |sz| sz[1]);
        [x, top, CARD_W, ch]
    });
    let bolt = f.strike(card_at);

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

    sky = sky.child(
        cloud_band(w, DEPTH, t, 1.0, &m.billows)
            .key("flood-cloud")
            .absolute(0.0, 0.0)
            .live_offset(&m.cloud),
    );

    if let Some(b) = &bolt {
        // The cloud glows from inside where the bolt leaves it.
        let gh = DEPTH + 140.0;
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
        let light = hit.map(|b| (b.bright, b.core, b.glow));
        sky = sky.child(
            card(s, &look.kit, look, light)
                .absolute(cx, 0.0)
                .live_offset(&m.card)
                .live_opacity(&m.card_alpha),
        );
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
            .child(primary("break-taken", "I took a break", k).on_activate_tx(|s: &mut App, _: &mut Tx| s.rest_taken()))
            .child(el().grow(1.0).a11y_hidden())
            .child(
                outline("break-snooze", format!("Snooze {}", mins(snooze)), Some("Esc"), 28.0, k)
                    .on_activate_tx(|s: &mut App, _: &mut Tx| s.rest_snooze()),
            ),
    )
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use rune::Runtime;

    use super::*;
    use crate::tests::{app, runtime, story};

    /// A clock with no lightning for the next 0.3 s.
    fn clear_sky() -> f32 {
        (0..400)
            .map(|k| 20.0 + k as f32 * 0.1)
            .find(|&t| (0..4).all(|j| strike(t + j as f32 * 0.1, 0.0, 1600.0, None).is_none()))
            .unwrap()
    }

    fn raining(t: f32) -> Runtime<App> {
        let mut a = app(story(), false);
        a.rest.weather = Weather::Flood(Instant::now().checked_sub(Duration::from_secs(12)).unwrap());
        a.rest.clock = t;
        let mut rt = runtime(a);
        rt.frame(1.0 / 60.0);
        rt
    }

    fn streaks(s: &App) -> Vec<[f32; 4]> {
        s.rest.motion.flood_drops.borrow().iter().map(|d| d.streak.get()).collect()
    }

    #[test]
    fn rain_falls_without_a_new_view() {
        let t = clear_sky();
        let mut rt = raining(t);
        assert!(rt.state.rest.motion.built.get().is_some());
        let before = streaks(&rt.state);
        assert!(!before.is_empty());
        rt.state.rest.clock = t + 0.2;
        assert!(!moved(&rt.state), "a frame of rain keeps the view");
        assert_ne!(streaks(&rt.state), before, "the drops fell");
    }

    #[test]
    fn lightning_or_a_missing_flood_asks_for_a_view() {
        let bolt = (0..400)
            .map(|k| 20.0 + k as f32 * 0.05)
            .find(|&t| strike(t, 0.0, 1600.0, None).is_some())
            .unwrap();
        let mut rt = raining(clear_sky());
        rt.state.rest.clock = bolt;
        assert!(moved(&rt.state), "the view draws each bolt");
        rt.state.rest.clock = clear_sky();
        rt.state.rest.motion.built.set(None);
        assert!(moved(&rt.state), "a flood nothing has mounted yet");
    }
}
