//! The settings screen, opened by holding KEY (a short press closes it): Wi-Fi
//! on or off, volume and brightness. Changes apply at once and are saved when it
//! closes. Drawn instead of the app, like the reminder card.

use core::cell::RefCell;
use core::fmt::Write;
use core::ops::Range;

use app_sdk::Sound;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use log::info;

use crate::state;
use crate::ui::{MAGENTA, PEACH, Strip, mix, solid, text, text_from, width};

/// Close by itself after this long untouched.
const IDLE_MS: u64 = 30_000;
const TITLE_Y: i32 = 112;
/// Baselines of the four rows; a row takes taps 34 px either side.
const ROWS: [i32; 4] = [180, 248, 316, 384];
const LABEL_X: i32 = 56;
/// Where "-" and "+" sit, and the value between them.
const MINUS_X: i32 = 290;
const PLUS_X: i32 = 420;
const VALUE_MID: i32 = 362;

struct Screen {
    open: bool,
    touched_ms: u64,
    changed: bool,
}

static SCREEN: Mutex<Cs, RefCell<Screen>> =
    Mutex::new(RefCell::new(Screen { open: false, touched_ms: 0, changed: false }));

fn with<R>(f: impl FnOnce(&mut Screen) -> R) -> R {
    SCREEN.lock(|s| f(&mut s.borrow_mut()))
}

/// What the main loop should do after a settings event.
#[derive(Default)]
pub struct Effect {
    /// Write settings to flash.
    pub save: bool,
    pub brightness: Option<u8>,
    pub volume: Option<u8>,
    pub sound: Option<Sound>,
}

pub fn is_open() -> bool {
    with(|s| s.open)
}

/// KEY held: open, or close and save.
pub fn toggle(now_ms: u64) -> Effect {
    with(|s| {
        s.open = !s.open;
        s.touched_ms = now_ms;
        info!("settings: {}", if s.open { "open" } else { "closed" });
        let save = !s.open && core::mem::take(&mut s.changed);
        Effect { save, sound: Some(Sound::Click), ..Effect::default() }
    })
}

/// Close if open (PWR, a reminder, or idle).
pub fn close() -> Effect {
    with(|s| {
        let was = core::mem::replace(&mut s.open, false);
        Effect { save: was && core::mem::take(&mut s.changed), ..Effect::default() }
    })
}

/// Call each frame while open.
pub fn tick(now_ms: u64) -> Effect {
    if with(|s| s.open && now_ms - s.touched_ms > IDLE_MS) {
        info!("settings: closed after idle");
        return close();
    }
    Effect::default()
}

fn brightness_pct() -> u8 {
    let raw = state::config(|c| c.brightness()) as u32;
    ((raw * 100 + 127) / 255).div_ceil(10).max(1) as u8 * 10
}

/// A tap at (x, y) while open.
pub fn tap(x: u16, y: u16, now_ms: u64) -> Effect {
    let (x, y) = (x as i32, y as i32);
    let Some(row) = ROWS.iter().position(|r| (y - (r - 14)).abs() <= 34) else {
        return Effect::default();
    };
    with(|s| {
        s.touched_ms = now_ms;
        s.changed = true;
    });
    // Right half of a number row is "+", the rest of the value area "-".
    let up = x >= VALUE_MID;
    match row {
        0 => {
            let off = state::config(|c| {
                c.set_wifi_off(!c.wifi_off());
                c.wifi_off()
            });
            state::WIFI_CHANGED.signal(());
            info!("settings: wifi {}", if off { "off" } else { "on" });
            Effect { sound: Some(Sound::Click), ..Effect::default() }
        }
        1 if x >= MINUS_X - 30 => {
            let v = state::config(|c| {
                let v = if up { c.volume().saturating_add(10).min(100) } else { c.volume().saturating_sub(10) };
                c.set_volume(v);
                v
            });
            Effect { volume: Some(v), sound: Some(Sound::Tick { pitch: 128 }), ..Effect::default() }
        }
        2 if x >= MINUS_X - 30 => {
            let pct = if up { (brightness_pct() + 10).min(100) } else { brightness_pct().saturating_sub(10).max(10) };
            let raw = (pct as u32 * 255 / 100) as u8;
            state::config(|c| c.set_brightness(raw));
            Effect { brightness: Some(raw), sound: Some(Sound::Tick { pitch: 180 }), ..Effect::default() }
        }
        3 => {
            let current = state::utc_offset_min();
            let new_min = if current == netproto::tz::DEFAULT_MIN {
                netproto::tz::INDIA_MIN
            } else {
                netproto::tz::DEFAULT_MIN
            };
            state::set_utc_offset_min(new_min);
            info!("settings: tz {}", new_min);
            Effect { sound: Some(Sound::Click), ..Effect::default() }
        }
        _ => Effect::default(),
    }
}

pub fn render(rows: Range<u16>, buf: &mut [u16]) {
    buf.fill(0);
    let strip: &mut Strip<'_> = &mut (rows, buf);
    let label = solid(PEACH);
    let dim = mix(PEACH, (0, 0, 0), 150);
    text(strip, "Settings", 2, TITLE_Y, mix(MAGENTA, (0, 0, 0), 40));

    let (off, volume) = state::config(|c| (c.wifi_off(), c.volume()));
    text_from(strip, "Wi-Fi", 2, LABEL_X, ROWS[0], label);
    let (word, color) = if off { ("Off", dim) } else { ("On", solid(MAGENTA)) };
    text_from(strip, word, 2, VALUE_MID - width(word, 2) / 2, ROWS[0], color);

    let mut value = heapless_str();
    let _ = write!(value, "{volume}");
    stepper(strip, "Volume", value.as_str(), ROWS[1], label);
    let mut value = heapless_str();
    let _ = write!(value, "{}%", brightness_pct());
    stepper(strip, "Brightness", value.as_str(), ROWS[2], label);

    let offset = state::utc_offset_min();
    let mut tz_val = heapless_str();
    match offset {
        netproto::tz::DEFAULT_MIN => {
            let _ = tz_val.write_str("SGT +8:00");
        }
        netproto::tz::INDIA_MIN => {
            let _ = tz_val.write_str("IST +5:30");
        }
        other => {
            let _ = netproto::tz::format(other, &mut tz_val);
        }
    }
    text_from(strip, "Time zone", 2, LABEL_X, ROWS[3], label);
    text_from(
        strip,
        tz_val.as_str(),
        2,
        VALUE_MID - width(tz_val.as_str(), 2) / 2,
        ROWS[3],
        solid(MAGENTA),
    );
}

fn stepper(strip: &mut Strip<'_>, name: &str, value: &str, y: i32, label: embedded_graphics::pixelcolor::Rgb565) {
    let accent = solid(MAGENTA);
    text_from(strip, name, 2, LABEL_X, y, label);
    text_from(strip, "-", 2, MINUS_X, y, accent);
    text_from(strip, value, 2, VALUE_MID - width(value, 2) / 2, y, label);
    text_from(strip, "+", 2, PLUS_X, y, accent);
}

/// A few bytes of text without allocating.
fn heapless_str() -> Small {
    Small { buf: [0; 16], len: 0 }
}

struct Small {
    buf: [u8; 16],
    len: usize,
}

impl Small {
    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }
}

impl Write for Small {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len + s.len();
        self.buf.get_mut(self.len..end).ok_or(core::fmt::Error)?.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}
