#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use app_sdk::{App, Input, Output, Tilt, Touch};
    use std::vec;

    /// 2026-10-26 (a Monday) 00:00 local.
    const DAY: u32 = 1_792_972_800;

    fn at(t: Option<u32>) -> Input {
        Input {
            tilt: Tilt { x_mg: 0, y_mg: 0, z_mg: 1000 },
            local_time_s: t,
            key: false,
            ..Input::default()
        }
    }

    fn settle(c: &mut Clock, input: &Input) {
        for _ in 0..120 {
            c.update(input, 33, &mut Output::default());
        }
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1, 4));
        assert_eq!(civil(DAY), (2026, 10, 26, 1));
        assert_eq!(civil(951_782_400), (2000, 2, 29, 2));
        assert_eq!(civil(4_102_444_799), (2099, 12, 31, 4));
    }

    #[test]
    fn shows_dashes_until_synced_then_the_time() {
        let mut c = Clock::new();
        c.update(&at(None), 33, &mut Output::default());
        assert_eq!(c.shown, *b"----");
        c.update(&at(Some(DAY + 13 * 3600 + 45 * 60 + 7)), 33, &mut Output::default());
        assert_eq!(c.shown, *b"1345");
        assert_eq!(c.second, Some(7));
        assert_eq!(c.date, *b"MON 26 OCT");
    }

    #[test]
    fn ticks_once_when_the_minute_changes() {
        let mut c = Clock::new();
        let t = DAY + 9 * 3600 + 59;
        let mut out = Output::default();
        c.update(&at(Some(t)), 33, &mut out);
        assert_eq!(out.sound, None, "first sync is silent");
        let mut ticks = 0;
        for s in 1..60 {
            let mut out = Output::default();
            c.update(&at(Some(t + s)), 33, &mut out);
            ticks += usize::from(out.sound.is_some());
        }
        assert_eq!(ticks, 1);
    }

    #[test]
    fn dots_scatter_from_touch_and_reform() {
        let mut c = Clock::new();
        let input = at(Some(DAY + 8 * 3600));
        settle(&mut c, &input);
        let rest = c.max_offset();
        assert!(rest < 2 * 256, "settled offset {rest}");
        let touched = Input {
            touch: Touch { down: true, x: 120, y: 172, tapped: false },
            ..input
        };
        for _ in 0..10 {
            c.update(&touched, 33, &mut Output::default());
        }
        assert!(c.max_offset() > 10 * 256);
        settle(&mut c, &input);
        assert!(c.max_offset() < 2 * 256);

        // Standing on its edge, dots sag but still render in the resting colour.
        let upright = Input { tilt: Tilt { x_mg: -1000, y_mg: 0, z_mg: 0 }, ..input };
        settle(&mut c, &upright);
        let mut full = vec![0; 480 * 480];
        c.render(0..480, &mut full);
        assert!(full.contains(&WARM) && !full.contains(&STIRRED));
    }

    #[test]
    fn arbitrary_strips_match_full_frame() {
        let mut c = Clock::new();
        c.update(&at(Some(DAY + 23 * 3600 + 59 * 60 + 30)), 33, &mut Output::default());
        let mut full = vec![0; 480 * 480];
        c.render(0..480, &mut full);
        assert!(full.iter().filter(|p| **p != 0).count() > 2000);
        for (start, end) in [(0, 2), (5, 17), (160, 172), (330, 342), (478, 480)] {
            let mut strip = vec![0; (end - start) * 480];
            c.render(start as u16..end as u16, &mut strip);
            assert_eq!(strip, full[start * 480..end * 480]);
        }
    }
}

use app_sdk::{rgb565, App, Input, Output, Sound};
use core::ops::Range;

/// Big digits: 5 × 7 dots each, 16 px apart.
const PITCH: i32 = 16;
const DOT_R: i32 = 6;
/// Column of each digit's left edge in the 25-column HH:MM row.
const DIGIT_COL: [i32; 4] = [0, 6, 14, 20];
const COLON_COL: i32 = 12;
const LEFT: i32 = 240 - 12 * PITCH;
const TOP: i32 = 172;
/// Slots: 4 digits × 35 dots, then the two colon dots.
const SLOTS: usize = 4 * 35 + 2;

const RING_R: i32 = 222;
/// sin(i × 6°) × 1000.
const SIN60: [i32; 60] = [
    0, 105, 208, 309, 407, 500, 588, 669, 743, 809, 866, 914, 951, 978, 995, 1000, 995, 978,
    951, 914, 866, 809, 743, 669, 588, 500, 407, 309, 208, 105, 0, -105, -208, -309, -407, -500,
    -588, -669, -743, -809, -866, -914, -951, -978, -995, -1000, -995, -978, -951, -914, -866,
    -809, -743, -669, -588, -500, -407, -309, -208, -105,
];

const DATE_PITCH: i32 = 5;
const DATE_TOP: i32 = 330;

const WARM: u16 = rgb565(240, 234, 220);
const STIRRED: u16 = rgb565(250, 170, 150);
const SCATTERED: u16 = rgb565(240, 90, 170);
const COLON_DIM: u16 = rgb565(110, 106, 100);
const RING_PAST: u16 = rgb565(40, 130, 128);
const RING_NOW: u16 = rgb565(90, 230, 220);
const RING_FUTURE: u16 = rgb565(36, 36, 40);
const DATE: u16 = rgb565(150, 146, 140);

const DAYS: [&[u8; 3]; 7] = [b"SUN", b"MON", b"TUE", b"WED", b"THU", b"FRI", b"SAT"];
const MONTHS: [&[u8; 3]; 12] = [
    b"JAN", b"FEB", b"MAR", b"APR", b"MAY", b"JUN", b"JUL", b"AUG", b"SEP", b"OCT", b"NOV", b"DEC",
];

/// 5 × 7 glyphs, one byte per row, bit 4 is the left column.
const fn glyph(c: u8) -> [u8; 7] {
    match c {
        b'0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        b'1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        b'3' => [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E],
        b'4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        b'5' => [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E],
        b'6' => [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        b'7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        b'8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        b'9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C],
        b'-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        b'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        b'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        b'D' => [0x1C, 0x12, 0x11, 0x11, 0x11, 0x12, 0x1C],
        b'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        b'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        b'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        b'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        b'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        b'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        b'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        b'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        b'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        b'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        b'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        b'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        b'Y' => [0x11, 0x11, 0x11, 0x0A, 0x04, 0x04, 0x04],
        _ => [0; 7],
    }
}

/// (year, month, day, weekday with 0 = Sunday) for local Unix seconds.
fn civil(t: u32) -> (u32, u32, u32, u32) {
    // Howard Hinnant's days-to-civil, for days on or after 1970-01-01.
    let days = t / 86_400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u32::from(month <= 2);
    (year, month, day, (days + 4) % 7)
}

fn isqrt(n: u32) -> u32 {
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// Home position of a slot's dot, in pixels.
const fn home(slot: usize) -> (i32, i32) {
    if slot >= 4 * 35 {
        let row = if slot == 4 * 35 { 2 } else { 4 };
        return (LEFT + COLON_COL * PITCH, TOP + row * PITCH);
    }
    let (digit, dot) = (slot / 35, slot % 35);
    let (row, col) = ((dot / 5) as i32, (dot % 5) as i32);
    (LEFT + (DIGIT_COL[digit] + col) * PITCH, TOP + row * PITCH)
}

/// A clock whose digits are springy dots: touch pushes them away and they
/// re-form; new digits fly in. A ring of 60 dots shows the seconds.
pub struct Clock {
    /// Characters on the four digit slots, `-` until synced.
    shown: [u8; 4],
    lit: [bool; SLOTS],
    /// Offset from home and velocity, in 1/256 px and 1/256 px per second.
    off: [(i32, i32); SLOTS],
    vel: [(i32, i32); SLOTS],
    second: Option<u32>,
    minute: Option<u32>,
    date: [u8; 10],
    rng: u32,
    /// Where gravity makes dots rest, as an offset from home in 1/256 px.
    sag: (i32, i32),
}

impl Clock {
    pub const fn new() -> Self {
        let mut c = Self {
            shown: [0; 4],
            lit: [false; SLOTS],
            off: [(0, 0); SLOTS],
            vel: [(0, 0); SLOTS],
            second: None,
            minute: None,
            date: [b' '; 10],
            rng: 0x2545_F491,
            sag: (0, 0),
        };
        c.lit[4 * 35] = true;
        c.lit[4 * 35 + 1] = true;
        c
    }

    fn random(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    /// Light the dots for `chars`; dots that newly light fly in from afar.
    fn show(&mut self, chars: [u8; 4]) {
        for (digit, &c) in chars.iter().enumerate() {
            if self.shown[digit] == c {
                continue;
            }
            let rows = glyph(c);
            for dot in 0..35 {
                let slot = digit * 35 + dot;
                let on = rows[dot / 5] & (0x10 >> (dot % 5)) != 0;
                if on && !self.lit[slot] {
                    let dx = (self.random() % 400) as i32 - 200;
                    let dy = (self.random() % 400) as i32 - 200;
                    self.off[slot] = (dx * 256, dy * 256);
                    self.vel[slot] = (0, 0);
                }
                self.lit[slot] = on;
            }
        }
        self.shown = chars;
    }

    #[cfg(test)]
    fn max_offset(&self) -> i32 {
        (0..SLOTS)
            .filter(|&s| self.lit[s])
            .map(|s| self.off[s].0.abs().max(self.off[s].1.abs()))
            .max()
            .unwrap_or(0)
    }
}

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl App for Clock {
    fn update(&mut self, input: &Input, dt_ms: u32, out: &mut Output) {
        match input.local_time_s {
            None => {
                self.show(*b"----");
                self.second = None;
            }
            Some(t) => {
                let of_day = t % 86_400;
                let (h, m, s) = (of_day / 3600, of_day / 60 % 60, of_day % 60);
                let digit = |n: u32| b'0' + n as u8;
                self.show([digit(h / 10), digit(h % 10), digit(m / 10), digit(m % 10)]);
                let minute = Some(t / 60);
                if self.minute.is_some() && self.minute != minute {
                    out.sound = Some(Sound::Tick { pitch: 200 });
                }
                self.minute = minute;
                self.second = Some(s);
                let (_, month, day, weekday) = civil(t);
                let mut date = [b' '; 10];
                date[..3].copy_from_slice(DAYS[weekday as usize]);
                date[4] = digit(day / 10);
                date[5] = digit(day % 10);
                date[7..].copy_from_slice(MONTHS[month as usize - 1]);
                self.date = date;
            }
        }

        // Springs. Gravity sags every dot a little toward the low side.
        let dt = dt_ms.min(50) as i32;
        // Up to about 5 px at 1 g.
        let sag = (input.tilt.x_mg as i32 * 256 / 200, input.tilt.y_mg as i32 * 256 / 200);
        self.sag = sag;
        let touch = input.touch;
        for slot in 0..SLOTS {
            if !self.lit[slot] {
                continue;
            }
            let (hx, hy) = home(slot);
            let (mut ox, mut oy) = self.off[slot];
            let (mut vx, mut vy) = self.vel[slot];
            // Spring constant 90 /s², damping 9 /s.
            let mut ax = -(ox - sag.0) * 90 - vx * 9;
            let mut ay = -(oy - sag.1) * 90 - vy * 9;
            if touch.down {
                let dx = hx * 256 + ox - touch.x as i32 * 256;
                let dy = hy * 256 + oy - touch.y as i32 * 256;
                let (px, py) = (dx / 256, dy / 256);
                let d = isqrt((px * px + py * py) as u32) as i32;
                const REACH: i32 = 100;
                if d < REACH {
                    let push = (REACH - d) * 900;
                    let d = d.max(1);
                    ax += px * push / d * 256 / 16;
                    ay += py * push / d * 256 / 16;
                }
            }
            vx += ax * dt / 1000;
            vy += ay * dt / 1000;
            ox += vx * dt / 1000;
            oy += vy * dt / 1000;
            // Keep dots on the screen.
            ox = ox.clamp((-hx) * 256, (479 - hx) * 256);
            oy = oy.clamp((-hy) * 256, (479 - hy) * 256);
            self.off[slot] = (ox, oy);
            self.vel[slot] = (vx, vy);
        }
    }

    fn render(&self, rows: Range<u16>, buf: &mut [u16]) {
        assert!(rows.start <= rows.end && rows.end <= 480);
        assert_eq!(buf.len(), (rows.end - rows.start) as usize * 480);
        buf.fill(0);
        let (top, bottom) = (rows.start as i32, rows.end as i32);
        let mut disc = |cx: i32, cy: i32, r: i32, colour: u16| {
            if cy + r < top || cy - r >= bottom {
                return;
            }
            for y in (cy - r).max(top)..(cy + r + 1).min(bottom) {
                let dy = y - cy;
                let line = &mut buf[(y - top) as usize * 480..][..480];
                for x in (cx - r).max(0)..(cx + r + 1).min(480) {
                    let dx = x - cx;
                    if dx * dx + dy * dy <= r * r {
                        line[x as usize] = colour;
                    }
                }
            }
        };

        // Seconds ring, starting at 12 o'clock.
        for i in 0..60 {
            let x = 240 + RING_R * SIN60[i] / 1000;
            let y = 240 - RING_R * SIN60[(i + 15) % 60] / 1000;
            let (r, colour) = match self.second {
                Some(s) if i as u32 == s => (5, RING_NOW),
                Some(s) if (i as u32) < s => (3, RING_PAST),
                _ => (3, RING_FUTURE),
            };
            disc(x, y, r, colour);
        }

        // Date, centred under the digits.
        let width = self.date.len() as i32 * 6 * DATE_PITCH - DATE_PITCH;
        let left = 240 - width / 2;
        for (i, &c) in self.date.iter().enumerate() {
            let g = glyph(c);
            for (row, bits) in g.iter().enumerate() {
                for col in 0..5 {
                    if bits & (0x10 >> col) != 0 {
                        let x = left + (i as i32 * 6 + col) * DATE_PITCH;
                        disc(x, DATE_TOP + row as i32 * DATE_PITCH, 2, DATE);
                    }
                }
            }
        }

        // Digits. The colon dims on odd seconds.
        for slot in 0..SLOTS {
            if !self.lit[slot] {
                continue;
            }
            let (hx, hy) = home(slot);
            let (ox, oy) = self.off[slot];
            let (x, y) = (hx + ox / 256, hy + oy / 256);
            // How disturbed the dot is, measured from where it rests.
            let far = ((ox - self.sag.0) / 256).abs().max(((oy - self.sag.1) / 256).abs());
            let colour = if slot >= 4 * 35 && self.second.is_some_and(|s| s % 2 == 1) {
                COLON_DIM
            } else if far > 24 {
                SCATTERED
            } else if far > 6 {
                STIRRED
            } else {
                WARM
            };
            disc(x, y, DOT_R, colour);
        }
    }
}
