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
    let days = days_from_civil(year as i64, month as i64, day as i64);
    if days < 0 {
        return None;
    }
    Some(days as u64 * 86_400 + h as u64 * 3600 + m as u64 * 60 + s as u64)
}

/// Raw CMOS registers describing the current date and time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RawDateTime {
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
}

#[cfg(test)]
mod tests;
