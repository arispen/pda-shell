//! Calendar dates and wall-clock times without a time zone library.
//! Days are counted from 1970-01-01 (proleptic Gregorian calendar).

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

/// A local wall-clock time on a date.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    pub date: Date,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

impl Date {
    pub fn new(year: i32, month: u32, day: u32) -> Option<Self> {
        (1..=12).contains(&month).then_some(())?;
        (1..=days_in_month(year, month)).contains(&day).then_some(Self { year, month, day })
    }

    /// Days since 1970-01-01 (H. Hinnant's days_from_civil).
    pub fn days(self) -> i64 {
        let y = self.year as i64 - (self.month <= 2) as i64;
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = self.month as i64;
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + self.day as i64 - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    pub fn from_days(z: i64) -> Self {
        let z = z + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + (month <= 2) as i64) as i32;
        Self { year, month, day }
    }

    pub fn add_days(self, n: i64) -> Self {
        Self::from_days(self.days() + n)
    }

    /// 0 = Monday ... 6 = Sunday.
    pub fn weekday(self) -> u32 {
        // 1970-01-01 was a Thursday.
        (self.days() + 3).rem_euclid(7) as u32
    }

    pub fn at(self, hour: u32, minute: u32) -> DateTime {
        DateTime { date: self, hour, minute, second: 0 }
    }

    /// "Tue 27 Oct 2026".
    pub fn long(self) -> String {
        format!("{} {} {} {}", WEEKDAYS[self.weekday() as usize], self.day, MONTHS[self.month as usize - 1], self.year)
    }

    /// "27 Oct".
    pub fn short(self) -> String {
        format!("{} {}", self.day, MONTHS[self.month as usize - 1])
    }

    /// "20261027".
    pub fn ical(self) -> String {
        format!("{:04}{:02}{:02}", self.year, self.month, self.day)
    }

    pub fn parse_ical(s: &str) -> Option<Self> {
        if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Self::new(s[..4].parse().ok()?, s[4..6].parse().ok()?, s[6..8].parse().ok()?)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl DateTime {
    /// Seconds since 1970-01-01 00:00 of the same clock (not UTC).
    pub fn seconds(self) -> i64 {
        self.date.days() * 86_400 + (self.hour * 3600 + self.minute * 60 + self.second) as i64
    }

    pub fn from_seconds(s: i64) -> Self {
        let date = Date::from_days(s.div_euclid(86_400));
        let r = s.rem_euclid(86_400) as u32;
        Self { date, hour: r / 3600, minute: r / 60 % 60, second: r % 60 }
    }

    pub fn add_minutes(self, m: i64) -> Self {
        Self::from_seconds(self.seconds() + m * 60)
    }

    /// "14:05".
    pub fn hm(self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// "20261027T140500".
    pub fn ical(self) -> String {
        format!("{}T{:02}{:02}{:02}", self.date.ical(), self.hour, self.minute, self.second)
    }

    /// "20261027T140500", with or without a final Z (left to the caller).
    pub fn parse_ical(s: &str) -> Option<Self> {
        let (d, t) = s.split_once('T')?;
        let t = t.strip_suffix('Z').unwrap_or(t);
        if t.len() != 6 || !t.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let (hour, minute, second): (u32, u32, u32) = (t[..2].parse().ok()?, t[2..4].parse().ok()?, t[4..6].parse().ok()?);
        // 60: a leap second.
        if hour > 23 || minute > 59 || second > 60 {
            return None;
        }
        Some(Self { date: Date::parse_ical(d)?, hour, minute, second: second.min(59) })
    }
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.date, self.hm())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_numbers_round_trip() {
        assert_eq!(Date::new(1970, 1, 1).unwrap().days(), 0);
        assert_eq!(Date::new(2000, 3, 1).unwrap().days(), 11_017);
        for z in [-800_000, -1, 0, 59, 10_000, 20_753, 2_000_000] {
            assert_eq!(Date::from_days(z).days(), z);
        }
    }

    #[test]
    fn calendar_facts() {
        let d = Date::new(2026, 10, 27).unwrap();
        assert_eq!(d.weekday(), 1);
        assert_eq!(d.long(), "Tue 27 Oct 2026");
        assert_eq!(Date::new(2024, 2, 28).unwrap().add_days(1), Date::new(2024, 2, 29).unwrap());
        assert_eq!(Date::new(2026, 12, 31).unwrap().add_days(1), Date::new(2027, 1, 1).unwrap());
        assert!(Date::new(2026, 2, 29).is_none());
        assert!(Date::new(2026, 13, 1).is_none());
        assert!(is_leap(2000) && !is_leap(1900));
    }

    #[test]
    fn ical_forms() {
        let t = DateTime::parse_ical("20261027T235900Z").unwrap();
        assert_eq!(t.ical(), "20261027T235900");
        assert_eq!(t.add_minutes(2).to_string(), "2026-10-28 00:01");
        assert!(DateTime::parse_ical("20261027T2459").is_none());
        assert!(Date::parse_ical("2026-10-27").is_none());
    }
}
