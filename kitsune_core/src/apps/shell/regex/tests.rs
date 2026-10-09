use super::*;

fn m(p: &str, t: &str) -> bool {
    Regex::new(p, false).unwrap().is_match(t)
}

#[test]
fn literals_and_dot() {
    assert!(m("abc", "xxabcxx"));
    assert!(!m("abc", "ab"));
    assert!(m("a.c", "abc"));
    assert!(!m("a.c", "ac"));
    assert!(m("", "anything"));
}

#[test]
fn anchors() {
    assert!(m("^abc", "abcdef"));
    assert!(!m("^abc", "xabc"));
    assert!(m("def$", "abcdef"));
    assert!(!m("def$", "defx"));
    assert!(m("^$", ""));
    assert!(!m("^$", "x"));
}

#[test]
fn quantifiers() {
    assert!(m("ab*c", "ac"));
    assert!(m("ab*c", "abbbc"));
    assert!(m("ab+c", "abc"));
    assert!(!m("ab+c", "ac"));
    assert!(m("ab?c", "ac"));
    assert!(m("^a.*z$", "a123z"));
}

#[test]
fn classes() {
    assert!(m("[abc]x", "bx"));
    assert!(!m("[abc]x", "dx"));
    assert!(m("[a-c]+$", "abcabc"));
    assert!(m("[^0-9]", "a"));
    assert!(!m("^[^0-9]+$", "ab1"));
    assert!(m("[]x]", "]"));
    assert!(m("[a-]", "-"));
}

#[test]
fn escapes() {
    assert!(m("a\\.b", "a.b"));
    assert!(!m("a\\.b", "axb"));
    assert!(m("\\d+", "abc123"));
    assert!(m("^\\w+$", "foo_bar1"));
    assert!(m("a\\sb", "a b"));
    assert!(m("\\(x\\)", "(x)"));
}

#[test]
fn alternation_and_groups() {
    assert!(m("cat|dog", "hotdog"));
    assert!(!m("cat|dog", "bird"));
    assert!(m("^(ab)+$", "ababab"));
    assert!(!m("^(ab)+$", "aba"));
    assert!(m("gr(a|e)y", "grey"));
    assert!(m("a|b|c", "c"));
}

#[test]
fn case_insensitive() {
    let r = Regex::new("hello", true).unwrap();
    assert!(r.is_match("Say HeLLo"));
    let r = Regex::new("[a-c]x", true).unwrap();
    assert!(r.is_match("BX"));
    assert!(!Regex::new("hello", false).unwrap().is_match("HELLO"));
}

#[test]
fn unicode_text() {
    assert!(m("ñ.", "añb"));
    assert!(m("^.$", "€"));
}

#[test]
fn syntax_errors() {
    assert_eq!(
        Regex::new("(a", false).unwrap_err(),
        RegexErr::UnbalancedParen
    );
    assert_eq!(
        Regex::new("a)", false).unwrap_err(),
        RegexErr::UnbalancedParen
    );
    assert_eq!(
        Regex::new("[a", false).unwrap_err(),
        RegexErr::UnterminatedClass
    );
    assert_eq!(
        Regex::new("*a", false).unwrap_err(),
        RegexErr::NothingToRepeat
    );
    assert_eq!(
        Regex::new("a\\", false).unwrap_err(),
        RegexErr::TrailingBackslash
    );
    assert_eq!(
        Regex::new(&"a".repeat(2000), false).unwrap_err(),
        RegexErr::TooLong
    );
    for e in [
        RegexErr::UnbalancedParen,
        RegexErr::UnterminatedClass,
        RegexErr::NothingToRepeat,
        RegexErr::TrailingBackslash,
        RegexErr::TooLong,
    ] {
        assert!(!e.message().is_empty());
    }
}

#[test]
fn pathological_pattern_terminates() {
    let r = Regex::new("(a*)*b", false).unwrap();
    let text = "a".repeat(5000);
    let _ = r.is_match(&text);
    let r = Regex::new("(a|aa)+$", false).unwrap();
    let _ = r.is_match(&alloc::format!("{}b", "a".repeat(5000)));
}

#[test]
fn deep_group_nesting_is_rejected_not_overflowed() {
    let p = alloc::format!("{}a{}", "(".repeat(100), ")".repeat(100));
    assert_eq!(Regex::new(&p, false).unwrap_err(), RegexErr::TooLong);
}
