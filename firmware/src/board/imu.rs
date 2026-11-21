//! QMI8658 6-axis IMU: accelerometer (±2 g) and gyroscope (±512 dps).

use esp_hal::delay::Delay;

use super::Bus;

const ADDR: u8 = 0x6B;
const WHO_AM_I: u8 = 0x00;
const CTRL1: u8 = 0x02;
const CTRL2: u8 = 0x03;
const CTRL3: u8 = 0x04;
const CTRL7: u8 = 0x08;
const AX_L: u8 = 0x35;
const RESET: u8 = 0x60;

const ACCEL_LSB_PER_G: i32 = 16384;
/// ±512 dps full-scale range = 64 LSB/dps.
const GYRO_LSB_PER_DPS: i32 = 64;

pub fn init(i2c: &mut Bus) -> Result<u8, esp_hal::i2c::master::Error> {
    i2c.write(ADDR, &[RESET, 0xB0])?;
    Delay::new().delay_millis(20);
    let mut id = [0u8];
    i2c.write_read(ADDR, &[WHO_AM_I], &mut id)?;
    i2c.write(ADDR, &[CTRL1, 0x40])?; // register address auto-increment
    i2c.write(ADDR, &[CTRL2, 0x05])?; // accel ±2 g, ~250 Hz
    i2c.write(ADDR, &[CTRL3, 0x55])?; // gyro ±512 dps, ~250 Hz
    i2c.write(ADDR, &[CTRL7, 0x03])?; // accelerometer + gyroscope on
    Ok(id[0])
}

/// Gravity in milli-g and angular rate in tenths of a degree per second
/// (deci-dps), aligned to screen axes: x toward the right edge (USB side),
/// y toward the bottom edge, z into the screen.
pub fn read_imu(
    i2c: &mut Bus,
) -> Result<((i16, i16, i16), (i16, i16, i16)), esp_hal::i2c::master::Error> {
    let mut b = [0u8; 12];
    i2c.write_read(ADDR, &[AX_L], &mut b)?;
    let mg = |lo: u8, hi: u8| {
        (i16::from_le_bytes([lo, hi]) as i32 * 1000 / ACCEL_LSB_PER_G) as i16
    };
    let dps10 = |lo: u8, hi: u8| {
        (i16::from_le_bytes([lo, hi]) as i32 * 10 / GYRO_LSB_PER_DPS) as i16
    };
    let (ax, ay, az) = (mg(b[0], b[1]), mg(b[2], b[3]), mg(b[4], b[5]));
    let (gx, gy, gz) = (dps10(b[6], b[7]), dps10(b[8], b[9]), dps10(b[10], b[11]));
    // The chip sits rotated +90° around Z on the board: screen x = chip y,
    // screen y = -chip x, screen z = chip z.
    Ok(((ay, ax.saturating_neg(), az), (gy, gx.saturating_neg(), gz)))
}
