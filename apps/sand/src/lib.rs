#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use app_sdk::{App, Input, Output, Tilt, Touch};
    use std::vec;

    fn tilted(x_mg: i16, y_mg: i16, z_mg: i16) -> Input {
        Input {
            tilt: Tilt { x_mg, y_mg, z_mg },
            ..Input::default()
        }
    }

    fn run(s: &mut Sand, input: &Input, frames: usize) {
        for _ in 0..frames {
            s.update(input, 33, &mut Output::default());
        }
    }

    /// Every grain sits in its own cell and the bitmap says exactly that.
    fn assert_consistent(s: &Sand) {
        let mut seen = [false; N * N];
        for g in &s.grains {
            let i = cell(g.x) + cell(g.y) * N;
            assert!(!seen[i], "two grains in cell {i}");
            seen[i] = true;
        }
        for (i, seen) in seen.iter().enumerate() {
            assert_eq!(s.occupied(i % N, i / N), *seen, "bitmap cell {i}");
        }
    }

    #[test]
    fn grains_are_conserved_in_every_direction() {
        let mut s = Sand::new();
        assert_consistent(&s);
        for (x, y) in [(0, 1000), (1000, 0), (-700, -700), (0, -1000), (600, 300), (0, 0)] {
            run(&mut s, &tilted(x, y, 300), 60);
            assert_consistent(&s);
        }
    }

    #[test]
    fn pools_on_the_low_side() {
        let mut s = Sand::new();
        run(&mut s, &tilted(1000, 0, 0), 300);
        // 700 grains slumped at about 45 degrees span roughly 37 cells.
        let min_x = s.grains.iter().map(|g| cell(g.x)).min().unwrap();
        assert!(min_x >= N - 42, "right-tilted pool reaches column {min_x}");
        assert!(s.grains.iter().filter(|g| cell(g.x) == N - 1).count() > 20);
        run(&mut s, &tilted(0, -1000, 0), 300);
        let max_y = s.grains.iter().map(|g| cell(g.y)).max().unwrap();
        assert!(max_y <= 42, "up-tilted pool reaches row {max_y}");
        assert!(s.grains.iter().filter(|g| cell(g.y) == 0).count() > 20);
    }

    #[test]
    fn stays_put_when_flat() {
        let mut s = Sand::new();
        run(&mut s, &tilted(700, 700, 300), 300);
        run(&mut s, &tilted(10, -10, 1000), 30);
        let before: vec::Vec<_> = s.grains.iter().map(|g| (cell(g.x), cell(g.y))).collect();
        let mut out = Output::default();
        for _ in 0..90 {
            s.update(&tilted(10, -10, 1000), 33, &mut out);
        }
        let after: vec::Vec<_> = s.grains.iter().map(|g| (cell(g.x), cell(g.y))).collect();
        assert_eq!(before, after);
        assert_eq!(out.sound, None);
    }

    #[test]
    fn finger_pushes_sand_away() {
        let mut s = Sand::new();
        run(&mut s, &tilted(0, 1000, 0), 120);
        // Touch the surface of the pool at the centre; the finger digs in.
        let near = |s: &Sand| {
            s.grains
                .iter()
                .filter(|g| {
                    let (dx, dy) = (cell(g.x) as i32 - 40, cell(g.y) as i32 - 72);
                    dx * dx + dy * dy <= 9
                })
                .count()
        };
        let before = near(&s);
        assert!(before > 10);
        let input = Input {
            touch: Touch { down: true, x: 243, y: 435, tapped: false },
            ..tilted(0, 1000, 0)
        };
        run(&mut s, &input, 30);
        assert!(near(&s) < before / 3, "{} of {before} grains left under the finger", near(&s));
        assert_consistent(&s);
    }

    #[test]
    fn ticks_when_sand_hits_a_wall() {
        let mut s = Sand::new();
        run(&mut s, &tilted(0, 1000, 0), 120);
        let mut ticks = 0;
        for _ in 0..90 {
            let mut out = Output::default();
            s.update(&tilted(1000, 0, 0), 33, &mut out);
            ticks += usize::from(out.sound.is_some());
        }
        assert!(ticks > 0 && ticks < 60, "ticks {ticks}");
    }

    #[test]
    fn arbitrary_strips_match_full_frame() {
        let mut s = Sand::new();
        run(&mut s, &tilted(400, 800, 300), 20);
        let mut full = vec![0; 480 * 480];
        s.render(0..480, &mut full);
        assert_eq!(full.iter().filter(|p| **p != 0).count(), GRAINS * CELL_PX * CELL_PX);
        for (start, end) in [(0, 2), (7, 19), (100, 112), (478, 480)] {
            let mut strip = vec![0; (end - start) * 480];
            s.render(start as u16..end as u16, &mut strip);
            assert_eq!(strip, full[start * 480..end * 480]);
        }
    }
}

use app_sdk::{rgb565, App, Input, Output, Sound};
use core::ops::Range;

// Particle sand after Adafruit's PixelDust: each grain has a fixed-point
// position and velocity, a cell map says which cells are taken (and by
// which shade, for drawing), and a grain that would land on another slides
// off a free corner or waits behind it.

/// Grid side in cells; each cell is 6 × 6 pixels.
const N: usize = 80;
const CELL_PX: usize = 6;
const GRAINS: usize = 700;

/// Positions and velocities are in 1/256 of a cell; 256 is one cell per step.
const ONE: i32 = 256;
const MAX_POS: i32 = N as i32 * ONE - 1;
/// Velocity gained per step at 1 g, for a grain of average weight.
const ACCEL: i32 = 46;
/// Tilt below this is noise, and with no touch the sand rests.
const DEAD_MG: i32 = 60;
/// Fraction of speed kept when bouncing off a wall, out of 256. Low: sand,
/// not balls.
const BOUNCE: i32 = 40;
/// Fraction of speed kept when blocked by another grain, out of 256. High,
/// so a pool flows as a whole instead of every contact stopping it dead.
const KEEP: i32 = 200;
const STEPS_PER_FRAME: usize = 3;
/// The finger pushes grains within this many cells.
const REACH: i32 = 9;
/// One blocked grain in this many may step sideways along the surface.
/// Lower spreads the pool flatter.
const SPREAD_ODDS: u32 = 3;

/// Three tones of one sand colour, each with a darker edge. Index 0 is empty.
const TONES: [(u16, u16); 4] = [
    (0, 0),
    (rgb565(236, 190, 112), rgb565(184, 136, 72)),
    (rgb565(248, 210, 142), rgb565(196, 152, 92)),
    (rgb565(218, 166, 92), rgb565(170, 120, 60)),
];

#[derive(Clone, Copy)]
struct Grain {
    x: i32,
    y: i32,
    vx: i32,
    vy: i32,
    /// Tone index into `TONES`, 1..=3.
    tone: u8,
    /// Acceleration multiplier in sixteenths, 13..=19. Grains of slightly
    /// different weight keep the flow from moving as one block.
    weight: u8,
}

const fn cell(p: i32) -> usize {
    (p / ONE) as usize
}

/// A small pool of sand that runs wherever the board tilts and is pushed
/// aside by a finger. State is about 18 KB.
pub struct Sand {
    grains: [Grain; GRAINS],
    /// Tone of the grain in each cell, 0 when empty.
    cells: [u8; N * N],
    rng: u32,
    since_tick_ms: u32,
}

impl Sand {
    pub const fn new() -> Self {
        let mut s = Self {
            grains: [Grain { x: 0, y: 0, vx: 0, vy: 0, tone: 1, weight: 16 }; GRAINS],
            cells: [0; N * N],
            rng: 0x9E37_79B9,
            since_tick_ms: 0,
        };
        // Fill from the bottom row up, the last partial row centred.
        let mut i = 0;
        while i < GRAINS {
            let row = i / N;
            let in_row = if (row + 1) * N <= GRAINS { N } else { GRAINS - row * N };
            let col = (N - in_row) / 2 + i % N;
            let (x, y) = (col, N - 1 - row);
            // A fixed hash of the index picks tone and weight.
            let h = (i as u32).wrapping_mul(0x9E37_79B9) >> 24;
            let tone = 1 + (h % 3) as u8;
            s.grains[i] = Grain {
                x: x as i32 * ONE + ONE / 2,
                y: y as i32 * ONE + ONE / 2,
                vx: 0,
                vy: 0,
                tone,
                weight: 13 + (h / 3 % 7) as u8,
            };
            s.cells[y * N + x] = tone;
            i += 1;
        }
        s
    }

    fn occupied(&self, x: usize, y: usize) -> bool {
        self.cells[y * N + x] != 0
    }

    fn random(&mut self) -> u32 {
        // xorshift32
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    /// -1 or +1.
    fn coin(&mut self) -> i32 {
        if self.random() & 1 == 0 { -1 } else { 1 }
    }

    /// Insertion sort so grains nearest the low side move first and are not
    /// blocked by grains that have not moved yet. Nearly sorted from the
    /// previous frame, so this is close to linear.
    fn sort_along(&mut self, ax: i32, ay: i32) {
        let key = |g: &Grain| -(g.x / ONE * ax + g.y / ONE * ay);
        for i in 1..GRAINS {
            let g = self.grains[i];
            let k = key(&g);
            let mut j = i;
            while j > 0 && key(&self.grains[j - 1]) > k {
                self.grains[j] = self.grains[j - 1];
                j -= 1;
            }
            self.grains[j] = g;
        }
    }

    /// One step. Returns how many grains hit a wall hard.
    fn step(&mut self, ax: i32, ay: i32, jitter: i32, touch: Option<(i32, i32)>) -> u32 {
        let mut thuds = 0;
        for i in 0..GRAINS {
            let mut g = self.grains[i];
            let w = g.weight as i32;
            let (mut gx, mut gy) = (ax * w / 16, ay * w / 16);
            if jitter > 0 {
                // Zero-mean nudges that let tall stacks topple.
                let span = (2 * jitter + 1) as u32;
                gx += (self.random() % span) as i32 - jitter;
                gy += (self.random() % span) as i32 - jitter;
            }
            if let Some((tx, ty)) = touch {
                // Distance in 1/16 cell: whole cells would truncate a grain
                // just above the finger to distance zero and never push it.
                let (cx, cy) = ((g.x - tx) / 16, (g.y - ty) / 16);
                let d2 = cx * cx + cy * cy;
                const R: i32 = REACH * 16;
                if d2 < R * R {
                    let d = isqrt(d2 as u32).max(1) as i32;
                    let push = (R - d) * 48 / 16;
                    gx += cx * push / d;
                    gy += cy * push / d;
                }
            }
            g.vx += gx;
            g.vy += gy;
            if jitter == 0 && touch.is_none() {
                // Resting: friction brings grains to a full stop.
                let damp = |v: i32| if v.abs() < 8 { 0 } else { v - v / 4 };
                g.vx = damp(g.vx);
                g.vy = damp(g.vy);
            }
            // Cap speed at one cell per step, keeping direction.
            let v2 = (g.vx * g.vx + g.vy * g.vy) as u32;
            if v2 > (ONE * ONE) as u32 {
                let v = isqrt(v2) as i32;
                g.vx = g.vx * ONE / v;
                g.vy = g.vy * ONE / v;
            }

            let (ox, oy) = (g.x, g.y);
            let (mut nx, mut ny) = (ox + g.vx, oy + g.vy);
            let hard = |v: i32| v.abs() > ONE / 2;
            if nx < 0 || nx > MAX_POS {
                thuds += u32::from(hard(g.vx));
                nx = nx.clamp(0, MAX_POS);
                g.vx = -g.vx * BOUNCE / 256;
            }
            if ny < 0 || ny > MAX_POS {
                thuds += u32::from(hard(g.vy));
                ny = ny.clamp(0, MAX_POS);
                g.vy = -g.vy * BOUNCE / 256;
            }

            let (ocx, ocy) = (cell(ox), cell(oy));
            let (ncx, ncy) = (cell(nx), cell(ny));
            if (ncx, ncy) != (ocx, ocy) && self.occupied(ncx, ncy) {
                // Blocked. Try, in order: the two cells 45 degrees either
                // side of the move (sand slides off the grain in front),
                // then sometimes the two cells at 90 degrees (it spreads
                // along the surface like a pool). Else stop and bounce.
                let (sx, sy) = (ncx as i32 - ocx as i32, ncy as i32 - ocy as i32);
                let mut slide = if sx != 0 && sy != 0 {
                    // Faster axis first, as in PixelDust.
                    if g.vx.abs() >= g.vy.abs() { [(sx, 0), (0, sy)] } else { [(0, sy), (sx, 0)] }
                } else if sx != 0 {
                    [(sx, -1), (sx, 1)]
                } else {
                    [(-1, sy), (1, sy)]
                };
                let mut spread = [(-sy, sx), (sy, -sx)];
                if sx == 0 || sy == 0 {
                    if self.coin() > 0 {
                        slide.swap(0, 1);
                    }
                }
                if self.coin() > 0 {
                    spread.swap(0, 1);
                }
                let spreading = self.random() % SPREAD_ODDS == 0;
                let to = slide
                    .into_iter()
                    .chain(spread.into_iter().filter(|_| spreading))
                    .map(|(dx, dy)| (ocx as i32 + dx, ocy as i32 + dy))
                    .find(|&(x, y)| {
                        (0..N as i32).contains(&x)
                            && (0..N as i32).contains(&y)
                            && (x, y) != (ocx as i32, ocy as i32)
                            && !self.occupied(x as usize, y as usize)
                    });
                match to {
                    Some((x, y)) => {
                        nx = if x as usize == ocx { ox } else { x * ONE + ONE / 2 };
                        ny = if y as usize == ocy { oy } else { y * ONE + ONE / 2 };
                        g.vx -= g.vx / 8;
                        g.vy -= g.vy / 8;
                    }
                    None => {
                        // Wait behind the grain in front, keeping most of the
                        // speed so the pool moves on as soon as it can.
                        (nx, ny) = (ox, oy);
                        g.vx = g.vx * KEEP / 256;
                        g.vy = g.vy * KEEP / 256;
                    }
                }
            }

            self.cells[ocy * N + ocx] = 0;
            self.cells[cell(ny) * N + cell(nx)] = g.tone;
            g.x = nx;
            g.y = ny;
            self.grains[i] = g;
        }
        thuds
    }
}

impl Default for Sand {
    fn default() -> Self {
        Self::new()
    }
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

/// Screen pixel to grain coordinates (the centre of that cell).
fn cell_from_px(p: u16) -> i32 {
    (p as i32 / CELL_PX as i32).min(N as i32 - 1) * ONE + ONE / 2
}

impl App for Sand {
    fn update(&mut self, input: &Input, dt_ms: u32, out: &mut Output) {
        let (tx, ty) = (input.tilt.x_mg as i32, input.tilt.y_mg as i32);
        let tilted = isqrt((tx * tx + ty * ty) as u32) as i32 >= DEAD_MG;
        let (ax, ay) = if tilted {
            (tx * ACCEL / 1000, ty * ACCEL / 1000)
        } else {
            (0, 0)
        };
        // More jitter the flatter the board, as in PixelDust, but none at
        // rest so a level board keeps the pool where you left it.
        let jitter = if tilted {
            1 + (input.tilt.z_mg as i32).abs() * 3 / 1000
        } else {
            0
        };
        let touch = input
            .touch
            .down
            .then(|| (cell_from_px(input.touch.x), cell_from_px(input.touch.y)));

        self.sort_along(ax, ay);
        let mut thuds = 0;
        for _ in 0..STEPS_PER_FRAME {
            thuds += self.step(ax, ay, jitter, touch);
        }

        self.since_tick_ms = self.since_tick_ms.saturating_add(dt_ms);
        if thuds > 0 && self.since_tick_ms >= 70 {
            self.since_tick_ms = 0;
            let pitch = 70 + thuds.min(40) * 4 + self.random() % 20;
            out.sound = Some(Sound::Tick { pitch: pitch.min(255) as u8 });
        }
    }

    fn render(&self, rows: Range<u16>, buf: &mut [u16]) {
        assert!(rows.start <= rows.end && rows.end <= 480);
        assert_eq!(buf.len(), (rows.end - rows.start) as usize * 480);
        for (line, y) in buf.chunks_exact_mut(480).zip(rows) {
            let (cy, py) = (y as usize / CELL_PX, y as usize % CELL_PX);
            let bottom = py == CELL_PX - 1;
            let cells = &self.cells[cy * N..][..N];
            // Eight cells (48 pixels) at a time, skipping empty runs.
            for (oct, px) in cells.chunks_exact(8).zip(line.chunks_exact_mut(8 * CELL_PX)) {
                if oct == [0; 8] {
                    px.fill(0);
                    continue;
                }
                for (&tone, p) in oct.iter().zip(px.chunks_exact_mut(CELL_PX)) {
                    let (face, edge) = TONES[tone as usize];
                    if bottom {
                        p.fill(edge);
                    } else {
                        p[..CELL_PX - 1].fill(face);
                        p[CELL_PX - 1] = edge;
                    }
                }
            }
        }
    }
}
