//! Locale-aware formatting: numbers, byte sizes, dates and times.
//!
//! Nothing here is language specific in code. The separators, the month and weekday names
//! and the date patterns all come from the language's catalog (`fmt.*` and `date.*` keys),
//! so a new language only needs its text file. Every function takes the [`Lang`] explicitly
//! (the `*_now` helpers use the language in effect), writes into a `core::fmt::Write`, and
//! never allocates or panics, whatever the inputs.

use super::template::{Arg, render};
use super::{Lang, tr_in};
use crate::hw::rtc;
use crate::tk;
use core::fmt::{self, Write};

fn first_char(lang: Lang, key: &str, default: char) -> char {
    tr_in(lang, key).chars().next().unwrap_or(default)
}

/// The decimal separator of `lang` (`,` in Portuguese, `.` in English).
pub fn decimal_sep(lang: Lang) -> char {
    first_char(lang, tk!("fmt.decimal"), '.')
}

/// The thousands separator of `lang`.
pub fn group_sep(lang: Lang) -> char {
    first_char(lang, tk!("fmt.group"), ',')
}

/// Write the digits of `v` with `sep` between groups of three.
fn write_grouped<W: Write>(w: &mut W, v: u64, sep: Option<char>) -> fmt::Result {
    let mut digits = [0u8; 20];
    let mut n = 0;
    let mut x = v;
    loop {
        digits[n] = b'0' + (x % 10) as u8;
        n += 1;
        x /= 10;
        if x == 0 {
            break;
        }
    }
    for i in (0..n).rev() {
        w.write_char(digits[i] as char)?;
        if let Some(s) = sep
            && i > 0
            && i % 3 == 0
        {
            w.write_char(s)?;
        }
    }
    Ok(())
}

/// Write the integer `n`; with `group` the digits are grouped by thousands (`1.234.567`).
pub fn write_int<W: Write>(w: &mut W, lang: Lang, n: i64, group: bool) -> fmt::Result {
    if n < 0 {
        w.write_char('-')?;
    }
    write_grouped(w, n.unsigned_abs(), group.then(|| group_sep(lang)))
}

/// Write `scaled / 10^places` (`write_dec(w, lang, 12345, 1)` is `1.234,5` in Portuguese).
/// `places` is capped at 9.
pub fn write_dec<W: Write>(w: &mut W, lang: Lang, scaled: i64, places: u8) -> fmt::Result {
    let places = places.min(9) as u32;
    let unit = 10u64.pow(places);
    let abs = scaled.unsigned_abs();
    if scaled < 0 {
        w.write_char('-')?;
    }
    write_grouped(w, abs / unit, Some(group_sep(lang)))?;
    if places > 0 {
        w.write_char(decimal_sep(lang))?;
        write!(w, "{:0>width$}", abs % unit, width = places as usize)?;
    }
    Ok(())
}

/// Write a byte count: `512 B`, `1,5 KiB`, `12,0 MiB`, `2,3 GiB`, `1,0 TiB` (binary units,
/// one decimal, the language's decimal separator).
pub fn write_size<W: Write>(w: &mut W, lang: Lang, bytes: u64) -> fmt::Result {
    const UNITS: [&str; 4] = ["KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return write!(w, "{bytes} B");
    }
    let mut unit = 0;
    let mut tenths = bytes as u128 * 10 / 1024; // tenths of the current unit
    while tenths >= 10_240 && unit + 1 < UNITS.len() {
        tenths /= 1024;
        unit += 1;
    }
    write!(
        w,
        "{}{}{} {}",
        tenths / 10,
        decimal_sep(lang),
        tenths % 10,
        UNITS[unit]
    )
}

/// A calendar date and time of day. Out-of-range fields are clamped when formatted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Civil {
    pub year: i32,
    /// 1..=12
    pub month: u8,
    /// 1..=31
    pub day: u8,
    /// 0 = Sunday .. 6 = Saturday
    pub weekday: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

impl Civil {
    /// The fields of an RTC date and time (the weekday is computed).
    pub fn from_rtc(dt: &rtc::DateTime) -> Civil {
        Civil {
            year: dt.date.y as i32,
            month: dt.date.m,
            day: dt.date.d,
            weekday: dt.weekday(),
            hour: dt.time.h,
            minute: dt.time.m,
            second: dt.time.s,
        }
    }
}

const WEEKDAY_SHORT: [&str; 7] = [
    tk!("date.wd.0"),
    tk!("date.wd.1"),
    tk!("date.wd.2"),
    tk!("date.wd.3"),
    tk!("date.wd.4"),
    tk!("date.wd.5"),
    tk!("date.wd.6"),
];
const WEEKDAY_LONG: [&str; 7] = [
    tk!("date.wdl.0"),
    tk!("date.wdl.1"),
    tk!("date.wdl.2"),
    tk!("date.wdl.3"),
    tk!("date.wdl.4"),
    tk!("date.wdl.5"),
    tk!("date.wdl.6"),
];
const MONTH_SHORT: [&str; 12] = [
    tk!("date.mon.1"),
    tk!("date.mon.2"),
    tk!("date.mon.3"),
    tk!("date.mon.4"),
    tk!("date.mon.5"),
    tk!("date.mon.6"),
    tk!("date.mon.7"),
    tk!("date.mon.8"),
    tk!("date.mon.9"),
    tk!("date.mon.10"),
    tk!("date.mon.11"),
    tk!("date.mon.12"),
];
const MONTH_LONG: [&str; 12] = [
    tk!("date.monl.1"),
    tk!("date.monl.2"),
    tk!("date.monl.3"),
    tk!("date.monl.4"),
    tk!("date.monl.5"),
    tk!("date.monl.6"),
    tk!("date.monl.7"),
    tk!("date.monl.8"),
    tk!("date.monl.9"),
    tk!("date.monl.10"),
    tk!("date.monl.11"),
    tk!("date.monl.12"),
];

/// The abbreviated weekday (`qui` / `Thu`); `wd` 0 is Sunday and wraps modulo 7.
pub fn weekday_short(lang: Lang, wd: u8) -> &'static str {
    tr_in(lang, WEEKDAY_SHORT[wd as usize % 7])
}

/// The full weekday name (`Quinta-feira` / `Thursday`).
pub fn weekday_long(lang: Lang, wd: u8) -> &'static str {
    tr_in(lang, WEEKDAY_LONG[wd as usize % 7])
}

const WEEKDAY_INITIAL: [&str; 7] = [
    tk!("date.wd1.0"),
    tk!("date.wd1.1"),
    tk!("date.wd1.2"),
    tk!("date.wd1.3"),
    tk!("date.wd1.4"),
    tk!("date.wd1.5"),
    tk!("date.wd1.6"),
];

/// The one-letter weekday of the calendar header (`D S T Q Q S S` / `S M T W T F S`).
pub fn weekday_initial(lang: Lang, wd: u8) -> &'static str {
    tr_in(lang, WEEKDAY_INITIAL[wd as usize % 7])
}

/// The abbreviated month (`out` / `Oct`); `m` is 1..=12, clamped.
pub fn month_short(lang: Lang, m: u8) -> &'static str {
    tr_in(lang, MONTH_SHORT[m.clamp(1, 12) as usize - 1])
}

/// The full month name (`outubro` / `October`).
pub fn month_long(lang: Lang, m: u8) -> &'static str {
    tr_in(lang, MONTH_LONG[m.clamp(1, 12) as usize - 1])
}

/// Which date text to produce.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DateStyle {
    /// `08/10/2026` / `10/08/2026`
    Short,
    /// `8 out 2026` / `Oct 8, 2026`
    Medium,
    /// `Quinta-feira, 8 de outubro de 2026` / `Thursday, October 8, 2026`
    Long,
    /// `8 de outubro de 2026` / `October 8, 2026`
    LongNoWeekday,
    /// `Outubro de 2026` / `October 2026`
    MonthYear,
    /// `Quinta-feira` / `Thursday`
    Weekday,
    /// `qui, 8 out 2026 23:49` / `Thu, Oct 8 2026 11:49 PM`
    Full,
    /// The panel clock: `qui 8 out  23:49` / `Thu Oct 8  11:49 PM`
    Panel,
}

impl DateStyle {
    const fn key(self) -> &'static str {
        match self {
            DateStyle::Short => tk!("fmt.date.short"),
            DateStyle::Medium => tk!("fmt.date.medium"),
            DateStyle::Long => tk!("fmt.date.long"),
            DateStyle::LongNoWeekday => tk!("fmt.date.long_nowd"),
            DateStyle::MonthYear => tk!("fmt.date.month_year"),
            DateStyle::Weekday => tk!("fmt.date.weekday"),
            DateStyle::Full => tk!("fmt.datetime.full"),
            DateStyle::Panel => tk!("fmt.datetime.panel"),
        }
    }
}

/// A date (and, for [`DateStyle::Full`] / [`DateStyle::Panel`], the time) ready to print.
pub struct DateFmt {
    pub lang: Lang,
    pub civil: Civil,
    pub style: DateStyle,
    /// 24-hour clock (only the styles that include the time look at it).
    pub clock24: bool,
}

impl fmt::Display for DateFmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (l, c) = (self.lang, &self.civil);
        let time = TimeFmt {
            lang: l,
            civil: *c,
            clock24: self.clock24,
            seconds: false,
        };
        let day = c.day.clamp(1, 31);
        let month = c.month.clamp(1, 12);
        let args: [(&str, Arg<'_>); 12] = [
            ("weekday", Arg::Str(weekday_short(l, c.weekday))),
            ("weekday_long", Arg::Str(weekday_long(l, c.weekday))),
            ("month_short", Arg::Str(month_short(l, month))),
            ("month_long", Arg::Str(month_long(l, month))),
            ("day", Arg::Int(day as i64)),
            ("day2", Arg::Pad(day as u64, 2)),
            ("month", Arg::Int(month as i64)),
            ("month2", Arg::Pad(month as u64, 2)),
            ("year", Arg::Int(c.year as i64)),
            ("year2", Arg::Pad(c.year.rem_euclid(100) as u64, 2)),
            ("time", Arg::Display(&time)),
            ("weekday_n", Arg::Int((c.weekday % 7) as i64)),
        ];
        render(f, l, tr_in(l, self.style.key()), &args)
    }
}

/// A time of day ready to print: `23:49`, `11:49 PM`, with or without seconds.
pub struct TimeFmt {
    pub lang: Lang,
    pub civil: Civil,
    pub clock24: bool,
    pub seconds: bool,
}

impl fmt::Display for TimeFmt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (l, c) = (self.lang, &self.civil);
        let h = c.hour.min(23);
        let h12 = match h % 12 {
            0 => 12,
            n => n,
        };
        let args: [(&str, Arg<'_>); 6] = [
            ("hour", Arg::Pad(h as u64, 2)),
            ("hour12", Arg::Int(h12 as i64)),
            ("minute", Arg::Pad(c.minute.min(59) as u64, 2)),
            ("second", Arg::Pad(c.second.min(59) as u64, 2)),
            (
                "ampm",
                Arg::Str(tr_in(
                    l,
                    if h >= 12 {
                        tk!("fmt.pm")
                    } else {
                        tk!("fmt.am")
                    },
                )),
            ),
            ("hour24", Arg::Int(h as i64)),
        ];
        let key = match (self.clock24, self.seconds) {
            (true, false) => tk!("fmt.time.24"),
            (true, true) => tk!("fmt.time.24s"),
            (false, false) => tk!("fmt.time.12"),
            (false, true) => tk!("fmt.time.12s"),
        };
        render(f, l, tr_in(l, key), &args)
    }
}

/// [`DateFmt`] for the language in effect.
pub fn date_now(civil: Civil, style: DateStyle, clock24: bool) -> DateFmt {
    DateFmt {
        lang: super::lang(),
        civil,
        style,
        clock24,
    }
}

/// [`TimeFmt`] for the language in effect.
pub fn time_now(civil: Civil, clock24: bool, seconds: bool) -> TimeFmt {
    TimeFmt {
        lang: super::lang(),
        civil,
        clock24,
        seconds,
    }
}

/// The clock format a language uses unless the user chose one: `true` = 24 hours.
pub fn default_clock24(lang: Lang) -> bool {
    tr_in(lang, tk!("fmt.clock24_default")).trim() != "0"
}

#[cfg(test)]
mod tests;
