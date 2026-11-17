//! Halftone blobs: soft blobs that drift, merge and split, drawn as a field
//! of halftone dots. Tilt pours them toward the low side; a finger pushes
//! them away; a tap changes the palette.
#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use app_sdk::{Input, Output, Tilt, Touch};
    use std::vec;

    fn run(app: &mut Blobs, input: &mut Input, frames: u32) {
        for _ in 0..frames {
            input.time_ms = input.time_ms.wrapping_add(33);
            app.update(input, 33, &mut Output::default());
        }
    }

    #[test]
    fn something_shows_and_not_everything() {
        let mut app = Blobs::new();
        run(&mut app, &mut Input::default(), 30);
        let lit = app.dots.iter().filter(|d| **d != 0).count();
        assert!(lit > 50 && lit < CELLS * 3 / 4, "{lit}");
    }

    #[test]
    fn tilt_pours_them_to_the_low_side() {
        let mut app = Blobs::new();
        let mut input = Input { tilt: Tilt { x_mg: 700, y_mg: 0, z_mg: 700 }, ..Input::default() };
        run(&mut app, &mut input, 150);
        let right = (0..CELLS).filter(|i| i % GRID >= GRID / 2 && app.dots[*i] != 0).count();
        let left = (0..CELLS).filter(|i| i % GRID < GRID / 2 && app.dots[*i] != 0).count();
        assert!(right > left * 2, "right {right} left {left}");
        for b in &app.balls {
            assert!((0..480 * 256).contains(&b.x) && (0..480 * 256).contains(&b.y));
        }
    }

    #[test]
    fn tap_cycles_palette_with_a_click() {
        let mut app = Blobs::new();
        let mut out = Output::default();
        let input = Input { touch: Touch { tapped: true, ..Touch::default() }, ..Input::default() };
        app.update(&input, 33, &mut out);
        assert_eq!(out.sound, Some(app_sdk::Sound::Click));
        assert_eq!(app.palette, 1);
    }

    #[test]
    fn extreme_inputs_do_not_panic() {
        let mut app = Blobs::new();
        let mut input = Input {
            tilt: Tilt { x_mg: i16::MIN, y_mg: i16::MAX, z_mg: 0 },
            touch: Touch { down: true, x: u16::MAX, y: u16::MAX, tapped: false },
            ..Input::default()
        };
        run(&mut app, &mut input, 200);
        app.update(&input, u32::MAX, &mut Output::default());
        app.render(0..480, &mut vec![0; 480 * 480]);
    }

    #[test]
    fn arbitrary_strips_match_full_frame() {
        let mut app = Blobs::new();
        run(&mut app, &mut Input::default(), 20);
        let mut full = vec![0; 480 * 480];
        app.render(0..480, &mut full);
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

// Halftone dots: ten-bit rows, leftmost pixel in bit 9, discs growing from a
// 2 px dot to a full cell. 
const GLYPHS: [[u16; 10]; 11] = [
    [0b0000000000, 0b0000000000, 0b0000000000, 0b0000000000, 0b0000110000, 0b0000110000, 0b0000000000, 0b0000000000, 0b0000000000, 0b0000000000],
    [0b0000000000, 0b0000000000, 0b0000000000, 0b0000110000, 0b0001111000, 0b0001111000, 0b0000110000, 0b0000000000, 0b0000000000, 0b0000000000],
    [0b0000000000, 0b0000000000, 0b0000000000, 0b0001111000, 0b0001111000, 0b0001111000, 0b0001111000, 0b0000000000, 0b0000000000, 0b0000000000],
    [0b0000000000, 0b0000000000, 0b0000110000, 0b0001111000, 0b0011111100, 0b0011111100, 0b0001111000, 0b0000110000, 0b0000000000, 0b0000000000],
    [0b0000000000, 0b0000000000, 0b0001111000, 0b0011111100, 0b0011111100, 0b0011111100, 0b0011111100, 0b0001111000, 0b0000000000, 0b0000000000],
    [0b0000000000, 0b0000110000, 0b0011111100, 0b0011111100, 0b0111111110, 0b0111111110, 0b0011111100, 0b0011111100, 0b0000110000, 0b0000000000],
    [0b0000000000, 0b0001111000, 0b0011111100, 0b0111111110, 0b0111111110, 0b0111111110, 0b0111111110, 0b0011111100, 0b0001111000, 0b0000000000],
    [0b0000000000, 0b0011111100, 0b0111111110, 0b0111111110, 0b0111111110, 0b0111111110, 0b0111111110, 0b0111111110, 0b0011111100, 0b0000000000],
    [0b0001111000, 0b0011111100, 0b0111111110, 0b1111111111, 0b1111111111, 0b1111111111, 0b1111111111, 0b0111111110, 0b0011111100, 0b0001111000],
    [0b0011111100, 0b0111111110, 0b1111111111, 0b1111111111, 0b1111111111, 0b1111111111, 0b1111111111, 0b1111111111, 0b0111111110, 0b0011111100],
    [0b1111111111, 0b1111111111, 0b1111111111, 0b1111111111, 0b1111111111, 0b1111111111, 0b1111111111, 0b1111111111, 0b1111111111, 0b1111111111],
];
/// Each palette is a (base, accent) pair. Accent differs from base in both
/// hue and lightness so the two read apart on black. Each colour is expanded
/// into `SHADES` brightness steps so sparse glyphs glow dimly and dense ones
/// are bright, which gives the field depth.

const CELL: i32 = 12;
const GRID: usize = 40;
const CELLS: usize = GRID * GRID;
const BALLS: usize = 5;
/// Blob size: a lone blob's field reaches the dot threshold about this far out, px.
const REACH: i32 = 58;
/// How far a blob's target wanders from the centre, px, and how far tilt
/// moves the targets downhill at 1 g.
const ORBIT: i32 = 120;
const POUR: i32 = 190;

/// From pattrnlabs.com: plum #45255a, orange #ef9d4b, olive #7e8a57 and
/// cream #f7efe6. The plum is lifted so it shows on black. (base, core)
const PALETTE_RGB: [[(u8, u8, u8); 2]; 3] = [
    [(239, 157, 75), (160, 100, 215)], // orange, plum
    [(150, 168, 98), (247, 239, 230)], // olive, cream
    [(165, 112, 225), (239, 157, 75)], // plum, orange
];

const fn shade(c: (u8, u8, u8), level: u32) -> u16 {
    rgb565(
        (c.0 as u32 * level / 256) as u8,
        (c.1 as u32 * level / 256) as u8,
        (c.2 as u32 * level / 256) as u8,
    )
}

/// [palette][core][dim, full]
const PALETTES: [[[u16; 2]; 2]; 3] = {
    let mut out = [[[0; 2]; 2]; 3];
    let mut p = 0;
    while p < 3 {
        let mut a = 0;
        while a < 2 {
            out[p][a][0] = shade(PALETTE_RGB[p][a], 130);
            out[p][a][1] = shade(PALETTE_RGB[p][a], 256);
            a += 1;
        }
        p += 1;
    }
    out
};

#[derive(Clone, Copy)]
struct Ball {
    /// Position and velocity in 1/256 px and 1/256 px per 33 ms frame.
    x: i32,
    y: i32,
    vx: i32,
    vy: i32,
}

/// Allocation-free; state is about 3.3 KB.
pub struct Blobs {
    balls: [Ball; BALLS],
    /// 0 = blank, else 1 + index into `GLYPHS`.
    dots: [u8; CELLS],
    /// Bit 1 = core colour, bit 0 = full brightness.
    colours: [u8; CELLS],
    palette: usize,
}

impl Blobs {
    pub const fn new() -> Self {
        const START: [(i32, i32); BALLS] = [(150, 170), (300, 140), (240, 260), (140, 330), (330, 330)];
        let mut balls = [Ball { x: 0, y: 0, vx: 0, vy: 0 }; BALLS];
        let mut i = 0;
        while i < BALLS {
            balls[i].x = START[i].0 * 256;
            balls[i].y = START[i].1 * 256;
            i += 1;
        }
        Self { balls, dots: [0; CELLS], colours: [0; CELLS], palette: 0 }
    }
}

impl Default for Blobs {
    fn default() -> Self {
        Self::new()
    }
}

impl App for Blobs {
    fn update(&mut self, input: &Input, dt_ms: u32, out: &mut Output) {
        if input.touch.tapped {
            self.palette = (self.palette + 1) % PALETTES.len();
            out.sound = Some(Sound::Click);
        }
        // Steps are scaled to a 33 ms frame; a long gap counts as one frame.
        let dt = dt_ms.clamp(1, 66) as i32;
        let gx = (input.tilt.x_mg as i32).clamp(-1000, 1000);
        let gy = (input.tilt.y_mg as i32).clamp(-1000, 1000);
        let t = input.time_ms;
        for (k, b) in self.balls.iter_mut().enumerate() {
            // Each blob chases its own target, which traces a slow Lissajous
            // curve round the middle; tilt slides every target downhill.
            let (fa, fb) = (t / (70 + k as u32 * 17), t / (95 + k as u32 * 11));
            let tx = 240 + SIN[((fa + k as u32 * 51) & 255) as usize] as i32 * ORBIT / 127 + gx * POUR / 1000;
            let ty = 240 + SIN[((fb + 64 + k as u32 * 83) & 255) as usize] as i32 * ORBIT / 127 + gy * POUR / 1000;
            // A soft spring: acceleration in 1/256 px per frame per frame.
            let mut ax = (tx * 256 - b.x) / 160;
            let mut ay = (ty * 256 - b.y) / 160;
            if input.touch.down {
                // The finger pushes blobs away within ~130 px.
                let dx = b.x / 256 - input.touch.x.min(479) as i32;
                let dy = b.y / 256 - input.touch.y.min(479) as i32;
                let d2 = dx * dx + dy * dy;
                if d2 < 130 * 130 {
                    let push = (130 * 130 - d2) / 64;
                    let d = isqrt(d2).max(1);
                    ax += dx * push / d / 4;
                    ay += dy * push / d / 4;
                }
            }
            b.vx = (b.vx + ax * dt / 33) * 15 / 16;
            b.vy = (b.vy + ay * dt / 33) * 15 / 16;
            b.vx = b.vx.clamp(-2048, 2048);
            b.vy = b.vy.clamp(-2048, 2048);
            b.x += b.vx * dt / 33;
            b.y += b.vy * dt / 33;
            // Soft walls: bounce, losing most of the speed.
            let (lo, hi) = (REACH / 2 * 256, (480 - REACH / 2) * 256);
            if b.x < lo || b.x > hi {
                b.x = b.x.clamp(lo, hi);
                b.vx = -b.vx / 3;
            }
            if b.y < lo || b.y > hi {
                b.y = b.y.clamp(lo, hi);
                b.vy = -b.vy / 3;
            }
        }
        let top = GLYPHS.len() as i32 - 1;
        for i in 0..CELLS {
            let cx = (i % GRID) as i32 * CELL + CELL / 2;
            let cy = (i / GRID) as i32 * CELL + CELL / 2;
            let mut field = 0;
            for b in &self.balls {
                let dx = cx - b.x / 256;
                let dy = cy - b.y / 256;
                field += REACH * REACH * 64 / (dx * dx + dy * dy + 256);
            }
            // field is ~64 at the edge of a lone blob, rising steeply inside.
            let level = (field - 70) / 22;
            self.dots[i] = if level < 0 { 0 } else { 1 + level.min(top - 1) as u8 };
            let core = field > 330;
            let full = field > 70;
            self.colours[i] = (u8::from(core) << 1) | u8::from(full);
        }
    }

    fn render(&self, rows: Range<u16>, buf: &mut [u16]) {
        assert!(rows.start <= rows.end && rows.end <= HEIGHT);
        assert_eq!(buf.len(), (rows.end - rows.start) as usize * WIDTH as usize);
        for (line, y) in buf.chunks_exact_mut(480).zip(rows) {
            line.fill(0);
            let gy = y as usize % CELL as usize;
            if gy == 0 || gy == 11 {
                continue;
            }
            let cell_row = y as usize / CELL as usize * GRID;
            for (column, span) in line.chunks_exact_mut(CELL as usize).enumerate() {
                let cell = cell_row + column;
                let dot = self.dots[cell];
                if dot == 0 {
                    continue;
                }
                let bits = GLYPHS[dot as usize - 1][gy - 1];
                let c = self.colours[cell] as usize;
                let colour = PALETTES[self.palette][c >> 1][c & 1];
                for x in 0..10 {
                    if bits & (1 << (9 - x)) != 0 {
                        span[x + 1] = colour;
                    }
                }
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
