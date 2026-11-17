//! The poster look: sky, sun or moon, city, sea and palms, drawn row by row.
//!
//! Every row is a handful of constant-colour runs (`fill`) plus a few small
//! overlays (stars, windows, lamps). Nothing here looks at single pixels in a
//! loop. All colours are worked out once per frame in `Scene::new`, so
//! `render` only reads them. The night haze works the same way: rows near the
//! ground use a hazier copy of the building colours.

use crate::art::{self, Building, Palm, Rgb, Top, HORIZON, SKY_BANDS};
use crate::events::Kind;
use crate::sky::Palette;
use app_sdk::rgb565;

const W: i32 = 480;
const H: i32 = 480;
const CX: i32 = 240;
/// How many steps of haze there are, from none to the thickest.
const HAZE_LEVELS: usize = 8;
/// How many tower lights leave a glint on the water.
const GLINTS: usize = art::STAIRS.len() + 1 + art::BEACONS.len();
/// How many strips the mirror on the water has.
const MIRRORS: usize = art::FRESNEL.len();
/// How many palms there are.
pub const PALMS: usize = art::PALMS.len();

/// Per-layer offsets and animation phases. Motion and tilt fill these in
/// later; the still poster uses `View::STILL`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct View {
    /// Sun or moon (x, y) offset.
    pub orb: (i16, i16),
    /// Cloud x offset.
    pub clouds: i16,
    /// Far buildings x offset.
    pub far: i16,
    /// Main skyline (x, y) offset.
    pub city: (i16, i16),
    /// Palm (x, y) offset.
    pub palms: (i16, i16),
    /// Sway of each palm at its crown, in pixels.
    pub sway: [i16; PALMS],
    /// Wave phase, in rows.
    pub wave: u16,
    /// Star twinkle time slot.
    pub twinkle: u32,
    /// Milliseconds clock for the shimmer of light on the water.
    pub shimmer: u32,
    /// The event running now: its kind, progress 0 to 1000, and a random
    /// number picked when it started (direction, height).
    pub event: Option<(Kind, u16, u32)>,
    /// The plane's blinking light is on.
    pub blink: bool,
}

impl View {
    pub const STILL: View = View {
        orb: (0, 0),
        clouds: 0,
        far: 0,
        city: (0, 0),
        palms: (0, 0),
        sway: [0; PALMS],
        wave: 0,
        twinkle: 0,
        shimmer: 0,
        event: None,
        blink: false,
    };
}

/// Every colour the poster needs this frame, in RGB565.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scene {
    ink: Inks,
    /// The same colours as mirrored by the water, one set per strip.
    mirror: [Inks; MIRRORS],
    /// The small time: a dim tint of the sun, or sodium amber at night.
    pub clock: u16,
}

/// The colours for one way of seeing the scene: directly, or in the water.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Inks {
    sky: [u16; SKY_BANDS],
    /// Glow rings round the orb for each sky band: (outer, inner).
    halo: [(u16, u16); SKY_BANDS],
    /// Building colours at each haze level, from none to the thickest.
    tones: [[u16; 3]; HAZE_LEVELS],
    far: [u16; HAZE_LEVELS],
    shade: [u16; 3],
    glass: [u16; 3],
    road: u16,
    sea: [u16; 3],
    wave: [u16; 3],
    lights_on: bool,
    amber: u16,
    amber_dim: u16,
    /// The lit crown: (body, lines).
    crown: (u16, u16),
    /// A roof beacon: (core, halo).
    beacon: (u16, u16),
    /// Each light's glint on the water, per sea band, from strong to faint.
    glint: [[[u16; 3]; 3]; GLINTS],
    refl: [u16; 3],
    refl_hw: i16,
    sun: [u16; 5],
    moon_c: u16,
    moon_low: u16,
    cloud: u16,
    palm: u16,
    clock: u16,
    tail: u16,
    trail: u16,
    tail_mid: u16,
    tail_sea: u16,
    head: u16,
    plane: u16,
    plane_red: u16,
    plane_white: u16,
    stars: [u16; 3],
    orb_y: i16,
    orb_r: i16,
    moon: bool,
    night: u8,
}

const fn c(rgb: Rgb) -> u16 {
    rgb565(rgb.0, rgb.1, rgb.2)
}

/// `a` blended toward `b` by `k` out of 255.
const fn mix(a: Rgb, b: Rgb, k: u8) -> Rgb {
    const fn m(a: u8, b: u8, k: u8) -> u8 {
        ((a as u32 * (255 - k as u32) + b as u32 * k as u32 + 127) / 255) as u8
    }
    (m(a.0, b.0, k), m(a.1, b.1, k), m(a.2, b.2, k))
}

const fn scale(a: u8, b: u8) -> u8 {
    ((a as u32 * b as u32 + 127) / 255) as u8
}

impl Scene {
    pub const fn new(p: &Palette, v: &View) -> Scene {
        let ink = Inks::new(p, v);
        let mut mirror = [ink; MIRRORS];
        let mut l = 0;
        while l < MIRRORS {
            mirror[l] = ink.mirrored(p.sea[art::MIRROR_SEA[l]], art::FRESNEL[l]);
            l += 1;
        }
        Scene { ink, mirror, clock: ink.clock }
    }
}

/// An RGB565 colour back to (red, green, blue).
const fn rgb(x: u16) -> Rgb {
    let r = (x >> 11) as u8;
    let g = ((x >> 5) & 63) as u8;
    let b = (x & 31) as u8;
    ((r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2))
}

/// `x` seen in water of colour `w` that lets `k` out of 255 of itself show.
/// Mirrored colours too dark to see turn fully black, so the night water
/// stays black (and the screen saves power).
const fn wet(x: u16, w: Rgb, k: u8) -> u16 {
    let (r, g, b) = mix(rgb(x), w, k);
    if r < 12 && g < 12 && b < 12 {
        0
    } else {
        c((r, g, b))
    }
}

const fn wet2(x: (u16, u16), w: Rgb, k: u8) -> (u16, u16) {
    (wet(x.0, w, k), wet(x.1, w, k))
}

/// The light every tower light throws on the water: (column, width,
/// colour, rows down). A light at some height mirrors that far below the
/// horizon, so its glint runs from the horizon down to there.
const fn glints() -> [(i16, i16, Rgb, i16); GLINTS] {
    let mut out = [(0, 0, (0, 0, 0), 0); GLINTS];
    let mut n = 0;
    while n < art::STAIRS.len() {
        let st = &art::STAIRS[n];
        let b = &art::BUILDINGS[st.building];
        out[n] = (b.x + st.x, 1, art::AMBER, b.h - 6);
        n += 1;
    }
    let b = &art::BUILDINGS[art::CROWN];
    out[n] = (b.x + b.w / 2 - 2, 4, art::TEAL, b.h + 21);
    n += 1;
    let mut i = 0;
    while i < art::BEACONS.len() {
        let b = &art::BUILDINGS[art::BEACONS[i]];
        out[n] = (beacon_x(b) as i16 - 1, 2, art::TEAL, top_height(b) as i16);
        n += 1;
        i += 1;
    }
    out
}

const GLINT_AT: [(i16, i16, Rgb, i16); GLINTS] = glints();

/// The column at the middle of a tower's beacon.
const fn beacon_x(b: &Building) -> i32 {
    b.x as i32 + b.w as i32 / 2 + if matches!(b.top, Top::Mast) { b.w as i32 / 5 } else { 0 }
}

impl Inks {
    /// These colours as seen in the water.
    const fn mirrored(&self, w: Rgb, k: u8) -> Inks {
        let mut m = *self;
        let mut i = 0;
        while i < SKY_BANDS {
            m.sky[i] = wet(self.sky[i], w, k);
            m.halo[i] = wet2(self.halo[i], w, k);
            i += 1;
        }
        let mut l = 0;
        while l < HAZE_LEVELS {
            m.far[l] = wet(self.far[l], w, k);
            let mut i = 0;
            while i < 3 {
                m.tones[l][i] = wet(self.tones[l][i], w, k);
                i += 1;
            }
            l += 1;
        }
        let mut i = 0;
        while i < 3 {
            m.shade[i] = wet(self.shade[i], w, k);
            m.glass[i] = wet(self.glass[i], w, k);
            i += 1;
        }
        let mut i = 0;
        while i < 5 {
            m.sun[i] = wet(self.sun[i], w, k);
            i += 1;
        }
        m.road = wet(self.road, w, k);
        m.amber = wet(self.amber, w, k);
        m.amber_dim = wet(self.amber_dim, w, k);
        m.crown = wet2(self.crown, w, k);
        m.beacon = wet2(self.beacon, w, k);
        m.moon_c = wet(self.moon_c, w, k);
        m.moon_low = wet(self.moon_low, w, k);
        m.cloud = wet(self.cloud, w, k);
        m
    }

    const fn new(p: &Palette, v: &View) -> Inks {
        let moon = p.moon >= 128;
        let night = p.night;
        let glow_src = if moon { p.orb_low } else { mix(p.orb, p.orb_low, 128) };
        let (outer, inner) = if moon { (34, 34) } else { (26, 58) };

        let mut sky = [0; SKY_BANDS];
        let mut halo = [(0, 0); SKY_BANDS];
        let mut i = 0;
        while i < SKY_BANDS {
            sky[i] = c(p.sky[i]);
            halo[i] = (c(mix(p.sky[i], glow_src, outer)), c(mix(p.sky[i], glow_src, inner)));
            i += 1;
        }

        // The haze comes in with the night and fades the feet of the
        // buildings into the glow.
        let mut tones = [[0; 3]; HAZE_LEVELS];
        let mut far = [0; HAZE_LEVELS];
        let mut l = 0;
        while l < HAZE_LEVELS {
            let k = scale((l * 255 / (HAZE_LEVELS - 1)) as u8, night);
            far[l] = c(mix(p.far, art::HAZE_FAR.0, k));
            let mut i = 0;
            while i < 3 {
                tones[l][i] = c(mix(p.tones[i], art::HAZE_NEAR.0, k));
                i += 1;
            }
            l += 1;
        }

        // The orb's light on the water fades out as it sinks below the horizon.
        let top = p.orb_y as i32 - p.orb_r as i32;
        let mut vis = HORIZON as i32 + 20 - top;
        if vis < 0 {
            vis = 0;
        } else if vis > 60 {
            vis = 60;
        }
        let vis = (vis * 255 / 60) as u8;
        let refl_src = if moon { p.orb } else { mix(p.orb, p.orb_low, 70) };
        let refl_k = [200, 150, 100];

        let mut refl = [0; 3];
        let mut shade = [0; 3];
        let mut glass = [0; 3];
        let mut sea = [0; 3];
        let mut wave = [0; 3];
        let mut i = 0;
        while i < 3 {
            refl[i] = c(mix(p.sea[i], refl_src, scale(refl_k[i], vis)));
            shade[i] = c(mix(p.tones[i], p.skyline, 70));
            let day_glass = mix(p.tones[i], p.sky[1], 150);
            glass[i] = c(mix(day_glass, p.tones[i], p.night));
            sea[i] = c(p.sea[i]);
            wave[i] = c(mix(p.sea[i], p.sky[SKY_BANDS - 1], 64));
            i += 1;
        }

        // The teal lights stutter during a flicker.
        let flick = match v.event {
            Some((Kind::CrownFlicker, prog, _)) => art::FLICKER[art::FLICKER.len() * prog as usize / 1001],
            _ => 255,
        };
        let teal = scale(night, flick);
        let crown_bg = p.tones[(art::BUILDINGS[art::CROWN].tone as usize) % 3];

        let mut glint = [[[0; 3]; 3]; GLINTS];
        let mut i = 0;
        while i < GLINTS {
            let (_, _, colour, _) = GLINT_AT[i];
            let mut b = 0;
            while b < 3 {
                let mut j = 0;
                while j < 3 {
                    glint[i][b][j] = c(mix(p.sea[b], colour, scale(art::GLINT[j], night)));
                    j += 1;
                }
                b += 1;
            }
            i += 1;
        }

        let mut sun = [0; 5];
        let mut k = 0;
        while k < 5 {
            sun[k] = c(mix(p.orb, p.orb_low, (k * 255 / 4) as u8));
            k += 1;
        }

        let star_bg = p.sky[1];
        Inks {
            sky,
            halo,
            tones,
            far,
            shade,
            glass,
            road: c(p.skyline),
            sea,
            wave,
            lights_on: night >= 24,
            amber: c(mix(p.tones[0], art::AMBER, night)),
            amber_dim: c(mix(p.tones[0], art::AMBER_DIM, night)),
            crown: (c(mix(crown_bg, art::TEAL_DIM, teal)), c(mix(crown_bg, art::TEAL, teal))),
            beacon: (c(mix(p.sky[3], art::TEAL, teal)), c(mix(p.sky[3], art::TEAL_HALO, teal))),
            glint,
            refl,
            refl_hw: if moon { p.orb_r * 55 / 100 } else { p.orb_r * 3 / 4 },
            sun,
            moon_c: c(p.orb),
            moon_low: c(p.orb_low),
            cloud: c(p.cloud),
            palm: c(p.palm),
            clock: c(mix(p.sea[1], mix(mix(p.orb, p.orb_low, 60), art::CLOCK_NIGHT, night), art::CLOCK_STRENGTH)),
            tail: c(art::TAIL_LIGHT),
            trail: c(mix(p.skyline, art::TAIL_LIGHT, 90)),
            tail_mid: c(mix(p.skyline, art::TAIL_LIGHT, 170)),
            tail_sea: c(mix(p.sea[0], art::TAIL_LIGHT, 80)),
            head: c(mix(p.skyline, art::HEAD_LIGHT, scale(night, 255) / 2 + 128)),
            plane: c(mix(p.sky[1], (40, 36, 70), 150)),
            plane_red: c(art::PLANE_LIGHT),
            plane_white: c(mix(p.sky[1], (255, 255, 255), scale(night, 220))),
            stars: [
                c(mix(star_bg, (120, 110, 170), night)),
                c(mix(star_bg, (190, 180, 235), night)),
                c(mix(star_bg, (255, 250, 255), night)),
            ],
            orb_y: p.orb_y,
            orb_r: p.orb_r,
            moon,
            night,
        }
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod count {
    use std::cell::Cell;
    std::thread_local! {
        pub static FILLS: Cell<u32> = const { Cell::new(0) };
        pub static PIXELS: Cell<u32> = const { Cell::new(0) };
    }
    pub fn reset() {
        FILLS.with(|c| c.set(0));
        PIXELS.with(|c| c.set(0));
    }
    pub fn get() -> (u32, u32) {
        (FILLS.with(|c| c.get()), PIXELS.with(|c| c.get()))
    }
}

/// Fills columns `x0..x1` of one row, clipped to the screen.
#[inline(always)]
fn fill(row: &mut [u16], x0: i32, x1: i32, colour: u16) {
    let a = x0.clamp(0, W) as usize;
    let b = x1.clamp(0, W) as usize;
    if a < b {
        row[a..b].fill(colour);
        #[cfg(test)]
        count::FILLS.with(|c| c.set(c.get() + 1));
    }
}

/// A small overlay of `w` pixels (a star, a window, a lamp).
#[inline(always)]
fn dot(row: &mut [u16], x: i32, w: i32, colour: u16) {
    let a = x.clamp(0, W) as usize;
    let b = (x + w).clamp(0, W) as usize;
    if a < b {
        row[a..b].fill(colour);
        #[cfg(test)]
        count::PIXELS.with(|c| c.set(c.get() + (b - a) as u32));
    }
}

#[inline(always)]
fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// Integer square root.
const fn isqrt(n: i32) -> i32 {
    if n <= 0 {
        return 0;
    }
    let mut x = n as u32;
    let mut r = 0u32;
    let mut bit = 1u32 << 30;
    while bit > x {
        bit >>= 2;
    }
    while bit != 0 {
        if x >= r + bit {
            x -= r + bit;
            r = (r >> 1) + bit;
        } else {
            r >>= 1;
        }
        bit >>= 2;
    }
    r as i32
}

/// Half width of a disc of radius `r`, `dy` rows from its centre.
#[inline(always)]
fn half_width(r: i32, dy: i32) -> Option<i32> {
    if dy.abs() > r {
        None
    } else {
        Some(isqrt(r * r + r - dy * dy))
    }
}

/// Which band a row falls in, with the stripe fade above each band edge.
#[inline(always)]
fn band(edges: &[i16], y: i32) -> usize {
    let n = edges.len() - 1;
    let mut i = 0;
    while i + 1 < n && y >= edges[i + 1] as i32 {
        i += 1;
    }
    if i + 1 < n {
        let d = edges[i + 1] as i32 - y;
        if d >= 1 && d as usize <= art::BAND_FADE.len() && art::BAND_FADE[d as usize - 1] {
            return i + 1;
        }
    }
    i
}

// ---------------------------------------------------------------------------
// Buildings
// ---------------------------------------------------------------------------

/// The columns a building covers at `hh` rows above the horizon.
const fn span(b: &Building, hh: i32) -> Option<(i32, i32)> {
    let (x, w, h) = (b.x as i32, b.w as i32, b.h as i32);
    if hh < 0 {
        return None;
    }
    if hh < h {
        return Some((x, x + w));
    }
    let up = hh - h;
    match b.top {
        Top::Flat => None,
        Top::Steps => {
            if up < 10 {
                Some((x + w / 6, x + w - w / 6))
            } else if up < 18 {
                Some((x + w / 3, x + w - w / 3))
            } else {
                None
            }
        }
        Top::Deco => {
            let mid = x + w / 2;
            if up < 8 {
                Some((x + w / 8, x + w - w / 8))
            } else if up < 15 {
                Some((x + w / 4, x + w - w / 4))
            } else if up < 21 {
                Some((x + w * 3 / 8, x + w - w * 3 / 8))
            } else if up < 46 {
                Some((mid - 1, mid + 1))
            } else {
                None
            }
        }
        Top::Mast => {
            let mid = x + w / 2 + w / 5;
            if up < 4 {
                Some((mid - 3, mid + 3))
            } else if up < 30 {
                Some((mid - 1, mid))
            } else {
                None
            }
        }
        Top::Dome => {
            let r = w / 2;
            if up < r {
                let hw = isqrt(r * r - up * up);
                Some((x + r - hw, x + r + hw))
            } else {
                None
            }
        }
    }
}

const fn top_height(b: &Building) -> i32 {
    b.h as i32
        + match b.top {
            Top::Flat => 0,
            Top::Steps => 18,
            Top::Deco => 46,
            Top::Mast => 30,
            Top::Dome => b.w as i32 / 2,
        }
}

const fn skyline_table(list: &[Building]) -> [i16; 480] {
    let mut t = [0i16; 480];
    let mut i = 0;
    while i < list.len() {
        let b = &list[i];
        let mut hh = 0;
        let top = top_height(b);
        while hh < top {
            if let Some((a, z)) = span(b, hh) {
                let mut x = if a < 0 { 0 } else { a };
                while x < z && x < 480 {
                    if t[x as usize] < (hh + 1) as i16 {
                        t[x as usize] = (hh + 1) as i16;
                    }
                    x += 1;
                }
            }
            hh += 1;
        }
        i += 1;
    }
    t
}

/// The height of the main skyline above the horizon at each column.
pub const SKYLINE: [i16; 480] = skyline_table(art::BUILDINGS);

/// Which haze level a row `hh` rows above the ground uses, when the haze
/// reaches `rows` up. Odd rows round up, so the steps show as fine stripes.
#[inline(always)]
fn haze_level(hh: i32, rows: i16, y: i32) -> usize {
    let rows = rows as i32;
    if hh >= rows {
        return 0;
    }
    let k = (255 - hh.max(0) * 255 / rows) as usize;
    let lift = if y & 1 == 1 { 127 } else { 0 };
    ((k * (HAZE_LEVELS - 1) + lift) / 255).min(HAZE_LEVELS - 1)
}

fn draw_far(row: &mut [u16], s: &Inks, v: &View, y: i32) {
    let hh = HORIZON as i32 - 1 - y;
    let dx = v.far as i32;
    let colour = s.far[haze_level(hh, art::HAZE_FAR.1, y)];
    for b in art::FAR_BUILDINGS {
        if let Some((a, z)) = span(b, hh) {
            fill(row, a + dx, z + dx, colour);
        }
    }
}

/// A teal beacon on a tower's top, `d` rows above the top row.
fn draw_beacon(row: &mut [u16], s: &Inks, b: &Building, dx: i32, d: i32) {
    if !(-2..=2).contains(&d) {
        return;
    }
    let cx = beacon_x(b) + dx;
    if (-1..=1).contains(&d) {
        fill(row, cx - 2, cx + 2, s.beacon.1);
    } else {
        fill(row, cx - 1, cx + 1, s.beacon.1);
    }
    if (-1..=0).contains(&d) {
        dot(row, cx - 1, 2, s.beacon.0);
    }
}

fn draw_city(row: &mut [u16], s: &Inks, v: &View, y: i32) {
    let hh = HORIZON as i32 - 1 - (y - v.city.1 as i32);
    let dx = v.city.0 as i32;
    let py = art::WINDOW_PITCH.1 as i32;
    let wh = art::WINDOW_SIZE.1 as i32;
    let tones = &s.tones[haze_level(hh, art::HAZE_NEAR.1, y)];
    for (bi, b) in art::BUILDINGS.iter().enumerate() {
        if s.lights_on && art::BEACONS.contains(&bi) {
            draw_beacon(row, s, b, dx, hh - top_height(b));
        }
        let Some((a, z)) = span(b, hh) else { continue };
        let tone = (b.tone as usize).min(2);
        fill(row, a + dx, z + dx, tones[tone]);
        let h = b.h as i32;
        if hh >= h {
            // The lit crown: flat teal with three bright lines.
            if s.lights_on && bi == art::CROWN && hh < h + 21 {
                let up = hh - h;
                let colour = if up == 0 || up == 8 || up == 15 { s.crown.1 } else { s.crown.0 };
                fill(row, a + dx + 1, z + dx - 1, colour);
            }
            continue;
        }
        let x0 = b.x as i32 + dx;
        let w = b.w as i32;
        // A shaded right side gives the blocks some depth by day. At night
        // the towers are flat dark shapes.
        if s.night < 128 {
            fill(row, x0 + w - w / 4, x0 + w, s.shade[tone]);
        }
        let from_top = h - 1 - hh;
        if b.bands && s.night < 240 && from_top >= 6 && hh >= 7 && (from_top - 6) % py < wh {
            fill(row, x0 + 3, x0 + w - 3, s.glass[tone]);
        }
        if !s.lights_on {
            continue;
        }
        for st in art::STAIRS {
            if st.building != bi {
                continue;
            }
            for &(fy, l, r, bright) in st.floors {
                if from_top == fy as i32 {
                    fill(row, x0 + l as i32, x0 + w - r as i32, if bright { s.amber } else { s.amber_dim });
                }
            }
            // A dashed line up the stair core, from just under the roof
            // down to the street.
            let ry = HORIZON as i32 - 1 - hh;
            if from_top >= 6 && hh >= 8 && ry & 3 < 2 {
                let bright = hash((bi as u32) << 12 ^ (ry / 4) as u32).is_multiple_of(4);
                dot(row, x0 + st.x as i32, 1, if bright { s.amber } else { s.amber_dim });
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Sky
// ---------------------------------------------------------------------------

fn draw_stars(row: &mut [u16], s: &Inks, v: &View, y: i32) {
    if s.night < 90 || y >= art::STAR_ROWS as i32 {
        return;
    }
    // A star lives on a row, at a hashed column. Bright ones get a small
    // cross, so the rows above and below look at their neighbour's star.
    for dy in -1..=1 {
        let sy = y + dy;
        if sy < 0 {
            continue;
        }
        let h = hash(sy as u32 ^ 0x51ed_270b);
        if !h.is_multiple_of(art::STAR_EVERY) {
            continue;
        }
        let x = ((h >> 8) % W as u32) as i32;
        let level = ((h >> 20) % 8) as usize;
        let tw = hash(h ^ v.twinkle).is_multiple_of(16);
        let bright = level == 7 && !tw;
        if dy == 0 {
            let k = if tw { 0 } else { [0, 0, 0, 1, 1, 1, 2, 2][level] };
            dot(row, x, 1, s.stars[k]);
            if bright {
                dot(row, x - 1, 1, s.stars[1]);
                dot(row, x + 1, 1, s.stars[1]);
            }
        } else if bright {
            dot(row, x, 1, s.stars[1]);
        }
    }
}

fn draw_orb(row: &mut [u16], s: &Inks, v: &View, y: i32, b: usize) {
    let cx = CX + v.orb.0 as i32;
    let cy = s.orb_y as i32 + v.orb.1 as i32;
    let r = s.orb_r as i32;
    let dy = y - cy;
    if s.moon {
        // A crescent: a glow disc and the moon disc, then a shadow disc in
        // the sky colour cuts both, so the glow follows the lit edge.
        let Some(ghw) = half_width(r + 6, dy) else { return };
        fill(row, cx - ghw, cx + ghw, s.halo[b].0);
        if let Some(hw) = half_width(r, dy) {
            fill(row, cx - hw, cx + hw, if dy >= 8 { s.moon_low } else { s.moon_c });
        }
        let (sx, sy) = art::MOON_SHADOW;
        let scx = cx + r * sx as i32 / 256;
        let scy = cy - r * sy as i32 / 256;
        if let Some(shw) = half_width(r + 1, y - scy) {
            let a = (scx - shw).max(cx - ghw);
            let z = (scx + shw).min(cx + ghw);
            fill(row, a, z, s.sky[b]);
        }
        return;
    }
    if let Some(hw) = half_width(r + 20, dy) {
        fill(row, cx - hw, cx + hw, s.halo[b].0);
    }
    if let Some(hw) = half_width(r + 9, dy) {
        fill(row, cx - hw, cx + hw, s.halo[b].1);
    }
    let Some(hw) = half_width(r, dy) else { return };
    // Sun: hard colour bands from top to bottom, cut by slits below centre.
    if dy > 0 && r > art::SLITS_FROM_R as i32 {
        let grow = (r - art::SLITS_FROM_R as i32).min(52);
        for &(at, thick) in art::SUN_SLITS {
            let a = r * at as i32 / 256;
            let t = (r * thick as i32 / 256) * grow / 52;
            if dy >= a && dy < a + t {
                return;
            }
        }
    }
    let k = ((dy + r) * 5 / (2 * r + 1)).clamp(0, 4) as usize;
    fill(row, cx - hw, cx + hw, s.sun[k]);
}

fn draw_clouds(row: &mut [u16], s: &Inks, v: &View, y: i32) {
    for &(x, top, w) in art::CLOUDS {
        let d = y - top as i32;
        if !(0..6).contains(&d) {
            continue;
        }
        // Clouds drift right and wrap round off the edges.
        let wrap = crate::motion::CLOUD_WRAP as i32;
        let x = (x as i32 + v.clouds as i32 + 160).rem_euclid(wrap) - 160;
        let w = w as i32;
        let (a, z) = match d {
            0 => (x + w / 5, x + w * 3 / 5),
            1 | 2 => (x + w / 10, x + w * 9 / 10),
            3 => (x, x + w),
            _ => (x + w / 4, x + w * 3 / 4),
        };
        fill(row, a, z, s.cloud);
    }
}

fn draw_road(row: &mut [u16], s: &Inks, y: i32) {
    if HORIZON as i32 - y <= 3 {
        fill(row, 0, W, s.road);
    }
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Where an event is across the screen: -40 to 520 by progress, left to
/// right or right to left.
fn across(prog: u16, right: bool) -> i32 {
    let x = -40 + 560 * prog as i32 / 1000;
    if right {
        x
    } else {
        W - x
    }
}

fn draw_event(row: &mut [u16], s: &Inks, v: &View, y: i32) {
    let Some((kind, prog, salt)) = v.event else { return };
    let right = salt & 1 == 0;
    match kind {
        Kind::TailLights => {
            // A car on the coast road: headlight in front, tail light and a
            // fading red streak behind.
            let r = HORIZON as i32 - y;
            let x = across(prog, right) + v.city.0 as i32;
            let d = if right { 1 } else { -1 };
            let tail = x - d * 9;
            let span = |len: i32| (tail.min(tail - d * len), tail.max(tail - d * len));
            if r == 2 || r == 3 {
                dot(row, x - (d < 0) as i32, 2, s.head);
                let (a, z) = span(34);
                fill(row, a, z, s.trail);
                let (a, z) = span(12);
                fill(row, a, z, s.tail_mid);
                dot(row, tail - 1, 3, s.tail);
            } else if r == -3 || r == -7 {
                // Its red light on the water just below.
                let (a, z) = span(26);
                fill(row, a, z, s.tail_sea);
            }
        }
        Kind::Plane => {
            let y0 = 46 + (salt >> 1) as i32 % 60 - prog as i32 * 24 / 1000;
            let x = across(prog, right);
            let d = if right { 1 } else { -1 };
            // A small jet: fuselage, a tail fin at the back, wings below,
            // a steady white light at the nose and a blinking red one.
            match y - y0 {
                -2 => dot(row, x - d * 8 - 1, 1, s.plane),
                -1 => dot(row, x - d * 8 - 1, 2, s.plane),
                0 => {
                    fill(row, x - 9, x + 9, s.plane);
                    dot(row, x + d * 9 - (d < 0) as i32, 1, s.plane_white);
                }
                1 => {
                    fill(row, x - 3, x + 4, s.plane);
                    if v.blink {
                        dot(row, x, 2, s.plane_red);
                    }
                }
                _ => {}
            }
        }
        // The flicker is in the crown and beacon colours.
        Kind::CrownFlicker => {}
    }
}

// ---------------------------------------------------------------------------
// Sea
// ---------------------------------------------------------------------------

/// sin of a phase (256 steps to a full turn) times `amp`.
#[inline(always)]
fn sin(phase: u32, amp: i32) -> i32 {
    crate::motion::SIN[(phase & 255) as usize] as i32 * amp / 127
}

/// The water, `d` rows below the horizon.
///
/// Each row traces one ray from your eye down to the water and bounces it
/// back up. On flat water the bounced ray meets the scene exactly as far
/// above the horizon as the row is below it, so the row is drawn as that
/// mirrored sky row. Ripples tilt the water a little, which bends the ray up
/// or down and to the side; rows nearer to you are seen through bigger
/// ripples, so they bend more. Water seen at a low angle mirrors almost
/// everything, and water close to you mostly shows its own colour (the
/// Fresnel effect), so each strip uses colours blended with the water.
fn draw_mirror(row: &mut [u16], sc: &Scene, v: &View, y: i32) {
    // Every row in a group traces from the group's first row.
    let g = mirror_group(y);
    let d = if g < 18 { g * 3 } else { 54 + (g - 18) * 4 };
    let y = HORIZON as i32 + d;
    let t = v.shimmer;
    let du = d as u32;

    // Where the bounced ray lands.
    let up = 1 + d / art::RIPPLE_ROWS_PER_PX as i32;
    let bend = sin(du * 9 + t / 45, up) + sin(du.wrapping_mul(23).wrapping_sub(t / 31), up / 2);
    let ox = sin(du * 5 + t / 70 + 64, d / art::WOBBLE_ROWS_PER_PX as i32);
    let src = (HORIZON as i32 - 1 - (d + bend).max(0)).max(0);

    let m = &sc.mirror[band(&art::MIRROR_EDGES, y)];
    let mv = View {
        orb: ((v.orb.0 as i32 + ox) as i16, v.orb.1),
        clouds: (v.clouds as i32 + ox) as i16,
        far: (v.far as i32 + ox) as i16,
        city: ((v.city.0 as i32 + ox) as i16, v.city.1),
        event: None,
        ..*v
    };
    let b = band(&art::SKY_EDGES, src);
    fill(row, 0, W, m.sky[b]);
    draw_orb(row, m, &mv, src, b);
    draw_clouds(row, m, &mv, src);
    draw_far(row, m, &mv, src);
    draw_city(row, m, &mv, src);
    draw_road(row, m, src);
}

/// What lies on top of the mirror: wave lines, the path of the sun or moon,
/// and the glints of the tower lights.
fn draw_water(row: &mut [u16], s: &Inks, v: &View, y: i32) {
    let d = y - HORIZON as i32;
    let b = band(&art::SEA_EDGES, y);
    let ph = v.wave as i32;

    // Calm wave lines: a few short light dashes on some rows.
    let h = hash((y + ph) as u32 ^ 0xa531_1e2b);
    if d > 2 && h.is_multiple_of(5) {
        let len = 10 + (h >> 24) as i32 % (14 + d / 3);
        let x = ((h >> 6) % W as u32) as i32;
        fill(row, x, x + len, s.wave[b]);
        let x2 = (x + 150 + ((h >> 16) % 180) as i32) % W;
        fill(row, x2, x2 + len * 2 / 3, s.wave[b]);
    }

    // The broken path of light from the sun or moon. The moon's path sits
    // under the lit part of the crescent.
    let hw0 = s.refl_hw as i32;
    if s.refl[0] != s.sea[0] && hw0 > 0 && (y + ph) % 4 < 2 {
        let hw = (hw0 - d * hw0 / 230).max(4);
        let level = if d < 26 { 0 } else if d < 70 { 1 } else { 2 };
        let lit = if s.moon { s.orb_r as i32 * art::MOON_PATH_SHIFT as i32 / 256 } else { 0 };
        let cx = CX + v.orb.0 as i32 + lit;
        // Each row reshuffles its breaks at its own moment, so the light
        // shimmers instead of jumping all at once.
        let slot = (v.shimmer / 4).wrapping_add((y as u32).wrapping_mul(97)) / 160;
        let hr = hash((y as u32).wrapping_mul(0x2c1b_3c6d) ^ slot.wrapping_mul(0x85eb_ca6b));
        let shift = (hr % 9) as i32 - 4;
        let gap_at = cx - hw + (hr >> 8) as i32 % (2 * hw).max(1);
        let gap = 3 + (hr >> 20) as i32 % (hw / 2 + 1);
        fill(row, cx - hw + shift, gap_at, s.refl[level]);
        fill(row, gap_at + gap, cx + hw + shift, s.refl[level]);
    }

    // Each tower light glints on the water straight below it, from the
    // horizon down to where it mirrors.
    if !s.lights_on {
        return;
    }
    let slot = v.shimmer / 600;
    for (i, &(x, w, _, len)) in GLINT_AT.iter().enumerate() {
        let (x, w, len) = (x as i32 + v.city.0 as i32, w as i32, len as i32);
        if d < 2 || d >= len || (y + x) % 3 == 0 || hash((x as u32) << 16 ^ y as u32).is_multiple_of(4) {
            continue;
        }
        let k = if d < len / 3 { 0 } else if d < len * 2 / 3 { 1 } else { 2 };
        let shift = (hash((x as u32) << 16 ^ (y / 2) as u32 ^ slot.wrapping_mul(0x9e37_79b9)) % 3) as i32 - 1;
        fill(row, x + shift, x + shift + w, s.glint[i][b][k]);
    }
}

// ---------------------------------------------------------------------------
// Palms
// ---------------------------------------------------------------------------

/// Most separate runs one palm may have on one row.
const SPANS: usize = 16;

/// A palm as runs of columns on each screen row, made when the crate builds.
pub struct PalmRows {
    count: [u8; 480],
    runs: [[(i16, i16); SPANS]; 480],
}

/// Left edge of the scratch bitmap, in screen columns. Palms may hang a little
/// past the screen edges.
const BX0: i32 = -64;
const BWORDS: usize = 19;
const BW: i32 = BWORDS as i32 * 32;

struct Bits {
    w: [[u32; BWORDS]; 480],
}

const fn set_span(b: &mut Bits, x0: i32, x1: i32, y: i32) {
    if y < 0 || y >= H {
        return;
    }
    let mut x = if x0 - BX0 < 0 { 0 } else { x0 - BX0 };
    let end = if x1 - BX0 >= BW { BW - 1 } else { x1 - BX0 };
    while x <= end {
        b.w[y as usize][(x / 32) as usize] |= 1 << (x % 32);
        x += 1;
    }
}

/// A disc. Positions and radius are in 1/16 pixel.
const fn disc(b: &mut Bits, x: i32, y: i32, r: i32) {
    let (cx, cy) = ((x + 8) >> 4, (y + 8) >> 4);
    let rr = (r * r + 8 * r) / 256;
    let ri = isqrt(rr);
    let mut dy = -ri;
    while dy <= ri {
        let hw = isqrt(rr - dy * dy);
        set_span(b, cx - hw, cx + hw, cy + dy);
        dy += 1;
    }
}

/// A tapering stroke from (x0, y0) to (x1, y1), all in 1/16 pixel.
const fn stroke(b: &mut Bits, x0: i32, y0: i32, x1: i32, y1: i32, r0: i32, r1: i32) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let adx = if dx < 0 { -dx } else { dx };
    let ady = if dy < 0 { -dy } else { dy };
    let n = (if adx > ady { adx } else { ady }) / 16 + 1;
    let mut i = 0;
    while i <= n {
        disc(b, x0 + dx * i / n, y0 + dy * i / n, r0 + (r1 - r0) * i / n);
        i += 1;
    }
}

/// A point on a quadratic curve a-p-c at step i of n.
const fn quad(a: i32, p: i32, c: i32, i: i32, n: i32) -> i32 {
    let (a, p, c, i, n) = (a as i64, p as i64, c as i64, i as i64, n as i64);
    let j = n - i;
    ((a * j * j + 2 * p * j * i + c * i * i) / (n * n)) as i32
}

/// Integer square root for big numbers, used only when the crate builds.
const fn isqrt64(n: i64) -> i64 {
    if n <= 0 {
        return 0;
    }
    let mut x = n as u64;
    let mut r = 0u64;
    let mut bit = 1u64 << 62;
    while bit > x {
        bit >>= 2;
    }
    while bit != 0 {
        if x >= r + bit {
            x -= r + bit;
            r = (r >> 1) + bit;
        } else {
            r >>= 1;
        }
        bit >>= 2;
    }
    r as i64
}

/// Unit vector (x 256) along (dx, dy).
const fn unit(dx: i32, dy: i32) -> (i32, i32) {
    let (x, y) = (dx as i64, dy as i64);
    let len = isqrt64(x * x + y * y);
    if len == 0 {
        (0, 256)
    } else {
        ((x * 256 / len) as i32, (y * 256 / len) as i32)
    }
}

const fn draw_trunk(b: &mut Bits, p: &Palm) {
    let (bx, by) = (p.base.0 as i32 * 16, p.base.1 as i32 * 16);
    let (cx, cy) = (p.crown.0 as i32 * 16, p.crown.1 as i32 * 16);
    let (mx, my) = ((bx + cx) / 2 + p.bend as i32 * 16, (by + cy) / 2);
    let n = (p.base.1 as i32 - p.crown.1 as i32) * 2;
    let mut i = 0;
    while i <= n {
        let x = quad(bx, mx, cx, i, n);
        let y = quad(by, my, cy, i, n);
        // Direction of the trunk here, to widen the run where it leans.
        let ddx = (mx - bx) * (n - i) + (cx - mx) * i;
        let ddy = (my - by) * (n - i) + (cy - my) * i;
        let (_, uy) = unit(ddx, ddy);
        let auy = if uy < 0 { -uy } else { uy };
        let lean = if auy < 128 { 512 } else { 256 * 256 / auy };
        let w0 = p.width.0 as i32;
        let w1 = p.width.1 as i32;
        // Half width in 1/16 px, a little wider at the foot.
        let mut hw = (w0 * (n - i) + w1 * i) * 8 / n;
        if i < n / 12 {
            hw += (n / 12 - i) * 40 / (n / 12);
        }
        let row = (y + 8) >> 4;
        // Rings: every 13 rows the trunk pinches in by a pixel.
        if (p.base.1 as i32 - row).rem_euclid(13) < 2 {
            hw -= 16;
        }
        let hwx = hw * lean / 256;
        set_span(b, (x - hwx + 8) >> 4, (x + hwx + 8) >> 4, row);
        i += 1;
    }
}

const fn draw_frond(b: &mut Bits, p: &Palm, f: &art::Frond) {
    let (cx, cy) = (p.crown.0 as i32 * 16, p.crown.1 as i32 * 16);
    let tx = cx + f.tip.0 as i32 * 16;
    let ty = cy + f.tip.1 as i32 * 16;
    let px = cx + f.tip.0 as i32 * 8;
    let py = cy + f.arch as i32 * 16;
    let n = 60;
    let leaf = f.leaf as i32;
    let mut i = 0;
    while i <= n {
        let x = quad(cx, px, tx, i, n);
        let y = quad(cy, py, ty, i, n);
        // The rib: thick at the crown, a hair at the tip.
        disc(b, x, y, 30 - 22 * i / n);
        if i >= n / 8 && i < n && i % 3 == 0 {
            let (ux, uy) = unit((px - cx) * (n - i) + (tx - px) * i, (py - cy) * (n - i) + (ty - py) * i);
            // The normal on the lower side of the leaf.
            let (lx, ly) = if ux >= 0 { (-uy, ux) } else { (uy, -ux) };
            let (lx, ly) = if ly < 0 { (-lx, -ly) } else { (lx, ly) };
            // Leaflet length: short at the crown, longest about 40% of the
            // way out, short again at the tip.
            let s = i * 256 / n;
            let prof = 4 * s * (256 - s) / 256 + 30;
            let prof = if prof > 256 { 256 } else { prof };
            // A little variety so the leaf does not look like a comb.
            let jit = (i * 37 + f.tip.0 as i32 * 11 + f.tip.1 as i32 * 7).rem_euclid(17);
            let len = leaf * prof / 256 * (88 + jit * 2) / 100;
            // Lower leaflets hang toward the tip and down.
            let (vx, vy) = unit(ux * 70 + lx * 70, uy * 70 + ly * 70 + 256 * 60);
            stroke(b, x, y, x + vx * len / 16, y + vy * len / 16, 16, 4);
            // Upper leaflets are shorter and point forward and up.
            if i % 6 == 0 {
                let (wx, wy) = unit(ux * 90 - lx * 50, uy * 90 - ly * 50);
                let l2 = len / 2;
                stroke(b, x, y, x + wx * l2 / 16, y + wy * l2 / 16, 14, 4);
            }
        }
        i += 1;
    }
}

impl PalmRows {
    /// The first run on screen row `y`, if the palm has one there.
    pub fn first_run(&self, y: usize) -> Option<(i16, i16)> {
        (self.count[y] > 0).then(|| self.runs[y][0])
    }
}

const fn build_palm(p: &Palm) -> PalmRows {
    let mut bits = Bits { w: [[0; BWORDS]; 480] };
    draw_trunk(&mut bits, p);
    let mut k = 0;
    while k < p.fronds.len() {
        draw_frond(&mut bits, p, &p.fronds[k]);
        k += 1;
    }
    // Coconuts and the knot at the crown.
    let (cx, cy) = (p.crown.0 as i32 * 16, p.crown.1 as i32 * 16);
    disc(&mut bits, cx, cy, 104);
    disc(&mut bits, cx - 80, cy + 120, 80);
    disc(&mut bits, cx + 80, cy + 128, 80);
    disc(&mut bits, cx, cy + 200, 72);

    let mut out = PalmRows { count: [0; 480], runs: [[(0, 0); SPANS]; 480] };
    let mut y = 0;
    while y < 480 {
        let mut n = 0usize;
        let mut x = 0;
        let mut inside = false;
        let mut start = 0;
        while x <= BW {
            let on = x < BW && (bits.w[y][(x / 32) as usize] >> (x % 32)) & 1 == 1;
            if on && !inside {
                start = x;
                inside = true;
            } else if !on && inside {
                inside = false;
                let (a, z) = ((start + BX0) as i16, (x + BX0) as i16);
                if n == SPANS {
                    // Too many runs: join the two closest neighbours to
                    // make room, so the shape keeps its gaps where they
                    // are widest.
                    let mut best = 0;
                    let mut j = 1;
                    while j < SPANS {
                        let gap = out.runs[y][j].0 - out.runs[y][j - 1].1;
                        if gap < out.runs[y][best + 1].0 - out.runs[y][best].1 {
                            best = j - 1;
                        }
                        j += 1;
                    }
                    let last_gap = a - out.runs[y][SPANS - 1].1;
                    if last_gap <= out.runs[y][best + 1].0 - out.runs[y][best].1 {
                        out.runs[y][SPANS - 1].1 = z;
                    } else {
                        out.runs[y][best].1 = out.runs[y][best + 1].1;
                        let mut k = best + 1;
                        while k + 1 < SPANS {
                            out.runs[y][k] = out.runs[y][k + 1];
                            k += 1;
                        }
                        out.runs[y][SPANS - 1] = (a, z);
                    }
                } else {
                    out.runs[y][n] = (a, z);
                    n += 1;
                }
            }
            // Skip empty words quickly.
            if !inside && x % 32 == 0 && x < BW && bits.w[y][(x / 32) as usize] == 0 {
                x += 32;
            } else {
                x += 1;
            }
        }
        out.count[y] = n as u8;
        y += 1;
    }
    out
}

pub static PALM_ROWS: [PalmRows; art::PALMS.len()] = {
    let mut i = 0;
    let mut out = [const { PalmRows { count: [0; 480], runs: [[(0, 0); SPANS]; 480] } }; art::PALMS.len()];
    while i < art::PALMS.len() {
        out[i] = build_palm(&art::PALMS[i]);
        i += 1;
    }
    out
};

/// How far palm `pi` is shifted on palm row `sy`: the tilt offset plus the
/// sway, which shears the palm (none at the foot, all of it at the crown).
fn palm_dx(v: &View, pi: usize, sy: i32) -> i32 {
    let crown = art::PALMS[pi].crown.1 as i32;
    let lift = (H - sy).clamp(0, H - crown + 60);
    v.palms.0 as i32 + v.sway[pi] as i32 * lift / (H - crown + 60)
}

/// Which palm, if any, a finger at (x, y) is on, and the palm's middle
/// column at that height. Fingers are imprecise, so the palm counts as
/// about 20 px wider and taller than it is.
pub fn palm_at(v: &View, x: i32, y: i32) -> Option<(usize, i32)> {
    const SLACK: i32 = 20;
    for (pi, p) in PALM_ROWS.iter().enumerate() {
        let mut sy = y - v.palms.1 as i32 - SLACK;
        while sy <= y - v.palms.1 as i32 + SLACK {
            let r = sy.clamp(0, H - 1);
            let dx = palm_dx(v, pi, r);
            let n = p.count[r as usize] as usize;
            let runs = &p.runs[r as usize][..n];
            for &(a, z) in runs {
                if x >= a as i32 + dx - SLACK && x < z as i32 + dx + SLACK {
                    let (a0, z0) = (runs[0].0 as i32, runs[n - 1].1 as i32);
                    return Some((pi, (a0 + z0) / 2 + dx));
                }
            }
            sy += 4;
        }
    }
    None
}

fn draw_palms(row: &mut [u16], s: &Inks, v: &View, y: i32) {
    // Below the screen the trunks carry on, so repeat the last row.
    let sy = (y - v.palms.1 as i32).min(H - 1);
    if sy < 0 {
        return;
    }
    for (pi, p) in PALM_ROWS.iter().enumerate() {
        let dx = palm_dx(v, pi, sy);
        let n = p.count[sy as usize] as usize;
        for &(a, z) in &p.runs[sy as usize][..n] {
            let (a, z) = (a as i32 + dx, z as i32 + dx);
            fill(row, a, z, s.palm);
        }
    }
}

// ---------------------------------------------------------------------------
// The whole frame
// ---------------------------------------------------------------------------

/// Draws one row of the poster into `row` (480 pixels).
pub fn draw_row(row: &mut [u16], sc: &Scene, v: &View, y: i32) {
    let s = &sc.ink;
    if y < HORIZON as i32 {
        let b = band(&art::SKY_EDGES, y);
        fill(row, 0, W, s.sky[b]);
        draw_stars(row, s, v, y);
        draw_orb(row, s, v, y, b);
        draw_clouds(row, s, v, y);
        draw_far(row, s, v, y);
        draw_city(row, s, v, y);
        draw_road(row, s, y);
    } else {
        draw_mirror(row, sc, v, y);
        draw_water(row, s, v, y);
    }
    draw_event(row, s, v, y);
    draw_palms(row, s, v, y);
}

/// Which water rows share one traced mirror row: pairs near the horizon,
/// threes further down where the mirror is faint.
#[inline(always)]
fn mirror_group(y: i32) -> i32 {
    let d = y - HORIZON as i32;
    if d < 54 {
        d / 3
    } else {
        18 + (d - 54) / 4
    }
}

/// Renders rows `rows` into `buf`, 480 pixels per row. To save time, water
/// rows in the same `mirror_group` trace the mirror once and copy it.
pub fn render(s: &Scene, v: &View, rows: core::ops::Range<u16>, buf: &mut [u16]) {
    let mut traced = [0u16; W as usize];
    let mut group = -1;
    for (i, y) in rows.enumerate() {
        let row = &mut buf[i * W as usize..(i + 1) * W as usize];
        let y = y as i32;
        if y < HORIZON as i32 {
            draw_row(row, s, v, y);
            continue;
        }
        let g = mirror_group(y);
        if g == group {
            row.copy_from_slice(&traced);
        } else {
            draw_mirror(row, s, v, y);
            traced.copy_from_slice(row);
            group = g;
        }
        draw_water(row, &s.ink, v, y);
        draw_event(row, &s.ink, v, y);
        draw_palms(row, &s.ink, v, y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skyline_table() {
        let mut seen = std::vec::Vec::new();
        for &h in SKYLINE.iter() {
            assert!((0..=200).contains(&h), "height {h}");
            if !seen.contains(&h) {
                seen.push(h);
            }
        }
        assert!(seen.len() >= 6);
    }

    fn scene_at(h: u32, m: u32) -> Scene {
        Scene::new(&crate::sky::target((h * 60 + m) * 60), &View::STILL)
    }

    fn rows(s: &Scene, v: &View) -> std::vec::Vec<u16> {
        let mut buf = std::vec![0u16; 480 * 480];
        render(s, v, 0..480, &mut buf);
        buf
    }

    #[test]
    fn haze_only_at_night() {
        let noon = scene_at(13, 0);
        let (noon, night) = (noon.ink, scene_at(23, 0).ink);
        assert!(noon.tones.iter().all(|t| *t == noon.tones[0]));
        assert!(noon.far.iter().all(|f| *f == noon.far[0]));
        assert!(!noon.lights_on);
        assert_ne!(night.tones[HAZE_LEVELS - 1], night.tones[0]);
        assert!(night.lights_on);
        // Thickest at the feet, none high up.
        assert_eq!(haze_level(0, art::HAZE_NEAR.1, 0), HAZE_LEVELS - 1);
        assert_eq!(haze_level(art::HAZE_NEAR.1 as i32, art::HAZE_NEAR.1, 1), 0);
    }

    #[test]
    fn tower_lights_show_at_night_only() {
        for (h, m, lit) in [(23, 0, true), (13, 0, false)] {
            let s = scene_at(h, m);
            let mut off = s;
            off.ink.lights_on = false;
            for m in off.mirror.iter_mut() {
                m.lights_on = false;
            }
            let a = rows(&s, &View::STILL);
            let b = rows(&off, &View::STILL);
            assert_eq!(a != b, lit, "at {h}:{m:02}");
            // The crown stands out, and its light shows on the water.
            if lit {
                assert!(a.contains(&s.ink.crown.1));
                let sea = (HORIZON as usize + 4) * 480..(HORIZON as usize + 30) * 480;
                assert!(a[sea.clone()] != b[sea]);
            }
        }
    }

    #[test]
    fn water_mirrors_the_towers() {
        // At night the water under a tall dark tower is darker than the
        // water under the low hotels, where it mirrors the orange haze.
        let s = scene_at(23, 0);
        let f = rows(&s, &View::STILL);
        let lum = |x: usize, y: usize| {
            let (r, g, b) = rgb(f[y * 480 + x]);
            r as u32 + g as u32 + b as u32
        };
        let y = HORIZON as usize + 26;
        assert!(lum(90, y) < lum(300, y), "{} vs {}", lum(90, y), lum(300, y));
    }

    #[test]
    fn palms_have_trunks_and_crowns() {
        for (i, p) in art::PALMS.iter().enumerate() {
            let rows = &PALM_ROWS[i];
            // The crown row is covered, and the trunk reaches the bottom.
            assert!(rows.count[p.crown.1 as usize] > 0);
            assert!(rows.count[479] > 0);
        }
    }
}

