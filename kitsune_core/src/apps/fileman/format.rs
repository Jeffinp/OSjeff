//! format (split out of `fileman.rs`).

use super::*;

/// `"512 B"`, `"1,5 KiB"` (pt) / `"1.5 KiB"` (en): one decimal, binary units, the language's
/// decimal separator (see [`crate::i18n::format_size`]).
pub fn format_size(bytes: u64) -> String {
    crate::i18n::format_size(bytes)
}

/// Days since 1970-01-01 to `(year, month, day)` (proleptic Gregorian).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A Unix time shifted by `tz_secs` (local time) as the language writes a date and a time
/// (`08/10/2026 23:49` / `10/08/2026 11:49 PM`; `clock24` picks the clock). `0` (no clock when
/// the file was written) shows `--`.
pub fn format_datetime(unix: u64, tz_secs: i32, clock24: bool) -> String {
    if unix == 0 {
        return String::from("--");
    }
    let t = unix as i64 + tz_secs as i64;
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let civil = crate::i18n::Civil {
        year: y.clamp(0, 9999) as i32,
        month: m as u8,
        day: d as u8,
        // 1970-01-01 was a Thursday.
        weekday: (days + 4).rem_euclid(7) as u8,
        hour: (secs / 3600) as u8,
        minute: ((secs % 3600) / 60) as u8,
        second: (secs % 60) as u8,
    };
    crate::t!(
        "files.when.datetime",
        date = &crate::i18n::format_date(civil, crate::i18n::DateStyle::Short, clock24),
        time = &crate::i18n::format_time(civil, clock24, false)
    )
}

/// Fold a UTF-8 name to what the ASCII bitmap font can draw: accented Latin letters
/// become their base letter, any other non-ASCII character becomes one `?`, control
/// bytes become `?`. Invalid UTF-8 bytes become `?` too. Never longer than the input.
pub fn display_ascii(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len());
    let mut i = 0;
    while i < name.len() {
        let b = name[i];
        if b < 0x80 {
            out.push(if b < 0x20 || b == 0x7F { b'?' } else { b });
            i += 1;
            continue;
        }
        let len = match b {
            0xC2..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF4 => 4,
            _ => 1,
        };
        let end = (i + len).min(name.len());
        let ch = core::str::from_utf8(&name[i..end])
            .ok()
            .and_then(|s| s.chars().next());
        out.push(ch.map_or(b'?', fold_char));
        i = end.max(i + 1);
    }
    out
}

pub(super) fn fold_char(c: char) -> u8 {
    match c {
        'à'..='å' => b'a',
        'À'..='Å' => b'A',
        'ç' => b'c',
        'Ç' => b'C',
        'è'..='ë' => b'e',
        'È'..='Ë' => b'E',
        'ì'..='ï' => b'i',
        'Ì'..='Ï' => b'I',
        'ñ' => b'n',
        'Ñ' => b'N',
        'ò'..='ö' | 'ø' => b'o',
        'Ò'..='Ö' | 'Ø' => b'O',
        'ù'..='ü' => b'u',
        'Ù'..='Ü' => b'U',
        'ý' | 'ÿ' => b'y',
        'Ý' => b'Y',
        _ => b'?',
    }
}

/// `text` cut to `max` columns with a trailing `...` when it does not fit.
pub fn ellipsize(text: &[u8], max: usize) -> Vec<u8> {
    if text.len() <= max {
        return text.to_vec();
    }
    if max <= 3 {
        return text[..max].to_vec();
    }
    let mut out = text[..max - 3].to_vec();
    out.extend_from_slice(b"...");
    out
}
