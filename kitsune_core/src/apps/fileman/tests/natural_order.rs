use super::*;

#[test]
fn natural_order_compares_numbers_by_value() {
    assert_eq!(natural_cmp(b"f2", b"f10"), Ordering::Less);
    assert_eq!(natural_cmp(b"f10", b"f2"), Ordering::Greater);
    assert_eq!(natural_cmp(b"f2", b"f2"), Ordering::Equal);
    assert_eq!(natural_cmp(b"a1b", b"a1c"), Ordering::Less);
}

#[test]
fn natural_order_ignores_case_but_is_total() {
    assert_eq!(natural_cmp(b"apple", b"Banana"), Ordering::Less);
    assert_ne!(natural_cmp(b"abc", b"ABC"), Ordering::Equal);
    assert_eq!(
        natural_cmp(b"abc", b"ABC").reverse(),
        natural_cmp(b"ABC", b"abc")
    );
}

#[test]
fn natural_order_leading_zeros_and_prefixes() {
    assert_eq!(natural_cmp(b"a01", b"a1"), Ordering::Greater);
    assert_eq!(natural_cmp(b"a1", b"a01"), Ordering::Less);
    assert_eq!(natural_cmp(b"a007", b"a8"), Ordering::Less);
    assert_eq!(natural_cmp(b"a", b"ab"), Ordering::Less);
    assert_eq!(natural_cmp(b"", b"a"), Ordering::Less);
    assert_eq!(natural_cmp(b"000", b"00"), Ordering::Greater);
}

#[test]
fn natural_order_handles_huge_digit_runs() {
    let a = [b'9'; 40];
    let mut b = vec![b'1'];
    b.extend_from_slice(&[b'0'; 40]);
    assert_eq!(natural_cmp(&a, &b), Ordering::Less);
}
