//! CST9217/CST9220 touch controller, polled (the INT line is not used).

use esp_hal::delay::Delay;
use esp_hal::gpio::Output;

use super::Bus;

const ADDR: u8 = 0x5A;
const READ: [u8; 2] = [0xD0, 0x00];
const ACK: u8 = 0xAB;

pub fn reset(rst: &mut Output<'static>) {
    let delay = Delay::new();
    rst.set_low();
    delay.delay_millis(10);
    rst.set_high();
    delay.delay_millis(50);
}

/// The first finger's position, or `None` when nothing is touching.
pub fn read(i2c: &mut Bus) -> Option<(u16, u16)> {
    let mut b = [0u8; 15];
    i2c.write_read(ADDR, &READ, &mut b).ok()?;
    i2c.write(ADDR, &[READ[0], READ[1], ACK]).ok()?;
    if b[6] != ACK || b[5] & 0x7F == 0 || b[0] & 0x0F != 0x06 {
        return None;
    }
    let x = ((b[1] as u16) << 4) | (b[3] as u16 >> 4);
    let y = ((b[2] as u16) << 4) | (b[3] as u16 & 0x0F);
    Some((x, y))
}
