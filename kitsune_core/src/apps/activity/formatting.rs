//! formatting (split out of `activity.rs`).

use super::*;

/// `12,3%` (`12.3%` in English) from tenths of a percent.
pub fn fmt_pct(pm: u32) -> FixedBuf<12> {
    let mut b = FixedBuf::new();
    let _ = write!(
        b,
        "{}{}{}%",
        pm / 10,
        locale::decimal_sep(i18n::lang()),
        pm % 10
    );
    b
}

/// `12%` from tenths of a percent (rounded), for places too narrow for a decimal.
pub fn fmt_pct_int(pm: u32) -> FixedBuf<8> {
    let mut b = FixedBuf::new();
    let _ = write!(b, "{}%", (pm + 5) / 10);
    b
}

/// `512 B`, `1,5 KiB`, `12,0 MiB`, `3,2 GiB` (`1.5 KiB` in English).
pub fn fmt_size(v: u64) -> FixedBuf<20> {
    let mut b = FixedBuf::new();
    let _ = locale::write_size(&mut b, i18n::lang(), v);
    b
}

/// `2,0 KiB/s` for a byte rate.
pub fn fmt_speed(bytes_per_s: u64) -> FixedBuf<24> {
    let mut b = FixedBuf::new();
    let _ = write!(b, "{}/s", fmt_size(bytes_per_s));
    b
}

/// A whole number with the language's thousands separator: `12.345` / `12,345`.
pub fn fmt_count(v: u64) -> FixedBuf<28> {
    let mut b = FixedBuf::new();
    let _ = locale::write_int(
        &mut b,
        i18n::lang(),
        i64::try_from(v).unwrap_or(i64::MAX),
        true,
    );
    b
}

/// A catalog text filled with `args`, in a fixed buffer.
pub(super) fn fill<const N: usize>(key: &str, args: &[(&str, Arg<'_>)]) -> FixedBuf<N> {
    let mut b = FixedBuf::new();
    let _ = b.write_str(&i18n::tr_fmt(key, args));
    b
}

/// Uptime for people: `42 s`, `3 min 05 s`, `1 h 02 min`, `2 d 03 h`.
pub fn fmt_elapsed(secs: u64) -> FixedBuf<20> {
    let int = |n: u64| Arg::Int(i64::try_from(n).unwrap_or(i64::MAX));
    let (h24, m60, s60) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if secs >= 86_400 {
        fill(
            tk!("tasks.fmt.dh"),
            &[("d", int(secs / 86_400)), ("h", Arg::Pad(h24, 2))],
        )
    } else if secs >= 3600 {
        fill(
            tk!("tasks.fmt.hm"),
            &[("h", int(secs / 3600)), ("m", Arg::Pad(m60, 2))],
        )
    } else if secs >= 60 {
        fill(
            tk!("tasks.fmt.ms"),
            &[("m", int(secs / 60)), ("s", Arg::Pad(s60, 2))],
        )
    } else {
        fill(tk!("tasks.fmt.s"), &[("s", int(secs))])
    }
}

/// `01:02:03` clock-style uptime, with `d` days in front past one day.
pub fn fmt_clock(secs: u64) -> FixedBuf<20> {
    let (d, h, m, s) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        let d = Arg::Int(i64::try_from(d).unwrap_or(i64::MAX));
        fill(
            tk!("tasks.fmt.clock_days"),
            &[
                ("d", d),
                ("h", Arg::Pad(h, 2)),
                ("m", Arg::Pad(m, 2)),
                ("s", Arg::Pad(s, 2)),
            ],
        )
    } else {
        let mut b = FixedBuf::new();
        let _ = write!(b, "{h:02}:{m:02}:{s:02}");
        b
    }
}

/// `há 12 s` / `12 s ago` for a graph's hover label (`0` is "agora" / "now").
pub fn fmt_ago(secs: u32) -> FixedBuf<16> {
    if secs == 0 {
        fill(tk!("tasks.fmt.now"), &[])
    } else {
        fill(tk!("tasks.fmt.ago"), &[("n", Arg::Int(i64::from(secs)))])
    }
}

/// A log timestamp (milliseconds since boot) as `12,345` seconds, or `3:25,100` once past
/// a minute and `1:02:03,400` past an hour.
pub fn fmt_log_time(ms: u32) -> FixedBuf<16> {
    let mut b = FixedBuf::new();
    let (h, m, s, ms) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    let dec = locale::decimal_sep(i18n::lang());
    if h > 0 {
        let _ = write!(b, "{h}:{m:02}:{s:02}{dec}{ms:03}");
    } else if m > 0 {
        let _ = write!(b, "{m}:{s:02}{dec}{ms:03}");
    } else {
        let _ = write!(b, "{s}{dec}{ms:03}");
    }
    b
}

/// The catalog key of the chip text of a log level.
pub const fn level_key(l: crate::system::klog::Level) -> &'static str {
    use crate::system::klog::Level;
    match l {
        Level::Trace => tk!("log.chip.trace"),
        Level::Debug => tk!("log.chip.debug"),
        Level::Info => tk!("log.chip.info"),
        Level::Warn => tk!("log.chip.warn"),
        Level::Error => tk!("log.chip.error"),
        Level::Fatal => tk!("log.chip.fatal"),
    }
}

/// The chip text of a log level, in the language in effect.
pub fn level_name(l: crate::system::klog::Level) -> &'static str {
    i18n::tr(level_key(l))
}

/// `0,42` from thousandths (the load average).
pub fn fmt_milli(v: u32) -> FixedBuf<12> {
    let mut b = FixedBuf::new();
    let _ = write!(
        b,
        "{}{}{:02}",
        v / 1000,
        locale::decimal_sep(i18n::lang()),
        v % 1000 / 10
    );
    b
}
