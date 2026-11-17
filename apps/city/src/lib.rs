#![no_std]
//! City Window: a neon beach city at the water's edge that follows the clock.
//!
//! - `art` holds every colour, building, palm and timing you may want to change.
//! - `sky` turns the time of day into a `Palette`.
//! - `poster` draws the poster look from that palette.
//! - `dots` draws the same scene as a halftone dot grid (KEY switches).
//! - `motion`, `events` and `digits` add movement, rare events and the time.

pub mod art;
pub mod digits;
pub mod dots;
pub mod events;
pub mod motion;
pub mod poster;
pub mod sky;

#[cfg(test)]
extern crate std;

use app_sdk::{App, Input, Output};
use core::ops::Range;
use dots::Dots;
use events::Events;
use motion::Motion;
use poster::{Scene, View};
use sky::Palette;

/// The two looks. KEY switches between them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    Poster,
    Dots,
}

pub struct City {
    /// Which look is showing. Kept in RAM only, so it starts as the poster.
    look: Look,
    /// The dot look's sizes and colours, filled only while it shows.
    dots: Dots,
    /// What is on screen now. It eases toward the time of day's palette.
    shown: Palette,
    /// Layer offsets and animation phases.
    view: View,
    /// Colours worked out from `shown` and `view` for this frame.
    scene: Scene,
    /// `update` has run at least once.
    started: bool,
    /// Tilt, shake and animation phases.
    motion: Motion,
    /// The rare small events: tail lights, a plane, the tower lights flickering.
    events: Events,
    /// Seconds after local midnight, once the clock is known.
    time: Option<u32>,
    /// A finger was on the screen last update.
    was_down: bool,
    /// The current touch began on a palm.
    on_palm: bool,
}

impl City {
    pub const fn new() -> Self {
        Self {
            look: Look::Poster,
            dots: Dots::new(),
            shown: sky::SUNSET,
            view: View::STILL,
            scene: Scene::new(&sky::SUNSET, &View::STILL),
            started: false,
            motion: Motion::new(),
            events: Events::new(),
            time: None,
            was_down: false,
            on_palm: false,
        }
    }
}

impl Default for City {
    fn default() -> Self {
        Self::new()
    }
}

impl App for City {
    fn update(&mut self, input: &Input, dt_ms: u32, _out: &mut Output) {
        self.time = input.local_time_s.map(|t| t % 86_400);
        let target = match input.local_time_s {
            Some(t) => sky::target(t % 86_400),
            None => sky::SUNSET,
        };
        if !self.started {
            // Known time on the very first frame (an app switch, or the
            // simulator): show it straight away. Otherwise the sunset shows
            // until the clock syncs, and then crossfades.
            self.started = true;
            self.events.seed(input.time_ms);
            if input.local_time_s.is_some() {
                self.shown = target;
            }
        } else {
            sky::approach(&mut self.shown, &target, dt_ms);
        }
        self.motion.update(input.tilt, dt_ms);
        if input.key {
            self.look = match self.look {
                Look::Poster => Look::Dots,
                Look::Dots => Look::Poster,
            };
        }
        let poster = self.look == Look::Poster;
        // A finger landing on a palm nudges it away from the finger; such a
        // touch does not start an event. A tap anywhere else still does.
        let touch = input.touch;
        let landed = touch.down && !self.was_down;
        // A tap with no touch-down seen first (a quick tap, or one sent by
        // `espctl input --tap`) counts as landing where it was.
        let bare_tap = touch.tapped && !self.was_down && !touch.down;
        if landed || bare_tap {
            let (x, y) = (touch.x.min(479) as i32, touch.y.min(479) as i32);
            self.on_palm = false;
            if let Some((palm, mid)) = poster::palm_at(&self.view, x, y) {
                self.motion.nudge(palm, x < mid);
                self.on_palm = true;
            }
        }
        self.was_down = touch.down;
        // A touch that began on a palm never starts an event, even if the
        // finger slid off the palm before it lifted.
        let tapped = touch.tapped && !self.on_palm;
        if touch.tapped {
            self.on_palm = false;
        }
        // Events show in the poster only; switching to dots ends one.
        self.events.update(dt_ms, self.shown.night, tapped, poster);
        let mut view = self.motion.view();
        view.event = self.events.current().map(|(kind, prog)| (kind, prog, self.events.salt()));
        // The plane's light blinks once a second.
        view.blink = self.motion.ms() % 1000 < 150;
        self.view = view;
        self.scene = Scene::new(&self.shown, &self.view);
        if !poster {
            self.dots.update(&self.scene, &self.view, self.motion.ms());
        }
    }

    fn render(&self, rows: Range<u16>, buf: &mut [u16]) {
        assert!(rows.start <= rows.end && rows.end <= 480);
        assert_eq!(buf.len(), (rows.end - rows.start) as usize * 480);
        match self.look {
            Look::Poster => poster::render(&self.scene, &self.view, rows.clone(), buf),
            Look::Dots => self.dots.render(rows.clone(), buf),
        }
        if let Some(t) = self.time {
            let (x0, y0, x1, y1) = digits::bounds(t);
            for (i, y) in rows.enumerate() {
                let row = &mut buf[i * 480..(i + 1) * 480];
                let y = y as i32;
                // On the dot grid the time needs a black patch to read.
                if self.look == Look::Dots && (y0 - 3..y1 + 3).contains(&y) {
                    row[(x0 - 3).max(0) as usize..(x1 + 3).min(480) as usize].fill(0);
                }
                digits::draw_row(row, y, t, self.scene.clock);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_sdk::{Input, Output};
    use std::{vec, vec::Vec};

    /// 2026-10-26 00:00 local.
    const DAY: u32 = 1_792_972_800;

    const fn hm(h: u32, m: u32) -> u32 {
        h * 3600 + m * 60
    }

    fn at(t: Option<u32>) -> Input {
        Input { local_time_s: t, ..Input::default() }
    }

    pub(crate) fn frame(city: &City, strip: u16) -> Vec<u16> {
        let mut out = vec![0u16; 480 * 480];
        let mut buf = vec![0u16; strip as usize * 480];
        for row in (0..480u16).step_by(strip as usize) {
            let rows = row..row + strip;
            city.render(rows, &mut buf);
            let start = row as usize * 480;
            let end = start + strip as usize * 480;
            out[start..end].copy_from_slice(&buf);
        }
        out
    }

    fn city_at(sec_of_day: u32) -> City {
        let mut c = City::new();
        c.update(&at(Some(DAY + sec_of_day)), 33, &mut Output::default());
        c
    }

    #[test]
    fn sunset_sky_before_sync() {
        let mut c = City::new();
        c.update(&at(None), 33, &mut Output::default());
        assert_eq!(c.shown, sky::SUNSET);
        let full = frame(&c, 12);
        assert_ne!(full[0..480], full[200 * 480..201 * 480]);
        // A row of open sky is one flat colour, apart from a star or two.
        let row = &full[10 * 480..11 * 480];
        assert!(row.iter().filter(|&&p| p != row[240]).count() < 8);
    }

    #[test]
    fn strips_match() {
        let mut c = City::new();
        c.update(&Input::default(), 33, &mut Output::default());
        assert_eq!(frame(&c, 2), frame(&c, 12));
        for t in [hm(6, 15), hm(13, 0), hm(18, 15), hm(23, 0)] {
            let c = city_at(t);
            assert_eq!(frame(&c, 2), frame(&c, 12), "at {t}");
        }
    }

    #[test]
    fn sync_crossfades() {
        for (before, after) in [(None, hm(2, 0)), (Some(hm(12, 0)), hm(3, 0))] {
            let mut c = City::new();
            for _ in 0..30 {
                c.update(&at(before.map(|s| DAY + s)), 33, &mut Output::default());
            }
            let mut prev = c.shown;
            c.update(&at(Some(DAY + after)), 33, &mut Output::default());
            let (col, _, n) = sky::max_diff(&prev, &c.shown);
            assert!(col <= 12 && n <= 12, "first step {col} {n}");
            assert_ne!(prev, c.shown);
            for _ in 1..150 {
                prev = c.shown;
                c.update(&at(Some(DAY + after)), 33, &mut Output::default());
                let (col, _, n) = sky::max_diff(&prev, &c.shown);
                assert!(col <= 12 && n <= 12);
            }
            assert_eq!(c.shown, sky::target(after));
        }
    }

    #[test]
    fn midnight_wrap() {
        let mut c = City::new();
        let t0 = DAY + hm(23, 59) + 50;
        c.update(&at(Some(t0)), 33, &mut Output::default());
        let start = c.shown;
        for s in 0..20 {
            c.update(&at(Some(t0 + s)), 1000, &mut Output::default());
        }
        let (col, pos, n) = sky::max_diff(&start, &c.shown);
        assert!(col <= 2 && pos <= 2 && n <= 2);
    }

    #[test]
    fn night_is_mostly_black() {
        let c = city_at(hm(23, 0));
        let f = frame(&c, 12);
        let black = f.iter().filter(|&&p| p == 0).count();
        assert!(black * 100 >= f.len() * 45, "{}% black", black * 100 / f.len());
    }

    #[test]
    fn state_size() {
        assert!(core::mem::size_of::<City>() <= 14 * 1024, "{}", core::mem::size_of::<City>());
    }

    #[test]
    fn row_work_is_bounded() {
        for t in [hm(6, 15), hm(13, 0), hm(18, 15), hm(23, 0)] {
            let c = city_at(t);
            let mut row = [0u16; 480];
            for y in 0..480 {
                poster::count::reset();
                poster::draw_row(&mut row, &c.scene, &c.view, y);
                let (fills, pixels) = poster::count::get();
                assert!(fills <= 64, "{fills} fills on row {y} at {t}");
                assert!(pixels <= 200, "{pixels} overlay pixels on row {y} at {t}");
            }
        }
    }

    fn tilted(sec: u32, x_mg: i16) -> Input {
        Input {
            tilt: app_sdk::Tilt { x_mg, y_mg: 1000, z_mg: 0 },
            ..at(Some(DAY + sec))
        }
    }

    #[test]
    fn never_still() {
        for t in [hm(18, 15), hm(23, 0)] {
            let mut c = city_at(t);
            let a = frame(&c, 12);
            for i in 1..=30 {
                c.update(&at(Some(DAY + t + i / 30)), 33, &mut Output::default());
            }
            assert_ne!(a, frame(&c, 12), "still at {t}");
        }
    }

    #[test]
    fn odd_tilt_is_safe() {
        for tilt in [
            app_sdk::Tilt { x_mg: 0, y_mg: 0, z_mg: 1000 },
            app_sdk::Tilt { x_mg: 0, y_mg: -1000, z_mg: 0 },
            app_sdk::Tilt { x_mg: i16::MIN, y_mg: i16::MIN, z_mg: i16::MIN },
            app_sdk::Tilt { x_mg: i16::MAX, y_mg: i16::MAX, z_mg: i16::MAX },
        ] {
            let mut c = City::new();
            for _ in 0..60 {
                c.update(&Input { tilt, ..at(Some(DAY + hm(23, 0))) }, 33, &mut Output::default());
            }
            assert_eq!(frame(&c, 2), frame(&c, 12));
        }
    }

    #[test]
    fn tilt_strips_match() {
        let mut c = City::new();
        for _ in 0..60 {
            c.update(&tilted(hm(18, 15), -450), 33, &mut Output::default());
        }
        assert_eq!(frame(&c, 2), frame(&c, 12));
    }

    /// A City at 22:00 running an event of `kind`, part way through.
    fn with_event(kind: events::Kind, frames: u32) -> City {
        for seed in 0..200u32 {
            let mut c = City::new();
            let base = Input { time_ms: seed * 7919, ..at(Some(DAY + hm(22, 0))) };
            c.update(&base, 33, &mut Output::default());
            let tap = Input { touch: app_sdk::Touch { tapped: true, ..Default::default() }, ..base };
            c.update(&tap, 33, &mut Output::default());
            if c.events.current().map(|e| e.0) == Some(kind) {
                for _ in 0..frames {
                    c.update(&base, 33, &mut Output::default());
                }
                return c;
            }
        }
        panic!("no {kind:?} event");
    }

    #[test]
    fn events_draw_and_strips_match() {
        use events::Kind;
        // Frames into each event; the flicker is in an "off" step then.
        for (kind, frames) in [(Kind::TailLights, 45), (Kind::Plane, 45), (Kind::CrownFlicker, 16)] {
            let c = with_event(kind, frames);
            assert_eq!(c.view.event.map(|e| e.0), Some(kind));
            let f = frame(&c, 2);
            assert!(f == frame(&c, 12), "{kind:?} strips differ");
            // The event changes the picture.
            let mut quiet = c.view;
            quiet.event = None;
            let mut plain = std::vec![0u16; 480 * 480];
            let scene = Scene::new(&c.shown, &quiet);
            poster::render(&scene, &quiet, 0..480, &mut plain);
            assert!(f != plain, "{kind:?} draws nothing");
        }
    }

    #[test]
    fn time_text() {
        let c = city_at(hm(5, 7));
        let f = frame(&c, 12);
        let (x0, y0, x1, y1) = digits::bounds(hm(5, 7));
        // Each font cell's centre is lit exactly when the glyph says so.
        let cols = [0, 4, 10, 14];
        for (k, d) in [0usize, 5, 0, 7].into_iter().enumerate() {
            for cy in 0..5 {
                for cx in 0..3 {
                    let x = x0 + (cols[k] + cx) * digits::CELL + 2;
                    let y = y0 + cy * digits::CELL + 2;
                    let lit = f[y as usize * 480 + x as usize] == c.scene.clock;
                    let want = digits::GLYPHS[d][cy as usize] & (0b100 >> cx) != 0;
                    assert_eq!(lit, want, "digit {k} cell {cx},{cy}");
                }
            }
        }
        // Without a known time nothing is drawn: only the box differs.
        let mut none = city_at(hm(5, 7));
        none.time = None;
        let g = frame(&none, 12);
        for y in 0..480 {
            for x in 0..480 {
                let inside = (x0..x1).contains(&x) && (y0..y1).contains(&y);
                if !inside {
                    assert_eq!(f[(y * 480 + x) as usize], g[(y * 480 + x) as usize]);
                }
            }
        }
        assert_ne!(f, g);
    }

    #[test]
    fn time_drifts() {
        assert_ne!(digits::bounds(hm(12, 0)), digits::bounds(hm(12, 3)));
        assert_eq!(digits::bounds(hm(12, 0)), digits::bounds(hm(12, 1)));
        // It stays on screen and over the sea.
        for m in 0..1440 {
            let (x0, y0, x1, y1) = digits::bounds(m * 60);
            assert!(x0 >= 0 && x1 <= 480 && y0 > art::HORIZON as i32 + 60 && y1 <= 480);
        }
    }

    fn key(sec: u32) -> Input {
        Input { key: true, ..at(Some(DAY + sec)) }
    }

    #[test]
    fn key_toggles_look() {
        let t = hm(18, 15);
        let mut a = City::new();
        let mut b = City::new();
        for _ in 0..5 {
            a.update(&at(Some(DAY + t)), 33, &mut Output::default());
            b.update(&at(Some(DAY + t)), 33, &mut Output::default());
        }
        let before = frame(&a, 12);
        a.update(&key(t), 33, &mut Output::default());
        b.update(&at(Some(DAY + t)), 33, &mut Output::default());
        assert_eq!(a.look, Look::Dots);
        assert_ne!(frame(&a, 12), before);
        a.update(&key(t), 33, &mut Output::default());
        b.update(&at(Some(DAY + t)), 33, &mut Output::default());
        assert_eq!(a.look, Look::Poster);
        assert!(frame(&a, 12) == frame(&b, 12));
    }

    fn dots_at(sec: u32, x_mg: i16) -> City {
        let mut c = City::new();
        c.update(&Input { key: true, ..tilted(sec, x_mg) }, 33, &mut Output::default());
        for _ in 0..30 {
            c.update(&tilted(sec, x_mg), 33, &mut Output::default());
        }
        c
    }

    #[test]
    fn dots_strips_match() {
        for t in [hm(13, 0), hm(23, 0)] {
            let c = dots_at(t, 350);
            assert_eq!(c.look, Look::Dots);
            assert!(frame(&c, 2) == frame(&c, 12), "dots strips differ at {t}");
        }
    }

    #[test]
    fn dots_move() {
        let mut c = dots_at(hm(18, 15), 0);
        let a = frame(&c, 12);
        for _ in 0..30 {
            c.update(&tilted(hm(18, 15), 0), 33, &mut Output::default());
        }
        assert!(a != frame(&c, 12));
    }

    fn tap_at(sec: u32, x: u16, y: u16) -> Input {
        Input {
            touch: app_sdk::Touch { down: false, x, y, tapped: true },
            ..at(Some(DAY + sec))
        }
    }

    #[test]
    fn sliding_off_a_palm_starts_no_event() {
        let mut c = City::new();
        let mut out = Output::default();
        let base = Input { local_time_s: Some(22 * 3600), ..Default::default() };
        c.update(&base, 33, &mut out);
        // Find a palm and a spot of open sky.
        let (px, py) = (0..480).flat_map(|y| (0..480).map(move |x| (x, y)))
            .find(|&(x, y)| poster::palm_at(&c.view, x, y).is_some()).unwrap();
        assert!(poster::palm_at(&c.view, 240, 20).is_none());
        let touch = |down, x: i32, y: i32, tapped| Input {
            touch: app_sdk::Touch { down, x: x as u16, y: y as u16, tapped },
            ..base
        };
        c.update(&touch(true, px, py, false), 33, &mut out);
        c.update(&touch(true, 240, 20, false), 33, &mut out);
        c.update(&touch(false, 240, 20, true), 33, &mut out);
        assert!(c.events.current().is_none());
        // The next tap on open sky does start one.
        c.update(&touch(false, 240, 20, true), 33, &mut out);
        assert!(c.events.current().is_some());
    }

    #[test]
    fn touching_a_palm_nudges_it() {
        let t = hm(22, 0);
        // A point on the left palm's trunk, and one in open sky.
        let row = &poster::PALM_ROWS[0];
        let (a, z) = row_run(row, 300);
        for look_key in [false, true] {
            let mut c = city_at(t);
            if look_key {
                c.update(&key(t), 33, &mut Output::default());
            }
            c.update(&tap_at(t, ((a + z) / 2) as u16, 300), 33, &mut Output::default());
            assert!(c.motion.nudge_sway(0) >= 5);
            assert_eq!(c.motion.nudge_sway(1), 0);
            assert_eq!(c.events.current(), None);
            for _ in 0..90 {
                c.update(&at(Some(DAY + t)), 33, &mut Output::default());
            }
            assert!(c.motion.nudge_sway(0) == 0, "still swaying");
        }
        let mut c = city_at(t);
        c.update(&tap_at(t, 240, 40), 33, &mut Output::default());
        assert!(c.events.current().is_some());
        assert_eq!((c.motion.nudge_sway(0), c.motion.nudge_sway(1)), (0, 0));
        // The right palm too, by its crown.
        let mut c = city_at(t);
        let crown = art::PALMS[1].crown;
        c.update(&tap_at(t, crown.0 as u16, crown.1 as u16), 33, &mut Output::default());
        assert!(c.motion.nudge_sway(1) >= 5 && c.motion.nudge_sway(0) == 0);
    }

    fn row_run(p: &poster::PalmRows, y: usize) -> (i16, i16) {
        p.first_run(y).expect("palm on this row")
    }

    #[test]
    fn huge_dt_is_safe() {
        let mut c = city_at(hm(12, 0));
        c.update(&Input { time_ms: u32::MAX - 5, ..at(Some(u32::MAX)) }, u32::MAX, &mut Output::default());
        assert_eq!(c.events.current(), None);
        let _ = frame(&c, 12);
    }
}

