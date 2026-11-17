//! Gentle motion: waves, drifting clouds, swaying palms, twinkling stars,
//! tilt parallax and shake. Everything advances here, in `update`, and ends
//! up as a `View` of offsets and phases that the renderers read.

use crate::poster::{View, PALMS};
use app_sdk::Tilt;

/// sin(i * 360 / 256 degrees) * 127.
pub const SIN: [i8; 256] = {
    // A quarter wave, mirrored into the other three.
    const Q: [i8; 65] = [
        0, 3, 6, 9, 12, 16, 19, 22, 25, 28, 31, 34, 37, 40, 43, 46, 49, 51, 54, 57, 60, 63, 65, 68,
        71, 73, 76, 78, 81, 83, 85, 88, 90, 92, 94, 96, 98, 100, 102, 104, 106, 107, 109, 111, 112,
        113, 115, 116, 117, 118, 120, 121, 122, 122, 123, 124, 125, 125, 126, 126, 126, 127, 127,
        127, 127,
    ];
    let mut t = [0i8; 256];
    let mut i = 0;
    while i < 256 {
        let q = i % 128;
        let v = if q <= 64 { Q[q] } else { Q[128 - q] };
        t[i] = if i < 128 { v } else { -v };
        i += 1;
    }
    t
};

/// sin of a phase that runs from 0 to `period`, times `amp`, rounded.
fn wave(ms: u32, period: u32, amp: i32) -> i32 {
    let i = (ms % period) as u64 * 256 / period as u64;
    let s = SIN[i as usize] as i32 * amp;
    if s >= 0 {
        (s + 63) / 127
    } else {
        (s - 63) / 127
    }
}

/// Tilt beyond this many milli-g counts as full tilt.
const FULL_MG: i32 = 500;
/// Tilt filter time constant.
const TILT_MS: i32 = 400;
/// Summed, decaying tilt change that counts as a shake (as in `pikachu`).
const SHAKE_MG: i32 = 3500;
/// Palm sway right after a shake, in pixels at the crown.
const SHAKE_SWAY: i32 = 14;

/// A nudged palm swings this far at first, in pixels at the crown: softer
/// than a shake.
const NUDGE_SWAY: i32 = 7;
/// One swing of a nudged palm takes this long.
const NUDGE_MS: u32 = 1_100;

/// How far each layer moves at full tilt, (x, y) in pixels. Near layers
/// move more, which gives the picture depth.
const PALMS_PX: i32 = 16;
const CITY_PX: i32 = 8;
const FAR_PX: i32 = 6;
const CLOUDS_PX: i32 = 4;
const ORB_PX: i32 = 2;

/// Clouds drift this many milliseconds per pixel (about 2 px a second).
const CLOUD_MS_PER_PX: u32 = 500;
/// Clouds wrap round after this many pixels.
pub const CLOUD_WRAP: u32 = 800;

#[derive(Clone, Copy, Debug)]
pub struct Motion {
    /// Animation clock, ms. Wraps after a few days, where every phase below
    /// has a whole number of periods, so the wrap is not seen.
    ms: u32,
    /// Filtered tilt, milli-g times 16, (x, y - 1000).
    tilt: (i32, i32),
    last_tilt: Option<(i16, i16, i16)>,
    /// Summed tilt change, decaying; above `SHAKE_MG` is a shake.
    shake: i32,
    /// Extra palm sway left from a shake, px times 256.
    sway_q8: i32,
    /// Each palm's nudge from a touch: (size, px times 256, signed for the
    /// direction it was pushed; ms since the touch).
    nudge: [(i32, u32); PALMS],
    started: bool,
}

/// The clock wraps here: a multiple of every period used below.
const CLOCK_WRAP: u32 = 400_000 * 63 * 13;

impl Motion {
    pub const fn new() -> Self {
        Self {
            ms: 0,
            tilt: (0, 0),
            last_tilt: None,
            shake: 0,
            sway_q8: 0,
            nudge: [(0, 0); PALMS],
            started: false,
        }
    }

    pub fn update(&mut self, tilt: Tilt, dt_ms: u32) {
        let dt = dt_ms.min(100);
        self.ms = (self.ms + dt) % CLOCK_WRAP;
        let dt = dt as i32;

        // Tilt: clamp, then a slow low-pass so sensor noise never jitters.
        let tx = (tilt.x_mg as i32).clamp(-FULL_MG, FULL_MG) * 16;
        let ty = (tilt.y_mg as i32 - 1000).clamp(-FULL_MG, FULL_MG) * 16;
        if !self.started {
            self.started = true;
            self.tilt = (tx, ty);
        } else {
            self.tilt.0 += (tx - self.tilt.0) * dt / (TILT_MS + dt);
            self.tilt.1 += (ty - self.tilt.1) * dt / (TILT_MS + dt);
        }

        // Shake: tilt that keeps changing, not a single turn.
        let now = (tilt.x_mg, tilt.y_mg, tilt.z_mg);
        if let Some((x, y, z)) = self.last_tilt {
            let change = (now.0 as i32 - x as i32).abs()
                + (now.1 as i32 - y as i32).abs()
                + (now.2 as i32 - z as i32).abs();
            self.shake = self.shake.saturating_add(change).min(4 * SHAKE_MG);
        }
        self.shake -= self.shake * dt / 150;
        self.last_tilt = Some(now);
        if self.shake > SHAKE_MG {
            self.sway_q8 = SHAKE_SWAY * 256;
        }
        // The shake sway dies away over about two seconds.
        self.sway_q8 -= self.sway_q8 * dt / 600;
        // So does a nudge, a little faster.
        for (amp, age) in self.nudge.iter_mut() {
            *amp -= *amp * dt / 450;
            *age = (*age + dt as u32).min(60_000);
        }
    }

    /// A finger pushed palm `palm` toward the right (`right`) or left.
    pub fn nudge(&mut self, palm: usize, right: bool) {
        if let Some(n) = self.nudge.get_mut(palm) {
            let amp = NUDGE_SWAY * 256;
            *n = (if right { amp } else { -amp }, 0);
        }
    }

    /// How far a nudge still swings palm `palm`, in pixels.
    pub fn nudge_sway(&self, palm: usize) -> i32 {
        self.nudge.get(palm).map_or(0, |n| n.0.abs() / 256)
    }

    /// Filtered tilt in milli-g, clamped to full tilt.
    fn tilt_mg(&self) -> (i32, i32) {
        (self.tilt.0 / 16, self.tilt.1 / 16)
    }

    /// The extra sway left from a shake, in pixels.
    pub fn shake_sway(&self) -> i32 {
        self.sway_q8 / 256
    }

    /// Milliseconds on the animation clock.
    pub fn ms(&self) -> u32 {
        self.ms
    }

    /// Offsets and phases for this frame.
    pub fn view(&self) -> View {
        let (tx, ty) = self.tilt_mg();
        let off = |px: i32| -> (i16, i16) { ((tx * px / FULL_MG) as i16, (ty * px / 4 / FULL_MG) as i16) };
        let ms = self.ms;
        // A slow breeze: up to 2 px at the crown, every five seconds or so,
        // plus a faster, larger swing after a shake.
        let breeze = wave(ms, 5_200, 2);
        let shaken = self.sway_q8 * wave(ms, 900, 127) / 127 / 256;
        let mut sway = [0i16; PALMS];
        for (s, &(amp, age)) in sway.iter_mut().zip(&self.nudge) {
            // Starts moving away from the finger, then swings back and settles.
            let nudged = amp * wave(age, NUDGE_MS, 127) / 127 / 256;
            *s = (breeze + shaken + nudged) as i16;
        }
        let drift = (ms % (CLOUD_WRAP * CLOUD_MS_PER_PX)) / CLOUD_MS_PER_PX;
        View {
            orb: off(ORB_PX),
            clouds: (drift as i32 + off(CLOUDS_PX).0 as i32) as i16,
            far: off(FAR_PX).0,
            city: off(CITY_PX),
            palms: off(PALMS_PX),
            sway,
            wave: ((ms / 350) % 1_200) as u16,
            shimmer: ms,
            twinkle: ms / 700,
            ..View::STILL
        }
    }
}

impl Default for Motion {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upright() -> Tilt {
        Tilt { x_mg: 0, y_mg: 1000, z_mg: 0 }
    }

    #[test]
    fn sine_table() {
        assert_eq!(SIN[0], 0);
        assert_eq!(SIN[64], 127);
        assert_eq!(SIN[192], -127);
        for i in 0..128 {
            assert_eq!(SIN[i], -SIN[i + 128]);
        }
    }

    #[test]
    fn tilt_moves_layers_by_depth() {
        let mut m = Motion::new();
        for _ in 0..90 {
            m.update(upright(), 33);
        }
        let v = m.view();
        assert_eq!((v.palms, v.city, v.orb, v.far), ((0, 0), (0, 0), (0, 0), 0));
        for _ in 0..90 {
            m.update(Tilt { x_mg: 400, y_mg: 1000, z_mg: 0 }, 33);
        }
        let v = m.view();
        assert!(v.palms.0 > v.city.0 && v.city.0 > v.orb.0 && v.orb.0 > 0, "{v:?}");
    }

    #[test]
    fn noise_does_not_jitter() {
        let mut m = Motion::new();
        m.update(upright(), 33);
        let first = m.view();
        for i in 0..300 {
            let x = if i % 2 == 0 { 15 } else { -15 };
            m.update(Tilt { x_mg: x, y_mg: 1000, z_mg: 0 }, 33);
            let v = m.view();
            for (a, b) in [(v.palms.0, first.palms.0), (v.city.0, first.city.0), (v.orb.0, first.orb.0)] {
                assert!((a - b).abs() <= 1);
            }
            assert!((v.palms.1 - first.palms.1).abs() <= 1);
        }
    }

    #[test]
    fn shake_sways_palms() {
        let mut m = Motion::new();
        for _ in 0..30 {
            m.update(upright(), 33);
        }
        assert_eq!(m.shake_sway(), 0);
        for i in 0..10 {
            let x = if i % 2 == 0 { 800 } else { -800 };
            m.update(Tilt { x_mg: x, y_mg: 1000, z_mg: 0 }, 33);
        }
        assert!(m.shake_sway() >= 10);
        for _ in 0..90 {
            m.update(upright(), 33);
        }
        assert!(m.shake_sway() <= 1, "{}", m.shake_sway());
    }

    #[test]
    fn odd_tilt_and_huge_dt_are_safe() {
        let mut m = Motion::new();
        for t in [
            Tilt { x_mg: 0, y_mg: 0, z_mg: 1000 },
            Tilt { x_mg: 0, y_mg: -1000, z_mg: 0 },
            Tilt { x_mg: i16::MIN, y_mg: i16::MIN, z_mg: i16::MIN },
            Tilt { x_mg: i16::MAX, y_mg: i16::MAX, z_mg: i16::MAX },
        ] {
            for _ in 0..60 {
                m.update(t, 33);
                let v = m.view();
                assert!(v.palms.0.abs() <= PALMS_PX as i16 && v.palms.1.abs() <= PALMS_PX as i16);
            }
        }
        m.update(upright(), u32::MAX);
        let _ = m.view();
    }
}
