//! State shared between the render loop and the network tasks.
//!
//! Everything here is single-core and short-lived under the lock. The app
//! itself sits behind an async mutex so the HTTP task can re-render strips
//! for a screenshot while the render loop waits.

use core::cell::RefCell;

use app_sdk::{Sound, Tilt};
use embassy_sync::blocking_mutex::Mutex as BlockingMutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use netproto::config::Config;
use netproto::reminders::Reminders;

use crate::apps::ActiveApp;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Connecting,
    Station,
    Setup,
    /// Wi-Fi switched off in settings: no scanning or joining.
    Off,
}

/// Sensor overrides from `POST /input`.
#[derive(Clone, Copy, Default)]
pub struct Override {
    pub until_ms: u64,
    pub tilt: Option<Tilt>,
    pub touch: Option<(u16, u16)>,
    pub tap: bool,
}

pub struct Shared {
    pub mode: Mode,
    pub ssid: [u8; 32],
    pub ssid_len: usize,
    pub ip: Option<[u8; 4]>,
    pub rssi: Option<i32>,
    pub fps: u32,
    pub ovr: Override,
    /// An OTA upload is in progress: the render loop backs off.
    pub busy: bool,
    /// Show the IP on screen until this time (ms since boot).
    pub show_ip_until_ms: u64,
    /// Unix seconds from the last SNTP reply, and `now_ms()` when it came.
    pub synced: Option<(u32, u64)>,
    pub utc_offset_min: i16,
}

impl Shared {
    pub fn ssid(&self) -> Option<&str> {
        match self.ssid_len {
            0 => None,
            n => core::str::from_utf8(&self.ssid[..n]).ok(),
        }
    }

    pub fn set_ssid(&mut self, ssid: &str) {
        let n = ssid.len().min(32);
        self.ssid[..n].copy_from_slice(&ssid.as_bytes()[..n]);
        self.ssid_len = n;
    }
}

pub static SHARED: BlockingMutex<Cs, RefCell<Shared>> = BlockingMutex::new(RefCell::new(Shared {
    mode: Mode::Connecting,
    ssid: [0; 32],
    ssid_len: 0,
    ip: None,
    rssi: None,
    fps: 0,
    ovr: Override { until_ms: 0, tilt: None, touch: None, tap: false },
    busy: false,
    show_ip_until_ms: 0,
    synced: None,
    utc_offset_min: netproto::tz::DEFAULT_MIN,
}));

pub fn utc_offset_min() -> i16 {
    with(|s| s.utc_offset_min)
}

pub fn set_utc_offset_min(min: i16) {
    let min = netproto::tz::clamp(min as i32);
    let old = with(|s| core::mem::replace(&mut s.utc_offset_min, min));
    if old != min {
        crate::remind::shift_clock(min as i32 - old as i32);
    }
}

/// Local wall-clock time as Unix seconds plus the zone offset, once synced.
pub fn local_time_s() -> Option<u32> {
    let (unix, at_ms) = with(|s| s.synced)?;
    let elapsed = ((now_ms() - at_ms) / 1000) as i64;
    let offset_s = (utc_offset_min() as i64) * 60;
    let total = (unix as i64) + elapsed + offset_s;
    Some(total.clamp(0, u32::MAX as i64) as u32)
}

pub fn with<R>(f: impl FnOnce(&mut Shared) -> R) -> R {
    SHARED.lock(|s| f(&mut s.borrow_mut()))
}

pub static CONFIG: BlockingMutex<Cs, RefCell<Config>> = BlockingMutex::new(RefCell::new(Config::new()));

pub fn config<R>(f: impl FnOnce(&mut Config) -> R) -> R {
    CONFIG.lock(|c| f(&mut c.borrow_mut()))
}

pub static REMINDERS: BlockingMutex<Cs, RefCell<Reminders>> = BlockingMutex::new(RefCell::new(Reminders::new()));

pub fn reminders<R>(f: impl FnOnce(&mut Reminders) -> R) -> R {
    REMINDERS.lock(|r| f(&mut r.borrow_mut()))
}

pub static APP: Mutex<Cs, ActiveApp> = Mutex::new(ActiveApp::DEFAULT);

/// Sounds to play, from the app or the API.
pub static SOUND: Signal<Cs, Sound> = Signal::new();
/// A new speaker volume to apply (the render loop owns the I2C bus).
pub static VOLUME: Signal<Cs, u8> = Signal::new();
/// Wi-Fi was switched on or off; wakes the station task.
pub static WIFI_CHANGED: Signal<Cs, ()> = Signal::new();
/// A KEY press from the API, as if the button were pressed: `true` for a
/// long press (settings), `false` for a short one (the app).
pub static KEY: Signal<Cs, bool> = Signal::new();
/// A requested app switch, applied by the render loop between frames.
pub static SWITCH_APP: Signal<Cs, &'static str> = Signal::new();

pub fn now_ms() -> u64 {
    embassy_time::Instant::now().as_millis()
}
