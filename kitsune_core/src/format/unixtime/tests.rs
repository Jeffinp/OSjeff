use super::*;

fn dt(y: i32, mo: u8, d: u8, h: u8, mi: u8, s: u8) -> DateTime {
    DateTime {
        year: y,
        month: mo,
        day: d,
        hour: h,
        minute: mi,
        second: s,
    }
}

#[test]
fn epoch_is_zero() {
    assert_eq!(dt(1970, 1, 1, 0, 0, 0).to_unix(), Some(0));
    assert_eq!(days_from_civil(1970, 1, 1), 0);
}

#[test]
fn known_instants() {
    assert_eq!(dt(2000, 1, 1, 0, 0, 0).to_unix(), Some(946_684_800));
    assert_eq!(dt(2024, 1, 1, 0, 0, 0).to_unix(), Some(MIN_PLAUSIBLE_UNIX));
    assert_eq!(dt(2100, 1, 1, 0, 0, 0).to_unix(), Some(MAX_PLAUSIBLE_UNIX));
    assert_eq!(dt(2026, 10, 7, 12, 30, 45).to_unix(), Some(1_791_376_245));
}

#[test]
fn leap_years() {
    assert!(is_leap(2000));
    assert!(!is_leap(1900));
    assert!(is_leap(2024));
    assert!(!is_leap(2100));
    assert_eq!(days_in_month(2024, 2), 29);
    assert_eq!(days_in_month(2023, 2), 28);
    assert_eq!(days_in_month(2023, 13), 0);
}

#[test]
fn rejects_impossible_dates() {
    assert_eq!(dt(2023, 2, 29, 0, 0, 0).to_unix(), None);
    assert!(dt(2024, 2, 29, 0, 0, 0).to_unix().is_some());
    assert_eq!(dt(2024, 4, 31, 0, 0, 0).to_unix(), None);
    assert_eq!(dt(2024, 0, 1, 0, 0, 0).to_unix(), None);
    assert_eq!(dt(2024, 1, 0, 0, 0, 0).to_unix(), None);
    assert_eq!(dt(2024, 1, 1, 24, 0, 0).to_unix(), None);
    assert_eq!(dt(2024, 1, 1, 0, 60, 0).to_unix(), None);
    assert_eq!(dt(2024, 1, 1, 0, 0, 60).to_unix(), None);
}

#[test]
fn before_1970_is_none() {
    assert_eq!(dt(1969, 12, 31, 23, 59, 59).to_unix(), None);
}

#[test]
fn roundtrip_every_day_of_four_years() {
    for days in 19_723..19_723 + 1461 {
        let (y, m, d) = civil_from_days(days);
        assert_eq!(days_from_civil(y, m, d), days);
    }
}

#[test]
fn roundtrip_wide_range() {
    let mut t = 0u64;
    while t < 8_000_000_000 {
        let d = DateTime::from_unix(t);
        assert_eq!(d.to_unix(), Some(t));
        t += 86_413 * 7 + 11;
    }
}

#[test]
fn from_unix_known() {
    assert_eq!(DateTime::from_unix(0), dt(1970, 1, 1, 0, 0, 0));
    assert_eq!(
        DateTime::from_unix(1_791_376_245),
        dt(2026, 10, 7, 12, 30, 45)
    );
    assert_eq!(DateTime::from_unix(951_782_400), dt(2000, 2, 29, 0, 0, 0));
}

#[test]
fn from_unix_saturates() {
    let d = DateTime::from_unix(u64::MAX);
    assert_eq!(d, dt(9999, 12, 31, 23, 59, 59));
}

#[test]
fn format_is_fixed_width() {
    let mut b = [0u8; 19];
    let n = dt(2026, 1, 2, 3, 4, 5).format(&mut b);
    assert_eq!(&b[..n], b"2026-01-02 03:04:05");
}

#[test]
fn plausibility_window() {
    assert!(!is_plausible(0));
    assert!(!is_plausible(MIN_PLAUSIBLE_UNIX - 1));
    assert!(is_plausible(MIN_PLAUSIBLE_UNIX));
    assert!(is_plausible(1_791_376_245));
    assert!(is_plausible(MAX_PLAUSIBLE_UNIX - 1));
    assert!(!is_plausible(MAX_PLAUSIBLE_UNIX));
}

#[test]
fn rtc_fields() {
    assert_eq!(
        from_rtc_fields(45, 30, 12, 7, 10, 26, 20),
        Some(1_791_376_245)
    );
    // No century register: assume 20xx.
    assert_eq!(
        from_rtc_fields(45, 30, 12, 7, 10, 26, 0),
        Some(1_791_376_245)
    );
    assert_eq!(from_rtc_fields(0, 0, 0, 31, 2, 26, 20), None);
}
