//! Firing reminders: the minute scheduler, the alert on screen and snoozes.
//!
//! The saved list lives in `state::REMINDERS`. This module only keeps what is
//! happening now, in RAM: a snooze does not survive a reboot.

use core::cell::RefCell;

use app_sdk::Sound;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use log::{info, warn};
use netproto::reminders::{Reminder, Schedule, TEXT_MAX};

use crate::state;

const CHIME_EVERY_MS: u64 = 10_000;
/// Chime for this long after an alert appears, then stay quiet until dismissed.
const CHIME_FOR_MS: u64 = 60_000;
pub const SNOOZE_MS: u64 = 10 * 60_000;
/// Minutes to catch up on if the loop or the clock skips ahead.
const CATCH_UP: u32 = 5;
const WAITING: usize = 6;

#[derive(Clone, Copy)]
pub struct Alert {
    pub id: u8,
    /// The local minute it was due, if the clock was set.
    pub minute: Option<u32>,
    text: [u8; TEXT_MAX],
    len: u8,
}

impl Alert {
    fn new(r: &Reminder, minute: Option<u32>) -> Self {
        let mut text = [0; TEXT_MAX];
        text[..r.text().len()].copy_from_slice(r.text().as_bytes());
        Self { id: r.id, minute, text, len: r.text().len() as u8 }
    }

    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.text[..self.len as usize]).unwrap_or("")
    }
}

struct Runtime {
    last_minute: Option<u32>,
    /// The alert on screen, when it appeared, and how many chimes it has had.
    active: Option<(Alert, u64, u32)>,
    /// Alerts to show once the screen is free, each from a time (ms since boot).
    waiting: [Option<(u64, Alert)>; WAITING],
}

static RT: Mutex<Cs, RefCell<Runtime>> = Mutex::new(RefCell::new(Runtime {
    last_minute: None,
    active: None,
    waiting: [None; WAITING],
}));

fn with<R>(f: impl FnOnce(&mut Runtime) -> R) -> R {
    RT.lock(|r| f(&mut r.borrow_mut()))
}

impl Runtime {
    fn queue(&mut self, from_ms: u64, alert: Alert) {
        match self.waiting.iter_mut().find(|w| w.is_none()) {
            Some(slot) => *slot = Some((from_ms, alert)),
            None => warn!("remind: too many waiting, dropped #{}", alert.id),
        }
    }
}

/// What the main loop should do after [`tick`].
#[derive(Default)]
pub struct Tick {
    /// A one-off fired and was removed: save the list.
    pub save: bool,
    pub sound: Option<Sound>,
}

/// Call once per frame.
pub fn tick(now_ms: u64, local_s: Option<u32>) -> Tick {
    let mut out = Tick::default();
    if let Some(m) = local_s.map(|s| s / 60) {
        let last = with(|rt| core::mem::replace(&mut rt.last_minute, Some(m)));
        // Only fire when the minute moves forward; an SNTP correction can step back.
        let first = match last {
            Some(p) if m <= p => {
                with(|rt| rt.last_minute = Some(p));
                None
            }
            Some(p) if m - p <= CATCH_UP => Some(p + 1),
            _ => Some(m),
        };
        if let Some(first) = first {
            out.save = fire_due(first, m, now_ms);
        }
    }
    with(|rt| {
        if rt.active.is_none() {
            let next = rt
                .waiting
                .iter()
                .enumerate()
                .filter_map(|(i, w)| w.filter(|(from, _)| *from <= now_ms).map(|(from, _)| (from, i)))
                .min();
            if let Some((_, i)) = next {
                let (_, alert) = rt.waiting[i].take().unwrap();
                info!("remind: showing #{} '{}'", alert.id, alert.text());
                rt.active = Some((alert, now_ms, 0));
            }
        }
        if let Some((_, shown, chimes)) = &mut rt.active {
            let since = now_ms - *shown;
            if since < CHIME_FOR_MS && since / CHIME_EVERY_MS >= *chimes as u64 {
                *chimes += 1;
                out.sound = Some(Sound::Chime);
            }
        }
    });
    out
}

/// Queue everything due in minutes `first..=last`. Returns whether a one-off
/// was removed from the saved list.
fn fire_due(first: u32, last: u32, now_ms: u64) -> bool {
    let mut fired = [None; WAITING];
    let mut n = 0;
    let mut removed = false;
    state::reminders(|list| {
        for r in list.iter() {
            let minute = match r.schedule {
                Schedule::Once { at } => (at <= last).then_some(at),
                _ => (first..=last).find(|m| r.due(*m)),
            };
            if let Some(m) = minute.filter(|_| n < WAITING) {
                fired[n] = Some((Alert::new(r, Some(m)), matches!(r.schedule, Schedule::Once { .. })));
                n += 1;
            }
        }
        for (alert, once) in fired[..n].iter().flatten() {
            if *once {
                removed |= list.remove(alert.id);
            }
        }
    });
    with(|rt| {
        for (alert, _) in fired[..n].iter().flatten() {
            info!("remind: #{} due", alert.id);
            rt.queue(now_ms, *alert);
        }
    });
    removed
}

/// The alert on screen and when it appeared.
pub fn current() -> Option<(Alert, u64)> {
    with(|rt| rt.active.map(|(a, shown, _)| (a, shown)))
}

pub fn dismiss() -> bool {
    with(|rt| rt.active.take()).inspect(|(a, ..)| info!("remind: dismissed #{}", a.id)).is_some()
}

pub fn snooze(now_ms: u64) -> bool {
    with(|rt| {
        let Some((alert, ..)) = rt.active.take() else { return false };
        info!("remind: snoozed #{}", alert.id);
        rt.queue(now_ms + SNOOZE_MS, alert);
        true
    })
}

/// Show a saved reminder now, to try it out.
pub fn fire_now(id: u8, now_ms: u64) -> bool {
    let minute = state::local_time_s().map(|s| s / 60);
    let Some(alert) = state::reminders(|l| l.get(id).map(|r| Alert::new(r, minute))) else {
        return false;
    };
    with(|rt| rt.queue(now_ms, alert));
    true
}

/// Snoozed or queued alerts not yet on screen.
pub fn waiting() -> usize {
    with(|rt| rt.waiting.iter().flatten().count())
}

/// The time zone moved by `delta_min`. Move the last minute checked by the
/// same amount, so no reminder is skipped and none fires a second time.
pub fn shift_clock(delta_min: i32) {
    with(|rt| rt.last_minute = rt.last_minute.map(|m| m.saturating_add_signed(delta_min)));
}
