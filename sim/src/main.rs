use app_sdk::{App, Input, Tilt, Touch};
use std::{
    env,
    fs::{self, File},
    io::{self, BufWriter},
    path::PathBuf,
};

struct Options {
    app: String,
    tilt: f64,
    direction: f64,
    touch: Option<(u16, u16)>,
    taps: Vec<u32>,
    keys: Vec<u32>,
    frames: u32,
    fps: u32,
    /// Local seconds since midnight at frame 0, on 2026-10-26.
    time: Option<u32>,
    out: PathBuf,
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let app = args.next().unwrap_or_default();
    if !APPS.contains(&app.as_str()) {
        return Err("usage: sim <visuals|glyphfield|blobs|flow|sand|clock|pikachu|city> [--tilt DEG] [--tilt-dir DEG] [--touch X,Y] [--tap-at FRAME]... [--key-at FRAME]... [--frames N] [--fps 30] [--time HH:MM[:SS]] [--out DIR]".into());
    }
    let mut options = Options {
        app,
        tilt: 0.,
        direction: 0.,
        touch: None,
        taps: Vec::new(),
        keys: Vec::new(),
        frames: 1,
        fps: 30,
        time: None,
        out: "./out".into(),
    };
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {flag}"))?;
        let bad = || format!("invalid value for {flag}: {value}");
        match flag.as_str() {
            "--tilt" => options.tilt = value.parse().map_err(|_| bad())?,
            "--tilt-dir" => options.direction = value.parse().map_err(|_| bad())?,
            "--touch" => {
                let (x, y) = value.split_once(',').ok_or_else(bad)?;
                let x: u16 = x.parse().map_err(|_| bad())?;
                let y: u16 = y.parse().map_err(|_| bad())?;
                if x >= 480 || y >= 480 {
                    return Err(bad());
                }
                options.touch = Some((x, y));
            }
            "--tap-at" => options.taps.push(value.parse().map_err(|_| bad())?),
            "--key-at" => options.keys.push(value.parse().map_err(|_| bad())?),
            "--frames" => options.frames = value.parse().map_err(|_| bad())?,
            "--fps" => options.fps = value.parse().map_err(|_| bad())?,
            "--time" => {
                let mut parts = value.split(':').map(|p| p.parse::<u32>());
                let (h, m) = match (parts.next(), parts.next()) {
                    (Some(Ok(h)), Some(Ok(m))) if h < 24 && m < 60 => (h, m),
                    _ => return Err(bad()),
                };
                let s = match parts.next() {
                    None => 0,
                    Some(Ok(s)) if s < 60 => s,
                    _ => return Err(bad()),
                };
                options.time = Some(h * 3600 + m * 60 + s);
            }
            "--out" => options.out = value.into(),
            _ => return Err(format!("unknown option: {flag}")),
        }
    }
    if !options.tilt.is_finite() || !options.direction.is_finite() {
        return Err("angles must be finite".into());
    }
    if options.frames == 0 || options.fps == 0 || options.fps > 1000 {
        return Err("frames must be positive; fps must be 1..=1000".into());
    }
    Ok(options)
}

const APPS: [&str; 8] = ["visuals", "glyphfield", "blobs", "flow", "sand", "clock", "pikachu", "city"];
/// 2026-10-26 00:00 as local Unix seconds.
const SIM_DAY: u32 = 1_792_972_800;

fn tilt(degrees: f64, direction: f64) -> Tilt {
    let angle = (degrees % 360.).to_radians();
    let direction = (direction % 360.).to_radians();
    Tilt {
        x_mg: (angle.sin() * direction.cos() * 1000.).round() as i16,
        y_mg: (angle.sin() * direction.sin() * 1000.).round() as i16,
        z_mg: (angle.cos() * 1000.).round() as i16,
    }
}

fn json_string(value: &str) -> String {
    let mut result = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            c if c < ' ' => result.push_str(&format!("\\u{:04x}", c as u32)),
            c => result.push(c),
        }
    }
    result.push('"');
    result
}

fn run(options: Options) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(&options.out)?;
    let mut app: Box<dyn App> = match options.app.as_str() {
        "visuals" => Box::new(visuals::Visuals::new()),
        "blobs" => Box::new(blobs::Blobs::new()),
        "flow" => Box::new(flow::Flow::new()),
        "sand" => Box::new(sand::Sand::new()),
        "clock" => Box::new(clock::Clock::new()),
        "pikachu" => Box::new(pikachu::Pikachu::new()),
        "city" => Box::new(city::City::new()),
        _ => Box::new(glyphfield::GlyphField::new()),
    };
    let dt = 1000 / options.fps;
    let mut strip = [0; 480 * 12];
    let mut rgb = vec![0; 480 * 480 * 3];
    let mut lit = 0;
    let mut sounds = 0;
    for frame in 0..options.frames {
        let (x, y) = options.touch.unwrap_or((0, 0));
        let mut output = app_sdk::Output::default();
        app.update(
            &Input {
                tilt: tilt(options.tilt, options.direction),
                touch: Touch {
                    down: options.touch.is_some(),
                    x,
                    y,
                    tapped: options.taps.contains(&frame),
                },
                time_ms: frame.wrapping_mul(dt),
                local_time_s: options
                    .time
                    .map(|t| SIM_DAY + t + frame * dt / 1000),
                key: options.keys.contains(&frame),
            },
            dt,
            &mut output,
        );
        sounds += usize::from(output.sound.is_some());
        lit = 0;
        for row in (0..480u16).step_by(12) {
            app.render(row..row + 12, &mut strip);
            let offset = row as usize * 480 * 3;
            for (pixel, output) in strip
                .iter()
                .zip(rgb[offset..offset + 480 * 12 * 3].chunks_exact_mut(3))
            {
                let (r, g, b) = app_sdk::rgb888(*pixel);
                output.copy_from_slice(&[r, g, b]);
                lit += usize::from(*pixel != 0);
            }
        }
        let file = File::create(options.out.join(format!("frame_{frame:04}.png")))?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), 480, 480);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&rgb)?;
        writer.finish()?;
    }
    println!(
        "{{\"app\":{},\"frames\":{},\"out\":{},\"lit_pixels_last\":{lit},\"sounds\":{sounds}}}",
        json_string(&options.app),
        options.frames,
        json_string(&options.out.to_string_lossy())
    );
    Ok(())
}

fn main() {
    let result = parse(env::args().skip(1))
        .map_err(|message| {
            Box::<dyn std::error::Error>::from(io::Error::new(io::ErrorKind::InvalidInput, message))
        })
        .and_then(run);
    if let Err(error) = result {
        eprintln!("sim: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gravity_direction() {
        assert_eq!(
            tilt(0., 0.),
            Tilt {
                x_mg: 0,
                y_mg: 0,
                z_mg: 1000
            }
        );
        assert_eq!(
            tilt(60., 90.),
            Tilt {
                x_mg: 0,
                y_mg: 866,
                z_mg: 500
            }
        );
    }
    #[test]
    fn cli_validation_and_repeated_taps() {
        let args = |s: &str| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        let options = parse(args("glyphfield --tap-at 0 --tap-at 2 --touch 300,200")).unwrap();
        assert_eq!(options.taps, [0, 2]);
        assert_eq!(options.touch, Some((300, 200)));
        for bad in [
            "glyphfield --fps 0",
            "glyphfield --frames 0",
            "glyphfield --tilt NaN",
            "glyphfield --touch 480,0",
            "glyphfield --wat 1",
            "glyphfield --out",
            "clock --time 24:00",
            "clock --time 12",
        ] {
            assert!(parse(args(bad)).is_err(), "{bad}");
        }
    }
    #[test]
    fn json_escaping() {
        assert_eq!(json_string("a\"\\\n"), "\"a\\\"\\\\\\u000a\"");
    }
}
