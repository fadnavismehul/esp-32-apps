//! Settings and reminders in the 4 KB `cfg` partition, plus shared flash
//! access for OTA. Both records share the one sector, so they are written together.

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use embassy_sync::mutex::Mutex;
use esp_bootloader_esp_idf::partitions::{self, FlashStorage, PARTITION_TABLE_MAX_LEN};
use log::warn;
use netproto::config::{Config, ENCODED_LEN};
use netproto::reminders::{self, Reminders};
use netproto::tz;

use crate::state;

pub static FLASH: Mutex<Cs, Option<FlashStorage<'static>>> = Mutex::new(None);

const LABEL: &str = "cfg";
/// Where the reminders record starts in the sector.
const REMINDERS_AT: u32 = 512;

fn with_cfg_region<R>(
    flash: &mut FlashStorage<'static>,
    f: impl FnOnce(&mut partitions::FlashRegion<'_, 'static>) -> R,
) -> Option<R> {
    let mut table = [0u8; PARTITION_TABLE_MAX_LEN];
    let pt = partitions::read_partition_table(flash, &mut table).ok()?;
    let entry = pt.iter().find(|p| p.label_as_str() == LABEL)?;
    let mut region = entry.as_flash_region(flash);
    Some(f(&mut region))
}

/// Load settings, reminders and time zone at boot. Missing or corrupt records give defaults.
pub fn load(flash: &mut FlashStorage<'static>) -> (Config, Reminders, i16) {
    let mut buf = [0u8; ENCODED_LEN];
    let mut rem = [0u8; reminders::ENCODED_LEN];
    let mut tz_buf = [0u8; tz::ENCODED_LEN];
    let read = with_cfg_region(flash, |r| {
        r.read(0, &mut buf).is_ok()
            && r.read(REMINDERS_AT, &mut rem).is_ok()
            && r.read(tz::AT as u32, &mut tz_buf).is_ok()
    })
    .unwrap_or(false);
    if !read {
        warn!("persist: no cfg partition, settings will not be saved");
    }
    (
        Config::decode(&buf).unwrap_or_else(Config::new),
        Reminders::decode(&rem).unwrap_or_default(),
        tz::decode(&tz_buf),
    )
}

/// Write the current settings to flash.
pub async fn save() {
    let mut buf = [0u8; ENCODED_LEN];
    state::config(|c| c.encode(&mut buf));
    let mut rem = [0u8; reminders::ENCODED_LEN];
    state::reminders(|r| r.encode(&mut rem));
    let mut tz_buf = [0u8; tz::ENCODED_LEN];
    tz::encode(state::utc_offset_min(), &mut tz_buf);
    let mut guard = FLASH.lock().await;
    let Some(flash) = guard.as_mut() else { return };
    let ok = with_cfg_region(flash, |r| {
        r.erase(0, FlashStorage::SECTOR_SIZE).is_ok()
            && r.write(0, &buf).is_ok()
            && r.write(tz::AT as u32, &tz_buf).is_ok()
            && r.write(REMINDERS_AT, &rem).is_ok()
    });
    if ok != Some(true) {
        warn!("persist: saving settings failed");
    }
}
