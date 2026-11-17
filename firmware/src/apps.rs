//! The native apps compiled into this firmware. Each lives in its own crate
//! under `apps/`. To add one: create the crate, add it to
//! `firmware/Cargo.toml`, then add a variant, a name and a constructor here.

use core::ops::Range;

use app_sdk::{App, Input, Output};
use city::City;
use clock::Clock;
use pikachu::Pikachu;
use sand::Sand;
use visuals::Visuals;

pub enum ActiveApp {
    Visuals(Visuals),
    Sand(Sand),
    Clock(Clock),
    Pikachu(Pikachu),
    City(City),
}

/// Also the order the middle button cycles through.
pub const NAMES: [&str; 5] = ["visuals", "sand", "clock", "pikachu", "city"];

impl ActiveApp {
    pub const DEFAULT: Self = ActiveApp::Visuals(Visuals::new());

    /// An app by name; a visual's own name opens `visuals` on that one.
    pub fn by_name(name: &str) -> Option<Self> {
        Some(match name {
            "visuals" => ActiveApp::Visuals(Visuals::new()),
            "sand" => ActiveApp::Sand(Sand::new()),
            "clock" => ActiveApp::Clock(Clock::new()),
            "pikachu" => ActiveApp::Pikachu(Pikachu::new()),
            "city" => ActiveApp::City(City::new()),
            _ => ActiveApp::Visuals(Visuals::starting(name)?),
        })
    }

    /// Replace this app with the one named, in place. Callers lock the slot
    /// first and build here, so a whole app is never held across an `await`:
    /// that would reserve room for it in the main task's stack-borne state.
    pub fn switch_to(&mut self, name: &str) -> bool {
        match Self::by_name(name) {
            Some(app) => {
                *self = app;
                true
            }
            None => false,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            ActiveApp::Visuals(_) => "visuals",
            ActiveApp::Sand(_) => "sand",
            ActiveApp::Clock(_) => "clock",
            ActiveApp::Pikachu(_) => "pikachu",
            ActiveApp::City(_) => "city",
        }
    }

    /// The app after this one, wrapping around.
    pub fn next_name(&self) -> &'static str {
        let i = NAMES.iter().position(|n| *n == self.name()).unwrap_or(0);
        NAMES[(i + 1) % NAMES.len()]
    }

    /// The canonical `'static` name for a user-supplied one: an app, or one of
    /// the visuals.
    pub fn canonical(name: &str) -> Option<&'static str> {
        NAMES.iter().chain(visuals::NAMES.iter()).copied().find(|n| *n == name)
    }
}

impl App for ActiveApp {
    fn update(&mut self, input: &Input, dt_ms: u32, out: &mut Output) {
        match self {
            ActiveApp::Visuals(a) => a.update(input, dt_ms, out),
            ActiveApp::Sand(a) => a.update(input, dt_ms, out),
            ActiveApp::Clock(a) => a.update(input, dt_ms, out),
            ActiveApp::Pikachu(a) => a.update(input, dt_ms, out),
            ActiveApp::City(a) => a.update(input, dt_ms, out),
        }
    }

    fn render(&self, rows: Range<u16>, buf: &mut [u16]) {
        match self {
            ActiveApp::Visuals(a) => a.render(rows, buf),
            ActiveApp::Sand(a) => a.render(rows, buf),
            ActiveApp::Clock(a) => a.render(rows, buf),
            ActiveApp::Pikachu(a) => a.render(rows, buf),
            ActiveApp::City(a) => a.render(rows, buf),
        }
    }
}
