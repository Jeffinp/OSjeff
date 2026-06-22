//! DOM tree and a tolerant HTML parser (also captures <style> CSS).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ---- DOM ----

/// A DOM node: either an element or a run of text.
#[derive(Debug)]
pub enum Node {
    Element(Element),
    Text(String),
}

/// An element node with its tag, attributes and children.
#[derive(Debug)]
pub struct Element {
    pub tag: String,
    pub attrs: BTreeMap<String, String>,
    pub children: Vec<Node>,
}

impl Element {
    pub fn id(&self) -> Option<&str> {
        self.attrs.get("id").map(|s| s.as_str())
    }
    pub fn classes(&self) -> impl Iterator<Item = &str> {
        self.attrs
            .get("class")
            .map(|s| s.as_str())
            .unwrap_or("")
            .split_ascii_whitespace()
    }
}

/// HTML elements that never have children (void elements).
fn is_void(tag: &str) -> bool {
    matches!(
        tag,
        "br" | "img"
            | "hr"
            | "meta"
            | "link"
            | "input"
            | "area"
            | "base"
            | "col"
            | "embed"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Parse an HTML document into a DOM tree plus the concatenated text of every
/// `<style>` element (the page's CSS). Tolerant of malformed markup: unknown
/// tags pass through, mismatched close tags pop to the nearest match.
pub fn parse_html(input: &[u8]) -> (Vec<Node>, String) {
    let s = core::str::from_utf8(input).unwrap_or("");
    let mut p = HtmlParser {
        b: s.as_bytes(),
        i: 0,
        css: String::new(),
    };
    let nodes = p.parse_nodes(&mut Vec::new());
    (nodes, p.css)
}

struct HtmlParser<'a> {
    b: &'a [u8],
    i: usize,
    css: String,
}

impl HtmlParser<'_> {
    fn eof(&self) -> bool {
        self.i >= self.b.len()
    }
    fn peek(&self) -> u8 {
        self.b[self.i]
    }
    fn starts_with(&self, s: &[u8]) -> bool {
        self.b[self.i..].starts_with(s)
    }

    /// Parse sibling nodes until EOF or an unmatched close tag whose name is on
    /// the `open` stack (so the caller can pop to it).
    fn parse_nodes(&mut self, open: &mut Vec<String>) -> Vec<Node> {
        let mut nodes = Vec::new();
        while !self.eof() {
            if self.starts_with(b"<!--") {
                self.skip_comment();
                continue;
            }
            if self.starts_with(b"<!") {
                self.skip_until(b'>');
                continue;
            }
            if self.starts_with(b"</") {
                // A close tag: stop so the matching opener can consume it.
                break;
            }
            if self.peek() == b'<' {
                if let Some(node) = self.parse_element(open) {
                    nodes.push(node);
                }
            } else {
                let text = self.parse_text();
                if !text.trim().is_empty() {
                    nodes.push(Node::Text(text));
                }
            }
        }
        nodes
    }

    fn parse_text(&mut self) -> String {
        let start = self.i;
        while !self.eof() && self.peek() != b'<' {
            self.i += 1;
        }
        decode_text(&self.b[start..self.i])
    }

    fn parse_element(&mut self, open: &mut Vec<String>) -> Option<Node> {
        // '<'
        self.i += 1;
        let tag = self.parse_name().to_ascii_lowercase();
        if tag.is_empty() {
            self.skip_until(b'>');
            return None;
        }
        let attrs = self.parse_attrs();
        let self_closing = self.consume_tag_end();

        // <script>/<style>: consume raw text up to the matching close tag.
        if tag == "script" || tag == "style" {
            let raw = self.read_raw_until_close(&tag);
            if tag == "style" {
                self.css.push_str(&raw);
                self.css.push('\n');
            }
            return None;
        }

        if self_closing || is_void(&tag) {
            return Some(Node::Element(Element {
                tag,
                attrs,
                children: Vec::new(),
            }));
        }

        open.push(tag.clone());
        let children = self.parse_nodes(open);
        // Consume the matching close tag if present.
        if self.starts_with(b"</") {
            let save = self.i;
            self.i += 2;
            let close = self.parse_name().to_ascii_lowercase();
            self.skip_until(b'>');
            // Mismatched close tag that an ancestor opened: rewind so it can
            // handle the close (auto-closing this element).
            if close != tag && open.contains(&close) {
                self.i = save;
            }
        }
        open.pop();
        Some(Node::Element(Element {
            tag,
            attrs,
            children,
        }))
    }

    fn parse_name(&mut self) -> String {
        let start = self.i;
        while !self.eof() {
            let c = self.peek();
            if c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b':' {
                self.i += 1;
            } else {
                break;
            }
        }
        String::from_utf8_lossy(&self.b[start..self.i]).into_owned()
    }

    fn parse_attrs(&mut self) -> BTreeMap<String, String> {
        let mut attrs = BTreeMap::new();
        loop {
            self.skip_ws();
            if self.eof() || self.peek() == b'>' || self.starts_with(b"/>") {
                break;
            }
            let name = self.parse_name().to_ascii_lowercase();
            if name.is_empty() {
                // Stray character (e.g. a lone '/'): skip it to make progress.
                self.i += 1;
                continue;
            }
            self.skip_ws();
            let value = if !self.eof() && self.peek() == b'=' {
                self.i += 1;
                self.skip_ws();
                self.parse_attr_value()
            } else {
                String::new()
            };
            attrs.insert(name, value);
        }
        attrs
    }

    fn parse_attr_value(&mut self) -> String {
        if self.eof() {
            return String::new();
        }
        let q = self.peek();
        if q == b'"' || q == b'\'' {
            self.i += 1;
            let start = self.i;
            while !self.eof() && self.peek() != q {
                self.i += 1;
            }
            let v = String::from_utf8_lossy(&self.b[start..self.i]).into_owned();
            if !self.eof() {
                self.i += 1; // closing quote
            }
            v
        } else {
            let start = self.i;
            while !self.eof() && !self.peek().is_ascii_whitespace() && self.peek() != b'>' {
                self.i += 1;
            }
            String::from_utf8_lossy(&self.b[start..self.i]).into_owned()
        }
    }

    /// Consume the `>` (or `/>`) that ends a start tag. Returns true if it was
    /// self-closing.
    fn consume_tag_end(&mut self) -> bool {
        self.skip_ws();
        let mut self_closing = false;
        if self.starts_with(b"/>") {
            self_closing = true;
            self.i += 2;
        } else if !self.eof() && self.peek() == b'>' {
            self.i += 1;
        } else {
            self.skip_until(b'>');
        }
        self_closing
    }

    fn read_raw_until_close(&mut self, tag: &str) -> String {
        let start = self.i;
        let needle_lower = alloc::format!("</{tag}");
        loop {
            if self.eof() {
                let raw = String::from_utf8_lossy(&self.b[start..self.i]).into_owned();
                return raw;
            }
            if self.b[self.i..].len() >= needle_lower.len() {
                let win = &self.b[self.i..self.i + needle_lower.len()];
                if win.eq_ignore_ascii_case(needle_lower.as_bytes()) {
                    let raw = String::from_utf8_lossy(&self.b[start..self.i]).into_owned();
                    self.skip_until(b'>');
                    return raw;
                }
            }
            self.i += 1;
        }
    }

    fn skip_ws(&mut self) {
        while !self.eof() && self.peek().is_ascii_whitespace() {
            self.i += 1;
        }
    }
    fn skip_until(&mut self, ch: u8) {
        while !self.eof() && self.peek() != ch {
            self.i += 1;
        }
        if !self.eof() {
            self.i += 1;
        }
    }
    fn skip_comment(&mut self) {
        self.i += 4; // <!--
        while !self.eof() && !self.starts_with(b"-->") {
            self.i += 1;
        }
        self.i = (self.i + 3).min(self.b.len());
    }
}

/// Decode HTML entities and collapse runs of ASCII whitespace into single
/// spaces (HTML's normal whitespace handling for flow content).
fn decode_text(bytes: &[u8]) -> String {
    let mut out = String::new();
    let mut last_space = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
            i += 1;
            continue;
        }
        last_space = false;
        if c == b'&' {
            let mut j = i + 1;
            while j < bytes.len() && j < i + 12 && bytes[j] != b';' {
                j += 1;
            }
            if j < bytes.len()
                && bytes[j] == b';'
                && let Some(ch) = crate::browser::decode_entity(&bytes[i + 1..j])
            {
                out.push(ch as char);
                i = j + 1;
                continue;
            }
            out.push('&');
            i += 1;
        } else if c >= 0x80 {
            let (cp, len) = crate::browser::decode_utf8(&bytes[i..]);
            if let Some(b) = crate::browser::fold_ascii(cp) {
                out.push(b as char);
            }
            i += len;
        } else {
            out.push(c as char);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod html_tests {
    use super::*;

    fn first_element(nodes: &[Node]) -> &Element {
        nodes
            .iter()
            .find_map(|n| match n {
                Node::Element(e) => Some(e),
                _ => None,
            })
            .expect("an element")
    }

    #[test]
    fn parses_nested_elements_and_text() {
        let (nodes, _) = parse_html(b"<div id=x class='a b'><p>Hello <b>world</b></p></div>");
        let div = first_element(&nodes);
        assert_eq!(div.tag, "div");
        assert_eq!(div.id(), Some("x"));
        let classes: Vec<_> = div.classes().collect();
        assert_eq!(classes, ["a", "b"]);
        let p = first_element(&div.children);
        assert_eq!(p.tag, "p");
    }

    #[test]
    fn captures_style_css_and_skips_script() {
        let (nodes, css) =
            parse_html(b"<style>p { color: red; }</style><script>var x=1<2;</script><p>hi</p>");
        assert!(css.contains("color: red"));
        // Only the <p> survives as an element (script/style produce no nodes).
        assert_eq!(
            nodes
                .iter()
                .filter(|n| matches!(n, Node::Element(_)))
                .count(),
            1
        );
    }

    #[test]
    fn void_and_self_closing_tags() {
        let (nodes, _) = parse_html(b"<div>a<br>b<img src=x/>c</div>");
        let div = first_element(&nodes);
        // text "a", br, text "b", img, text "c"
        assert!(div.children.len() >= 3);
    }

    fn count_elements(nodes: &[Node]) -> usize {
        nodes
            .iter()
            .map(|n| match n {
                Node::Element(e) => 1 + count_elements(&e.children),
                Node::Text(_) => 0,
            })
            .sum()
    }

    #[test]
    fn tolerates_unclosed_tags() {
        // Both <p> elements are recovered (nested rather than siblings — full
        // optional-end-tag handling is out of scope, but nothing is lost).
        let (nodes, _) = parse_html(b"<p>one<p>two");
        assert_eq!(count_elements(&nodes), 2);
    }

    #[test]
    fn decodes_entities_in_text() {
        let (nodes, _) = parse_html(b"<p>a &amp; b &#233;</p>");
        let p = first_element(&nodes);
        if let Node::Text(t) = &p.children[0] {
            assert!(t.contains("a & b"));
            assert!(t.contains('e')); // &#233; (é) folded to 'e'
        } else {
            panic!("expected text");
        }
    }
}
