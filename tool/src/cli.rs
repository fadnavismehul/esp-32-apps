#[cfg(test)]
mod tests {
    use super::*;
    fn parse(s: &str) -> Result<Options, String> {
        Options::parse(s.split_whitespace().map(str::to_owned).collect())
    }
    #[test]
    fn all_commands() {
        for (s, expected) in [
            (
                "status",
                Command::Request {
                    method: "GET",
                    path: "/status".into(),
                },
            ),
            (
                "wifi list",
                Command::Request {
                    method: "GET",
                    path: "/wifi".into(),
                },
            ),
            (
                "app hello",
                Command::Request {
                    method: "POST",
                    path: "/app?name=hello".into(),
                },
            ),
            (
                "volume 50",
                Command::Request {
                    method: "POST",
                    path: "/volume?level=50".into(),
                },
            ),
            (
                "sound tick",
                Command::Request {
                    method: "POST",
                    path: "/sound?kind=tick".into(),
                },
            ),
            (
                "sound click",
                Command::Request {
                    method: "POST",
                    path: "/sound?kind=click".into(),
                },
            ),
            (
                "key",
                Command::Request {
                    method: "POST",
                    path: "/key".into(),
                },
            ),
            (
                "key --long",
                Command::Request {
                    method: "POST",
                    path: "/key?long=1".into(),
                },
            ),
            (
                "reboot",
                Command::Request {
                    method: "POST",
                    path: "/reboot".into(),
                },
            ),
            (
                "setup",
                Command::Request {
                    method: "POST",
                    path: "/setup".into(),
                },
            ),
            (
                "wifi add a&b --pass x+y",
                Command::Request {
                    method: "POST",
                    path: "/wifi?ssid=a%26b&pass=x%2By".into(),
                },
            ),
            (
                "wifi forget a&b",
                Command::Request {
                    method: "POST",
                    path: "/wifi/forget?ssid=a%26b".into(),
                },
            ),
            (
                "wifi add home --pass-stdin",
                Command::WifiStdin {
                    ssid: "home".into(),
                },
            ),
            ("logs", Command::Logs { follow: false }),
            ("logs --follow", Command::Logs { follow: true }),
            ("screenshot out.png", Command::Screenshot("out.png".into())),
            ("build", Command::Build),
            (
                "flash --usb --port /dev/cu.test",
                Command::Flash {
                    ota: false,
                    port: Some("/dev/cu.test".into()),
                },
            ),
            (
                "flash --ota",
                Command::Flash {
                    ota: true,
                    port: None,
                },
            ),
            (
                "pair --port tty",
                Command::Pair {
                    port: Some("tty".into()),
                },
            ),
            (
                "sim sand --frames 3",
                Command::Sim(vec!["sand".into(), "--frames".into(), "3".into()]),
            ),
            (
                "remind add 18:30 Take meds --days weekdays",
                Command::Request {
                    method: "POST",
                    path: "/reminders?time=18%3A30&days=weekdays&text=Take%20meds".into(),
                },
            ),
            (
                "remind add 09:00 Stand up --every 90m --until 17:00",
                Command::Request {
                    method: "POST",
                    path: "/reminders?time=09%3A00&every=90&until=17%3A00&text=Stand%20up".into(),
                },
            ),
            (
                "remind add 7:05 Call home --date 2026-11-01",
                Command::Request {
                    method: "POST",
                    path: "/reminders?time=7%3A05&date=2026-11-01&text=Call%20home".into(),
                },
            ),
            (
                "remind list",
                Command::Request {
                    method: "GET",
                    path: "/reminders".into(),
                },
            ),
            (
                "remind delete 3",
                Command::Request {
                    method: "POST",
                    path: "/reminders/delete?id=3".into(),
                },
            ),
            ("timezone", Command::Timezone(None)),
            ("timezone sgt", Command::Timezone(Some("sgt".into()))),
            ("timezone +5:30", Command::Timezone(Some("+5:30".into()))),
            ("help", Command::Help),
            ("", Command::Help),
        ] {
            assert_eq!(parse(s).unwrap().command, expected, "{s}");
        }
    }
    #[test]
    fn input_conversion_and_validation() {
        assert_eq!(
            parse("input --tilt-deg 60 --tilt-dir 90 --touch 20,30 --tap --hold-ms 500")
                .unwrap()
                .command,
            Command::Request {
                method: "POST",
                path: "/input?tilt=0%2C866%2C500&touch=20%2C30&tap=1&hold_ms=500".into()
            }
        );
        assert_eq!(
            parse("input --tilt -10,20,1000").unwrap().command,
            Command::Request {
                method: "POST",
                path: "/input?tilt=-10%2C20%2C1000".into()
            }
        );
        for s in [
            "volume 101",
            "volume -1",
            "sound beep",
            "status junk",
            "logs --oops",
            "flash",
            "flash --usb --ota",
            "flash --ota --port tty",
            "wifi add",
            "wifi add a --pass x --pass-stdin",
            "input --tilt 1,2",
            "input --touch 480,0",
            "input --hold-ms 60001",
            "input --tilt-deg NaN",
            "input --tilt 0,0,1 --tilt-deg 2",
            "input --tilt-dir 3",
            "pair --port",
            "remind add 25:00 x",
            "remind add 10:00",
            "remind add 10:00 x --every 0m",
            "remind add 10:00 x --every 25h",
            "remind add 10:00 x --until 12:00",
            "remind add 10:00 x --date 2026-01-01 --days daily",
            "remind add 10:00 caf\u{e9}",
            "remind delete x",
            "remind wat",
            "timezone 25:00",
            "timezone bad",
            "sim unknown",
            "wat",
        ] {
            assert!(parse(s).is_err(), "{s}");
        }
    }
    #[test]
    fn global_flags_and_sim_passthrough() {
        let opts = parse("--host 127.0.0.1:8080 status --json").unwrap();
        assert_eq!(opts.host.as_deref(), Some("127.0.0.1:8080"));
        assert!(opts.json);
        assert_eq!(
            parse("sim sand --out --host").unwrap().command,
            Command::Sim(vec!["sand".into(), "--out".into(), "--host".into()])
        );
    }
    #[test]
    fn global_flag_looking_password_is_a_value() {
        assert_eq!(
            parse("wifi add home --pass --json").unwrap().command,
            Command::Request {
                method: "POST",
                path: "/wifi?ssid=home&pass=--json".into()
            }
        );
    }
    #[test]
    fn query_encoding() {
        assert_eq!(encode("a b&=+/?é"), "a%20b%26%3D%2B%2F%3F%C3%A9");
    }
}

use std::collections::VecDeque;
#[derive(Debug, PartialEq)]
pub enum Command {
    Help,
    Request { method: &'static str, path: String },
    Logs { follow: bool },
    Screenshot(String),
    WifiStdin { ssid: String },
    WifiSync { port: Option<String> },
    Build,
    Flash { ota: bool, port: Option<String> },
    Pair { port: Option<String> },
    Sim(Vec<String>),
    Timezone(Option<String>),
}
#[derive(Debug)]
pub struct Options {
    pub host: Option<String>,
    pub json: bool,
    pub command: Command,
}
fn value(args: &mut VecDeque<String>, flag: &str) -> Result<String, String> {
    args.pop_front()
        .ok_or_else(|| format!("missing value for {flag}"))
}
fn number<T: std::str::FromStr>(s: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("invalid number: {s}"))
}
pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
pub fn query(path: &str, params: &[(&str, String)]) -> String {
    if params.is_empty() {
        return path.into();
    }
    format!(
        "{path}?{}",
        params
            .iter()
            .map(|(k, v)| format!("{k}={}", encode(v)))
            .collect::<Vec<_>>()
            .join("&")
    )
}
fn post(path: &str, params: &[(&str, String)]) -> Command {
    Command::Request {
        method: "POST",
        path: query(path, params),
    }
}
impl Options {
    pub fn parse(raw: Vec<String>) -> Result<Self, String> {
        let mut host = None;
        let mut json = false;
        let mut args = VecDeque::new();
        let mut raw = VecDeque::from(raw);
        // Everything after `sim` is passed through verbatim.
        while let Some(arg) = raw.pop_front() {
            match arg.as_str() {
                "--host" => host = Some(value(&mut raw, "--host")?),
                "--json" => json = true,
                "sim" => {
                    args.push_back(arg);
                    args.append(&mut raw);
                    break;
                }
                "--pass" | "--port" | "--tilt" | "--tilt-deg" | "--tilt-dir" | "--touch"
                | "--hold-ms" | "--date" | "--days" | "--every" | "--until" => {
                    let v = value(&mut raw, &arg)?;
                    args.push_back(arg);
                    args.push_back(v);
                }
                _ => args.push_back(arg),
            }
        }
        let command = match args.pop_front().as_deref().unwrap_or("help") {
            "help" | "--help" | "-h" => Command::Help,
            "status" => Command::Request {
                method: "GET",
                path: "/status".into(),
            },
            "logs" => {
                let follow = args.front().map(String::as_str) == Some("--follow");
                if follow {
                    args.pop_front();
                }
                Command::Logs { follow }
            }
            "screenshot" => Command::Screenshot(value(&mut args, "screenshot")?),
            "app" => post("/app", &[("name", value(&mut args, "app")?)]),
            "volume" => {
                let level: u8 = number(&value(&mut args, "volume")?)?;
                if level > 100 {
                    return Err("volume must be 0..=100".into());
                }
                post("/volume", &[("level", level.to_string())])
            }
            "timezone" => {
                let arg = args.pop_front();
                if let Some(ref s) = arg {
                    if netproto::tz::parse(s).is_none() {
                        return Err(format!("invalid timezone: {s}"));
                    }
                }
                Command::Timezone(arg)
            }
            "sound" => {
                let kind = value(&mut args, "sound")?;
                if !matches!(kind.as_str(), "tick" | "click" | "chime") {
                    return Err("sound must be tick, click or chime".into());
                }
                post("/sound", &[("kind", kind)])
            }
            "reboot" => post("/reboot", &[]),
            "key" => match args.pop_front().as_deref() {
                None => post("/key", &[]),
                Some("--long") => post("/key", &[("long", "1".into())]),
                Some(s) => return Err(format!("unknown key option: {s}")),
            },
            "setup" => post("/setup", &[]),
            "input" => parse_input(&mut args)?,
            "remind" => parse_remind(&mut args)?,
            "wifi" => match value(&mut args, "wifi")?.as_str() {
                "list" => Command::Request {
                    method: "GET",
                    path: "/wifi".into(),
                },
                "forget" => post("/wifi/forget", &[("ssid", value(&mut args, "ssid")?)]),
                "add" => {
                    let ssid = value(&mut args, "ssid")?;
                    match args.pop_front().as_deref() {
                        None => post("/wifi", &[("ssid", ssid), ("pass", String::new())]),
                        Some("--pass") => post(
                            "/wifi",
                            &[("ssid", ssid), ("pass", value(&mut args, "--pass")?)],
                        ),
                        Some("--pass-stdin") => Command::WifiStdin { ssid },
                        Some(s) => return Err(format!("unknown wifi option: {s}")),
                    }
                }
                "sync" => {
                    let mut usb = false;
                    let mut port = None;
                    while let Some(flag) = args.pop_front() {
                        match flag.as_str() {
                            "--usb" if !usb => usb = true,
                            "--port" if port.is_none() => port = Some(value(&mut args, "--port")?),
                            _ => return Err(format!("invalid wifi sync option: {flag}")),
                        }
                    }
                    if !usb {
                        return Err("wifi sync requires --usb".into());
                    }
                    Command::WifiSync { port }
                }
                _ => return Err("wifi requires list, add, forget, or sync".into()),
            },
            "build" => Command::Build,
            "flash" => {
                let mut mode = None;
                let mut port = None;
                while let Some(flag) = args.pop_front() {
                    match flag.as_str() {
                        "--usb" | "--ota" if mode.is_none() => mode = Some(flag == "--ota"),
                        "--port" if port.is_none() => port = Some(value(&mut args, "--port")?),
                        _ => return Err(format!("invalid flash option: {flag}")),
                    }
                }
                let ota = mode.ok_or("flash requires --usb or --ota")?;
                if ota && port.is_some() {
                    return Err("--port requires --usb".into());
                }
                Command::Flash { ota, port }
            }
            "pair" => {
                let port = if args.front().map(String::as_str) == Some("--port") {
                    args.pop_front();
                    Some(value(&mut args, "--port")?)
                } else {
                    None
                };
                Command::Pair { port }
            }
            "sim" => {
                if !matches!(
                    args.front().map(String::as_str),
                    Some("visuals" | "glyphfield" | "blobs" | "flow" | "sand" | "clock" | "pikachu" | "city" | "pokeball")
                ) {
                    return Err("sim requires visuals, glyphfield, blobs, flow, sand, clock, pikachu, city or pokeball".into());
                }
                Command::Sim(args.drain(..).collect())
            }
            other => return Err(format!("unknown command: {other}")),
        };
        if !args.is_empty() {
            return Err(format!(
                "unexpected arguments: {}",
                args.into_iter().collect::<Vec<_>>().join(" ")
            ));
        }
        Ok(Self {
            host,
            json,
            command,
        })
    }
}
fn parse_input(args: &mut VecDeque<String>) -> Result<Command, String> {
    let (mut tilt, mut degrees, mut direction, mut touch, mut hold) =
        (None, None, None, None, None);
    let mut tap = false;
    while let Some(flag) = args.pop_front() {
        match flag.as_str() {
            "--tap" if !tap => tap = true,
            "--tilt" if tilt.is_none() => {
                let s = value(args, &flag)?;
                let numbers = s
                    .split(',')
                    .map(number::<i16>)
                    .collect::<Result<Vec<_>, _>>()?;
                if numbers.len() != 3 {
                    return Err("tilt requires x,y,z".into());
                }
                tilt = Some(
                    numbers
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(","),
                );
            }
            "--tilt-deg" | "--tilt-dir" => {
                let n: f64 = number(&value(args, &flag)?)?;
                if !n.is_finite() {
                    return Err("angles must be finite".into());
                }
                let slot = if flag == "--tilt-deg" {
                    &mut degrees
                } else {
                    &mut direction
                };
                if slot.replace(n).is_some() {
                    return Err(format!("duplicate option: {flag}"));
                }
            }
            "--touch" if touch.is_none() => {
                let s = value(args, &flag)?;
                let nums = s
                    .split(',')
                    .map(number::<u16>)
                    .collect::<Result<Vec<_>, _>>()?;
                if nums.len() != 2 || nums.iter().any(|n| *n >= 480) {
                    return Err("touch requires x,y in 0..480".into());
                }
                touch = Some(format!("{},{}", nums[0], nums[1]));
            }
            "--hold-ms" if hold.is_none() => {
                let n: u32 = number(&value(args, &flag)?)?;
                if n > 60000 {
                    return Err("hold-ms must be at most 60000".into());
                }
                hold = Some(n.to_string());
            }
            _ => return Err(format!("invalid input option: {flag}")),
        }
    }
    if tilt.is_some() && degrees.is_some() {
        return Err("use --tilt or --tilt-deg".into());
    }
    if direction.is_some() && degrees.is_none() {
        return Err("--tilt-dir requires --tilt-deg".into());
    }
    if let Some(degrees) = degrees {
        let a = (degrees % 360.).to_radians();
        let d = (direction.unwrap_or(0.) % 360.).to_radians();
        tilt = Some(format!(
            "{},{},{}",
            (a.sin() * d.cos() * 1000.).round() as i16,
            (a.sin() * d.sin() * 1000.).round() as i16,
            (a.cos() * 1000.).round() as i16
        ));
    }
    let mut params = Vec::new();
    if let Some(v) = tilt {
        params.push(("tilt", v));
    }
    if let Some(v) = touch {
        params.push(("touch", v));
    }
    if tap {
        params.push(("tap", "1".into()));
    }
    if let Some(v) = hold {
        params.push(("hold_ms", v));
    }
    Ok(post("/input", &params))
}

fn parse_remind(args: &mut VecDeque<String>) -> Result<Command, String> {
    let id = |args: &mut VecDeque<String>| -> Result<String, String> {
        let id: u8 = number(&value(args, "id")?)?;
        Ok(id.to_string())
    };
    Ok(match value(args, "remind")?.as_str() {
        "list" => Command::Request {
            method: "GET",
            path: "/reminders".into(),
        },
        "delete" => post("/reminders/delete", &[("id", id(args)?)]),
        "test" => post("/reminders/test", &[("id", id(args)?)]),
        "dismiss" => post("/reminders/dismiss", &[]),
        "snooze" => post("/reminders/snooze", &[]),
        "add" => {
            let time = value(args, "time")?;
            let valid_time = time
                .split_once(':')
                .and_then(|(h, m)| Some((h.parse::<u8>().ok()?, m.parse::<u8>().ok()?)))
                .is_some_and(|(h, m)| h < 24 && m < 60);
            if !valid_time {
                return Err("time must be HH:MM".into());
            }
            let mut params = vec![("time", time)];
            let mut text = Vec::new();
            let (mut date, mut days, mut every, mut until) = (None, None, None, None);
            while let Some(arg) = args.pop_front() {
                let slot = match arg.as_str() {
                    "--date" => &mut date,
                    "--days" => &mut days,
                    "--every" => &mut every,
                    "--until" => &mut until,
                    _ => {
                        text.push(arg);
                        continue;
                    }
                };
                if slot.replace(value(args, &arg)?).is_some() {
                    return Err(format!("duplicate option: {arg}"));
                }
            }
            let text = text.join(" ");
            if text.is_empty() || text.len() > 60 || !text.bytes().all(|b| (0x20..0x7F).contains(&b)) {
                return Err("reminder text must be 1..60 plain ASCII characters".into());
            }
            if date.is_some() && (days.is_some() || every.is_some()) {
                return Err("--date is for one-off reminders; drop --days/--every".into());
            }
            if until.is_some() && every.is_none() {
                return Err("--until needs --every".into());
            }
            if let Some(d) = date {
                params.push(("date", d));
            }
            if let Some(d) = days {
                params.push(("days", d));
            }
            if let Some(e) = every {
                params.push(("every", minutes(&e)?.to_string()));
            }
            if let Some(u) = until {
                params.push(("until", u));
            }
            params.push(("text", text));
            post("/reminders", &params)
        }
        other => return Err(format!("remind requires list, add, delete, test, dismiss or snooze, not {other}")),
    })
}

/// "30m", "2h" or a plain number of minutes.
fn minutes(s: &str) -> Result<u16, String> {
    let (n, unit) = match s.strip_suffix('h') {
        Some(n) => (n, 60),
        None => (s.strip_suffix('m').unwrap_or(s), 1),
    };
    let n: u16 = number(n)?;
    match n.checked_mul(unit) {
        Some(m) if (1..=1440).contains(&m) => Ok(m),
        _ => Err("--every must be 1m..24h".into()),
    }
}

pub const USAGE: &str = "Usage: espctl [--host HOST[:PORT]] [--json] <command>
  status                         Device status
  logs [--follow]                Read logs; follow polls every second
  screenshot <out.png>           Save a 480x480 PNG
  input [--tilt x,y,z | --tilt-deg D [--tilt-dir DEG]]
        [--touch x,y] [--tap] [--hold-ms N]
  app <name>                    Switch app
  volume <0-100>                Set speaker volume
  timezone [<sgt|ist|+H:MM>]    Get or set time zone
  sound <tick|click|chime>      Play a sound
  remind list                   Saved reminders and when each fires next
  remind add <HH:MM> <text> [--date YYYY-MM-DD | --days D | --every 30m|2h [--until HH:MM] [--days D]]
                                No option: once, the next time it is HH:MM.
                                D: daily, weekdays, weekends or mon,wed,fri
  remind delete <id>            Delete a reminder
  remind test <id>              Show a reminder now
  remind dismiss | snooze       Dismiss or snooze (10 min) the one on screen
  key [--long]                  Press KEY (the app's modifier; in visuals,
                                the next visual), or hold it (settings)
  reboot                        Reboot device
  setup                         Reboot into Wi-Fi setup mode
  wifi list                     List saved networks
  wifi add <ssid> [--pass <p> | --pass-stdin]
  wifi forget <ssid>            Forget a saved network
  wifi sync --usb [--port P]    Write WIFI_SSID/WIFI_PASS from .env over USB
  build                         Build release firmware
  flash --usb [--port P]        Build and flash over USB
  flash --ota                   Build and flash over Wi-Fi
  pair [--port P]               Save token and IP from USB monitor
  sim <visuals|glyphfield|blobs|flow|sand|clock|pikachu|city|pokeball> [args...]  Run simulator (arguments passed through)
  help                          Show this help
Global flags go before `sim`; subsequent flags belong to the simulator.";
