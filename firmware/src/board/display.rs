//! CO5300 480x480 AMOLED over QSPI.
//!
//! Wire protocol (same as Arduino_GFX's ESP32QSPI bus):
//! - register write: opcode 0x02, 24-bit address `00 <cmd> 00`, data, all
//!   on one line.
//! - pixel write: opcode 0x32, address `00 2C 00` (RAMWR) or `00 3C 00`
//!   (RAMWRC, continue), opcode and address on one line, pixels on four
//!   lines, RGB565 big-endian.
//!
//! Init runs on the blocking driver so bare commands can go out with no data.
//! Frames then go out by DMA in strips, two buffers in flight: the CPU fills
//! one while the other is on the wire. The address window is the full screen
//! and never changes, so a frame is RAMWR followed by RAMWRC strips.

use core::ops::Range;

use esp_hal::Blocking;
use esp_hal::delay::Delay;
use esp_hal::dma::DmaTxBuf;
use esp_hal::spi::master::{Address, Command, DataMode, Spi, SpiDma};

pub const WIDTH: u16 = 480;
pub const HEIGHT: u16 = 480;
pub const STRIP_ROWS: u16 = 12;
pub const STRIP_PIXELS: usize = WIDTH as usize * STRIP_ROWS as usize;
pub const STRIP_BYTES: usize = STRIP_PIXELS * 2;

const OP_REG: u16 = 0x02;
const OP_PIXELS: u16 = 0x32;
const RAMWR: u8 = 0x2C;
const RAMWRC: u8 = 0x3C;

fn addr(cmd: u8) -> Address {
    Address::_24Bit((cmd as u32) << 8, DataMode::Single)
}

fn reg(spi: &mut Spi<'static, Blocking>, cmd: u8, data: &[u8]) {
    spi.half_duplex_write(
        DataMode::Single,
        Command::_8Bit(OP_REG, DataMode::Single),
        addr(cmd),
        0,
        data,
    )
    .unwrap();
}

/// Bring the panel up. The PMU rails must already be on.
pub fn init(spi: &mut Spi<'static, Blocking>, brightness: u8) {
    let delay = Delay::new();
    reg(spi, 0x01, &[]); // SWRESET (no reset GPIO on this board)
    delay.delay_millis(200);
    reg(spi, 0x11, &[]); // SLPOUT
    delay.delay_millis(120);

    reg(spi, 0xFE, &[0x00]); // user command page
    reg(spi, 0xC4, &[0x80]); // SPI mode control
    reg(spi, 0x3A, &[0x55]); // 16 bit/pixel
    reg(spi, 0x53, &[0x20]); // brightness control on
    reg(spi, 0x63, &[0xFF]); // HBM brightness
    reg(spi, 0x29, &[]); // DISPON
    reg(spi, 0x51, &[brightness]);
    reg(spi, 0x58, &[0x00]); // contrast enhancement off
    delay.delay_millis(10);
    reg(spi, 0x20, &[]); // INVOFF

    // Panel driving voltage on manufacturer page 0x20. Without these the
    // panel stays black on this board.
    reg(spi, 0xFE, &[0x20]);
    reg(spi, 0x19, &[0x10]);
    reg(spi, 0x1C, &[0xA0]);
    reg(spi, 0xFE, &[0x00]);
    delay.delay_millis(20);

    reg(spi, 0x36, &[0x00]); // MADCTL: USB port on the side
    let last = (WIDTH - 1).to_be_bytes();
    reg(spi, 0x2A, &[0, 0, last[0], last[1]]); // CASET 0..479
    let last = (HEIGHT - 1).to_be_bytes();
    reg(spi, 0x2B, &[0, 0, last[0], last[1]]); // PASET 0..479
}

pub struct Display {
    spi: Option<SpiDma<'static, Blocking>>,
    bufs: [Option<DmaTxBuf>; 2],
    scratch: [u16; STRIP_PIXELS],
}

impl Display {
    pub fn new(spi: SpiDma<'static, Blocking>, a: DmaTxBuf, b: DmaTxBuf) -> Self {
        Self {
            spi: Some(spi),
            bufs: [Some(a), Some(b)],
            scratch: [0; STRIP_PIXELS],
        }
    }

    /// Draw one frame. `render` fills native-endian RGB565 for the given rows.
    pub fn frame(&mut self, mut render: impl FnMut(Range<u16>, &mut [u16])) {
        let mut in_flight = None;
        let mut next = 0;
        let mut y = 0;
        while y < HEIGHT {
            let rows = y..(y + STRIP_ROWS).min(HEIGHT);
            let pixels = rows.len() * WIDTH as usize;
            render(rows, &mut self.scratch[..pixels]);

            let mut buf = self.bufs[next].take().expect("strip buffer in use");
            for (dst, px) in buf.as_mut_slice().chunks_exact_mut(2).zip(&self.scratch[..pixels]) {
                dst.copy_from_slice(&px.to_be_bytes());
            }

            // Wait for the previous strip; its buffer becomes free again.
            if let Some((transfer, slot)) = in_flight.take() {
                self.finish(transfer, slot);
            }

            let cmd = if y == 0 { RAMWR } else { RAMWRC };
            let spi = self.spi.take().unwrap();
            let transfer = spi
                .half_duplex_write_buffer(
                    DataMode::Quad,
                    Command::_8Bit(OP_PIXELS, DataMode::Single),
                    addr(cmd),
                    0,
                    pixels * 2,
                    buf,
                )
                .map_err(|(e, _, _)| e)
                .expect("start strip transfer");
            in_flight = Some((transfer, next));

            next ^= 1;
            y += STRIP_ROWS;
        }
        if let Some((transfer, slot)) = in_flight {
            self.finish(transfer, slot);
        }
    }

    /// Set panel brightness (register 0x51), between frames.
    pub fn set_brightness(&mut self, level: u8) {
        let mut buf = self.bufs[0].take().expect("strip buffer in use");
        buf.as_mut_slice()[0] = level;
        let spi = self.spi.take().unwrap();
        let transfer = spi
            .half_duplex_write_buffer(
                DataMode::Single,
                Command::_8Bit(OP_REG, DataMode::Single),
                addr(0x51),
                0,
                1,
                buf,
            )
            .map_err(|(e, _, _)| e)
            .expect("start register write");
        self.finish(transfer, 0);
    }

    fn finish(
        &mut self,
        transfer: esp_hal::spi::master::SpiDmaTransfer<'static, Blocking, DmaTxBuf>,
        slot: usize,
    ) {
        let (spi, buf) = transfer.wait();
        self.spi = Some(spi);
        self.bufs[slot] = Some(buf);
    }
}
