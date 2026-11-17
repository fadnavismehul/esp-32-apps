//! One-at-a-time rare event scheduler.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    TailLights,
    Plane,
    CrownFlicker,
}

#[derive(Clone, Copy, Debug)]
struct ActiveEvent {
    kind: Kind,
    elapsed_ms: u32,
    duration_ms: u32,
    /// A random number for the drawing: direction, height, which light.
    salt: u32,
}

#[derive(Debug)]
pub struct Events {
    current: Option<ActiveEvent>,
    next_in_ms: u32,
    rng: u32,
}

impl Events {
    pub const fn new() -> Self {
        Self {
            current: None,
            next_in_ms: 60_000,
            rng: 0x9e37_79b9,
        }
    }

    /// Seeds the random numbers (City uses the boot time of its first
    /// update) and picks the first gap from them.
    pub fn seed(&mut self, seed: u32) {
        let s = seed ^ 0x9e37_79b9;
        self.rng = if s == 0 { 0x2545_f491 } else { s };
        // Warm up, then pick the first gap: 30 to 90 seconds.
        for _ in 0..4 {
            self.rand();
        }
        self.next_in_ms = 30_000 + self.rand() % 60_001;
    }

    fn rand(&mut self) -> u32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        x
    }

    fn pick_kind(&mut self, night: u8) -> Kind {
        if night >= 96 {
            match self.rand() % 3 {
                0 => Kind::TailLights,
                1 => Kind::Plane,
                _ => Kind::CrownFlicker,
            }
        } else {
            Kind::Plane
        }
    }

    fn start_event(&mut self, night: u8) {
        let kind = self.pick_kind(night);
        let duration_ms = match kind {
            Kind::TailLights => 6_000,
            Kind::Plane => 12_000,
            Kind::CrownFlicker => 2_000,
        };
        let salt = self.rand();
        self.current = Some(ActiveEvent {
            kind,
            elapsed_ms: 0,
            duration_ms,
            salt,
        });
    }

    pub fn update(&mut self, dt_ms: u32, night: u8, tapped: bool, poster: bool) {
        // A long pause (an alert card, an app switch) must not start a
        // burst of events, so time moves at most 100 ms per update.
        let dt_ms = dt_ms.min(100);
        if !poster {
            self.current = None;
            return;
        }

        if let Some(ref mut active) = self.current {
            active.elapsed_ms = active.elapsed_ms.saturating_add(dt_ms);
            if active.elapsed_ms >= active.duration_ms {
                self.current = None;
                self.next_in_ms = 60_000 + (self.rand() % 60_001);
            }
            // A tap while one runs changes nothing
            return;
        }

        // None running
        if tapped {
            self.start_event(night);
            return;
        }

        if self.next_in_ms <= dt_ms {
            self.start_event(night);
        } else {
            self.next_in_ms -= dt_ms;
        }
    }

    /// The random number of the running event.
    pub fn salt(&self) -> u32 {
        self.current.map_or(0, |a| a.salt)
    }

    pub fn current(&self) -> Option<(Kind, u16)> {
        self.current.as_ref().map(|active| {
            let progress = if active.duration_ms == 0 {
                1000
            } else {
                let p = (active.elapsed_ms as u64 * 1000) / active.duration_ms as u64;
                p.min(1000) as u16
            };
            (active.kind, progress)
        })
    }
}

impl Default for Events {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_at_a_time_and_rare() {
        let mut events = Events::new();
        let mut started = 0;
        let mut was_running = false;
        // 30 minutes at 33 ms steps = 1800 * 1000 / 33 ≈ 54545 steps
        let steps = (30 * 60 * 1000) / 33;
        for _ in 0..steps {
            events.update(33, 255, false, true);
            let cur = events.current();
            let is_running = cur.is_some();
            if is_running && !was_running {
                started += 1;
            }
            was_running = is_running;
        }
        assert!(
            (12..=32).contains(&started),
            "started {started} events, expected between 12 and 32"
        );
    }

    #[test]
    fn tap_starts_one() {
        let mut events = Events::new();
        assert_eq!(events.current(), None);
        // Tap with none running starts one
        events.update(33, 255, true, true);
        let cur = events.current();
        assert!(cur.is_some(), "tap should start an event");
        let (first_kind, first_prog) = cur.unwrap();

        // Tap while one runs changes nothing (same kind, progresses normally)
        events.update(33, 255, true, true);
        let cur2 = events.current();
        assert!(cur2.is_some());
        let (second_kind, second_prog) = cur2.unwrap();
        assert_eq!(first_kind, second_kind);
        assert!(second_prog >= first_prog);
    }

    #[test]
    fn day_only_plane() {
        let mut events = Events::new();
        let mut plane_count = 0;
        let mut was_running = false;
        let steps = (30 * 60 * 1000) / 33;
        for _ in 0..steps {
            events.update(33, 0, false, true);
            if let Some((kind, _)) = events.current() {
                assert_eq!(kind, Kind::Plane, "only Plane allowed during day (night=0)");
                if !was_running {
                    plane_count += 1;
                }
                was_running = true;
            } else {
                was_running = false;
            }
        }
        assert!(plane_count > 0, "should have started at least one plane event");
    }

    #[test]
    fn huge_dt_starts_nothing() {
        let mut events = Events::new();
        events.update(u32::MAX, 255, false, true);
        assert_eq!(events.current(), None);
    }

    #[test]
    fn seeds_differ() {
        let mut a = Events::new();
        let mut b = Events::new();
        a.seed(1234);
        b.seed(98_765);
        assert_ne!(a.next_in_ms, b.next_in_ms);
        assert!((30_000..=90_000).contains(&a.next_in_ms));
        a.seed(0x9e37_79b9);
        a.update(33, 255, true, true);
        assert!(a.current().is_some());
    }

    #[test]
    fn not_in_dots() {
        let mut events = Events::new();
        // Even with a tap, nothing starts in dots mode (poster = false)
        events.update(33, 255, true, false);
        assert_eq!(events.current(), None);

        // Over 30 simulated minutes with poster = false, nothing starts
        let steps = (30 * 60 * 1000) / 33;
        for _ in 0..steps {
            events.update(33, 255, false, false);
            assert_eq!(events.current(), None);
        }
    }
}
