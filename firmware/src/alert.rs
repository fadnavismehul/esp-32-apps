//! The full-screen reminder card, in the clock's accent colours: the
//! message as large as it fits, the time it was due above it, and a ring of
//! dots with a slow magenta-to-peach wave. Drawn instead of the app, strip by
//! strip.

use core::ops::Range;

use embedded_graphics::prelude::*;

use crate::remind::Alert;
use crate::ui::{CX, MAGENTA, PEACH, isqrt, mix, solid, text};

const DOTS: usize = 60;
const DOT_RING: i32 = 222;
const DOT_R: i32 = 4;
/// One turn of the colour wave around the ring.
const WAVE_MS: u64 = 8000;
/// Where the message block is centred.
const TEXT_MID: i32 = 252;

pub fn render(rows: Range<u16>, buf: &mut [u16], alert: &Alert, shown_ms: u64, now_ms: u64) {
    buf.fill(0);
    dots(&rows, buf, now_ms.saturating_sub(shown_ms));
    let strip: &mut crate::ui::Strip<'_> = &mut (rows, buf);

    // As large as fits: 3x in up to three lines, 2x in four, else 1x.
    let msg = alert.text();
    let (scale, (lines, n)) = match (wrap(msg, 12), wrap(msg, 17)) {
        ((l, n), _) if n <= 3 => (3, (l, n)),
        (_, (l, n)) if n <= 4 => (2, (l, n)),
        _ => (1, wrap(msg, 32)),
    };
    let n = n.min(4);
    let pitch = 22 * scale;
    let top = TEXT_MID - n as i32 * pitch / 2;
    for (i, &(a, b)) in lines[..n].iter().enumerate() {
        let baseline = top + i as i32 * pitch + 16 * scale;
        text(strip, &msg[a..b], scale, baseline, solid(PEACH));
    }

    if let Some(m) = alert.minute {
        let t = m % 1440;
        let time = [b'0' + (t / 600) as u8, b'0' + (t / 60 % 10) as u8, b':', b'0' + (t % 60 / 10) as u8, b'0' + (t % 10) as u8];
        let time = core::str::from_utf8(&time).unwrap_or("");
        text(strip, time, 2, (top - 26).min(110), mix(MAGENTA, (0, 0, 0), 64));
    }
}

/// sin of `deg` degrees, scaled to -256..256 (Bhaskara's approximation).
const fn sin256(deg: i32) -> i32 {
    let d = deg.rem_euclid(360);
    let (x, sign) = if d < 180 { (d, 1) } else { (d - 180, -1) };
    sign * 256 * 4 * x * (180 - x) / (40500 - x * (180 - x))
}

/// Dot centres around the ring, starting at 12 o'clock and going clockwise.
const DOT_AT: [(i32, i32); DOTS] = {
    let mut t = [(0, 0); DOTS];
    let mut i = 0;
    while i < DOTS {
        let a = i as i32 * 360 / DOTS as i32;
        t[i] = (CX + DOT_RING * sin256(a) / 256, CX - DOT_RING * sin256(a + 90) / 256);
        i += 1;
    }
    t
};

/// The ring of dots. A wave of colour runs round it, magenta into peach.
fn dots(rows: &Range<u16>, buf: &mut [u16], since_ms: u64) {
    let turn = ((since_ms % WAVE_MS) * 360 / WAVE_MS) as i32;
    let (y0, y1) = (rows.start as i32, rows.end as i32);
    for (i, &(x, y)) in DOT_AT.iter().enumerate() {
        if y + DOT_R < y0 || y - DOT_R >= y1 {
            continue;
        }
        let wave = sin256(i as i32 * 360 / DOTS as i32 - turn); // -256..256
        let c = mix(MAGENTA, PEACH, (wave + 256) / 2).into_storage();
        for yy in (y - DOT_R).max(y0)..(y + DOT_R + 1).min(y1) {
            let dy = yy - y;
            let dx = isqrt(DOT_R * DOT_R - dy * dy);
            let row = (yy - y0) as usize * 480;
            buf[row + (x - dx) as usize..=row + (x + dx) as usize].fill(c);
        }
    }
}

/// Greedy word wrap into at most 4 lines of `width` characters, as byte
/// ranges into `s` (ASCII). Returns the lines and how many there are; a
/// count over 4 means it did not fit and the rest is cut.
fn wrap(s: &str, width: usize) -> ([(usize, usize); 4], usize) {
    let b = s.as_bytes();
    let mut lines = [(0, 0); 4];
    let mut n = 0;
    let mut start = 0;
    while start < b.len() {
        while start < b.len() && b[start] == b' ' {
            start += 1;
        }
        if start == b.len() {
            break;
        }
        let mut end = (start + width).min(b.len());
        if end < b.len() {
            // Break at the last space that fits, or split a long word.
            if let Some(sp) = b[start..=end].iter().rposition(|&c| c == b' ') {
                if sp > 0 {
                    end = start + sp;
                }
            }
        }
        if n < 4 {
            lines[n] = (start, end);
        }
        n += 1;
        start = end;
    }
    (lines, n.min(5))
}
