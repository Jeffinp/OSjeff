use super::*;

#[test]
fn find_is_case_insensitive_and_counts_matches() {
    let p = text_page();
    assert_eq!(p.find("the", &FixedAdvance).len(), 3);
    assert_eq!(p.find("THE", &FixedAdvance).len(), 3);
    assert_eq!(p.find("fox", &FixedAdvance).len(), 1);
    assert_eq!(p.find("zebra", &FixedAdvance).len(), 0);
    assert!(p.find("", &FixedAdvance).is_empty());
}

#[test]
fn find_matches_inside_a_word_with_a_precise_box() {
    let p = render(b"<p>abcdef</p>", 600);
    let m = p.find("cd", &FixedAdvance);
    assert_eq!(m.len(), 1);
    let s = m[0][0];
    assert_eq!(s.w, 2 * 8); // two characters of 8 px
    let word_x = p
        .cmds
        .iter()
        .find_map(|c| match c {
            Cmd::Text { x, .. } => Some(*x),
            _ => None,
        })
        .unwrap();
    assert_eq!(s.x, word_x + 2 * 8);
}

#[test]
fn find_inside_one_run_is_one_box() {
    let p = text_page();
    let m = p.find("quick brown", &FixedAdvance);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].len(), 1);
    assert_eq!(m[0][0].w, "quick brown".len() as i32 * 8);
}

#[test]
fn find_spans_lines() {
    let p = text_page();
    let m = p.find("fox jumps", &FixedAdvance);
    assert_eq!(m.len(), 1);
    assert!(m[0][1].y > m[0][0].y);
}

#[test]
fn find_matches_are_in_reading_order() {
    let p = text_page();
    let ys: Vec<i32> = p
        .find("the", &FixedAdvance)
        .iter()
        .map(|m| m[0].y)
        .collect();
    assert!(ys.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn find_does_not_overlap_matches() {
    let p = render(b"<p>aaaa</p>", 600);
    assert_eq!(p.find("aa", &FixedAdvance).len(), 2);
}

#[test]
fn find_with_a_huge_needle_or_page_is_bounded() {
    let p = text_page();
    assert!(p.find(&"x".repeat(500), &FixedAdvance).is_empty());
    let html = format!("<p>{}</p>", "a ".repeat(5000));
    let big = render(html.as_bytes(), 600);
    assert!(big.find("a", &FixedAdvance).len() <= super::textops::MAX_MATCHES);
}
