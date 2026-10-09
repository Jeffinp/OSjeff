use super::*;

#[test]
fn fixed_advance_is_exact_and_deterministic() {
    let m = FixedAdvance;
    let f = Font::new(16);
    assert_eq!(m.width("abcd", f), 32);
    assert_eq!(m.width("", f), 0);
    let mono = Font { mono: true, ..f };
    assert_eq!(m.width("abcd", mono), 4 * ((16 * 3 + 2) / 5));
    let bold = Font { bold: true, ..f };
    assert!(m.width("abcd", bold) > m.width("abcd", f));
    assert_eq!(m.line_height(f), 19);
    assert_eq!(m.ascent(f), 16);
}

#[test]
fn zero_width_characters_take_no_room() {
    let m = FixedAdvance;
    let f = Font::new(10);
    assert_eq!(m.width("a\u{200d}b\u{fe0f}", f), 10);
    assert!(is_zero_width('\u{200b}'));
    assert!(!is_zero_width('a'));
}
