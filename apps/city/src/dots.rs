//! The dot look: the same scene as a 40 x 40 halftone grid, like the
//! `glyphfield` and `blobs` visuals. Dot size carries brightness and dot
//! colour carries the colour, so the picture is the poster seen through a
//! screen of dots.
//!
//! `update` samples the poster itself (a few of its rows per grid row), so
//! the two looks always agree. `render` only reads the two tables.

use crate::motion::SIN;
use crate::poster::{self, Scene, View};
use app_sdk::rgb565;

/// Pixels per dot cell.
pub const PITCH: usize = 8;
/// Cells across and down.
pub const GRID: usize = 480 / PITCH;
const CELLS: usize = GRID * GRID;
/// The row of cells that holds the horizon.
const SEA_FROM: usize = (crate::art::HORIZON as usize + PITCH / 2) / PITCH;
/// The largest dot, in pixels across. One pixel smaller than the cell, so a
/// black gap always shows between dots.
const MAX_D: usize = PITCH - 1;

/// Each row of a dot of each size as one run (start, end) inside the cell,
/// centred. Index 0 is the smallest dot, one pixel across.
const RUNS: [[(u8, u8); PITCH]; MAX_D] = {
    let mut out = [[(0u8, 0u8); PITCH]; MAX_D];
    let mut d = 1;
    while d <= MAX_D {
        // Centre and radius in half pixels.
        let c2 = PITCH as i32 - 2;
        let r2 = d as i32;
        let mut row = 0;
        while row < PITCH {
            let dy = 2 * row as i32 - c2;
            let mut a = PITCH as i32;
            let mut z = 0;
            let mut x = 0;
            while x < PITCH as i32 {
                let dx = 2 * x - c2;
                // Inside the disc, a little generous so small dots are round.
                if 2 * (dx * dx + dy * dy) <= 2 * r2 * r2 + r2 {
                    if x < a {
                        a = x;
                    }
                    z = x + 1;
                }
                x += 1;
            }
            if z > 0 {
                out[d - 1][row] = (a as u8, z as u8);
            }
            row += 1;
        }
        d += 1;
    }
    out
};
const SIZES: u8 = MAX_D as u8;

/// Dot size for a brightness 0..=255. The curve (brightness to the power
/// 0.4) opens up the dark end, so night buildings and the sky glow behind
/// them get clearly different dots. Very dark is left empty.
const SIZE_OF: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        let n = MAX_D as u64;
        let mut s = 0u64;
        // Largest s with (s / n) ^ 2.5 <= i / 255.
        while (s + 1).pow(5) * 255 * 255 <= (i as u64).pow(2) * n.pow(5) {
            s += 1;
        }
        t[i] = if i < 6 { 0 } else if s == 0 { 1 } else { s as u8 };
        i += 1;
    }
    t
};

/// Where in a cell the nine samples are taken, across and down.
const SAMPLE: [usize; 3] = [1, PITCH / 2, PITCH - 2];

pub struct Dots {
    /// Dot size per cell, 0 (none) to 11.
    size: [u8; CELLS],
    /// Dot colour per cell.
    colour: [u16; CELLS],
}

impl Dots {
    pub const fn new() -> Self {
        Self { size: [0; CELLS], colour: [0; CELLS] }
    }

    /// Samples the poster for this frame and sets every dot.
    pub fn update(&mut self, scene: &Scene, view: &View, ms: u32) {
        let mut rows = [[0u16; 480]; 3];
        for gy in 0..GRID {
            for (k, row) in rows.iter_mut().enumerate() {
                poster::draw_row(row, scene, view, (gy * PITCH + SAMPLE[k]) as i32);
            }
            for gx in 0..GRID {
                // Average nine samples in the cell.
                let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
                for row in &rows {
                    for k in 0..3 {
                        let p = row[gx * PITCH + SAMPLE[k]];
                        r += (p >> 11) as u32 & 31;
                        g += (p >> 5) as u32 & 63;
                        b += p as u32 & 31;
                    }
                }
                // Back to 8-bit channels.
                let (r, g, b) = (r * 255 / (31 * 9), g * 255 / (63 * 9), b * 255 / (31 * 9));
                let lum = (r * 2 + g * 5 + b) / 8;
                let mut size = SIZE_OF[lum as usize] as i32;

                // Heat haze in the sky, rolling bands in the sea.
                if size > 0 {
                    let wave = if gy < SEA_FROM {
                        // Heat haze: thin, slightly tilted lines that rise
                        // slowly and shrink the dots they cross by one step.
                        let ph = (gy * 37 + gx * 5) as u32 + (ms / 20) % 4096;
                        if SIN[ph as usize & 255] > 115 {
                            -1
                        } else {
                            0
                        }
                    } else {
                        let ph = (gy * 19) as u32 + 4096 - (ms / 9) % 4096 + (gx as u32 * 2);
                        SIN[(ph & 255) as usize] as i32 * 3 / 200
                    };
                    size = (size + wave).clamp(1, SIZES as i32);
                }
                let i = gy * GRID + gx;
                self.size[i] = size as u8;

                // Colour: the cell's colour lifted toward full brightness,
                // since the dot's size already shows how bright it is.
                let max = r.max(g).max(b).max(1);
                let gain = (255 * 256 / max).min(2 * 256);
                let lift = |c: u32| ((c * gain / 256).min(255)) as u8;
                self.colour[i] = rgb565(lift(r), lift(g), lift(b));
            }
        }
    }

    pub fn render(&self, rows: core::ops::Range<u16>, buf: &mut [u16]) {
        for (line, y) in buf.as_chunks_mut::<480>().0.iter_mut().zip(rows) {
            line.fill(0);
            let gy = y as usize % PITCH;
            let cell_row = y as usize / PITCH * GRID;
            for gx in 0..GRID {
                let i = cell_row + gx;
                let s = self.size[i];
                if s == 0 {
                    continue;
                }
                let (a, z) = RUNS[s as usize - 1][gy];
                if a < z {
                    let x = gx * PITCH;
                    line[x + a as usize..x + z as usize].fill(self.colour[i]);
                }
            }
        }
    }
}

impl Default for Dots {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_grow_and_keep_a_gap() {
        let mut prev = 0;
        for (d, runs) in RUNS.iter().enumerate() {
            let area: u32 = runs.iter().map(|&(a, z)| (z - a) as u32).sum();
            assert!(area > prev, "size {d}");
            prev = area;
            // The bottom row and right column of a cell stay black.
            assert_eq!(runs[PITCH - 1], (0, 0));
            assert!(runs.iter().all(|&(_, z)| (z as usize) < PITCH));
        }
    }

    #[test]
    fn size_curve() {
        assert_eq!(SIZE_OF[0], 0);
        assert_eq!(SIZE_OF[255], MAX_D as u8);
        for w in SIZE_OF.windows(2) {
            assert!(w[0] <= w[1]);
        }
    }
}
