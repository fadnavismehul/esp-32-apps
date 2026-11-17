mod cli;
mod config;
mod device;
mod monitor;
mod usbcfg;

use cli::{Command, Options};
use device::{Device, Result};
use serde_json::{json, Value};
use std::{
    env, fs,
    io::{self, BufRead, Write},
    path::Path,
    process::{Command as Process, Stdio},
    thread,
    time::{Duration, Instant},
};

fn emit(json_mode: bool, value: Value, human: &str) {
    if json_mode {
        println!("{value}");
    } else {
        println!("{human}");
    }
}
fn response(bytes: &[u8]) -> Result<Value> {
    if bytes.is_empty() {
        return Ok(json!({"ok":true}));
    }
    Ok(serde_json::from_slice(bytes)?)
}
fn describe(value: &Value) -> String {
    if let Some(app) = value.get("app").and_then(Value::as_str) {
        let tz_str = value
            .get("utc_offset_min")
            .and_then(Value::as_i64)
            .map(|m| {
                let mut s = String::new();
                let _ = netproto::tz::format(m as i16, &mut s);
                let name = match m as i16 {
                    netproto::tz::DEFAULT_MIN => " (SGT)",
                    netproto::tz::INDIA_MIN => " (IST)",
                    _ => "",
                };
                format!(", timezone: {s}{name}")
            })
            .unwrap_or_default();
        format!(
            "app: {app}, firmware: {}, IP: {}, uptime: {}s{tz_str}",
            value["fw"].as_str().unwrap_or("?"),
            value["ip"].as_str().unwrap_or("offline"),
            value["uptime_s"]
        )
    } else if let Some(min) = value.get("utc_offset_min").and_then(Value::as_i64) {
        let mut s = String::new();
        let _ = netproto::tz::format(min as i16, &mut s);
        let name = match min as i16 {
            netproto::tz::DEFAULT_MIN => " (SGT)",
            netproto::tz::INDIA_MIN => " (IST)",
            _ => "",
        };
        format!("Time zone: {s}{name}")
    } else if let Some(list) = value.get("reminders").and_then(Value::as_array) {
        describe_reminders(value, list)
    } else if let (Some(id), Some(next)) = (value.get("id"), value.get("next")) {
        match next.as_str() {
            Some(next) => format!("Added reminder #{id}, next at {next}"),
            None => format!("Added reminder #{id} (device clock not set yet)"),
        }
    } else if let Some(networks) = value.get("networks").and_then(Value::as_array) {
        format!(
            "Saved Wi-Fi: {}",
            networks
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        "OK".into()
    }
}
fn describe_reminders(value: &Value, list: &[Value]) -> String {
    let s = |v: &Value, k: &str| v[k].as_str().unwrap_or("?").to_owned();
    let mut out = match value["now"].as_str() {
        Some(now) => format!("Device time {now}"),
        None => "Device clock not set yet".into(),
    };
    if let Some(a) = value["showing"].as_object() {
        out += &format!("\nOn screen now: #{} {}", a["id"], a["text"].as_str().unwrap_or(""));
    }
    if list.is_empty() {
        out += "\nNo reminders";
    }
    for r in list {
        let when = match r["repeat"].as_str() {
            Some("once") => format!("once {}", s(r, "at")),
            Some("weekly") => format!("{} {}", s(r, "time"), s(r, "days")),
            Some("interval") => format!(
                "every {} min {}-{} {}",
                r["every_min"],
                s(r, "from"),
                s(r, "to"),
                s(r, "days")
            ),
            _ => "?".into(),
        };
        let next = r["next"].as_str().map(|n| format!("  (next {n})")).unwrap_or_default();
        out += &format!("\n#{:<3} {:<34} {}{next}", r["id"], when, s(r, "text"));
    }
    out
}
fn execute(mut cmd: Process, json_mode: bool) -> Result<()> {
    // Keep JSON stdout reserved for the command result, even for verbose children.
    if json_mode {
        cmd.stdout(Stdio::from(io::stderr()));
    }
    let status = cmd.status()?;
    if !status.success() {
        return Err(format!("subprocess exited with {status}").into());
    }
    Ok(())
}
fn build(root: &Path, json_mode: bool) -> Result<()> {
    let mut cmd = Process::new("cargo");
    cmd.args(["build", "--release"])
        .current_dir(root.join("firmware"));
    execute(cmd, json_mode)
}
fn run(options: Options) -> Result<()> {
    let root = config::root_from(&env::current_dir()?);
    let dotenv = config::read_env(&root.join(".env"))?;
    let device = Device {
        settings: config::resolve(options.host, &dotenv, |k| env::var(k).ok()),
    };
    let json_mode = options.json;
    match options.command {
        Command::Help => emit(json_mode, json!({"usage":cli::USAGE}), cli::USAGE),
        Command::Request { method, path } => {
            let bytes = device.request(method, &path, &[], Duration::from_secs(30))?;
            let value = response(&bytes)?;
            let human = describe(&value);
            emit(json_mode, value, &human);
        }
        Command::Timezone(val) => {
            let bytes = match val {
                Some(zone) => device.request("POST", "/timezone", zone.as_bytes(), Duration::from_secs(30))?,
                None => device.request("GET", "/timezone", &[], Duration::from_secs(30))?,
            };
            let value = response(&bytes)?;
            let human = describe(&value);
            emit(json_mode, value, &human);
        }
        Command::WifiStdin { ssid } => {
            let mut password = String::new();
            io::stdin().lock().read_line(&mut password)?;
            let password = password.trim_end_matches(['\r', '\n']);
            let path = cli::query("/wifi", &[("ssid", ssid), ("pass", password.into())]);
            device.request("POST", &path, &[], Duration::from_secs(30))?;
            emit(json_mode, json!({"ok":true}), "Wi-Fi saved");
        }
        Command::WifiSync { port } => {
            let networks = usbcfg::networks_from_env(&dotenv);
            if networks.is_empty() {
                return Err("no WIFI_SSID in .env".into());
            }
            let written = usbcfg::sync(&root, port.as_deref(), &networks)?;
            let human = if written.is_empty() {
                "Wi-Fi already saved on the device".to_owned()
            } else {
                format!("Wi-Fi saved over USB: {}", written.join(", "))
            };
            emit(json_mode, json!({"written":written}), &human);
        }
        Command::Logs { follow } => {
            let mut previous = String::new();
            loop {
                let current = String::from_utf8(device.get("/logs")?)?;
                let new = device::new_logs(&previous, &current);
                if json_mode {
                    // Follow is a stream: one object per nonempty update.
                    if !follow || !new.is_empty() {
                        println!("{}", json!({"logs":new}));
                    }
                } else {
                    print!("{new}");
                    io::stdout().flush()?;
                }
                if !follow {
                    break;
                }
                previous = current;
                thread::sleep(Duration::from_secs(1));
            }
        }
        Command::Screenshot(path) => {
            let bytes = device.get("/screenshot")?;
            device::screenshot(Path::new(&path), &bytes)?;
            emit(
                json_mode,
                json!({"out":path,"width":480,"height":480}),
                &format!("Saved {path}"),
            );
        }
        Command::Build => {
            build(&root, json_mode)?;
            emit(
                json_mode,
                json!({"ok":true,"elf":root.join("firmware/target/riscv32imac-unknown-none-elf/release/firmware")}),
                "Firmware built",
            );
        }
        Command::Flash { ota, port } => {
            build(&root, json_mode)?;
            let elf = root.join("firmware/target/riscv32imac-unknown-none-elf/release/firmware");
            let partitions = root.join("firmware/partitions.csv");
            let mut cmd = Process::new("espflash");
            if !ota {
                cmd.args(["flash", "--chip", "esp32c6", "--partition-table"])
                    .arg(partitions)
                    .args(["--erase-parts", "otadata", "--non-interactive"]);
                if let Some(port) = &port {
                    cmd.args(["--port", &port]);
                }
                cmd.arg(elf);
                execute(cmd, json_mode)?;
                // Keep .env networks on the device; a no-op when already saved.
                let networks = usbcfg::networks_from_env(&dotenv);
                let written = if networks.is_empty() {
                    Vec::new()
                } else {
                    usbcfg::sync(&root, port.as_deref(), &networks)?
                };
                let human = if written.is_empty() {
                    "Flashed over USB".to_owned()
                } else {
                    format!("Flashed over USB, Wi-Fi saved: {}", written.join(", "))
                };
                emit(
                    json_mode,
                    json!({"ok":true,"transport":"usb","wifi_written":written}),
                    &human,
                );
            } else {
                let image = root.join("firmware/target/firmware.bin");
                cmd.args(["save-image", "--chip", "esp32c6", "--flash-size", "16mb", "--partition-table"])
                    .arg(partitions)
                    .arg(elf)
                    .arg(&image);
                execute(cmd, json_mode)?;
                let bytes = fs::read(image)?;
                device.request("POST", "/ota", &bytes, Duration::from_secs(120))?;
                let status = wait_for_reboot(&device)?;
                let human = format!("OTA complete: {}", describe(&status));
                emit(json_mode, status, &human);
            }
        }
        Command::Pair { port } => {
            let (token, ip) = monitor::pair(port.as_deref())?;
            let path = root.join(".env");
            config::save_pair(&path, &token, ip.as_deref())?;
            let masked = format!("{}…", &token[..4]);
            let human = format!(
                "Saved ESPCTL_TOKEN={masked}{} to {}",
                ip.as_ref()
                    .map(|ip| format!(", ESPCTL_HOST={ip}"))
                    .unwrap_or_default(),
                path.display()
            );
            emit(
                json_mode,
                json!({"saved":path,"token":masked,"host":ip}),
                &human,
            );
        }
        Command::Sim(args) => {
            let mut cmd = Process::new("cargo");
            cmd.args(["run", "--release", "-q", "-p", "sim", "--"])
                .args(args)
                .current_dir(root);
            if json_mode {
                let output = cmd.stderr(Stdio::inherit()).output()?;
                if !output.status.success() {
                    return Err(format!("sim exited with {}", output.status).into());
                }
                let value: Value = serde_json::from_slice(&output.stdout)?;
                if !value.is_object() {
                    return Err("sim did not return a JSON object".into());
                }
                println!("{value}");
            } else {
                execute(cmd, false)?;
            }
        }
    }
    Ok(())
}
fn wait_for_reboot(device: &Device) -> Result<Value> {
    // OTA responds before rebooting; don't accept the old firmware's immediate response.
    let deadline = Instant::now() + Duration::from_secs(90);
    thread::sleep(Duration::from_secs(2));
    let mut last = String::new();
    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        match device.request(
            "GET",
            "/status",
            &[],
            remaining.min(Duration::from_secs(30)),
        ) {
            Ok(bytes) => return response(&bytes),
            Err(e) => last = e.to_string(),
        }
        if let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            thread::sleep(remaining.min(Duration::from_secs(2)));
        }
    }
    Err(format!("device did not return within 90 seconds after OTA: {last}").into())
}
fn main() {
    let options = match Options::parse(env::args().skip(1).collect()) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("espctl: {e}\nRun `espctl help` for usage.");
            std::process::exit(2);
        }
    };
    if let Err(e) = run(options) {
        eprintln!("espctl: {e}");
        std::process::exit(1);
    }
}
