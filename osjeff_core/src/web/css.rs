//! CSS parser: rules, selectors (type, class, id, attribute, descendant and
//! child combinators), specificity and declaration lists.
//!
//! Anything the engine cannot honour is dropped at parse time instead of being
//! approximated: a selector with a pseudo-class (`a:hover`), a sibling combinator
//! or a pseudo-element never matches, so it cannot restyle the wrong elements.

use super::dom::Element;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---- CSS ----

/// Maximum number of rules kept from one stylesheet.
///
/// The cascade tests every rule against every element, so rules x elements is
/// the cost. With `MAX_NODES` elements this bounds the whole cascade to a few
/// million selector tests. Later rules are dropped (document order is cascade
/// order, so the page keeps its earliest, usually most structural, rules).
pub const MAX_RULES: usize = 1_000;

/// Maximum selectors summed over all rules of one stylesheet: a single rule
/// with a huge comma list would otherwise dodge [`MAX_RULES`].
pub const MAX_SELECTORS: usize = 2_000;

/// Longest compound chain of a selector (`a b > c d` is 4).
pub const MAX_COMPOUNDS: usize = 5;

/// Longest selector text considered, in bytes.
const MAX_SELECTOR_LEN: usize = 256;

/// Longest stylesheet text parsed, in bytes (the HTML body is capped far below this).
const MAX_CSS_BYTES: usize = 512 * 1024;

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

/// How an attribute selector compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrOp {
    /// `[name]`
    Exists,
    /// `[name=value]`
    Equals,
    /// `[name~=value]` (one of the space separated words)
    Word,
    /// `[name^=value]`
    Prefix,
    /// `[name$=value]`
    Suffix,
    /// `[name*=value]`
    Contains,
}

#[derive(Debug, Clone)]
pub struct AttrSel {
    pub name: String,
    pub op: AttrOp,
    pub value: String,
}

impl AttrSel {
    fn matches(&self, el: &Element) -> bool {
        let Some(v) = el.attrs.get(self.name.as_str()) else {
            return false;
        };
        let (v, want) = (v.as_str(), self.value.as_str());
        let eq = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
        match self.op {
            AttrOp::Exists => true,
            AttrOp::Equals => eq(v, want),
            AttrOp::Word => v.split_ascii_whitespace().any(|w| eq(w, want)),
            AttrOp::Prefix => {
                !want.is_empty() && v.len() >= want.len() && eq(&v[..want.len()], want)
            }
            AttrOp::Suffix => {
                !want.is_empty()
                    && v.len() >= want.len()
                    && v.get(v.len() - want.len()..).is_some_and(|t| eq(t, want))
            }
            AttrOp::Contains => {
                !want.is_empty() && v.to_ascii_lowercase().contains(&want.to_ascii_lowercase())
            }
        }
    }
}

/// One compound selector: `tag#id.class[attr]`.
#[derive(Debug, Default, Clone)]
pub struct Compound {
    pub tag: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attrs: Vec<AttrSel>,
}

impl Compound {
    fn matches(&self, el: &Element) -> bool {
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
        self.attrs.iter().all(|a| a.matches(el))
    }

    fn specificity(&self) -> Specificity {
        (
            usize::from(self.id.is_some()),
            self.classes.len() + self.attrs.len(),
            usize::from(self.tag.as_deref().is_some_and(|t| t != "*")),
        )
    }
}

/// How a compound relates to the one on its right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comb {
    /// `a b`: any ancestor.
    Descendant,
    /// `a > b`: the parent.
    Child,
}

/// A selector: the compound for the element itself (the rightmost one, kept in
/// the flat fields) and, nearest first, the compounds it must have above it.
#[derive(Debug, Default, Clone)]
pub struct Selector {
    pub tag: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub attrs: Vec<AttrSel>,
    /// Compounds to the left, nearest first, with how each relates to the one to its right.
    pub ancestors: Vec<(Comb, Compound)>,
}

#[derive(Debug, Clone)]
pub struct Decl {
    pub name: String,
    pub value: String,
    pub important: bool,
}

/// CSS specificity as `(ids, classes, tags)`, compared lexicographically.
pub type Specificity = (usize, usize, usize);

impl Selector {
    fn own(&self) -> Compound {
        Compound {
            tag: self.tag.clone(),
            id: self.id.clone(),
            classes: self.classes.clone(),
            attrs: self.attrs.clone(),
        }
    }

    pub fn specificity(&self) -> Specificity {
        let mut s = self.own().specificity();
        for (_, c) in &self.ancestors {
            let a = c.specificity();
            s = (s.0 + a.0, s.1 + a.1, s.2 + a.2);
        }
        s
    }

    /// Does the rightmost compound match `el`, ignoring the ancestors?
    pub fn matches_element(&self, el: &Element) -> bool {
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
        self.attrs.iter().all(|a| a.matches(el))
    }

    /// Does this selector match `el`, whose ancestors are `anc` (outermost first,
    /// the parent last)? `budget` bounds the ancestor walking of a whole layout:
    /// when it runs out, selectors that need an ancestor stop matching.
    pub fn matches(&self, el: &Element, anc: &[&Element], budget: &mut u32) -> bool {
        if !self.matches_element(el) {
            return false;
        }
        let mut idx = anc.len();
        for (comb, comp) in &self.ancestors {
            match comb {
                Comb::Child => {
                    if idx == 0 || *budget == 0 {
                        return false;
                    }
                    *budget -= 1;
                    idx -= 1;
                    if !comp.matches(anc[idx]) {
                        return false;
                    }
                }
                Comb::Descendant => loop {
                    if idx == 0 || *budget == 0 {
                        return false;
                    }
                    *budget -= 1;
                    idx -= 1;
                    if comp.matches(anc[idx]) {
                        break;
                    }
                },
            }
        }
        true
    }
}

/// Drop `/* ... */` comments.
fn strip_comments(input: &str) -> String {
    if !input.contains("/*") {
        return String::from(input);
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        match rest[i + 2..].find("*/") {
            Some(j) => rest = &rest[i + 2 + j + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Parse a stylesheet. Tolerant: malformed rules and unsupported `@` rules are
/// skipped without aborting the parse.
pub fn parse_css(input: &str) -> Stylesheet {
    let mut end = input.len().min(MAX_CSS_BYTES);
    while !input.is_char_boundary(end) {
        end -= 1;
    }
    let text = strip_comments(&input[..end]);
    let b = text.as_bytes();
    let mut i = 0;
    let mut rules = Vec::new();
    let mut selector_total = 0usize;
    while i < b.len() {
        if rules.len() >= MAX_RULES || selector_total >= MAX_SELECTORS {
            break;
        }
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
            // A stray '}' (the end of something unsupported): skip it and go on.
            if i < b.len() {
                i += 1;
                continue;
            }
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
        let mut selectors = parse_selectors(sel_text);
        // Keep only as many selectors as the sheet-wide budget has left.
        selectors.truncate(MAX_SELECTORS - selector_total);
        let decls = parse_decls(decl_text);
        if !selectors.is_empty() && !decls.is_empty() {
            selector_total += selectors.len();
            rules.push(Rule { selectors, decls });
        }
    }
    Stylesheet { rules }
}

fn skip_css_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
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
        let part = part.trim();
        if part.is_empty() || part.len() > MAX_SELECTOR_LEN {
            continue;
        }
        if let Some(sel) = parse_complex_selector(part) {
            out.push(sel);
        }
    }
    out
}

/// Split a complex selector into compounds and combinators and build the
/// [`Selector`]. `None` for anything unsupported.
fn parse_complex_selector(s: &str) -> Option<Selector> {
    let b = s.as_bytes();
    let mut parts: Vec<(Comb, Compound)> = Vec::new(); // (relation to the NEXT compound, compound)
    let mut i = 0;
    let mut pending = Comb::Descendant;
    while i < b.len() {
        match b[i] {
            c if c.is_ascii_whitespace() => {
                i += 1;
            }
            b'>' => {
                pending = Comb::Child;
                i += 1;
            }
            b'+' | b'~' => return None,
            _ => {
                let (comp, n) = parse_compound(&s[i..])?;
                i += n;
                if parts.len() >= MAX_COMPOUNDS {
                    return None;
                }
                parts.push((pending, comp));
                pending = Comb::Descendant;
            }
        }
    }
    // `pending` after the loop would be a dangling combinator ("a >"): invalid.
    if parts.is_empty() || (pending == Comb::Child) {
        return None;
    }
    // `parts[k].0` is the combinator between compound k-1 and compound k.
    let n = parts.len();
    let mut ancestors: Vec<(Comb, Compound)> = Vec::new();
    for k in (0..n - 1).rev() {
        ancestors.push((parts[k + 1].0, parts[k].1.clone()));
    }
    let last = parts.pop()?.1;
    Some(Selector {
        tag: last.tag,
        id: last.id,
        classes: last.classes,
        attrs: last.attrs,
        ancestors,
    })
}

/// Parse one compound at the start of `s`; returns it and the bytes used.
fn parse_compound(s: &str) -> Option<(Compound, usize)> {
    let b = s.as_bytes();
    let mut c = Compound::default();
    let mut i = 0;
    if i < b.len() && (b[i].is_ascii_alphabetic() || b[i] == b'*') {
        let start = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b'*') {
            i += 1;
        }
        c.tag = Some(s[start..i].to_ascii_lowercase());
    }
    while i < b.len() {
        match b[i] {
            b'.' | b'#' => {
                let kind = b[i];
                i += 1;
                let start = i;
                while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b'_')
                {
                    i += 1;
                }
                let name = s[start..i].to_string();
                if name.is_empty() {
                    return None;
                }
                if kind == b'.' {
                    c.classes.push(name);
                } else {
                    c.id = Some(name);
                }
            }
            b'[' => {
                let close = s[i..].find(']')? + i;
                c.attrs.push(parse_attr(&s[i + 1..close])?);
                i = close + 1;
            }
            b':' => {
                // `:root` is the one pseudo-class we can answer.
                if s[i..].starts_with(":root") && c.tag.is_none() {
                    c.tag = Some("html".to_string());
                    i += 5;
                } else {
                    return None;
                }
            }
            c if c.is_ascii_whitespace() || c == b'>' => break,
            b'+' | b'~' | b',' => return None,
            _ => return None,
        }
    }
    if c.tag.is_none() && c.id.is_none() && c.classes.is_empty() && c.attrs.is_empty() {
        return None;
    }
    Some((c, i))
}

fn parse_attr(inner: &str) -> Option<AttrSel> {
    let inner = inner.trim();
    let ops = [
        ("~=", AttrOp::Word),
        ("^=", AttrOp::Prefix),
        ("$=", AttrOp::Suffix),
        ("*=", AttrOp::Contains),
        ("=", AttrOp::Equals),
    ];
    for (tok, op) in ops {
        if let Some((n, v)) = inner.split_once(tok) {
            let name = n.trim().to_ascii_lowercase();
            if name.is_empty() {
                return None;
            }
            // `[a="b" i]` flags are ignored (matching is case-insensitive anyway).
            let v = v.trim();
            let v = v.split_once(" i").map_or(v, |(a, _)| a).trim();
            let v = v.trim_matches(|c| c == '"' || c == '\'');
            return Some(AttrSel {
                name,
                op,
                value: v.to_string(),
            });
        }
    }
    let name = inner.to_ascii_lowercase();
    if name.is_empty()
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return None;
    }
    Some(AttrSel {
        name,
        op: AttrOp::Exists,
        value: String::new(),
    })
}

pub(crate) fn parse_decls(text: &str) -> Vec<Decl> {
    let text = if text.contains("/*") {
        strip_comments(text)
    } else {
        String::from(text)
    };
    let mut out = Vec::new();
    for chunk in text.split(';') {
        let chunk = chunk.trim();
        if chunk.is_empty() {
            continue;
        }
        if let Some((name, value)) = chunk.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            let mut value = value.trim();
            let mut important = false;
            if let Some(p) = value.rfind('!')
                && value[p + 1..].trim().eq_ignore_ascii_case("important")
            {
                important = true;
                value = value[..p].trim();
            }
            if !name.is_empty() && !value.is_empty() {
                out.push(Decl {
                    name,
                    value: value.to_string(),
                    important,
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod css_tests {
    use super::super::dom::{Node, parse_html};
    use super::*;

    fn el_of(html: &str) -> Vec<Node> {
        parse_html(html.as_bytes()).0
    }

    fn first(nodes: &[Node]) -> &Element {
        match &nodes[0] {
            Node::Element(e) => e,
            _ => panic!("not an element"),
        }
    }

    fn sel(s: &str) -> Selector {
        parse_complex_selector(s).unwrap_or_else(|| panic!("{s} did not parse"))
    }

    fn m(s: &str, el: &Element, anc: &[&Element]) -> bool {
        let mut b = 1000;
        sel(s).matches(el, anc, &mut b)
    }

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
        let nodes = el_of("<p id=lead class='a b'>x</p>");
        let el = first(&nodes);
        assert!(m("p", el, &[]));
        assert!(m(".a", el, &[]));
        assert!(m("#lead", el, &[]));
        assert!(m("p.a.b", el, &[]));
        assert!(!m(".c", el, &[]));
        assert!(sel("#lead").specificity() > sel("p").specificity());
        assert!(sel("div p").specificity() > sel("p").specificity());
    }

    /// Regression: unbounded rule and selector counts made the cascade
    /// O(rules x elements) with no ceiling.
    #[test]
    fn rule_and_selector_counts_are_capped() {
        let mut css = String::new();
        for i in 0..(MAX_RULES * 3) {
            css.push_str(&format!(".r{i}{{color:red}}"));
        }
        let ss = parse_css(&css);
        assert_eq!(ss.rules.len(), MAX_RULES);
        // The first rules are the ones kept (document order = cascade order).
        assert_eq!(ss.rules[0].selectors[0].classes, ["r0"]);

        // One rule with a huge selector list cannot dodge the selector budget.
        let list = (0..MAX_SELECTORS * 4)
            .map(|i| format!(".s{i}"))
            .collect::<Vec<_>>()
            .join(",");
        let ss = parse_css(&format!("{list}{{color:red}} p{{margin:1px}}"));
        let total: usize = ss.rules.iter().map(|r| r.selectors.len()).sum();
        assert!(total <= MAX_SELECTORS, "{total} selectors kept");
    }

    #[test]
    fn skips_at_rules_and_comments() {
        let ss = parse_css("@media x { p{color:red} } /* c */ a { color: blue; }");
        // The @media block is skipped; only the `a` rule remains.
        assert_eq!(ss.rules.len(), 1);
        assert_eq!(ss.rules[0].selectors[0].tag.as_deref(), Some("a"));
    }

    #[test]
    fn complex_selectors_keep_their_ancestors() {
        let ss = parse_css("div.box > p span.hl { color: red }");
        let s = &ss.rules[0].selectors[0];
        assert_eq!(s.tag.as_deref(), Some("span"));
        assert_eq!(s.classes, ["hl"]);
        assert_eq!(s.ancestors.len(), 2);
        assert_eq!(s.ancestors[0].0, Comb::Descendant);
        assert_eq!(s.ancestors[0].1.tag.as_deref(), Some("p"));
        assert_eq!(s.ancestors[1].0, Comb::Child);
        assert_eq!(s.ancestors[1].1.classes, ["box"]);
    }

    #[test]
    fn descendant_and_child_combinators_match_the_chain() {
        let nodes = el_of("<div class=card><section><p class=t>x</p></section></div>");
        let div = first(&nodes);
        let Node::Element(section) = &div.children[0] else {
            panic!()
        };
        let Node::Element(p) = &section.children[0] else {
            panic!()
        };
        let anc = [div, section];
        assert!(m(".card p", p, &anc));
        assert!(m("div section > p.t", p, &anc));
        assert!(!m(".card > p", p, &anc));
        assert!(!m(".other p", p, &anc));
        assert!(!m(".card p", p, &[]));
    }

    #[test]
    fn unsupported_selectors_never_match() {
        for s in [
            "a:hover",
            "p::before",
            "li:first-child",
            "h1 + p",
            "h1 ~ p",
            "a >",
            "p:not(.x)",
        ] {
            assert!(parse_complex_selector(s).is_none(), "{s}");
        }
        // The whole rule is dropped, not widened to `a`.
        assert!(parse_css("a:hover{color:red}").rules.is_empty());
        // A list keeps its good members.
        let ss = parse_css("a:hover, b{color:red}");
        assert_eq!(ss.rules[0].selectors.len(), 1);
    }

    #[test]
    fn attribute_selectors() {
        let nodes = el_of("<input type=Submit class=go data-x='a b c'>");
        let el = first(&nodes);
        assert!(m("input[type=submit]", el, &[]));
        assert!(m("[type=\"SUBMIT\"]", el, &[]));
        assert!(m("[class]", el, &[]));
        assert!(m("[data-x~=b]", el, &[]));
        assert!(m("[data-x^='a ']", el, &[]));
        assert!(m("[data-x$=c]", el, &[]));
        assert!(m("[data-x*=' b ']", el, &[]));
        assert!(!m("[type=text]", el, &[]));
        assert!(!m("[nope]", el, &[]));
    }

    #[test]
    fn root_selects_html_and_important_is_flagged() {
        let ss = parse_css(":root{color:red} p{margin:0 !important;color:blue}");
        assert_eq!(ss.rules[0].selectors[0].tag.as_deref(), Some("html"));
        let d = &ss.rules[1].decls;
        assert!(d[0].important && d[0].value == "0");
        assert!(!d[1].important);
    }

    #[test]
    fn comments_inside_blocks_and_the_ancestor_budget() {
        let ss = parse_css("p{ /* x */ color:red; /* y */ margin:1px }");
        assert_eq!(ss.rules[0].decls.len(), 2);
        let nodes = el_of("<div><p>x</p></div>");
        let div = first(&nodes);
        let Node::Element(p) = &div.children[0] else {
            panic!()
        };
        let mut none = 0;
        assert!(!sel("div p").matches(p, &[div], &mut none));
        let mut some = 5;
        assert!(sel("div p").matches(p, &[div], &mut some));
    }
}
