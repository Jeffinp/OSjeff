//! cascade (split out of `style.rs`).

use super::*;

/// Collect the declarations matching `el`, split into the user-agent sheet's and the
/// page's: normal ones lowest specificity first, then the `!important` ones, so later
/// declarations override earlier ones.
pub(super) fn matched_decls<'a>(
    el: &Element,
    anc: &[&Element],
    sheet: &'a Stylesheet,
    budget: &mut u32,
) -> (Vec<&'a Decl>, Vec<&'a Decl>) {
    let mut hit: Vec<(Specificity, &'a super::super::css::Rule)> = Vec::new();
    let mut test = |r: &'a super::super::css::Rule| {
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
    };
    match sheet.candidates(&el.tag) {
        Some(ix) => {
            for &i in ix {
                test(&sheet.rules[i as usize]);
            }
        }
        None => {
            for r in &sheet.rules {
                test(r);
            }
        }
    }
    hit.sort_by_key(|(sp, _)| *sp);
    let group = |author: bool| -> Vec<&'a Decl> {
        let mut out: Vec<&Decl> = Vec::new();
        for (_, r) in hit.iter().filter(|(_, r)| r.author == author) {
            out.extend(r.decls.iter().filter(|d| !d.important));
        }
        for (_, r) in hit.iter().filter(|(_, r)| r.author == author) {
            out.extend(r.decls.iter().filter(|d| d.important));
        }
        out
    };
    (group(false), group(true))
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
    c.spacing = 2;
    c.collapse = false;
    c.valign = VAlign::Middle;
    c.vshift = 0;

    let (ua, mut au) = matched_decls(el, anc, sheet, budget);
    let parent_px = parent.font_px;
    apply_decls(&mut c, &ua, parent_px);
    // Presentational attributes beat the user-agent sheet and lose to the page's.
    presentational(el, anc, &mut c);
    // Inline `style="..."` wins over everything from the stylesheets (except !important there).
    let inline: Vec<Decl> = el
        .attrs
        .get("style")
        .map(|s| parse_decls(s))
        .unwrap_or_default();
    au.extend(inline.iter().filter(|d| !d.important));
    au.extend(inline.iter().filter(|d| d.important));
    apply_decls(&mut c, &au, parent_px);
    c
}

/// Apply declarations in order to `c`. The font size goes first: `em` lengths below refer to it.
pub(super) fn apply_decls(c: &mut Computed, decls: &[&Decl], parent_px: i32) {
    // The font size first: `em` lengths below refer to it.
    for d in decls {
        let v = d.value.trim();
        match d.name.as_str() {
            "font-size" => {
                if let Some(px) = font_size_value(v, parent_px) {
                    c.font_px = px;
                }
            }
            "font" => apply_font_shorthand(c, v, parent_px),
            _ => {}
        }
    }
    let em = c.font_px;
    for d in decls {
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
            "border" => apply_border_side(c, &[0, 1, 2, 3], v),
            "border-top" => apply_border_side(c, &[0], v),
            "border-right" => apply_border_side(c, &[1], v),
            "border-bottom" => apply_border_side(c, &[2], v),
            "border-left" => apply_border_side(c, &[3], v),
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
            "border-collapse" => c.collapse = v.eq_ignore_ascii_case("collapse"),
            "border-spacing" => {
                if let Some(px) = tokens(v).first().and_then(|t| parse_px(t, em)) {
                    c.spacing = px.clamp(0, 64);
                }
            }
            "vertical-align" => {
                let l = v.to_ascii_lowercase();
                c.vshift = match l.as_str() {
                    "super" => 1,
                    "sub" => -1,
                    _ => 0,
                };
                c.valign = match l.as_str() {
                    "top" | "text-top" => VAlign::Top,
                    "bottom" | "text-bottom" => VAlign::Bottom,
                    _ => VAlign::Middle,
                };
            }
            _ => {}
        }
    }
}

/// The presentational attributes old pages still use (`align`, `bgcolor`, `<font>`,
/// `border` and `cellpadding` of tables, `type` of lists...). They apply before the
/// stylesheets, so any CSS overrides them.
pub(super) fn presentational(el: &Element, anc: &[&Element], c: &mut Computed) {
    let attr = |n: &str| el.attrs.get(n).map(|v| v.trim());
    let table = || anc.iter().rev().find(|a| a.tag == "table");
    let em = c.font_px;
    match el.tag.as_str() {
        "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "td" | "th" | "tr" | "caption"
        | "section" | "article" | "header" | "footer" | "center" => {
            match attr("align").map(str::to_ascii_lowercase).as_deref() {
                Some("center") => c.align = Align::Center,
                Some("right") => c.align = Align::Right,
                Some("left") => c.align = Align::Left,
                _ => {}
            }
        }
        "table" => {
            match attr("align").map(str::to_ascii_lowercase).as_deref() {
                Some("center") => {
                    c.margin[1] = Len::Auto;
                    c.margin[3] = Len::Auto;
                }
                Some("right") => c.margin[3] = Len::Auto,
                _ => {}
            }
            if let Some(b) = attr("border") {
                let n = if b.is_empty() {
                    1
                } else {
                    b.parse::<i32>().unwrap_or(1)
                };
                if n > 0 {
                    c.border_w = [n.clamp(1, 8); 4];
                    c.border_color = Rgb(0x80, 0x80, 0x80);
                }
            }
            if let Some(Len::Px(n)) = attr("cellspacing").and_then(|v| parse_len(v, em)) {
                c.spacing = n.clamp(0, 64);
            }
        }
        "ol" | "ul" | "li" => {
            c.list = match attr("type") {
                Some("a") => ListStyle::LowerAlpha,
                Some("A") => ListStyle::UpperAlpha,
                Some("i") => ListStyle::LowerRoman,
                Some("I") => ListStyle::UpperRoman,
                Some("1") => ListStyle::Decimal,
                Some(t) if t.eq_ignore_ascii_case("disc") => ListStyle::Disc,
                Some(t) if t.eq_ignore_ascii_case("circle") => ListStyle::Circle,
                Some(t) if t.eq_ignore_ascii_case("square") => ListStyle::Square,
                _ => c.list,
            };
        }
        "font" => {
            if let Some(col) = attr("color").and_then(parse_color) {
                c.color = col;
            }
            if let Some(n) =
                attr("size").and_then(|v| v.trim_start_matches('+').parse::<usize>().ok())
            {
                const SIZES: [i32; 8] = [10, 10, 13, 16, 18, 24, 32, 48];
                c.font_px = SIZES[n.min(7)];
            }
        }
        _ => {}
    }
    if matches!(el.tag.as_str(), "td" | "th") {
        if let Some(t) = table() {
            if t.attrs.get("border").is_some_and(|b| {
                let b = b.trim();
                b.is_empty() || b.parse::<i32>().unwrap_or(1) > 0
            }) {
                c.border_w = [1; 4];
                c.border_color = Rgb(0xA0, 0xA0, 0xA0);
            }
            if let Some(Len::Px(n)) = t.attrs.get("cellpadding").and_then(|v| parse_len(v, em)) {
                c.padding = [Len::Px(n.clamp(0, 64)); 4];
            }
        }
        match attr("valign").map(str::to_ascii_lowercase).as_deref() {
            Some("top") => c.valign = VAlign::Top,
            Some("bottom") => c.valign = VAlign::Bottom,
            Some("middle") => c.valign = VAlign::Middle,
            _ => {}
        }
    }
    if matches!(el.tag.as_str(), "table" | "td" | "th")
        && let Some(l) = attr("width").and_then(|v| parse_len(v, em))
    {
        c.width = non_neg_or_auto(l);
    }
    if let Some(col) = attr("bgcolor").and_then(parse_color) {
        c.bg = Some(col);
    }
    if el.tag == "body"
        && let Some(col) = attr("text").and_then(parse_color)
    {
        c.color = col;
    }
}

pub(super) fn non_neg_or_auto(l: Len) -> Len {
    match l {
        Len::Px(p) => Len::Px(p.max(0)),
        Len::Pct(p) => Len::Pct(p.max(0)),
        Len::Auto => Len::Auto,
    }
}

pub(super) fn side_index(name: &str) -> usize {
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
