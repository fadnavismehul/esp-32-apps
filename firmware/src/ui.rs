//! Drawing helpers shared by the base firmware's own screens (the reminder
//! card and settings): fast scaled text and the clock's colours.

use core::cell::RefCell;
use core::ops::Range;

use app_sdk::rgb565;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex as Cs;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::{BinaryColor, Rgb565};
use embedded_graphics::prelude::*;
use embedded_graphics::text::{Baseline, Text};

pub const CX: i32 = 240;
/// The clock app's touch colours.
pub const PEACH: (i32, i32, i32) = (250, 170, 150);
pub const MAGENTA: (i32, i32, i32) = (240, 90, 170);

/// RGB565 for `c`.
pub fn solid(c: (i32, i32, i32)) -> Rgb565 {
    mix(c, c, 0)
}

/// RGB565 for `a` blended toward `b` by `t`/256.
pub fn mix(a: (i32, i32, i32), b: (i32, i32, i32), t: i32) -> Rgb565 {
    let c = |x: i32, y: i32| (x + (y - x) * t / 256) as u8;
    Rgb565::from(embedded_graphics::pixelcolor::raw::RawU16::new(rgb565(c(a.0, b.0), c(a.1, b.1), c(a.2, b.2))))
}

pub fn isqrt(n: i32) -> i32 {
    if n <= 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

/// FONT_10X20 as bits: 95 printable ASCII glyphs, 20 rows of 10 bits, the
/// leftmost pixel in bit 9. Captured once, because drawing through
/// `embedded-graphics` touches every glyph pixel for every strip.
type Glyphs = [[u16; GH]; 95];
const GW: i32 = 10;
const GH: usize = 20;
static GLYPHS: Mutex<Cs, RefCell<Option<Glyphs>>> = Mutex::new(RefCell::new(None));

struct Capture<'a>(&'a mut [u16; GH]);

impl OriginDimensions for Capture<'_> {
    fn size(&self) -> Size {
        Size::new(GW as u32, GH as u32)
    }
}

impl DrawTarget for Capture<'_> {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;

    fn draw_iter<I: IntoIterator<Item = Pixel<BinaryColor>>>(&mut self, pixels: I) -> Result<(), Self::Error> {
        for Pixel(p, c) in pixels {
            if c.is_on() && (0..GW).contains(&p.x) && (0..GH as i32).contains(&p.y) {
                self.0[p.y as usize] |= 1 << (GW - 1 - p.x);
            }
        }
        Ok(())
    }
}

fn capture() -> Glyphs {
    let mut glyphs = [[0u16; GH]; 95];
    let style = MonoTextStyle::new(&FONT_10X20, BinaryColor::On);
    for (i, g) in glyphs.iter_mut().enumerate() {
        let mut ch = [0u8; 4];
        let s = char::from(0x20 + i as u8).encode_utf8(&mut ch);
        let _ = Text::with_baseline(s, Point::zero(), style, Baseline::Top).draw(&mut Capture(g));
    }
    glyphs
}

/// A strip of screen rows and its pixels.
pub type Strip<'a> = (Range<u16>, &'a mut [u16]);

/// Draw `s` (ASCII) centred on x 240 with its baseline at `baseline`, each
/// font pixel `scale` screen pixels square. Only rows in the strip are touched.
pub fn text(strip: &mut Strip<'_>, s: &str, scale: i32, baseline: i32, color: Rgb565) {
    text_from(strip, s, scale, CX - width(s, scale) / 2, baseline, color)
}

/// Width of `s` in pixels at `scale`.
pub fn width(s: &str, scale: i32) -> i32 {
    s.len() as i32 * GW * scale
}

/// Like [`text`], but starting at x `left`.
pub fn text_from(strip: &mut Strip<'_>, s: &str, scale: i32, left: i32, baseline: i32, color: Rgb565) {
    let (rows, buf) = strip;
    let top = baseline - FONT_10X20.baseline as i32 * scale;
    let (y0, y1) = ((rows.start as i32).max(top), (rows.end as i32).min(top + GH as i32 * scale));
    if y0 >= y1 {
        return;
    }
    let color = color.into_storage();
    GLYPHS.lock(|g| {
        let mut g = g.borrow_mut();
        let glyphs = g.get_or_insert_with(capture);
        for y in y0..y1 {
            let r = ((y - top) / scale) as usize;
            let row = &mut buf[(y - rows.start as i32) as usize * 480..][..480];
            for (i, b) in s.bytes().enumerate() {
                let bits = glyphs[b.wrapping_sub(0x20).min(94) as usize][r];
                if bits == 0 {
                    continue;
                }
                let x0 = left + i as i32 * GW * scale;
                for bit in 0..GW {
                    if bits & (1 << (GW - 1 - bit)) != 0 {
                        let x = x0 + bit * scale;
                        if x >= 0 && x + scale <= 480 {
                            row[x as usize..(x + scale) as usize].fill(color);
                        }
                    }
                }
            }
        }
    });
}

