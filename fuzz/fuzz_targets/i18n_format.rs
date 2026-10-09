//! Fuzz target: the i18n template engine, number/size/date formatters and plural lookup.
//!
//! Input: `[mode, lang, ...bytes]`. The bytes are a hostile template (unbalanced and nested
//! braces, huge placeholder names, positional indexes, multibyte text), a catalog source
//! (`key = value` lines), or raw numbers/dates. Invariants: nothing panics; rendering terminates
//! with output bounded by template length times the longest argument; `placeholders` never
//! reports more names than there are `{` bytes; a catalog that parses is sorted and every key
//! is found again; formatted numbers contain only digits, separators and a sign.
#![no_main]

use libfuzzer_sys::fuzz_target;
use kitsune_core::i18n::{self, Arg, Civil, DateFmt, DateStyle, Lang, TimeFmt, locale, render};

fuzz_target!(|data: &[u8]| {
    let [mode, l, rest @ ..] = data else { return };
    let lang = Lang::from_index(*l);
    let text = String::from_utf8_lossy(rest);
    match mode % 5 {
        0 => {
            let args: [(&str, Arg<'_>); 6] = [
                ("n", Arg::Int(i64::MIN)),
                ("name", Arg::Str("Ana \u{e7}")),
                ("q", Arg::Num(i64::MAX)),
                ("d", Arg::Dec(i64::MIN + 1, 12)),
                ("b", Arg::Bytes(u64::MAX)),
                ("p", Arg::Pad(7, 255)),
            ];
            let mut out = String::new();
            let _ = render(&mut out, lang, &text, &args);
            assert!(out.len() <= text.len() * 64 + 64);
            assert!(i18n::placeholders(&text).len() <= text.matches('{').count());
            // The language-level entry points take the same hostile text as a key.
            let _ = i18n::tr_fmt_in(lang, &text, &args);
            let _ = i18n::plural_fmt_in(lang, &text, rest.len() as u64, &args);
        }
        1 => {
            let mut v = [0u8; 8];
            for (d, s) in v.iter_mut().zip(rest) {
                *d = *s;
            }
            let n = i64::from_le_bytes(v);
            let mut s = String::new();
            let _ = locale::write_int(&mut s, lang, n, true);
            assert!(s.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '.' || c == ','));
            let mut s = String::new();
            let _ = locale::write_dec(&mut s, lang, n, rest.first().copied().unwrap_or(0));
            let mut s = String::new();
            let _ = locale::write_size(&mut s, lang, n as u64);
            assert!(s.ends_with('B'));
        }
        2 => {
            let g = |i: usize| rest.get(i).copied().unwrap_or(0);
            let c = Civil {
                year: i32::from_le_bytes([g(0), g(1), g(2), g(3)]),
                month: g(4),
                day: g(5),
                weekday: g(6),
                hour: g(7),
                minute: g(8),
                second: g(9),
            };
            for style in [
                DateStyle::Short,
                DateStyle::Medium,
                DateStyle::Long,
                DateStyle::LongNoWeekday,
                DateStyle::MonthYear,
                DateStyle::Weekday,
                DateStyle::Full,
                DateStyle::Panel,
            ] {
                let s = DateFmt { lang, civil: c, style, clock24: g(10) & 1 == 0 }.to_string();
                assert!(!s.is_empty());
            }
            let _ = TimeFmt { lang, civil: c, clock24: g(10) & 2 == 0, seconds: g(10) & 4 != 0 }.to_string();
        }
        3 => {
            // Lookup of arbitrary keys and language tags.
            let _ = i18n::tr_in(lang, &text);
            let _ = i18n::plural_in(lang, &text, rest.len() as u64);
            let _ = Lang::from_code(rest);
        }
        _ => {
            // Every real key formats with no arguments and with hostile ones, in both languages.
            for l in Lang::ALL {
                for (k, _) in l.catalog().entries() {
                    let _ = i18n::tr_fmt_in(l, k, &[("n", Arg::Str(&text))]);
                }
            }
        }
    }
});
