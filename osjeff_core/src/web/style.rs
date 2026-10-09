//! Style cascade: combine the UA + page stylesheets into computed values.
//!
//! Lengths stay in CSS pixels here (`em` and `rem` are resolved against the
//! element's own font size); the page zoom is applied by the layout.

use super::css::{Decl, Specificity, Stylesheet, parse_decls};
use super::dom::Element;
use super::{Rgb, parse_color};
use alloc::string::String;
use alloc::vec::Vec;

// ---- the user-agent sheet ----

/// The baseline look pages get when they say nothing: system-like text, block
/// structure, readable headings, indigo links. Written in the same CSS the engine
/// reads from pages, so one parser covers both.
pub(crate) const UA_CSS: &str = "
html,body,div,p,h1,h2,h3,h4,h5,h6,ul,ol,dl,dt,dd,header,footer,article,section,nav,main,aside,blockquote,pre,form,figure,figcaption,address,details,summary,fieldset,legend,hgroup,center,hr,menu,dir,caption{display:block}
li{display:list-item}
table{display:table}
tr{display:table-row}
td,th{display:table-cell}
thead,tbody,tfoot{display:table-row-group}
script,style,head,title,meta,link,select,option,textarea,datalist,template,noembed,svg,canvas,video,audio,iframe,object,embed,map,area,param,source,track,base,dialog,col,colgroup{display:none}
html{color:#1d1d1f;font-size:16px;line-height:1.5}
body{margin:8px}
h1,h2,h3,h4,h5,h6{font-weight:bold;line-height:1.25}
h1{font-size:2em;margin:.67em 0}
h2{font-size:1.5em;margin:.83em 0}
h3{font-size:1.17em;margin:1em 0}
h4{font-size:1em;margin:1.33em 0}
h5{font-size:.83em;margin:1.67em 0}
h6{font-size:.67em;margin:2.33em 0}
p{margin:1em 0}
blockquote{margin:1em 0;padding:.25em 1em;border-left:3px solid #c7c7cc;color:#515154}
ul,ol,menu,dir{margin:1em 0;padding-left:28px}
ul{list-style-type:disc}
ol{list-style-type:decimal}
ul ul,ul ol,ol ul,ol ol{margin:0}
ul ul{list-style-type:circle}
ul ul ul{list-style-type:square}
dl{margin:1em 0}
dt{font-weight:bold}
dd{margin-left:32px}
a{color:#4f46e5}
b,strong{font-weight:bold}
i,em,cite,dfn,var,address{font-style:italic}
u,ins{text-decoration:underline}
s,strike,del{text-decoration:line-through}
small{font-size:.83em}
big{font-size:1.17em}
sub,sup{font-size:.75em}
mark{background:#fff3a3;color:#1d1d1f}
code,kbd,samp,tt{font-family:monospace;font-size:.92em}
code,kbd{background:#f1f1f5;border-radius:4px}
pre{font-family:monospace;font-size:.9em;white-space:pre;margin:1em 0;padding:12px 14px;background:#f5f5f7;border-radius:8px;line-height:1.4}
pre code{background:none;font-size:1em}
hr{margin:1.2em 0;border-top:1px solid #d1d1d6;height:0}
center{text-align:center}
caption{text-align:center;font-weight:bold;padding:4px 0}
th{font-weight:bold;text-align:center}
td,th{padding:4px 8px}
figure{margin:1em 40px}
figcaption{font-size:.9em;color:#6e6e73}
fieldset{margin:1em 0;padding:.5em 1em;border:1px solid #d1d1d6;border-radius:8px}
legend{font-weight:bold}
summary{font-weight:bold}
";

// ---- computed values ----

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Disp {
    Block,
    Inline,
    ListItem,
    None,
    Table,
    TableRowGroup,
    TableRow,
    TableCell,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Align {
    Left,
    Center,
    Right,
}

/// A length that may depend on the container.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Len {
    /// CSS pixels.
    Px(i32),
    /// Percent of the container's width.
    Pct(i32),
    Auto,
}

impl Len {
    /// Resolve against a container `base` pixels wide; `zoom` percent applies to `Px`.
    pub(crate) fn resolve(self, base: i32, zoom: i32) -> Option<i32> {
        match self {
            Len::Px(v) => Some(zoom_px(v, zoom)),
            Len::Pct(p) => {
                Some((i64::from(base) * i64::from(p) / 100).clamp(-100_000, 100_000) as i32)
            }
            Len::Auto => None,
        }
    }
}

/// Scale a CSS pixel length by the zoom percent.
pub(crate) fn zoom_px(v: i32, zoom: i32) -> i32 {
    (i64::from(v) * i64::from(zoom) / 100).clamp(-100_000, 100_000) as i32
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LineH {
    Normal,
    /// Percent of the font size (150 = 1.5).
    Mult(i32),
    Px(i32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ListStyle {
    None,
    Disc,
    Circle,
    Square,
    Decimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Ws {
    Normal,
    /// Whitespace and newlines kept, long lines still wrap (we cannot scroll sideways).
    Pre,
    NoWrap,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Transform {
    None,
    Upper,
    Lower,
    Capital,
}

/// Computed style for one element: only the properties our renderer uses.
#[derive(Clone, Debug)]
pub(crate) struct Computed {
    pub(crate) display: Disp,
    pub(crate) color: Rgb,
    pub(crate) bg: Option<Rgb>,
    /// Font size in CSS pixels.
    pub(crate) font_px: i32,
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) mono: bool,
    pub(crate) underline: bool,
    pub(crate) strike: bool,
    pub(crate) line_h: LineH,
    pub(crate) align: Align,
    pub(crate) ws: Ws,
    pub(crate) transform: Transform,
    pub(crate) list: ListStyle,
    /// top, right, bottom, left.
    pub(crate) margin: [Len; 4],
    pub(crate) padding: [Len; 4],
    pub(crate) border_w: [i32; 4],
    pub(crate) border_color: Rgb,
    pub(crate) radius: i32,
    pub(crate) width: Len,
    pub(crate) max_width: Len,
    /// `height`, used as a minimum height (CSS pixels, 0 = none).
    pub(crate) min_height: i32,
    /// `box-sizing: border-box`.
    pub(crate) border_box: bool,
}

impl Computed {
    /// The inherited root style (before any element).
    pub(crate) fn root() -> Self {
        Computed {
            display: Disp::Block,
            color: Rgb(0x1d, 0x1d, 0x1f),
            bg: None,
            font_px: 16,
            bold: false,
            italic: false,
            mono: false,
            underline: false,
            strike: false,
            line_h: LineH::Mult(150),
            align: Align::Left,
            ws: Ws::Normal,
            transform: Transform::None,
            list: ListStyle::Disc,
            margin: [Len::Px(0); 4],
            padding: [Len::Px(0); 4],
            border_w: [0; 4],
            border_color: Rgb(0x1d, 0x1d, 0x1f),
            radius: 0,
            width: Len::Auto,
            max_width: Len::Auto,
            min_height: 0,
            border_box: false,
        }
    }
}

fn default_display(tag: &str) -> Disp {
    match tag {
        "script" | "style" | "head" | "title" | "meta" | "link" | "select" | "option"
        | "textarea" | "datalist" => Disp::None,
        _ => Disp::Inline,
    }
}

/// Upper bound for any CSS length we honour (px). Far beyond any real layout
/// value, and small enough that summing it over a whole page stays in `i32`.
pub(crate) const MAX_PX: i32 = 4096;

/// Smallest and largest font size honoured (CSS px).
const MIN_FONT: i32 = 4;
const MAX_FONT: i32 = 300;

/// Parse a CSS number with up to three decimals into thousandths: `1.5` is 1500.
/// Returns the value and the rest of the string. Page-controlled, so every step
/// saturates.
fn parse_milli(s: &str) -> Option<(i64, &str)> {
    let b = s.as_bytes();
    let mut i = 0;
    let neg = match b.first() {
        Some(b'-') => {
            i = 1;
            true
        }
        Some(b'+') => {
            i = 1;
            false
        }
        _ => false,
    };
    let start = i;
    let mut int: i64 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        int = (int * 10 + i64::from(b[i] - b'0')).min(1_000_000_000);
        i += 1;
    }
    let mut frac: i64 = 0;
    let mut digits = i - start;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        let mut scale = 100;
        while i < b.len() && b[i].is_ascii_digit() {
            frac += i64::from(b[i] - b'0') * scale;
            scale /= 10;
            i += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return None;
    }
    let v = int * 1000 + frac;
    Some((if neg { -v } else { v }, &s[i..]))
}

/// Parse one CSS length. `em` is the font size `em` refers to. `None` for anything
/// not understood (`calc()`, `vw`, `var()`).
pub(crate) fn parse_len(v: &str, em: i32) -> Option<Len> {
    let v = v.trim();
    if v.eq_ignore_ascii_case("auto") {
        return Some(Len::Auto);
    }
    let (n, unit) = parse_milli(v)?;
    let unit = unit.trim().to_ascii_lowercase();
    // Rounded to the nearest pixel (0.67em of 16 px is 11, not 10).
    let px = |milli: i64| -> Option<Len> {
        let r = if milli >= 0 {
            (milli + 500) / 1000
        } else {
            -((-milli + 500) / 1000)
        };
        Some(Len::Px(
            r.clamp(-i64::from(MAX_PX), i64::from(MAX_PX)) as i32
        ))
    };
    match unit.as_str() {
        "" => {
            if n == 0 {
                px(0)
            } else {
                // Unitless non-zero lengths are invalid CSS but common in attributes and
                // quirks mode; read them as pixels.
                px(n)
            }
        }
        "px" => px(n),
        "pt" => px(n * 4 / 3),
        "pc" => px(n * 16),
        "in" => px(n * 96),
        "cm" => px(n * 96 * 100 / 254),
        "mm" => px(n * 96 * 10 / 254),
        "em" => px(n * i64::from(em)),
        "rem" => px(n * 16),
        "ex" => px(n * i64::from(em) / 2),
        "%" => Some(Len::Pct((n / 1000).clamp(-10_000, 10_000) as i32)),
        _ => None,
    }
}

/// A length that must be absolute (no percent, no auto).
fn parse_px(v: &str, em: i32) -> Option<i32> {
    match parse_len(v, em)? {
        Len::Px(p) => Some(p),
        _ => None,
    }
}

/// Split a value into whitespace separated tokens, keeping `(...)` groups whole.
fn tokens(v: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let b = v.as_bytes();
    let (mut start, mut depth) = (0usize, 0i32);
    let mut in_tok = false;
    for (i, &c) in b.iter().enumerate() {
        match c {
            b'(' => {
                depth += 1;
                in_tok = true;
            }
            b')' => depth = (depth - 1).max(0),
            c if c.is_ascii_whitespace() && depth == 0 => {
                if in_tok {
                    out.push(&v[start..i]);
                    in_tok = false;
                }
                continue;
            }
            _ => {}
        }
        if !in_tok {
            start = i;
            in_tok = true;
        }
    }
    if in_tok {
        out.push(&v[start..]);
    }
    out.truncate(16);
    out
}

/// One to four lengths in `top right bottom left` order.
fn four(v: &str, em: i32) -> Option<[Len; 4]> {
    let t = tokens(v);
    let get = |i: usize| parse_len(t[i], em);
    Some(match t.len() {
        1 => [get(0)?; 4],
        2 => {
            let (a, b) = (get(0)?, get(1)?);
            [a, b, a, b]
        }
        3 => {
            let (a, b, c) = (get(0)?, get(1)?, get(2)?);
            [a, b, c, b]
        }
        4 => [get(0)?, get(1)?, get(2)?, get(3)?],
        _ => return None,
    })
}

fn non_neg(l: Len) -> Len {
    match l {
        Len::Px(p) => Len::Px(p.max(0)),
        Len::Pct(p) => Len::Pct(p.max(0)),
        Len::Auto => Len::Px(0),
    }
}

/// The first colour in `v`, looking inside gradients (their first stop stands in
/// for the whole gradient).
fn first_color(v: &str) -> Option<Rgb> {
    for t in tokens(v) {
        let lower = t.to_ascii_lowercase();
        if lower.starts_with("url(") {
            continue;
        }
        if let Some(c) = parse_color(t) {
            return Some(c);
        }
        if lower.contains("gradient(")
            && let Some(open) = t.find('(')
        {
            let inner = t[open + 1..].trim_end_matches(')');
            // Arguments separated by commas outside parentheses.
            let mut depth = 0i32;
            let mut start = 0;
            let bytes = inner.as_bytes();
            for (i, &c) in bytes.iter().enumerate().chain([(bytes.len(), &b',')]) {
                match c {
                    b'(' => depth += 1,
                    b')' => depth -= 1,
                    b',' if depth == 0 => {
                        for part in tokens(&inner[start..i]) {
                            if let Some(c) = parse_color(part) {
                                return Some(c);
                            }
                        }
                        start = i + 1;
                    }
                    _ => {}
                }
            }
        }
    }
    None
}

/// Does a `font-family` list select the monospace face?
fn family_is_mono(v: &str) -> bool {
    let l = v.to_ascii_lowercase();
    l.split(',').any(|f| {
        let f = f.trim().trim_matches(|c| c == '"' || c == '\'');
        matches!(
            f,
            "monospace"
                | "ui-monospace"
                | "courier"
                | "courier new"
                | "consolas"
                | "menlo"
                | "monaco"
                | "lucida console"
                | "sf mono"
                | "jetbrains mono"
                | "fira code"
                | "source code pro"
                | "dejavu sans mono"
        )
    })
}

fn font_size_value(v: &str, parent_px: i32) -> Option<i32> {
    let l = v.trim().to_ascii_lowercase();
    let px = match l.as_str() {
        "xx-small" => 9,
        "x-small" => 10,
        "small" => 13,
        "medium" => 16,
        "large" => 18,
        "x-large" => 24,
        "xx-large" => 32,
        "xxx-large" => 48,
        "smaller" => parent_px * 5 / 6,
        "larger" => parent_px * 6 / 5,
        _ => match parse_len(&l, parent_px)? {
            Len::Px(p) => p,
            Len::Pct(p) => (i64::from(parent_px) * i64::from(p) / 100) as i32,
            Len::Auto => return None,
        },
    };
    Some(px.clamp(MIN_FONT, MAX_FONT))
}

fn line_height_value(v: &str, font_px: i32) -> Option<LineH> {
    let l = v.trim().to_ascii_lowercase();
    if l == "normal" {
        return Some(LineH::Normal);
    }
    // A bare number multiplies the element's own font size.
    if let Some((n, rest)) = parse_milli(&l)
        && rest.trim().is_empty()
    {
        return Some(LineH::Mult((n / 10).clamp(50, 1000) as i32));
    }
    match parse_len(&l, font_px)? {
        Len::Px(p) => Some(LineH::Px(p.clamp(1, 1000))),
        Len::Pct(p) => Some(LineH::Mult(p.clamp(50, 1000))),
        Len::Auto => None,
    }
}

fn weight_is_bold(v: &str) -> Option<bool> {
    let l = v.trim().to_ascii_lowercase();
    match l.as_str() {
        "bold" | "bolder" => Some(true),
        "normal" | "lighter" => Some(false),
        n => n.parse::<u32>().ok().map(|w| w >= 600),
    }
}

/// `font: [style] [weight] size[/line-height] family`
fn apply_font_shorthand(c: &mut Computed, v: &str, parent_px: i32) {
    let l = v.to_ascii_lowercase();
    if matches!(
        l.trim(),
        "inherit"
            | "initial"
            | "unset"
            | "caption"
            | "icon"
            | "menu"
            | "message-box"
            | "status-bar"
    ) {
        return;
    }
    c.bold = false;
    c.italic = false;
    for t in tokens(&l) {
        if t == "italic" || t == "oblique" {
            c.italic = true;
        } else if t == "normal" || t == "small-caps" {
            // the initial value: nothing to set
        } else if matches!(t, "bold" | "bolder") || t.parse::<u32>().is_ok_and(|w| w >= 600) {
            c.bold = true;
        } else if t == "lighter" || t.parse::<u32>().is_ok() {
            c.bold = false;
        } else {
            let (size, lh) = t.split_once('/').map_or((t, None), |(a, b)| (a, Some(b)));
            if let Some(px) = font_size_value(size, parent_px) {
                c.font_px = px;
                if let Some(lh) = lh
                    && let Some(h) = line_height_value(lh, px)
                {
                    c.line_h = h;
                }
                // Everything after the size is the family list.
                let end = (t.as_ptr() as usize - l.as_ptr() as usize) + t.len();
                c.mono = l.get(end..).is_some_and(family_is_mono);
                return;
            }
        }
    }
}

fn apply_border_side(c: &mut Computed, sides: &[usize], v: &str) {
    let mut w = 3; // `medium`
    let mut style_none = false;
    let mut have_style = false;
    for t in tokens(v) {
        let l = t.to_ascii_lowercase();
        match l.as_str() {
            "none" | "hidden" => {
                style_none = true;
                have_style = true;
            }
            "solid" | "dashed" | "dotted" | "double" | "groove" | "ridge" | "inset" | "outset" => {
                have_style = true;
            }
            "thin" => w = 1,
            "medium" => w = 3,
            "thick" => w = 5,
            _ => {
                if let Some(Len::Px(p)) = parse_len(&l, c.font_px) {
                    w = p.clamp(0, 64);
                } else if let Some(col) = parse_color(t) {
                    c.border_color = col;
                }
            }
        }
    }
    // Without a style the border does not exist (CSS), but `border: 1px #ccc` is common.
    let _ = have_style;
    let w = if style_none { 0 } else { w };
    for &s in sides {
        c.border_w[s] = w;
    }
}

/// Collect the declarations matching `el`: normal ones lowest specificity first,
/// then the `!important` ones, so later declarations override earlier ones.
fn matched_decls<'a>(
    el: &Element,
    anc: &[&Element],
    sheet: &'a Stylesheet,
    budget: &mut u32,
) -> Vec<&'a Decl> {
    let mut hit: Vec<(Specificity, &'a super::css::Rule)> = Vec::new();
    for r in &sheet.rules {
        let mut best: Option<Specificity> = None;
        for s in &r.selectors {
            // The cheap test first: most selectors fail on the element itself.
            if s.matches(el, anc, budget) {
                let sp = s.specificity();
                if best.is_none_or(|b| sp > b) {
                    best = Some(sp);
                }
            }
        }
        if let Some(sp) = best {
            hit.push((sp, r));
        }
    }
    hit.sort_by_key(|(sp, _)| *sp);
    let mut out: Vec<&Decl> = Vec::new();
    for (_, r) in &hit {
        out.extend(r.decls.iter().filter(|d| !d.important));
    }
    for (_, r) in &hit {
        out.extend(r.decls.iter().filter(|d| d.important));
    }
    out
}

/// Resolve the computed style for `el` given its inherited parent style and its
/// ancestors (outermost first).
pub(crate) fn compute(
    el: &Element,
    sheet: &Stylesheet,
    parent: &Computed,
    anc: &[&Element],
    budget: &mut u32,
) -> Computed {
    let mut c = parent.clone();
    // Not inherited: reset to the initial values.
    c.display = default_display(&el.tag);
    c.bg = None;
    c.margin = [Len::Px(0); 4];
    c.padding = [Len::Px(0); 4];
    c.border_w = [0; 4];
    c.radius = 0;
    c.width = Len::Auto;
    c.max_width = Len::Auto;
    c.min_height = 0;
    c.border_box = false;

    let mut decls = matched_decls(el, anc, sheet, budget);
    // Inline `style="..."` wins over everything from the stylesheets (except !important there).
    let inline: Vec<Decl> = el
        .attrs
        .get("style")
        .map(|s| parse_decls(s))
        .unwrap_or_default();
    decls.extend(inline.iter().filter(|d| !d.important));
    decls.extend(inline.iter().filter(|d| d.important));

    // The font size first: `em` lengths below refer to it.
    let parent_px = parent.font_px;
    for d in &decls {
        let v = d.value.trim();
        match d.name.as_str() {
            "font-size" => {
                if let Some(px) = font_size_value(v, parent_px) {
                    c.font_px = px;
                }
            }
            "font" => apply_font_shorthand(&mut c, v, parent_px),
            _ => {}
        }
    }
    let em = c.font_px;
    for d in &decls {
        let v = d.value.trim();
        match d.name.as_str() {
            "display" => {
                c.display = match v.to_ascii_lowercase().as_str() {
                    "block" | "flex" | "grid" | "flow-root" | "inline-flex" | "inline-grid" => {
                        Disp::Block
                    }
                    "inline" | "inline-block" | "contents" => Disp::Inline,
                    "list-item" => Disp::ListItem,
                    "none" => Disp::None,
                    "table" | "inline-table" => Disp::Table,
                    "table-row-group" | "table-header-group" | "table-footer-group" => {
                        Disp::TableRowGroup
                    }
                    "table-row" => Disp::TableRow,
                    "table-cell" => Disp::TableCell,
                    _ => c.display,
                }
            }
            "color" => {
                if let Some(rgb) = parse_color(v) {
                    c.color = rgb;
                }
            }
            "background" => {
                // `none` / `transparent` clear it; anything with a colour (or a gradient) sets it.
                let l = v.to_ascii_lowercase();
                if l == "none" || l == "transparent" {
                    c.bg = None;
                } else if let Some(col) = first_color(v) {
                    c.bg = Some(col);
                }
            }
            "background-color" => {
                let l = v.to_ascii_lowercase();
                if l == "transparent" || l == "none" {
                    c.bg = None;
                } else if let Some(col) = parse_color(v) {
                    c.bg = Some(col);
                }
            }
            "background-image" => {
                if c.bg.is_none() && v.to_ascii_lowercase().contains("gradient(") {
                    c.bg = first_color(v);
                }
            }
            "font-weight" => {
                if let Some(b) = weight_is_bold(v) {
                    c.bold = b;
                }
            }
            "font-style" => {
                let l = v.to_ascii_lowercase();
                c.italic = l.starts_with("italic") || l.starts_with("oblique");
            }
            "font-family" => c.mono = family_is_mono(v),
            "text-align" => {
                c.align = match v.to_ascii_lowercase().as_str() {
                    "center" | "-webkit-center" => Align::Center,
                    "right" | "end" => Align::Right,
                    _ => Align::Left,
                };
            }
            "line-height" => {
                if let Some(h) = line_height_value(v, em) {
                    c.line_h = h;
                }
            }
            "text-decoration" | "text-decoration-line" => {
                let l = v.to_ascii_lowercase();
                c.underline = l.contains("underline");
                c.strike = l.contains("line-through");
            }
            "text-transform" => {
                c.transform = match v.to_ascii_lowercase().as_str() {
                    "uppercase" => Transform::Upper,
                    "lowercase" => Transform::Lower,
                    "capitalize" => Transform::Capital,
                    _ => Transform::None,
                };
            }
            "white-space" => {
                c.ws = match v.to_ascii_lowercase().as_str() {
                    "pre" | "pre-wrap" | "pre-line" | "break-spaces" => Ws::Pre,
                    "nowrap" => Ws::NoWrap,
                    _ => Ws::Normal,
                };
            }
            "list-style" | "list-style-type" => {
                let l = v.to_ascii_lowercase();
                for t in tokens(&l) {
                    c.list = match t {
                        "none" => ListStyle::None,
                        "disc" => ListStyle::Disc,
                        "circle" => ListStyle::Circle,
                        "square" => ListStyle::Square,
                        "decimal" | "decimal-leading-zero" => ListStyle::Decimal,
                        "lower-alpha" | "lower-latin" => ListStyle::LowerAlpha,
                        "upper-alpha" | "upper-latin" => ListStyle::UpperAlpha,
                        "lower-roman" => ListStyle::LowerRoman,
                        "upper-roman" => ListStyle::UpperRoman,
                        _ => c.list,
                    };
                }
            }
            "margin" => {
                if let Some(m) = four(v, em) {
                    c.margin = m;
                }
            }
            "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
                if let Some(l) = parse_len(v, em) {
                    c.margin[side_index(&d.name)] = l;
                }
            }
            "padding" => {
                if let Some(p) = four(v, em) {
                    c.padding = p.map(non_neg);
                }
            }
            "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
                if let Some(l) = parse_len(v, em) {
                    c.padding[side_index(&d.name)] = non_neg(l);
                }
            }
            "border" => apply_border_side(&mut c, &[0, 1, 2, 3], v),
            "border-top" => apply_border_side(&mut c, &[0], v),
            "border-right" => apply_border_side(&mut c, &[1], v),
            "border-bottom" => apply_border_side(&mut c, &[2], v),
            "border-left" => apply_border_side(&mut c, &[3], v),
            "border-width" => {
                if let Some(w) = four(v, em) {
                    for (i, l) in w.iter().enumerate() {
                        if let Len::Px(p) = l {
                            c.border_w[i] = (*p).clamp(0, 64);
                        }
                    }
                }
            }
            "border-color" => {
                if let Some(col) = first_color(v) {
                    c.border_color = col;
                }
            }
            "border-style" => {
                if v.eq_ignore_ascii_case("none") || v.eq_ignore_ascii_case("hidden") {
                    c.border_w = [0; 4];
                }
            }
            "border-radius" => {
                let first = tokens(v).first().copied().unwrap_or("0");
                if let Some(Len::Px(p)) = parse_len(first, em) {
                    c.radius = p.clamp(0, 200);
                } else if let Some(Len::Pct(_)) = parse_len(first, em) {
                    c.radius = 200;
                }
            }
            "width" => {
                if let Some(l) = parse_len(v, em) {
                    c.width = non_neg_or_auto(l);
                }
            }
            "max-width" => {
                if let Some(l) = parse_len(v, em) {
                    c.max_width = non_neg_or_auto(l);
                } else if v.eq_ignore_ascii_case("none") {
                    c.max_width = Len::Auto;
                }
            }
            "height" | "min-height" => {
                if let Some(p) = parse_px(v, em) {
                    c.min_height = p.max(c.min_height.min(0)).max(0);
                }
            }
            "box-sizing" => c.border_box = v.eq_ignore_ascii_case("border-box"),
            _ => {}
        }
    }
    c
}

fn non_neg_or_auto(l: Len) -> Len {
    match l {
        Len::Px(p) => Len::Px(p.max(0)),
        Len::Pct(p) => Len::Pct(p.max(0)),
        Len::Auto => Len::Auto,
    }
}

fn side_index(name: &str) -> usize {
    if name.ends_with("top") {
        0
    } else if name.ends_with("right") {
        1
    } else if name.ends_with("bottom") {
        2
    } else {
        3
    }
}

/// `text` with `transform` applied (page text only; ASCII and Latin-1 letters via
/// the standard library's case mapping).
pub(crate) fn transform_text(text: &str, t: Transform) -> String {
    match t {
        Transform::None => String::from(text),
        Transform::Upper => text.to_uppercase(),
        Transform::Lower => text.to_lowercase(),
        Transform::Capital => {
            let mut out = String::with_capacity(text.len());
            let mut start = true;
            for ch in text.chars() {
                if start && ch.is_alphabetic() {
                    out.extend(ch.to_uppercase());
                } else {
                    out.push(ch);
                }
                start = ch.is_whitespace();
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::css::parse_css;
    use crate::web::dom::{Node, parse_html};

    fn style_of(html: &str, css: &str) -> Computed {
        let (nodes, page_css) = parse_html(html.as_bytes());
        let mut sheet = parse_css(UA_CSS);
        sheet.rules.extend(parse_css(css).rules);
        sheet.rules.extend(parse_css(&page_css).rules);
        let Node::Element(el) = &nodes[0] else {
            panic!("no element")
        };
        let mut b = 100_000;
        compute(el, &sheet, &Computed::root(), &[], &mut b)
    }

    #[test]
    fn lengths_in_every_unit() {
        assert_eq!(parse_len("12px", 16), Some(Len::Px(12)));
        assert_eq!(parse_len("1.5em", 20), Some(Len::Px(30)));
        assert_eq!(parse_len("2rem", 20), Some(Len::Px(32)));
        assert_eq!(parse_len("12pt", 16), Some(Len::Px(16)));
        assert_eq!(parse_len("50%", 16), Some(Len::Pct(50)));
        assert_eq!(parse_len("0", 16), Some(Len::Px(0)));
        assert_eq!(parse_len("auto", 16), Some(Len::Auto));
        assert_eq!(parse_len("-4px", 16), Some(Len::Px(-4)));
        assert_eq!(parse_len(".5em", 10), Some(Len::Px(5)));
        assert_eq!(parse_len("calc(1px + 2px)", 16), None);
        assert_eq!(parse_len("10vw", 16), None);
        assert_eq!(parse_len("", 16), None);
        // Hostile numbers saturate.
        assert_eq!(
            parse_len("99999999999999999999999px", 16),
            Some(Len::Px(MAX_PX))
        );
    }

    #[test]
    fn shorthand_margins_expand_to_four_sides() {
        let c = style_of("<p style='margin:1px 2px 3px 4px'>x</p>", "");
        assert_eq!(c.margin, [Len::Px(1), Len::Px(2), Len::Px(3), Len::Px(4)]);
        let c = style_of("<p style='margin:0 auto'>x</p>", "");
        assert_eq!(c.margin, [Len::Px(0), Len::Auto, Len::Px(0), Len::Auto]);
        let c = style_of("<p style='margin:5px 6px;margin-left:9px'>x</p>", "");
        assert_eq!(c.margin, [Len::Px(5), Len::Px(6), Len::Px(5), Len::Px(9)]);
    }

    #[test]
    fn ua_sizes_and_styles() {
        let h1 = style_of("<h1>x</h1>", "");
        assert_eq!(h1.font_px, 32);
        assert!(h1.bold);
        let em = style_of("<em>x</em>", "");
        assert!(em.italic && !em.bold);
        let code = style_of("<code>x</code>", "");
        assert!(code.mono);
        assert_eq!(code.font_px, 15); // .92 * 16 = 14.72, rounded
        let a = style_of("<a href=x>x</a>", "");
        assert_eq!(a.color, Rgb(0x4f, 0x46, 0xe5));
    }

    #[test]
    fn font_shorthand_and_keywords() {
        let c = style_of(
            "<p style='font:italic 600 20px/1.2 Georgia, serif'>x</p>",
            "",
        );
        assert!(c.italic && c.bold);
        assert_eq!(c.font_px, 20);
        assert_eq!(c.line_h, LineH::Mult(120));
        let c = style_of("<p style='font-size:large'>x</p>", "");
        assert_eq!(c.font_px, 18);
        let c = style_of("<p style='font-size:200%'>x</p>", "");
        assert_eq!(c.font_px, 32);
        let c = style_of("<p style='font: 13px monospace'>x</p>", "");
        assert!(c.mono && c.font_px == 13);
    }

    #[test]
    fn em_lengths_follow_the_elements_own_font_size() {
        let c = style_of("<p style='margin-top:2em;font-size:10px'>x</p>", "");
        assert_eq!(c.margin[0], Len::Px(20));
    }

    #[test]
    fn border_shorthand_and_sides() {
        let c = style_of("<p style='border:2px solid #ff0000'>x</p>", "");
        assert_eq!(c.border_w, [2; 4]);
        assert_eq!(c.border_color, Rgb(255, 0, 0));
        let c = style_of("<p style='border-bottom:1px solid #ccc'>x</p>", "");
        assert_eq!(c.border_w, [0, 0, 1, 0]);
        let c = style_of("<p style='border:none'>x</p>", "");
        assert_eq!(c.border_w, [0; 4]);
        let c = style_of("<p style='border-radius:8px 2px'>x</p>", "");
        assert_eq!(c.radius, 8);
    }

    #[test]
    fn background_shorthand_picks_the_colour_or_a_gradient_stop() {
        let c = style_of("<p style='background:#eee url(x.png) no-repeat'>x</p>", "");
        assert_eq!(c.bg, Some(Rgb(0xee, 0xee, 0xee)));
        let c = style_of(
            "<p style='background:linear-gradient(90deg, #ff0000, #0000ff)'>x</p>",
            "",
        );
        assert_eq!(c.bg, Some(Rgb(255, 0, 0)));
        let c = style_of("<p style='background:transparent'>x</p>", "");
        assert_eq!(c.bg, None);
    }

    #[test]
    fn important_and_specificity_order() {
        let c = style_of(
            "<p class=a id=b>x</p>",
            "#b{color:#00ff00} .a{color:#ff0000 !important} p{color:#0000ff}",
        );
        assert_eq!(c.color, Rgb(255, 0, 0));
        let c = style_of("<p id=b style='color:#112233'>x</p>", "#b{color:#00ff00}");
        assert_eq!(c.color, Rgb(0x11, 0x22, 0x33));
    }

    #[test]
    fn inheritance_and_reset() {
        let (nodes, _) = parse_html(b"<div style='color:#102030;font-weight:bold;background:#fff;margin:9px'><span>x</span></div>");
        let sheet = parse_css(UA_CSS);
        let Node::Element(div) = &nodes[0] else {
            panic!()
        };
        let mut b = 1000;
        let dc = compute(div, &sheet, &Computed::root(), &[], &mut b);
        let Node::Element(span) = &div.children[0] else {
            panic!()
        };
        let sc = compute(span, &sheet, &dc, &[div], &mut b);
        assert_eq!(sc.color, Rgb(0x10, 0x20, 0x30));
        assert!(sc.bold);
        assert_eq!(sc.bg, None);
        assert_eq!(sc.margin, [Len::Px(0); 4]);
    }

    #[test]
    fn text_transform_cases() {
        assert_eq!(transform_text("ação", Transform::Upper), "AÇÃO");
        assert_eq!(transform_text("a b", Transform::Capital), "A B");
        assert_eq!(transform_text("ABC", Transform::Lower), "abc");
    }
}
