//! The contract between the base firmware and apps.
//!
//! Apps are pure logic: they receive input, update their state, and fill
//! strips of pixels on request. They never touch hardware, block, or
//! allocate in `render`. The same app runs on the device and in `sim`.
#![no_std]

use core::ops::Range;

pub const WIDTH: u16 = 480;
pub const HEIGHT: u16 = 480;

/// Which way gravity pulls, in milli-g along the screen axes: `x_mg` toward
/// the right edge, `y_mg` toward the bottom edge. Flat on a table is roughly
/// (0, 0, ±1000); standing upright (buttons on the left) is (0, 1000, 0).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tilt {
    pub x_mg: i16,
    pub y_mg: i16,
    pub z_mg: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Touch {
    /// A finger is on the screen right now.
    pub down: bool,
    pub x: u16,
    pub y: u16,
    /// A tap (press and release) completed since the previous update.
    pub tapped: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub tilt: Tilt,
    pub touch: Touch,
    /// Milliseconds since boot.
    pub time_ms: u32,
    /// Local wall-clock time as Unix seconds shifted by the time zone, or
    /// `None` until the device has synced over the network.
    pub local_time_s: Option<u32>,
    /// The KEY button was pressed briefly since the previous update. Apps
    /// may use it as a modifier; holding KEY opens settings instead.
    pub key: bool,
}

/// A short sound the base plays on the speaker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sound {
    /// A soft detent click. `pitch` 0 is low, 255 is high.
    Tick { pitch: u8 },
    /// A louder click for a confirmed action such as a tap.
    Click,
    /// A soft rising four-note arpeggio, about two seconds long. The base
    /// uses it for reminders.
    Chime,
}

/// What an app asks the base to do this frame, besides drawing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Output {
    /// Played once. If an app sets several in one frame, the last one wins.
    pub sound: Option<Sound>,
}

pub trait App {
    /// Advance state. Called once per frame before rendering. `out` starts
    /// empty each frame.
    fn update(&mut self, input: &Input, dt_ms: u32, out: &mut Output);

    /// Fill `buf` with RGB565 pixels (native-endian u16) for screen rows
    /// `rows`, row-major, `WIDTH` pixels per row. `buf.len()` is
    /// `rows.len() * WIDTH`. Must depend only on app state, so the base can
    /// re-render a frame to take a screenshot.
    fn render(&self, rows: Range<u16>, buf: &mut [u16]);
}

/// Pack 8-bit channels into RGB565.
pub const fn rgb565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0xF8) << 8) | ((g as u16 & 0xFC) << 3) | (b as u16 >> 3)
}

/// Unpack RGB565 into 8-bit channels.
pub const fn rgb888(c: u16) -> (u8, u8, u8) {
    let r = ((c >> 11) & 0x1F) as u8;
    let g = ((c >> 5) & 0x3F) as u8;
    let b = (c & 0x1F) as u8;
    ((r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2))
}
