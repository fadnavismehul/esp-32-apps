//! AXP2101 power management. The LCD and touch run off its ALDO rails, so
//! this must run before the display is touched. ALDO3 doubles as the LCD
//! reset line: pulsing it resets the panel. The PWR button is also read
//! here: the chip latches a short press in an IRQ status bit.

use esp_hal::delay::Delay;

use super::Bus;

const ADDR: u8 = 0x34;
const LDO_ONOFF_CTRL0: u8 = 0x90;
const ALDO1_VOLTAGE: u8 = 0x92; // ALDO1..4 are 0x92..0x95
const ALDO_3V3: u8 = ((3300u16 - 500) / 100) as u8;
const IRQ_ENABLE2: u8 = 0x41;
const IRQ_STATUS2: u8 = 0x49;
/// PWR short press, latched on release. Write 1 to clear.
const PKEY_SHORT: u8 = 1 << 3;

const ALDO1: u8 = 1 << 0;
const ALDO2: u8 = 1 << 1;
const ALDO3: u8 = 1 << 2;
const ALDO4: u8 = 1 << 3;

pub fn init(i2c: &mut Bus) -> Result<(), esp_hal::i2c::master::Error> {
    let delay = Delay::new();
    for reg in ALDO1_VOLTAGE..ALDO1_VOLTAGE + 4 {
        i2c.write(ADDR, &[reg, ALDO_3V3])?;
    }

    let mut on = [0u8];
    i2c.write_read(ADDR, &[LDO_ONOFF_CTRL0], &mut on)?;
    let base = on[0] | ALDO1 | ALDO2 | ALDO4;

    // Reset pulse on ALDO3: on, off, on, 100 ms each.
    for v in [base | ALDO3, base & !ALDO3, base | ALDO3] {
        i2c.write(ADDR, &[LDO_ONOFF_CTRL0, v])?;
        delay.delay_millis(100);
    }

    let mut en = [0u8];
    i2c.write_read(ADDR, &[IRQ_ENABLE2], &mut en)?;
    i2c.write(ADDR, &[IRQ_ENABLE2, en[0] | PKEY_SHORT])?;
    i2c.write(ADDR, &[IRQ_STATUS2, PKEY_SHORT])?;
    Ok(())
}

/// True once per short press of the PWR button.
pub fn take_short_press(i2c: &mut Bus) -> bool {
    let mut status = [0u8];
    if i2c.write_read(ADDR, &[IRQ_STATUS2], &mut status).is_err() {
        return false;
    }
    let pressed = status[0] & PKEY_SHORT != 0;
    if pressed {
        let _ = i2c.write(ADDR, &[IRQ_STATUS2, PKEY_SHORT]);
    }
    pressed
}
