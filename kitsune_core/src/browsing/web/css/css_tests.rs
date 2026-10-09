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

#[test]
fn the_tag_index_offers_every_rule_that_can_match_in_rule_order() {
    let mut ss =
        parse_css("p{a:1} .x{a:2} div p{a:3} h1,p{a:4} *{a:5} [id]{a:6} span > b{a:7} a, .y{a:8}");
    // Without an index every rule is a candidate.
    assert!(ss.candidates("p").is_none());
    ss.build_index();
    let names = |t: &str| -> Vec<usize> {
        ss.candidates(t)
            .unwrap()
            .iter()
            .map(|&i| i as usize)
            .collect()
    };
    // Rules ending in `p` plus the ones that name no element.
    assert_eq!(names("p"), [0, 1, 2, 3, 4, 5, 7]);
    // An element no rule names still gets the generic ones, in order.
    assert_eq!(names("section"), [1, 4, 5, 7]);
    assert_eq!(names("h1"), [1, 3, 4, 5, 7]);
    assert_eq!(names("b"), [1, 4, 5, 6, 7]);
    assert_eq!(names("a"), [1, 4, 5, 7]);
    for t in ["p", "section", "h1", "b", "a"] {
        let v = names(t);
        assert!(v.windows(2).all(|w| w[0] < w[1]), "{t}: {v:?}");
    }
}
