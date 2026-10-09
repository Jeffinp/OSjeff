use super::*;
use crate::browsing::web::css::parse_css;
use crate::browsing::web::dom::{Node, parse_html};

fn style_of(html: &str, css: &str) -> Computed {
    let (nodes, page_css) = parse_html(html.as_bytes());
    let mut sheet = parse_css(UA_CSS);
    sheet.rules.extend(parse_css(css).rules);
    sheet.rules.extend(parse_css(&page_css).rules);
    let Node::Element(el) = &nodes[0] else {
        panic!("no element")
    };
    let mut b = 100_000;
    compute(el, &sheet, &Computed::root(), &[], &mut b)
}

#[test]
fn lengths_in_every_unit() {
    assert_eq!(parse_len("12px", 16), Some(Len::Px(12)));
    assert_eq!(parse_len("1.5em", 20), Some(Len::Px(30)));
    assert_eq!(parse_len("2rem", 20), Some(Len::Px(32)));
    assert_eq!(parse_len("12pt", 16), Some(Len::Px(16)));
    assert_eq!(parse_len("50%", 16), Some(Len::Pct(50)));
    assert_eq!(parse_len("0", 16), Some(Len::Px(0)));
    assert_eq!(parse_len("auto", 16), Some(Len::Auto));
    assert_eq!(parse_len("-4px", 16), Some(Len::Px(-4)));
    assert_eq!(parse_len(".5em", 10), Some(Len::Px(5)));
    assert_eq!(parse_len("calc(1px + 2px)", 16), None);
    assert_eq!(parse_len("10vw", 16), None);
    assert_eq!(parse_len("", 16), None);
    // Hostile numbers saturate.
    assert_eq!(
        parse_len("99999999999999999999999px", 16),
        Some(Len::Px(MAX_PX))
    );
}

#[test]
fn shorthand_margins_expand_to_four_sides() {
    let c = style_of("<p style='margin:1px 2px 3px 4px'>x</p>", "");
    assert_eq!(c.margin, [Len::Px(1), Len::Px(2), Len::Px(3), Len::Px(4)]);
    let c = style_of("<p style='margin:0 auto'>x</p>", "");
    assert_eq!(c.margin, [Len::Px(0), Len::Auto, Len::Px(0), Len::Auto]);
    let c = style_of("<p style='margin:5px 6px;margin-left:9px'>x</p>", "");
    assert_eq!(c.margin, [Len::Px(5), Len::Px(6), Len::Px(5), Len::Px(9)]);
}

#[test]
fn ua_sizes_and_styles() {
    let h1 = style_of("<h1>x</h1>", "");
    assert_eq!(h1.font_px, 32);
    assert!(h1.bold);
    let em = style_of("<em>x</em>", "");
    assert!(em.italic && !em.bold);
    let code = style_of("<code>x</code>", "");
    assert!(code.mono);
    assert_eq!(code.font_px, 15); // .92 * 16 = 14.72, rounded
    let a = style_of("<a href=x>x</a>", "");
    assert_eq!(a.color, Rgb(0x4f, 0x46, 0xe5));
}

#[test]
fn font_shorthand_and_keywords() {
    let c = style_of(
        "<p style='font:italic 600 20px/1.2 Georgia, serif'>x</p>",
        "",
    );
    assert!(c.italic && c.bold);
    assert_eq!(c.font_px, 20);
    assert_eq!(c.line_h, LineH::Mult(120));
    let c = style_of("<p style='font-size:large'>x</p>", "");
    assert_eq!(c.font_px, 18);
    let c = style_of("<p style='font-size:200%'>x</p>", "");
    assert_eq!(c.font_px, 32);
    let c = style_of("<p style='font: 13px monospace'>x</p>", "");
    assert!(c.mono && c.font_px == 13);
}

#[test]
fn em_lengths_follow_the_elements_own_font_size() {
    let c = style_of("<p style='margin-top:2em;font-size:10px'>x</p>", "");
    assert_eq!(c.margin[0], Len::Px(20));
}

#[test]
fn border_shorthand_and_sides() {
    let c = style_of("<p style='border:2px solid #ff0000'>x</p>", "");
    assert_eq!(c.border_w, [2; 4]);
    assert_eq!(c.border_color, Rgb(255, 0, 0));
    let c = style_of("<p style='border-bottom:1px solid #ccc'>x</p>", "");
    assert_eq!(c.border_w, [0, 0, 1, 0]);
    let c = style_of("<p style='border:none'>x</p>", "");
    assert_eq!(c.border_w, [0; 4]);
    let c = style_of("<p style='border-radius:8px 2px'>x</p>", "");
    assert_eq!(c.radius, 8);
}

#[test]
fn background_shorthand_picks_the_colour_or_a_gradient_stop() {
    let c = style_of("<p style='background:#eee url(x.png) no-repeat'>x</p>", "");
    assert_eq!(c.bg, Some(Rgb(0xee, 0xee, 0xee)));
    let c = style_of(
        "<p style='background:linear-gradient(90deg, #ff0000, #0000ff)'>x</p>",
        "",
    );
    assert_eq!(c.bg, Some(Rgb(255, 0, 0)));
    let c = style_of("<p style='background:transparent'>x</p>", "");
    assert_eq!(c.bg, None);
}

#[test]
fn important_and_specificity_order() {
    let c = style_of(
        "<p class=a id=b>x</p>",
        "#b{color:#00ff00} .a{color:#ff0000 !important} p{color:#0000ff}",
    );
    assert_eq!(c.color, Rgb(255, 0, 0));
    let c = style_of("<p id=b style='color:#112233'>x</p>", "#b{color:#00ff00}");
    assert_eq!(c.color, Rgb(0x11, 0x22, 0x33));
}

#[test]
fn inheritance_and_reset() {
    let (nodes, _) = parse_html(b"<div style='color:#102030;font-weight:bold;background:#fff;margin:9px'><span>x</span></div>");
    let sheet = parse_css(UA_CSS);
    let Node::Element(div) = &nodes[0] else {
        panic!()
    };
    let mut b = 1000;
    let dc = compute(div, &sheet, &Computed::root(), &[], &mut b);
    let Node::Element(span) = &div.children[0] else {
        panic!()
    };
    let sc = compute(span, &sheet, &dc, &[div], &mut b);
    assert_eq!(sc.color, Rgb(0x10, 0x20, 0x30));
    assert!(sc.bold);
    assert_eq!(sc.bg, None);
    assert_eq!(sc.margin, [Len::Px(0); 4]);
}

#[test]
fn text_transform_cases() {
    assert_eq!(transform_text("ação", Transform::Upper), "AÇÃO");
    assert_eq!(transform_text("a b", Transform::Capital), "A B");
    assert_eq!(transform_text("ABC", Transform::Lower), "abc");
}
