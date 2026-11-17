#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use app_sdk::{App, Input, Output, Tilt, Touch};
    use std::vec;

    #[test]
    fn sine_symmetry_and_range() {
        assert_eq!(SIN[0], 0);
        assert_eq!(SIN[64], 127);
        for i in 0..128 {
            assert_eq!(SIN[i], -SIN[i + 128]);
        }
        for i in 0..64 {
            assert_eq!(SIN[i], SIN[128 - i]);
        }
    }

    #[test]
    fn ramp_density_increases() {
        let mut previous = 0;
        for glyph in GLYPHS {
            let count: u32 = glyph.iter().map(|r| r.count_ones()).sum();
            assert!(count > previous, "{count} <= {previous}");
            assert!(glyph.iter().all(|r| *r < 1024));
            previous = count;
        }
    }

    #[test]
    fn tilt_controls_coverage() {
        let mut app = GlyphField::new();
        let mut input = Input {
            tilt: Tilt {
                x_mg: 0,
                y_mg: 0,
                z_mg: 1000,
            },
            ..Input::default()
        };
        app.update(&input, 33, &mut Output::default());
        let flat = app.glyphs.iter().filter(|g| **g != 0).count();
        assert!(flat > 0 && flat < 100);
        for i in 0..1600 {
            if app.glyphs[i] != 0 {
                assert!(app.radii[i] < 32);
            }
        }
        input.tilt = Tilt {
            x_mg: 866,
            y_mg: 0,
            z_mg: 500,
        };
        for frame in 0..100 {
            input.time_ms = frame * 33;
            app.update(&input, 33, &mut Output::default());
        }
        assert!(app.glyphs.iter().filter(|g| **g != 0).count() > 1500);
    }

    #[test]
    fn ticks_on_ring_changes_and_clicks_on_tap() {
        let mut app = GlyphField::new();
        let mut input = Input::default();
        input.tilt.z_mg = 1000;
        let mut out = Output::default();
        app.update(&input, 33, &mut out);
        assert_eq!(out.sound, None, "no tick on the first frame");
        input.tilt = Tilt { x_mg: 900, y_mg: 0, z_mg: 400 };
        let mut ticks = 0;
        for frame in 1..30 {
            let mut out = Output::default();
            input.time_ms = frame * 33;
            app.update(&input, 33, &mut out);
            if matches!(out.sound, Some(app_sdk::Sound::Tick { .. })) {
                ticks += 1;
            }
        }
        assert!(ticks > 3, "growing radius should tick, got {ticks}");
        let mut out = Output::default();
        input.touch.tapped = true;
        app.update(&input, 33, &mut out);
        assert_eq!(out.sound, Some(app_sdk::Sound::Click));
    }

    #[test]
    fn arbitrary_strips_match_full_frame() {
        let mut app = GlyphField::new();
        app.update(
            &Input {
                tilt: Tilt {
                    x_mg: 900,
                    y_mg: 0,
                    z_mg: 400,
                },
                touch: Touch {
                    down: true,
                    x: 300,
                    y: 200,
                    tapped: true,
                },
                time_ms: 345,
                local_time_s: None,
                key: false,
            },
            33,
            &mut Output::default(),
        );
        let mut full = vec![0; 480 * 480];
        app.render(0..480, &mut full);
        for (start, end) in [(0, 12), (7, 31), (233, 246), (468, 480), (480, 480)] {
            let mut strip = vec![0xFFFF; (end - start) as usize * 480];
            app.render(start..end, &mut strip);
            assert_eq!(strip, full[start as usize * 480..end as usize * 480]);
        }
        assert!(full.iter().any(|p| *p != 0));
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
// 2 px dot to a full cell. Zero in the cell array means blank.
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
/// From pattrnlabs.com: plum #45255a, orange #ef9d4b, olive #7e8a57 and
/// cream #f7efe6. The plum is lifted so it shows on black.
const PALETTE_RGB: [[(u8, u8, u8); 2]; 3] = [
    [(239, 157, 75), (160, 100, 215)], // orange, plum
    [(150, 168, 98), (247, 239, 230)], // olive, cream
    [(165, 112, 225), (239, 157, 75)], // plum, orange
];
const SHADES: usize = 4;
/// Brightness per shade step, out of 256.
const SHADE_LEVEL: [u32; SHADES] = [70, 120, 185, 256];

const fn shade(c: (u8, u8, u8), level: u32) -> u16 {
    rgb565(
        (c.0 as u32 * level / 256) as u8,
        (c.1 as u32 * level / 256) as u8,
        (c.2 as u32 * level / 256) as u8,
    )
}

const PALETTES: [[[u16; SHADES]; 2]; 3] = {
    let mut out = [[[0; SHADES]; 2]; 3];
    let mut p = 0;
    while p < 3 {
        let mut a = 0;
        while a < 2 {
            let mut s = 0;
            while s < SHADES {
                out[p][a][s] = shade(PALETTE_RGB[p][a], SHADE_LEVEL[s]);
                s += 1;
            }
            a += 1;
        }
        p += 1;
    }
    out
};

const fn sqrt(n: u32) -> u32 {
    let mut lo = 0;
    let mut hi = 65536;
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if mid * mid <= n {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Allocation-free 40 × 40 glyph field. State occupies approximately 4.8 KB.
pub struct GlyphField {
    radii: [u8; 1600],  // eighths of a cell
    glyphs: [u8; 1600], // 0 = blank, 1..=11 = ramp
    colours: [u8; 1600], // bit 2 = accent, bits 0..1 = shade
    radius_q8: i32,
    initialized: bool,
    palette: usize,
    /// Radius in whole cells at the last tick, for detent sounds.
    last_ring: u32,
}

impl GlyphField {
    pub const fn new() -> Self {
        let mut radii = [0; 1600];
        let mut i = 0;
        while i < 1600 {
            let x = (i % 40) as i32 * 8 - 156;
            let y = (i / 40) as i32 * 8 - 156;
            radii[i] = sqrt((x * x + y * y) as u32) as u8;
            i += 1;
        }
        Self {
            radii,
            glyphs: [0; 1600],
            colours: [0; 1600],
            radius_q8: 32 * 256,
            initialized: false,
            palette: 0,
            last_ring: 0,
        }
    }
}

impl Default for GlyphField {
    fn default() -> Self {
        Self::new()
    }
}

impl App for GlyphField {
    fn update(&mut self, input: &Input, dt_ms: u32, out: &mut Output) {
        let x = (input.tilt.x_mg as i32).unsigned_abs();
        let y = (input.tilt.y_mg as i32).unsigned_abs();
        // Horizontal gravity reaches 707 mg at 45 degrees. Saturate there.
        let horizontal = sqrt(x * x + y * y).min(707);
        let target = (32 + horizontal as i32 * 224 / 707) * 256;
        if !self.initialized {
            self.radius_q8 = target;
            self.initialized = true;
        } else {
            // Time-aware low pass, approximately 180 ms time constant.
            let dt = dt_ms.min(1000) as i32;
            self.radius_q8 += (target - self.radius_q8) * dt / (180 + dt);
        }
        if input.touch.tapped {
            self.palette = (self.palette + 1) % PALETTES.len();
            out.sound = Some(Sound::Click);
        }
        let radius = (self.radius_q8 / 256).max(1) as u32;
        // One detent tick per whole cell of radius, pitch rising outward.
        let ring = radius / 8;
        if ring != self.last_ring {
            if self.last_ring != 0 && out.sound.is_none() {
                out.sound = Some(Sound::Tick { pitch: (ring * 255 / 32).min(255) as u8 });
            }
            self.last_ring = ring;
        }
        let time = input.time_ms / 12;
        for i in 0..1600 {
            let r = self.radii[i] as u32;
            if r >= radius {
                self.glyphs[i] = 0;
                continue;
            }
            let phase = (r * 3).wrapping_sub(time) as u8;
            let wave = (SIN[phase as usize] as i32 + 128) as u32;
            let density = wave * (radius - r) / radius;
            self.glyphs[i] = (1 + density * 11 / 256) as u8;
            let mut shade = (density * SHADES as u32 / 256) as i32;
            // A narrow band of each wave crest takes the accent colour.
            let mut accent = phase < 40;
            if input.touch.down {
                // The finger is a light source: nearby cells brighten and
                // shift to the accent colour, fading out over ~150 px.
                let dx = (i % 40) as i32 * 12 + 6 - input.touch.x.min(479) as i32;
                let dy = (i / 40) as i32 * 12 + 6 - input.touch.y.min(479) as i32;
                let light = (22500 - (dx * dx + dy * dy)).max(0) * 256 / 22500;
                shade += light * 3 / 256;
                // Dithered edge so the lit area fades rather than cuts off.
                let threshold = ((i % 40 * 37 + i / 40 * 73) & 255) as i32;
                accent |= light > threshold;
            }
            self.colours[i] = (u8::from(accent) << 2) | shade.clamp(0, SHADES as i32 - 1) as u8;
        }
    }

    fn render(&self, rows: Range<u16>, buf: &mut [u16]) {
        assert!(rows.start <= rows.end && rows.end <= HEIGHT);
        assert_eq!(buf.len(), (rows.end - rows.start) as usize * WIDTH as usize);
        for (line, y) in buf.chunks_exact_mut(480).zip(rows) {
            let cell_row = y as usize / 12 * 40;
            let gy = y as usize % 12;
            for (column, span) in line.chunks_exact_mut(12).enumerate() {
                span.fill(0);
                let cell = cell_row + column;
                let glyph = self.glyphs[cell];
                if glyph == 0 || gy == 0 || gy == 11 {
                    continue;
                }
                let bits = GLYPHS[glyph as usize - 1][gy - 1];
                let c = self.colours[cell] as usize;
                let colour = PALETTES[self.palette][c >> 2][c & 3];
                for x in 0..10 {
                    if bits & (1 << (9 - x)) != 0 {
                        span[x + 1] = colour;
                    }
                }
            }
        }
    }
}
