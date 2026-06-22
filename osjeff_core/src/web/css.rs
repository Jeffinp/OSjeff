//! CSS parser: rules, simple selectors, specificity.

use super::dom::Element;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

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

pub(crate) fn parse_decls(text: &str) -> Vec<Decl> {
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

#[cfg(test)]
mod css_tests {
    use super::super::dom::{Node, parse_html};
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
