use super::*;
use crate::apps::shell::fs::MemFs;

fn pat(s: &str) -> Vec<PatChar> {
    s.chars().map(|c| (c, c == '*' || c == '?')).collect()
}

fn ev(s: &str) -> i64 {
    eval_arith(s, &|n| if n == "x" { 7 } else { 0 }).unwrap()
}

#[test]
fn segment_matching() {
    assert!(match_segment(&pat("*.txt"), "a.txt"));
    assert!(!match_segment(&pat("*.txt"), "a.txt.bak"));
    assert!(match_segment(&pat("a?c"), "abc"));
    assert!(!match_segment(&pat("a?c"), "ac"));
    assert!(match_segment(&pat("*"), ""));
    assert!(match_segment(&pat("a*b*c"), "aXXbYYc"));
    assert!(!match_segment(&pat("a*b*c"), "aXXbYY"));
    assert!(match_segment(&pat("**"), "anything"));
}

#[test]
fn dotfiles_need_explicit_dot() {
    assert!(!match_segment(&pat("*"), ".hidden"));
    assert!(match_segment(&pat(".*"), ".hidden"));
    assert!(!match_segment(&pat("?hidden"), ".hidden"));
}

#[test]
fn quoted_wildcards_are_literal() {
    let p: Vec<PatChar> = "a*".chars().map(|c| (c, false)).collect();
    assert!(match_segment(&p, "a*"));
    assert!(!match_segment(&p, "ab"));
}

fn sample() -> MemFs {
    MemFs::new()
        .with_file("/a.txt", b"")
        .with_file("/b.txt", b"")
        .with_file("/c.md", b"")
        .with_file("/.hidden", b"")
        .with_dir("/d")
        .with_file("/d/x.txt", b"")
        .with_file("/d/y.md", b"")
        .with_dir("/e")
        .with_file("/e/z.txt", b"")
}

#[test]
fn expand_simple_star() {
    let fs = sample();
    assert_eq!(expand(&fs, &pat("*.txt"), 100), ["a.txt", "b.txt"]);
    assert_eq!(expand(&fs, &pat("/*.md"), 100), ["/c.md"]);
    assert_eq!(expand(&fs, &pat("?.md"), 100), ["c.md"]);
}

#[test]
fn expand_across_directories() {
    let fs = sample();
    assert_eq!(expand(&fs, &pat("*/*.txt"), 100), ["d/x.txt", "e/z.txt"]);
    assert_eq!(expand(&fs, &pat("/d/*"), 100), ["/d/x.txt", "/d/y.md"]);
    assert_eq!(expand(&fs, &pat("d/../d/*.md"), 100), ["d/../d/y.md"]);
}

#[test]
fn expand_dirs_only_with_trailing_slash() {
    let fs = sample();
    assert_eq!(expand(&fs, &pat("*/"), 100), ["d/", "e/"]);
}

#[test]
fn expand_no_match_is_empty() {
    let fs = sample();
    assert!(expand(&fs, &pat("*.zzz"), 100).is_empty());
    assert!(expand(&fs, &pat("nodir/*"), 100).is_empty());
    assert!(expand(&fs, &pat("a.txt/*"), 100).is_empty());
}

#[test]
fn expand_respects_limit_and_cwd() {
    let mut fs = sample();
    assert_eq!(expand(&fs, &pat("*"), 2).len(), 2);
    fs.set_cwd("/d").unwrap();
    assert_eq!(expand(&fs, &pat("*.txt"), 10), ["x.txt"]);
    assert_eq!(expand(&fs, &pat("../*.md"), 10), ["../c.md"]);
}

#[test]
fn arithmetic_basics() {
    assert_eq!(ev("1 + 2 * 3"), 7);
    assert_eq!(ev("(1 + 2) * 3"), 9);
    assert_eq!(ev("10 / 3"), 3);
    assert_eq!(ev("10 % 3"), 1);
    assert_eq!(ev("-5 + 2"), -3);
    assert_eq!(ev("x * 2"), 14);
    assert_eq!(ev("unknown + 1"), 1);
    assert_eq!(ev(""), 0);
    assert_eq!(ev("  42  "), 42);
}

#[test]
fn arithmetic_logic() {
    assert_eq!(ev("3 < 4"), 1);
    assert_eq!(ev("3 >= 4"), 0);
    assert_eq!(ev("1 == 1 && 2 != 3"), 1);
    assert_eq!(ev("0 || 0"), 0);
    assert_eq!(ev("!0"), 1);
    assert_eq!(ev("!5"), 0);
}

#[test]
fn arithmetic_errors() {
    let z = |_: &str| 0;
    assert_eq!(eval_arith("1 / 0", &z), Err(ArithErr::DivZero));
    assert_eq!(eval_arith("5 % 0", &z), Err(ArithErr::DivZero));
    assert_eq!(eval_arith("1 +", &z), Err(ArithErr::Syntax));
    assert_eq!(eval_arith("(1", &z), Err(ArithErr::Syntax));
    assert_eq!(eval_arith("1 2", &z), Err(ArithErr::Syntax));
    let deep = alloc::format!("{}1{}", "(".repeat(500), ")".repeat(500));
    assert_eq!(eval_arith(&deep, &z), Err(ArithErr::TooDeep));
}

#[test]
fn arithmetic_wraps_on_overflow() {
    let z = |_: &str| 0;
    assert!(eval_arith("9223372036854775807 + 1", &z).is_ok());
    assert!(eval_arith("-9223372036854775807 - 1 / -1", &z).is_ok());
    let min = "(0-9223372036854775807-1)";
    assert!(eval_arith(&alloc::format!("{min} / -1"), &z).is_ok());
    assert!(eval_arith(&alloc::format!("{min} % -1"), &z).is_ok());
}
