//! Turns a sprite sheet (`sheet.png` plus `sheet.toml`) into indexed frames
//! the pikachu app embeds. Used by `build.rs`, and compiled into the crate's tests.
//!
//! The sheet is a grid of equal frames. Each animation is one row of the
//! grid, read left to right; `sheet.toml` says which row and how many frames.

use std::format;
use std::string::String;
use std::vec::Vec;

/// Every animation the app knows, in the order of the generated `ANIMS`.
/// Only `idle` is required; a missing one plays `idle` instead.
pub const ANIMS: [&str; 6] = ["idle", "blink", "walk", "happy", "dizzy", "sleep"];
const DEFAULT_MS: [u32; 6] = [500, 150, 150, 120, 200, 800];

/// The largest frame side, and how big the creature is drawn by default.
pub const MAX_FRAME: usize = 240;
pub const TARGET_PX: usize = 240;
const MAX_PIXELS: usize = 1 << 20;

#[derive(Debug, PartialEq)]
pub struct Anim {
    pub first: usize,
    pub count: usize,
    pub ms: u32,
}

#[derive(Debug)]
pub struct Sheet {
    pub frame_w: usize,
    pub frame_h: usize,
    /// Screen pixels per art pixel.
    pub scale: usize,
    pub faces_left: bool,
    /// RGB565 colours; index 0 is transparent and its entry unused.
    pub palette: Vec<u16>,
    /// Frames back to back, each `frame_w * frame_h` palette indices.
    pub pixels: Vec<u8>,
    /// One per name in `ANIMS`.
    pub anims: Vec<Anim>,
    /// Rows from the top of a frame down to the creature's feet: one past
    /// the lowest opaque row of the first idle frame.
    pub foot: usize,
}

struct Image {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

fn decode(png_bytes: &[u8]) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(png_bytes);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| format!("sheet.png: {e}"))?;
    let mut buf = std::vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| format!("sheet.png: {e}"))?;
    let (width, height) = (info.width as usize, info.height as usize);
    let channels = info.color_type.samples();
    let mut rgba = Vec::with_capacity(width * height * 4);
    for px in buf[..width * height * channels].chunks_exact(channels) {
        rgba.extend_from_slice(&match px {
            [v] => [*v, *v, *v, 255],
            [v, a] => [*v, *v, *v, *a],
            [r, g, b] => [*r, *g, *b, 255],
            [r, g, b, a] => [*r, *g, *b, *a],
            _ => unreachable!(),
        });
    }
    Ok(Image { width, height, rgba })
}

fn int(table: &toml::Table, key: &str, what: &str) -> Result<Option<usize>, String> {
    match table.get(key) {
        None => Ok(None),
        Some(toml::Value::Integer(n)) if *n > 0 => Ok(Some(*n as usize)),
        Some(_) => Err(format!("sheet.toml: {what}{key} must be a positive integer")),
    }
}

pub fn convert(toml_text: &str, png_bytes: &[u8]) -> Result<Sheet, String> {
    let table: toml::Table = toml_text.parse().map_err(|e| format!("sheet.toml: {e}"))?;
    for key in table.keys() {
        if !["frame", "faces", "scale"].contains(&key.as_str()) && !ANIMS.contains(&key.as_str()) {
            return Err(format!(
                "sheet.toml: unknown key `{key}`; expected frame, faces, scale or one of {}",
                ANIMS.join(", ")
            ));
        }
    }
    let (frame_w, frame_h) = match table.get("frame").and_then(|v| v.as_array()).map(|a| a.as_slice()) {
        Some([toml::Value::Integer(w), toml::Value::Integer(h)]) if *w > 0 && *h > 0 => {
            (*w as usize, *h as usize)
        }
        _ => return Err("sheet.toml: `frame = [width, height]` is required".into()),
    };
    if frame_w > MAX_FRAME || frame_h > MAX_FRAME {
        return Err(format!("frames are {frame_w}x{frame_h}; at most {MAX_FRAME}x{MAX_FRAME} fits"));
    }
    let faces_left = match table.get("faces").map(|v| v.as_str()) {
        None | Some(Some("right")) => false,
        Some(Some("left")) => true,
        _ => return Err("sheet.toml: faces must be \"left\" or \"right\"".into()),
    };
    let scale = int(&table, "scale", "")?.unwrap_or((TARGET_PX / frame_w.max(frame_h)).max(1));
    if frame_w.max(frame_h) * scale > 480 {
        return Err(format!("scale {scale} makes the creature wider than the screen"));
    }

    let image = decode(png_bytes)?;
    if image.width % frame_w != 0 || image.height % frame_h != 0 {
        return Err(format!(
            "sheet.png is {}x{}, not a whole grid of {frame_w}x{frame_h} frames",
            image.width, image.height
        ));
    }
    let (cols, rows) = (image.width / frame_w, image.height / frame_h);

    let mut palette = std::vec![0u16];
    let mut pixels = Vec::new();
    let mut anims = Vec::new();
    // (row, first frame, count) already copied, so animations that share a
    // row share its frames.
    let mut copied: Vec<(usize, usize, usize)> = Vec::new();
    for (name, default_ms) in ANIMS.iter().zip(DEFAULT_MS) {
        let Some(value) = table.get(*name) else {
            if *name == "idle" {
                return Err("sheet.toml: an [idle] animation is required".into());
            }
            // Filled in from idle below.
            anims.push(Anim { first: usize::MAX, count: 0, ms: default_ms });
            continue;
        };
        let anim = value
            .as_table()
            .ok_or_else(|| format!("sheet.toml: [{name}] must be a table"))?;
        for key in anim.keys() {
            if !["row", "frames", "ms"].contains(&key.as_str()) {
                return Err(format!("sheet.toml: unknown key `{key}` in [{name}]; expected row, frames, ms"));
            }
        }
        let row = match anim.get("row") {
            Some(toml::Value::Integer(r)) if *r >= 0 => *r as usize,
            _ => return Err(format!("sheet.toml: [{name}] needs row = <0-based grid row>")),
        };
        let count = int(anim, "frames", &format!("[{name}] "))?
            .ok_or_else(|| format!("sheet.toml: [{name}] needs frames = <count>"))?;
        let ms = int(anim, "ms", &format!("[{name}] "))?.map_or(default_ms, |m| m as u32);
        if row >= rows || count > cols {
            return Err(format!(
                "[{name}] wants row {row} with {count} frames, but the sheet has {rows} rows of {cols}"
            ));
        }
        if let Some(&(_, first, _)) = copied.iter().find(|(r, _, n)| *r == row && *n >= count) {
            anims.push(Anim { first, count, ms });
            continue;
        }
        let first = pixels.len() / (frame_w * frame_h);
        copied.push((row, first, count));
        anims.push(Anim { first, count, ms });
        for f in 0..count {
            for y in 0..frame_h {
                for x in 0..frame_w {
                    let i = ((row * frame_h + y) * image.width + f * frame_w + x) * 4;
                    let [r, g, b, a] = image.rgba[i..i + 4] else { unreachable!() };
                    if a < 128 {
                        pixels.push(0);
                        continue;
                    }
                    let c = app_rgb565(r, g, b);
                    let index = match palette[1..].iter().position(|p| *p == c) {
                        Some(i) => i + 1,
                        None => {
                            palette.push(c);
                            palette.len() - 1
                        }
                    };
                    if index > 255 {
                        return Err("sheet.png has more than 255 colours".into());
                    }
                    pixels.push(index as u8);
                }
            }
        }
        if pixels.len() > MAX_PIXELS {
            return Err(format!("the animations add up to more than {MAX_PIXELS} pixels"));
        }
    }
    let idle = Anim { ms: 0, ..anims[0] };
    for anim in &mut anims[1..] {
        if anim.count == 0 {
            *anim = Anim { ms: anim.ms, ..idle };
        }
    }
    let first_idle = &pixels[..frame_w * frame_h];
    let foot = (0..frame_h)
        .rev()
        .find(|y| first_idle[y * frame_w..(y + 1) * frame_w].iter().any(|p| *p != 0))
        .ok_or("the first idle frame is empty")?
        + 1;
    Ok(Sheet { frame_w, frame_h, scale, faces_left, palette, pixels, anims, foot })
}

/// Same packing as `app_sdk::rgb565`, which build scripts cannot reach.
fn app_rgb565(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 & 0xF8) << 8) | ((g as u16 & 0xFC) << 3) | (b as u16 >> 3)
}

/// Rust source for the sheet; the pixels go in a separate file that it
/// includes from `OUT_DIR` as `pixels_file`.
pub fn to_rust(sheet: &Sheet, pixels_file: &str) -> String {
    let mut s = String::new();
    s += "// Generated by build.rs from Pikachu's sheet.toml and sheet.png.\n";
    s += &format!("pub const FRAME_W: usize = {};\n", sheet.frame_w);
    s += &format!("pub const FRAME_H: usize = {};\n", sheet.frame_h);
    s += &format!("pub const SCALE: usize = {};\n", sheet.scale);
    s += &format!("pub const FACES_LEFT: bool = {};\n", sheet.faces_left);
    s += &format!("pub const FOOT: usize = {};\n", sheet.foot);
    s += &format!("pub const PALETTE: [u16; {}] = {:?};\n", sheet.palette.len(), sheet.palette);
    s += &format!(
        "pub static PIXELS: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{pixels_file}\"));\n"
    );
    s += "pub const ANIMS: [Anim; 6] = [\n";
    for (a, name) in sheet.anims.iter().zip(ANIMS) {
        s += &format!("    Anim {{ first: {}, count: {}, ms: {} }}, // {name}\n", a.first, a.count, a.ms);
    }
    s += "];\n";
    s
}
