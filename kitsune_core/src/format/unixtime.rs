//! Calendar arithmetic shared by the TLS certificate checks, SNTP and the RTC
//! reader: proleptic-Gregorian civil date <-> days since 1970-01-01 <-> Unix
//! seconds. Pure integer math (Howard Hinnant's `days_from_civil` /
//! `civil_from_days`), no tables and no panics for any input.

/// A civil date and time of day (UTC unless the caller says otherwise).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateTime {
    pub year: i32,
    /// 1..=12
    pub month: u8,
    /// 1..=31
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

/// First instant the OS accepts as a plausible "now" (2024-01-01T00:00:00Z):
/// any clock source earlier than this is considered broken.
pub const MIN_PLAUSIBLE_UNIX: u64 = 1_704_067_200;
/// Last plausible instant (2100-01-01T00:00:00Z, exclusive).
pub const MAX_PLAUSIBLE_UNIX: u64 = 4_102_444_800;

/// Seconds between the NTP epoch (1900-01-01) and the Unix epoch (1970-01-01).
pub const NTP_UNIX_OFFSET: u64 = 2_208_988_800;

/// True for leap years.
pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days in `month` (1..=12) of `year`; 0 for an invalid month.
pub fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Days since 1970-01-01 of the civil date `y-m-d` (month 1..=12, day 1..=31).
pub fn days_from_civil(y: i32, m: u8, d: u8) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400); // [0, 399]
    let mp = (i64::from(m) + 9) % 12; // March = 0
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Civil date `(year, month, day)` of `days` since 1970-01-01.
pub fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8; // [1, 12]
    ((y + i64::from(m <= 2)) as i32, m, d)
}

impl DateTime {
    /// Validate the fields (real calendar day, `second` up to 59, leap seconds
    /// are rejected) and convert to Unix seconds. `None` for an impossible date
    /// or a time before 1970.
    pub fn to_unix(&self) -> Option<u64> {
        if !(1..=12).contains(&self.month)
            || self.day == 0
            || self.day > days_in_month(self.year, self.month)
            || self.hour > 23
            || self.minute > 59
            || self.second > 59
        {
            return None;
        }
        let days = days_from_civil(self.year, self.month, self.day);
        let secs = days
            .checked_mul(86_400)?
            .checked_add(i64::from(self.hour) * 3600 + i64::from(self.minute) * 60)?
            .checked_add(i64::from(self.second))?;
        u64::try_from(secs).ok()
    }

    /// Break `unix` seconds into a civil UTC date and time. Saturates far in
    /// the future (year > 9999 is clamped to 9999-12-31T23:59:59).
    pub fn from_unix(unix: u64) -> DateTime {
        const MAX: u64 = 253_402_300_799; // 9999-12-31T23:59:59Z
        let unix = unix.min(MAX);
        let days = (unix / 86_400) as i64;
        let rem = (unix % 86_400) as u32;
        let (year, month, day) = civil_from_days(days);
        DateTime {
            year,
            month,
            day,
            hour: (rem / 3600) as u8,
            minute: (rem % 3600 / 60) as u8,
            second: (rem % 60) as u8,
        }
    }

    /// `YYYY-MM-DD HH:MM:SS` into `out` (ASCII); returns the length (19).
    pub fn format(&self, out: &mut [u8; 19]) -> usize {
        fn put(out: &mut [u8], at: usize, v: u32, digits: usize) {
            let mut v = v;
            for i in (0..digits).rev() {
                out[at + i] = b'0' + (v % 10) as u8;
                v /= 10;
            }
        }
        put(out, 0, self.year.clamp(0, 9999) as u32, 4);
        out[4] = b'-';
        put(out, 5, u32::from(self.month), 2);
        out[7] = b'-';
        put(out, 8, u32::from(self.day), 2);
        out[10] = b' ';
        put(out, 11, u32::from(self.hour), 2);
        out[13] = b':';
        put(out, 14, u32::from(self.minute), 2);
        out[16] = b':';
        put(out, 17, u32::from(self.second), 2);
        19
    }
}

/// Is `unix` inside the window the OS trusts as a real "now"?
pub fn is_plausible(unix: u64) -> bool {
    (MIN_PLAUSIBLE_UNIX..MAX_PLAUSIBLE_UNIX).contains(&unix)
}

/// Convert CMOS RTC date registers (already binary, UTC) to Unix seconds.
/// `year` is the two-digit year register; `century` is the century register
/// (0 when the RTC has none: 20 is assumed). `None` for an impossible reading.
pub fn from_rtc_fields(
    second: u8,
    minute: u8,
    hour: u8,
    day: u8,
    month: u8,
    year: u8,
    century: u8,
) -> Option<u64> {
    let century = if (19..=21).contains(&century) {
        i32::from(century)
    } else {
        20
    };
    DateTime {
        year: century * 100 + i32::from(year),
        month,
        day,
        hour,
        minute,
        second,
    }
    .to_unix()
}

#[cfg(test)]
mod tests;
