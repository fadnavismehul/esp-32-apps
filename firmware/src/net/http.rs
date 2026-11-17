//! HTTP API (see docs/api.md) and the setup-mode form.
//!
//! Several server tasks so a slow request (OTA, screenshot) does not block
//! `GET /status`, and so a phone's burst of parallel connections in setup
//! mode is not answered with resets. One request per connection.

use alloc::string::String;
use core::fmt::Write;

use app_sdk::{App, Sound, Tilt};
use embassy_executor::Spawner;
use embassy_net::Stack;
use embassy_net::tcp::TcpSocket;
use embassy_time::{Duration, Timer};
use embedded_io_async::Write as _;
use esp_bootloader_esp_idf::ota::OtaImageState;
use esp_bootloader_esp_idf::ota_updater::OtaUpdater;
use esp_bootloader_esp_idf::partitions::{FlashStorage, PARTITION_TABLE_MAX_LEN};
use log::{info, warn};
use netproto::http::{self, Request, param, url_decode};
use netproto::reminders::{self, Schedule, format_days, format_minute, parse_date, parse_days, parse_hhmm};

use crate::apps::{ActiveApp, NAMES};
use crate::state::{self, Mode};
use crate::{logger, persist, remind};

const FW_VERSION: &str = env!("CARGO_PKG_VERSION");
const PORTAL: &str = include_str!("portal.html");

/// Listening sockets. Each one costs about 5.5 KB of RAM. smoltcp resets any
/// connection that arrives while all of them are busy.
pub const SERVERS: usize = 5;

pub fn spawn(spawner: Spawner, stack: Stack<'static>, setup: bool) {
    for _ in 0..SERVERS {
        spawner.spawn(server(stack, setup).unwrap());
    }
}

#[embassy_executor::task(pool_size = SERVERS)]
async fn server(stack: Stack<'static>, setup: bool) {
    let mut rx = [0u8; 2048];
    let mut tx = [0u8; 2048];
    loop {
        let mut sock = TcpSocket::new(stack, &mut rx, &mut tx);
        sock.set_timeout(Some(Duration::from_secs(20)));
        if sock.accept(80).await.is_err() {
            continue;
        }
        let started = state::now_ms();
        let peer = sock.remote_endpoint();
        if setup {
            info!("http: accept {peer:?}");
        }
        if let Err(e) = serve(&mut sock, setup).await {
            warn!("http: {e:?}");
        }
        let served = state::now_ms();
        sock.close();
        let flushed = sock.flush().await;
        if setup {
            info!(
                "http: {peer:?} served in {} ms, flush {flushed:?} after {} ms, state {:?}",
                served - started,
                state::now_ms() - served,
                sock.state(),
            );
        }
        Timer::after(Duration::from_millis(20)).await;
        sock.abort();
    }
}

type Sock<'a> = TcpSocket<'a>;
type Res = Result<(), embassy_net::tcp::Error>;

async fn serve(sock: &mut Sock<'_>, setup: bool) -> Res {
    let mut buf = [0u8; 1536];
    let mut filled = 0;
    let req = loop {
        let n = sock.read(&mut buf[filled..]).await?;
        if n == 0 {
            return Ok(());
        }
        filled += n;
        match http::parse_request(&buf[..filled]) {
            Ok(Some(_)) => break http::parse_request(&buf[..filled]).ok().flatten().unwrap(),
            Ok(None) if filled < buf.len() => continue,
            _ => return respond(sock, 400, "text/plain", b"bad request\n").await,
        }
    };
    let body_start = req.header_len.min(filled);
    let early = &buf[body_start..filled];

    if setup {
        info!("setup: {} {}", req.method, req.path);
        return serve_setup(sock, &req, early).await;
    }

    if req.path != "/status" {
        let ok = state::config(|c| !c.token().is_empty() && req.bearer == Some(c.token()));
        if !ok {
            return respond(sock, 401, "text/plain", b"missing or wrong token\n").await;
        }
    }

    match (req.method, req.path) {
        ("GET", "/status") => status(sock).await,
        ("GET", "/logs") => {
            let mut text = [0u8; 4096];
            let n = logger::snapshot(&mut text);
            respond(sock, 200, "text/plain", &text[..n]).await
        }
        ("GET", "/screenshot") => screenshot(sock).await,
        ("POST", "/input") => input(sock, req.query).await,
        ("POST", "/app") => {
            let name = param(req.query, "name").and_then(ActiveApp::canonical);
            let Some(name) = name else {
                return respond(sock, 404, "text/plain", b"unknown app\n").await;
            };
            state::SWITCH_APP.signal(name);
            state::config(|c| {
                let _ = c.set_app(name);
            });
            persist::save().await;
            respond(sock, 204, "text/plain", b"").await
        }
        ("POST", "/volume") => {
            let level = param(req.query, "level").and_then(|v| v.parse::<u8>().ok());
            let Some(level) = level.filter(|l| *l <= 100) else {
                return respond(sock, 400, "text/plain", b"level must be 0..100\n").await;
            };
            state::config(|c| c.set_volume(level));
            state::VOLUME.signal(level);
            persist::save().await;
            respond(sock, 204, "text/plain", b"").await
        }
        ("GET", "/timezone") => {
            let min = state::utc_offset_min();
            let mut b = String::new();
            let _ = write!(b, "{{\"utc_offset_min\":{min}}}\n");
            respond(sock, 200, "application/json", b.as_bytes()).await
        }
        ("POST", "/timezone") => {
            let len = req.content_length.unwrap_or(early.len()).min(64);
            let mut body_buf = [0u8; 64];
            let mut got = early.len().min(len);
            body_buf[..got].copy_from_slice(&early[..got]);
            while got < len {
                let n = sock.read(&mut body_buf[got..len]).await?;
                if n == 0 {
                    break;
                }
                got += n;
            }
            let body_str = core::str::from_utf8(&body_buf[..got]).ok().map(str::trim).filter(|s| !s.is_empty());
            // The zone comes as the plain body, or as `?zone=` with `+` written `%2B`.
            let mut q_buf = [0u8; 32];
            let input = body_str.or_else(|| param(req.query, "zone").and_then(|v| url_decode(v, &mut q_buf)));
            let offset = input.and_then(netproto::tz::parse);
            let Some(min) = offset else {
                return respond(sock, 400, "text/plain", b"invalid timezone\n").await;
            };
            state::set_utc_offset_min(min);
            persist::save().await;
            let mut b = String::new();
            let _ = write!(b, "{{\"utc_offset_min\":{min}}}\n");
            respond(sock, 200, "application/json", b.as_bytes()).await
        }
        ("POST", "/sound") => {
            let sound = match param(req.query, "kind") {
                Some("tick") => Sound::Tick { pitch: 128 },
                Some("click") => Sound::Click,
                Some("chime") => Sound::Chime,
                _ => return respond(sock, 400, "text/plain", b"kind must be tick, click or chime\n").await,
            };
            state::SOUND.signal(sound);
            respond(sock, 204, "text/plain", b"").await
        }
        ("GET", "/wifi") => {
            let mut body = String::from("{\"networks\":[");
            state::config(|c| {
                for (i, n) in c.networks().enumerate() {
                    if i > 0 {
                        body.push(',');
                    }
                    push_json_str(&mut body, n.ssid());
                }
            });
            body.push_str("]}");
            respond(sock, 200, "application/json", body.as_bytes()).await
        }
        ("POST", "/wifi") => {
            let (mut s, mut p) = ([0u8; 64], [0u8; 128]);
            let ssid = param(req.query, "ssid").and_then(|v| url_decode(v, &mut s));
            let pass = param(req.query, "pass").map(|v| url_decode(v, &mut p)).unwrap_or(Some(""));
            match (ssid, pass) {
                (Some(ssid), Some(pass)) if state::config(|c| c.add_network(ssid, pass).is_ok()) => {
                    info!("wifi: saved network '{ssid}'");
                    persist::save().await;
                    respond(sock, 204, "text/plain", b"").await
                }
                _ => respond(sock, 400, "text/plain", b"bad ssid or pass\n").await,
            }
        }
        ("POST", "/wifi/forget") => {
            let mut s = [0u8; 64];
            let ssid = param(req.query, "ssid").and_then(|v| url_decode(v, &mut s));
            let forgot = ssid.is_some_and(|ssid| state::config(|c| c.forget(ssid)));
            if !forgot {
                return respond(sock, 404, "text/plain", b"not saved\n").await;
            }
            persist::save().await;
            respond(sock, 204, "text/plain", b"").await
        }
        ("GET", "/reminders") => list_reminders(sock).await,
        ("POST", "/reminders") => add_reminder(sock, req.query).await,
        ("POST", "/reminders/delete") => {
            let id = param(req.query, "id").and_then(|v| v.parse::<u8>().ok());
            if !id.is_some_and(|id| state::reminders(|l| l.remove(id))) {
                return respond(sock, 404, "text/plain", b"no such reminder\n").await;
            }
            persist::save().await;
            respond(sock, 204, "text/plain", b"").await
        }
        ("POST", "/reminders/test") => {
            let id = param(req.query, "id").and_then(|v| v.parse::<u8>().ok());
            if !id.is_some_and(|id| remind::fire_now(id, state::now_ms())) {
                return respond(sock, 404, "text/plain", b"no such reminder\n").await;
            }
            respond(sock, 204, "text/plain", b"").await
        }
        ("POST", "/reminders/dismiss") => match remind::dismiss() {
            true => respond(sock, 204, "text/plain", b"").await,
            false => respond(sock, 404, "text/plain", b"no reminder showing\n").await,
        },
        ("POST", "/reminders/snooze") => match remind::snooze(state::now_ms()) {
            true => respond(sock, 204, "text/plain", b"").await,
            false => respond(sock, 404, "text/plain", b"no reminder showing\n").await,
        },
        ("POST", "/ota") => ota(sock, &req, early).await,
        ("POST", "/key") => {
            state::KEY.signal(param(req.query, "long").is_some_and(|v| v == "1"));
            respond(sock, 204, "text/plain", b"").await
        }
        ("POST", "/reboot") => {
            respond(sock, 204, "text/plain", b"").await?;
            reboot_soon(false).await
        }
        ("POST", "/setup") => {
            respond(sock, 204, "text/plain", b"").await?;
            reboot_soon(true).await
        }
        _ => respond(sock, 404, "text/plain", b"not found\n").await,
    }
}

fn push_hhmm(b: &mut String, minute: u16) {
    let _ = write!(b, "\"{:02}:{:02}\"", minute / 60, minute % 60);
}

async fn list_reminders(sock: &mut Sock<'_>) -> Res {
    let now = state::local_time_s().map(|s| s / 60);
    let mut b = String::from("{\"now\":");
    match now {
        Some(m) => {
            b.push('"');
            let _ = format_minute(m, &mut b);
            b.push('"');
        }
        None => b.push_str("null"),
    }
    b.push_str(",\"showing\":");
    match remind::current() {
        Some((a, _)) => {
            let _ = write!(b, "{{\"id\":{},\"text\":", a.id);
            push_json_str(&mut b, a.text());
            b.push('}');
        }
        None => b.push_str("null"),
    }
    let _ = write!(b, ",\"waiting\":{},\"reminders\":[", remind::waiting());
    state::reminders(|list| {
        for (i, r) in list.iter().enumerate() {
            let _ = write!(b, "{}{{\"id\":{},\"text\":", if i > 0 { "," } else { "" }, r.id);
            push_json_str(&mut b, r.text());
            match r.schedule {
                Schedule::Once { at } => {
                    b.push_str(",\"repeat\":\"once\",\"at\":\"");
                    let _ = format_minute(at, &mut b);
                    b.push('"');
                }
                Schedule::Weekly { days, minute } => {
                    b.push_str(",\"repeat\":\"weekly\",\"time\":");
                    push_hhmm(&mut b, minute);
                    b.push_str(",\"days\":\"");
                    let _ = format_days(days, &mut b);
                    b.push('"');
                }
                Schedule::Interval { days, from, to, every } => {
                    let _ = write!(b, ",\"repeat\":\"interval\",\"every_min\":{every},\"from\":");
                    push_hhmm(&mut b, from);
                    b.push_str(",\"to\":");
                    push_hhmm(&mut b, to);
                    b.push_str(",\"days\":\"");
                    let _ = format_days(days, &mut b);
                    b.push('"');
                }
            }
            b.push_str(",\"next\":");
            match now.and_then(|m| r.next(m)) {
                Some(n) => {
                    b.push('"');
                    let _ = format_minute(n, &mut b);
                    b.push('"');
                }
                None => b.push_str("null"),
            }
            b.push('}');
        }
    });
    b.push_str("]}");
    respond(sock, 200, "application/json", b.as_bytes()).await
}

/// `time=HH:MM&text=...`, plus one of: nothing (the next HH:MM), `date=YYYY-MM-DD`,
/// `days=...` (weekly), or `every=N` minutes with optional `until=HH:MM` and `days`.
async fn add_reminder(sock: &mut Sock<'_>, query: &str) -> Res {
    let mut dec = [0u8; 64];
    let mut get = |key: &str| -> Option<String> {
        param(query, key).and_then(|v| url_decode(v, &mut dec)).map(String::from)
    };
    let mut text_buf = [0u8; 4 * reminders::TEXT_MAX];
    let text = param(query, "text").and_then(|v| url_decode(v, &mut text_buf));
    let (Some(time), Some(text)) = (get("time").as_deref().and_then(parse_hhmm), text) else {
        return respond(sock, 400, "text/plain", b"time=HH:MM and text are required\n").await;
    };
    let days = match get("days") {
        Some(d) => match parse_days(&d) {
            Some(d) => Some(d),
            None => return respond(sock, 400, "text/plain", b"days: daily, weekdays, weekends or mon,tue,...\n").await,
        },
        None => None,
    };
    let now = state::local_time_s().map(|s| s / 60);
    let schedule = if let Some(every) = get("every") {
        let every = every.parse::<u16>().ok().filter(|e| (1..=1440).contains(e));
        let until = match get("until") {
            Some(u) => parse_hhmm(&u),
            None => Some(23 * 60 + 59),
        };
        let (Some(every), Some(to)) = (every, until) else {
            return respond(sock, 400, "text/plain", b"every=1..1440 minutes, until=HH:MM\n").await;
        };
        if to < time {
            return respond(sock, 400, "text/plain", b"until must not be before time\n").await;
        }
        Schedule::Interval { days: days.unwrap_or(reminders::DAILY), from: time, to, every }
    } else if let Some(days) = days {
        Schedule::Weekly { days, minute: time }
    } else if let Some(date) = get("date") {
        let Some(day) = parse_date(&date) else {
            return respond(sock, 400, "text/plain", b"date must be YYYY-MM-DD\n").await;
        };
        let at = day * 1440 + time as u32;
        if now.is_some_and(|m| at < m) {
            return respond(sock, 400, "text/plain", b"that time has passed\n").await;
        }
        Schedule::Once { at }
    } else {
        let Some(m) = now else {
            return respond(sock, 503, "text/plain", b"clock not set yet; give a date\n").await;
        };
        let today = m / 1440 * 1440 + time as u32;
        Schedule::Once { at: if today < m { today + 1440 } else { today } }
    };
    let added = state::reminders(|l| l.add(schedule, text));
    let id = match added {
        Ok(id) => id,
        Err(reminders::ReminderError::Full) => {
            return respond(sock, 409, "text/plain", b"16 reminders saved already; delete one\n").await;
        }
        Err(reminders::ReminderError::BadText) => {
            return respond(sock, 400, "text/plain", b"text must be 1..60 plain ASCII characters\n").await;
        }
        Err(reminders::ReminderError::BadSchedule) => {
            return respond(sock, 400, "text/plain", b"bad schedule\n").await;
        }
    };
    persist::save().await;
    info!("remind: added #{id}");
    let mut b = String::new();
    let _ = write!(b, "{{\"id\":{id},\"next\":");
    let next = now.and_then(|m| state::reminders(|l| l.get(id).and_then(|r| r.next(m))));
    match next {
        Some(n) => {
            b.push('"');
            let _ = format_minute(n, &mut b);
            b.push('"');
        }
        None => b.push_str("null"),
    }
    b.push('}');
    respond(sock, 201, "application/json", b.as_bytes()).await
}

async fn reboot_soon(setup: bool) -> ! {
    Timer::after(Duration::from_millis(500)).await;
    if setup {
        super::request_setup_and_reboot()
    }
    esp_hal::system::software_reset()
}

async fn respond(sock: &mut Sock<'_>, status: u16, ctype: &str, body: &[u8]) -> Res {
    let mut head = [0u8; 256];
    let n = http::write_response(&mut head, status, ctype, body.len());
    sock.write_all(&head[..n]).await?;
    sock.write_all(body).await
}

fn push_json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

async fn status(sock: &mut Sock<'_>) -> Res {
    let app = state::APP.lock().await.name();
    let (mode, ssid, ip, rssi, fps) = state::with(|s| {
        let mode = match s.mode {
            Mode::Setup => "ap",
            Mode::Off => "off",
            _ => "sta",
        };
        (mode, s.ssid().map(String::from), s.ip, s.rssi, s.fps)
    });
    let volume = state::config(|c| c.volume());
    let offset_min = state::utc_offset_min();
    let mut b = String::new();
    let _ = write!(b, "{{\"fw\":\"{FW_VERSION}\",\"mode\":\"{mode}\",\"ssid\":");
    match ssid {
        Some(s) => push_json_str(&mut b, &s),
        None => b.push_str("null"),
    }
    match ip {
        Some(ip) => {
            let _ = write!(b, ",\"ip\":\"{}.{}.{}.{}\"", ip[0], ip[1], ip[2], ip[3]);
        }
        None => b.push_str(",\"ip\":null"),
    }
    match rssi {
        Some(r) => {
            let _ = write!(b, ",\"rssi\":{r}");
        }
        None => b.push_str(",\"rssi\":null"),
    }
    let _ = write!(b, ",\"app\":\"{app}\",\"apps\":[");
    for (i, n) in NAMES.iter().enumerate() {
        let _ = write!(b, "{}\"{n}\"", if i > 0 { "," } else { "" });
    }
    let _ = write!(
        b,
        "],\"fps\":{fps},\"uptime_s\":{},\"heap_free\":{},\"volume\":{volume},\"utc_offset_min\":{offset_min},\"ota_slot\":\"{}\"}}",
        state::now_ms() / 1000,
        esp_alloc::HEAP.free(),
        crate::ota_slot_name(),
    );
    respond(sock, 200, "application/json", b.as_bytes()).await
}

async fn screenshot(sock: &mut Sock<'_>) -> Res {
    const ROWS: u16 = 2;
    let mut head = [0u8; 256];
    let n = http::write_response(&mut head, 200, "application/octet-stream", 480 * 480 * 2);
    sock.write_all(&head[..n]).await?;
    let mut px = [0u16; 480 * ROWS as usize];
    let mut bytes = [0u8; 480 * ROWS as usize * 2];
    // One moment for the whole picture, so animation does not band it.
    let now = state::now_ms();
    let mut y = 0;
    while y < 480 {
        match crate::remind::current() {
            Some((a, shown)) => crate::alert::render(y..y + ROWS, &mut px, &a, shown, now),
            None if crate::settings::is_open() => crate::settings::render(y..y + ROWS, &mut px),
            None => state::APP.lock().await.render(y..y + ROWS, &mut px),
        }
        for (b, p) in bytes.chunks_exact_mut(2).zip(&px) {
            b.copy_from_slice(&p.to_be_bytes());
        }
        sock.write_all(&bytes).await?;
        y += ROWS;
    }
    Ok(())
}

/// Parse "a,b,c" (already URL-decoded) into exactly `N` integers.
fn parse_list<const N: usize>(v: &str) -> Option<[i32; N]> {
    let mut out = [0i32; N];
    let mut parts = v.split(',');
    for slot in out.iter_mut() {
        *slot = parts.next()?.trim().parse().ok()?;
    }
    parts.next().is_none().then_some(out)
}

async fn input(sock: &mut Sock<'_>, query: &str) -> Res {
    let mut dec = [0u8; 64];
    let mut get = |key: &str| -> Option<String> {
        param(query, key).and_then(|v| url_decode(v, &mut dec)).map(String::from)
    };
    let tilt = get("tilt").map(|v| parse_list::<3>(&v));
    let touch = get("touch").map(|v| parse_list::<2>(&v));
    let tap = get("tap").is_some_and(|v| v == "1" || v == "true");
    let hold = get("hold_ms").and_then(|v| v.parse::<u64>().ok()).unwrap_or(3000).min(60_000);
    if matches!(tilt, Some(None)) || matches!(touch, Some(None)) {
        return respond(sock, 400, "text/plain", b"tilt=x,y,z touch=x,y\n").await;
    }
    let tilt = tilt.flatten().map(|[x, y, z]| Tilt {
        x_mg: x.clamp(-4000, 4000) as i16,
        y_mg: y.clamp(-4000, 4000) as i16,
        z_mg: z.clamp(-4000, 4000) as i16,
    });
    let touch = touch.flatten().map(|[x, y]| (x.clamp(0, 479) as u16, y.clamp(0, 479) as u16));
    state::with(|s| {
        s.ovr.until_ms = state::now_ms() + hold;
        s.ovr.tilt = tilt;
        s.ovr.touch = touch;
        s.ovr.tap |= tap;
    });
    respond(sock, 204, "text/plain", b"").await
}

async fn ota(sock: &mut Sock<'_>, req: &Request<'_>, early: &[u8]) -> Res {
    let Some(len) = req.content_length.filter(|l| *l > 0) else {
        return respond(sock, 400, "text/plain", b"Content-Length required\n").await;
    };
    info!("ota: receiving {len} bytes");
    state::with(|s| s.busy = true);
    let result = ota_write(sock, len, early).await;
    state::with(|s| s.busy = false);
    match result {
        Ok(()) => {
            info!("ota: done, rebooting into the new image");
            respond(sock, 204, "text/plain", b"").await?;
            reboot_soon(false).await
        }
        Err(msg) => {
            warn!("ota: failed: {msg}");
            respond(sock, 400, "text/plain", msg.as_bytes()).await
        }
    }
}

async fn ota_write(sock: &mut Sock<'_>, len: usize, early: &[u8]) -> Result<(), &'static str> {
    const SECTOR: usize = FlashStorage::SECTOR_SIZE as usize;
    let mut guard = persist::FLASH.lock().await;
    let flash = guard.as_mut().ok_or("no flash\n")?;
    let mut table = [0u8; PARTITION_TABLE_MAX_LEN];
    let mut ota = OtaUpdater::new(flash, &mut table).map_err(|_| "no OTA partitions\n")?;
    {
        let (mut region, slot) = ota.next_partition().map_err(|_| "no next slot\n")?;
        if len > region.capacity() {
            return Err("image larger than the slot\n");
        }
        info!("ota: writing to {slot:?}");
        let mut chunk = [0u8; SECTOR];
        let mut written = 0;
        let mut pending = early.len().min(len);
        chunk[..pending].copy_from_slice(&early[..pending]);
        if pending > 0 && chunk[0] != 0xE9 {
            return Err("not an ESP app image\n");
        }
        while written < len {
            let want = (len - written).min(SECTOR);
            while pending < want {
                let n = sock.read(&mut chunk[pending..want]).await.map_err(|_| "upload interrupted\n")?;
                if n == 0 {
                    return Err("upload cut short\n");
                }
                pending += n;
            }
            if written == 0 && chunk[0] != 0xE9 {
                return Err("not an ESP app image\n");
            }
            let padded = want.next_multiple_of(4);
            chunk[want..padded].fill(0xFF);
            let at = written as u32;
            region.erase(at, at + SECTOR as u32).map_err(|_| "flash erase failed\n")?;
            region.write(at, &chunk[..padded]).map_err(|_| "flash write failed\n")?;
            written += want;
            pending = 0;
        }
    }
    ota.activate_next_partition().map_err(|_| "activate failed\n")?;
    ota.set_current_ota_state(OtaImageState::New).map_err(|_| "set state failed\n")?;
    Ok(())
}

async fn serve_setup(sock: &mut Sock<'_>, req: &Request<'_>, early: &[u8]) -> Res {
    match (req.method, req.path) {
        ("GET", "/status") => status(sock).await,
        ("POST", "/") => {
            let len = req.content_length.unwrap_or(0).min(512);
            let mut body = [0u8; 512];
            let mut got = early.len().min(len);
            body[..got].copy_from_slice(&early[..got]);
            while got < len {
                let n = sock.read(&mut body[got..len]).await?;
                if n == 0 {
                    break;
                }
                got += n;
            }
            let form = core::str::from_utf8(&body[..got]).unwrap_or("");
            let (mut s, mut p) = ([0u8; 64], [0u8; 128]);
            let ssid = param(form, "ssid").and_then(|v| url_decode(v, &mut s)).map(str::trim);
            let pass = param(form, "pass").map(|v| url_decode(v, &mut p)).unwrap_or(Some(""));
            let saved = match (ssid, pass) {
                (Some(ssid), Some(pass)) if !ssid.is_empty() => {
                    state::config(|c| c.add_network(ssid, pass).is_ok())
                }
                _ => false,
            };
            if !saved {
                let page = PORTAL.replace("<!--MSG-->", "<p class=err>That did not look right. Try again.</p>");
                return respond(sock, 400, "text/html", page.as_bytes()).await;
            }
            persist::save().await;
            info!("setup: saved network, rebooting into station mode");
            let page = PORTAL
                .replace("<!--MSG-->", "<p class=ok>Saved. The device is rebooting and will join the network.</p>")
                .replace("<!--FORM-->", "<!--");
            respond(sock, 200, "text/html", page.as_bytes()).await?;
            reboot_soon(false).await
        }
        ("GET", "/") => respond(sock, 200, "text/html", PORTAL.as_bytes()).await,
        // Anything else (phone captive-portal probes included) goes to the form.
        _ => {
            let head = "HTTP/1.1 302 Found\r\nLocation: http://192.168.4.1/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            sock.write_all(head.as_bytes()).await
        }
    }
}
