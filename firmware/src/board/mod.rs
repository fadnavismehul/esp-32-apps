//! Drivers for the Waveshare ESP32-C6 Touch AMOLED 2.16.
//!
//! Pin map and init quirks come from the vendor BSP, as captured in the
//! Clawdmeter firmware (boards/waveshare_amoled_216_c6). Touch, PMU and IMU
//! share one I2C bus on GPIO8 (SDA) / GPIO7 (SCL).

pub mod audio;
pub mod display;
pub mod imu;
pub mod pmu;
pub mod touch;

use esp_hal::Blocking;
use esp_hal::i2c::master::I2c;

pub type Bus = I2c<'static, Blocking>;
