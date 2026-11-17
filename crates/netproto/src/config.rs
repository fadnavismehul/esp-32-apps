//! Fixed record: magic, version, count, four (length + SSID + length +
//! password) slots, token length + token, app length + app, volume, then in
//! version 2 flags (bit 0: Wi-Fi off) and display brightness, then
//! little-endian IEEE CRC-32. Unused bytes are zeroed. Version 1 records,
//! without the two extra bytes, still decode.
pub const MAX_NETWORKS: usize = 4;
pub const ENCODED_LEN: usize = 455;
const V1_LEN: usize = 453;
pub const DEFAULT_BRIGHTNESS: u8 = 0xD0;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    InvalidSsid,
    PasswordTooLong,
    InvalidApp,
}
#[derive(Clone, Copy)]
pub struct Network {
    ssid: [u8; 32],
    ssid_len: u8,
    password: [u8; 64],
    password_len: u8,
}
impl Network {
    const EMPTY: Self = Self {
        ssid: [0; 32],
        ssid_len: 0,
        password: [0; 64],
        password_len: 0,
    };
    pub fn ssid(&self) -> &str {
        core::str::from_utf8(&self.ssid[..self.ssid_len as usize]).unwrap_or("")
    }
    pub fn password(&self) -> &str {
        core::str::from_utf8(&self.password[..self.password_len as usize]).unwrap_or("")
    }
}
#[derive(Clone)]
pub struct Config {
    networks: [Network; 4],
    count: u8,
    token: [u8; 32],
    token_len: u8,
    app: [u8; 16],
    app_len: u8,
    volume: u8,
    wifi_off: bool,
    brightness: u8,
}
impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}
impl Config {
    pub const fn new() -> Self {
        Self {
            networks: [Network::EMPTY; 4],
            count: 0,
            token: [0; 32],
            token_len: 0,
            app: *b"glyphfield\0\0\0\0\0\0",
            app_len: 10,
            volume: 60,
            wifi_off: false,
            brightness: DEFAULT_BRIGHTNESS,
        }
    }
    pub fn networks(&self) -> impl Iterator<Item = &Network> {
        self.networks[..self.count as usize].iter()
    }
    pub fn add_network(&mut self, ssid: &str, password: &str) -> Result<(), ConfigError> {
        if ssid.is_empty() || ssid.len() > 32 {
            return Err(ConfigError::InvalidSsid);
        }
        if password.len() > 64 {
            return Err(ConfigError::PasswordTooLong);
        }
        let mut entry = Network::EMPTY;
        entry.ssid[..ssid.len()].copy_from_slice(ssid.as_bytes());
        entry.ssid_len = ssid.len() as u8;
        entry.password[..password.len()].copy_from_slice(password.as_bytes());
        entry.password_len = password.len() as u8;
        self.forget(ssid);
        let count = (self.count as usize).min(3);
        for i in (0..count).rev() {
            self.networks[i + 1] = self.networks[i]
        }
        self.networks[0] = entry;
        self.count = (count + 1) as u8;
        Ok(())
    }
    pub fn forget(&mut self, ssid: &str) -> bool {
        let Some(i) = self.networks().position(|n| n.ssid() == ssid) else {
            return false;
        };
        let n = self.count as usize;
        for j in i + 1..n {
            self.networks[j - 1] = self.networks[j]
        }
        self.networks[n - 1] = Network::EMPTY;
        self.count -= 1;
        true
    }
    pub fn token(&self) -> &str {
        core::str::from_utf8(&self.token[..self.token_len as usize]).unwrap_or("")
    }
    pub fn set_token_from_random(&mut self, random: [u8; 16]) {
        for (i, b) in random.iter().enumerate() {
            self.token[i * 2] = b"0123456789abcdef"[(b >> 4) as usize];
            self.token[i * 2 + 1] = b"0123456789abcdef"[(b & 15) as usize]
        }
        self.token_len = 32;
    }
    pub fn app(&self) -> &str {
        core::str::from_utf8(&self.app[..self.app_len as usize]).unwrap_or("")
    }
    pub fn set_app(&mut self, name: &str) -> Result<(), ConfigError> {
        if name.is_empty() || name.len() > 16 {
            return Err(ConfigError::InvalidApp);
        }
        self.app.fill(0);
        self.app[..name.len()].copy_from_slice(name.as_bytes());
        self.app_len = name.len() as u8;
        Ok(())
    }
    pub fn volume(&self) -> u8 {
        self.volume
    }
    pub fn set_volume(&mut self, v: u8) {
        self.volume = v.min(100)
    }
    pub fn wifi_off(&self) -> bool {
        self.wifi_off
    }
    pub fn set_wifi_off(&mut self, off: bool) {
        self.wifi_off = off
    }
    /// Panel brightness, 0..=255.
    pub fn brightness(&self) -> u8 {
        self.brightness
    }
    pub fn set_brightness(&mut self, b: u8) {
        self.brightness = b
    }
    pub fn encode(&self, out: &mut [u8; ENCODED_LEN]) {
        out.fill(0);
        out[..4].copy_from_slice(b"GCF1");
        out[4] = 2;
        out[5] = self.count;
        let mut p = 6;
        for n in &self.networks {
            out[p] = n.ssid_len;
            out[p + 1..p + 33].copy_from_slice(&n.ssid);
            out[p + 33] = n.password_len;
            out[p + 34..p + 98].copy_from_slice(&n.password);
            p += 98;
        }
        out[p] = self.token_len;
        out[p + 1..p + 33].copy_from_slice(&self.token);
        p += 33;
        out[p] = self.app_len;
        out[p + 1..p + 17].copy_from_slice(&self.app);
        out[p + 17] = self.volume;
        out[p + 18] = self.wifi_off as u8;
        out[p + 19] = self.brightness;
        let crc = crc32(&out[..ENCODED_LEN - 4]);
        out[ENCODED_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
    }
    pub fn decode(buf: &[u8]) -> Option<Self> {
        let len = match buf.get(4)? {
            1 => V1_LEN,
            2 => ENCODED_LEN,
            _ => return None,
        };
        let b = buf.get(..len)?;
        if b.get(..4)? != b"GCF1"
            || b[5] > 4
            || crc32(&b[..len - 4]) != u32::from_le_bytes(b[len - 4..].try_into().ok()?)
        {
            return None;
        }
        let mut c = Self::new();
        c.count = b[5];
        let mut p = 6;
        for (i, n) in c.networks.iter_mut().enumerate() {
            let sl = b[p] as usize;
            let pl = b[p + 33] as usize;
            if sl > 32 || pl > 64 || (i < c.count as usize && sl == 0) {
                return None;
            }
            core::str::from_utf8(&b[p + 1..p + 1 + sl]).ok()?;
            core::str::from_utf8(&b[p + 34..p + 34 + pl]).ok()?;
            n.ssid_len = sl as u8;
            n.password_len = pl as u8;
            n.ssid.copy_from_slice(&b[p + 1..p + 33]);
            n.password.copy_from_slice(&b[p + 34..p + 98]);
            p += 98;
        }
        let tl = b[p];
        if tl != 0 && tl != 32 {
            return None;
        }
        c.token_len = tl;
        c.token.copy_from_slice(&b[p + 1..p + 33]);
        if !c.token[..tl as usize]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
        {
            return None;
        }
        p += 33;
        let al = b[p] as usize;
        if al == 0 || al > 16 || b[p + 17] > 100 {
            return None;
        }
        core::str::from_utf8(&b[p + 1..p + 1 + al]).ok()?;
        c.app.copy_from_slice(&b[p + 1..p + 17]);
        c.app_len = al as u8;
        c.volume = b[p + 17];
        if len == ENCODED_LEN {
            if b[p + 18] > 1 {
                return None;
            }
            c.wifi_off = b[p + 18] == 1;
            c.brightness = b[p + 19];
        }
        Some(c)
    }
}
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for b in bytes {
        crc ^= *b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb88320u32.wrapping_mul(crc & 1));
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn crc_reference_and_invalid_fields() {
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
        let mut valid = [0; ENCODED_LEN];
        Config::new().encode(&mut valid);
        // Recompute CRC so these exercise validation rather than corruption detection.
        for (offset, value) in [
            (4, 3),
            (5, 5),
            (6, 33),
            (39, 65),
            (398, 31),
            (431, 17),
            (448, 101),
            (449, 2),
        ] {
            let mut b = valid;
            b[offset] = value;
            let crc = crc32(&b[..ENCODED_LEN - 4]);
            b[ENCODED_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
            assert!(Config::decode(&b).is_none(), "offset {offset}");
        }
    }

    #[test]
    fn version_1_still_decodes() {
        let mut c = Config::new();
        c.set_token_from_random([3; 16]);
        c.add_network("home", "pw").unwrap();
        c.set_volume(40);
        let mut v2 = [0; ENCODED_LEN];
        c.encode(&mut v2);
        // Rebuild the same record as version 1: no flags or brightness.
        let mut v1 = [0u8; V1_LEN];
        v1[..V1_LEN - 4].copy_from_slice(&v2[..V1_LEN - 4]);
        v1[4] = 1;
        let crc = crc32(&v1[..V1_LEN - 4]);
        v1[V1_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
        let d = Config::decode(&v1).unwrap();
        assert_eq!(d.token(), c.token());
        assert_eq!(d.networks().next().unwrap().ssid(), "home");
        assert_eq!(d.volume(), 40);
        assert!(!d.wifi_off());
        assert_eq!(d.brightness(), DEFAULT_BRIGHTNESS);
    }

    #[test]
    fn version_2_round_trip() {
        let mut c = Config::new();
        c.set_wifi_off(true);
        c.set_brightness(90);
        let mut b = [0; ENCODED_LEN];
        c.encode(&mut b);
        let d = Config::decode(&b).unwrap();
        assert!(d.wifi_off());
        assert_eq!(d.brightness(), 90);
    }
}
