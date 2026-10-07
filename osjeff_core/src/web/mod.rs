//! A minimal HTML + CSS rendering engine (box model), inspired by Matt
//! Brubeck's "robinson" toy engine. Pure and `alloc`-only, so the whole
//! pipeline is unit-tested on the host.
//!
//! Pipeline: HTML bytes -> DOM tree -> (user-agent + page CSS) -> styled tree ->
//! block layout with inline text flow -> a flat display list of rectangles and
//! text runs. The kernel rasterizes that display list to the framebuffer.
//!
//! Scope is deliberately small: it renders simple, mostly-static HTML/CSS
//! correctly and degrades real-world pages to a readable single column. It is
//! NOT a standards browser -- no flexbox/grid/float/JS/images.

mod css;
mod dom;
pub mod form;
pub mod imgcache;
mod layout;
mod style;
#[cfg(test)]
mod tests_img_form;
pub mod textops;

pub use css::{Decl, MAX_RULES, MAX_SELECTORS, Rule, Selector, Specificity, Stylesheet, parse_css};
pub use dom::{Element, MAX_DEPTH, MAX_NODES, Node, parse_html};
pub use layout::{
    Cmd, Doc, FieldBox, ImgRef, Layout, LinkHit, MAX_IMAGES, MAX_LINKS, MAX_ZOOM, MIN_ZOOM, Page,
    ZOOM_STEPS, render, zoom_in, zoom_out,
};

// ---- colors ----

/// An 8-bit RGB color.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb(r, g, b)
    }
}

/// Parse a CSS color: `#rgb`, `#rrggbb`, `rgb(r,g,b)`, or a small set of named
/// colors. Returns `None` on anything unrecognized.
pub fn parse_color(s: &str) -> Option<Rgb> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        // ASCII hex digits only: the fixed byte-offset slicing below would
        // otherwise panic on a char boundary (and `from_str_radix` accepts '+').
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        return match hex.len() {
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
                let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
                let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
                Some(Rgb(r * 17, g * 17, b * 17))
            }
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some(Rgb(r, g, b))
            }
            _ => None,
        };
    }
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|x| x.strip_suffix(')')) {
        let mut it = inner.split(',').map(|p| p.trim().parse::<u8>().ok());
        return Some(Rgb(it.next()??, it.next()??, it.next()??));
    }
    Some(match s.to_ascii_lowercase().as_str() {
        "black" => Rgb(0, 0, 0),
        "white" => Rgb(255, 255, 255),
        "red" => Rgb(0xD3, 0x2F, 0x2F),
        "green" => Rgb(0x2E, 0x7D, 0x32),
        "blue" => Rgb(0x15, 0x65, 0xC0),
        "navy" => Rgb(0x0D, 0x47, 0xA1),
        "gray" | "grey" => Rgb(0x75, 0x75, 0x75),
        "silver" => Rgb(0xC0, 0xC0, 0xC0),
        "orange" => Rgb(0xF5, 0x7C, 0x00),
        "teal" => Rgb(0x00, 0x80, 0x80),
        "transparent" => return None,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_known_colors() {
        assert_eq!(parse_color("#fff"), Some(Rgb(255, 255, 255)));
        assert_eq!(parse_color("#1565C0"), Some(Rgb(0x15, 0x65, 0xC0)));
        assert_eq!(parse_color("rgb(1, 2, 3)"), Some(Rgb(1, 2, 3)));
        assert_eq!(parse_color("Navy"), Some(Rgb(0x0D, 0x47, 0xA1)));
        assert_eq!(parse_color("#12"), None);
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
}
