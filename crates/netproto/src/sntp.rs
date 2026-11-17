//! SNTP (RFC 4330) client packets: one request, one reply, seconds only.

pub const PORT: u16 = 123;
pub const LEN: usize = 48;
/// Seconds from 1900-01-01 (NTP era 0) to 1970-01-01.
const UNIX_OFFSET: u32 = 2_208_988_800;

/// A client request: version 4, mode 3, everything else zero.
pub fn request() -> [u8; LEN] {
    let mut p = [0u8; LEN];
    p[0] = (4 << 3) | 3;
    p
}

/// Unix seconds from a server reply, or `None` if it is not a usable
/// server reply (wrong mode, kiss-of-death, unsynchronised, or zero time).
pub fn parse_reply(p: &[u8]) -> Option<u32> {
    let p = p.get(..LEN)?;
    let (leap, mode, stratum) = (p[0] >> 6, p[0] & 7, p[1]);
    if mode != 4 || leap == 3 || stratum == 0 || stratum > 15 {
        return None;
    }
    let ntp = u32::from_be_bytes(p[40..44].try_into().ok()?);
    // Era 0 ends in 2036; after that NTP seconds wrap and are small.
    let unix = ntp.wrapping_sub(UNIX_OFFSET);
    (ntp != 0).then_some(unix)
}
