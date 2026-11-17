//! The visuals app: glyphfield, halftone blobs and flow field in one slot.
//! A short press of KEY moves to the next one. Only the one showing keeps
//! state, so the slot costs no more than the largest of them.
#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use app_sdk::{Input, Output};

    #[test]
    fn key_cycles_through_all_three() {
        let mut app = Visuals::new();
        let mut seen = std::vec![app.name()];
        for _ in 0..3 {
            let mut out = Output::default();
            app.update(&Input { key: true, ..Input::default() }, 33, &mut out);
            assert!(out.sound.is_some());
            seen.push(app.name());
        }
        assert_eq!(seen, ["glyphfield", "blobs", "flow", "glyphfield"]);
    }

    #[test]
    fn starts_on_a_named_one() {
        assert_eq!(Visuals::starting("flow").unwrap().name(), "flow");
        assert!(Visuals::starting("sand").is_none());
    }
}

use app_sdk::{App, Input, Output, Sound};
use blobs::Blobs;
use core::ops::Range;
use flow::Flow;
use glyphfield::GlyphField;

/// In KEY order.
pub const NAMES: [&str; 3] = ["glyphfield", "blobs", "flow"];

enum Showing {
    Glyphfield(GlyphField),
    Blobs(Blobs),
    Flow(Flow),
}

pub struct Visuals {
    showing: Showing,
}

impl Visuals {
    pub const fn new() -> Self {
        Self { showing: Showing::Glyphfield(GlyphField::new()) }
    }

    /// Start on one of `NAMES`.
    pub fn starting(name: &str) -> Option<Self> {
        let showing = match name {
            "glyphfield" => Showing::Glyphfield(GlyphField::new()),
            "blobs" => Showing::Blobs(Blobs::new()),
            "flow" => Showing::Flow(Flow::new()),
            _ => return None,
        };
        Some(Self { showing })
    }

    pub fn name(&self) -> &'static str {
        match self.showing {
            Showing::Glyphfield(_) => "glyphfield",
            Showing::Blobs(_) => "blobs",
            Showing::Flow(_) => "flow",
        }
    }
}

impl Default for Visuals {
    fn default() -> Self {
        Self::new()
    }
}

impl App for Visuals {
    fn update(&mut self, input: &Input, dt_ms: u32, out: &mut Output) {
        if input.key {
            let i = NAMES.iter().position(|n| *n == self.name()).unwrap_or(0);
            *self = Self::starting(NAMES[(i + 1) % NAMES.len()]).unwrap_or_default();
            out.sound = Some(Sound::Click);
        }
        match &mut self.showing {
            Showing::Glyphfield(a) => a.update(input, dt_ms, out),
            Showing::Blobs(a) => a.update(input, dt_ms, out),
            Showing::Flow(a) => a.update(input, dt_ms, out),
        }
    }

    fn render(&self, rows: Range<u16>, buf: &mut [u16]) {
        match &self.showing {
            Showing::Glyphfield(a) => a.render(rows, buf),
            Showing::Blobs(a) => a.render(rows, buf),
            Showing::Flow(a) => a.render(rows, buf),
        }
    }
}
