use super::*;
use alloc::string::String;
use alloc::vec::Vec;

fn feed(c: &mut Calc, s: &str) {
    for b in s.bytes() {
        c.input(b);
    }
}

fn shown(c: &Calc) -> &str {
    core::str::from_utf8(c.display()).unwrap()
}

#[test]
fn starts_at_zero() {
    assert_eq!(shown(&Calc::new()), "0");
}

#[test]
fn types_digits_replacing_leading_zero() {
    let mut c = Calc::new();
    feed(&mut c, "42");
    assert_eq!(shown(&c), "42");
}

#[test]
fn simple_addition() {
    let mut c = Calc::new();
    feed(&mut c, "2+3=");
    assert_eq!(shown(&c), "5");
}

#[test]
fn subtraction_and_multiplication() {
    let mut c = Calc::new();
    feed(&mut c, "9-4=");
    assert_eq!(shown(&c), "5");
    c.clear();
    feed(&mut c, "6*7=");
    assert_eq!(shown(&c), "42");
}

#[test]
fn chained_operations_fold_left_to_right() {
    let mut c = Calc::new();
    feed(&mut c, "2+3*4="); // (2+3)*4
    assert_eq!(shown(&c), "20");
}

#[test]
fn division_with_decimal_result() {
    let mut c = Calc::new();
    feed(&mut c, "10/4=");
    assert_eq!(shown(&c), "2.5");
}

#[test]
fn decimal_entry() {
    let mut c = Calc::new();
    feed(&mut c, "1.5+2.5=");
    assert_eq!(shown(&c), "4");
}

#[test]
fn only_one_dot_allowed() {
    let mut c = Calc::new();
    feed(&mut c, "1.2.3");
    assert_eq!(shown(&c), "1.23");
}

#[test]
fn leading_dot_becomes_zero_dot() {
    let mut c = Calc::new();
    feed(&mut c, ".5=");
    assert_eq!(shown(&c), "0.5");
}

#[test]
fn divide_by_zero_is_error() {
    let mut c = Calc::new();
    feed(&mut c, "5/0=");
    assert!(c.is_error());
    assert_eq!(shown(&c), "ERROR");
}

#[test]
fn digit_after_error_starts_fresh() {
    let mut c = Calc::new();
    feed(&mut c, "5/0=");
    assert!(c.is_error());
    feed(&mut c, "7");
    assert_eq!(shown(&c), "7");
    assert!(!c.is_error());
}

#[test]
fn clear_resets() {
    let mut c = Calc::new();
    feed(&mut c, "123+45");
    c.clear();
    assert_eq!(shown(&c), "0");
    assert_eq!(c.operator(), None);
}

#[test]
fn backspace_removes_last_char() {
    let mut c = Calc::new();
    feed(&mut c, "123");
    c.backspace();
    assert_eq!(shown(&c), "12");
    c.backspace();
    c.backspace();
    assert_eq!(shown(&c), "0");
}

#[test]
fn negative_result() {
    let mut c = Calc::new();
    feed(&mut c, "3-8=");
    assert_eq!(shown(&c), "-5");
}

#[test]
fn operator_is_exposed_while_pending() {
    let mut c = Calc::new();
    feed(&mut c, "7+");
    assert_eq!(c.operator(), Some(b'+'));
}

#[test]
fn continue_after_equals_uses_result() {
    let mut c = Calc::new();
    feed(&mut c, "2+3=");
    feed(&mut c, "*2=");
    assert_eq!(shown(&c), "10");
}

#[test]
fn overflow_is_error() {
    let mut c = Calc::new();
    feed(&mut c, "999999999*999999=");
    assert!(c.is_error());
}

#[test]
fn parse_decimal_roundtrip() {
    assert!((parse_decimal(b"3.25") - 3.25).abs() < 1e-9);
    assert!((parse_decimal(b"-12") + 12.0).abs() < 1e-9);
    assert!((parse_decimal(b"0") - 0.0).abs() < 1e-9);
}

#[test]
fn percent_of_the_left_operand_for_plus_and_minus() {
    let mut c = Calc::new();
    feed(&mut c, "200+10%");
    assert_eq!(shown(&c), "20");
    feed(&mut c, "=");
    assert_eq!(shown(&c), "220");
    let mut c = Calc::new();
    feed(&mut c, "200-25%=");
    assert_eq!(shown(&c), "150");
}

#[test]
fn percent_alone_and_with_times_divides_by_100() {
    let mut c = Calc::new();
    feed(&mut c, "50%");
    assert_eq!(shown(&c), "0.5");
    let mut c = Calc::new();
    feed(&mut c, "80*50%=");
    assert_eq!(shown(&c), "40");
}

#[test]
fn negate_toggles_the_sign() {
    let mut c = Calc::new();
    feed(&mut c, "n");
    assert_eq!(shown(&c), "0");
    feed(&mut c, "5n");
    assert_eq!(shown(&c), "-5");
    feed(&mut c, "n");
    assert_eq!(shown(&c), "5");
    feed(&mut c, "n+3=");
    assert_eq!(shown(&c), "-2");
    // The result of an operation can be negated too.
    feed(&mut c, "n");
    assert_eq!(shown(&c), "2");
    // Full entry: no overflow.
    let mut c = Calc::new();
    feed(&mut c, "1234567890123456n");
    assert_eq!(c.display().len(), 16);
}

#[test]
fn memory_keys_store_add_subtract_recall_and_clear() {
    let mut c = Calc::new();
    assert!(!c.has_memory());
    c.input(KEY_MR);
    assert_eq!(shown(&c), "0");
    feed(&mut c, "12");
    c.input(KEY_MADD);
    assert!(c.has_memory());
    feed(&mut c, "5");
    c.input(KEY_MSUB);
    feed(&mut c, "99");
    c.input(KEY_MR);
    assert_eq!(shown(&c), "7");
    // Clear leaves the memory alone; MC empties it.
    c.clear();
    assert!(c.has_memory());
    c.input(KEY_MR);
    assert_eq!(shown(&c), "7");
    c.input(KEY_MC);
    assert!(!c.has_memory());
    c.input(KEY_MR);
    assert_eq!(shown(&c), "7"); // nothing stored: the entry stays
}

#[test]
fn history_keeps_the_last_four_operations() {
    let mut c = Calc::new();
    assert_eq!(c.history().count(), 0);
    for (a, b) in [(1, 1), (2, 2), (3, 3), (4, 4), (5, 5)] {
        feed(&mut c, &alloc::format!("{a}+{b}="));
    }
    let h: Vec<String> = c
        .history()
        .map(|l| String::from_utf8_lossy(l).into_owned())
        .collect();
    assert_eq!(h, ["2 + 2 = 4", "3 + 3 = 6", "4 + 4 = 8", "5 + 5 = 10"]);
    // A chained operation records the partial result.
    let mut c = Calc::new();
    feed(&mut c, "2+3*4=");
    let h: Vec<&[u8]> = c.history().collect();
    assert_eq!(h, [&b"2 + 3 = 5"[..], b"5 * 4 = 20"]);
    // A failed operation records nothing.
    let mut c = Calc::new();
    feed(&mut c, "5/0=");
    assert_eq!(c.history().count(), 0);
    // Clear forgets the history.
    feed(&mut c, "1+1=");
    c.clear();
    assert_eq!(c.history().count(), 0);
}

#[test]
fn pending_expression_is_shown() {
    let mut c = Calc::new();
    assert!(c.pending_text().is_none());
    feed(&mut c, "12*");
    let (t, n) = c.pending_text().unwrap();
    assert_eq!(&t[..n], b"12 *");
}

#[test]
fn comma_is_a_decimal_point_and_tiny_negatives_lose_their_sign() {
    let mut c = Calc::new();
    feed(&mut c, "1,5+1=");
    assert_eq!(shown(&c), "2.5");
    let mut c = Calc::new();
    feed(&mut c, "0.0000001n=");
    c.input(b'+');
    feed(&mut c, "0=");
    assert_eq!(shown(&c), "0");
}

#[test]
fn pretty_groups_thousands_with_a_decimal_comma() {
    let _lang = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::Pt);
    assert_eq!(pretty(b"0"), "0");
    assert_eq!(pretty(b"999"), "999");
    assert_eq!(pretty(b"1000"), "1.000");
    assert_eq!(pretty(b"-1234567.5"), "-1.234.567,5");
    assert_eq!(pretty(b"5."), "5,");
    assert_eq!(pretty(b"0.25"), "0,25");
    assert_eq!(pretty(b"ERROR"), "Erro");
    assert_eq!(pretty(b""), "");
    assert_eq!(pretty(b"abc"), "abc");
    assert_eq!(pretty(b"12.3.4"), "12.3.4");
}

#[test]
fn pretty_follows_the_language() {
    let _lang = crate::i18n::testlang::LangGuard::new(crate::i18n::Lang::En);
    assert_eq!(pretty(b"1000"), "1,000");
    assert_eq!(pretty(b"-1234567.5"), "-1,234,567.5");
    assert_eq!(pretty(b"5."), "5.");
    assert_eq!(pretty(b"0.25"), "0.25");
    assert_eq!(pretty(b"ERROR"), "Error");
}
