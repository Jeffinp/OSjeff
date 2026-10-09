use super::*;

#[test]
fn sizes_are_formatted_with_binary_units() {
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(1023), "1023 B");
    assert_eq!(format_size(1024), "1,0 KiB");
    assert_eq!(format_size(1536), "1,5 KiB");
    assert_eq!(format_size(3 * 1024 * 1024), "3,0 MiB");
    assert_eq!(format_size(1024 * 1024 - 1), "1023,9 KiB");
    assert_eq!(
        format_size(5 * 1024 * 1024 * 1024 + 512 * 1024 * 1024),
        "5,5 GiB"
    );
    assert_eq!(format_size(u64::MAX), "16777215,9 TiB");
}

#[test]
fn civil_dates() {
    assert_eq!(civil_from_days(0), (1970, 1, 1));
    assert_eq!(civil_from_days(59), (1970, 3, 1));
    assert_eq!(civil_from_days(10_957), (2000, 1, 1));
    assert_eq!(civil_from_days(11_016), (2000, 2, 29)); // leap day
    assert_eq!(civil_from_days(-1), (1969, 12, 31));
}

#[test]
fn datetimes_are_local() {
    let _g = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    assert_eq!(format_datetime(0, 0, true), "--");
    assert_eq!(format_datetime(1_700_000_000, 0, true), "14/11/2023 22:13");
    assert_eq!(
        format_datetime(1_700_000_000, -3 * 3600, true),
        "14/11/2023 19:13"
    );
    // Crossing midnight backwards.
    assert_eq!(
        format_datetime(86_400 + 60, -3600, true),
        "01/01/1970 23:01"
    );
}

#[test]
fn datetimes_follow_the_language_and_the_clock() {
    use crate::i18n::{Lang, testlang::LangGuard};
    let t = 1_700_000_000; // 2023-11-14 22:13 UTC, a Tuesday
    {
        let _g = LangGuard::new(Lang::En);
        assert_eq!(format_datetime(t, 0, false), "11/14/2023 10:13 PM");
        assert_eq!(format_datetime(t, 0, true), "11/14/2023 22:13");
        assert_eq!(format_datetime(0, 0, false), "--");
        // 1970-01-01 00:01 local (the first minute of the clock's epoch) keeps its AM.
        assert_eq!(format_datetime(60, 0, false), "01/01/1970 12:01 AM");
    }
    let _g = LangGuard::new(Lang::Pt);
    assert_eq!(format_datetime(t, 0, false), "14/11/2023 10:13 PM");
    assert_eq!(format_datetime(t, 0, true), "14/11/2023 22:13");
}

#[test]
fn sizes_follow_the_language() {
    use crate::i18n::{Lang, testlang::LangGuard};
    let big = 1234 * 1024 + 512 * 1024 / 10; // 1234,05 KiB -> 1,2 MiB
    {
        let _g = LangGuard::new(Lang::En);
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(big), "1.2 MiB");
        assert_eq!(format_size(1024 * 1024 - 1), "1023.9 KiB");
    }
    let _g = LangGuard::new(Lang::Pt);
    assert_eq!(format_size(1536), "1,5 KiB");
    assert_eq!(format_size(big), "1,2 MiB");
}

#[test]
fn display_folds_accents_and_unknowns() {
    assert_eq!(display_ascii("relatório.txt".as_bytes()), b"relatorio.txt");
    assert_eq!(display_ascii("AÇÃO".as_bytes()), b"ACAO");
    assert_eq!(display_ascii("日本.png".as_bytes()), b"??.png");
    assert_eq!(display_ascii(&[b'a', 0xFF, b'b']), b"a?b");
    assert_eq!(display_ascii(b"a\x01b"), b"a?b");
    assert_eq!(display_ascii(&[0xE6, 0x97]), b"?"); // truncated sequence
    for name in ["ñandú", "über", "naïve café", "😀x"] {
        assert!(display_ascii(name.as_bytes()).len() <= name.len());
    }
}

#[test]
fn ellipsize_cuts_long_text() {
    assert_eq!(ellipsize(b"short", 10), b"short");
    assert_eq!(ellipsize(b"0123456789", 8), b"01234...");
    assert_eq!(ellipsize(b"0123456789", 3), b"012");
    assert_eq!(ellipsize(b"0123456789", 10), b"0123456789");
}
