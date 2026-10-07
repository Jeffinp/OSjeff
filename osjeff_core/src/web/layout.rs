//! Block layout with inline text flow, producing the display list.

use super::Rgb;
use super::css::{Stylesheet, parse_css};
use super::dom::{Element, Node, parse_html};
use super::style::{Align, Computed, Disp, UA_CSS, compute};
use alloc::string::String;
use alloc::vec::Vec;

// ---- layout + display list ----

/// A rectangle or a run of text to paint. The kernel maps `Rgb`/`scale` onto
/// its framebuffer and bitmap font.
#[derive(Debug, Clone)]
pub enum Cmd {
    Rect {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        color: Rgb,
    },
    Text {
        x: i32,
        y: i32,
        text: String,
        color: Rgb,
        scale: u8,
        bold: bool,
    },
}

/// A laid-out page: a flat display list plus the total content height (for
/// scrolling).
#[derive(Debug, Default)]
pub struct Page {
    pub cmds: Vec<Cmd>,
    pub height: i32,
    /// `href` of every `<a href>` on the page (at most [`MAX_LINKS`]).
    pub links: Vec<String>,
    /// Clickable boxes of link text, in page coordinates (same space as `cmds`).
    pub hits: Vec<LinkHit>,
}

/// Most links recorded per page (the rest still render, they are just not clickable).
pub const MAX_LINKS: usize = 2000;

/// The box of one word of link text and the link it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkHit {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Index into [`Page::links`].
    pub link: usize,
}

impl Page {
    /// The `href` of the link under page coordinates `(x, y)`, if any.
    pub fn link_at(&self, x: i32, y: i32) -> Option<&str> {
        self.hits
            .iter()
            .rev()
            .find(|h| x >= h.x && x < h.x + h.w && y >= h.y && y < h.y + h.h)
            .and_then(|h| self.links.get(h.link))
            .map(String::as_str)
    }
}

/// Font metrics of the bitmap font: pixels per character cell and per text line
/// at a given integer scale.
fn char_w(scale: u8) -> i32 {
    6 * scale as i32
}
fn line_h(scale: u8) -> i32 {
    9 * scale as i32
}

/// One word of inline content, carrying the style it should render with.
struct Word {
    text: String,
    color: Rgb,
    scale: u8,
    bold: bool,
    /// Index into the page's link table when inside `<a href>`.
    link: Option<usize>,
}

struct Painter {
    cmds: Vec<Cmd>,
    links: Vec<String>,
    hits: Vec<LinkHit>,
}

/// Render an HTML document to a display list laid out for `viewport_w` pixels.
pub fn render(html: &[u8], viewport_w: i32) -> Page {
    let (dom, css) = parse_html(html);
    let mut sheet = parse_css(UA_CSS);
    sheet.rules.extend(parse_css(&css).rules);

    let root = Computed::root();
    let mut painter = Painter {
        cmds: Vec::new(),
        links: Vec::new(),
        hits: Vec::new(),
    };
    let pad = 12;
    let y = layout_children(
        &dom,
        &sheet,
        &root,
        pad,
        pad,
        (viewport_w - 2 * pad).max(40),
        &mut painter,
    );
    Page {
        cmds: painter.cmds,
        height: y + pad,
        links: painter.links,
        hits: painter.hits,
    }
}

/// Lay out a list of sibling nodes in a block formatting context, returning the
/// y just below the last one. Runs of inline content between block children are
/// gathered into line boxes.
fn layout_children(
    nodes: &[Node],
    sheet: &Stylesheet,
    parent: &Computed,
    x: i32,
    mut y: i32,
    width: i32,
    p: &mut Painter,
) -> i32 {
    let mut inline: Vec<Word> = Vec::new();
    for node in nodes {
        match node {
            Node::Text(t) => push_words(&mut inline, t, parent, None),
            Node::Element(el) => {
                let c = compute(el, sheet, parent);
                match c.display {
                    Disp::None => {}
                    Disp::Inline => collect_inline(el, sheet, &c, &mut inline, &mut p.links, None),
                    Disp::Block | Disp::ListItem => {
                        y = flush_inline(&mut inline, x, y, width, parent.align, p);
                        y = layout_block(el, &c, sheet, x, y, width, p);
                    }
                }
            }
        }
    }
    flush_inline(&mut inline, x, y, width, parent.align, p)
}

/// Lay out a single block element (margins, padding, background, then content).
fn layout_block(
    el: &Element,
    c: &Computed,
    sheet: &Stylesheet,
    x: i32,
    mut y: i32,
    width: i32,
    p: &mut Painter,
) -> i32 {
    y += c.margin;
    let cx = x + c.padding + if c.display == Disp::ListItem { 8 } else { 0 };
    let cw = (width - 2 * c.padding).max(20);
    let top = y;
    let bg_index = p.cmds.len();
    y += c.padding;

    // List marker.
    if c.display == Disp::ListItem {
        p.cmds.push(Cmd::Text {
            x: cx - 12,
            y,
            text: "-".into(),
            color: c.color,
            scale: c.scale,
            bold: false,
        });
    }

    y = layout_children(&el.children, sheet, c, cx, y, cw, p);
    y += c.padding;

    // Background fills the border box; inserted behind the content.
    if let Some(bg) = c.bg {
        p.cmds.insert(
            bg_index,
            Cmd::Rect {
                x,
                y: top,
                w: width,
                h: (y - top).max(0),
                color: bg,
            },
        );
    }
    y + c.margin
}

/// Recursively gather a word stream from an inline element subtree. `<a href>`
/// registers its target in `links` and tags every word below it.
fn collect_inline(
    el: &Element,
    sheet: &Stylesheet,
    parent: &Computed,
    out: &mut Vec<Word>,
    links: &mut Vec<String>,
    link: Option<usize>,
) {
    if el.tag == "br" {
        out.push(Word {
            text: "\n".into(),
            color: parent.color,
            scale: parent.scale,
            bold: parent.bold,
            link: None,
        });
        return;
    }
    let mut link = link;
    if el.tag == "a"
        && let Some(href) = el.attrs.get("href")
        && links.len() < MAX_LINKS
    {
        link = Some(links.len());
        links.push(href.clone());
    }
    for node in &el.children {
        match node {
            Node::Text(t) => push_words(out, t, parent, link),
            Node::Element(child) => {
                let c = compute(child, sheet, parent);
                if c.display != Disp::None {
                    collect_inline(child, sheet, &c, out, links, link);
                }
            }
        }
    }
}

fn push_words(out: &mut Vec<Word>, text: &str, c: &Computed, link: Option<usize>) {
    for w in text.split(' ') {
        if w.is_empty() {
            continue;
        }
        out.push(Word {
            text: w.into(),
            color: c.color,
            scale: c.scale,
            bold: c.bold,
            link,
        });
    }
}

/// Emit the buffered inline words as wrapped, optionally centered, line boxes.
fn flush_inline(
    words: &mut Vec<Word>,
    x: i32,
    mut y: i32,
    width: i32,
    align: Align,
    p: &mut Painter,
) -> i32 {
    if words.is_empty() {
        return y;
    }
    // Group consecutive words into lines that fit `width`.
    let mut line: Vec<&Word> = Vec::new();
    let mut line_w = 0;
    let mut max_scale = 1u8;
    let flush_line = |line: &mut Vec<&Word>,
                      line_w: &mut i32,
                      max_scale: &mut u8,
                      y: &mut i32,
                      p: &mut Painter| {
        if line.is_empty() {
            return;
        }
        let mut lx = x;
        if align == Align::Center && *line_w < width {
            lx += (width - *line_w) / 2;
        }
        for w in line.iter() {
            if let Some(link) = w.link {
                p.hits.push(LinkHit {
                    x: lx,
                    y: *y,
                    w: char_w(w.scale) * w.text.chars().count() as i32,
                    h: line_h(w.scale),
                    link,
                });
            }
            p.cmds.push(Cmd::Text {
                x: lx,
                y: *y,
                text: w.text.clone(),
                color: w.color,
                scale: w.scale,
                bold: w.bold,
            });
            lx += char_w(w.scale) * w.text.chars().count() as i32 + char_w(w.scale);
        }
        *y += line_h(*max_scale);
        line.clear();
        *line_w = 0;
        *max_scale = 1;
    };

    for w in words.iter() {
        if w.text == "\n" {
            flush_line(&mut line, &mut line_w, &mut max_scale, &mut y, p);
            continue;
        }
        let ww = char_w(w.scale) * w.text.chars().count() as i32;
        let space = if line.is_empty() { 0 } else { char_w(w.scale) };
        if line_w + space + ww > width && !line.is_empty() {
            flush_line(&mut line, &mut line_w, &mut max_scale, &mut y, p);
        }
        line_w += if line.is_empty() { ww } else { space + ww };
        max_scale = max_scale.max(w.scale);
        line.push(w);
    }
    flush_line(&mut line, &mut line_w, &mut max_scale, &mut y, p);
    words.clear();
    y
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    fn texts(page: &Page) -> Vec<String> {
        page.cmds
            .iter()
            .filter_map(|c| match c {
                Cmd::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn renders_heading_and_paragraph() {
        let page = render(b"<h1>Title</h1><p>Hello world</p>", 600);
        let t = texts(&page);
        assert!(t.iter().any(|s| s == "Title"));
        assert!(t.iter().any(|s| s == "Hello"));
        assert!(t.iter().any(|s| s == "world"));
        assert!(page.height > 0);
    }

    #[test]
    fn heading_is_larger_than_paragraph() {
        let page = render(b"<h1>Big</h1><p>small</p>", 600);
        let big = page.cmds.iter().find_map(|c| match c {
            Cmd::Text { text, scale, .. } if text == "Big" => Some(*scale),
            _ => None,
        });
        let small = page.cmds.iter().find_map(|c| match c {
            Cmd::Text { text, scale, .. } if text == "small" => Some(*scale),
            _ => None,
        });
        assert!(big > small);
    }

    #[test]
    fn css_color_and_background_applied() {
        let page = render(
            b"<style>.hl{color:#ff0000} body{background:#ffffff}</style><p class=hl>red</p>",
            600,
        );
        let red = page.cmds.iter().any(|c| matches!(c, Cmd::Text { text, color, .. } if text == "red" && *color == Rgb(255,0,0)));
        assert!(red, "class color should apply");
    }

    #[test]
    fn display_none_hides_content() {
        let page = render(b"<p>shown</p><div style='display:none'>hidden</div>", 600);
        let t = texts(&page);
        assert!(t.iter().any(|s| s == "shown"));
        assert!(!t.iter().any(|s| s == "hidden"));
    }

    #[test]
    fn long_text_wraps_within_width() {
        let long = "word ".repeat(100);
        let html = alloc::format!("<p>{long}</p>");
        let page = render(html.as_bytes(), 300);
        // Multiple lines → distinct y values among the text commands.
        let ys: Vec<i32> = page
            .cmds
            .iter()
            .filter_map(|c| match c {
                Cmd::Text { y, .. } => Some(*y),
                _ => None,
            })
            .collect();
        assert!(ys.iter().max() > ys.iter().min());
    }

    /// Regression: CSS lengths were unbounded i32s fed straight into layout
    /// arithmetic (`y += margin`, `width - 2 * padding`, ...). A page with
    /// `margin:2147483647` panicked with overflow checks and, in the kernel's
    /// release build, wrapped into garbage (negative) coordinates that the
    /// rasterizer then cast to usize.
    #[test]
    fn huge_css_lengths_do_not_overflow_layout() {
        let html = b"<div style='margin:2147483647;padding:2147483647'>\
            <p style='margin:2147483647;padding:2147483647'>x</p>\
            <p style='margin:2147483647'>y</p></div>\
            <p style='font-size:2147483647px;margin:99999999999999999999'>z</p>";
        let page = render(html, 600);
        assert!(page.height >= 0);
        for c in &page.cmds {
            match c {
                Cmd::Rect { x, y, w, h, .. } => {
                    assert!(*x >= 0 && *y >= 0 && *w >= 0 && *h >= 0);
                }
                Cmd::Text { x, y, .. } => assert!(*x >= 0 && *y >= 0),
            }
        }
    }

    #[test]
    fn links_get_the_ua_blue() {
        let page = render(b"<p>see <a href=x>this link</a> ok</p>", 600);
        let link_blue = page.cmds.iter().any(|c| matches!(c, Cmd::Text { text, color, .. } if text == "link" && *color == Rgb(0x15,0x65,0xC0)));
        assert!(link_blue);
    }

    #[test]
    fn links_are_recorded_with_hit_boxes() {
        let page = render(
            b"<p>go <a href='/x'>to x</a> or <a href=y>there</a></p>",
            600,
        );
        assert_eq!(page.links, ["/x", "y"]);
        // "to", "x", "there": one box per word of link text.
        assert_eq!(page.hits.len(), 3);
        let first = page.hits[0];
        assert_eq!(page.link_at(first.x + 1, first.y + 1), Some("/x"));
        let last = page.hits[2];
        assert_eq!(
            page.link_at(last.x + last.w - 1, last.y + last.h - 1),
            Some("y")
        );
        // Plain text is not a link; neither is outside every box.
        assert_eq!(page.link_at(0, 0), None);
        assert_eq!(page.link_at(first.x, first.y + first.h), None);
    }

    #[test]
    fn nested_inline_inside_a_link_stays_clickable() {
        let page = render(b"<a href='/n'>plain <b>bold</b></a> after", 600);
        assert_eq!(page.links, ["/n"]);
        assert_eq!(page.hits.len(), 2);
        assert!(page.hits.iter().all(|h| page.links[h.link] == "/n"));
    }

    #[test]
    fn anchor_without_href_is_not_a_link_and_links_are_capped() {
        let page = render(b"<a name=top>x</a><a>y</a>", 600);
        assert!(page.links.is_empty() && page.hits.is_empty());
        let mut html = String::new();
        for i in 0..(MAX_LINKS + 50) {
            html.push_str(&alloc::format!("<a href=/{i}>l</a> "));
        }
        let page = render(html.as_bytes(), 600);
        assert!(page.links.len() <= MAX_LINKS);
    }
}
