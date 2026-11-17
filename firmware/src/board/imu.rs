//! QMI8658 accelerometer. Only the accelerometer is used, at ±2 g.

use esp_hal::delay::Delay;

use super::Bus;

const ADDR: u8 = 0x6B;
const WHO_AM_I: u8 = 0x00;
const CTRL1: u8 = 0x02;
const CTRL2: u8 = 0x03;
const CTRL7: u8 = 0x08;
const AX_L: u8 = 0x35;
const RESET: u8 = 0x60;

const LSB_PER_G: i32 = 16384;

pub fn init(i2c: &mut Bus) -> Result<u8, esp_hal::i2c::master::Error> {
    i2c.write(ADDR, &[RESET, 0xB0])?;
    Delay::new().delay_millis(20);
    let mut id = [0u8];
    i2c.write_read(ADDR, &[WHO_AM_I], &mut id)?;
    i2c.write(ADDR, &[CTRL1, 0x40])?; // register address auto-increment
    i2c.write(ADDR, &[CTRL2, 0x05])?; // ±2 g, ~250 Hz
    i2c.write(ADDR, &[CTRL7, 0x01])?; // accelerometer on
    Ok(id[0])
}

/// Gravity in milli-g along the screen axes: x toward the right edge (the
/// USB side), y toward the bottom edge. The buttons are on the left edge.
pub fn read_mg(i2c: &mut Bus) -> Result<(i16, i16, i16), esp_hal::i2c::master::Error> {
    let mut b = [0u8; 6];
    i2c.write_read(ADDR, &[AX_L], &mut b)?;
    let mg = |lo: u8, hi: u8| (i16::from_le_bytes([lo, hi]) as i32 * 1000 / LSB_PER_G) as i16;
    let (x, y, z) = (mg(b[0], b[1]), mg(b[2], b[3]), mg(b[4], b[5]));
    // The chip sits rotated on the board. Measured with the clock app: screen
    // upright reads chip x = -960; button (left) side down reads chip y = -1077.
    Ok((y, x.saturating_neg(), z))
}
