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
    }
}
