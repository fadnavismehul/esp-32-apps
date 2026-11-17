//! Reminders: when each one fires, and the record they are saved in.
//!
//! Times are local: Unix seconds shifted by the device's time zone, so
//! `minute % 1440` is minutes past local midnight. A "minute" here is local
//! minutes since 1970-01-01, which was a Thursday.
//!
//! Record: magic, version, count, next id, `MAX` fixed slots (id, kind, days,
//! minute, end, every, at, text length, text), then little-endian CRC-32.

pub const MAX: usize = 16;
pub const TEXT_MAX: usize = 60;
const SLOT: usize = 1 + 1 + 1 + 2 + 2 + 2 + 4 + 1 + TEXT_MAX;
pub const ENCODED_LEN: usize = 4 + 1 + 1 + 1 + MAX * SLOT + 4;
pub const DAILY: u8 = 0x7F;
const DAY_NAMES: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderError {
    BadText,
    BadSchedule,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    /// Once, at this local minute.
    Once { at: u32 },
    /// On `days` (bit 0 is Monday) at `minute` past midnight.
    Weekly { days: u8, minute: u16 },
    /// On `days`, every `every` minutes from `from` to `to` past midnight, inclusive.
    Interval { days: u8, from: u16, to: u16, every: u16 },
}

impl Schedule {
    fn valid(&self) -> bool {
        let days_ok = |d: u8| d != 0 && d <= DAILY;
        match *self {
            Schedule::Once { .. } => true,
            Schedule::Weekly { days, minute } => days_ok(days) && minute < 1440,
            Schedule::Interval { days, from, to, every } => {
                days_ok(days) && from <= to && to < 1440 && (1..=1440).contains(&every)
            }
        }
    }
}

/// Monday is 0.
pub fn weekday(minute: u32) -> u8 {
    ((minute / 1440 + 3) % 7) as u8
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reminder {
    pub id: u8,
    pub schedule: Schedule,
    text: [u8; TEXT_MAX],
    text_len: u8,
}

impl Reminder {
    const EMPTY: Self = Self {
        id: 0,
        schedule: Schedule::Once { at: 0 },
        text: [0; TEXT_MAX],
        text_len: 0,
    };

    pub fn new(id: u8, schedule: Schedule, text: &str) -> Result<Self, ReminderError> {
        let text = text.trim();
        // The device font is ASCII only.
        if text.is_empty() || text.len() > TEXT_MAX || !text.bytes().all(|b| (0x20..0x7F).contains(&b)) {
            return Err(ReminderError::BadText);
        }
        if !schedule.valid() {
            return Err(ReminderError::BadSchedule);
        }
        let mut r = Self { id, schedule, ..Self::EMPTY };
        r.text[..text.len()].copy_from_slice(text.as_bytes());
        r.text_len = text.len() as u8;
        Ok(r)
    }

    pub fn text(&self) -> &str {
        core::str::from_utf8(&self.text[..self.text_len as usize]).unwrap_or("")
    }

    /// Whether it fires at local minute `m`. A one-off fires at its minute or
    /// any time after, so one missed while the device was off still shows.
    pub fn due(&self, m: u32) -> bool {
        let on = |days: u8| days & (1 << weekday(m)) != 0;
        let t = (m % 1440) as u16;
        match self.schedule {
            Schedule::Once { at } => at <= m,
            Schedule::Weekly { days, minute } => on(days) && t == minute,
            Schedule::Interval { days, from, to, every } => {
                on(days) && (from..=to).contains(&t) && (t - from).is_multiple_of(every)
            }
        }
    }

    /// The first minute at or after `m` when it fires, looking a week ahead.
    pub fn next(&self, m: u32) -> Option<u32> {
        if let Schedule::Once { at } = self.schedule {
            return Some(at.max(m));
        }
        (0..8).find_map(|d| {
            let day = m / 1440 + d;
            let start = day * 1440;
            if !self.due_day(day) {
                return None;
            }
            let earliest = if d == 0 { (m - start) as u16 } else { 0 };
            let t = match self.schedule {
                Schedule::Weekly { minute, .. } => minute,
                Schedule::Interval { from, every, .. } if earliest > from => {
                    from + (earliest - from).div_ceil(every) * every
                }
                Schedule::Interval { from, .. } => from,
                Schedule::Once { .. } => unreachable!(),
            };
            let last = match self.schedule {
                Schedule::Interval { to, .. } => to,
                _ => t,
            };
            (t >= earliest && t <= last).then_some(start + t as u32)
        })
    }

    fn due_day(&self, day: u32) -> bool {
        match self.schedule {
            Schedule::Weekly { days, .. } | Schedule::Interval { days, .. } => {
                days & (1 << weekday(day * 1440)) != 0
            }
            Schedule::Once { .. } => true,
        }
    }
}

#[derive(Clone)]
pub struct Reminders {
    list: [Reminder; MAX],
    count: u8,
    next_id: u8,
}

impl Default for Reminders {
    fn default() -> Self {
        Self::new()
    }
}

impl Reminders {
    pub const fn new() -> Self {
        Self { list: [Reminder::EMPTY; MAX], count: 0, next_id: 1 }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Reminder> {
        self.list[..self.count as usize].iter()
    }

    pub fn get(&self, id: u8) -> Option<&Reminder> {
        self.iter().find(|r| r.id == id)
    }

    /// Add one and return its id. Ids count up and skip ones in use.
    pub fn add(&mut self, schedule: Schedule, text: &str) -> Result<u8, ReminderError> {
        if self.count as usize >= MAX {
            return Err(ReminderError::Full);
        }
        let mut id = self.next_id.max(1);
        while self.get(id).is_some() {
            id = id.checked_add(1).unwrap_or(1);
        }
        self.list[self.count as usize] = Reminder::new(id, schedule, text)?;
        self.count += 1;
        self.next_id = id.checked_add(1).unwrap_or(1);
        Ok(id)
    }

    pub fn remove(&mut self, id: u8) -> bool {
        let n = self.count as usize;
        let Some(i) = self.list[..n].iter().position(|r| r.id == id) else {
            return false;
        };
        self.list.copy_within(i + 1..n, i);
        self.list[n - 1] = Reminder::EMPTY;
        self.count -= 1;
        true
    }

    pub fn encode(&self, out: &mut [u8; ENCODED_LEN]) {
        out.fill(0);
        out[..4].copy_from_slice(b"GRM1");
        out[4] = 1;
        out[5] = self.count;
        out[6] = self.next_id;
        let mut p = 7;
        for r in &self.list {
            let (kind, days, minute, end, every, at) = match r.schedule {
                Schedule::Once { at } => (0, 0, 0, 0, 0, at),
                Schedule::Weekly { days, minute } => (1, days, minute, 0, 0, 0),
                Schedule::Interval { days, from, to, every } => (2, days, from, to, every, 0),
            };
            out[p] = r.id;
            out[p + 1] = kind;
            out[p + 2] = days;
            out[p + 3..p + 5].copy_from_slice(&minute.to_le_bytes());
            out[p + 5..p + 7].copy_from_slice(&end.to_le_bytes());
            out[p + 7..p + 9].copy_from_slice(&every.to_le_bytes());
            out[p + 9..p + 13].copy_from_slice(&at.to_le_bytes());
            out[p + 13] = r.text_len;
            out[p + 14..p + SLOT].copy_from_slice(&r.text);
            p += SLOT;
        }
        let crc = crate::config::crc32(&out[..ENCODED_LEN - 4]);
        out[ENCODED_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
    }

    pub fn decode(buf: &[u8]) -> Option<Self> {
        let b = buf.get(..ENCODED_LEN)?;
        if b.get(..4)? != b"GRM1"
            || b[4] != 1
            || b[5] as usize > MAX
            || crate::config::crc32(&b[..ENCODED_LEN - 4])
                != u32::from_le_bytes(b[ENCODED_LEN - 4..].try_into().ok()?)
        {
            return None;
        }
        let mut rs = Self::new();
        rs.next_id = b[6];
        let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        for i in 0..b[5] as usize {
            let p = 7 + i * SLOT;
            let schedule = match b[p + 1] {
                0 => Schedule::Once { at: u32::from_le_bytes(b[p + 9..p + 13].try_into().ok()?) },
                1 => Schedule::Weekly { days: b[p + 2], minute: u16_at(p + 3) },
                2 => Schedule::Interval {
                    days: b[p + 2],
                    from: u16_at(p + 3),
                    to: u16_at(p + 5),
                    every: u16_at(p + 7),
                },
                _ => return None,
            };
            let len = (b[p + 13] as usize).min(TEXT_MAX);
            let text = core::str::from_utf8(&b[p + 14..p + 14 + len]).ok()?;
            rs.list[i] = Reminder::new(b[p], schedule, text).ok()?;
        }
        rs.count = b[5];
        Some(rs)
    }
}

/// "HH:MM" to minutes past midnight.
pub fn parse_hhmm(s: &str) -> Option<u16> {
    let (h, m) = s.split_once(':')?;
    let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60 && s.len() <= 5).then_some(h * 60 + m)
}

/// "daily", "weekdays", "weekends", or names like "mon,wed,fri".
pub fn parse_days(s: &str) -> Option<u8> {
    match s {
        "daily" | "every" => return Some(DAILY),
        "weekdays" => return Some(0x1F),
        "weekends" => return Some(0x60),
        _ => {}
    }
    s.split(',').try_fold(0u8, |mask, name| {
        let i = DAY_NAMES.iter().position(|d| name.trim().eq_ignore_ascii_case(d))?;
        Some(mask | 1 << i)
    })
}

/// "YYYY-MM-DD" to days since 1970-01-01.
pub fn parse_date(s: &str) -> Option<u32> {
    let mut it = s.splitn(3, '-');
    let y: i64 = it.next()?.parse().ok()?;
    let m: u32 = it.next()?.parse().ok()?;
    let d: u32 = it.next()?.parse().ok()?;
    let month_len = [31, 28 + leap(y) as u32, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1970..=2100).contains(&y) || !(1..=12).contains(&m) || d == 0 || d > month_len[m as usize - 1] {
        return None;
    }
    Some(days_from_civil(y, m, d) as u32)
}

fn leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

/// Howard Hinnant's days_from_civil.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Days since 1970-01-01 to (year, month, day).
pub fn civil_from_days(days: u32) -> (u32, u32, u32) {
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (yoe + era * 400 + (m <= 2) as i64) as u32;
    (y, m, d)
}

/// Write `days` as "daily", "weekdays", "weekends" or "mon,wed".
pub fn format_days(days: u8, out: &mut impl core::fmt::Write) -> core::fmt::Result {
    match days {
        DAILY => return out.write_str("daily"),
        0x1F => return out.write_str("weekdays"),
        0x60 => return out.write_str("weekends"),
        _ => {}
    }
    let mut first = true;
    for (i, name) in DAY_NAMES.iter().enumerate() {
        if days & (1 << i) != 0 {
            if !first {
                out.write_char(',')?;
            }
            out.write_str(name)?;
            first = false;
        }
    }
    Ok(())
}

/// Write a local minute as "YYYY-MM-DD HH:MM".
pub fn format_minute(m: u32, out: &mut impl core::fmt::Write) -> core::fmt::Result {
    let (y, mo, d) = civil_from_days(m / 1440);
    let t = m % 1440;
    write!(out, "{y:04}-{mo:02}-{d:02} {:02}:{:02}", t / 60, t % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::string::String;

    /// Local minute for a date and time.
    fn at(date: &str, hhmm: &str) -> u32 {
        parse_date(date).unwrap() * 1440 + parse_hhmm(hhmm).unwrap() as u32
    }

    #[test]
    fn dates_and_weekdays() {
        assert_eq!(parse_date("1970-01-01"), Some(0));
        assert_eq!(weekday(0), 3, "1970-01-01 was a Thursday");
        // 2026-10-26 is a Monday.
        assert_eq!(weekday(at("2026-10-26", "00:00")), 0);
        assert_eq!(civil_from_days(parse_date("2024-02-29").unwrap()), (2024, 2, 29));
        assert_eq!(parse_date("2025-02-29"), None);
        assert_eq!(parse_date("2026-13-01"), None);
        let mut s = String::new();
        format_minute(at("2026-10-26", "18:05"), &mut s).unwrap();
        assert_eq!(s, "2026-10-26 18:05");
    }

    #[test]
    fn parsing() {
        assert_eq!(parse_hhmm("18:30"), Some(1110));
        assert_eq!(parse_hhmm("7:05"), Some(425));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(parse_days("daily"), Some(DAILY));
        assert_eq!(parse_days("weekdays"), Some(0x1F));
        assert_eq!(parse_days("Mon,wed,SUN"), Some(0b100_0101));
        assert_eq!(parse_days("mon,funday"), None);
        let mut s = String::new();
        format_days(0b100_0101, &mut s).unwrap();
        assert_eq!(s, "mon,wed,sun");
    }

    #[test]
    fn weekly_fires_on_its_days_only() {
        let r = Reminder::new(1, Schedule::Weekly { days: 0x1F, minute: 1110 }, "Meds").unwrap();
        assert!(r.due(at("2026-10-26", "18:30")));
        assert!(!r.due(at("2026-10-26", "18:31")));
        assert!(!r.due(at("2026-10-31", "18:30")), "Saturday");
        // Friday evening after it fired: next is Monday.
        assert_eq!(r.next(at("2026-10-30", "18:31")), Some(at("2026-11-02", "18:30")));
        assert_eq!(r.next(at("2026-10-30", "18:30")), Some(at("2026-10-30", "18:30")));
    }

    #[test]
    fn interval_steps_inside_its_window() {
        let s = Schedule::Interval { days: DAILY, from: 540, to: 1020, every: 90 };
        let r = Reminder::new(1, s, "Stand up").unwrap();
        assert!(r.due(at("2026-10-26", "09:00")));
        assert!(r.due(at("2026-10-26", "10:30")));
        assert!(!r.due(at("2026-10-26", "10:00")));
        assert!(r.due(at("2026-10-26", "16:30")));
        assert!(!r.due(at("2026-10-26", "18:00")), "past the window");
        assert_eq!(r.next(at("2026-10-26", "10:31")), Some(at("2026-10-26", "12:00")));
        assert_eq!(r.next(at("2026-10-26", "16:31")), Some(at("2026-10-27", "09:00")));
        assert_eq!(r.next(at("2026-10-26", "05:00")), Some(at("2026-10-26", "09:00")));
    }

    #[test]
    fn once_fires_late_rather_than_never() {
        let r = Reminder::new(1, Schedule::Once { at: at("2026-10-27", "09:00") }, "Call").unwrap();
        assert!(!r.due(at("2026-10-27", "08:59")));
        assert!(r.due(at("2026-10-27", "09:00")));
        assert!(r.due(at("2026-10-28", "12:00")));
    }

    #[test]
    fn validation() {
        let s = Schedule::Weekly { days: DAILY, minute: 0 };
        assert_eq!(Reminder::new(1, s, "  ").unwrap_err(), ReminderError::BadText);
        assert_eq!(Reminder::new(1, s, "caf\u{e9}").unwrap_err(), ReminderError::BadText);
        assert_eq!(Reminder::new(1, s, &"x".repeat(61)).unwrap_err(), ReminderError::BadText);
        let bad = Schedule::Weekly { days: 0, minute: 0 };
        assert_eq!(Reminder::new(1, bad, "x").unwrap_err(), ReminderError::BadSchedule);
        let bad = Schedule::Interval { days: 1, from: 600, to: 500, every: 10 };
        assert_eq!(Reminder::new(1, bad, "x").unwrap_err(), ReminderError::BadSchedule);
    }

    #[test]
    fn list_add_remove_and_round_trip() {
        let mut rs = Reminders::new();
        let a = rs.add(Schedule::Once { at: 99 }, "one").unwrap();
        let b = rs.add(Schedule::Weekly { days: 3, minute: 60 }, "two").unwrap();
        let c = rs.add(Schedule::Interval { days: DAILY, from: 0, to: 600, every: 30 }, "three").unwrap();
        assert_eq!((a, b, c), (1, 2, 3));
        assert!(rs.remove(b));
        assert!(!rs.remove(b));
        assert_eq!(rs.add(Schedule::Once { at: 5 }, "four").unwrap(), 4, "ids are not reused");
        let mut buf = [0; ENCODED_LEN];
        rs.encode(&mut buf);
        let back = Reminders::decode(&buf).unwrap();
        assert!(back.iter().eq(rs.iter()));
        assert_eq!(back.iter().map(|r| r.text()).collect::<std::vec::Vec<_>>(), ["one", "three", "four"]);
        buf[20] ^= 1;
        assert!(Reminders::decode(&buf).is_none());
        for _ in 0..MAX - 3 {
            rs.add(Schedule::Once { at: 1 }, "x").unwrap();
        }
        assert_eq!(rs.add(Schedule::Once { at: 1 }, "x"), Err(ReminderError::Full));
    }
}
