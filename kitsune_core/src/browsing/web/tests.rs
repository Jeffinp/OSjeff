use super::*;

#[test]
fn parses_known_colors() {
    assert_eq!(parse_color("#fff"), Some(Rgb(255, 255, 255)));
    assert_eq!(parse_color("#1565C0"), Some(Rgb(0x15, 0x65, 0xC0)));
    assert_eq!(parse_color("rgb(1, 2, 3)"), Some(Rgb(1, 2, 3)));
    assert_eq!(parse_color("Navy"), Some(Rgb(0, 0, 0x80)));
    assert_eq!(parse_color("red"), Some(Rgb(255, 0, 0)));
    assert_eq!(parse_color("rebeccapurple"), Some(Rgb(0x66, 0x33, 0x99)));
    assert_eq!(parse_color("nonsense"), None);
    assert_eq!(parse_color("transparent"), None);
    assert_eq!(parse_color("#12"), None);
}

#[test]
fn parses_modern_color_syntax() {
    assert_eq!(parse_color("rgb(255 128 0)"), Some(Rgb(255, 128, 0)));
    assert_eq!(parse_color("rgb(100%, 0%, 50%)"), Some(Rgb(255, 0, 127)));
    assert_eq!(parse_color("rgba(0,0,0,0.5)"), Some(Rgb(128, 128, 128)));
    assert_eq!(parse_color("rgb(0 0 0 / 100%)"), Some(Rgb(0, 0, 0)));
    assert_eq!(parse_color("#00000080"), Some(Rgb(127, 127, 127)));
    assert_eq!(parse_color("#0008"), Some(Rgb(119, 119, 119)));
    assert_eq!(parse_color("hsl(0, 100%, 50%)"), Some(Rgb(255, 0, 0)));
    assert_eq!(parse_color("hsl(120deg 100% 25%)"), Some(Rgb(0, 127, 0)));
    assert_eq!(parse_color("hsla(240, 100%, 50%, 1)"), Some(Rgb(0, 0, 255)));
    assert_eq!(parse_color("rgb(1,2)"), None);
    assert_eq!(parse_color("rgb(a,b,c)"), None);
    assert_eq!(parse_color("rgb(1,2,3,4,5)"), None);
    assert_eq!(parse_color("hsl(1)"), None);
}

/// Regression: `#` followed by multi-byte UTF-8 whose *byte* length is 3 or
/// 6 was sliced at fixed byte offsets (`&hex[0..1]`), landing inside a
/// character and panicking. Page CSS is attacker-controlled.
#[test]
fn non_ascii_hex_color_is_rejected_not_panicking() {
    assert_eq!(parse_color("#\u{e9}1"), None); // 3 bytes: 0xC3 0xA9 '1'
    assert_eq!(parse_color("#\u{e9}\u{e9}\u{e9}"), None); // 6 bytes
    assert_eq!(parse_color("#1\u{e9}\u{e9}1"), None); // 6 bytes, mixed
    assert_eq!(parse_color("#\u{20ac}\u{20ac}"), None); // 2 x 3-byte chars
}

/// Regression: the cascade is O(rules x elements) and neither was bounded,
/// so a 256 KiB page with thousands of `<style>` rules and thousands of
/// elements burned seconds (1.6 s in release on a loaded host at
/// 5000 x 5000; far worse on the emulated CPU). Both are capped now: the DOM at `MAX_NODES` and
/// the stylesheet at `MAX_RULES` rules / `MAX_SELECTORS` selectors.
#[test]
fn huge_document_renders_fast_and_without_panic() {
    let mut html = String::from("<style>");
    for i in 0..5000 {
        html.push_str(&format!(".c{i}, p.x{i} {{ color: red; margin: 1px }}\n"));
    }
    html.push_str("</style><body>");
    for i in 0..5000 {
        html.push_str(&format!("<p class='c{i}'>row {i}</p>"));
    }
    html.push_str("</body>");

    let start = std::time::Instant::now();
    let page = render(html.as_bytes(), 600);
    let took = start.elapsed();
    assert!(page.height > 0);
    // Display commands are bounded by the node budget (one run per text
    // node at most here), so the output cannot balloon either.
    assert!(page.cmds.len() <= MAX_NODES, "{} cmds", page.cmds.len());
    // Hang detector only (this is a debug build on a shared CI box): the
    // capped render is ~0.2 s in release, the uncapped one was 1.6 s+ and
    // grows with the product of the two counts.
    assert!(
        took < std::time::Duration::from_secs(20),
        "render of an oversized page took {took:?}"
    );
}

#[test]
fn page_limits_are_sane() {
    // A real page (and the user-agent sheet) must fit comfortably.
    const {
        assert!(MAX_NODES >= 4096);
        assert!(MAX_RULES >= 256);
        assert!(MAX_SELECTORS >= MAX_RULES);
        assert!(MAX_DEPTH >= 20);
    }
}
