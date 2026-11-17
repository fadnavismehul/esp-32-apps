#![no_std]

extern crate alloc;

pub mod alert;
pub mod apps;
pub mod board;
pub mod logger;
pub mod net;
pub mod overlay;
pub mod persist;
pub mod remind;
pub mod settings;
pub mod state;
pub mod ui;

use esp_bootloader_esp_idf::ota::OtaImageState;
use esp_bootloader_esp_idf::ota_updater::OtaUpdater;
use esp_bootloader_esp_idf::partitions::{AppPartitionSubType, FlashStorage, PARTITION_TABLE_MAX_LEN};

static mut SLOT: &str = "unknown";

/// The OTA slot this image booted from, read once at boot.
pub fn ota_slot_name() -> &'static str {
    unsafe { SLOT }
}

pub fn ota_slot_name_from(flash: &mut FlashStorage<'static>) -> &'static str {
    let mut table = [0u8; PARTITION_TABLE_MAX_LEN];
    let name = match OtaUpdater::new(flash, &mut table).and_then(|mut o| o.selected_partition()) {
        Ok(AppPartitionSubType::Ota0) => "ota_0",
        Ok(AppPartitionSubType::Ota1) => "ota_1",
        Ok(AppPartitionSubType::Factory) => "factory",
        Ok(_) => "other",
        Err(_) => "no-ota",
    };
    unsafe { SLOT = name };
    name
}

/// Unconfirmed boots of the current image, kept across resets (not power loss).
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut TRIES: u32 = 0;
const TRIES_MAGIC: u32 = 0x07A0_0000;
/// Boots an OTA image gets to confirm itself before we go back to the old one.
const MAX_TRIES: u32 = 3;

/// Whether the running image still has to confirm itself, read once at boot.
static mut PENDING: bool = false;

fn with_ota<R>(flash: &mut FlashStorage<'static>, f: impl FnOnce(&mut OtaUpdater<'_, 'static>) -> R) -> Option<R> {
    let mut table = [0u8; PARTITION_TABLE_MAX_LEN];
    OtaUpdater::new(flash, &mut table).ok().map(|mut o| f(&mut o))
}

/// Called early at boot. A fresh OTA image is on probation: count the boot, and
/// if it has failed to confirm itself too often (it keeps crashing, and the
/// watchdog keeps resetting it), go back to the previous image.
pub fn check_boot(flash: &mut FlashStorage<'static>) {
    let state = with_ota(flash, |o| o.current_ota_state().ok()).flatten();
    let pending = matches!(state, Some(OtaImageState::New | OtaImageState::PendingVerify));
    let tries = unsafe {
        if TRIES & 0xFFFF_0000 != TRIES_MAGIC || !pending {
            TRIES = TRIES_MAGIC;
        }
        if pending {
            TRIES += 1;
        }
        TRIES & 0xFFFF
    };
    log::info!("ota: state {state:?}, try {tries}");
    unsafe { PENDING = pending };
    if pending && tries > MAX_TRIES {
        roll_back(flash);
    }
}

/// Whether this image still has to call [`confirm_image`].
pub fn image_pending() -> bool {
    unsafe { PENDING }
}

/// Mark the running image bad, select the other slot and reboot into it.
pub fn roll_back(flash: &mut FlashStorage<'static>) -> ! {
    log::warn!("ota: image never confirmed, rolling back");
    with_ota(flash, |o| {
        let _ = o.set_current_ota_state(OtaImageState::Invalid);
        if o.activate_next_partition().is_ok() {
            let _ = o.set_current_ota_state(OtaImageState::Valid);
        }
    });
    unsafe { TRIES = 0 };
    esp_hal::system::software_reset()
}

/// Wi-Fi and the app are up: tell the bootloader this image is good.
pub async fn confirm_image() {
    if !image_pending() {
        return;
    }
    let mut guard = persist::FLASH.lock().await;
    let Some(flash) = guard.as_mut() else { return };
    with_ota(flash, |o| {
        if matches!(o.current_ota_state(), Ok(OtaImageState::New | OtaImageState::PendingVerify)) {
            let _ = o.set_current_ota_state(OtaImageState::Valid);
        }
    });
    unsafe {
        PENDING = false;
        TRIES = TRIES_MAGIC;
    }
    log::info!("ota: image confirmed");
}

/// Give up on an image that did not confirm in time.
pub async fn roll_back_now() -> ! {
    let mut guard = persist::FLASH.lock().await;
    match guard.as_mut() {
        Some(flash) => roll_back(flash),
        None => esp_hal::system::software_reset(),
    }
}
