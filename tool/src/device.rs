#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rgb565_big_endian() {
        assert_eq!(
            rgb8(&[0xf8, 0, 0x07, 0xe0, 0, 0x1f, 0xff, 0xff, 0, 0]).unwrap(),
            [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0]
        );
        assert!(rgb8(&[0]).is_err());
    }
    #[test]
    fn rolling_log_tail() {
        assert_eq!(new_logs("", "abc\n"), "abc\n");
        assert_eq!(new_logs("abc\ndef\n", "abc\ndef\nghi\n"), "ghi\n");
        assert_eq!(new_logs("abc\ndef\n", "def\nghi\n"), "ghi\n");
        assert_eq!(new_logs("abc\n", "abc\n"), "");
        assert_eq!(new_logs("old\n", "reboot\n"), "reboot\n");
    }
}

use crate::config::Settings;
use std::{error::Error, fs::File, io::BufWriter, path::Path, time::Duration};
pub type Result<T> = std::result::Result<T, Box<dyn Error>>;
pub struct Device {
    pub settings: Settings,
}
impl Device {
    pub fn request(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        let host = self
            .settings
            .host
            .strip_prefix("http://")
            .unwrap_or(&self.settings.host)
            .trim_end_matches('/');
        if host.is_empty()
            || host.contains(['/', '?', '#', '@'])
            || host.chars().any(char::is_whitespace)
        {
            return Err("host must be a hostname or IP with optional port".into());
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .proxy(None)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(5).min(timeout)))
            .timeout_recv_response(Some(timeout))
            .timeout_recv_body(Some(timeout))
            .timeout_global(Some(timeout))
            .build()
            .into();
        let url = format!("http://{host}{path}");
        let mut response = if method == "GET" {
            let mut req = agent.get(&url).header("Connection", "close");
            if let Some(token) = &self.settings.token {
                req = req.header("Authorization", format!("Bearer {token}"));
            }
            req.call()?
        } else {
            let mut req = agent
                .post(&url)
                .header("Connection", "close")
                .header("Content-Length", body.len().to_string());
            if let Some(token) = &self.settings.token {
                req = req.header("Authorization", format!("Bearer {token}"));
            }
            if path == "/ota" {
                req = req.header("Content-Type", "application/octet-stream");
            }
            req.send(body)?
        };
        let status = response.status().as_u16();
        let bytes = response
            .body_mut()
            .with_config()
            .limit(16 * 1024 * 1024)
            .read_to_vec()?;
        if !(200..300).contains(&status) {
            // Do not include query strings: Wi-Fi passwords live there.
            return Err(format!(
                "HTTP {status} for {method} {}: {}",
                path.split('?').next().unwrap_or(path),
                String::from_utf8_lossy(&bytes).trim()
            )
            .into());
        }
        Ok(bytes)
    }
    pub fn get(&self, path: &str) -> Result<Vec<u8>> {
        self.request("GET", path, &[], Duration::from_secs(30))
    }
}
pub fn rgb8(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() % 2 != 0 {
        return Err("RGB565 data has an odd byte count".into());
    }
    let mut out = Vec::with_capacity(bytes.len() / 2 * 3);
    for pair in bytes.chunks_exact(2) {
        let c = u16::from_be_bytes([pair[0], pair[1]]);
        let r = ((c >> 11) & 31) as u8;
        let g = ((c >> 5) & 63) as u8;
        let b = (c & 31) as u8;
        out.extend_from_slice(&[
            (r << 3) | (r >> 2),
            (g << 2) | (g >> 4),
            (b << 3) | (b >> 2),
        ]);
    }
    Ok(out)
}
pub fn screenshot(path: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.len() != 480 * 480 * 2 {
        return Err(format!(
            "screenshot: expected 460800 bytes, received {}",
            bytes.len()
        )
        .into());
    }
    let rgb = rgb8(bytes)?;
    let mut encoder = png::Encoder::new(BufWriter::new(File::create(path)?), 480, 480);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&rgb)?;
    writer.finish()?;
    Ok(())
}
pub fn new_logs<'a>(previous: &str, current: &'a str) -> &'a str {
    if previous.is_empty() {
        return current;
    }
    // The device tail may have rolled over. Match its longest overlapping suffix.
    for (offset, _) in previous.char_indices() {
        let suffix = &previous[offset..];
        if current.starts_with(suffix) {
            return &current[suffix.len()..];
        }
    }
    current
}
