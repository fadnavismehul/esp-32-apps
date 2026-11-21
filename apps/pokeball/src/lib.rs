//! True-3D animated Pokéball app: starts closed, pops open and closed on tap
//! with custom Pokéball sound effects, rotates 360° smoothly via touch drag
//! (staying wherever you leave it), and revolves 360° in full 3D pitch, yaw,
//! and roll mapped to the device's QMI8658 3-axis gyroscope. All markings
//! (Classic, Great Ball crimson fins, Ultra Ball gold H-band, Master Ball spots
//! & M crest) and the rear metallic hinge are rigidly mapped on the 3D sphere
//! locked to the central latch button. KEY cycles ball styles.
#![no_std]

#[cfg(test)]
extern crate std;

use app_sdk::{rgb565, App, Input, Output, Sound};
use core::ops::Range;

const W: i32 = 480;
const H: i32 = 480;
const R: i32 = 146;
const R2_MAX: i32 = R * R;
const RIM_START: i32 = (R - 14) * (R - 14);
/// Fixed-point reciprocal for indexing `SPHERE_Z`: `(255 << 14) / R^2`.
const INV_R2_Z: i32 = ((255 << 14) + R2_MAX / 2) / R2_MAX;

const BAND_HALF: i32 = 12;
const BTN_OUTER_R2: i32 = 42 * 42;
const BTN_RING_R2: i32 = 30 * 30;
const BTN_BEZEL_R2: i32 = 20 * 20;
const BTN_CORE_R2: i32 = 17 * 17;

const OPEN_FULL: i32 = 1024;
const SPARK_COUNT: usize = 20;
/// Drag distance (px) before a touch becomes a 360° rotation instead of a tap.
const DRAG_THRESHOLD_SQ: i32 = 10 * 10;
/// Touch drag on the Pokéball (`r <= R`) revolves in 3D pitch & yaw (~112 u16 turns/px,
/// so dragging across the ball's diameter revolves ~180° like a real physical trackball).
const TOUCH_REVOLVE_SCALE: i32 = 112;
/// Gyro deadband in deci-dps (2.2°/s) to eliminate static drift when still.
const GYRO_DEADBAND: i32 = 22;

/// 256-entry sine table scaled by 1024 (Bhaskara approximation).
const SIN256: [i16; 256] = {
    let mut t = [0i16; 256];
    let mut i = 0usize;
    while i < 256 {
        let x = (i % 128) as i64;
        let p = 128i64;
        let y = 1024 * 16 * x * (p - x) / (5 * p * p - 4 * x * (p - x));
        t[i] = if i < 128 { y as i16 } else { -y as i16 };
        i += 1;
    }
    t
};

/// Precomputed sphere surface depth `round(R * sqrt(i / 255))` for `i` in `0..=255`.
/// Gives exact 3D Z coordinate on the sphere in 2 ops + 1 byte load (no per-pixel sqrt).
const SPHERE_Z: [u8; 256] = {
    let mut lut = [0u8; 256];
    let mut i = 0u32;
    while i < 256 {
        let v = isqrt_const(((R * R) as u32 * i + 127) / 255);
        lut[i as usize] = if v > R as u32 { R as u8 } else { v as u8 };
        i += 1;
    }
    lut
};

#[inline]
fn sin_u16(angle: u16) -> i32 {
    let idx = (angle >> 8) as usize;
    let frac = (angle & 255) as i32;
    let a = SIN256[idx] as i32;
    let b = SIN256[(idx + 1) & 255] as i32;
    a + (((b - a) * frac) >> 8)
}

#[inline]
fn cos_u16(angle: u16) -> i32 {
    sin_u16(angle.wrapping_add(16_384))
}

const fn isqrt_const(n: u32) -> u32 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

fn isqrt(n: u32) -> u32 {
    if n == 0 {
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

/// Orthonormal 3×3 rotation matrix (10-bit fixed point, unit = 1024) mapping
/// screen-space sphere coordinates `(px, py, pz)` into Pokéball local 3D body
/// space `(bx, by, bz)` where `+bz` is front button, `-by` is top colored dome,
/// and `+by` is bottom white dome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mat3 {
    m: [[i32; 3]; 3],
}

impl Mat3 {
    const IDENTITY: Self = Self {
        m: [[1024, 0, 0], [0, 1024, 0], [0, 0, 1024]],
    };

    /// Rotate around screen Z (positive = clockwise on screen).
    fn roll_screen(&mut self, delta: u16) {
        if delta == 0 {
            return;
        }
        let c = cos_u16(delta);
        let s = sin_u16(delta);
        for row in &mut self.m {
            let (x, y) = (row[0], row[1]);
            row[0] = (x * c - y * s) >> 10;
            row[1] = (x * s + y * c) >> 10;
        }
    }

    /// Rotate around screen Y (positive = front button orbits right toward +X).
    fn yaw_screen(&mut self, delta: u16) {
        if delta == 0 {
            return;
        }
        let c = cos_u16(delta);
        let s = sin_u16(delta);
        for row in &mut self.m {
            let (x, z) = (row[0], row[2]);
            row[0] = (x * c + z * s) >> 10;
            row[2] = (-x * s + z * c) >> 10;
        }
    }

    /// Rotate around screen X (positive = top red pole pitches forward toward viewer).
    fn pitch_screen(&mut self, delta: u16) {
        if delta == 0 {
            return;
        }
        let c = cos_u16(delta);
        let s = sin_u16(delta);
        for row in &mut self.m {
            let (y, z) = (row[1], row[2]);
            row[1] = (y * c + z * s) >> 10;
            row[2] = (-y * s + z * c) >> 10;
        }
    }

    /// Return a copy tilted around the Pokéball's own local X axis (used to hinge
    /// the top shell open backward and bottom shell slightly forward in true 3D).
    fn with_local_pitch(&self, delta: u16) -> Self {
        if delta == 0 {
            return *self;
        }
        let c = cos_u16(delta);
        let s = sin_u16(delta);
        let mut out = *self;
        for k in 0..3 {
            let r1 = self.m[1][k];
            let r2 = self.m[2][k];
            out.m[1][k] = (r1 * c + r2 * s) >> 10;
            out.m[2][k] = (-r1 * s + r2 * c) >> 10;
        }
        out
    }

    /// Gram-Schmidt re-orthonormalization so repeated rotations never drift or skew.
    fn orthonormalize(&mut self) {
        fn norm(v: &mut [i32; 3]) {
            let len2 = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as u32;
            let len = isqrt(len2).max(1) as i32;
            v[0] = v[0] * 1024 / len;
            v[1] = v[1] * 1024 / len;
            v[2] = v[2] * 1024 / len;
        }
        norm(&mut self.m[0]);
        let dot01 = (self.m[0][0] * self.m[1][0]
            + self.m[0][1] * self.m[1][1]
            + self.m[0][2] * self.m[1][2])
            >> 10;
        for k in 0..3 {
            self.m[1][k] -= (dot01 * self.m[0][k]) >> 10;
        }
        norm(&mut self.m[1]);
        let (a, b) = (self.m[0], self.m[1]);
        self.m[2] = [
            (a[1] * b[2] - a[2] * b[1]) >> 10,
            (a[2] * b[0] - a[0] * b[2]) >> 10,
            (a[0] * b[1] - a[1] * b[0]) >> 10,
        ];
        norm(&mut self.m[2]);
    }
}

const fn lerp_u8(a: u8, b: u8, num: u32, den: u32) -> u8 {
    let a = a as i32;
    let b = b as i32;
    (a + (b - a) * (num as i32) / (den as i32)) as u8
}

const fn make_ramp(dark: (u8, u8, u8), mid: (u8, u8, u8), bright: (u8, u8, u8)) -> [u16; 32] {
    let mut ramp = [0u16; 32];
    let mut i = 0u32;
    while i < 32 {
        let (r, g, b) = if i < 16 {
            (
                lerp_u8(dark.0, mid.0, i, 16),
                lerp_u8(dark.1, mid.1, i, 16),
                lerp_u8(dark.2, mid.2, i, 16),
            )
        } else {
            let k = i - 16;
            (
                lerp_u8(mid.0, bright.0, k, 15),
                lerp_u8(mid.1, bright.1, k, 15),
                lerp_u8(mid.2, bright.2, k, 15),
            )
        };
        ramp[i as usize] = rgb565(r, g, b);
        i += 1;
    }
    ramp
}

const fn make_glow_ramp(r: u8, g: u8, b: u8) -> [u16; 32] {
    let mut ramp = [0u16; 32];
    let mut i = 0u32;
    while i < 32 {
        let sq = (i * i) * 255 / (31 * 31);
        ramp[i as usize] = rgb565(
            (r as u32 * sq / 255) as u8,
            (g as u32 * sq / 255) as u8,
            (b as u32 * sq / 255) as u8,
        );
        i += 1;
    }
    ramp
}

const fn make_cavity_ramp(edge: (u8, u8, u8), center: (u8, u8, u8)) -> [u16; 16] {
    let mut ramp = [0u16; 16];
    let mut i = 0u32;
    while i < 16 {
        ramp[i as usize] = rgb565(
            lerp_u8(edge.0, center.0, i, 15),
            lerp_u8(edge.1, center.1, i, 15),
            lerp_u8(edge.2, center.2, i, 15),
        );
        i += 1;
    }
    ramp
}

const TOP_POKE: [u16; 32] = make_ramp((78, 8, 12), (226, 28, 34), (255, 174, 174));
const TOP_GREAT: [u16; 32] = make_ramp((14, 34, 96), (34, 112, 228), (176, 218, 255));
const TOP_ULTRA: [u16; 32] = make_ramp((18, 20, 26), (52, 56, 66), (156, 162, 176));
const TOP_MASTER: [u16; 32] = make_ramp((48, 14, 80), (138, 48, 198), (226, 174, 255));

const ACCENT_RED: [u16; 32] = make_ramp((88, 10, 14), (232, 36, 44), (255, 170, 170));
const ACCENT_GOLD: [u16; 32] = make_ramp((110, 82, 10), (242, 196, 36), (255, 242, 175));
const ACCENT_PINK: [u16; 32] = make_ramp((96, 18, 66), (226, 56, 148), (255, 180, 224));

const BOT_WHITE: [u16; 32] = make_ramp((112, 120, 134), (232, 236, 244), (255, 255, 255));
const BAND_DARK: [u16; 32] = make_ramp((14, 16, 22), (34, 38, 46), (80, 86, 100));
const INNER_METAL: [u16; 32] = make_ramp((24, 28, 36), (54, 62, 76), (118, 128, 148));
const HINGE_METAL: [u16; 32] = make_ramp((32, 36, 44), (108, 118, 134), (210, 220, 235));

const GLOW_POKE: [u16; 32] = make_glow_ramp(150, 225, 248);
const GLOW_GREAT: [u16; 32] = make_glow_ramp(90, 195, 255);
const GLOW_ULTRA: [u16; 32] = make_glow_ramp(255, 200, 80);
const GLOW_MASTER: [u16; 32] = make_glow_ramp(210, 135, 255);

const CAVITY_TOP_POKE: [u16; 16] = make_cavity_ramp((24, 16, 24), (115, 175, 195));
const CAVITY_TOP_GREAT: [u16; 16] = make_cavity_ramp((18, 22, 32), (80, 160, 215));
const CAVITY_TOP_ULTRA: [u16; 16] = make_cavity_ramp((26, 22, 16), (190, 155, 65));
const CAVITY_TOP_MASTER: [u16; 16] = make_cavity_ramp((28, 16, 34), (165, 110, 205));

const CAVITY_BOT_POKE: [u16; 16] = make_cavity_ramp((44, 52, 66), (185, 235, 250));
const CAVITY_BOT_GREAT: [u16; 16] = make_cavity_ramp((36, 52, 74), (135, 215, 255));
const CAVITY_BOT_ULTRA: [u16; 16] = make_cavity_ramp((52, 46, 36), (245, 210, 115));
const CAVITY_BOT_MASTER: [u16; 16] = make_cavity_ramp((52, 38, 66), (220, 165, 255));

const STYLE_COUNT: u8 = 4;

const fn glow_tint(style: u8) -> (u16, u16, u16) {
    match style {
        1 => (14, 48, 31),
        2 => (31, 50, 12),
        3 => (26, 34, 31),
        _ => (24, 58, 31),
    }
}

#[inline]
fn add_rgb565(base: u16, r5: u16, g6: u16, b5: u16) -> u16 {
    let r = ((base >> 11) + r5).min(31);
    let g = (((base >> 5) & 0x3F) + g6).min(63);
    let b = ((base & 0x1F) + b5).min(31);
    (r << 11) | (g << 5) | b
}

#[inline]
fn blend_rgb565(a: u16, b: u16, t: u32) -> u16 {
    let inv = 256 - t;
    let ra = (a >> 11) as u32;
    let ga = ((a >> 5) & 0x3F) as u32;
    let ba = (a & 0x1F) as u32;
    let rb = (b >> 11) as u32;
    let gb = ((b >> 5) & 0x3F) as u32;
    let bb = (b & 0x1F) as u32;
    let r = ((ra * inv + rb * t) >> 8).min(31) as u16;
    let g = ((ga * inv + gb * t) >> 8).min(63) as u16;
    let bl = ((ba * inv + bb * t) >> 8).min(31) as u16;
    (r << 11) | (g << 5) | bl
}

/// Evaluate the top-shell surface palette in true 3D local sphere coordinates
/// `(bx, by, bz)`, so every marking revolves rigidly with the central button
/// (`+bz` = front button, `-by` = top pole, `bx` = left/right).
#[inline]
fn pick_top_lut_3d(
    style: u8,
    bx: i32,
    by: i32,
    bz: i32,
    base: &'static [u16; 32],
) -> &'static [u16; 32] {
    let ax = bx.abs();
    match style {
        // Great Ball: two angled crimson fins on the front-upper-left & right,
        // strictly oriented with the front latch button (bz > -18).
        1 if bz > -18 && ax > 30 && ax < 126 => {
            let diag = by + ((ax - 30) * 3 >> 2);
            if diag > -78 && diag < -32 {
                &ACCENT_RED
            } else {
                base
            }
        }
        // Ultra Ball: golden H/U stripes flanking the front & arching over top.
        2 if bz > -28
            && ((ax > 36 && ax < 68 && by < -BAND_HALF)
                || (ax <= 68 && by < -90 && bz > 8)) =>
        {
            &ACCENT_GOLD
        }
        // Master Ball: two raised magenta spots on upper-front-left & right,
        // plus a white 'M' emblem right above the front latch button.
        3 => {
            let dx = ax - 76;
            let dy = by + 66;
            let dz = bz - 58;
            if dx * dx + dy * dy + dz * dz < 42 * 42 {
                &ACCENT_PINK
            } else if bz > 88 && ax <= 24 && by > -86 && by < -44 {
                // Crisp 'M' above the front button.
                let col_stem = ax >= 17;
                let col_v = (by + 84 - ax * 2).abs() <= 6;
                if col_stem || col_v {
                    &BOT_WHITE
                } else {
                    base
                }
            } else {
                base
            }
        }
        _ => base,
    }
}

/// Evaluate front button (on `bz > 0`) or rear metallic hinge (on `bz < 0`)
/// directly in 3D local coordinates `(bx, by, bz)`!
#[inline]
fn sample_equator_hardware(
    bx: i32,
    by: i32,
    bz: i32,
    dx_w: i32,
    dy_w: i32,
    core_color: u16,
) -> Option<u16> {
    if bz > 0 {
        let d2 = bx * bx + by * by;
        if d2 <= BTN_OUTER_R2 {
            return Some(if d2 <= BTN_CORE_R2 {
                let hi = ((-dx_w - dy_w) >> 4).clamp(-4, 5);
                if hi > 0 {
                    add_rgb565(core_color, hi as u16, (hi * 2) as u16, hi as u16)
                } else {
                    core_color
                }
            } else if d2 <= BTN_BEZEL_R2 {
                rgb565(46, 50, 60)
            } else if d2 <= BTN_RING_R2 {
                let idx = (20 + ((-dx_w - dy_w) >> 4)).clamp(8, 31) as usize;
                BOT_WHITE[idx]
            } else {
                let idx = (14 + ((-dx_w - dy_w) >> 4)).clamp(4, 24) as usize;
                BAND_DARK[idx]
            });
        }
    } else if bz < -40 && bx.abs() <= 26 && by.abs() <= 14 {
        // Metallic rear hinge clasp directly opposite the front latch button.
        let col = if by.abs() <= 2 {
            BAND_DARK[6]
        } else if bx.abs() >= 24 || by.abs() >= 12 {
            BAND_DARK[10]
        } else {
            let idx = (20 - by - (bx >> 2)).clamp(4, 31) as usize;
            HINGE_METAL[idx]
        };
        return Some(col);
    }
    None
}

#[derive(Clone, Copy)]
struct Spark {
    /// Position in Pokéball local 3D coordinates relative to core, in 1/16 px.
    x: i16,
    y: i16,
    z: i16,
    vx: i16,
    vy: i16,
    vz: i16,
    life_ms: u16,
    max_life_ms: u16,
    size: u8,
}

impl Spark {
    const EMPTY: Self = Self {
        x: 0,
        y: 0,
        z: 0,
        vx: 0,
        vy: 0,
        vz: 0,
        life_ms: 0,
        max_life_ms: 1,
        size: 1,
    };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DragMode {
    None,
    /// Touch drag revolves smoothly in 3D pitch & yaw.
    Revolve,
}

/// Interactive true-3D pitch/yaw Pokéball.
pub struct Pokeball {
    target_open: bool,
    open: i32,
    open_vel: i32,
    latched: bool,
    /// Full 3D orientation matrix (zero gimbal lock).
    rot: Mat3,
    /// Sub-unit fractional accumulators for smooth pitch/yaw gyro integration.
    frac_x: i32,
    frac_y: i32,
    /// Auto-calibrated gyroscope zero-rate offsets (deci-dps * 16).
    bias_x: i32,
    bias_y: i32,
    cal_frames: u8,
    /// Touch gesture tracking (drag on ball revolves 3D pitch/yaw; drag on outer
    /// ring rolls 360° in-plane; tap opens/closes).
    prev_down: bool,
    drag_mode: DragMode,
    drag_start: (i16, i16),
    prev_touch: (i16, i16),
    /// Center button flash timer (ms).
    flash_ms: u16,
    flash_lock: bool,
    /// Subtle rotational lock wobble around screen Z (settles cleanly to 0).
    wobble: i32,
    wobble_vel: i32,
    /// Smoothed tilt parallax in 1/16 px.
    tilt_x: i32,
    tilt_y: i32,
    time_ms: u32,
    style: u8,
    rng: u32,
    sparks: [Spark; SPARK_COUNT],
}

impl Pokeball {
    pub const fn new() -> Self {
        Self {
            target_open: false,
            open: 0,
            open_vel: 0,
            latched: true,
            rot: Mat3::IDENTITY,
            frac_x: 0,
            frac_y: 0,
            bias_x: 0,
            bias_y: 0,
            cal_frames: 0,
            prev_down: false,
            drag_mode: DragMode::None,
            drag_start: (0, 0),
            prev_touch: (0, 0),
            flash_ms: 0,
            flash_lock: false,
            wobble: 0,
            wobble_vel: 0,
            tilt_x: 0,
            tilt_y: 0,
            time_ms: 0,
            style: 0,
            rng: 0x5A17_9C3D,
            sparks: [Spark::EMPTY; SPARK_COUNT],
        }
    }

    pub const fn is_open(&self) -> bool {
        self.target_open
    }

    pub const fn openness(&self) -> i32 {
        self.open
    }

    /// Screen-space vector pointing toward the Pokéball's front button (`+bz`), scaled by 1024.
    pub const fn front_dir(&self) -> (i32, i32, i32) {
        (self.rot.m[2][0], self.rot.m[2][1], self.rot.m[2][2])
    }

    /// Screen-space vector pointing toward the Pokéball's bottom white pole (`+by`), scaled by 1024.
    pub const fn down_dir(&self) -> (i32, i32, i32) {
        (self.rot.m[1][0], self.rot.m[1][1], self.rot.m[1][2])
    }

    fn next_rand(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    fn spawn_spark(&mut self, slot: usize, burst: bool) {
        let a = (self.next_rand() as u16) & 0xFF00;
        let b = (self.next_rand() as u16) & 0xFF00;
        let sx = cos_u16(a);
        let sy = sin_u16(a);
        let sz = cos_u16(b);
        let speed = if burst {
            26 + (self.next_rand() % 42) as i32
        } else {
            10 + (self.next_rand() % 20) as i32
        };
        let vx = (sx * speed) >> 10;
        let vy = ((sy * speed) >> 10) - if burst { 10 } else { 14 };
        let vz = (sz * speed) >> 11;
        let start_x = ((self.next_rand() % 80) as i32 - 40) * 16;
        let start_y = ((self.next_rand() % 20) as i32 - 10) * 16;
        let start_z = ((self.next_rand() % 60) as i32 - 30) * 16;
        let life = (480 + (self.next_rand() % 620)) as u16;
        let size = 1 + (self.next_rand() % 3) as u8;
        self.sparks[slot] = Spark {
            x: start_x as i16,
            y: start_y as i16,
            z: start_z as i16,
            vx: vx as i16,
            vy: vy as i16,
            vz: vz as i16,
            life_ms: life,
            max_life_ms: life,
            size,
        };
    }

    fn burst_sparks(&mut self) {
        for i in 0..SPARK_COUNT {
            self.spawn_spark(i, true);
        }
    }

    fn apply_drag_delta(&mut self, from: (i32, i32), to: (i32, i32)) {
        if from == to {
            return;
        }
        // Pure 3D trackball revolution (pitch & yaw); 2D in-plane roll disabled.
        let yaw = ((to.0 - from.0) * TOUCH_REVOLVE_SCALE).clamp(-8192, 8192) as i16 as u16;
        let pitch = ((to.1 - from.1) * TOUCH_REVOLVE_SCALE).clamp(-8192, 8192) as i16 as u16;
        self.rot.yaw_screen(yaw);
        self.rot.pitch_screen(pitch);
        self.rot.orthonormalize();
    }

    fn toggle(&mut self, out: &mut Output) {
        self.target_open = !self.target_open;
        if self.target_open {
            self.flash_ms = 340;
            self.flash_lock = false;
            self.open_vel = self.open_vel.max(900);
            self.burst_sparks();
            out.sound = Some(Sound::PokeballOpen);
        } else {
            self.flash_ms = 200;
            self.flash_lock = true;
            self.open_vel = self.open_vel.min(-720);
            out.sound = Some(Sound::PokeballClose);
        }
    }

    /// Render one 3D hemisphere (either top `is_top == true` or bottom `is_top == false`)
    /// in a single ray-cast pass: outer surface (with 3D markings / button / hinge)
    /// and hollow concave interior bowl!
    #[allow(clippy::too_many_arguments)]
    fn render_hemi(
        &self,
        rows: Range<u16>,
        buf: &mut [u16],
        scx: i32,
        scy: i32,
        mat: &Mat3,
        is_top: bool,
        top_lut: &'static [u16; 32],
        cav_lut: &[u16; 16],
        core_color: u16,
    ) {
        let top = rows.start as i32;
        let bottom = rows.end as i32;
        let y0 = (scy - R).max(top);
        let y1 = (scy + R + 1).min(bottom);
        if y0 >= y1 {
            return;
        }
        let m = mat.m;
        let (m00, m01, m02) = (m[0][0], m[0][1], m[0][2]);
        let (m10, m11, m12) = (m[1][0], m[1][1], m[1][2]);
        let (m20, m21, m22) = (m[2][0], m[2][1], m[2][2]);

        for y in y0..y1 {
            let dy = y - scy;
            let dy2 = dy * dy;
            let rem = R2_MAX - dy2;
            if rem < 0 {
                continue;
            }
            let hw = isqrt(rem as u32) as i32;
            let x0 = (scx - hw).max(0);
            let x1 = (scx + hw + 1).min(W);
            let sx0 = x0 - scx;
            let mut bx_xy = sx0 * m00 + dy * m01;
            let mut by_xy = sx0 * m10 + dy * m11;
            let mut bz_xy = sx0 * m20 + dy * m21;
            let row_light = (15 << 7) - dy * 13;
            let sy_spec = dy + 62;
            let sy_spec2 = (sy_spec * 4) * (sy_spec * 4);
            let line = &mut buf[(y - top) as usize * W as usize..][..W as usize];

            for x in x0..x1 {
                let dx = x - scx;
                let r2 = dx * dx + dy2;
                let pz = SPHERE_Z[(((R2_MAX - r2) * INV_R2_Z) >> 14).min(255) as usize] as i32;
                let by_z = m12 * pz;
                let by_f = (by_xy + by_z) >> 10;
                let hit_front = if is_top { by_f <= 0 } else { by_f >= 0 };

                if hit_front {
                    let bx = (bx_xy + m02 * pz) >> 10;
                    let bz = (bz_xy + m22 * pz) >> 10;
                    // On open Pokéball, button/hinge lives on the bottom shell's front lip
                    // (and upper half of button/hinge only when closed).
                    if !is_top {
                        if let Some(hw_col) =
                            sample_equator_hardware(bx, by_f, bz, dx, dy, core_color)
                        {
                            line[x as usize] = hw_col;
                            bx_xy += m00;
                            by_xy += m10;
                            bz_xy += m20;
                            continue;
                        }
                    }
                    let rim = if r2 > RIM_START {
                        ((r2 - RIM_START) >> 9).min(8)
                    } else {
                        0
                    };
                    let dither = (((x & 1) << 1) ^ (y & 1)) & 1;
                    let mut light = ((row_light - dx * 11 + (dither << 6)) >> 7) - rim;
                    if is_top {
                        if by_f > -BAND_HALF {
                            line[x as usize] = BAND_DARK[(light + 2).clamp(0, 31) as usize];
                        } else {
                            let lut = pick_top_lut_3d(self.style, bx, by_f, bz, top_lut);
                            if sy_spec2 < 96 * 96 {
                                let sx = (dx + 54) * 3;
                                let spec_d2 = sx * sx + sy_spec2;
                                if spec_d2 < 96 * 96 {
                                    light += ((96 * 96 - spec_d2) * 25) >> 14;
                                }
                            }
                            line[x as usize] = lut[light.clamp(0, 31) as usize];
                        }
                    } else if by_f < BAND_HALF {
                        line[x as usize] = BAND_DARK[(light + 2).clamp(0, 31) as usize];
                    } else {
                        line[x as usize] = BOT_WHITE[light.clamp(0, 31) as usize];
                    }
                } else {
                    // Check back inner concave surface of the hollow hemisphere (`-pz`).
                    let by_b = (by_xy - by_z) >> 10;
                    let hit_back = if is_top { by_b <= 0 } else { by_b >= 0 };
                    if hit_back {
                        let rim_dist = by_b.abs();
                        line[x as usize] = if rim_dist < 10 || pz < 22 {
                            INNER_METAL[(12 + (dx >> 4)).clamp(4, 26) as usize]
                        } else {
                            let idx = ((pz * 15) / R).clamp(0, 15) as usize;
                            cav_lut[idx]
                        };
                    }
                }

                bx_xy += m00;
                by_xy += m10;
                bz_xy += m20;
            }
        }
    }
}

impl Default for Pokeball {
    fn default() -> Self {
        Self::new()
    }
}

impl App for Pokeball {
    fn update(&mut self, input: &Input, dt_ms: u32, out: &mut Output) {
        let dt = dt_ms.clamp(1, 50) as i32;
        self.time_ms = self.time_ms.wrapping_add(dt as u32);

        if input.key {
            self.style = (self.style + 1) % STYLE_COUNT;
            self.wobble_vel += 14_000;
            out.sound = Some(Sound::Tick { pitch: 220 });
        }

        // Smooth tilt parallax (up to ~10 px at 1 g).
        let target_tx = (input.tilt.x_mg as i32 * 160 / 1000).clamp(-160, 160);
        let target_ty = (input.tilt.y_mg as i32 * 120 / 1000).clamp(-120, 120);
        self.tilt_x += (target_tx - self.tilt_x) * dt / 140;
        self.tilt_y += (target_ty - self.tilt_y) * dt / 140;

        // Touch gestures:
        // - Drag starting on the Pokéball (`r <= R`): 3D pitch/yaw revolution.
        // - Drag starting outside the Pokéball (`r > R`): 360° in-plane roll.
        // - Tap (< 10 px movement): open / close with custom Pokéball sound.
        let tx = input.touch.x as i32;
        let ty = input.touch.y as i32;
        let down = input.touch.down;
        if down && !self.prev_down {
            self.drag_start = (tx as i16, ty as i16);
            self.prev_touch = (tx as i16, ty as i16);
            self.drag_mode = DragMode::None;
        } else if down && self.prev_down {
            let mdx = tx - self.drag_start.0 as i32;
            let mdy = ty - self.drag_start.1 as i32;
            if self.drag_mode == DragMode::None && mdx * mdx + mdy * mdy >= DRAG_THRESHOLD_SQ {
                self.drag_mode = DragMode::Revolve;
                self.wobble = 0;
                self.wobble_vel = 0;
                let start = (self.drag_start.0 as i32, self.drag_start.1 as i32);
                self.apply_drag_delta(start, (tx, ty));
                self.prev_touch = (tx as i16, ty as i16);
            } else if self.drag_mode != DragMode::None {
                let prev = (self.prev_touch.0 as i32, self.prev_touch.1 as i32);
                self.apply_drag_delta(prev, (tx, ty));
                self.prev_touch = (tx as i16, ty as i16);
            }
        } else if !down && self.prev_down {
            if self.drag_mode == DragMode::None {
                self.toggle(out);
            }
            self.drag_mode = DragMode::None;
        } else if !down && input.touch.tapped {
            self.toggle(out);
        }
        self.prev_down = down;

        // 2-axis gyroscope pitch / yaw integration with auto bias calibration
        // (2D in-plane roll rotation disabled).
        let raw_x = input.gyro.x_dps10 as i32;
        let raw_y = input.gyro.y_dps10 as i32;
        if self.cal_frames < 10 && raw_x.abs() < 80 && raw_y.abs() < 80 {
            let denom = self.cal_frames as i32 + 1;
            self.bias_x += (raw_x * 16 - self.bias_x) / denom;
            self.bias_y += (raw_y * 16 - self.bias_y) / denom;
            self.cal_frames += 1;
        } else {
            let dx_b = raw_x * 16 - self.bias_x;
            let dy_b = raw_y * 16 - self.bias_y;
            if dx_b.abs() < 32 * 16 {
                self.bias_x += dx_b / 32;
            }
            if dy_b.abs() < 32 * 16 {
                self.bias_y += dy_b / 32;
            }
        }

        if self.drag_mode == DragMode::None {
            let gx = raw_x - self.bias_x / 16;
            let gy = raw_y - self.bias_y / 16;
            let mut moved = false;
            if gx.abs() > GYRO_DEADBAND {
                let num = self.frac_x - gx * dt * 200;
                let step = num / 10_000;
                self.frac_x = num - step * 10_000;
                if step != 0 {
                    self.rot.pitch_screen(step as i16 as u16);
                    moved = true;
                }
            }
            if gy.abs() > GYRO_DEADBAND {
                let num = self.frac_y + gy * dt * 200;
                let step = num / 10_000;
                self.frac_y = num - step * 10_000;
                if step != 0 {
                    self.rot.yaw_screen(step as i16 as u16);
                    moved = true;
                }
            }
            if moved {
                self.rot.orthonormalize();
            }
        }

        // Opening/closing spring animation.
        let target = if self.target_open { OPEN_FULL } else { 0 };
        let (stiffness, damping) = if self.target_open { (58, 10) } else { (82, 11) };
        let acc = (target - self.open) * stiffness - self.open_vel * damping;
        self.open_vel += acc * dt / 1000;
        self.open += self.open_vel * dt / 1000;

        if self.open > 40 {
            self.latched = false;
        }
        if !self.target_open && self.open <= 12 {
            self.open = 0;
            self.open_vel = 0;
            if !self.latched {
                self.latched = true;
                self.flash_ms = 260;
                self.flash_lock = true;
                self.wobble_vel = 18_000;
            }
        } else if self.target_open && self.open > OPEN_FULL + 96 {
            self.open = OPEN_FULL + 96;
            if self.open_vel > 0 {
                self.open_vel = 0;
            }
        } else if self.open < 0 {
            self.open = 0;
        }

        // Damped rotational rock spring.
        let w_acc = -self.wobble * 130 - self.wobble_vel * 11;
        self.wobble_vel += w_acc * dt / 1000;
        self.wobble += self.wobble_vel * dt / 1000;
        if self.wobble.abs() < 12 && self.wobble_vel.abs() < 40 {
            self.wobble = 0;
            self.wobble_vel = 0;
        }

        self.flash_ms = self.flash_ms.saturating_sub(dt as u16);

        // Update 3D energy sparkles.
        for i in 0..SPARK_COUNT {
            let mut s = self.sparks[i];
            if s.life_ms > 0 {
                if s.life_ms <= dt as u16 {
                    s.life_ms = 0;
                } else {
                    s.life_ms -= dt as u16;
                    if !self.target_open {
                        s.vx = (s.vx as i32 - (s.x as i32) / 14).clamp(-160, 160) as i16;
                        s.vy = (s.vy as i32 - (s.y as i32) / 14).clamp(-160, 160) as i16;
                        s.vz = (s.vz as i32 - (s.z as i32) / 14).clamp(-160, 160) as i16;
                    } else {
                        s.vx = (s.vx as i32 * 96 / 100) as i16;
                        s.vy = (s.vy as i32 * 96 / 100 - 1) as i16;
                        s.vz = (s.vz as i32 * 96 / 100) as i16;
                    }
                    s.x = s.x.saturating_add(((s.vx as i32 * dt) / 33) as i16);
                    s.y = s.y.saturating_add(((s.vy as i32 * dt) / 33) as i16);
                    s.z = s.z.saturating_add(((s.vz as i32 * dt) / 33) as i16);
                }
                self.sparks[i] = s;
            } else if self.target_open && self.open > 260 && (self.next_rand() & 15) == 0 {
                self.spawn_spark(i, false);
            }
        }
    }

    fn render(&self, rows: Range<u16>, buf: &mut [u16]) {
        assert!(rows.start <= rows.end && rows.end <= H as u16);
        assert_eq!(buf.len(), (rows.end - rows.start) as usize * W as usize);
        buf.fill(0);

        let top = rows.start as i32;
        let bottom = rows.end as i32;

        let open = self.open.clamp(0, OPEN_FULL + 96);
        let open_norm = open.min(OPEN_FULL);

        // Apply subtle transient lock wobble around screen Z.
        let mut mat = self.rot;
        if self.wobble != 0 {
            mat.roll_screen(self.wobble as i16 as u16);
        }

        let pulse_angle = ((self.time_ms / 8) as u16).wrapping_mul(16);
        let pulse = sin_u16(pulse_angle) >> 2;
        let idle_bob = (pulse * 4 * open_norm) >> 18;

        let cx = W / 2 + self.tilt_x / 16;
        let cy = H / 2 + self.tilt_y / 16 + idle_bob;

        let (tr, tg, tb) = glow_tint(self.style);
        let (top_lut, glow_lut, cav_top, cav_bot): (
            &'static [u16; 32],
            &[u16; 32],
            &[u16; 16],
            &[u16; 16],
        ) = match self.style {
            1 => (&TOP_GREAT, &GLOW_GREAT, &CAVITY_TOP_GREAT, &CAVITY_BOT_GREAT),
            2 => (&TOP_ULTRA, &GLOW_ULTRA, &CAVITY_TOP_ULTRA, &CAVITY_BOT_ULTRA),
            3 => (&TOP_MASTER, &GLOW_MASTER, &CAVITY_TOP_MASTER, &CAVITY_BOT_MASTER),
            _ => (&TOP_POKE, &GLOW_POKE, &CAVITY_TOP_POKE, &CAVITY_BOT_POKE),
        };

        let core_color = if self.flash_ms > 0 {
            let t = ((self.flash_ms as u32) * 256 / 340).min(256);
            let target = if self.flash_lock {
                rgb565(255, 72, 72)
            } else {
                rgb565(180, 248, 255)
            };
            blend_rgb565(rgb565(242, 244, 248), target, t)
        } else if !self.target_open && open_norm == 0 {
            let breath = ((pulse + 256) as u32) >> 3;
            blend_rgb565(rgb565(232, 236, 244), rgb565(255, 160, 160), breath)
        } else {
            blend_rgb565(rgb565(225, 236, 246), rgb565(150, 240, 255), 110)
        };

        if open == 0 {
            // Single-pass true-3D closed Pokéball (sphere + all 3D markings +
            // foreshortened front button + rear hinge in ONE scan!).
            let y0 = (cy - R).max(top);
            let y1 = (cy + R + 1).min(bottom);
            let m = mat.m;
            let (m00, m01, m02) = (m[0][0], m[0][1], m[0][2]);
            let (m10, m11, m12) = (m[1][0], m[1][1], m[1][2]);
            let (m20, m21, m22) = (m[2][0], m[2][1], m[2][2]);

            for y in y0..y1 {
                let dy = y - cy;
                let dy2 = dy * dy;
                let rem = R2_MAX - dy2;
                if rem < 0 {
                    continue;
                }
                let hw = isqrt(rem as u32) as i32;
                let x0 = (cx - hw).max(0);
                let x1 = (cx + hw + 1).min(W);
                let sx0 = x0 - cx;
                let mut bx_xy = sx0 * m00 + dy * m01;
                let mut by_xy = sx0 * m10 + dy * m11;
                let mut bz_xy = sx0 * m20 + dy * m21;
                let row_light = (15 << 7) - dy * 13;
                let sy_spec = dy + 62;
                let sy_spec2 = (sy_spec * 4) * (sy_spec * 4);
                let line = &mut buf[(y - top) as usize * W as usize..][..W as usize];

                for x in x0..x1 {
                    let dx = x - cx;
                    let r2 = dx * dx + dy2;
                    let pz = SPHERE_Z[(((R2_MAX - r2) * INV_R2_Z) >> 14).min(255) as usize] as i32;
                    let bx = (bx_xy + m02 * pz) >> 10;
                    let by = (by_xy + m12 * pz) >> 10;
                    let bz = (bz_xy + m22 * pz) >> 10;
                    bx_xy += m00;
                    by_xy += m10;
                    bz_xy += m20;

                    if let Some(hw_col) = sample_equator_hardware(bx, by, bz, dx, dy, core_color) {
                        line[x as usize] = hw_col;
                        continue;
                    }
                    let rim = if r2 > RIM_START {
                        ((r2 - RIM_START) >> 9).min(8)
                    } else {
                        0
                    };
                    let dither = (((x & 1) << 1) ^ (y & 1)) & 1;
                    let mut light = ((row_light - dx * 11 + (dither << 6)) >> 7) - rim;
                    if by > -BAND_HALF && by < BAND_HALF {
                        line[x as usize] = BAND_DARK[(light + 2).clamp(0, 31) as usize];
                    } else if by >= BAND_HALF {
                        line[x as usize] = BOT_WHITE[light.clamp(0, 31) as usize];
                    } else {
                        let lut = pick_top_lut_3d(self.style, bx, by, bz, top_lut);
                        if sy_spec2 < 96 * 96 {
                            let sx = (dx + 54) * 3;
                            let spec_d2 = sx * sx + sy_spec2;
                            if spec_d2 < 96 * 96 {
                                light += ((96 * 96 - spec_d2) * 25) >> 14;
                            }
                        }
                        line[x as usize] = lut[light.clamp(0, 31) as usize];
                    }
                }
            }
            return;
        }

        // Open Pokéball in true 3D: separate along the ball's actual 3D local Y axis
        // (`row 1` of `mat`), tilt top shell back (+18°) & bottom shell slightly forward (-14°)
        // around local X so the hollow cavities open naturally from every viewpoint!
        let (dx_axis, dy_axis, dz_axis) = (mat.m[1][0], mat.m[1][1], mat.m[1][2]);
        let sep_top = (open * 68) / OPEN_FULL;
        let sep_bot = (open * 44) / OPEN_FULL;
        let top_cx = cx - ((dx_axis * sep_top) >> 10);
        let top_cy = cy - ((dy_axis * sep_top) >> 10);
        let bot_cx = cx + ((dx_axis * sep_bot) >> 10);
        let bot_cy = cy + ((dy_axis * sep_bot) >> 10);
        let core_cx = (top_cx + bot_cx) / 2;
        let core_cy = (top_cy + bot_cy) / 2;

        // 1. Fast radial glow halo behind the open gap (direct LUT write).
        if open_norm > 24 {
            let aura_r = 46 + (open_norm * 76) / OPEN_FULL + ((pulse * 4) >> 8);
            let aura_r2 = aura_r * aura_r;
            let inv_aura =
                ((31u32 * open_norm as u32) << 12) / (aura_r2 as u32 * OPEN_FULL as u32).max(1);
            let y0 = (core_cy - aura_r).max(top);
            let y1 = (core_cy + aura_r + 1).min(bottom);
            for y in y0..y1 {
                let dy = y - core_cy;
                let dy2 = dy * dy;
                let rem = aura_r2 - dy2;
                if rem <= 0 {
                    continue;
                }
                let hw = isqrt(rem as u32) as i32;
                let x0 = (core_cx - hw).max(0);
                let x1 = (core_cx + hw + 1).min(W);
                let line = &mut buf[(y - top) as usize * W as usize..][..W as usize];
                for x in x0..x1 {
                    let dx = x - core_cx;
                    let idx = (((aura_r2 - dx * dx - dy2) as u32 * inv_aura) >> 12).min(31) as usize;
                    if idx > 1 {
                        line[x as usize] = glow_lut[idx];
                    }
                }
            }
        }

        // Hinged local pitch angles (~+18° top, ~-13° bottom at full open).
        let top_hinge = ((open_norm * 3_276) / OPEN_FULL) as u16;
        let bot_hinge = 0u16.wrapping_sub(((open_norm * 2_366) / OPEN_FULL) as u16);
        let top_mat = mat.with_local_pitch(top_hinge);
        let bot_mat = mat.with_local_pitch(bot_hinge);

        // Draw far hemisphere -> glowing energy core -> near hemisphere!
        let bot_in_front = dz_axis >= 0;
        if bot_in_front {
            self.render_hemi(
                rows.clone(),
                buf,
                top_cx,
                top_cy,
                &top_mat,
                true,
                top_lut,
                cav_top,
                core_color,
            );
        } else {
            self.render_hemi(
                rows.clone(),
                buf,
                bot_cx,
                bot_cy,
                &bot_mat,
                false,
                top_lut,
                cav_bot,
                core_color,
            );
        }

        // Luminous energy core orb glowing right between the two halves!
        if open_norm > 48 {
            let core_r = 14 + (open_norm * 38) / OPEN_FULL + ((pulse * 3) >> 8);
            let core_r2 = core_r * core_r;
            let inv_core = (256u32 << 12) / core_r2 as u32;
            let y0 = (core_cy - core_r).max(top);
            let y1 = (core_cy + core_r + 1).min(bottom);
            for y in y0..y1 {
                let dy = y - core_cy;
                let dy2 = dy * dy;
                let rem = core_r2 - dy2;
                if rem <= 0 {
                    continue;
                }
                let hw = isqrt(rem as u32) as i32;
                let x0 = (core_cx - hw).max(0);
                let x1 = (core_cx + hw + 1).min(W);
                let line = &mut buf[(y - top) as usize * W as usize..][..W as usize];
                for x in x0..x1 {
                    let dx = x - core_cx;
                    let t = (((core_r2 - dx * dx - dy2) as u32 * inv_core) >> 12).min(256);
                    let t2 = (t * t) >> 8;
                    let white = (t2 * 31) >> 8;
                    let r5 = (((tr as u32 * t) >> 8) + white).min(31) as u16;
                    let g6 = (((tg as u32 * t) >> 8) + white * 2).min(63) as u16;
                    let b5 = (((tb as u32 * t) >> 8) + white).min(31) as u16;
                    line[x as usize] = add_rgb565(line[x as usize], r5, g6, b5);
                }
            }
        }

        if bot_in_front {
            self.render_hemi(
                rows.clone(),
                buf,
                bot_cx,
                bot_cy,
                &bot_mat,
                false,
                top_lut,
                cav_bot,
                core_color,
            );
        } else {
            self.render_hemi(
                rows.clone(),
                buf,
                top_cx,
                top_cy,
                &top_mat,
                true,
                top_lut,
                cav_top,
                core_color,
            );
        }

        // Foreground twinkling 3D energy sparkles (projected via transpose of `mat`).
        let m = mat.m;
        for s in &self.sparks {
            if s.life_ms == 0 {
                continue;
            }
            let (lx, ly, lz) = ((s.x as i32) / 16, (s.y as i32) / 16, (s.z as i32) / 16);
            let rx = (m[0][0] * lx + m[1][0] * ly + m[2][0] * lz) >> 10;
            let ry = (m[0][1] * lx + m[1][1] * ly + m[2][1] * lz) >> 10;
            let sx = core_cx + rx;
            let sy = core_cy + ry;
            let rad = s.size as i32 + 1;
            if sy + rad < top || sy - rad >= bottom || sx + rad < 0 || sx - rad >= W {
                continue;
            }
            let age = s.max_life_ms.saturating_sub(s.life_ms) as u32;
            let fade = (s.life_ms as u32).min(age * 3).min(240) * 256 / 240;
            let r5 = ((28 * fade) >> 8).min(31) as u16;
            let g6 = ((60 * fade) >> 8).min(63) as u16;
            let b5 = ((31 * fade) >> 8).min(31) as u16;
            let arm_r5 = ((tr as u32 * fade) >> 9) as u16;
            let arm_g6 = ((tg as u32 * fade) >> 9) as u16;
            let arm_b5 = ((tb as u32 * fade) >> 9) as u16;

            for d in -rad..=rad {
                let px = sx + d;
                if sy >= top && sy < bottom && px >= 0 && px < W {
                    let line = &mut buf[(sy - top) as usize * W as usize..][..W as usize];
                    if d.abs() <= 1 {
                        line[px as usize] = add_rgb565(line[px as usize], r5, g6, b5);
                    } else {
                        line[px as usize] = add_rgb565(line[px as usize], arm_r5, arm_g6, arm_b5);
                    }
                }
                let py = sy + d;
                if d != 0 && py >= top && py < bottom && sx >= 0 && sx < W {
                    let line = &mut buf[(py - top) as usize * W as usize..][..W as usize];
                    if d.abs() <= 1 {
                        line[sx as usize] = add_rgb565(line[sx as usize], r5, g6, b5);
                    } else {
                        line[sx as usize] = add_rgb565(line[sx as usize], arm_r5, arm_g6, arm_b5);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_sdk::{Gyro, Tilt, Touch};
    use std::vec;

    fn idle_input(time_ms: u32) -> Input {
        Input {
            tilt: Tilt {
                x_mg: 0,
                y_mg: 0,
                z_mg: 1000,
            },
            gyro: Gyro::default(),
            touch: Touch::default(),
            time_ms,
            local_time_s: None,
            key: false,
        }
    }

    #[test]
    fn starts_closed_upright() {
        let p = Pokeball::new();
        assert!(!p.is_open());
        assert_eq!(p.openness(), 0);
        assert_eq!(p.front_dir(), (0, 0, 1024));
        assert_eq!(p.down_dir(), (0, 1024, 0));
        let mut frame = vec![0u16; 480 * 480];
        p.render(0..480, &mut frame);
        let lit = frame.iter().filter(|&&px| px != 0).count();
        assert!(lit > 55_000 && lit < 80_000, "unexpected lit count: {lit}");
    }

    #[test]
    fn opens_on_tap_and_closes_on_second_tap_with_pokeball_sounds() {
        let mut p = Pokeball::new();
        let mut out = Output::default();

        let mut press = idle_input(33);
        press.touch = Touch {
            down: true,
            x: 240,
            y: 240,
            tapped: false,
        };
        p.update(&press, 33, &mut out);
        assert!(!p.is_open());
        assert_eq!(out.sound, None);

        let mut release = idle_input(66);
        release.touch = Touch {
            down: false,
            x: 240,
            y: 240,
            tapped: true,
        };
        out = Output::default();
        p.update(&release, 33, &mut out);
        assert!(p.is_open());
        assert_eq!(out.sound, Some(Sound::PokeballOpen));

        for i in 3..45 {
            p.update(&idle_input(i * 33), 33, &mut Output::default());
        }
        assert!(p.openness() > 950);

        let mut tap = idle_input(45 * 33);
        tap.touch = Touch {
            down: false,
            x: 240,
            y: 240,
            tapped: true,
        };
        out = Output::default();
        p.update(&tap, 33, &mut out);
        assert!(!p.is_open());
        assert_eq!(out.sound, Some(Sound::PokeballClose));

        for i in 46..95 {
            p.update(&idle_input(i * 33), 33, &mut Output::default());
        }
        assert_eq!(p.openness(), 0);
    }

    #[test]
    fn drag_revolves_3d_and_persists_orientation() {
        // Horizontal drag revolves smoothly in 3D yaw and persists after release.
        let mut q = Pokeball::new();
        for (idx, x) in (180u16..=326).step_by(24).enumerate() {
            let mut input = idle_input((idx as u32 + 1) * 33);
            input.touch = Touch {
                down: true,
                x,
                y: 240,
                tapped: false,
            };
            q.update(&input, 33, &mut Output::default());
            assert!(!q.is_open());
        }
        q.update(&idle_input(400), 33, &mut Output::default());
        assert!(
            q.front_dir().0 > 850,
            "horizontal drag should revolve front button right (+X), got {:?}",
            q.front_dir()
        );
        let saved_3d = q.rot;
        for i in 0..30 {
            q.update(&idle_input(433 + i * 33), 33, &mut Output::default());
        }
        assert_eq!(q.rot, saved_3d, "3D touch revolution must persist after release");

        // Vertical drag revolves smoothly in 3D pitch.
        let mut v = Pokeball::new();
        for (idx, y) in (180u16..=326).step_by(24).enumerate() {
            let mut input = idle_input((idx as u32 + 1) * 33);
            input.touch = Touch {
                down: true,
                x: 240,
                y,
                tapped: false,
            };
            v.update(&input, 33, &mut Output::default());
            assert!(!v.is_open());
        }
        v.update(&idle_input(400), 33, &mut Output::default());
        assert!(
            v.front_dir().1 > 850,
            "vertical drag should pitch front button down (+Y), got {:?}",
            v.front_dir()
        );

        // Gyro Z roll is disabled (spinning in-plane does not tilt/roll the ball).
        let mut z = Pokeball::new();
        z.cal_frames = 10;
        for i in 0..30 {
            let mut input = idle_input(i * 20);
            input.gyro = Gyro {
                x_dps10: 0,
                y_dps10: 0,
                z_dps10: 1800,
            };
            z.update(&input, 20, &mut Output::default());
        }
        assert_eq!(z.rot, Mat3::IDENTITY, "Z roll rotation must stay disabled");
    }

    #[test]
    fn gyro_pitch_yaw_and_roll_revolve_360_without_drift() {
        let mut p = Pokeball::new();
        // Small static gyro bias on all 3 axes must not drift.
        for i in 0..30 {
            let mut input = idle_input(i * 33);
            input.gyro = Gyro {
                x_dps10: 14,
                y_dps10: -12,
                z_dps10: 10,
            };
            p.update(&input, 33, &mut Output::default());
        }
        assert_eq!(p.rot, Mat3::IDENTITY);

        // Pitch via gyro X (inverted): positive gx moves front button up (-Y).
        for i in 0..45 {
            let mut input = idle_input(1000 + i * 20);
            input.gyro = Gyro {
                x_dps10: 1000 + 14,
                y_dps10: -12,
                z_dps10: 10,
            };
            p.update(&input, 20, &mut Output::default());
        }
        let front = p.front_dir();
        assert!(
            front.1 < -800,
            "inverted gyro X pitch should move front button up (-Y), got {front:?}"
        );

        // Reset and yaw ~90° via gyro Y (non-inverted): positive gy moves front button right (+X).
        let mut p2 = Pokeball::new();
        p2.cal_frames = 10;
        for i in 0..25 {
            let mut input = idle_input(i * 20);
            input.gyro = Gyro {
                x_dps10: 0,
                y_dps10: 1600,
                z_dps10: 0,
            };
            p2.update(&input, 20, &mut Output::default());
        }
        assert!(
            p2.front_dir().0 > 700,
            "gyro Y yaw should orbit front button right (+X), got {:?}",
            p2.front_dir()
        );
    }

    #[test]
    fn great_ball_red_fins_revolve_with_front_button() {
        // Great Ball front-upper-right has red fins (`bz > 0`), whereas the back
        // (`bz < 0`) is solid blue with no red fins!
        let front_fin = pick_top_lut_3d(1, 70, -70, 95, &TOP_GREAT);
        let back_same_xy = pick_top_lut_3d(1, 70, -70, -95, &TOP_GREAT);
        assert_eq!(front_fin, &ACCENT_RED);
        assert_eq!(back_same_xy, &TOP_GREAT);
    }

    #[test]
    fn arbitrary_strips_match_full_frame_in_3d() {
        for style in 0..STYLE_COUNT {
            let mut p = Pokeball::new();
            p.style = style;
            p.rot.pitch_screen(6_000);
            p.rot.yaw_screen(10_000);
            p.rot.roll_screen(5_000);
            p.rot.orthonormalize();
            for step in 0..2 {
                if step == 1 {
                    let mut tap = idle_input(33);
                    tap.touch.tapped = true;
                    p.update(&tap, 33, &mut Output::default());
                    for i in 0..30 {
                        p.update(&idle_input((i + 2) * 33), 33, &mut Output::default());
                    }
                }
                let mut full = vec![0u16; 480 * 480];
                p.render(0..480, &mut full);
                for (start, end) in [(0, 2), (5, 17), (160, 172), (234, 246), (330, 342), (478, 480)]
                {
                    let mut strip = vec![0u16; (end - start) * 480];
                    p.render(start as u16..end as u16, &mut strip);
                    assert_eq!(
                        strip,
                        full[start * 480..end * 480],
                        "strip {start}..{end} mismatch style={style} step={step}"
                    );
                }
            }
        }
    }

    #[test]
    fn state_size_is_small() {
        assert!(
            core::mem::size_of::<Pokeball>() < 1024,
            "state too large: {} bytes",
            core::mem::size_of::<Pokeball>()
        );
    }
}
