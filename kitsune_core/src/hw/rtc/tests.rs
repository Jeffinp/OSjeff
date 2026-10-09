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
