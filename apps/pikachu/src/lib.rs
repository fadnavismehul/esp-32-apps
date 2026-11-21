#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(test)]
#[path = "../build/convert.rs"]
mod convert;

#[cfg(test)]
mod tests {
    use super::*;
    use app_sdk::{App, Input, Output, Tilt, Touch};
    use std::{string::String, vec, vec::Vec};

    const UPRIGHT: Tilt = Tilt { x_mg: 0, y_mg: 1000, z_mg: 0 };
    /// 2026-10-26 as local Unix seconds.
    const DAY: u32 = 1_792_972_800;

    struct Rig {
        p: Pikachu,
        t: u32,
        clock: Option<u32>,
    }

    impl Rig {
        fn new() -> Self {
            Rig { p: Pikachu::new(), t: 0, clock: Some(DAY + 12 * 3600) }
        }

        fn step(&mut self, tilt: Tilt, touch: Touch, key: bool) -> Output {
            self.t += 33;
            let mut out = Output::default();
            let input = Input {
                tilt,
                touch,
                time_ms: self.t,
                local_time_s: self.clock.map(|c| c + self.t / 1000),
                key,
                ..Input::default()
            };
            self.p.update(&input, 33, &mut out);
            out
        }

        fn idle(&mut self, frames: usize) {
            for _ in 0..frames {
                self.step(UPRIGHT, Touch::default(), false);
            }
        }

        fn tap(&mut self, x: u16, y: u16) -> Output {
            self.step(UPRIGHT, Touch { down: false, x, y, tapped: true }, false)
        }

        /// The middle of the sprite on screen.
        fn centre(&self) -> (u16, u16) {
            let (left, top, w, h) = self.p.sprite_box();
            ((left + w / 2) as u16, (top + h / 2) as u16)
        }
    }

    #[test]
    fn tapping_pikachu_makes_it_happy() {
        let mut r = Rig::new();
        r.idle(5);
        let (x, y) = r.centre();
        let out = r.tap(x, y);
        assert!(matches!(out.sound, Some(Sound::Tick { .. })));
        assert_eq!(r.p.anim(), HAPPY);
        assert!(r.p.sparks.iter().filter(|s| s.life > 0).count() >= 8);
        r.idle(10);
        assert!(r.p.jump() > 0, "it hops");
        r.idle(60);
        assert_ne!(r.p.anim(), HAPPY);
    }

    #[test]
    fn tapping_elsewhere_walks_over() {
        let mut r = Rig::new();
        r.idle(5);
        r.tap(470, 100);
        assert_eq!(r.p.anim(), WALK);
        assert!(!r.p.facing_left);
        r.idle(150);
        assert_eq!(r.p.x_px(), MAX_X, "walked to the right edge");
        assert_ne!(r.p.anim(), WALK);
        r.tap(0, 100);
        r.idle(2);
        assert!(r.p.facing_left);
    }

    #[test]
    fn sleeps_at_night_and_half_wakes_on_a_tap() {
        let mut r = Rig::new();
        r.clock = Some(DAY + 23 * 3600 + 30 * 60);
        r.idle(3);
        assert_eq!(r.p.anim(), SLEEP);
        let (x, y) = r.centre();
        r.tap(x, y);
        r.idle(1);
        assert_eq!(r.p.anim(), IDLE, "half awake");
        r.idle(150);
        assert_eq!(r.p.anim(), SLEEP, "dozes off again");
        // Morning.
        r.clock = Some(DAY + 7 * 3600 - 10);
        r.idle(3);
        assert_eq!(r.p.anim(), SLEEP);
        r.idle(400);
        assert_ne!(r.p.anim(), SLEEP);
    }

    #[test]
    fn no_clock_means_no_sleep() {
        let mut r = Rig::new();
        r.clock = None;
        r.idle(30);
        assert_ne!(r.p.anim(), SLEEP);
    }

    #[test]
    fn shaking_makes_it_dizzy_but_turning_does_not() {
        let mut r = Rig::new();
        r.idle(5);
        // One steady quarter turn and back.
        for tilt in [Tilt { x_mg: 1000, y_mg: 0, z_mg: 0 }, UPRIGHT] {
            for _ in 0..10 {
                r.step(tilt, Touch::default(), false);
            }
        }
        assert_ne!(r.p.anim(), DIZZY);
        for i in 0..8 {
            let x_mg = if i % 2 == 0 { 900 } else { -900 };
            r.step(Tilt { x_mg, y_mg: 1000, z_mg: 0 }, Touch::default(), false);
        }
        assert_eq!(r.p.anim(), DIZZY);
        r.idle(120);
        assert_ne!(r.p.anim(), DIZZY);
    }

    #[test]
    fn slides_to_the_low_side_then_stops() {
        let mut r = Rig::new();
        r.idle(5);
        let start = r.p.x_px();
        let left = Tilt { x_mg: -600, y_mg: 800, z_mg: 0 };
        for _ in 0..20 {
            r.step(left, Touch::default(), false);
        }
        assert!(r.p.x_px() < start - 30, "slid from {start} to {}", r.p.x_px());
        r.idle(30);
        let rest = r.p.x_px();
        r.idle(10);
        assert_eq!(r.p.x_px(), rest, "came to rest");
    }

    #[test]
    fn key_toggles_ambient_sparks() {
        let mut r = Rig::new();
        let mut lit = 0;
        for _ in 0..200 {
            r.idle(1);
            lit += usize::from(r.p.sparks.iter().any(|s| s.life > 0));
        }
        assert!(lit > 10, "sparks on by default");
        r.step(UPRIGHT, Touch::default(), true);
        assert!(!r.p.sparks_on);
        r.idle(200);
        assert!(r.p.sparks.iter().all(|s| s.life == 0));
        r.step(UPRIGHT, Touch::default(), true);
        assert!(r.p.sparks_on);
    }

    #[test]
    fn blinks_now_and_then() {
        let mut r = Rig::new();
        r.clock = None;
        let mut blinks = 0;
        for _ in 0..600 {
            r.idle(1);
            blinks += usize::from(r.p.anim() == BLINK);
        }
        // A blink lasts as long as its animation, which depends on the art.
        assert!(blinks > 0 && blinks < 150, "blink frames {blinks}");
    }

    #[test]
    fn arbitrary_strips_match_full_frame() {
        let mut r = Rig::new();
        r.idle(20);
        let (x, y) = r.centre();
        r.tap(x, y);
        r.idle(5);
        let mut full = vec![0; 480 * 480];
        r.p.render(0..480, &mut full);
        assert!(full.iter().filter(|p| **p != 0).count() > 10_000);
        for (start, end) in [(0, 2), (7, 19), (200, 212), (478, 480)] {
            let mut strip = vec![0; (end - start) * 480];
            r.p.render(start as u16..end as u16, &mut strip);
            assert_eq!(strip, full[start * 480..end * 480]);
        }
    }

    // The sheet converter.

    fn png(width: usize, height: usize, rgba: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().unwrap().write_image_data(rgba).unwrap();
        bytes
    }

    /// A 2x2 sheet of 2x2 frames: row 0 is two frames, row 1 one.
    fn sheet() -> Vec<u8> {
        let (o, r, g) = ([0, 0, 0, 0], [255, 0, 0, 255], [0, 255, 0, 255]);
        let rows = [[r, o, g, g], [r, r, g, o], [o, o, o, o], [r, o, o, o]];
        png(4, 4, &rows.concat().concat())
    }

    #[test]
    fn converts_a_sheet() {
        let toml = "frame = [2, 2]\n[idle]\nrow = 0\nframes = 2\n[sleep]\nrow = 1\nframes = 1\nms = 900\n";
        let s = convert::convert(toml, &sheet()).unwrap();
        assert_eq!(s.palette.len(), 3, "transparent, red, green");
        assert_eq!(s.pixels, [1, 0, 1, 1, 2, 2, 2, 0, 0, 0, 1, 0]);
        assert_eq!(s.scale, 120);
        assert_eq!(s.foot, 2);
        assert_eq!(s.anims[0], convert::Anim { first: 0, count: 2, ms: 500 });
        assert_eq!(s.anims[5], convert::Anim { first: 2, count: 1, ms: 900 });
        // Missing ones play idle at their own speed.
        assert_eq!(s.anims[2], convert::Anim { first: 0, count: 2, ms: 150 });
        let rust = convert::to_rust(&s, "x.bin");
        assert!(rust.contains("pub const SCALE: usize = 120;"));
        assert!(rust.contains("Anim { first: 2, count: 1, ms: 900 }, // sleep"));
    }

    #[test]
    fn animations_on_one_row_share_frames() {
        let toml = "frame = [2, 2]\n[idle]\nrow = 0\nframes = 2\n[walk]\nrow = 0\nframes = 1\n[sleep]\nrow = 1\nframes = 1\n";
        let s = convert::convert(toml, &sheet()).unwrap();
        assert_eq!(s.pixels.len(), 3 * 4, "two idle frames and one sleep frame");
        assert_eq!(s.anims[2], convert::Anim { first: 0, count: 1, ms: 150 });
        assert_eq!(s.anims[5], convert::Anim { first: 2, count: 1, ms: 800 });
    }

    #[test]
    fn rejects_bad_sheets() {
        let err = |toml: &str| convert::convert(toml, &sheet()).unwrap_err();
        assert!(err("frame = [2, 2]\n[walk]\nrow = 0\nframes = 1\n").contains("[idle]"));
        assert!(err("frame = [3, 3]\n[idle]\nrow = 0\nframes = 1\n").contains("whole grid"));
        assert!(err("frame = [2, 2]\n[idle]\nrow = 2\nframes = 1\n").contains("2 rows"));
        assert!(err("frame = [2, 2]\n[idle]\nrow = 0\nframes = 3\n").contains("of 2"));
        assert!(err("frame = [2, 2]\n[idel]\nrow = 0\nframes = 1\n").contains("unknown key `idel`"));
        assert!(err("frame = [300, 300]\n[idle]\nrow = 0\nframes = 1\n").contains("at most"));
        assert!(err("[idle]\nrow = 0\nframes = 1\n").contains("frame = [width, height]"));
    }

    #[test]
    fn default_art_converts() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/art/default/");
        let toml = String::from_utf8(std::fs::read(std::format!("{dir}sheet.toml")).unwrap()).unwrap();
        let s = convert::convert(&toml, &std::fs::read(std::format!("{dir}sheet.png")).unwrap()).unwrap();
        assert!(s.anims.iter().all(|a| a.count > 0));
    }
}

use app_sdk::{rgb565, App, Input, Output, Sound};
use core::ops::Range;

// Pikachu lives on a ground line near the bottom of the screen. It wanders,
// blinks, hops when tapped, walks to where you tap, slides when the device
// tilts, gets dizzy when shaken, and sleeps at night. Its art comes from a
// sprite sheet that build.rs converts (see `art/`); sparks, "z"s and dizzy
// stars are drawn here so they work with any sheet.

/// One animation: `count` frames starting at frame `first`, `ms` each.
#[derive(Clone, Copy)]
pub struct Anim {
    pub first: usize,
    pub count: usize,
    pub ms: u32,
}

mod sheet {
    use super::Anim;
    include!(concat!(env!("OUT_DIR"), "/pikachu_sheet.rs"));
}

use sheet::{ANIMS, FACES_LEFT, FOOT, FRAME_H, FRAME_W, PALETTE, PIXELS, SCALE};

// Indices into `ANIMS`, in the order `convert::ANIMS` lists them.
const IDLE: usize = 0;
const BLINK: usize = 1;
const WALK: usize = 2;
const HAPPY: usize = 3;
const DIZZY: usize = 4;
const SLEEP: usize = 5;

const SPRITE_W: i32 = (FRAME_W * SCALE) as i32;
const SPRITE_H: i32 = (FRAME_H * SCALE) as i32;
/// Screen row the feet stand on.
const GROUND_Y: i32 = 410;
/// How far the sprite's centre may go, in pixels.
const MIN_X: i32 = SPRITE_W / 2 + 8;
const MAX_X: i32 = 480 - SPRITE_W / 2 - 8;

/// Positions are in 1/256 pixel, velocities in 1/256 pixel per ms.
const FIX: i32 = 256;
const WALK_SPEED: i32 = 24; // about 90 px/s
const TILT_DEADZONE_MG: i32 = 250;
/// Summed tilt change, decaying, that counts as a shake.
const SHAKE_MG: i32 = 3500;

const HAPPY_MS: u32 = 1200;
const HOP_MS: u32 = 600;
const HOP_PX: i32 = 24;
const DIZZY_MS: u32 = 2500;
const HALF_AWAKE_MS: u32 = 4000;

const SPARKS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Idle,
    Walk { target: i32 },
    Happy { since: u32 },
    Dizzy { until: u32 },
    Sleep,
}

#[derive(Clone, Copy, Default)]
struct Spark {
    /// Position in 1/16 pixel and velocity in 1/16 pixel per ms.
    x: i32,
    y: i32,
    vx: i32,
    vy: i32,
    life: u32,
}

pub struct Pikachu {
    now: u32,
    mode: Mode,
    /// Centre of the sprite, in 1/256 pixel.
    x: i32,
    vx: i32,
    facing_left: bool,
    anim: usize,
    anim_since: u32,
    blink_until: u32,
    next_blink: u32,
    next_wander: u32,
    next_spark: u32,
    awake_until: u32,
    sparks_on: bool,
    sparks: [Spark; SPARKS],
    shake: i32,
    last_tilt: Option<(i16, i16, i16)>,
    seed: u32,
}

impl Default for Pikachu {
    fn default() -> Self {
        Self::new()
    }
}

impl Pikachu {
    pub const fn new() -> Self {
        Pikachu {
            now: 0,
            mode: Mode::Idle,
            x: 240 * FIX,
            vx: 0,
            facing_left: false,
            anim: IDLE,
            anim_since: 0,
            blink_until: 0,
            next_blink: 2000,
            next_wander: 4000,
            next_spark: 1500,
            awake_until: 0,
            sparks_on: true,
            sparks: [Spark { x: 0, y: 0, vx: 0, vy: 0, life: 0 }; SPARKS],
            shake: 0,
            last_tilt: None,
            seed: 0x9e37_79b9,
        }
    }

    fn rand(&mut self) -> u32 {
        let mut x = self.seed;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        x
    }

    fn x_px(&self) -> i32 {
        self.x / FIX
    }

    /// Pixels above the ground for the happy hops.
    fn jump(&self) -> i32 {
        let Mode::Happy { since } = self.mode else { return 0 };
        let t = (self.now.wrapping_sub(since) % HOP_MS) as i32;
        let hop = HOP_MS as i32;
        4 * HOP_PX * t / hop * (hop - t) / hop
    }

    /// Left, top, width and height of the sprite on screen.
    fn sprite_box(&self) -> (i32, i32, i32, i32) {
        let top = GROUND_Y - (FOOT * SCALE) as i32 - self.jump();
        (self.x_px() - SPRITE_W / 2, top, SPRITE_W, SPRITE_H)
    }

    fn anim(&self) -> usize {
        match self.mode {
            Mode::Happy { .. } => HAPPY,
            Mode::Dizzy { .. } => DIZZY,
            Mode::Sleep if self.awake_until > self.now => IDLE,
            Mode::Sleep => SLEEP,
            Mode::Walk { .. } => WALK,
            Mode::Idle if self.vx.abs() > 6 => WALK,
            Mode::Idle if self.blink_until > self.now => BLINK,
            Mode::Idle => IDLE,
        }
    }

    fn burst(&mut self, count: usize, speed: i32) {
        let (left, top, w, h) = self.sprite_box();
        for _ in 0..count {
            let Some(slot) = self.sparks.iter().position(|s| s.life == 0) else { break };
            // From around the face: the middle of the upper half.
            let x = left + w / 4 + (self.rand() % (w as u32 / 2)) as i32;
            let y = top + h / 4 + (self.rand() % (h as u32 / 4)) as i32;
            let vx = (self.rand() % (2 * speed as u32 + 1)) as i32 - speed;
            let vy = -((self.rand() % speed as u32) as i32) - speed / 2;
            let life = 300 + self.rand() % 400;
            self.sparks[slot] = Spark { x: x * 16, y: y * 16, vx, vy, life };
        }
    }

    fn hit(&self, x: u16, y: u16) -> bool {
        let (left, top, w, h) = self.sprite_box();
        let (x, y) = (x as i32, y as i32);
        x >= left - 20 && x < left + w + 20 && y >= top - 20 && y < top + h + 20
    }
}

impl App for Pikachu {
    fn update(&mut self, input: &Input, dt_ms: u32, out: &mut Output) {
        self.now = input.time_ms;
        let now = self.now;
        let dt = dt_ms.min(100) as i32;

        // Shakes: tilt that keeps changing, not a single turn.
        let t = input.tilt;
        if let Some((x, y, z)) = self.last_tilt {
            let change = (t.x_mg - x).unsigned_abs() + (t.y_mg - y).unsigned_abs() + (t.z_mg - z).unsigned_abs();
            self.shake += change as i32;
        }
        self.shake -= self.shake * dt / 150;
        self.last_tilt = Some((t.x_mg, t.y_mg, t.z_mg));
        if self.shake > SHAKE_MG && !matches!(self.mode, Mode::Dizzy { .. }) {
            self.mode = Mode::Dizzy { until: now + DIZZY_MS };
            out.sound = Some(Sound::Tick { pitch: 40 });
        }

        if input.key {
            self.sparks_on = !self.sparks_on;
            out.sound = Some(Sound::Tick { pitch: if self.sparks_on { 200 } else { 90 } });
        }

        let night = input.local_time_s.is_some_and(|s| {
            let hour = s / 3600 % 24;
            !(7..23).contains(&hour)
        });

        if input.touch.tapped {
            let (x, y) = (input.touch.x, input.touch.y);
            if self.mode == Mode::Sleep {
                self.awake_until = now + HALF_AWAKE_MS;
                out.sound = Some(Sound::Tick { pitch: 120 });
            } else if self.hit(x, y) {
                self.mode = Mode::Happy { since: now };
                self.vx = 0;
                self.burst(12, 5);
                out.sound = Some(Sound::Tick { pitch: 220 });
            } else if !matches!(self.mode, Mode::Dizzy { .. }) {
                self.mode = Mode::Walk { target: (x as i32).clamp(MIN_X, MAX_X) * FIX };
            }
        }

        // Timed modes end; night and day.
        match self.mode {
            Mode::Happy { since } if now.wrapping_sub(since) >= HAPPY_MS => self.mode = Mode::Idle,
            Mode::Dizzy { until } if now >= until => self.mode = Mode::Idle,
            Mode::Sleep if !night => {
                self.mode = Mode::Idle;
                self.next_wander = now + 2000;
            }
            Mode::Idle | Mode::Walk { .. } if night && now >= self.awake_until => {
                self.mode = Mode::Sleep;
                self.vx = 0;
            }
            _ => {}
        }

        // Moving: walking to a target, or sliding on a slope.
        match self.mode {
            Mode::Walk { target } => {
                let d = target - self.x;
                self.facing_left = d < 0;
                let step = WALK_SPEED * dt;
                if d.abs() <= step {
                    self.x = target;
                    self.mode = Mode::Idle;
                    self.next_wander = now + 3000 + self.rand() % 4000;
                } else {
                    self.x += step * d.signum();
                }
                self.vx = 0;
            }
            Mode::Idle | Mode::Dizzy { .. } => {
                let x_mg = t.x_mg as i32;
                let slope = x_mg - x_mg.signum() * TILT_DEADZONE_MG;
                if x_mg.abs() > TILT_DEADZONE_MG {
                    self.vx += slope * dt / 1500;
                }
                self.vx -= self.vx * dt / 200;
                if self.vx.abs() > 6 {
                    self.facing_left = self.vx < 0;
                } else if x_mg.abs() <= TILT_DEADZONE_MG {
                    self.vx = 0;
                }
                self.x += self.vx * dt;
            }
            Mode::Happy { .. } | Mode::Sleep => {}
        }
        if self.x <= MIN_X * FIX || self.x >= MAX_X * FIX {
            self.x = self.x.clamp(MIN_X * FIX, MAX_X * FIX);
            self.vx = 0;
        }

        // Idle habits: blinking, wandering, crackling cheeks.
        if self.mode == Mode::Idle && self.vx == 0 {
            if now >= self.next_blink {
                // A sheet without a blink plays idle instead; keep that short.
                let blink = ANIMS[BLINK];
                let long = if blink.first == ANIMS[IDLE].first { 150 } else { blink.ms * blink.count as u32 };
                self.blink_until = now + long;
                self.next_blink = now + 2500 + self.rand() % 3500;
            }
            if now >= self.next_wander {
                self.next_wander = now + 3000 + self.rand() % 6000;
                match self.rand() % 4 {
                    0 | 1 => {
                        let span = (MAX_X - MIN_X) as u32;
                        self.mode = Mode::Walk { target: (MIN_X + (self.rand() % span) as i32) * FIX };
                    }
                    2 => self.facing_left = !self.facing_left,
                    _ => {}
                }
            }
        }
        if self.sparks_on && self.mode != Mode::Sleep && now >= self.next_spark {
            self.next_spark = now + 800 + self.rand() % 1500;
            self.burst(3, 3);
        }

        for s in &mut self.sparks {
            if s.life == 0 {
                continue;
            }
            s.life = s.life.saturating_sub(dt as u32);
            s.x += s.vx * dt;
            s.y += s.vy * dt;
            s.vy += dt / 48; // a little gravity
        }

        let anim = self.anim();
        if anim != self.anim {
            self.anim = anim;
            self.anim_since = now;
        }
    }

    fn render(&self, rows: Range<u16>, buf: &mut [u16]) {
        buf.fill(0);
        let (r0, r1) = (rows.start as i32, rows.end as i32);
        let (left, top, _, _) = self.sprite_box();

        // Shadow: an ellipse on the ground that shrinks while hopping.
        let rx = SPRITE_W * 3 / 8 - self.jump() / 2;
        let ry = 10;
        for y in r0.max(GROUND_Y - ry)..r1.min(GROUND_Y + ry + 1) {
            let dy = y - GROUND_Y;
            let half = rx * isqrt((ry * ry - dy * dy) * 1024) / (ry * 32);
            let row = &mut buf[((y - r0) * 480) as usize..][..480];
            let (a, b) = ((self.x_px() - half).max(0), (self.x_px() + half).min(479));
            for p in &mut row[a as usize..=b as usize] {
                *p = SHADOW;
            }
        }

        // The sprite.
        let anim = ANIMS[self.anim];
        let frame = anim.first + (self.now.wrapping_sub(self.anim_since) / anim.ms.max(1)) as usize % anim.count;
        let pixels = &PIXELS[frame * FRAME_W * FRAME_H..][..FRAME_W * FRAME_H];
        let flip = self.facing_left != FACES_LEFT;
        for y in r0.max(top)..r1.min(top + SPRITE_H) {
            let src = &pixels[((y - top) as usize / SCALE) * FRAME_W..][..FRAME_W];
            let row = &mut buf[((y - r0) * 480) as usize..][..480];
            for (fx, index) in src.iter().enumerate() {
                if *index == 0 {
                    continue;
                }
                let col = if flip { FRAME_W - 1 - fx } else { fx };
                let x = left + (col * SCALE) as i32;
                let (a, b) = (x.max(0), (x + SCALE as i32).min(480));
                if a < b {
                    row[a as usize..b as usize].fill(PALETTE[*index as usize]);
                }
            }
        }

        // Sparks.
        for s in self.sparks.iter().filter(|s| s.life > 0) {
            let colour = if s.life > 250 { SPARK_HOT } else { SPARK };
            plus(buf, r0, r1, s.x / 16, s.y / 16, 5, colour);
        }

        // Sleep: three "z"s drifting up from the head.
        if self.anim == SLEEP {
            for i in 0..3u32 {
                let t = (self.now + i * 1000) % 3000;
                let x = self.x_px() + if FACES_LEFT == self.facing_left { 40 } else { -40 } + (t / 60) as i32;
                let y = top + SPRITE_H / 3 - (t / 25) as i32;
                let size = 2 + (t / 1000) as i32;
                glyph_z(buf, r0, r1, x, y, size, if t < 2400 { ZZZ } else { ZZZ_FADED });
            }
        }

        // Dizzy: stars circling above the head.
        if self.anim == DIZZY {
            for i in 0..3u32 {
                let a = ((self.now / 8 + i * 21) % 64) as usize;
                let x = self.x_px() + SIN[(a + 16) % 64] * 60 / 256;
                let y = top + SPRITE_H / 6 + SIN[a] * 14 / 256;
                plus(buf, r0, r1, x, y, 4, STAR);
            }
        }
    }
}

const SHADOW: u16 = rgb565(34, 34, 44);
const SPARK: u16 = rgb565(255, 214, 40);
const SPARK_HOT: u16 = rgb565(255, 250, 200);
const ZZZ: u16 = rgb565(170, 200, 255);
const ZZZ_FADED: u16 = rgb565(70, 85, 120);
const STAR: u16 = rgb565(255, 230, 90);

/// sin(2πi/64) × 256.
const SIN: [i32; 64] = [
    0, 25, 50, 74, 98, 121, 142, 162, 181, 198, 213, 226, 237, 245, 251, 255, 256, 255, 251, 245, 237, 226,
    213, 198, 181, 162, 142, 121, 98, 74, 50, 25, 0, -25, -50, -74, -98, -121, -142, -162, -181, -198, -213,
    -226, -237, -245, -251, -255, -256, -255, -251, -245, -237, -226, -213, -198, -181, -162, -142, -121,
    -98, -74, -50, -25,
];

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

fn put(buf: &mut [u16], r0: i32, r1: i32, x: i32, y: i32, colour: u16) {
    if (0..480).contains(&x) && y >= r0 && y < r1 {
        buf[((y - r0) * 480 + x) as usize] = colour;
    }
}

/// A plus sign `arm` pixels from the centre each way, two pixels thick.
fn plus(buf: &mut [u16], r0: i32, r1: i32, cx: i32, cy: i32, arm: i32, colour: u16) {
    for d in -arm..=arm {
        for t in 0..2 {
            put(buf, r0, r1, cx + d, cy + t, colour);
            put(buf, r0, r1, cx + t, cy + d, colour);
        }
    }
}

/// A 5x5 "z", each dot `size` pixels square, centred on (cx, cy).
fn glyph_z(buf: &mut [u16], r0: i32, r1: i32, cx: i32, cy: i32, size: i32, colour: u16) {
    const Z: [u8; 5] = [0b11111, 0b00010, 0b00100, 0b01000, 0b11111];
    for (gy, bits) in Z.iter().enumerate() {
        for gx in 0..5 {
            if bits & (0b10000 >> gx) == 0 {
                continue;
            }
            for dy in 0..size {
                for dx in 0..size {
                    let x = cx + (gx - 2) * size + dx;
                    let y = cy + (gy as i32 - 2) * size + dy;
                    put(buf, r0, r1, x, y, colour);
                }
            }
        }
    }
}
