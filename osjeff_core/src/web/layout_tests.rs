//! Tests of the layout: proportional wrapping, styles, lists, blocks, hostile input.
//!
//! The metrics are [`FixedAdvance`]: at 16 px every character is 8 px wide, a
//! natural line is 19 px and the paragraph line-height (1.5) is 24 px, so the
//! expected numbers can be worked out by hand.

use super::imgcache::NoImages;
use super::*;

fn lay(html: &str, w: i32) -> Page {
    render(html.as_bytes(), w)
}

fn lay_zoom(html: &str, w: i32, zoom: u16) -> Page {
    Doc::parse(html.as_bytes()).layout(&Layout {
        width: w,
        zoom,
        images: &NoImages,
        metrics: &FixedAdvance,
    })
}

/// `(x, y, text)` of every text run.
fn runs(p: &Page) -> Vec<(i32, i32, String)> {
    p.cmds
        .iter()
        .filter_map(|c| match c {
            Cmd::Text { x, y, text, .. } => Some((*x, *y, text.clone())),
            _ => None,
        })
        .collect()
}

fn texts(p: &Page) -> Vec<String> {
    runs(p).into_iter().map(|r| r.2).collect()
}

fn run_named<'a>(p: &'a Page, name: &str) -> &'a Cmd {
    p.cmds
        .iter()
        .find(|c| matches!(c, Cmd::Text { text, .. } if text == name))
        .unwrap_or_else(|| panic!("no run {name:?} in {:?}", texts(p)))
}

fn font_of(p: &Page, name: &str) -> Font {
    match run_named(p, name) {
        Cmd::Text { font, .. } => *font,
        _ => unreachable!(),
    }
}

fn x_of(p: &Page, name: &str) -> i32 {
    match run_named(p, name) {
        Cmd::Text { x, .. } => *x,
        _ => unreachable!(),
    }
}

fn y_of(p: &Page, name: &str) -> i32 {
    match run_named(p, name) {
        Cmd::Text { y, .. } => *y,
        _ => unreachable!(),
    }
}

fn distinct_ys(p: &Page) -> Vec<i32> {
    let mut ys: Vec<i32> = runs(p).iter().map(|r| r.1).collect();
    ys.sort_unstable();
    ys.dedup();
    ys
}

// ---- text and wrapping ----

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

// ---- styles ----

#[test]
fn css_color_and_background_applied() {
    let p = lay(
        "<style>.hl{color:#ff0000} body{background:#102030}</style><p class=hl>red</p>",
        600,
    );
    assert!(p.cmds.iter().any(
        |c| matches!(c, Cmd::Text { text, color, .. } if text == "red" && *color == Rgb(255, 0, 0))
    ));
    assert_eq!(p.background, Rgb(0x10, 0x20, 0x30));
}

#[test]
fn the_page_background_defaults_to_white_and_html_wins() {
    assert_eq!(lay("<p>x</p>", 300).background, Rgb(255, 255, 255));
    let p = lay(
        "<html style='background:#111'><body style='background:#eee'>x",
        300,
    );
    assert_eq!(p.background, Rgb(0x11, 0x11, 0x11));
    // The canvas colour is not also painted as a box.
    assert!(!p.cmds.iter().any(|c| matches!(c, Cmd::Rect { .. })));
}

#[test]
fn block_background_border_and_radius_are_emitted() {
    let p = lay(
        "<body style='margin:0'><div style='background:#eee;border:2px solid #333;padding:10px;border-radius:8px'>x</div></body>",
        300,
    );
    let rect = p.cmds.iter().find_map(|c| match c {
        Cmd::Rect {
            x, y, w, h, radius, ..
        } => Some((*x, *y, *w, *h, *radius)),
        _ => None,
    });
    let (x, y, w, h, r) = rect.expect("background");
    assert_eq!((x, y, w), (0, 0, 300));
    assert_eq!(r, 8);
    // 2 border + 10 padding + a 24 px line + 10 + 2.
    assert_eq!(h, 2 + 10 + 24 + 10 + 2);
    assert!(p.cmds.iter().any(|c| matches!(
        c,
        Cmd::Border {
            widths: [2, 2, 2, 2],
            radius: 8,
            ..
        }
    )));
    // The text sits inside border + padding.
    assert_eq!(x_of(&p, "x"), 12);
}

#[test]
fn display_none_hides_content() {
    let p = lay("<p>shown</p><div style='display:none'>hidden</div>", 600);
    assert!(texts(&p).iter().any(|s| s == "shown"));
    assert!(!texts(&p).iter().any(|s| s == "hidden"));
}

#[test]
fn text_align_center_and_right() {
    let body = "<body style='margin:0'>";
    let p = lay(
        &format!("{body}<p style='margin:0;text-align:center'>abcd</p></body>"),
        200,
    );
    assert_eq!(x_of(&p, "abcd"), (200 - 32) / 2);
    let p = lay(
        &format!("{body}<p style='margin:0;text-align:right'>abcd</p></body>"),
        200,
    );
    assert_eq!(x_of(&p, "abcd"), 200 - 32);
    let p = lay(&format!("{body}<center>abcd</center></body>"), 200);
    assert_eq!(x_of(&p, "abcd"), (200 - 32) / 2);
}

#[test]
fn text_align_is_inherited_by_children() {
    let p = lay(
        "<body style='margin:0'><div style='text-align:center'><p style='margin:0'>ab</p></div></body>",
        100,
    );
    assert_eq!(x_of(&p, "ab"), (100 - 16) / 2);
}

#[test]
fn line_height_changes_the_line_pitch() {
    let one = |lh: &str| {
        let p = lay(
            &format!(
                "<body style='margin:0'><p style='margin:0;width:40px;line-height:{lh}'>aaaa bbbb</p></body>"
            ),
            600,
        );
        let ys = distinct_ys(&p);
        assert_eq!(ys.len(), 2);
        ys[1] - ys[0]
    };
    assert_eq!(one("1"), 16);
    assert_eq!(one("2"), 32);
    assert_eq!(one("30px"), 30);
    assert_eq!(one("150%"), 24);
}

#[test]
fn font_size_weight_and_style_from_css() {
    let p = lay(
        "<p style='font-size:20px;font-weight:700'>big</p><p style='font-style:italic'>it</p><p style='font-family:monospace'>mono</p>",
        600,
    );
    let f = font_of(&p, "big");
    assert!(f.size == 20 && f.bold && !f.italic);
    assert!(font_of(&p, "it").italic);
    assert!(font_of(&p, "mono").mono);
}

#[test]
fn nested_inline_styles_compose() {
    let p = lay(
        "<p>a <b>b <i>bi <u>biu</u></i></b> <span style='color:#00ff00'>g <s>gs</s></span></p>",
        600,
    );
    let f = font_of(&p, "bi");
    assert!(f.bold && f.italic);
    match run_named(&p, "biu") {
        Cmd::Text { font, deco, .. } => {
            assert!(font.bold && font.italic);
            assert_eq!(*deco & DECO_UNDERLINE, DECO_UNDERLINE);
        }
        _ => unreachable!(),
    }
    match run_named(&p, "gs") {
        Cmd::Text { color, deco, .. } => {
            assert_eq!(*color, Rgb(0, 255, 0));
            assert_eq!(*deco & DECO_STRIKE, DECO_STRIKE);
        }
        _ => unreachable!(),
    }
}

#[test]
fn code_and_pre_use_the_monospace_face() {
    let p = lay("<p>use <code>cargo</code></p><pre>a  b\n  c</pre>", 600);
    assert!(font_of(&p, "cargo").mono);
    // Preformatted text keeps its spaces and its line break.
    let t = texts(&p);
    assert!(t.contains(&String::from("a  b")), "{t:?}");
    assert!(t.contains(&String::from("  c")), "{t:?}");
    assert!(font_of(&p, "a  b").mono);
    // pre gets a box.
    assert!(p.cmds.iter().any(|c| matches!(
        c,
        Cmd::Rect {
            color: Rgb(0xf5, 0xf5, 0xf7),
            ..
        }
    )));
}

#[test]
fn a_leading_newline_in_pre_is_dropped_and_tabs_expand() {
    let p = lay("<pre>\nx\ty</pre>", 600);
    assert_eq!(texts(&p), ["x   y"]);
}

#[test]
fn long_pre_lines_wrap_instead_of_overflowing() {
    let line = "w".repeat(200);
    let p = lay(&format!("<pre>{line}</pre>"), 300);
    for c in &p.cmds {
        if let Cmd::Text { x, w, .. } = c {
            assert!(*x + *w <= 300);
        }
    }
    assert!(runs(&p).len() > 1);
}

#[test]
fn white_space_nowrap_keeps_words_together() {
    let p = lay(
        "<body style='margin:0'><p style='margin:0;width:40px;white-space:nowrap'>aaaa bbbb cccc</p></body>",
        600,
    );
    assert_eq!(distinct_ys(&p).len(), 1);
}

#[test]
fn text_transform_and_decoration() {
    let p = lay(
        "<p style='text-transform:uppercase'>abc</p><p style='text-decoration:underline'>u</p>",
        600,
    );
    assert!(texts(&p).contains(&String::from("ABC")));
    match run_named(&p, "u") {
        Cmd::Text { deco, .. } => assert_eq!(*deco & DECO_UNDERLINE, DECO_UNDERLINE),
        _ => unreachable!(),
    }
}

// ---- boxes ----

#[test]
fn vertical_margins_collapse_between_siblings() {
    // Two paragraphs with 1em (16px) margins sit 16 px apart plus the line, not 32.
    let p = lay("<body style='margin:0'><p>aa</p><p>bb</p></body>", 600);
    let gap = y_of(&p, "bb") - y_of(&p, "aa");
    assert_eq!(gap, 24 + 16);
    // The first margin collapses through the body too: the line starts 16 px down, and the
    // text's top is 2 px below it (strut 18 above the baseline, ascent 16).
    assert_eq!(y_of(&p, "aa"), 18);
}

#[test]
fn width_max_width_and_auto_margins_centre_a_column() {
    let p = lay(
        "<body style='margin:0'><div style='max-width:200px;margin:0 auto;background:#eee'>x</div></body>",
        600,
    );
    let r = p.cmds.iter().find_map(|c| match c {
        Cmd::Rect { x, w, .. } => Some((*x, *w)),
        _ => None,
    });
    assert_eq!(r, Some((200, 200)));
    let p = lay(
        "<body style='margin:0'><div style='width:50%;margin:0 auto;background:#eee'>x</div></body>",
        600,
    );
    let r = p.cmds.iter().find_map(|c| match c {
        Cmd::Rect { x, w, .. } => Some((*x, *w)),
        _ => None,
    });
    assert_eq!(r, Some((150, 300)));
}

#[test]
fn width_never_exceeds_the_container() {
    let p = lay(
        "<body style='margin:0'><div style='width:5000px;background:#eee'>x</div></body>",
        300,
    );
    let w = p.cmds.iter().find_map(|c| match c {
        Cmd::Rect { w, .. } => Some(*w),
        _ => None,
    });
    assert_eq!(w, Some(300));
}

#[test]
fn box_sizing_border_box_includes_padding() {
    let p = lay(
        "<body style='margin:0'><div style='box-sizing:border-box;width:200px;padding:20px;background:#eee'>x</div></body>",
        600,
    );
    let w = p.cmds.iter().find_map(|c| match c {
        Cmd::Rect { w, .. } => Some(*w),
        _ => None,
    });
    assert_eq!(w, Some(200));
    let p = lay(
        "<body style='margin:0'><div style='width:200px;padding:20px;background:#eee'>x</div></body>",
        600,
    );
    let w = p.cmds.iter().find_map(|c| match c {
        Cmd::Rect { w, .. } => Some(*w),
        _ => None,
    });
    assert_eq!(w, Some(240));
}

#[test]
fn hr_draws_a_rule() {
    let p = lay("<body style='margin:0'><p>a</p><hr><p>b</p></body>", 300);
    assert!(p.cmds.iter().any(|c| matches!(
        c,
        Cmd::Border {
            widths: [1, 0, 0, 0],
            w: 300,
            ..
        }
    )));
    assert!(y_of(&p, "b") > y_of(&p, "a") + 30);
}

#[test]
fn blockquote_has_a_bar_and_an_indent() {
    let p = lay(
        "<body style='margin:0'><blockquote>quoted</blockquote></body>",
        400,
    );
    assert!(p.cmds.iter().any(|c| matches!(
        c,
        Cmd::Border {
            widths: [0, 0, 0, 3],
            ..
        }
    )));
    assert_eq!(x_of(&p, "quoted"), 3 + 16);
}

#[test]
fn unordered_lists_get_bullets_and_indent() {
    let p = lay(
        "<body style='margin:0'><ul><li>one</li><li>two</li></ul></body>",
        400,
    );
    let bullets: Vec<_> = p
        .cmds
        .iter()
        .filter(|c| matches!(c, Cmd::Rect { radius, .. } if *radius > 0))
        .collect();
    assert_eq!(bullets.len(), 2);
    assert_eq!(x_of(&p, "one"), 28);
    assert!(y_of(&p, "two") > y_of(&p, "one"));
}

#[test]
fn nested_lists_change_the_marker() {
    let p = lay(
        "<ul><li>a<ul><li>b<ul><li>c</li></ul></li></ul></li></ul>",
        400,
    );
    assert!(
        p.cmds
            .iter()
            .any(|c| matches!(c, Cmd::Border { radius, .. } if *radius > 0)),
        "circle"
    );
    assert!(
        p.cmds
            .iter()
            .any(|c| matches!(c, Cmd::Rect { radius: 0, w, h, .. } if w == h && *w < 10)),
        "square"
    );
}

#[test]
fn ordered_lists_number_their_items() {
    let p = lay(
        "<ol start=3><li>a</li><li value=10>b</li><li>c</li></ol>",
        400,
    );
    let t = texts(&p);
    assert!(
        t.contains(&String::from("3."))
            && t.contains(&String::from("10."))
            && t.contains(&String::from("11.")),
        "{t:?}"
    );
    // The numbers are right-aligned just left of the text.
    let n3 = p.cmds.iter().find_map(|c| match c {
        Cmd::Text { text, x, w, .. } if text == "3." => Some(x + w),
        _ => None,
    });
    assert!(n3.unwrap() < x_of(&p, "a"));
    let p = lay(
        "<ol style='list-style-type:lower-roman'><li>a</li><li>b</li><li>c</li><li>d</li></ol>",
        400,
    );
    assert!(texts(&p).contains(&String::from("iv.")));
    let p = lay(
        "<ol type=a style='list-style-type:upper-alpha'><li>a</li><li>b</li></ol>",
        400,
    );
    assert!(texts(&p).contains(&String::from("B.")));
}

#[test]
fn list_items_with_block_content_keep_their_marker_on_the_first_line() {
    let p = lay("<ul><li><p>para</p></li></ul>", 400);
    assert_eq!(
        p.cmds
            .iter()
            .filter(|c| matches!(c, Cmd::Rect { radius, .. } if *radius > 0))
            .count(),
        1
    );
}

#[test]
fn definition_lists_indent_the_definition() {
    let p = lay("<dl><dt>term</dt><dd>meaning</dd></dl>", 400);
    assert!(x_of(&p, "meaning") > x_of(&p, "term"));
    assert!(font_of(&p, "term").bold);
}

// ---- links ----

#[test]
fn links_get_the_ua_indigo() {
    let p = lay("<p>see <a href=x>this link</a> ok</p>", 600);
    assert!(p.cmds.iter().any(|c| matches!(c, Cmd::Text { text, color, .. } if text == "this link" && *color == Rgb(0x4f, 0x46, 0xe5))));
}

#[test]
fn links_are_recorded_with_hit_boxes() {
    let p = lay(
        "<p>go <a href='/x'>to x</a> or <a href=y>there</a></p>",
        600,
    );
    assert_eq!(p.links, ["/x", "y"]);
    assert_eq!(p.hits.len(), 2, "one box per run of link text");
    let first = p.hits[0];
    assert_eq!(p.link_at(first.x + 1, first.y + 1), Some("/x"));
    assert_eq!(p.link_index_at(first.x + 1, first.y + 1), Some(0));
    let last = p.hits[1];
    assert_eq!(
        p.link_at(last.x + last.w - 1, last.y + last.h - 1),
        Some("y")
    );
    assert_eq!(p.link_at(0, 0), None);
    assert_eq!(p.link_at(first.x, first.y + first.h), None);
}

#[test]
fn link_runs_carry_their_link_index() {
    let p = lay("<p><a href=a>one</a> <a href=b>two</a></p>", 600);
    let idx: Vec<Option<u32>> = p
        .cmds
        .iter()
        .filter_map(|c| match c {
            Cmd::Text { link, .. } => Some(*link),
            _ => None,
        })
        .collect();
    assert_eq!(idx, [Some(0), Some(1)]);
}

#[test]
fn nested_inline_inside_a_link_stays_clickable() {
    let p = lay("<a href='/n'>plain <b>bold</b></a> after", 600);
    assert_eq!(p.links, ["/n"]);
    assert_eq!(p.hits.len(), 2);
    assert!(p.hits.iter().all(|h| p.links[h.link] == "/n"));
}

#[test]
fn a_block_inside_a_link_keeps_the_link() {
    let p = lay("<a href='/card'><div>title</div><div>body</div></a>", 600);
    assert_eq!(p.links, ["/card"]);
    assert_eq!(p.hits.len(), 2);
}

#[test]
fn anchor_without_href_is_not_a_link_and_links_are_capped() {
    let p = lay("<a name=top>x</a><a>y</a>", 600);
    assert!(p.links.is_empty() && p.hits.is_empty());
    let mut html = String::new();
    for i in 0..(MAX_LINKS + 50) {
        html.push_str(&format!("<a href=/{i}>l</a> "));
    }
    let p = lay(&html, 600);
    assert!(p.links.len() <= MAX_LINKS);
}

// ---- zoom ----

#[test]
fn zoom_scales_text_and_lengths_from_50_to_300() {
    let html = "<body style='margin:0'><p style='margin:10px 0;padding:4px'>hello</p></body>";
    let base = lay_zoom(html, 800, 100);
    for z in [50u16, 75, 100, 125, 150, 200, 250, 300] {
        let p = lay_zoom(html, 800, z);
        assert_eq!(font_of(&p, "hello").size, (16 * z / 100), "zoom {z}");
        let want = (base.height as i64 * i64::from(z) / 100) as i32;
        assert!(
            (p.height - want).abs() <= 8,
            "zoom {z}: {} vs {want}",
            p.height
        );
        for c in &p.cmds {
            if let Cmd::Text { x, w, .. } = c {
                assert!(*x + *w <= 800);
            }
        }
    }
}

#[test]
fn zoomed_text_wraps_more() {
    let html = "<body style='margin:0'><p style='margin:0'>one two three four five six seven eight nine ten</p></body>";
    let a = distinct_ys(&lay_zoom(html, 400, 100)).len();
    let b = distinct_ys(&lay_zoom(html, 400, 300)).len();
    assert!(b > a);
}

// ---- budgets and hostility ----

#[test]
fn huge_css_lengths_do_not_overflow_layout() {
    let html = "<div style='margin:2147483647;padding:2147483647'>\
        <p style='margin:2147483647;padding:2147483647'>x</p>\
        <p style='margin:2147483647'>y</p></div>\
        <p style='font-size:2147483647px;margin:99999999999999999999'>z</p>";
    for z in [50, 100, 300] {
        let p = lay_zoom(html, 600, z);
        assert!(p.height >= 0);
        for c in &p.cmds {
            match c {
                Cmd::Rect { x, y, w, h, .. } => assert!(*x >= 0 && *y >= 0 && *w >= 0 && *h >= 0),
                Cmd::Text { x, y, .. } => assert!(*x >= 0 && *y >= 0),
                Cmd::Image { x, y, w, h, .. } => assert!(*x >= 0 && *y >= 0 && *w >= 0 && *h >= 0),
                Cmd::Border { w, h, .. } => assert!(*w >= 0 && *h >= 0),
            }
        }
    }
}

#[test]
fn negative_and_absurd_values_do_not_panic() {
    for css in [
        "margin:-99999px",
        "padding:-5px",
        "width:-10px",
        "max-width:0",
        "font-size:0",
        "font-size:-3px",
        "line-height:0",
        "line-height:99999",
        "border:99999px solid red",
        "border-radius:99999px",
        "height:99999px",
        "margin:0 auto 0 auto;width:1px",
    ] {
        let html = format!("<div style='{css}'><p>text <b>bold</b></p><ul><li>x</li></ul></div>");
        for z in [50, 300] {
            let p = lay_zoom(&html, 300, z);
            assert!(p.height >= 0, "{css}");
        }
    }
}

#[test]
fn huge_document_renders_fast_and_bounded() {
    let mut html = String::from("<style>");
    for i in 0..3000 {
        html.push_str(&format!(
            ".c{i}, div p.x{i} {{ color: red; margin: 1px }}\n"
        ));
    }
    html.push_str("</style><body>");
    for i in 0..6000 {
        html.push_str(&format!("<div><p class='c{i}'>row {i}</p></div>"));
    }
    html.push_str("</body>");
    let start = std::time::Instant::now();
    let p = lay(&html, 600);
    assert!(start.elapsed() < std::time::Duration::from_secs(30));
    assert!(p.height > 0);
    assert!(p.cmds.len() <= MAX_CMDS);
}

#[test]
fn selector_work_is_bounded_by_the_budget() {
    // Descendant selectors on deep chains cannot take unbounded time.
    let mut css = String::new();
    for _ in 0..900 {
        css.push_str("div div div div p { color: red }\n");
    }
    let html = format!(
        "<style>{css}</style>{}<p>x</p>{}",
        "<div>".repeat(35),
        "</div>".repeat(35)
    );
    let start = std::time::Instant::now();
    let p = lay(&html, 600);
    assert!(start.elapsed() < std::time::Duration::from_secs(20));
    assert!(p.height >= 0);
}

#[test]
fn deep_nesting_lays_out_on_a_small_stack() {
    let mut html = String::new();
    for _ in 0..5_000 {
        html.push_str("<div style='padding:1px'><ul><li><b><i>");
    }
    html.push_str("deep text");
    let h = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let p = render(html.as_bytes(), 600);
            assert!(p.height > 0);
        })
        .unwrap();
    h.join().unwrap();
}

#[test]
fn many_words_are_bounded() {
    let html = format!("<p>{}</p>", "ab ".repeat(600_000));
    let p = lay(&html, 500);
    let chars: usize = runs(&p).iter().map(|r| r.2.chars().count()).sum();
    assert!(chars <= 2 * MAX_TEXT_CHARS, "{chars}"); // spaces between words are not counted
}

#[test]
fn layout_is_deterministic() {
    let html = "<h1>T</h1><p>some <b>text</b> <a href=x>link</a> and more words to wrap around</p><ul><li>a</li></ul>";
    let a = lay(html, 300);
    let b = lay(html, 300);
    assert_eq!(a.height, b.height);
    assert_eq!(format!("{:?}", a.cmds), format!("{:?}", b.cmds));
}

#[test]
fn relayout_at_another_width_reflows() {
    let html = "<p>one two three four five six seven eight nine ten eleven twelve</p>";
    let doc = Doc::parse(html.as_bytes());
    let at = |w| {
        doc.layout(&Layout {
            width: w,
            zoom: 100,
            images: &NoImages,
            metrics: &FixedAdvance,
        })
    };
    assert!(at(200).height > at(900).height);
    assert!(at(40).height > 0);
    assert!(at(0).height > 0);
    assert!(at(-5).height > 0);
}

#[test]
fn metrics_are_used_not_character_counts() {
    // The same text in a narrower face takes fewer lines.
    struct Narrow;
    impl TextMetrics for Narrow {
        fn width(&self, t: &str, f: Font) -> i32 {
            t.chars().count() as i32 * i32::from(f.size) / 4
        }
        fn line_height(&self, f: Font) -> i32 {
            i32::from(f.size) * 6 / 5
        }
        fn ascent(&self, f: Font) -> i32 {
            i32::from(f.size)
        }
    }
    let html = "<p>one two three four five six seven eight nine ten eleven twelve</p>";
    let wide = render_with(html.as_bytes(), 200, &FixedAdvance);
    let narrow = render_with(html.as_bytes(), 200, &Narrow);
    assert!(narrow.height < wide.height);
}

// ---- tables ----

fn body0(inner: &str) -> String {
    format!("<body style='margin:0'>{inner}</body>")
}

fn rect_of(p: &Page, color: Rgb) -> Option<(i32, i32, i32, i32)> {
    p.cmds.iter().find_map(|c| match c {
        Cmd::Rect {
            x,
            y,
            w,
            h,
            color: col,
            ..
        } if *col == color => Some((*x, *y, *w, *h)),
        _ => None,
    })
}

#[test]
fn table_columns_follow_their_content() {
    // 8 px per character; cells pad 8 px each side; 2 px spacing.
    let p = lay(
        &body0("<table><tr><td>aa</td><td>bbbb</td></tr></table>"),
        600,
    );
    assert_eq!(x_of(&p, "aa"), 2 + 8);
    assert_eq!(x_of(&p, "bbbb"), 2 + (16 + 16) + 2 + 8);
    assert_eq!(y_of(&p, "aa"), y_of(&p, "bbbb"));
}

#[test]
fn an_auto_table_is_as_wide_as_its_content() {
    let p = lay(
        &body0("<table style='background:#abcdef'><tr><td>aa</td><td>bbbb</td></tr></table>"),
        600,
    );
    let (x, _, w, _) = rect_of(&p, Rgb(0xab, 0xcd, 0xef)).expect("table background");
    assert_eq!(x, 0);
    assert_eq!(w, 2 + 32 + 2 + 48 + 2);
}

#[test]
fn a_table_with_width_fills_it() {
    let p = lay(
        &body0(
            "<table style='width:100%;background:#abcdef'><tr><td>aa</td><td>bbbb</td></tr></table>",
        ),
        600,
    );
    let (_, _, w, _) = rect_of(&p, Rgb(0xab, 0xcd, 0xef)).unwrap();
    assert_eq!(w, 600);
    // The columns grew in proportion: the second starts far right of where its content needs.
    assert!(
        x_of(&p, "bbbb") > 2 + 32 + 2 + 8 + 100,
        "{}",
        x_of(&p, "bbbb")
    );
}

#[test]
fn cells_wrap_when_the_table_is_too_narrow() {
    let long = "palavra ".repeat(20);
    let p = lay(
        &body0(&format!(
            "<table style='width:200px'><tr><td>{long}</td><td>curto</td></tr></table>"
        )),
        600,
    );
    assert!(
        distinct_ys(&p).len() > 3,
        "the long cell wraps over several lines"
    );
    for c in &p.cmds {
        if let Cmd::Text { x, w, .. } = c {
            assert!(*x + *w <= 200, "text ends inside the table: {}", x + w);
        }
    }
    // The short column keeps its content on one line.
    assert!(texts(&p).contains(&String::from("curto")));
}

#[test]
fn a_word_wider_than_the_table_squeezes_not_overflows() {
    let word = "w".repeat(60);
    let p = lay(
        &body0(&format!(
            "<table style='width:150px'><tr><td>{word}</td></tr></table>"
        )),
        600,
    );
    for c in &p.cmds {
        if let Cmd::Text { x, w, .. } = c {
            assert!(*x + *w <= 150 + 8, "{} {}", x, w);
        }
    }
}

#[test]
fn colspan_spreads_over_columns() {
    let p = lay(
        &body0(
            "<table><tr><td>a</td><td>b</td></tr><tr><td colspan=2>una celda de dos columnas ancha</td></tr></table>",
        ),
        600,
    );
    // The wide cell forces the two columns to share its width.
    let a = x_of(&p, "a");
    let b = x_of(&p, "b");
    assert!(b - a > 40, "columns grew: {}", b - a);
    assert!(texts(&p).iter().any(|t| t.starts_with("una celda")));
}

#[test]
fn rows_stack_and_cells_share_the_row_height() {
    let p = lay(
        &body0(
            "<table><tr><td style='background:#112233'>x</td><td>un texto largo que da mas lineas aqui</td></tr><tr><td>y</td></tr></table>",
        ),
        200,
    );
    let (_, _, _, h) = rect_of(&p, Rgb(0x11, 0x22, 0x33)).unwrap();
    // The first cell's background is as tall as the taller neighbour.
    assert!(h > 24 + 8, "cell box stretched to the row: {h}");
    assert!(y_of(&p, "y") > y_of(&p, "x") + 24);
}

#[test]
fn cell_content_is_middle_aligned_by_default() {
    let p = lay(
        &body0("<table><tr><td>one<br>two<br>three</td><td>mid</td></tr></table>"),
        600,
    );
    assert!(y_of(&p, "mid") > y_of(&p, "one"));
    let p = lay(
        &body0("<table><tr><td>one<br>two<br>three</td><td valign=top>top</td></tr></table>"),
        600,
    );
    assert_eq!(y_of(&p, "top"), y_of(&p, "one"));
    let p = lay(
        &body0(
            "<table><tr><td>one<br>two<br>three</td><td style='vertical-align:bottom'>bot</td></tr></table>",
        ),
        600,
    );
    assert!(y_of(&p, "bot") > y_of(&p, "two"));
}

#[test]
fn table_border_attribute_draws_cell_borders() {
    let p = lay(
        &body0("<table border=1><tr><td>a</td><td>b</td></tr></table>"),
        600,
    );
    let borders = p
        .cmds
        .iter()
        .filter(|c| matches!(c, Cmd::Border { .. }))
        .count();
    assert_eq!(borders, 3, "the table and its two cells");
}

#[test]
fn collapsed_borders_share_an_edge() {
    let p = lay(
        &body0(
            "<style>table{border-collapse:collapse} td{border:1px solid #333}</style><table><tr><td>a</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>",
        ),
        600,
    );
    let widths: Vec<[i32; 4]> = p
        .cmds
        .iter()
        .filter_map(|c| match c {
            Cmd::Border { widths, .. } => Some(*widths),
            _ => None,
        })
        .collect();
    assert_eq!(widths.len(), 4);
    assert_eq!(widths[0], [1, 1, 1, 1]); // first row, first column
    assert_eq!(widths[1], [1, 1, 1, 0]); // first row, second column: no left edge
    assert_eq!(widths[2], [0, 1, 1, 1]); // second row: no top edge
    assert_eq!(widths[3], [0, 1, 1, 0]);
}

#[test]
fn caption_comes_first_and_centred() {
    let p = lay(
        &body0("<table><caption>Titulo</caption><tr><td>a</td></tr></table>"),
        600,
    );
    assert!(y_of(&p, "Titulo") < y_of(&p, "a"));
    assert!(font_of(&p, "Titulo").bold);
}

#[test]
fn nested_tables_lay_out() {
    let p = lay(
        &body0(
            "<table><tr><td>fora<table><tr><td>dentro</td><td>outro</td></tr></table></td></tr></table>",
        ),
        600,
    );
    assert!(y_of(&p, "dentro") > y_of(&p, "fora"));
    assert_eq!(y_of(&p, "dentro"), y_of(&p, "outro"));
    assert!(x_of(&p, "outro") > x_of(&p, "dentro"));
}

#[test]
fn table_links_and_forms_keep_their_boxes_after_alignment() {
    let p = lay(
        &body0("<table><tr><td>a<br>b<br>c</td><td><a href=x>link</a></td></tr></table>"),
        600,
    );
    let hit = p.hits[0];
    let ty = y_of(&p, "link");
    assert!(
        hit.y <= ty && ty < hit.y + hit.h,
        "the hit box moved with its text"
    );
    assert_eq!(p.link_at(hit.x + 1, hit.y + 1), Some("x"));
}

#[test]
fn vertical_alignment_moves_only_the_cell_it_belongs_to() {
    // A tall middle cell between short neighbours (one with a background): centring the
    // neighbours must not drag the cells laid out after them.
    let row = |n: &str| {
        alloc::format!(
            "<tr><td style=\"background:#66f;width:40px\">{n}</td><td>top{n}<br>bot{n}</td><td>end{n}</td></tr>"
        )
    };
    let p = lay(
        &alloc::format!("<table>{}{}{}</table>", row("1"), row("2"), row("3")),
        400,
    );
    for n in ["1", "2", "3"] {
        let (top, bot, end) = (
            y_of(&p, &alloc::format!("top{n}")),
            y_of(&p, &alloc::format!("bot{n}")),
            y_of(&p, &alloc::format!("end{n}")),
        );
        assert!(bot > top);
        // The short cells are centred on the two-line cell, not pushed below it.
        assert!(
            end >= top && end < bot,
            "end{n} at {end}, lines at {top}/{bot}"
        );
        let tile = y_of(&p, n);
        assert!(tile >= top && tile <= bot, "tile {n} at {tile}");
    }
    assert!(y_of(&p, "top2") > y_of(&p, "bot1"));
    assert!(y_of(&p, "top3") > y_of(&p, "bot2"));
}

#[test]
fn hostile_tables_are_bounded() {
    // Thousands of columns and rows, huge colspans.
    let mut html = String::from("<table>");
    for r in 0..3000 {
        html.push_str("<tr>");
        for c in 0..40 {
            html.push_str(&format!(
                "<td colspan='{}'>{}</td>",
                if c == 0 { 99999 } else { 1 },
                r
            ));
        }
        html.push_str("</tr>");
    }
    html.push_str("</table>");
    let start = std::time::Instant::now();
    let p = lay(&html, 500);
    assert!(start.elapsed() < std::time::Duration::from_secs(20));
    assert!(p.height > 0);
    for c in &p.cmds {
        if let Cmd::Text { x, w, .. } = c {
            assert!(*x >= 0 && *x + *w <= 500 + 64, "{x} {w}");
        }
    }
}

#[test]
fn empty_and_odd_tables_do_not_panic() {
    for html in [
        "<table></table>",
        "<table><tr></tr></table>",
        "<table><tr><td></td></tr></table>",
        "<table><td>loose cell</td></table>",
        "<tr><td>row without table</td></tr>",
        "<table><caption>only</caption></table>",
        "<table><thead><tbody><tr><td>x</td></tr></tbody></thead></table>",
        "<table width=0><tr><td width=0>x</td></tr></table>",
        "<table style='width:5px'><tr><td style='width:99999px'>x</td></tr></table>",
        "<table cellpadding=99999 cellspacing=99999 border=99999><tr><td>x</td></tr></table>",
    ] {
        for z in [50, 300] {
            let p = lay_zoom(html, 300, z);
            assert!(p.height >= 0, "{html}");
        }
    }
}

#[test]
fn presentational_attributes_apply_and_css_overrides_them() {
    let p = lay(&body0("<p align=center>ab</p>"), 100);
    assert_eq!(x_of(&p, "ab"), (100 - 16) / 2);
    let p = lay(
        &body0("<p align=center style='text-align:left'>ab</p>"),
        100,
    );
    assert_eq!(x_of(&p, "ab"), 0);
    let p = lay("<font color='#ff0000' size=5>big</font>", 300);
    assert_eq!(font_of(&p, "big").size, 24);
    let p = lay(&body0("<div bgcolor='#112233'>x</div>"), 300);
    assert!(rect_of(&p, Rgb(0x11, 0x22, 0x33)).is_some());
    let p = lay("<ol type=a><li>x</li><li>y</li></ol>", 300);
    assert!(texts(&p).contains(&String::from("b.")));
}

#[test]
fn sup_and_sub_move_the_baseline() {
    let p = lay("<p>x<sup>2</sup> y<sub>i</sub> z</p>", 300);
    let base = y_of(&p, "x");
    let sup = y_of(&p, "2");
    let sub = y_of(&p, "i");
    assert!(sup < base + 3, "raised: {sup} vs {base}");
    assert!(sub > sup);
}

#[test]
fn inline_padding_shows_its_background_around_the_words() {
    let p = lay(
        &body0(
            "<p style='margin:0'>a <span style='background:#123456;padding:0 10px'>bb</span> c</p>",
        ),
        300,
    );
    let (x, _, w, _) = rect_of(&p, Rgb(0x12, 0x34, 0x56)).expect("badge background");
    let bb = x_of(&p, "bb");
    assert_eq!(x, bb - 10);
    assert_eq!(w, 16 + 20);
    // One box: the words' own background joins the padding's.
    let n = p
        .cmds
        .iter()
        .filter(|c| matches!(c, Cmd::Rect { color, .. } if *color == Rgb(0x12, 0x34, 0x56)))
        .count();
    assert_eq!(n, 1);
}

#[test]
fn controls_sit_on_the_text_baseline() {
    let p = lay(
        &body0("<form><p style='margin:0'>Nome: <input name=n></p></form>"),
        400,
    );
    let f = p.fields[0];
    // The label's baseline (16 below its top) is the field text's baseline (control font 14 px,
    // line 17, a 30 px box: 6 + 14 below the top).
    assert_eq!(y_of(&p, "Nome:") + 16, f.y + 20);
}

#[test]
fn a_check_box_keeps_a_gap_before_its_label() {
    let p = lay(
        &body0("<form><p style='margin:0'><input type=checkbox name=c> Aceito</p></form>"),
        400,
    );
    let f = p.fields[0];
    assert_eq!(f.w, 16);
    assert!(x_of(&p, "Aceito") >= f.x + f.w + 6);
}

#[test]
fn a_full_width_table_keeps_pixel_width_columns_and_widens_the_others() {
    let p = lay(
        "<table style=\"width:100%\"><tr><td style=\"width:40px\">a</td><td>bb</td></tr></table>",
        400,
    );
    // The first column stays 40 px wide: the second cell starts right after it.
    assert!(
        x_of(&p, "bb") - x_of(&p, "a") < 60,
        "{}",
        x_of(&p, "bb") - x_of(&p, "a")
    );
}

#[test]
fn a_long_paragraph_of_short_words_keeps_its_words_whole() {
    // More characters than one word may have: the count is per word, so ordinary words
    // late in a long text node are not cut in two.
    let text = "palavra ".repeat(1200);
    let p = lay(&alloc::format!("<p>{text}</p>"), 400);
    let all: Vec<String> = p
        .cmds
        .iter()
        .filter_map(|c| match c {
            Cmd::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    let joined = all.join(" ");
    assert!(joined.split(' ').all(|w| w == "palavra"), "{joined:.80}");
    assert_eq!(joined.split(' ').count(), 1200);
}
