use crate::dns::{name, u16_at};
pub fn reply(packet: &[u8], hostname: &str, ip: [u8; 4], out: &mut [u8]) -> Option<usize> {
    if u16_at(packet, 2)? & 0xf800 != 0 {
        return None;
    }
    let count = u16_at(packet, 4)?;
    packet.get(..12)?;
    let mut expected = [0; 255];
    let n = host(hostname, &mut expected)?;
    let mut pos = 12;
    let mut matched = false;
    for _ in 0..count {
        let mut decoded = [0; 255];
        let (end, len) = name(packet, pos, &mut decoded)?;
        let kind = u16_at(packet, end)?;
        let class = u16_at(packet, end + 2)?;
        if decoded[..len].eq_ignore_ascii_case(&expected[..n])
            && (kind == 1 || kind == 255)
            && class & 0x7fff == 1
        {
            matched = true
        }
        pos = end.checked_add(4)?;
    }
    if matched {
        announce(hostname, ip, out)
    } else {
        None
    }
}
fn host(hostname: &str, out: &mut [u8; 255]) -> Option<usize> {
    if hostname.is_empty()
        || hostname.len() > 63
        || !hostname
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return None;
    }
    let n = hostname.len();
    out[..n].copy_from_slice(hostname.as_bytes());
    out[n..n + 6].copy_from_slice(b".local");
    Some(n + 6)
}
pub fn announce(hostname: &str, ip: [u8; 4], out: &mut [u8]) -> Option<usize> {
    host(hostname, &mut [0; 255])?;
    let n = hostname.len();
    let len = 12 + n + 8 + 14;
    let dst = out.get_mut(..len)?;
    dst.fill(0);
    dst[2] = 0x84;
    dst[7] = 1;
    dst[12] = n as u8;
    dst[13..13 + n].copy_from_slice(hostname.as_bytes());
    dst[13 + n..20 + n].copy_from_slice(b"\x05local\0");
    dst[20 + n..30 + n].copy_from_slice(&[0, 1, 0x80, 1, 0, 0, 0, 120, 0, 4]);
    dst[30 + n..].copy_from_slice(&ip);
    Some(len)
}
