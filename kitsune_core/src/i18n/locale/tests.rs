use super::*;
use alloc::string::{String, ToString};

// 2026-10-08 was a Thursday.
const C: Civil = Civil {
    year: 2026,
    month: 10,
    day: 8,
    weekday: 4,
    hour: 23,
    minute: 49,
    second: 5,
};

fn s(d: impl fmt::Display) -> String {
    d.to_string()
}

fn date(l: Lang, style: DateStyle, clock24: bool) -> String {
    s(DateFmt {
        lang: l,
        civil: C,
        style,
        clock24,
    })
}

#[test]
fn grouped_integers() {
    let mut o = String::new();
    for (n, pt, en) in [
        (0i64, "0", "0"),
        (999, "999", "999"),
        (1000, "1.000", "1,000"),
        (-1234567, "-1.234.567", "-1,234,567"),
        (
            i64::MAX,
            "9.223.372.036.854.775.807",
            "9,223,372,036,854,775,807",
        ),
        (
            i64::MIN,
            "-9.223.372.036.854.775.808",
            "-9,223,372,036,854,775,808",
        ),
    ] {
        o.clear();
        write_int(&mut o, Lang::Pt, n, true).unwrap();
        assert_eq!(o, pt);
        o.clear();
        write_int(&mut o, Lang::En, n, true).unwrap();
        assert_eq!(o, en);
    }
    o.clear();
    write_int(&mut o, Lang::Pt, 1234, false).unwrap();
    assert_eq!(o, "1234");
}

#[test]
fn decimals() {
    let f = |l, v, p| {
        let mut o = String::new();
        write_dec(&mut o, l, v, p).unwrap();
        o
    };
    assert_eq!(f(Lang::Pt, 12345, 1), "1.234,5");
    assert_eq!(f(Lang::En, 12345, 1), "1,234.5");
    assert_eq!(f(Lang::Pt, 5, 2), "0,05");
    assert_eq!(f(Lang::En, -5, 2), "-0.05");
    assert_eq!(f(Lang::En, 1234, 0), "1,234");
    assert_eq!(f(Lang::Pt, 1, 20), "0,000000001"); // places capped at 9
    assert_eq!(f(Lang::Pt, i64::MIN, 3), "-9.223.372.036.854.775,808");
}

#[test]
fn sizes() {
    let f = |l, b| {
        let mut o = String::new();
        write_size(&mut o, l, b).unwrap();
        o
    };
    assert_eq!(f(Lang::Pt, 0), "0 B");
    assert_eq!(f(Lang::Pt, 1023), "1023 B");
    assert_eq!(f(Lang::Pt, 1024), "1,0 KiB");
    assert_eq!(f(Lang::En, 1536), "1.5 KiB");
    assert_eq!(f(Lang::Pt, 3 * 1024 * 1024), "3,0 MiB");
    assert_eq!(f(Lang::En, 5 * 1024 * 1024 * 1024 / 2), "2.5 GiB");
    assert_eq!(f(Lang::En, 1024u64.pow(4)), "1.0 TiB");
    // Past TiB the number just grows; it never panics.
    assert_eq!(f(Lang::En, u64::MAX), "16777215.9 TiB");
}

#[test]
fn size_matches_the_file_manager_format() {
    for b in [
        0,
        1,
        1023,
        1024,
        1500,
        10_240,
        1 << 20,
        123_456_789,
        1 << 40,
    ] {
        let mut o = String::new();
        write_size(&mut o, Lang::Pt, b).unwrap();
        assert_eq!(o, crate::apps::fileman::format_size(b), "{b}");
    }
}

#[test]
fn names() {
    assert_eq!(weekday_short(Lang::Pt, 0), "dom");
    assert_eq!(weekday_short(Lang::Pt, 6), "sáb");
    assert_eq!(weekday_short(Lang::En, 4), "Thu");
    assert_eq!(weekday_long(Lang::Pt, 2), "terça-feira");
    assert_eq!(weekday_long(Lang::En, 7), "Sunday"); // wraps
    assert_eq!(weekday_initial(Lang::Pt, 3), "Q");
    assert_eq!(weekday_initial(Lang::En, 3), "W");
    assert_eq!(month_short(Lang::Pt, 10), "out");
    assert_eq!(month_short(Lang::En, 0), "Jan"); // clamped
    assert_eq!(month_long(Lang::Pt, 3), "março");
    assert_eq!(month_long(Lang::En, 99), "December");
}

#[test]
fn dates_in_both_languages() {
    assert_eq!(date(Lang::Pt, DateStyle::Short, true), "08/10/2026");
    assert_eq!(date(Lang::En, DateStyle::Short, true), "10/08/2026");
    assert_eq!(date(Lang::Pt, DateStyle::Medium, true), "8 out 2026");
    assert_eq!(date(Lang::En, DateStyle::Medium, true), "Oct 8, 2026");
    assert_eq!(
        date(Lang::Pt, DateStyle::Long, true),
        "Quinta-feira, 8 de outubro de 2026"
    );
    assert_eq!(
        date(Lang::En, DateStyle::Long, true),
        "Thursday, October 8, 2026"
    );
    assert_eq!(
        date(Lang::Pt, DateStyle::LongNoWeekday, true),
        "8 de outubro de 2026"
    );
    assert_eq!(date(Lang::En, DateStyle::MonthYear, true), "October 2026");
    assert_eq!(
        date(Lang::Pt, DateStyle::MonthYear, true),
        "Outubro de 2026"
    );
    assert_eq!(date(Lang::Pt, DateStyle::Weekday, true), "Quinta-feira");
    assert_eq!(date(Lang::En, DateStyle::Weekday, true), "Thursday");
}

#[test]
fn date_and_time_together() {
    assert_eq!(
        date(Lang::Pt, DateStyle::Full, true),
        "qui, 8 out 2026 23:49"
    );
    assert_eq!(
        date(Lang::En, DateStyle::Full, false),
        "Thu, Oct 8 2026 11:49 PM"
    );
    assert_eq!(date(Lang::Pt, DateStyle::Panel, true), "qui 8 out  23:49");
    assert_eq!(
        date(Lang::En, DateStyle::Panel, false),
        "Thu Oct 8  11:49 PM"
    );
    assert_eq!(date(Lang::En, DateStyle::Panel, true), "Thu Oct 8  23:49");
}

#[test]
fn times() {
    let t = |l, c: Civil, c24, secs| {
        s(TimeFmt {
            lang: l,
            civil: c,
            clock24: c24,
            seconds: secs,
        })
    };
    assert_eq!(t(Lang::Pt, C, true, false), "23:49");
    assert_eq!(t(Lang::Pt, C, true, true), "23:49:05");
    assert_eq!(t(Lang::En, C, false, true), "11:49:05 PM");
    let midnight = Civil {
        hour: 0,
        minute: 5,
        ..C
    };
    assert_eq!(t(Lang::En, midnight, false, false), "12:05 AM");
    assert_eq!(t(Lang::En, midnight, true, false), "00:05");
    let noon = Civil {
        hour: 12,
        minute: 0,
        ..C
    };
    assert_eq!(t(Lang::En, noon, false, false), "12:00 PM");
    let nine = Civil {
        hour: 9,
        minute: 3,
        ..C
    };
    assert_eq!(t(Lang::En, nine, false, false), "9:03 AM");
}

#[test]
fn hostile_civil_values_do_not_panic() {
    let bad = Civil {
        year: i32::MIN,
        month: 255,
        day: 0,
        weekday: 255,
        hour: 255,
        minute: 255,
        second: 255,
    };
    for l in Lang::ALL {
        for st in [
            DateStyle::Short,
            DateStyle::Medium,
            DateStyle::Long,
            DateStyle::LongNoWeekday,
            DateStyle::MonthYear,
            DateStyle::Weekday,
            DateStyle::Full,
            DateStyle::Panel,
        ] {
            for c24 in [true, false] {
                let _ = s(DateFmt {
                    lang: l,
                    civil: bad,
                    style: st,
                    clock24: c24,
                });
            }
        }
    }
}

#[test]
fn from_rtc_computes_the_weekday() {
    let dt = rtc::DateTime {
        date: rtc::Date {
            y: 2026,
            m: 10,
            d: 8,
        },
        time: rtc::Time { h: 23, m: 49, s: 5 },
    };
    assert_eq!(Civil::from_rtc(&dt), C);
}

#[test]
fn language_clock_defaults() {
    assert!(default_clock24(Lang::Pt));
    assert!(!default_clock24(Lang::En));
}
