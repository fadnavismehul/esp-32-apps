//! Status text drawn over the app, strip by strip: setup instructions in
//! setup mode, and the IP for a few seconds after connecting.

use core::ops::Range;

use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::ascii::FONT_10X20;
use embedded_graphics::pixelcolor::Rgb565;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle};
use embedded_graphics::text::{Alignment, Text};

const WIDTH: i32 = 480;
const LINE: i32 = 26;

struct Strip<'a> {
    rows: Range<u16>,
    buf: &'a mut [u16],
}

impl OriginDimensions for Strip<'_> {
    fn size(&self) -> Size {
        Size::new(480, 480)
    }
}

impl DrawTarget for Strip<'_> {
    type Color = Rgb565;
    type Error = core::convert::Infallible;

    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, pixels: I) -> Result<(), Self::Error> {
        let (y0, y1) = (self.rows.start as i32, self.rows.end as i32);
        for Pixel(p, c) in pixels {
            if p.y >= y0 && p.y < y1 && p.x >= 0 && p.x < WIDTH {
                self.buf[((p.y - y0) * WIDTH + p.x) as usize] = c.into_storage();
            }
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Rgb565) -> Result<(), Self::Error> {
        let (y0, y1) = (self.rows.start as i32, self.rows.end as i32);
        let top = area.top_left.y.max(y0);
        let bottom = (area.top_left.y + area.size.height as i32).min(y1);
        let left = area.top_left.x.max(0);
        let right = (area.top_left.x + area.size.width as i32).min(WIDTH);
        for y in top..bottom {
            let row = ((y - y0) * WIDTH) as usize;
            self.buf[row + left as usize..row + right as usize].fill(color.into_storage());
        }
        Ok(())
    }
}

/// Draw `lines` centred on a dark panel. Cheap when the strip misses it.
pub fn draw(rows: Range<u16>, buf: &mut [u16], lines: &[&str]) {
    if lines.is_empty() {
        return;
    }
    let height = lines.len() as i32 * LINE + 24;
    let top = 240 - height / 2;
    if rows.end as i32 <= top || rows.start as i32 >= top + height {
        return;
    }
    let mut strip = Strip { rows, buf };
    let _ = Rectangle::new(Point::new(30, top), Size::new(420, height as u32))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::new(2, 4, 3)))
        .draw(&mut strip);
    let style = MonoTextStyle::new(&FONT_10X20, Rgb565::new(28, 58, 28));
    for (i, line) in lines.iter().enumerate() {
        let y = top + 12 + LINE * i as i32 + 18;
        // FONT_10X20 spans baseline-16 .. baseline+4; skip lines this strip misses.
        if y + 4 <= strip.rows.start as i32 || y - 16 >= strip.rows.end as i32 {
            continue;
        }
        let _ = Text::with_alignment(line, Point::new(240, y), style, Alignment::Center).draw(&mut strip);
    }
}
