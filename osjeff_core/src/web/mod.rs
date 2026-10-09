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
//! NOT a standards browser -- no flexbox/grid/float/JS.

mod css;
mod dom;
pub mod find;
pub mod form;
pub mod imgcache;
mod layout;
#[cfg(test)]
mod layout_tests;
pub mod metrics;
mod style;
#[cfg(test)]
mod tests_img_form;
pub mod textops;

pub use css::{Decl, MAX_RULES, MAX_SELECTORS, Rule, Selector, Specificity, Stylesheet, parse_css};
pub use dom::{Element, MAX_DEPTH, MAX_NODES, Node, parse_html};

/// Text made fit for one line of display: whitespace and controls collapse to one space and
/// characters that could spoof or break the text (bidirectional overrides, zero-width joiners)
/// are dropped. Accents and other characters are kept.
pub fn fold_for_display(s: &str) -> alloc::string::String {
    dom::fold_display(s)
}
pub use layout::{
    Cmd, DECO_STRIKE, DECO_UNDERLINE, Doc, FieldBox, ImgRef, Layout, LinkHit, MAX_CMDS, MAX_IMAGES,
    MAX_LINKS, MAX_TEXT_CHARS, MAX_WORD_CHARS, MAX_ZOOM, MIN_ZOOM, Page, ZOOM_STEPS, render,
    render_with, zoom_in, zoom_out,
};
pub use metrics::{FixedAdvance, Font, TextMetrics};

// ---- colors ----

/// An 8-bit RGB color.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb(r, g, b)
    }
}

/// Blend `fg` over white at `alpha` (0..=255): translucent colours are resolved against the
/// page's usual white because the display list has no alpha.
fn over_white(r: u8, g: u8, b: u8, alpha: u32) -> Rgb {
    let mix = |c: u8| ((u32::from(c) * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
    Rgb(mix(r), mix(g), mix(b))
}

/// One argument of `rgb()` / `hsl()`: thousandths, and whether it was written as a percent.
#[derive(Clone, Copy)]
struct Arg {
    milli: i64,
    pct: bool,
}

impl Arg {
    /// A 0..=255 channel (a percent is of 255).
    fn channel(self) -> u8 {
        let v = if self.pct {
            self.milli * 255 / 100_000
        } else {
            self.milli / 1000
        };
        v.clamp(0, 255) as u8
    }

    /// 0..=100 (saturation, lightness).
    fn percent(self) -> i32 {
        (self.milli / 1000).clamp(0, 100) as i32
    }

    /// An alpha, 0..=255: `0.5` or `50%`.
    fn alpha(self) -> u32 {
        let v = if self.pct {
            self.milli * 255 / 100_000
        } else {
            self.milli * 255 / 1000
        };
        v.clamp(0, 255) as u32
    }
}

/// Split `rgb()` / `hsl()` arguments: separators are commas, spaces or a `/`.
fn color_args(inner: &str) -> Option<([Arg; 3], u32)> {
    let mut args: [Arg; 4] = [Arg {
        milli: 0,
        pct: false,
    }; 4];
    args[3] = Arg {
        milli: 1000,
        pct: false,
    };
    let mut n = 0;
    for part in inner.split(|c: char| c == ',' || c == '/' || c.is_whitespace()) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if n >= 4 {
            return None;
        }
        let (num, pct) = match part.strip_suffix('%') {
            Some(p) => (p, true),
            None => (part.strip_suffix("deg").unwrap_or(part), false),
        };
        let neg = num.starts_with('-');
        let digits = num.trim_start_matches(['-', '+']);
        let (int, frac) = digits.split_once('.').unwrap_or((digits, ""));
        if (int.is_empty() && frac.is_empty())
            || !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let i: i64 = int.parse().unwrap_or(0).min(100_000);
        let f: i64 = frac
            .bytes()
            .take(3)
            .enumerate()
            .map(|(k, b)| i64::from(b - b'0') * [100, 10, 1][k])
            .sum();
        let milli = (i * 1000 + f) * if neg { -1 } else { 1 };
        args[n] = Arg { milli, pct };
        n += 1;
    }
    (n >= 3).then(|| ([args[0], args[1], args[2]], args[3].alpha()))
}

fn hsl_to_rgb(h: i64, s: i32, l: i32) -> (u8, u8, u8) {
    // h in degrees, s and l in percent; integer maths.
    let h = h.rem_euclid(360);
    let (s, l) = (i64::from(s), i64::from(l));
    let c = (100 - (2 * l - 100).abs()) * s / 100; // chroma, percent
    let hp = h * 1000 / 60; // sector in thousandths
    let x = c * (1000 - (hp % 2000 - 1000).abs()) / 1000;
    let (r1, g1, b1) = match hp / 1000 {
        0 => (c, x, 0),
        1 => (x, c, 0),
        2 => (0, c, x),
        3 => (0, x, c),
        4 => (x, 0, c),
        _ => (c, 0, x),
    };
    let m = l - c / 2;
    let ch = |v: i64| ((v + m) * 255 / 100).clamp(0, 255) as u8;
    (ch(r1), ch(g1), ch(b1))
}

/// Parse a CSS color: `#rgb`, `#rrggbb`, `#rgba`, `#rrggbbaa`, `rgb()`/`rgba()`,
/// `hsl()`/`hsla()` and the named colors. Translucent values are blended over white.
/// Returns `None` on anything unrecognized (and on `transparent`).
pub fn parse_color(s: &str) -> Option<Rgb> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        // ASCII hex digits only: the fixed byte-offset slicing below would
        // otherwise panic on a char boundary (and `from_str_radix` accepts '+').
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let h = |i: usize, n: usize| u8::from_str_radix(&hex[i..i + n], 16).ok();
        return match hex.len() {
            3 | 4 => {
                let (r, g, b) = (h(0, 1)? * 17, h(1, 1)? * 17, h(2, 1)? * 17);
                let a = if hex.len() == 4 { h(3, 1)? * 17 } else { 255 };
                Some(over_white(r, g, b, u32::from(a)))
            }
            6 | 8 => {
                let a = if hex.len() == 8 { h(6, 2)? } else { 255 };
                Some(over_white(h(0, 2)?, h(2, 2)?, h(4, 2)?, u32::from(a)))
            }
            _ => None,
        };
    }
    let lower = s.to_ascii_lowercase();
    let func = |name: &str| -> Option<&str> {
        lower
            .strip_prefix(name)
            .and_then(|x| x.trim_start().strip_prefix('('))
            .and_then(|x| x.strip_suffix(')'))
    };
    if let Some(inner) = func("rgba").or_else(|| func("rgb")) {
        let (a, alpha) = color_args(inner)?;
        return Some(over_white(
            a[0].channel(),
            a[1].channel(),
            a[2].channel(),
            alpha,
        ));
    }
    if let Some(inner) = func("hsla").or_else(|| func("hsl")) {
        let (a, alpha) = color_args(inner)?;
        let (r, g, b) = hsl_to_rgb(a[0].milli / 1000, a[1].percent(), a[2].percent());
        return Some(over_white(r, g, b, alpha));
    }
    named_color(&lower)
}

fn named_color(name: &str) -> Option<Rgb> {
    const NAMED: &[(&str, u32)] = &[
        ("black", 0x000000),
        ("white", 0xffffff),
        ("red", 0xff0000),
        ("green", 0x008000),
        ("blue", 0x0000ff),
        ("navy", 0x000080),
        ("gray", 0x808080),
        ("grey", 0x808080),
        ("silver", 0xc0c0c0),
        ("orange", 0xffa500),
        ("teal", 0x008080),
        ("yellow", 0xffff00),
        ("purple", 0x800080),
        ("maroon", 0x800000),
        ("olive", 0x808000),
        ("lime", 0x00ff00),
        ("aqua", 0x00ffff),
        ("cyan", 0x00ffff),
        ("fuchsia", 0xff00ff),
        ("magenta", 0xff00ff),
        ("brown", 0xa52a2a),
        ("pink", 0xffc0cb),
        ("gold", 0xffd700),
        ("indigo", 0x4b0082),
        ("violet", 0xee82ee),
        ("coral", 0xff7f50),
        ("crimson", 0xdc143c),
        ("salmon", 0xfa8072),
        ("tomato", 0xff6347),
        ("khaki", 0xf0e68c),
        ("lavender", 0xe6e6fa),
        ("beige", 0xf5f5dc),
        ("ivory", 0xfffff0),
        ("tan", 0xd2b48c),
        ("turquoise", 0x40e0d0),
        ("skyblue", 0x87ceeb),
        ("steelblue", 0x4682b4),
        ("royalblue", 0x4169e1),
        ("dodgerblue", 0x1e90ff),
        ("slategray", 0x708090),
        ("slategrey", 0x708090),
        ("darkgray", 0xa9a9a9),
        ("darkgrey", 0xa9a9a9),
        ("lightgray", 0xd3d3d3),
        ("lightgrey", 0xd3d3d3),
        ("dimgray", 0x696969),
        ("dimgrey", 0x696969),
        ("gainsboro", 0xdcdcdc),
        ("whitesmoke", 0xf5f5f5),
        ("snow", 0xfffafa),
        ("darkred", 0x8b0000),
        ("darkgreen", 0x006400),
        ("darkblue", 0x00008b),
        ("darkorange", 0xff8c00),
        ("lightblue", 0xadd8e6),
        ("lightgreen", 0x90ee90),
        ("lightyellow", 0xffffe0),
        ("lightpink", 0xffb6c1),
        ("forestgreen", 0x228b22),
        ("seagreen", 0x2e8b57),
        ("limegreen", 0x32cd32),
        ("midnightblue", 0x191970),
        ("firebrick", 0xb22222),
        ("orangered", 0xff4500),
        ("hotpink", 0xff69b4),
        ("deeppink", 0xff1493),
        ("orchid", 0xda70d6),
        ("plum", 0xdda0dd),
        ("chocolate", 0xd2691e),
        ("sienna", 0xa0522d),
        ("peru", 0xcd853f),
        ("wheat", 0xf5deb3),
        ("aliceblue", 0xf0f8ff),
        ("mintcream", 0xf5fffa),
        ("honeydew", 0xf0fff0),
        ("azure", 0xf0ffff),
        ("linen", 0xfaf0e6),
        ("cornsilk", 0xfff8dc),
        ("rebeccapurple", 0x663399),
    ];
    let (_, rgb) = NAMED.iter().find(|(n, _)| *n == name)?;
    Some(Rgb((rgb >> 16) as u8, (rgb >> 8) as u8, *rgb as u8))
}

#[cfg(test)]
mod tests {
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
}
