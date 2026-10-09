use super::*;

/// 8 px per character, 2 px kern between 'A' and 'V'.
struct Fake;
impl Metrics for Fake {
    fn advance_q8(&self, c: char) -> i32 {
        if c == '\u{2026}' { 12 * Q } else { 8 * Q }
    }
    fn kern_q8(&self, l: char, r: char) -> i32 {
        if (l, r) == ('A', 'V') { -2 * Q } else { 0 }
    }
}

#[test]
fn measure_adds_advances_and_kerning() {
    assert_eq!(measure(&Fake, ""), 0);
    assert_eq!(measure(&Fake, "abc"), 24);
    assert_eq!(measure(&Fake, "AV"), 14);
    assert_eq!(measure_q8(&Fake, "AV"), 14 * Q);
}

#[test]
fn fit_prefix_is_exact() {
    assert_eq!(fit_prefix(&Fake, "abcdef", 24), 3);
    assert_eq!(fit_prefix(&Fake, "abcdef", 23), 2);
    assert_eq!(fit_prefix(&Fake, "abcdef", 1000), 6);
    assert_eq!(fit_prefix(&Fake, "abcdef", 0), 0);
    assert_eq!(fit_prefix(&Fake, "abcdef", -5), 0);
    // Multi-byte characters never split.
    assert_eq!(fit_prefix(&Fake, "çãõ", 16), "çã".len());
}

#[test]
fn ellipsize_fits_and_marks() {
    let (s, cut) = ellipsize(&Fake, "hello world", 200);
    assert_eq!((s.as_str(), cut), ("hello world", false));
    let (s, cut) = ellipsize(&Fake, "hello world", 50);
    assert!(cut);
    assert!(s.ends_with('\u{2026}'));
    assert!(measure(&Fake, &s) <= 50, "{s:?} {}", measure(&Fake, &s));
    assert_eq!(s, "hell\u{2026}");
    // No room for the ellipsis itself.
    assert_eq!(ellipsize(&Fake, "hello", 8).0, "");
}

#[test]
fn ellipsize_never_exceeds_the_width() {
    for w in 0..120 {
        let (s, _) = ellipsize(&Fake, "The quick brown fox", w);
        assert!(measure(&Fake, &s) <= w.max(0), "w={w} {s:?}");
    }
}

#[test]
fn middle_ellipsis_keeps_the_extension() {
    let s = ellipsize_middle(&Fake, "relatorio-final-v2.pdf", 120);
    assert!(s.contains('\u{2026}') && s.ends_with(".pdf"), "{s:?}");
    assert!(measure(&Fake, &s) <= 120, "{}", measure(&Fake, &s));
    assert_eq!(ellipsize_middle(&Fake, "a.txt", 120), "a.txt");
}

#[test]
fn wrap_breaks_on_spaces_and_newlines() {
    let t = "one two three\nfour";
    let lines: Vec<&str> = wrap(&Fake, t, 8 * 8, 10)
        .into_iter()
        .map(|(a, b)| &t[a..b])
        .collect();
    assert_eq!(lines, ["one two", "three", "four"]);
    // A long word is split by characters.
    let t = "abcdefghijkl";
    let lines: Vec<&str> = wrap(&Fake, t, 40, 10)
        .into_iter()
        .map(|(a, b)| &t[a..b])
        .collect();
    assert_eq!(lines, ["abcde", "fghij", "kl"]);
    assert_eq!(wrap(&Fake, "a b c d e f g h", 16, 2).len(), 2);
}
