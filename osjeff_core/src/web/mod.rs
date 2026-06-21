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
mod layout;
mod style;

pub use css::{Decl, Rule, Selector, Specificity, Stylesheet, parse_css};
pub use dom::{Element, Node, parse_html};
pub use layout::{Cmd, Page, render};

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

