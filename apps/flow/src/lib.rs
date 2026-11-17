//! Flow field: particles with streaking tails riding slowly turning currents.
//! Each particle has momentum: a finger is a gravity well that pulls them in,
//! and once it lifts they coast on and ease back into the current. Tilt is a
//! wind; a tap changes the palette.
#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use app_sdk::{Input, Output, Tilt, Touch};
    use std::vec;

    fn run(app: &mut Flow, input: &mut Input, frames: u32) {
        for _ in 0..frames {
            input.time_ms = input.time_ms.wrapping_add(33);
            app.update(input, 33, &mut Output::default());
        }
    }

    fn mean_x(app: &Flow) -> i32 {
        app.particles.iter().map(|p| p.x / 16).sum::<i32>() / COUNT as i32
    }

    #[test]
    fn particles_stay_on_screen_and_move() {
        let mut app = Flow::new();
        let before = app.particles[0];
        run(&mut app, &mut Input::default(), 300);
        assert_ne!((before.x, before.y), (app.particles[0].x, app.particles[0].y));
        for p in &app.particles {
            assert!((0..480 * 16).contains(&p.x) && (0..480 * 16).contains(&p.y));
        }
    }

    #[test]
    fn touch_pulls_them_in() {
        let mut app = Flow::new();
        let mut input = Input { touch: Touch { down: true, x: 420, y: 240, tapped: false }, ..Input::default() };
        run(&mut app, &mut input, 60);
        assert!(mean_x(&app) > 280, "{}", mean_x(&app));
    }

    #[test]
    fn they_coast_after_the_finger_lifts() {
        let mut app = Flow::new();
        let mut input = Input { touch: Touch { down: true, x: 470, y: 240, tapped: false }, ..Input::default() };
        run(&mut app, &mut input, 15);
        input.touch.down = false;
        run(&mut app, &mut input, 1);
        // The ones still short of where the finger was keep heading there.
        let short: std::vec::Vec<i32> =
            app.particles.iter().filter(|p| p.x < 380 * 16).map(|p| p.vx as i32).collect();
        let vx = short.iter().sum::<i32>() / short.len() as i32;
        assert!(vx > 20, "{vx}");
        // And eased back into the current a few seconds later.
        run(&mut app, &mut input, 120);
        let vx: i32 = app.particles.iter().map(|p| p.vx as i32).sum::<i32>() / COUNT as i32;
        assert!(vx.abs() < SPEED, "{vx}");
    }

    #[test]
    fn tilt_blows_them_downhill() {
        let mut app = Flow::new();
        let mut input = Input { tilt: Tilt { x_mg: -800, y_mg: 0, z_mg: 600 }, ..Input::default() };
        let before = app.particles;
        run(&mut app, &mut input, 6);
        let left = before.iter().zip(&app.particles).filter(|(a, b)| b.x < a.x).count();
        assert!(left > COUNT * 3 / 4, "{left}");
    }

    #[test]
    fn tap_cycles_palette_with_a_click() {
        let mut app = Flow::new();
        let mut out = Output::default();
        let input = Input { touch: Touch { tapped: true, ..Touch::default() }, ..Input::default() };
        app.update(&input, 33, &mut out);
        assert_eq!(out.sound, Some(app_sdk::Sound::Click));
        assert_eq!(app.palette, 1);
    }

    #[test]
    fn extreme_inputs_do_not_panic() {
        let mut app = Flow::new();
        let mut input = Input {
            tilt: Tilt { x_mg: i16::MIN, y_mg: i16::MAX, z_mg: 0 },
            touch: Touch { down: true, x: u16::MAX, y: u16::MAX, tapped: false },
            time_ms: u32::MAX - 100,
            ..Input::default()
        };
        run(&mut app, &mut input, 50);
        app.update(&input, u32::MAX, &mut Output::default());
        app.render(0..480, &mut vec![0; 480 * 480]);
    }

    #[test]
    fn arbitrary_strips_match_full_frame() {
        let mut app = Flow::new();
        run(&mut app, &mut Input::default(), 20);
        let mut full = vec![0; 480 * 480];
        app.render(0..480, &mut full);
        assert!(full.iter().any(|p| *p != 0));
        for (start, end) in [(0, 12), (7, 31), (233, 246), (468, 480), (480, 480)] {
            let mut strip = vec![0xFFFF; (end - start) as usize * 480];
            app.render(start..end, &mut strip);
            assert_eq!(strip, full[start as usize * 480..end as usize * 480]);
        }
    }
}

use app_sdk::{rgb565, App, Input, Output, Sound, HEIGHT, WIDTH};
use core::ops::Range;

// Signed amplitude 127; phase 0..255 is one revolution.
const SIN: [i8; 256] = [
    0, 3, 6, 9, 12, 16, 19, 22, 25, 28, 31, 34, 37, 40, 43, 46, 49, 51, 54, 57, 60, 63, 65, 68, 71,
    73, 76, 78, 81, 83, 85, 88, 90, 92, 94, 96, 98, 100, 102, 104, 106, 107, 109, 111, 112, 113,
    115, 116, 117, 118, 120, 121, 122, 122, 123, 124, 125, 125, 126, 126, 126, 127, 127, 127, 127,
    127, 127, 127, 126, 126, 126, 125, 125, 124, 123, 122, 122, 121, 120, 118, 117, 116, 115, 113,
    112, 111, 109, 107, 106, 104, 102, 100, 98, 96, 94, 92, 90, 88, 85, 83, 81, 78, 76, 73, 71, 68,
    65, 63, 60, 57, 54, 51, 49, 46, 43, 40, 37, 34, 31, 28, 25, 22, 19, 16, 12, 9, 6, 3, 0, -3, -6,
    -9, -12, -16, -19, -22, -25, -28, -31, -34, -37, -40, -43, -46, -49, -51, -54, -57, -60, -63,
    -65, -68, -71, -73, -76, -78, -81, -83, -85, -88, -90, -92, -94, -96, -98, -100, -102, -104,
    -106, -107, -109, -111, -112, -113, -115, -116, -117, -118, -120, -121, -122, -122, -123, -124,
    -125, -125, -126, -126, -126, -127, -127, -127, -127, -127, -127, -127, -126, -126, -126, -125,
    -125, -124, -123, -122, -122, -121, -120, -118, -117, -116, -115, -113, -112, -111, -109, -107,
    -106, -104, -102, -100, -98, -96, -94, -92, -90, -88, -85, -83, -81, -78, -76, -73, -71, -68,
    -65, -63, -60, -57, -54, -51, -49, -46, -43, -40, -37, -34, -31, -28, -25, -22, -19, -16, -12,
    -9, -6, -3,
];


const COUNT: usize = 220;
/// Tail length in frames (points).
const TAIL: usize = 10;
/// Speed along the current, 1/16 px per 33 ms frame.
const SPEED: i32 = 64;
/// Frames for a particle to ease back into the current after a push: its
/// momentum.
const EASE: i32 = 14;
/// Pull of a finger, and the fastest a particle may go (1/16 px per frame).
const GRAVITY: i32 = 2000;
const MAX_SPEED: i32 = 260;
/// Each particle lives this many frames plus up to `LIFE_SPREAD` more, then
/// starts again somewhere random, so streams never clump for long.
const LIFE: u32 = 90;
const LIFE_SPREAD: u32 = 120;

/// From pattrnlabs.com: plum #45255a, orange #ef9d4b, olive #7e8a57 and
/// cream #f7efe6. The plum is lifted so it shows on black. (base, accent)
const PALETTE_RGB: [[(u8, u8, u8); 2]; 3] = [
    [(239, 157, 75), (160, 100, 215)], // orange, plum
    [(150, 168, 98), (247, 239, 230)], // olive, cream
    [(165, 112, 225), (239, 157, 75)], // plum, orange
];
/// Tail brightness from the head back, out of 256.
const FADE: [u32; TAIL] = [256, 236, 214, 190, 166, 142, 118, 94, 70, 46];

const fn shade(c: (u8, u8, u8), level: u32) -> u16 {
    rgb565(
        (c.0 as u32 * level / 256) as u8,
        (c.1 as u32 * level / 256) as u8,
        (c.2 as u32 * level / 256) as u8,
    )
}

/// [palette][accent][age]
const PALETTES: [[[u16; TAIL]; 2]; 3] = {
    let mut out = [[[0; TAIL]; 2]; 3];
    let mut p = 0;
    while p < 3 {
        let mut a = 0;
        while a < 2 {
            let mut s = 0;
            while s < TAIL {
                out[p][a][s] = shade(PALETTE_RGB[p][a], FADE[s]);
                s += 1;
            }
            a += 1;
        }
        p += 1;
    }
    out
};

#[derive(Clone, Copy)]
struct Particle {
    /// Position in 1/16 px.
    x: i32,
    y: i32,
    /// Velocity in 1/16 px per 33 ms frame.
    vx: i16,
    vy: i16,
    /// Frames left to live.
    life: u16,
}

/// Allocation-free; state is about 13 KB.
pub struct Flow {
    particles: [Particle; COUNT],
    /// Recent positions in px, a ring per particle; `head` is the newest.
    tails: [[(i16, i16); TAIL]; COUNT],
    /// Each tail's top and bottom row, so a strip skips tails it misses.
    spans: [(i16, i16); COUNT],
    head: usize,
    seed: u32,
    palette: usize,
}

impl Flow {
    pub const fn new() -> Self {
        let mut seed = 0x1234_5678;
        let mut particles = [Particle { x: 0, y: 0, vx: 0, vy: 0, life: 0 }; COUNT];
        let mut tails = [[(0, 0); TAIL]; COUNT];
        let mut spans = [(0, 0); COUNT];
        let mut i = 0;
        while i < COUNT {
            seed = next(seed);
            let x = (seed >> 8) % 480;
            seed = next(seed);
            let y = (seed >> 8) % 480;
            seed = next(seed);
            let life = ((seed >> 8) % (LIFE + LIFE_SPREAD)) as u16;
            particles[i] = Particle { x: x as i32 * 16, y: y as i32 * 16, vx: 0, vy: 0, life };
            tails[i] = [(x as i16, y as i16); TAIL];
            spans[i] = (y as i16, y as i16);
            i += 1;
        }
        Self { particles, tails, spans, head: 0, seed, palette: 0 }
    }
}

impl Default for Flow {
    fn default() -> Self {
        Self::new()
    }
}

const fn next(s: u32) -> u32 {
    s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223)
}

impl App for Flow {
    fn update(&mut self, input: &Input, dt_ms: u32, out: &mut Output) {
        if input.touch.tapped {
            self.palette = (self.palette + 1) % PALETTES.len();
            out.sound = Some(Sound::Click);
        }
        let dt = dt_ms.clamp(1, 66) as i32;
        // The currents turn over time.
        let t1 = (input.time_ms / 90) as i32;
        let t2 = (input.time_ms / 130) as i32;
        let t3 = (input.time_ms / 170) as i32;
        let wind_x = (input.tilt.x_mg as i32).clamp(-1000, 1000) * 60 / 1000;
        let wind_y = (input.tilt.y_mg as i32).clamp(-1000, 1000) * 60 / 1000;
        let finger = input.touch.down.then(|| (input.touch.x.min(479) as i32, input.touch.y.min(479) as i32));
        self.head = (self.head + 1) % TAIL;
        for ((p, tail), span) in self.particles.iter_mut().zip(self.tails.iter_mut()).zip(self.spans.iter_mut()) {
            let (px, py) = (p.x / 16, p.y / 16);
            // The current is the curl of psi = sin(ax + t1) + sin(by - t2) +
            // sin(c(x + y) + t3): swirling, and with no sinks, so particles
            // never pile up into a few streams.
            let ca = SIN[((px * 2 / 5 + t1 + 64) & 255) as usize] as i32;
            let cb = SIN[((py / 2 - t2 + 64) & 255) as usize] as i32;
            let cc = SIN[(((px + py) / 3 + t3 + 64) & 255) as usize] as i32;
            let fx = (cb + cc) * SPEED / 200 + wind_x;
            let fy = -(ca + cc) * SPEED / 200 + wind_y;
            // Ease toward the current; whatever speed a push gave is kept and
            // only fades, so released particles coast on.
            let mut vx = p.vx as i32 + (fx - p.vx as i32) * dt / (33 * EASE);
            let mut vy = p.vy as i32 + (fy - p.vy as i32) * dt / (33 * EASE);
            if let Some((tx, ty)) = finger {
                // A pull of GRAVITY / (d + 30), so it reaches across the
                // screen but is strongest near the finger.
                let (dx, dy) = (tx - px, ty - py);
                let d = isqrt(dx * dx + dy * dy).max(1);
                let k = (d + 30) * d;
                vx += dx * GRAVITY / k * dt / 33;
                vy += dy * GRAVITY / k * dt / 33;
            }
            let speed = isqrt(vx * vx + vy * vy);
            if speed > MAX_SPEED {
                vx = vx * MAX_SPEED / speed;
                vy = vy * MAX_SPEED / speed;
            }
            (p.vx, p.vy) = (vx as i16, vy as i16);
            p.x += vx * dt / 33;
            p.y += vy * dt / 33;
            p.life = p.life.saturating_sub(1);
            if p.life == 0 || !(0..480 * 16).contains(&p.x) || !(0..480 * 16).contains(&p.y) {
                // Old or off the edge: start again somewhere new, with no tail.
                self.seed = next(self.seed);
                p.life = (LIFE + (self.seed >> 8) % LIFE_SPREAD) as u16;
                self.seed = next(self.seed);
                p.x = ((self.seed >> 8) % 480) as i32 * 16;
                self.seed = next(self.seed);
                p.y = ((self.seed >> 8) % 480) as i32 * 16;
                (p.vx, p.vy) = (0, 0);
                *tail = [((p.x / 16) as i16, (p.y / 16) as i16); TAIL];
            }
            tail[self.head] = ((p.x / 16) as i16, (p.y / 16) as i16);
            let (lo, hi) = tail.iter().fold((i16::MAX, i16::MIN), |(lo, hi), &(_, y)| (lo.min(y), hi.max(y)));
            *span = (lo, hi + 1);
        }
    }

    fn render(&self, rows: Range<u16>, buf: &mut [u16]) {
        assert!(rows.start <= rows.end && rows.end <= HEIGHT);
        assert_eq!(buf.len(), (rows.end - rows.start) as usize * WIDTH as usize);
        buf.fill(0);
        let (y0, y1) = (rows.start as i32, rows.end as i32);
        let colours = &PALETTES[self.palette];
        for (i, (tail, &(top, bottom))) in self.tails.iter().zip(&self.spans).enumerate() {
            if bottom as i32 + 1 < y0 || top as i32 > y1 {
                continue;
            }
            // Every third particle takes the accent colour.
            let shades = &colours[usize::from(i % 3 == 0)];
            // Oldest segment first, so the brighter head draws on top.
            for age in (0..TAIL - 1).rev() {
                let a = tail[(self.head + TAIL - age - 1) % TAIL];
                let b = tail[(self.head + TAIL - age) % TAIL];
                line(buf, y0, y1, a, b, shades[age]);
            }
        }
    }
}

/// A 2 px wide line from `a` to `b`, clipped to the strip rows `y0..y1`.
fn line(buf: &mut [u16], y0: i32, y1: i32, a: (i16, i16), b: (i16, i16), colour: u16) {
    let (ax, ay, bx, by) = (a.0 as i32, a.1 as i32, b.0 as i32, b.1 as i32);
    if ay.max(by) + 1 < y0 || ay.min(by) >= y1 {
        return;
    }
    let steps = (bx - ax).abs().max((by - ay).abs()).max(1);
    for s in 0..=steps {
        let x = ax + (bx - ax) * s / steps;
        let y = ay + (by - ay) * s / steps;
        for yy in y.max(y0)..(y + 2).min(y1) {
            let row = ((yy - y0) * 480) as usize;
            for xx in x.max(0)..(x + 2).min(480) {
                buf[row + xx as usize] = colour;
            }
        }
    }
}

fn isqrt(n: i32) -> i32 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}
