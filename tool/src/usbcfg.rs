//! Write Wi-Fi networks from `.env` straight into the device's `cfg`
//! partition over USB, so no phone or setup hotspot is needed. The partition
//! is not touched by `flash`, so networks saved once survive reflashing.

use crate::device::Result;
use netproto::config::{Config, ENCODED_LEN};
use std::{collections::HashMap, fs, path::Path, process::Command};

const SECTOR: usize = 0x1000;

/// `WIFI_SSID`/`WIFI_PASS`, then `WIFI_SSID_2`/`WIFI_PASS_2` up to `_4`.
/// A missing password means an open network.
pub fn networks_from_env(env: &HashMap<String, String>) -> Vec<(String, String)> {
    (1..=netproto::config::MAX_NETWORKS)
        .filter_map(|i| {
            let suffix = if i == 1 { String::new() } else { format!("_{i}") };
            let ssid = env.get(&format!("WIFI_SSID{suffix}"))?.clone();
            let pass = env.get(&format!("WIFI_PASS{suffix}")).cloned().unwrap_or_default();
            Some((unquote(ssid), unquote(pass)))
        })
        .collect()
}

fn unquote(s: String) -> String {
    match s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        Some(inner) => inner.to_owned(),
        None => s,
    }
}

/// The settings record with `networks` added, or `None` when every network is
/// already saved with the same password. A blank or corrupt record starts
/// from defaults (the device then makes a new token, so pair again).
pub fn merge(current: &[u8], networks: &[(String, String)]) -> Result<Option<[u8; ENCODED_LEN]>> {
    let mut cfg = Config::decode(current).unwrap_or_else(Config::new);
    let unchanged = networks.iter().all(|(ssid, pass)| {
        cfg.networks().any(|n| n.ssid() == ssid && n.password() == pass)
    });
    if unchanged && Config::decode(current).is_some() {
        return Ok(None);
    }
    // Added in reverse so the first `.env` network ends up first.
    for (ssid, pass) in networks.iter().rev() {
        cfg.add_network(ssid, pass)
            .map_err(|e| format!("Wi-Fi network '{ssid}' rejected: {e:?}"))?;
    }
    let mut out = [0u8; ENCODED_LEN];
    cfg.encode(&mut out);
    Ok(Some(out))
}

pub fn cfg_offset(partitions_csv: &Path) -> Result<u32> {
    let text = fs::read_to_string(partitions_csv)?;
    let line = text
        .lines()
        .find(|l| l.split(',').next().map(str::trim) == Some("cfg"))
        .ok_or("no cfg partition in partitions.csv")?;
    let offset = line.split(',').nth(3).ok_or("cfg line has no offset")?.trim();
    Ok(u32::from_str_radix(offset.trim_start_matches("0x"), 16)?)
}

fn espflash(port: Option<&str>, args: &[&str]) -> Result<()> {
    let mut cmd = Command::new("espflash");
    cmd.args(args).args(["--chip", "esp32c6", "--non-interactive"]);
    if let Some(port) = port {
        cmd.args(["--port", port]);
    }
    let out = cmd.output()?;
    if !out.status.success() {
        return Err(format!(
            "espflash {} failed: {}",
            args[0],
            String::from_utf8_lossy(&out.stderr).trim()
        )
        .into());
    }
    Ok(())
}

/// Returns the SSIDs written, or an empty list when nothing changed.
pub fn sync(root: &Path, port: Option<&str>, networks: &[(String, String)]) -> Result<Vec<String>> {
    let offset = format!("{:#x}", cfg_offset(&root.join("firmware/partitions.csv"))?);
    let dir = root.join("target/espctl");
    fs::create_dir_all(&dir)?;
    let file = dir.join("cfg.bin");
    let file_str = file.to_str().ok_or("non-UTF-8 path")?;

    espflash(port, &["read-flash", &offset, &format!("{SECTOR:#x}"), file_str])?;
    let current = fs::read(&file)?;
    let merged = merge(&current, networks)?;
    let result = match merged {
        None => Ok(Vec::new()),
        Some(record) => {
            // Keep the rest of the sector: the reminders record lives there too.
            let mut sector = current.clone();
            sector.resize(SECTOR, 0xFF);
            sector[..ENCODED_LEN].copy_from_slice(&record);
            fs::write(&file, &sector)?;
            espflash(port, &["write-bin", &offset, file_str])
                .map(|_| networks.iter().map(|(s, _)| s.clone()).collect())
        }
    };
    // The file holds passwords; do not leave it around.
    let _ = fs::remove_file(&file);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_networks_and_merge() {
        let env: HashMap<String, String> = [
            ("WIFI_SSID", "home"),
            ("WIFI_PASS", "\"p=w d\""),
            ("WIFI_SSID_3", "cafe"),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect();
        let nets = networks_from_env(&env);
        assert_eq!(nets, [("home".into(), "p=w d".into()), ("cafe".into(), String::new())]);

        let mut start = Config::new();
        start.set_token_from_random([7; 16]);
        start.add_network("old", "x").unwrap();
        let mut bytes = [0u8; ENCODED_LEN];
        start.encode(&mut bytes);

        let merged = merge(&bytes, &nets).unwrap().unwrap();
        let cfg = Config::decode(&merged).unwrap();
        let names: Vec<&str> = cfg.networks().map(|n| n.ssid()).collect();
        assert_eq!(names, ["home", "cafe", "old"]);
        assert_eq!(cfg.token(), start.token());
        assert!(merge(&merged, &nets).unwrap().is_none());

        // Erased flash starts from defaults.
        let fresh = merge(&[0xFF; SECTOR], &nets).unwrap().unwrap();
        assert_eq!(Config::decode(&fresh).unwrap().networks().count(), 2);
    }

    #[test]
    fn finds_cfg_offset() {
        let root = crate::config::root_from(&std::env::current_dir().unwrap());
        assert_eq!(cfg_offset(&root.join("firmware/partitions.csv")).unwrap(), 0x820000);
    }

    #[test]
    fn merge_preserves_tz_record() {
        let mut sector = [0xFFu8; SECTOR];
        let mut tz_bytes = [0u8; netproto::tz::ENCODED_LEN];
        netproto::tz::encode(netproto::tz::INDIA_MIN, &mut tz_bytes);
        sector[netproto::tz::AT..netproto::tz::AT + netproto::tz::ENCODED_LEN]
            .copy_from_slice(&tz_bytes);

        let nets = vec![("newnet".into(), "pass".into())];
        let merged = merge(&sector, &nets).unwrap().unwrap();

        let mut new_sector = sector;
        new_sector[..ENCODED_LEN].copy_from_slice(&merged);

        let decoded_tz = netproto::tz::decode(
            &new_sector[netproto::tz::AT..netproto::tz::AT + netproto::tz::ENCODED_LEN],
        );
        assert_eq!(decoded_tz, netproto::tz::INDIA_MIN);
    }
}
