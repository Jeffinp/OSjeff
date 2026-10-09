use super::*;

#[test]
fn dates_follow_the_order_of_the_language() {
    assert_eq!(format_date_in(Lang::Pt, 0), "01/01/1970");
    assert_eq!(format_date_in(Lang::Pt, 1_700_000_000), "14/11/2023");
    assert_eq!(format_date_in(Lang::En, 1_700_000_000), "11/14/2023");
    let _ = format_date_in(Lang::En, u64::MAX);
    let _ = format_date(u64::MAX);
}

#[test]
fn garbage_is_not_a_certificate() {
    assert!(CertInfo::from_leaf(b"", "a.test", 1, None).is_none());
    assert!(CertInfo::from_leaf(&[0x30, 0x03, 1, 2, 3], "a.test", 1, None).is_none());
}

#[test]
fn names_are_cleaned_and_bounded() {
    assert_eq!(clean(b"a\x00b\nc"), "abc");
    let long = "é".repeat(100);
    let c = clean(long.as_bytes());
    assert!(c.len() <= MAX_NAME && c.chars().all(|ch| ch == 'é'));
}
