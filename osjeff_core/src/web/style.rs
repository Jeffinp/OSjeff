//! Style cascade: combine the UA + page stylesheets into computed values.

use super::css::{Decl, Rule, Specificity, Stylesheet, parse_decls};
use super::dom::Element;
use super::{Rgb, parse_color};
use alloc::string::String;
use alloc::vec::Vec;

// ---- style: resolve computed values per element ----

/// A user-agent default stylesheet: the baseline look browsers ship (block
/// elements stack, headings are bold and larger, links are blue, lists indent).
pub(crate) const UA_CSS: &str = "
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
pub(crate) enum Disp {
    Block,
    Inline,
    ListItem,
    None,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Align {
    Left,
    Center,
}

/// Computed style for one element: only the properties our renderer uses.
#[derive(Clone)]
pub(crate) struct Computed {
    pub(crate) display: Disp,
    pub(crate) color: Rgb,
    pub(crate) bg: Option<Rgb>,
    pub(crate) scale: u8,
    pub(crate) bold: bool,
    pub(crate) margin: i32,
    pub(crate) padding: i32,
    pub(crate) align: Align,
}

impl Computed {
    /// The inherited root style (the `body` defaults before any element).
    pub(crate) fn root() -> Self {
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
    rules
        .into_iter()
        .flat_map(|(_, r)| r.decls.iter())
        .collect()
}

/// Resolve the computed style for `el` given its inherited parent style.
pub(crate) fn compute(el: &Element, sheet: &Stylesheet, parent: &Computed) -> Computed {
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
