//! Time zone record: magic b"TZ", version 1, little-endian i16 minutes offset,
//! little-endian CRC-32 of the first 5 bytes. Unused bytes in the cfg sector are
//! left untouched. Missing or invalid records default to Singapore (+8:00, 480 min).

pub const AT: usize = 464; // byte offset in the cfg sector
pub const ENCODED_LEN: usize = 9; // b"TZ", version 1, i16 LE minutes, CRC-32 LE of the first 5 bytes
pub const DEFAULT_MIN: i16 = 480; // Singapore
pub const INDIA_MIN: i16 = 330;
pub const MIN_MIN: i16 = -720;
pub const MAX_MIN: i16 = 840;

pub fn clamp(min: i32) -> i16 {
    min.clamp(MIN_MIN as i32, MAX_MIN as i32) as i16
}

pub fn encode(offset_min: i16, out: &mut [u8; ENCODED_LEN]) {
    let offset_min = clamp(offset_min as i32);
    out[0..2].copy_from_slice(b"TZ");
    out[2] = 1;
    out[3..5].copy_from_slice(&offset_min.to_le_bytes());
    let crc = crate::config::crc32(&out[..5]);
    out[5..9].copy_from_slice(&crc.to_le_bytes());
}

pub fn decode(buf: &[u8]) -> i16 {
    let Some(b) = buf.get(..ENCODED_LEN) else {
        return DEFAULT_MIN;
    };
    if &b[0..2] != b"TZ" || b[2] != 1 {
        return DEFAULT_MIN;
    }
    let Ok(crc_bytes) = b[5..9].try_into() else {
        return DEFAULT_MIN;
    };
    let expected_crc = u32::from_le_bytes(crc_bytes);
    if crate::config::crc32(&b[..5]) != expected_crc {
        return DEFAULT_MIN;
    }
    let min = i16::from_le_bytes([b[3], b[4]]);
    if !(MIN_MIN..=MAX_MIN).contains(&min) {
        return DEFAULT_MIN;
    }
    min
}

pub fn parse(s: &str) -> Option<i16> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.eq_ignore_ascii_case("sgt") {
        return Some(DEFAULT_MIN);
    }
    if s.eq_ignore_ascii_case("ist") {
        return Some(INDIA_MIN);
    }

    let (negative, rest) = if let Some(stripped) = s.strip_prefix('-') {
        (true, stripped)
    } else if let Some(stripped) = s.strip_prefix('+') {
        (false, stripped)
    } else {
        (false, s)
    };

    if rest.is_empty() {
        return None;
    }

    let total_min = if let Some((h_str, m_str)) = rest.split_once(':') {
        if h_str.is_empty()
            || m_str.len() != 2
            || !m_str.bytes().all(|b| b.is_ascii_digit())
            || !h_str.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let hours: u32 = h_str.parse().ok()?;
        let mins: u32 = m_str.parse().ok()?;
        if mins >= 60 || hours > 24 {
            return None;
        }
        hours * 60 + mins
    } else {
        if !rest.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let hours: u32 = rest.parse().ok()?;
        if hours > 24 {
            return None;
        }
        hours * 60
    };

    let signed_min = if negative {
        -(total_min as i32)
    } else {
        total_min as i32
    };

    if !(MIN_MIN as i32..=MAX_MIN as i32).contains(&signed_min) {
        return None;
    }
    Some(signed_min as i16)
}

pub fn format(offset_min: i16, out: &mut impl core::fmt::Write) -> core::fmt::Result {
    let sign = if offset_min < 0 { '-' } else { '+' };
    let abs_min = offset_min.unsigned_abs();
    let hours = abs_min / 60;
    let mins = abs_min % 60;
    write!(out, "{sign}{hours}:{mins:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::String;

    #[test]
    fn partition_offsets_and_limits() {
        assert!(AT >= crate::config::ENCODED_LEN);
        assert!(AT + ENCODED_LEN <= 512);
    }

    #[test]
    fn round_trip() {
        for val in [DEFAULT_MIN, INDIA_MIN, -210, MIN_MIN, MAX_MIN] {
            let mut buf = [0u8; ENCODED_LEN];
            encode(val, &mut buf);
            assert_eq!(decode(&buf), val, "round trip for {val}");
        }
    }

    #[test]
    fn decode_invalid_fallback() {
        assert_eq!(decode(&[0xFF; ENCODED_LEN]), DEFAULT_MIN);
        assert_eq!(decode(&[0; ENCODED_LEN]), DEFAULT_MIN);

        let mut valid = [0u8; ENCODED_LEN];
        encode(INDIA_MIN, &mut valid);
        assert_eq!(decode(&valid), INDIA_MIN);

        // Flipped bit
        for i in 0..ENCODED_LEN {
            let mut bad = valid;
            bad[i] ^= 1;
            assert_eq!(decode(&bad), DEFAULT_MIN, "flipped bit at {i}");
        }

        // Short slice
        for len in 0..ENCODED_LEN {
            assert_eq!(decode(&valid[..len]), DEFAULT_MIN, "short slice of len {len}");
        }

        // Out-of-range value with valid CRC
        for out_val in [MIN_MIN - 1, MAX_MIN + 1, -1000, 1000] {
            let mut buf = [0u8; ENCODED_LEN];
            buf[0..2].copy_from_slice(b"TZ");
            buf[2] = 1;
            buf[3..5].copy_from_slice(&out_val.to_le_bytes());
            let crc = crate::config::crc32(&buf[..5]);
            buf[5..9].copy_from_slice(&crc.to_le_bytes());
            assert_eq!(decode(&buf), DEFAULT_MIN, "out of range {out_val}");
        }
    }

    #[test]
    fn parse_valid_and_rejects() {
        assert_eq!(parse("sgt"), Some(DEFAULT_MIN));
        assert_eq!(parse("SGT"), Some(DEFAULT_MIN));
        assert_eq!(parse("ist"), Some(INDIA_MIN));
        assert_eq!(parse("IST"), Some(INDIA_MIN));
        assert_eq!(parse("+5:30"), Some(330));
        assert_eq!(parse("+05:30"), Some(330));
        assert_eq!(parse("-3:00"), Some(-180));
        assert_eq!(parse("8"), Some(480));

        for bad in [
            "", "+25:00", "+5:60", "abc", "330m", "+", "-", "+:00", "+5:3", "25", "+15:00",
            "-13:00",
        ] {
            assert_eq!(parse(bad), None, "expected reject for {bad:?}");
        }
    }

    #[test]
    fn format_outputs() {
        let fmt = |min: i16| {
            let mut s = String::new();
            format(min, &mut s).unwrap();
            s
        };
        assert_eq!(fmt(480), "+8:00");
        assert_eq!(fmt(330), "+5:30");
        assert_eq!(fmt(-210), "-3:30");
        assert_eq!(fmt(0), "+0:00");
    }
}
