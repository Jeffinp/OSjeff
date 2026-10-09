use super::*;

#[test]
fn renders_heading_and_paragraph_at_real_sizes() {
    let p = lay("<h1>Title</h1><p>Hello world</p>", 600);
    assert_eq!(font_of(&p, "Title").size, 32);
    assert!(font_of(&p, "Title").bold);
    assert_eq!(font_of(&p, "Hello world").size, 16);
    assert!(p.height > 0);
}

#[test]
fn heading_sizes_follow_the_ua_scale() {
    let p = lay(
        "<h1>a</h1><h2>b</h2><h3>c</h3><h4>d</h4><h5>e</h5><h6>f</h6>",
        600,
    );
    let sizes: Vec<u16> = ["a", "b", "c", "d", "e", "f"]
        .iter()
        .map(|n| font_of(&p, n).size)
        .collect();
    assert_eq!(sizes, [32, 24, 19, 16, 13, 11]);
}

#[test]
fn small_and_big_change_the_size() {
    let p = lay("<p>x <small>sm</small> <big>bg</big></p>", 600);
    assert_eq!(font_of(&p, "sm").size, 13);
    assert_eq!(font_of(&p, "bg").size, 19);
}

#[test]
fn words_of_one_style_merge_into_a_run() {
    let p = lay("<p>one two three</p>", 600);
    assert_eq!(texts(&p), ["one two three"]);
}

#[test]
fn a_style_change_splits_the_run_and_keeps_the_space() {
    let p = lay("<p>one <b>two</b> three</p>", 600);
    assert_eq!(texts(&p), ["one", "two", "three"]);
    // 8 px per char, a space between each: one(24) + 8 | two bold (9 per char) | ...
    let x1 = x_of(&p, "one");
    let x2 = x_of(&p, "two");
    let x3 = x_of(&p, "three");
    assert_eq!(x2 - x1, 3 * 8 + 8);
    assert_eq!(x3 - x2, 3 * 9 + 8);
}

#[test]
fn no_space_is_invented_between_adjacent_elements() {
    // "foo<b>bar</b>" is one word visually: no gap, and punctuation hugs the link.
    let p = lay("<p>foo<b>bar</b>baz <a href=x>link</a>.</p>", 600);
    let foo = x_of(&p, "foo");
    let bar = x_of(&p, "bar");
    assert_eq!(bar - foo, 3 * 8);
    let dot = x_of(&p, ".");
    let link = x_of(&p, "link");
    assert_eq!(dot - link, 4 * 8, "the full stop touches the link");
}

#[test]
fn whitespace_between_inline_elements_is_one_space() {
    let p = lay("<p><b>a</b>\n   <i>b</i></p>", 600);
    assert_eq!(x_of(&p, "b") - x_of(&p, "a"), 9 + 8);
}

#[test]
fn text_wraps_at_the_measured_width() {
    // 8 px per character: "aaaa bbbb" is 72 px; in a 80 px column with 16 px of body margin
    // the available width is 64 px, so the words stack.
    let p = lay(
        "<body style='margin:0'><div style='width:64px'>aaaa bbbb cccc</div></body>",
        600,
    );
    let t = runs(&p);
    assert_eq!(t.len(), 3, "{t:?}");
    assert!(t[1].1 > t[0].1 && t[2].1 > t[1].1);
    // Exactly 8 chars fit: "aaaa bbbb" does not (9 chars = 72 px).
    let p = lay(
        "<body style='margin:0'><div style='width:72px'>aaaa bbbb cccc</div></body>",
        600,
    );
    assert_eq!(texts(&p), ["aaaa bbbb", "cccc"]);
}

#[test]
fn long_text_wraps_within_width() {
    let long = "word ".repeat(100);
    let p = lay(&format!("<p>{long}</p>"), 300);
    assert!(distinct_ys(&p).len() > 5);
    for c in &p.cmds {
        if let Cmd::Text { x, w, .. } = c {
            assert!(*x + *w <= 300, "run ends at {}", x + w);
        }
    }
}

#[test]
fn a_word_longer_than_the_line_is_cut_into_pieces() {
    let word = "x".repeat(100); // 800 px
    let p = lay(
        &format!("<body style='margin:0'><p style='margin:0'>{word}</p></body>"),
        200,
    );
    let t = runs(&p);
    assert!(t.len() >= 4, "{t:?}");
    assert_eq!(t.iter().map(|r| r.2.len()).sum::<usize>(), 100);
    for c in &p.cmds {
        if let Cmd::Text { x, w, .. } = c {
            assert!(*x + *w <= 200);
        }
    }
}

#[test]
fn pathological_words_stay_bounded() {
    let word = "y".repeat(300_000);
    let html = format!("<p>{word}</p><p>{word}</p>");
    let start = std::time::Instant::now();
    let p = lay(&html, 300);
    assert!(start.elapsed() < std::time::Duration::from_secs(20));
    let total: usize = runs(&p).iter().map(|r| r.2.chars().count()).sum();
    assert!(total <= 2 * MAX_TEXT_CHARS);
    assert!(p.cmds.len() <= MAX_CMDS);
    assert!(p.height > 0);
}

#[test]
fn one_enormous_word_is_cut_not_hung() {
    let p = lay(&format!("<p>{}</p>", "z".repeat(10_000)), 200);
    let total: usize = runs(&p).iter().map(|r| r.2.chars().count()).sum();
    assert!(total >= MAX_WORD_CHARS.min(10_000) - 1);
}

#[test]
fn cjk_breaks_between_any_two_characters() {
    // 20 ideographs = 160 px in a 100 px column.
    let cjk: String = std::iter::repeat_n('\u{4e2d}', 20).collect();
    let p = lay(
        &format!("<body style='margin:0'><p style='margin:0'>{cjk}</p></body>"),
        100,
    );
    let t = runs(&p);
    assert!(t.len() >= 2, "{t:?}");
    assert_eq!(t.iter().map(|r| r.2.chars().count()).sum::<usize>(), 20);
    for c in &p.cmds {
        if let Cmd::Text { x, w, .. } = c {
            assert!(*x + *w <= 100);
        }
    }
}

#[test]
fn mixed_latin_and_cjk_do_not_panic_and_keep_all_text() {
    let p = lay(
        "<p>abc \u{4e2d}\u{6587}def \u{1f600}\u{1f600} \u{0627}\u{0644}</p>",
        120,
    );
    let all: String = texts(&p).join(" ");
    assert!(all.contains("abc") && all.contains('\u{4e2d}') && all.contains('\u{1f600}'));
}

#[test]
fn zero_width_space_is_a_break_opportunity_and_invisible() {
    let p = lay(
        "<body style='margin:0'><p style='margin:0;width:40px'>abcd\u{200b}efgh</p></body>",
        600,
    );
    assert_eq!(texts(&p), ["abcd", "efgh"]);
}

#[test]
fn empty_and_whitespace_only_pages_have_no_text() {
    for html in [
        "",
        "   \n\t ",
        "<p></p>",
        "<div> </div>",
        "<p>\n</p>",
        "<br>",
    ] {
        let p = lay(html, 400);
        assert!(texts(&p).is_empty(), "{html:?}");
        assert!(p.height >= 0);
    }
}

#[test]
fn entities_and_unicode_text_survive() {
    let p = lay(
        "<p>caf&eacute; &mdash; a\u{e7}\u{e3}o &#x1F600; &nbsp;fim &amp; &lt;b&gt;</p>",
        600,
    );
    let t = texts(&p).join(" ");
    assert!(t.contains("caf\u{e9}"), "{t}");
    assert!(t.contains('\u{2014}') && t.contains("a\u{e7}\u{e3}o"));
    assert!(t.contains("& <b>"));
}

#[test]
fn invalid_utf8_pages_are_read_as_latin1() {
    let mut html = b"<p>a".to_vec();
    html.push(0xE7); // c-cedilla in Latin-1, invalid UTF-8
    html.push(0xE3);
    html.extend_from_slice(b"o</p>");
    let p = render(&html, 400);
    assert_eq!(texts(&p), ["a\u{e7}\u{e3}o"]);
}

#[test]
fn bidi_overrides_and_controls_are_stripped() {
    let p = lay("<p>a\u{202e}b\u{200f}c\u{0}d\u{7f}e</p>", 400);
    assert_eq!(texts(&p), ["abcde"]);
}

#[test]
fn br_breaks_lines_and_double_br_leaves_a_blank_line() {
    let p = lay("<p>one<br>two<br><br>three</p>", 600);
    assert_eq!(texts(&p), ["one", "two", "three"]);
    let ys: Vec<i32> = runs(&p).iter().map(|r| r.1).collect();
    assert_eq!(ys[1] - ys[0], 24);
    assert_eq!(ys[2] - ys[1], 48, "an empty line between");
}
