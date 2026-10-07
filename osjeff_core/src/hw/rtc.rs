//! CMOS RTC register decoding: BCD, 12h/24h and time-zone shift.

/// Wall-clock time as read from the RTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Time {
    pub h: u8,
    pub m: u8,
    pub s: u8,
}

/// Status register B: bit 2 set = binary values (clear = BCD).
const REGB_BINARY: u8 = 0x04;
/// Status register B: bit 1 set = 24-hour mode (clear = 12-hour with PM flag).
const REGB_24H: u8 = 0x02;

/// Converts one packed-BCD byte to binary.
pub fn bcd_to_bin(v: u8) -> u8 {
    (v & 0x0F) + (v >> 4) * 10
}

/// Decodes raw seconds/minutes/hours registers per status register B and
/// applies `tz_offset_hours` (UTC -> local), wrapping the hour into `0..24`.
///
/// In 12-hour mode the PM flag lives in bit 7 of the hours register.
pub fn decode(raw_s: u8, raw_m: u8, raw_h: u8, regb: u8, tz_offset_hours: i32) -> Time {
    let (mut s, mut m, mut h) = (raw_s, raw_m, raw_h);

    if regb & REGB_BINARY == 0 {
        s = bcd_to_bin(s);
        m = bcd_to_bin(m);
        let pm = h & 0x80 != 0; // 12h mode keeps PM flag in high bit
        h = ((h & 0x0F) + ((h & 0x70) >> 4) * 10) | (if pm { 0x80 } else { 0 });
    }

    // Convert 12h -> 24h when the RTC is in 12h mode.
    if regb & REGB_24H == 0 {
        let pm = h & 0x80 != 0;
        h &= 0x7F;
        if pm && h != 12 {
            h += 12;
        } else if !pm && h == 12 {
            h = 0;
        }
    }

    let local = (h as i32 + tz_offset_hours).rem_euclid(24) as u8;
    Time { h: local, m, s }
}

/// Days from 1970-01-01 to the given proleptic Gregorian date (negative before
/// the epoch). `month` is 1..=12, `day` 1..=31 (not checked against the month).
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    // Howard Hinnant's algorithm: shift the year to start in March so the leap
    // day is the last day of the (shifted) year.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400); // [0, 399]
    let mp = (month as i64 + 9) % 12; // March = 0
    let doy = (153 * mp + 2) / 5 + day as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Seconds since the Unix epoch for a UTC calendar date and time. `None` when a
/// field is out of range (month 1..=12, a day that exists in that month,
/// hour < 24, minute/second < 60) or the instant is before 1970.
pub fn unix_time(year: u32, month: u8, day: u8, h: u8, m: u8, s: u8) -> Option<u64> {
    let (month, day) = (month as u32, day as u32);
    let leap = (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
    let dim = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {

// ------------------------------------------------------------ date and time

/// A calendar date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Date {
    pub y: u16,
    pub m: u8,
    pub d: u8,
}

/// A calendar date and a time of day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DateTime {
    pub date: Date,
    pub time: Time,
}

pub const fn is_leap(y: u16) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

/// Days in month `m` (1..=12) of year `y`; 0 for an invalid month.
pub const fn days_in_month(y: u16, m: u8) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
        _ => return None,
    };
    if day == 0 || day > dim || h >= 24 || m >= 60 || s >= 60 {
        return None;
    }
    let days = days_from_civil(year as i64, month, day);
    if days < 0 {
        return None;
    }
    Some(days as u64 * 86_400 + h as u64 * 3600 + m as u64 * 60 + s as u64)
}

/// Raw CMOS registers describing the current date and time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RawDateTime {

        _ => 0,
    }
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date `days` days after 1970-01-01.
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

impl DateTime {
    /// Earliest and latest year the editor accepts.
    pub const MIN_YEAR: u16 = 1970;
    pub const MAX_YEAR: u16 = 2099;

    /// Is every field in range (a real calendar day, 00:00:00..=23:59:59,
    /// year in [`MIN_YEAR`](Self::MIN_YEAR)..=[`MAX_YEAR`](Self::MAX_YEAR))?
    pub fn is_valid(&self) -> bool {
        let Date { y, m, d } = self.date;
        (Self::MIN_YEAR..=Self::MAX_YEAR).contains(&y)
            && (1..=12).contains(&m)
            && d >= 1
            && d <= days_in_month(y, m)
            && self.time.h < 24
            && self.time.m < 60
            && self.time.s < 60
    }

    /// Seconds since 1970-01-01 00:00:00 (the fields are taken as they are).
    pub fn to_epoch(&self) -> i64 {
        let days = days_from_civil(self.date.y as i64, self.date.m as i64, self.date.d as i64);
        days * 86_400 + self.time.h as i64 * 3600 + self.time.m as i64 * 60 + self.time.s as i64
    }

    pub fn from_epoch(secs: i64) -> DateTime {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400);
        let (y, m, d) = civil_from_days(days);
        DateTime {
            date: Date {
                y: y.clamp(0, u16::MAX as i64) as u16,
                m: m as u8,
                d: d as u8,
            },
            time: Time {
                h: (rem / 3600) as u8,
                m: (rem / 60 % 60) as u8,
                s: (rem % 60) as u8,
            },
        }
    }

    /// This instant moved by `minutes` (negative = earlier), carrying into the date.
    pub fn shifted(&self, minutes: i32) -> DateTime {
        DateTime::from_epoch(self.to_epoch() + minutes as i64 * 60)
    }

    /// Day of the week, 0 = Sunday.
    pub fn weekday(&self) -> u8 {
        let days = days_from_civil(self.date.y as i64, self.date.m as i64, self.date.d as i64);
        (days + 4).rem_euclid(7) as u8
    }
}

/// A field of [`DateTime`] the clock editor steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Day,
    Month,
    Year,
    Hour,
    Minute,
    Second,
}

/// Wrap `v` into `lo..=hi` (so stepping past the end comes back at the start).
fn wrap_range(v: i32, lo: i32, hi: i32) -> i32 {
    lo + (v - lo).rem_euclid(hi - lo + 1)
}

impl DateTime {
    /// Step one field by `delta` (positive or negative), wrapping inside the
    /// field's range, then pull the day back into the month if the new month
    /// or year is shorter (31 Jan + 1 month = 28/29 Feb). The result is always
    /// valid when `self` was.
    pub fn step(&self, f: Field, delta: i32) -> DateTime {
        let mut d = *self;
        match f {
            Field::Year => {
                d.date.y = wrap_range(
                    d.date.y as i32 + delta,
                    Self::MIN_YEAR as i32,
                    Self::MAX_YEAR as i32,
                ) as u16
            }
            Field::Month => d.date.m = wrap_range(d.date.m as i32 + delta, 1, 12) as u8,
            Field::Day => {
                let dim = days_in_month(d.date.y, d.date.m).max(1) as i32;
                d.date.d = wrap_range(d.date.d as i32 + delta, 1, dim) as u8;
            }
            Field::Hour => d.time.h = wrap_range(d.time.h as i32 + delta, 0, 23) as u8,
            Field::Minute => d.time.m = wrap_range(d.time.m as i32 + delta, 0, 59) as u8,
            Field::Second => d.time.s = wrap_range(d.time.s as i32 + delta, 0, 59) as u8,
        }
        let dim = days_in_month(d.date.y, d.date.m).max(1);
        d.date.d = d.date.d.clamp(1, dim);
        d
    }
}

/// Time-zone offsets are minutes east of UTC, within UTC-12:00..=UTC+14:00.
pub const TZ_MIN: i32 = -12 * 60;
pub const TZ_MAX: i32 = 14 * 60;

/// UTC (what the CMOS RTC holds) to local time.
pub fn utc_to_local(utc: DateTime, tz_minutes: i32) -> DateTime {
    utc.shifted(tz_minutes.clamp(TZ_MIN, TZ_MAX))
}

/// Local time (what the user types) to UTC for the CMOS RTC.
pub fn local_to_utc(local: DateTime, tz_minutes: i32) -> DateTime {
    local.shifted(-tz_minutes.clamp(TZ_MIN, TZ_MAX))
}

/// The raw CMOS registers that make up the date and time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RawRtc {
    pub sec: u8,
    pub min: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    /// Two-digit year register (0x09).
    pub year: u8,
    /// Century register (0x32), or 0 if the platform has none.
    pub century: u8,
}

/// Decode raw CMOS registers into Unix seconds, **UTC** (no time-zone shift; the
/// RTC is kept in UTC, [`decode`]'s offset is a display concern). `regb` is
/// status register B (BCD vs binary, 12h vs 24h). A missing or implausible
/// century register falls back to 20xx. `None` for an impossible date.
pub fn decode_unix(raw: RawDateTime, regb: u8) -> Option<u64> {
    let t = decode(raw.sec, raw.min, raw.hour, regb, 0);
    let bcd = regb & REGB_BINARY == 0;
    let conv = |v: u8| if bcd { bcd_to_bin(v) } else { v };
    let yy = conv(raw.year) as u32;
    let cc = conv(raw.century) as u32;
    // Plausible centuries only (19..=39); anything else (0 = absent, garbage) means 20xx.
    let century = if (19..=39).contains(&cc) { cc } else { 20 };
    unix_time(
        century * 100 + yy,
        conv(raw.month),
        conv(raw.day),
        t.h,
        t.m,
        t.s,
    )

    pub year: u8,
    /// Register 0x32; some firmware leaves it 0.
    pub century: u8,
}

fn from_reg(v: u8, regb: u8) -> u8 {
    if regb & REGB_BINARY == 0 {
        bcd_to_bin(v)
    } else {
        v
    }
}

fn to_reg(v: u8, regb: u8) -> u8 {
    if regb & REGB_BINARY == 0 {
        (v / 10) << 4 | (v % 10)
    } else {
        v
    }
}

/// Decode the date and time registers (UTC) honouring BCD / binary and
/// 12 / 24-hour mode. A missing or implausible century reads as 20.
pub fn decode_datetime(raw: RawRtc, regb: u8) -> DateTime {
    let t = decode(raw.sec, raw.min, raw.hour, regb, 0);
    let c = from_reg(raw.century, regb);
    let century = if (19..=21).contains(&c) { c as u16 } else { 20 };
    DateTime {
        date: Date {
            y: century * 100 + from_reg(raw.year, regb) as u16 % 100,
            m: from_reg(raw.month, regb).clamp(1, 12),
            d: from_reg(raw.day, regb).clamp(1, 31),
        },
        time: t,
    }
}

/// Encode `dt` (UTC) into the registers for the RTC mode `regb`. The hour uses
/// the 12-hour form with the PM flag in bit 7 when the RTC is in that mode.
pub fn encode_datetime(dt: &DateTime, regb: u8) -> RawRtc {
    let h = dt.time.h % 24;
    let hour = if regb & REGB_24H != 0 {
        to_reg(h, regb)
    } else {
        let pm = h >= 12;
        let h12 = match h % 12 {
            0 => 12,
            n => n,
        };
        to_reg(h12, regb) | if pm { 0x80 } else { 0 }
    };
    RawRtc {
        sec: to_reg(dt.time.s % 60, regb),
        min: to_reg(dt.time.m % 60, regb),
        hour,
        day: to_reg(dt.date.d, regb),
        month: to_reg(dt.date.m, regb),
        year: to_reg((dt.date.y % 100) as u8, regb),
        century: to_reg((dt.date.y / 100) as u8, regb),
    }
}

/// `t` moved by `minutes` within the day (wrapping past midnight either way).
/// The hot per-frame clock read uses this instead of the full [`DateTime`]
/// conversion: it only needs the time of day.
pub fn shift_time_of_day(t: Time, minutes: i32) -> Time {
    let secs = t.h as i32 * 3600 + t.m as i32 * 60 + t.s as i32 + minutes * 60;
    let secs = secs.rem_euclid(86_400);
    Time {
        h: (secs / 3600) as u8,
        m: (secs / 60 % 60) as u8,
        s: (secs % 60) as u8,
    }
}

/// Longest [`format_clock`] output (`"03:45:12 PM"`).
pub const CLOCK_LEN: usize = 11;

/// Format the clock pill text: `"15:45:12"` in 24-hour mode, `"03:45:12 PM"`
/// in 12-hour mode. Returns the length written.
pub fn format_clock(t: Time, clock24: bool, out: &mut [u8; CLOCK_LEN]) -> usize {
    let two = |out: &mut [u8; CLOCK_LEN], i: usize, v: u8| {
        out[i] = b'0' + (v / 10) % 10;
        out[i + 1] = b'0' + v % 10;
    };
    let h = if clock24 {
        t.h
    } else {
        match t.h % 12 {
            0 => 12,
            n => n,
        }
    };
    two(out, 0, h);
    out[2] = b':';
    two(out, 3, t.m);
    out[5] = b':';
    two(out, 6, t.s);
    if clock24 {
        8
    } else {
        out[8] = b' ';
        out[9] = if t.h >= 12 { b'P' } else { b'A' };
        out[10] = b'M';
        CLOCK_LEN
    }
}

/// Length [`format_clock`] writes for the given mode (to size the clock pill).
pub const fn clock_len(clock24: bool) -> usize {
    if clock24 { 8 } else { CLOCK_LEN }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIN24: u8 = 0x04 | 0x02;
    const BCD24: u8 = 0x02;
    const BCD12: u8 = 0x00;
    const BIN12: u8 = 0x04;

    #[test]
    fn bcd_digits() {
        assert_eq!(bcd_to_bin(0x00), 0);
        assert_eq!(bcd_to_bin(0x09), 9);
        assert_eq!(bcd_to_bin(0x10), 10);
        assert_eq!(bcd_to_bin(0x59), 59);
        assert_eq!(bcd_to_bin(0x99), 99);
    }

    #[test]
    fn bcd_with_invalid_nibbles_does_not_overflow() {
        // Not valid BCD, but the arithmetic must stay in range (no overflow).
        assert_eq!(bcd_to_bin(0xFF), 15 + 15 * 10);
        assert_eq!(bcd_to_bin(0xA0), 100);
    }

    #[test]
    fn binary_24h_passthrough() {
        let t = decode(30, 45, 13, BIN24, 0);
        assert_eq!(
            t,
            Time {
                h: 13,
                m: 45,
                s: 30
            }
        );
    }

    #[test]
    fn bcd_24h() {
        let t = decode(0x59, 0x08, 0x23, BCD24, 0);
        assert_eq!(t, Time { h: 23, m: 8, s: 59 });
    }

    #[test]
    fn timezone_shift_wraps_both_ways() {
        assert_eq!(decode(0, 0, 2, BIN24, -3).h, 23); // 02:00 UTC -> 23:00 prev day
        assert_eq!(decode(0, 0, 23, BIN24, 3).h, 2);
        assert_eq!(decode(0, 0, 12, BIN24, -3).h, 9);
        assert_eq!(decode(0, 0, 5, BIN24, -48).h, 5); // multi-day offsets wrap too
    }

    #[test]
    fn twelve_hour_bcd_midnight_noon_and_afternoon() {
        // 12:xx AM -> 0, 12:xx PM -> 12, 1 PM -> 13, 11 PM -> 23, 1 AM -> 1.
        assert_eq!(decode(0, 0, 0x12, BCD12, 0).h, 0);
        assert_eq!(decode(0, 0, 0x12 | 0x80, BCD12, 0).h, 12);
        assert_eq!(decode(0, 0, 0x01 | 0x80, BCD12, 0).h, 13);
        assert_eq!(decode(0, 0, 0x11 | 0x80, BCD12, 0).h, 23);
        assert_eq!(decode(0, 0, 0x01, BCD12, 0).h, 1);
    }

    #[test]
    fn twelve_hour_binary_mode() {
        assert_eq!(decode(0, 0, 12, BIN12, 0).h, 0);
        assert_eq!(decode(0, 0, 12 | 0x80, BIN12, 0).h, 12);
        assert_eq!(decode(0, 0, 3 | 0x80, BIN12, 0).h, 15);
    }

    #[test]
    fn twelve_hour_with_timezone() {
        // 1 AM UTC with -3 -> 22:00 previous day.
        assert_eq!(decode(0, 0, 0x01, BCD12, -3).h, 22);
        // 11 PM UTC with -3 -> 20:00.
        assert_eq!(decode(0, 0, 0x11 | 0x80, BCD12, -3).h, 20);
    }

    #[test]
    fn minutes_and_seconds_unaffected_by_hour_logic() {
        let t = decode(0x45, 0x30, 0x12 | 0x80, BCD12, -3);
        assert_eq!(t, Time { h: 9, m: 30, s: 45 });
    }

    #[test]
    fn malformed_hours_register_stays_in_0_to_23() {
        for raw in 0..=255u8 {
            for regb in [BIN24, BCD24, BCD12, BIN12] {
                let t = decode(0, 0, raw, regb, -3);
                assert!(t.h < 24, "raw={raw:#x} regb={regb:#x} -> {}", t.h);
            }
        }
    }

    #[test]
    fn unix_epoch_and_known_instants() {
        assert_eq!(unix_time(1970, 1, 1, 0, 0, 0), Some(0));
        assert_eq!(unix_time(2000, 2, 29, 23, 59, 59), Some(951_868_799));
        assert_eq!(unix_time(2024, 12, 31, 0, 0, 0), Some(1_735_603_200));
        assert_eq!(unix_time(2026, 10, 7, 12, 34, 56), Some(1_791_376_496));
        assert_eq!(unix_time(2038, 1, 19, 3, 14, 8), Some(2_147_483_648));
    }

    #[test]
    fn unix_rejects_impossible_dates() {
        assert_eq!(unix_time(2026, 0, 1, 0, 0, 0), None);
        assert_eq!(unix_time(2026, 13, 1, 0, 0, 0), None);
        assert_eq!(unix_time(2026, 1, 0, 0, 0, 0), None);
        assert_eq!(unix_time(2026, 4, 31, 0, 0, 0), None);
        assert_eq!(unix_time(2026, 2, 29, 0, 0, 0), None); // not a leap year
        assert_eq!(unix_time(2024, 2, 29, 0, 0, 0), Some(1_709_164_800));
        assert_eq!(unix_time(2100, 2, 29, 0, 0, 0), None); // century, not leap
        assert_eq!(unix_time(2000, 2, 29, 0, 0, 0), Some(951_782_400)); // 400 rule
        assert_eq!(unix_time(2026, 1, 1, 24, 0, 0), None);
        assert_eq!(unix_time(2026, 1, 1, 0, 60, 0), None);
        assert_eq!(unix_time(2026, 1, 1, 0, 0, 60), None);
        assert_eq!(unix_time(1969, 12, 31, 23, 59, 59), None); // before the epoch
    }

    #[test]
    fn unix_days_are_monotonic_across_every_month_end() {
        // Walking day by day for 8 years must always advance by exactly 86400.
        let mut prev = unix_time(2020, 1, 1, 0, 0, 0).unwrap();
        for year in 2020..2028u32 {
            for month in 1..=12u8 {
                for day in 1..=31u8 {
                    if year == 2020 && month == 1 && day == 1 {
                        continue;
                    }
                    if let Some(t) = unix_time(year, month, day, 0, 0, 0) {
                        assert_eq!(t - prev, 86_400, "{year}-{month}-{day}");
                        prev = t;
                    }
                }

    // ------------------------------------------------------- date and time

    fn dt(y: u16, m: u8, d: u8, h: u8, mi: u8, s: u8) -> DateTime {
        DateTime {
            date: Date { y, m, d },
            time: Time { h, m: mi, s },
        }
    }

    #[test]
    fn leap_years_and_month_lengths() {
        assert!(is_leap(2000) && is_leap(2024) && !is_leap(1900) && !is_leap(2023));
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2023, 4), 30);
        assert_eq!(days_in_month(2023, 12), 31);
        assert_eq!(days_in_month(2023, 13), 0);
    }

    #[test]
    fn epoch_known_values() {
        assert_eq!(dt(1970, 1, 1, 0, 0, 0).to_epoch(), 0);
        assert_eq!(dt(2000, 1, 1, 0, 0, 0).to_epoch(), 946_684_800);
        assert_eq!(dt(2024, 2, 29, 12, 0, 0).to_epoch(), 1_709_208_000);
        assert_eq!(
            DateTime::from_epoch(1_709_208_000),
            dt(2024, 2, 29, 12, 0, 0)
        );
        assert_eq!(DateTime::from_epoch(-1), dt(1969, 12, 31, 23, 59, 59));
    }

    #[test]
    fn epoch_roundtrips_over_decades() {
        let mut t = dt(1970, 1, 1, 0, 0, 0).to_epoch();
        while t < dt(2100, 1, 1, 0, 0, 0).to_epoch() {
            let d = DateTime::from_epoch(t);
            assert_eq!(d.to_epoch(), t);
            assert!(d.date.m >= 1 && d.date.m <= 12 && d.date.d >= 1);
            assert!(d.date.d <= days_in_month(d.date.y, d.date.m));
            t += 86_400 * 7 + 13;
        }
    }

    #[test]
    fn weekdays() {
        assert_eq!(dt(1970, 1, 1, 0, 0, 0).weekday(), 4); // Thursday
        assert_eq!(dt(2024, 2, 29, 0, 0, 0).weekday(), 4);
        assert_eq!(dt(2000, 1, 1, 0, 0, 0).weekday(), 6); // Saturday
        assert_eq!(dt(2026, 10, 7, 0, 0, 0).weekday(), 3); // Wednesday
    }

    #[test]
    fn validity() {
        assert!(dt(2024, 2, 29, 23, 59, 59).is_valid());
        assert!(!dt(2023, 2, 29, 0, 0, 0).is_valid());
        assert!(!dt(2024, 13, 1, 0, 0, 0).is_valid());
        assert!(!dt(2024, 0, 1, 0, 0, 0).is_valid());
        assert!(!dt(2024, 4, 31, 0, 0, 0).is_valid());
        assert!(!dt(2024, 1, 1, 24, 0, 0).is_valid());
        assert!(!dt(2024, 1, 1, 0, 60, 0).is_valid());
        assert!(!dt(1969, 12, 31, 0, 0, 0).is_valid());
        assert!(!dt(2100, 1, 1, 0, 0, 0).is_valid());
    }

    #[test]
    fn timezone_carries_into_the_date() {
        // 00:30 UTC on March 1st 2024 is still Feb 29th evening in Brasilia.
        let local = utc_to_local(dt(2024, 3, 1, 0, 30, 0), -180);
        assert_eq!(local, dt(2024, 2, 29, 21, 30, 0));
        assert_eq!(local_to_utc(local, -180), dt(2024, 3, 1, 0, 30, 0));
        // New year, half-hour zone (India).
        assert_eq!(
            utc_to_local(dt(2023, 12, 31, 20, 0, 0), 330),
            dt(2024, 1, 1, 1, 30, 0)
        );
        // Out-of-range offsets are clamped.
        assert_eq!(
            utc_to_local(dt(2024, 1, 1, 0, 0, 0), 100_000),
            dt(2024, 1, 1, 14, 0, 0)
        );
    }

    #[test]
    fn registers_roundtrip_in_every_mode() {
        let samples = [
            dt(2024, 2, 29, 0, 0, 0),
            dt(2024, 2, 29, 12, 0, 0),
            dt(2025, 12, 31, 23, 59, 59),
            dt(2007, 7, 4, 9, 5, 7),
            dt(2099, 1, 1, 13, 30, 15),
            dt(1999, 12, 31, 1, 2, 3),
        ];
        for regb in [BIN24, BCD24, BCD12, BIN12] {
            for s in samples {
                let raw = encode_datetime(&s, regb);
                assert_eq!(decode_datetime(raw, regb), s, "regb={regb:#x} {s:?}");
            }
        }
    }

    #[test]
    fn decode_unix_bcd_24h_with_century() {
        // 2026-10-07 12:34:56, BCD, 24h, century register 0x20.
        let raw = RawDateTime {
            sec: 0x56,
            min: 0x34,
            hour: 0x12,
            day: 0x07,
            month: 0x10,
            year: 0x26,
            century: 0x20,
        };
        assert_eq!(decode_unix(raw, BCD24), Some(1_791_376_496));
    }

    #[test]
    fn decode_unix_binary_and_12h() {
        let raw = RawDateTime {
            sec: 56,
            min: 34,
            hour: 12 | 0x80, // 12 PM = 12:00
            day: 7,
            month: 10,
            year: 26,
            century: 20,
        };
        assert_eq!(decode_unix(raw, BIN12), Some(1_791_376_496));
    }

    #[test]
    fn decode_unix_ignores_the_timezone_and_defaults_the_century() {
        // No century register (0) or garbage: 20xx.
        let mut raw = RawDateTime {

    fn bcd_encoding_is_packed_decimal() {
        let raw = encode_datetime(&dt(2024, 12, 31, 23, 59, 58), BCD24);
        assert_eq!(
            raw,
            RawRtc {
                sec: 0x58,
                min: 0x59,
                hour: 0x23,
                day: 0x31,
                month: 0x12,
                year: 0x24,
                century: 0x20
            }
        );
    }

    #[test]
    fn twelve_hour_registers_carry_the_pm_flag() {
        // 00:15 -> 12:15 AM, 12:15 -> 12:15 PM, 13:00 -> 1:00 PM.
        assert_eq!(encode_datetime(&dt(2024, 1, 1, 0, 15, 0), BCD12).hour, 0x12);
        assert_eq!(
            encode_datetime(&dt(2024, 1, 1, 12, 15, 0), BCD12).hour,
            0x12 | 0x80
        );
        assert_eq!(
            encode_datetime(&dt(2024, 1, 1, 13, 0, 0), BCD12).hour,
            0x01 | 0x80
        );
        assert_eq!(
            encode_datetime(&dt(2024, 1, 1, 13, 0, 0), BIN12).hour,
            1 | 0x80
        );
    }

    #[test]
    fn missing_century_reads_as_2000s() {
        let raw = RawRtc {
            sec: 0,
            min: 0,
            hour: 0,
            day: 1,
            month: 1,
            year: 0x30,
            century: 0,
        };
        assert_eq!(decode_unix(raw, BCD24), unix_time(2030, 1, 1, 0, 0, 0));
        raw.century = 0xFF;
        assert_eq!(decode_unix(raw, BCD24), unix_time(2030, 1, 1, 0, 0, 0));
        // Garbage date: None.
        raw.month = 0x13;
        assert_eq!(decode_unix(raw, BCD24), None);

            year: 0x24,
            century: 0,
        };
        assert_eq!(decode_datetime(raw, BCD24).date.y, 2024);
        let junk = RawRtc {
            century: 0xFF,
            ..raw
        };
        assert_eq!(decode_datetime(junk, BCD24).date.y, 2024);
    }

    #[test]
    fn garbage_registers_never_panic_and_stay_in_range() {
        for b in 0..=255u8 {
            for regb in [BIN24, BCD24, BCD12, BIN12] {
                let raw = RawRtc {
                    sec: b,
                    min: b,
                    hour: b,
                    day: b,
                    month: b,
                    year: b,
                    century: b,
                };
                let d = decode_datetime(raw, regb);
                assert!((1..=12).contains(&d.date.m));
                assert!((1..=31).contains(&d.date.d));
                assert!(d.time.h < 24);
            }
        }
    }

    #[test]
    fn time_of_day_shift_wraps_and_matches_whole_hours() {
        let t = Time { h: 1, m: 15, s: 30 };
        assert_eq!(shift_time_of_day(t, 0), t);
        assert_eq!(
            shift_time_of_day(t, -180),
            Time {
                h: 22,
                m: 15,
                s: 30
            }
        );
        assert_eq!(shift_time_of_day(t, 330), Time { h: 6, m: 45, s: 30 });
        assert_eq!(shift_time_of_day(t, -75), Time { h: 0, m: 0, s: 30 });
        assert_eq!(
            shift_time_of_day(t, 24 * 60 * 3 + 5),
            Time { h: 1, m: 20, s: 30 }
        );
        // Same hour result as the whole-hour shift `decode` has always done.
        for tz in [-12, -3, 0, 5, 14] {
            for h in 0..24u8 {
                let a = decode(7, 9, h, BIN24, tz);
                let b = shift_time_of_day(decode(7, 9, h, BIN24, 0), tz * 60);
                assert_eq!(a, b, "h={h} tz={tz}");
            }
        }
    }

    #[test]
    fn stepping_fields_wraps_and_keeps_the_date_valid() {
        let d = dt(2024, 1, 31, 23, 59, 59);
        assert_eq!(d.step(Field::Month, 1), dt(2024, 2, 29, 23, 59, 59));
        assert_eq!(dt(2023, 1, 31, 0, 0, 0).step(Field::Month, 1).date.d, 28);
        assert_eq!(d.step(Field::Month, -1), dt(2024, 12, 31, 23, 59, 59));
        assert_eq!(d.step(Field::Day, 1), dt(2024, 1, 1, 23, 59, 59));
        assert_eq!(dt(2024, 3, 1, 0, 0, 0).step(Field::Day, -1).date.d, 31);
        assert_eq!(d.step(Field::Hour, 1).time.h, 0);
        assert_eq!(d.step(Field::Minute, 1).time.m, 0);
        assert_eq!(d.step(Field::Second, 1).time.s, 0);
        assert_eq!(d.step(Field::Second, -60), d);
        // Leap day, then the year goes to a non-leap one.
        let leap = dt(2024, 2, 29, 0, 0, 0);
        assert_eq!(leap.step(Field::Year, 1), dt(2025, 2, 28, 0, 0, 0));
        assert_eq!(dt(2099, 6, 1, 0, 0, 0).step(Field::Year, 1).date.y, 1970);
        assert_eq!(dt(1970, 6, 1, 0, 0, 0).step(Field::Year, -1).date.y, 2099);
    }

    #[test]
    fn stepping_never_leaves_the_valid_range() {
        let mut d = dt(2024, 1, 31, 12, 30, 30);
        let fields = [
            Field::Day,
            Field::Month,
            Field::Year,
            Field::Hour,
            Field::Minute,
            Field::Second,
        ];
        let mut seed = 12345u32;
        for _ in 0..5000 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let f = fields[(seed >> 16) as usize % fields.len()];
            let delta = (seed >> 8) as i32 % 70 - 35;
            d = d.step(f, delta);
            assert!(d.is_valid(), "{d:?}");
        }
    }

    #[test]
    fn clock_text_24h_and_12h() {
        let mut out = [0u8; CLOCK_LEN];
        let n = format_clock(Time { h: 15, m: 4, s: 9 }, true, &mut out);
        assert_eq!(&out[..n], b"15:04:09");
        let n = format_clock(Time { h: 15, m: 4, s: 9 }, false, &mut out);
        assert_eq!(&out[..n], b"03:04:09 PM");
        let n = format_clock(Time { h: 0, m: 0, s: 0 }, false, &mut out);
        assert_eq!(&out[..n], b"12:00:00 AM");
        let n = format_clock(Time { h: 12, m: 30, s: 0 }, false, &mut out);
        assert_eq!(&out[..n], b"12:30:00 PM");
        let n = format_clock(Time { h: 9, m: 5, s: 1 }, false, &mut out);
        assert_eq!(&out[..n], b"09:05:01 AM");
        assert_eq!(clock_len(true), 8);
        assert_eq!(clock_len(false), CLOCK_LEN);
    }
}
