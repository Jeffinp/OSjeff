//! A minimal HTML + CSS rendering engine (box model), inspired by Matt
//! Brubeck's "robinson" toy engine. Pure and `alloc`-only, so the whole
//! pipeline is unit-tested on the host.
//!
//! Pipeline: HTML bytes → DOM tree → (user-agent + page CSS) → styled tree →
//! block layout with inline text flow → a flat display list of rectangles and
//! text runs. The kernel rasterizes that display list to the framebuffer.
//!
//! Scope is deliberately small: it renders simple, mostly-static HTML/CSS
//! correctly and degrades real-world pages to a readable single column (block
//! stacking + the UA stylesheet for headings/links/lists). It is NOT a
//! standards browser — no flexbox/grid/float/JS/images.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

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

// ---- CSS ----

/// A parsed stylesheet: an ordered list of rules.
#[derive(Debug, Default)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

#[derive(Debug)]
pub struct Rule {
    pub selectors: Vec<Selector>,
    pub decls: Vec<Decl>,
}

/// A single simple selector (we use the rightmost compound selector of any
/// complex selector — combinators/ancestors are ignored, which over-matches but
/// keeps the engine small).
#[derive(Debug, Default, Clone)]
pub struct Selector {
    pub tag: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Decl {
    pub name: String,
    pub value: String,
}

/// CSS specificity as `(ids, classes, tags)`, compared lexicographically.
pub type Specificity = (usize, usize, usize);

impl Selector {
    pub fn specificity(&self) -> Specificity {
        (
            self.id.is_some() as usize,
            self.classes.len(),
            self.tag.is_some() as usize,
        )
    }

    /// Does this selector match `el`?
    pub fn matches(&self, el: &Element) -> bool {
        if let Some(t) = &self.tag
            && t != "*"
            && *t != el.tag
        {
            return false;
        }
        if let Some(id) = &self.id
            && el.id() != Some(id.as_str())
        {
            return false;
        }
        for class in &self.classes {
            if !el.classes().any(|c| c == class) {
                return false;
            }
        }
        true
    }
}

/// Parse a stylesheet. Tolerant: malformed rules and unsupported `@` rules are
/// skipped without aborting the parse.
pub fn parse_css(input: &str) -> Stylesheet {
    let b = input.as_bytes();
    let mut i = 0;
    let mut rules = Vec::new();
    while i < b.len() {
        i = skip_css_ws(b, i);
        if i >= b.len() {
            break;
        }
        if b[i] == b'@' {
            // Skip @import; (to ';') or @media {...} (to matching '}').
            i = skip_at_rule(b, i);
            continue;
        }
        // Selector list up to '{'.
        let sel_start = i;
        while i < b.len() && b[i] != b'{' && b[i] != b'}' {
            i += 1;
        }
        if i >= b.len() || b[i] == b'}' {
            break;
        }
        let sel_text = core::str::from_utf8(&b[sel_start..i]).unwrap_or("");
        i += 1; // '{'
        let decl_start = i;
        while i < b.len() && b[i] != b'}' {
            i += 1;
        }
        let decl_text = core::str::from_utf8(&b[decl_start..i]).unwrap_or("");
        if i < b.len() {
            i += 1; // '}'
        }
        let selectors = parse_selectors(sel_text);
        let decls = parse_decls(decl_text);
        if !selectors.is_empty() && !decls.is_empty() {
            rules.push(Rule { selectors, decls });
        }
    }
    Stylesheet { rules }
}

fn skip_css_ws(b: &[u8], mut i: usize) -> usize {
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i = (i + 2).min(b.len());
        } else {
            return i;
        }
    }
}

fn skip_at_rule(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i] != b'{' && b[i] != b';' {
        i += 1;
    }
    if i < b.len() && b[i] == b';' {
        return i + 1;
    }
    // Balanced block.
    let mut depth = 0;
    while i < b.len() {
        match b[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    i
}

fn parse_selectors(text: &str) -> Vec<Selector> {
    let mut out = Vec::new();
    for part in text.split(',') {
        // Use the rightmost compound selector of a complex selector.
        let last = part.split_ascii_whitespace().last().unwrap_or("").trim();
        if last.is_empty() {
            continue;
        }
        if let Some(sel) = parse_simple_selector(last) {
            out.push(sel);
        }
    }
    out
}

fn parse_simple_selector(s: &str) -> Option<Selector> {
    let mut sel = Selector::default();
    let bytes = s.as_bytes();
    let mut i = 0;
    // Leading tag or universal.
    if i < bytes.len() && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'*') {
        let start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'*')
        {
            i += 1;
        }
        sel.tag = Some(s[start..i].to_ascii_lowercase());
    }
    while i < bytes.len() {
        let kind = bytes[i];
        if kind != b'.' && kind != b'#' {
            break;
        }
        i += 1;
        let start = i;
        while i < bytes.len()
            && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_')
        {
            i += 1;
        }
        let name = s[start..i].to_string();
        if name.is_empty() {
            break;
        }
        if kind == b'.' {
            sel.classes.push(name);
        } else {
            sel.id = Some(name);
        }
    }
    if sel.tag.is_none() && sel.id.is_none() && sel.classes.is_empty() {
        None
    } else {
        Some(sel)
    }
}

fn parse_decls(text: &str) -> Vec<Decl> {
    let mut out = Vec::new();
    for chunk in text.split(';') {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        if let Some((name, value)) = chunk.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if !name.is_empty() && !value.is_empty() {
                out.push(Decl { name, value });
            }
        }
    }
    out
}

// ---- style: resolve computed values per element ----

/// A user-agent default stylesheet: the baseline look browsers ship (block
/// elements stack, headings are bold and larger, links are blue, lists indent).
const UA_CSS: &str = "
html,body,div,p,h1,h2,h3,h4,h5,h6,ul,ol,header,footer,article,section,nav,main,blockquote,pre,table,tr,form,figure,figcaption{display:block}
li{display:list-item}
script,style,head,title,meta,link{display:none}
h1{font-size:30px;font-weight:bold;margin:16px}
h2{font-size:25px;font-weight:bold;margin:14px}
h3{font-size:21px;font-weight:bold;margin:12px}
h4,h5,h6{font-size:18px;font-weight:bold;margin:10px}
p{margin:10px}
ul,ol{margin:10px;padding:20px}
blockquote{margin:14px}
a{color:#1565C0}
b,strong{font-weight:bold}
body{color:#14233A;font-size:16px}
";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Disp {
    Block,
    Inline,
    ListItem,
    None,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Align {
    Left,
    Center,
}

/// Computed style for one element: only the properties our renderer uses.
#[derive(Clone)]
struct Computed {
    display: Disp,
    color: Rgb,
    bg: Option<Rgb>,
    scale: u8,
    bold: bool,
    margin: i32,
    padding: i32,
    align: Align,
}

impl Computed {
    /// The inherited root style (the `body` defaults before any element).
    fn root() -> Self {
        Computed {
            display: Disp::Block,
            color: Rgb(0x14, 0x23, 0x3A),
            bg: None,
            scale: 2,
            bold: false,
            margin: 0,
            padding: 0,
            align: Align::Left,
        }
    }
}

fn default_display(tag: &str) -> Disp {
    match tag {
        "html" | "body" | "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol"
        | "header" | "footer" | "article" | "section" | "nav" | "main" | "blockquote" | "pre"
        | "table" | "tr" | "form" | "figure" | "figcaption" => Disp::Block,
        "li" => Disp::ListItem,
        "script" | "style" | "head" | "title" | "meta" | "link" => Disp::None,
        _ => Disp::Inline,
    }
}

/// Map a CSS pixel font size to one of our bitmap font scales (1..6).
fn scale_from_px(px: i32) -> u8 {
    ((px + 4) / 8).clamp(1, 6) as u8
}

/// Parse a leading CSS length like `12px` / `12` into pixels.
fn parse_px(s: &str) -> Option<i32> {
    let s = s.trim().trim_end_matches("px").trim();
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse::<i32>().ok()
}

/// Collect the declarations matching `el` from `sheet`, lowest specificity
/// first, so later (higher-specificity) declarations override earlier ones.
fn matched_decls<'a>(el: &Element, sheet: &'a Stylesheet) -> Vec<&'a Decl> {
    let mut rules: Vec<(Specificity, &Rule)> = sheet
        .rules
        .iter()
        .filter_map(|r| {
            r.selectors
                .iter()
                .filter(|s| s.matches(el))
                .map(|s| s.specificity())
                .max()
                .map(|sp| (sp, r))
        })
        .collect();
    rules.sort_by_key(|(sp, _)| *sp);
    rules.into_iter().flat_map(|(_, r)| r.decls.iter()).collect()
}

/// Resolve the computed style for `el` given its inherited parent style.
fn compute(el: &Element, sheet: &Stylesheet, parent: &Computed) -> Computed {
    let mut c = Computed {
        display: default_display(&el.tag),
        color: parent.color,
        bg: None,
        scale: parent.scale,
        bold: parent.bold,
        margin: 0,
        padding: 0,
        align: parent.align,
    };
    let mut decls = matched_decls(el, sheet);
    // Inline `style="..."` wins over everything from the stylesheets.
    let inline: Vec<Decl> = el
        .attrs
        .get("style")
        .map(|s| parse_decls(s))
        .unwrap_or_default();
    let inline_refs: Vec<&Decl> = inline.iter().collect();
    decls.extend(inline_refs);

    for d in decls {
        let v = d.value.trim();
        match d.name.as_str() {
            "display" => {
                c.display = match v {
                    "block" => Disp::Block,
                    "inline" | "inline-block" => Disp::Inline,
                    "list-item" => Disp::ListItem,
                    "none" => Disp::None,
                    _ => c.display,
                }
            }
            "color" => {
                if let Some(rgb) = parse_color(v) {
                    c.color = rgb;
                }
            }
            "background" | "background-color" => {
                c.bg = parse_color(v.split_whitespace().next().unwrap_or(v));
            }
            "font-size" => {
                if let Some(px) = parse_px(v) {
                    c.scale = scale_from_px(px);
                }
            }
            "font-weight" => {
                c.bold = matches!(v, "bold" | "bolder" | "600" | "700" | "800" | "900");
            }
            "text-align" => {
                c.align = if v == "center" {
                    Align::Center
                } else {
                    Align::Left
                };
            }
            "margin" | "margin-top" => {
                if let Some(px) = parse_px(v) {
                    c.margin = px;
                }
            }
            "padding" | "padding-left" => {
                if let Some(px) = parse_px(v) {
                    c.padding = px;
                }
            }
            _ => {}
        }
    }
    c
}

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
}

struct Painter {
    cmds: Vec<Cmd>,
}

/// Render an HTML document to a display list laid out for `viewport_w` pixels.
pub fn render(html: &[u8], viewport_w: i32) -> Page {
    let (dom, css) = parse_html(html);
    let mut sheet = parse_css(UA_CSS);
    sheet.rules.extend(parse_css(&css).rules);

    let root = Computed::root();
    let mut painter = Painter { cmds: Vec::new() };
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
            Node::Text(t) => push_words(&mut inline, t, parent),
            Node::Element(el) => {
                let c = compute(el, sheet, parent);
                match c.display {
                    Disp::None => {}
                    Disp::Inline => collect_inline(el, sheet, &c, &mut inline),
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

/// Recursively gather a word stream from an inline element subtree.
fn collect_inline(el: &Element, sheet: &Stylesheet, parent: &Computed, out: &mut Vec<Word>) {
    if el.tag == "br" {
        out.push(Word {
            text: "\n".into(),
            color: parent.color,
            scale: parent.scale,
            bold: parent.bold,
        });
        return;
    }
    for node in &el.children {
        match node {
            Node::Text(t) => push_words(out, t, parent),
            Node::Element(child) => {
                let c = compute(child, sheet, parent);
                if c.display != Disp::None {
                    collect_inline(child, sheet, &c, out);
                }
            }
        }
    }
}

fn push_words(out: &mut Vec<Word>, text: &str, c: &Computed) {
    for w in text.split(' ') {
        if w.is_empty() {
            continue;
        }
        out.push(Word {
            text: w.into(),
            color: c.color,
            scale: c.scale,
            bold: c.bold,
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
    let flush_line =
        |line: &mut Vec<&Word>, line_w: &mut i32, max_scale: &mut u8, y: &mut i32, p: &mut Painter| {
            if line.is_empty() {
                return;
            }
            let mut lx = x;
            if align == Align::Center && *line_w < width {
                lx += (width - *line_w) / 2;
            }
            for w in line.iter() {
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

    #[test]
    fn links_get_the_ua_blue() {
        let page = render(b"<p>see <a href=x>this link</a> ok</p>", 600);
        let link_blue = page.cmds.iter().any(|c| matches!(c, Cmd::Text { text, color, .. } if text == "link" && *color == Rgb(0x15,0x65,0xC0)));
        assert!(link_blue);
    }
}

#[cfg(test)]
mod css_tests {
    use super::*;

    #[test]
    fn parses_rules_and_decls() {
        let ss = parse_css("h1, .big { color: #fff; font-size: 32px; } p{margin:10px}");
        assert_eq!(ss.rules.len(), 2);
        assert_eq!(ss.rules[0].selectors.len(), 2);
        assert_eq!(ss.rules[0].decls.len(), 2);
        assert_eq!(ss.rules[0].decls[0].name, "color");
    }

    #[test]
    fn selector_matching_and_specificity() {
        let (nodes, _) = parse_html(b"<p id=lead class='a b'>x</p>");
        let el = match &nodes[0] {
            Node::Element(e) => e,
            _ => panic!(),
        };
        assert!(parse_simple_selector("p").unwrap().matches(el));
        assert!(parse_simple_selector(".a").unwrap().matches(el));
        assert!(parse_simple_selector("#lead").unwrap().matches(el));
        assert!(parse_simple_selector("p.a.b").unwrap().matches(el));
        assert!(!parse_simple_selector(".c").unwrap().matches(el));
        assert!(
            parse_simple_selector("#lead").unwrap().specificity()
                > parse_simple_selector("p").unwrap().specificity()
        );
    }

    #[test]
    fn skips_at_rules_and_comments() {
        let ss = parse_css("@media x { p{color:red} } /* c */ a { color: blue; }");
        // The @media block is skipped; only the `a` rule remains.
        assert_eq!(ss.rules.len(), 1);
        assert_eq!(ss.rules[0].selectors[0].tag.as_deref(), Some("a"));
    }

    #[test]
    fn rightmost_compound_of_complex_selector() {
        let ss = parse_css("div.box > p span.hl { color: red }");
        let sel = &ss.rules[0].selectors[0];
        assert_eq!(sel.tag.as_deref(), Some("span"));
        assert_eq!(sel.classes, ["hl"]);
    }
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
