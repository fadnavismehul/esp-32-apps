//! The time-of-day model: turns the time into a set of colours and positions.
//!
//! Everything here is pure. `target` says what the scene should look like at
//! a time of day. `approach` moves what is on screen toward that, a little
//! each frame, so a jump in the clock becomes a short crossfade.

use crate::art::{self, Key, Rgb, SKY_BANDS};

/// What the scene looks like at one moment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Palette {
    pub sky: [Rgb; SKY_BANDS],
    pub sea: [Rgb; 3],
    pub skyline: Rgb,
    pub tones: [Rgb; 3],
    pub far: Rgb,
    pub orb: Rgb,
    pub orb_low: Rgb,
    pub cloud: Rgb,
    pub palm: Rgb,
    pub orb_y: i16,
    pub orb_r: i16,
    /// 0 shows the sun, 255 the moon.
    pub moon: u8,
    /// 0 by day, 255 at full night.
    pub night: u8,
}

/// Blend steps between two keys: 0 is the first key, `ONE` the second.
const ONE: u32 = 4096;

const fn mix8(a: u8, b: u8, t: u32) -> u8 {
    ((a as u32 * (ONE - t) + b as u32 * t + ONE / 2) / ONE) as u8
}

const fn mix_rgb(a: Rgb, b: Rgb, t: u32) -> Rgb {
    (mix8(a.0, b.0, t), mix8(a.1, b.1, t), mix8(a.2, b.2, t))
}

const fn mix_i16(a: i16, b: i16, t: u32) -> i16 {
    let d = (b as i32 - a as i32) * t as i32;
    // Round to nearest, symmetric for negative steps.
    let r = if d >= 0 { (d + ONE as i32 / 2) / ONE as i32 } else { (d - ONE as i32 / 2) / ONE as i32 };
    (a as i32 + r) as i16
}

const fn mix_n<const N: usize>(a: [Rgb; N], b: [Rgb; N], t: u32) -> [Rgb; N] {
    let mut out = a;
    let mut i = 0;
    while i < N {
        out[i] = mix_rgb(a[i], b[i], t);
        i += 1;
    }
    out
}

const fn blend(a: &Key, b: &Key, t: u32) -> Palette {
    let (la, lb) = (&a.look, &b.look);
    Palette {
        sky: mix_n(la.sky, lb.sky, t),
        sea: mix_n(la.sea, lb.sea, t),
        skyline: mix_rgb(la.skyline, lb.skyline, t),
        tones: mix_n(la.tones, lb.tones, t),
        far: mix_rgb(la.far, lb.far, t),
        orb: mix_rgb(la.orb, lb.orb, t),
        orb_low: mix_rgb(la.orb_low, lb.orb_low, t),
        cloud: mix_rgb(la.cloud, lb.cloud, t),
        palm: mix_rgb(la.palm, lb.palm, t),
        orb_y: mix_i16(a.orb_y, b.orb_y, t),
        orb_r: mix_i16(a.orb_r, b.orb_r, t),
        moon: mix8(if a.moon { 255 } else { 0 }, if b.moon { 255 } else { 0 }, t),
        night: mix8(a.night, b.night, t),
    }
}

/// The palette for a time of day, in seconds after midnight. Blends in a
/// straight line between the two keys around that time, wrapping from the
/// last key of the day to the first.
pub const fn target(sec_of_day: u32) -> Palette {
    let keys = art::KEYS;
    let n = keys.len();
    let sec = sec_of_day % 86_400;
    // The last key at or before `sec`; before the first key, the day's last.
    let mut i = n - 1;
    let mut k = 0;
    while k < n {
        if keys[k].at_min as u32 * 60 <= sec {
            i = k;
        }
        k += 1;
    }
    let a = &keys[i];
    let b = &keys[(i + 1) % n];
    let start = a.at_min as u32 * 60;
    let span = (b.at_min as u32 * 60 + 86_400 - start) % 86_400;
    let pos = (sec + 86_400 - start) % 86_400;
    let t = if span == 0 { 0 } else { (pos as u64 * ONE as u64 / span as u64) as u32 };
    blend(a, b, t)
}

/// What shows before the clock is known: the sunset.
pub const SUNSET: Palette = target(art::BEFORE_SYNC_MIN as u32 * 60);

/// A full change (black to white, or sunset to night) takes about this long.
const CROSSFADE_MS: u32 = 3000;

fn step8(v: &mut u8, to: u8, max: u32) {
    let d = to as i32 - *v as i32;
    let m = max as i32;
    *v = (*v as i32 + d.clamp(-m, m)) as u8;
}

fn step16(v: &mut i16, to: i16, max: u32) {
    let d = to as i32 - *v as i32;
    let m = max as i32;
    *v = (*v as i32 + d.clamp(-m, m)) as i16;
}

fn step_rgb(v: &mut Rgb, to: Rgb, max: u32) {
    step8(&mut v.0, to.0, max);
    step8(&mut v.1, to.1, max);
    step8(&mut v.2, to.2, max);
}

/// Moves every field of `shown` toward `to`. The rate crosses the whole
/// colour range in about 3 seconds and the whole sky height in about 1.5, so
/// normal minute-to-minute changes are followed exactly and a clock jump
/// becomes a crossfade.
pub fn approach(shown: &mut Palette, to: &Palette, dt_ms: u32) {
    let dt = dt_ms.min(CROSSFADE_MS);
    if dt == 0 {
        return;
    }
    let c = (dt * 255 / CROSSFADE_MS).max(1);
    let px = (dt * 480 / CROSSFADE_MS).max(1);
    for i in 0..SKY_BANDS {
        step_rgb(&mut shown.sky[i], to.sky[i], c);
    }
    for i in 0..3 {
        step_rgb(&mut shown.sea[i], to.sea[i], c);
        step_rgb(&mut shown.tones[i], to.tones[i], c);
    }
    step_rgb(&mut shown.skyline, to.skyline, c);
    step_rgb(&mut shown.far, to.far, c);
    step_rgb(&mut shown.orb, to.orb, c);
    step_rgb(&mut shown.orb_low, to.orb_low, c);
    step_rgb(&mut shown.cloud, to.cloud, c);
    step_rgb(&mut shown.palm, to.palm, c);
    step16(&mut shown.orb_y, to.orb_y, px);
    step16(&mut shown.orb_r, to.orb_r, px);
    step8(&mut shown.moon, to.moon, c);
    step8(&mut shown.night, to.night, c);
}

#[cfg(test)]
pub(crate) fn max_diff(a: &Palette, b: &Palette) -> (u8, i16, u8) {
    fn d(a: Rgb, b: Rgb) -> u8 {
        a.0.abs_diff(b.0).max(a.1.abs_diff(b.1)).max(a.2.abs_diff(b.2))
    }
    let mut c = 0;
    for i in 0..SKY_BANDS {
        c = c.max(d(a.sky[i], b.sky[i]));
    }
    for i in 0..3 {
        c = c.max(d(a.sea[i], b.sea[i])).max(d(a.tones[i], b.tones[i]));
    }
    for (x, y) in [
        (a.skyline, b.skyline),
        (a.far, b.far),
        (a.orb, b.orb),
        (a.orb_low, b.orb_low),
        (a.cloud, b.cloud),
        (a.palm, b.palm),
    ] {
        c = c.max(d(x, y));
    }
    let pos = (a.orb_y - b.orb_y).abs().max((a.orb_r - b.orb_r).abs());
    let n = a.moon.abs_diff(b.moon).max(a.night.abs_diff(b.night));
    (c, pos, n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::art::HORIZON;

    const fn hm(h: u32, m: u32) -> u32 {
        h * 3600 + m * 60
    }

    #[test]
    fn keys_are_sorted_and_in_range() {
        for w in art::KEYS.windows(2) {
            assert!(w[0].at_min < w[1].at_min);
        }
        for k in art::KEYS {
            assert!(k.at_min < 1440);
        }
    }

    #[test]
    fn no_jumps_over_24h() {
        for m in 0..1440u32 {
            let a = target(m * 60);
            let b = target(((m + 1) % 1440) * 60);
            let (c, pos, n) = max_diff(&a, &b);
            assert!(c <= 6, "colour jump of {c} at minute {m}");
            assert!(pos <= 3, "orb jump of {pos} at minute {m}");
            assert!(n <= 8, "night or moon jump of {n} at minute {m}");
        }
    }

    #[test]
    fn moon_swaps_only_while_hidden() {
        for m in 0..1440u32 {
            let p = target(m * 60);
            if p.moon != 0 && p.moon != 255 {
                assert!(p.orb_y - p.orb_r > HORIZON, "swap seen at minute {m}");
            }
        }
    }

    #[test]
    fn phases() {
        let noon = target(hm(13, 0));
        let night = target(hm(23, 0));
        let sunset = target(hm(18, 15));
        assert_eq!(noon.night, 0);
        assert_eq!(night.night, 255);
        assert_eq!(night.sky[0], (0, 0, 0));
        // The sun sits on the skyline: its centre within 40 rows of the low
        // buildings in the middle, about 40 rows above the horizon.
        assert!((sunset.orb_y - (HORIZON - 40)).abs() <= 40, "orb_y {}", sunset.orb_y);
        assert!(noon.orb_y < sunset.orb_y);
        assert_eq!(SUNSET, sunset);
    }

    #[test]
    fn approach_reaches_target_and_is_bounded() {
        let mut shown = target(hm(12, 0));
        let to = target(hm(23, 0));
        let mut frames = 0;
        while shown != to {
            let before = shown;
            approach(&mut shown, &to, 33);
            let (c, _, n) = max_diff(&before, &shown);
            assert!(c <= 12 && n <= 12);
            frames += 1;
            assert!(frames < 150);
        }
        // Huge steps are clamped and safe.
        approach(&mut shown, &target(0), u32::MAX);
    }
}
