//! Block layout with inline text flow, producing the display list.
//!
//! Besides text and boxes the layout places two kinds of *inline objects* in
//! the line boxes: pictures (`<img>`, see [`super::imgcache`]) and form
//! controls (`<input>`, `<button>`, see [`super::form`]). Both are sized here
//! (declared attributes first, then what is known about the picture), so a page
//! lays out the same before and after its images arrive unless a picture's own
//! size differs from what the page declared.
//!
//! Integer zoom (percent) scales every length and rounds the font scale; no
//! floating point anywhere.

use super::Rgb;
use super::css::{Stylesheet, parse_css};
use super::dom::{Element, Node, decode_attr, fold_display, parse_html, text_content};
use super::form::{FieldInfo, FieldKind, FormInfo};
use super::imgcache::{ImageLookup, ImgState, NoImages};
use super::style::{Align, Computed, Disp, UA_CSS, compute};
use alloc::string::String;
use alloc::vec::Vec;

// ---- layout + display list ----

/// A rectangle, a run of text or a picture to paint. The kernel maps
/// `Rgb`/`scale` onto its framebuffer and bitmap font.
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
    /// A decoded picture: paint `Page::images[idx]`'s pixels scaled into the box.
    Image {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        idx: usize,
    },
}

/// One `<img>` of the page: the `src` as written and the text to show when
/// there is no picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImgRef {
    pub src: String,
    /// `alt`, entities decoded and folded to the font's ASCII.
    pub alt: String,
}

/// The box of one form control, in page coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldBox {
    pub form: usize,
    pub field: usize,
    pub kind: FieldKind,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Font scale of the control's text.
    pub scale: u8,
    /// Inset of the text from the box's top-left corner.
    pub pad_x: i32,
    pub pad_y: i32,
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
    /// Every `<img>` (at most [`MAX_IMAGES`]); `Cmd::Image::idx` indexes this.
    pub images: Vec<ImgRef>,
    /// Every `<form>` with its controls (at most [`super::form::MAX_FORMS`]).
    pub forms: Vec<FormInfo>,
    /// Boxes of the visible controls, in document order.
    pub fields: Vec<FieldBox>,
}

/// Most links recorded per page (the rest still render, they are just not clickable).
pub const MAX_LINKS: usize = 2000;

/// Most `<img>` elements recorded per page (the rest are not laid out).
pub const MAX_IMAGES: usize = 200;

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

    /// The control under page coordinates `(x, y)`, if any.
    pub fn field_at(&self, x: i32, y: i32) -> Option<&FieldBox> {
        self.fields
            .iter()
            .rev()
            .find(|f| x >= f.x && x < f.x + f.w && y >= f.y && y < f.y + f.h)
    }
}

/// How to lay a document out.
pub struct Layout<'a> {
    /// Viewport width in pixels.
    pub width: i32,
    /// Page zoom in percent ([`MIN_ZOOM`]..=[`MAX_ZOOM`], 100 = normal).
    pub zoom: u16,
    /// What is known about each `<img>`.
    pub images: &'a dyn ImageLookup,
}

/// Smallest and largest page zoom (percent).
pub const MIN_ZOOM: u16 = 50;
pub const MAX_ZOOM: u16 = 300;
/// The zoom steps Ctrl+plus / Ctrl+minus walk through.
pub const ZOOM_STEPS: [u16; 8] = [50, 75, 100, 125, 150, 200, 250, 300];

/// The next zoom step above `z` (or `z` itself at the top).
pub fn zoom_in(z: u16) -> u16 {
    ZOOM_STEPS
        .iter()
        .copied()
        .find(|&s| s > z)
        .unwrap_or(MAX_ZOOM)
}

/// The next zoom step below `z` (or `z` itself at the bottom).
pub fn zoom_out(z: u16) -> u16 {
    ZOOM_STEPS
        .iter()
        .rev()
        .copied()
        .find(|&s| s < z)
        .unwrap_or(MIN_ZOOM)
}

/// A parsed document: the DOM and the stylesheet, so a page can be laid out
/// again (resize, zoom, an image arriving) without parsing the HTML again.
pub struct Doc {
    dom: Vec<Node>,
    sheet: Stylesheet,
    title: String,
}

impl Doc {
    /// Parse `html` (tolerant of any input, see [`parse_html`]).
    pub fn parse(html: &[u8]) -> Doc {
        let (dom, css) = parse_html(html);
        let mut sheet = parse_css(UA_CSS);
        sheet.rules.extend(parse_css(&css).rules);
        let title = find_title(&dom, 0)
            .map(|t| fold_display(&t))
            .unwrap_or_default();
        Doc { dom, sheet, title }
    }

    /// The document's `<title>` text, folded to the font's ASCII (empty if none).
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Lay the document out.
    pub fn layout(&self, opts: &Layout) -> Page {
        let zoom = i32::from(opts.zoom.clamp(MIN_ZOOM, MAX_ZOOM));
        let root = Computed::root();
        let mut painter = Painter {
            cmds: Vec::new(),
            links: Vec::new(),
            hits: Vec::new(),
            images: Vec::new(),
            forms: Vec::new(),
            fields: Vec::new(),
            cur_form: None,
            zoom,
            lookup: opts.images,
        };
        let pad = painter.z(12);
        let y = layout_children(
            &self.dom,
            &self.sheet,
            &root,
            pad,
            pad,
            (opts.width - 2 * pad).max(40),
            &mut painter,
        );
        Page {
            cmds: painter.cmds,
            height: y + pad,
            links: painter.links,
            hits: painter.hits,
            images: painter.images,
            forms: painter.forms,
            fields: painter.fields,
        }
    }
}

/// The text of the first `<title>` element (depth-limited: the DOM is
/// page-controlled).
fn find_title(nodes: &[Node], depth: usize) -> Option<String> {
    if depth > 8 {
        return None;
    }
    for n in nodes {
        if let Node::Element(e) = n {
            if e.tag == "title" {
                let t = text_content(&e.children);
                let t = t.trim();
                return (!t.is_empty()).then(|| t.into());
            }
            if let Some(t) = find_title(&e.children, depth + 1) {
                return Some(t);
            }
        }
    }
    None
}

/// Font metrics of the bitmap font: pixels per character cell and per text line
/// at a given integer scale.
fn char_w(scale: u8) -> i32 {
    6 * scale as i32
}
fn line_h(scale: u8) -> i32 {
    9 * scale as i32
}

/// An inline object: sized when its line is laid out.
enum Obj {
    Img {
        idx: usize,
        /// Declared `width` / `height` attributes (0 = absent), already zoomed.
        dw: i32,
        dh: i32,
        state: ImgState,
        alt: String,
    },
    Field {
        form: usize,
        field: usize,
        kind: FieldKind,
        /// Width in characters (text controls) or the label (buttons).
        chars: usize,
        label: String,
    },
}

/// One word of inline content, carrying the style it should render with.
struct Word {
    text: String,
    color: Rgb,
    scale: u8,
    bold: bool,
    /// Index into the page's link table when inside `<a href>`.
    link: Option<usize>,
    obj: Option<Obj>,
}

struct Painter<'a> {
    cmds: Vec<Cmd>,
    links: Vec<String>,
    hits: Vec<LinkHit>,
    images: Vec<ImgRef>,
    forms: Vec<FormInfo>,
    fields: Vec<FieldBox>,
    /// The `<form>` being laid out, if any.
    cur_form: Option<usize>,
    /// Zoom in percent.
    zoom: i32,
    lookup: &'a dyn ImageLookup,
}

impl Painter<'_> {
    /// Scale a length by the zoom (integer arithmetic).
    fn z(&self, v: i32) -> i32 {
        (i64::from(v) * i64::from(self.zoom) / 100).clamp(-100_000, 100_000) as i32
    }

    /// Scale a font scale by the zoom, rounding to nearest (at least 1).
    fn zs(&self, scale: u8) -> u8 {
        ((i32::from(scale) * self.zoom + 50) / 100).clamp(1, 18) as u8
    }
}

/// Render an HTML document to a display list laid out for `viewport_w` pixels
/// (no images known, zoom 100%).
pub fn render(html: &[u8], viewport_w: i32) -> Page {
    Doc::parse(html).layout(&Layout {
        width: viewport_w,
        zoom: 100,
        images: &NoImages,
    })
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
            Node::Text(t) => push_words(&mut inline, t, parent, None, p),
            Node::Element(el) => {
                let c = compute(el, sheet, parent);
                match c.display {
                    Disp::None => {}
                    Disp::Inline => collect_inline(el, sheet, &c, &mut inline, p, None),
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
    let margin = p.z(c.margin);
    let padding = p.z(c.padding);
    y += margin;
    let cx = x
        + padding
        + if c.display == Disp::ListItem {
            p.z(8)
        } else {
            0
        };
    let cw = (width - 2 * padding).max(20);
    let top = y;
    let bg_index = p.cmds.len();
    y += padding;

    // List marker.
    if c.display == Disp::ListItem {
        p.cmds.push(Cmd::Text {
            x: cx - p.z(12),
            y,
            text: "-".into(),
            color: c.color,
            scale: p.zs(c.scale),
            bold: false,
        });
    }

    // A <form> opens a scope for the controls inside it.
    let saved_form = p.cur_form;
    if el.tag == "form" && p.forms.len() < super::form::MAX_FORMS {
        p.forms.push(FormInfo::from_attrs(
            el.attrs.get("method").map(String::as_str),
            el.attrs.get("action").map(|a| decode_attr(a)),
        ));
        p.cur_form = Some(p.forms.len() - 1);
    }
    y = layout_children(&el.children, sheet, c, cx, y, cw, p);
    p.cur_form = saved_form;
    y += padding;

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
    y + margin
}

/// Parse a pixel attribute (`width="120"`, `"120px"`); percentages and
/// anything else are ignored (0).
fn attr_px(el: &Element, name: &str) -> i32 {
    let Some(v) = el.attrs.get(name) else {
        return 0;
    };
    let v = v.trim().trim_end_matches("px").trim();
    if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
        return 0;
    }
    v.parse::<i32>().unwrap_or(0).clamp(0, 4096)
}

/// Recursively gather a word stream from an inline element subtree. `<a href>`
/// registers its target in `links` and tags every word below it.
fn collect_inline(
    el: &Element,
    sheet: &Stylesheet,
    parent: &Computed,
    out: &mut Vec<Word>,
    p: &mut Painter,
    link: Option<usize>,
) {
    if el.tag == "br" {
        out.push(Word {
            text: "\n".into(),
            color: parent.color,
            scale: p.zs(parent.scale),
            bold: parent.bold,
            link: None,
            obj: None,
        });
        return;
    }
    if el.tag == "img" {
        if p.images.len() < MAX_IMAGES {
            let src = decode_attr(el.attrs.get("src").map(String::as_str).unwrap_or(""));
            let alt = fold_display(&decode_attr(
                el.attrs.get("alt").map(String::as_str).unwrap_or(""),
            ));
            let state = if src.trim().is_empty() {
                ImgState::Failed
            } else {
                p.lookup.lookup(&src)
            };
            let idx = p.images.len();
            p.images.push(ImgRef {
                src,
                alt: alt.clone(),
            });
            let (dw, dh) = (p.z(attr_px(el, "width")), p.z(attr_px(el, "height")));
            out.push(Word {
                text: String::new(),
                color: parent.color,
                scale: p.zs(parent.scale),
                bold: false,
                link,
                obj: Some(Obj::Img {
                    idx,
                    dw,
                    dh,
                    state,
                    alt,
                }),
            });
        }
        return;
    }
    if (el.tag == "input" || el.tag == "button") && collect_control(el, parent, out, p) {
        return;
    }
    let mut link = link;
    if el.tag == "a"
        && let Some(href) = el.attrs.get("href")
        && p.links.len() < MAX_LINKS
    {
        link = Some(p.links.len());
        p.links.push(decode_attr(href));
    }
    for node in &el.children {
        match node {
            Node::Text(t) => push_words(out, t, parent, link, p),
            Node::Element(child) => {
                let c = compute(child, sheet, parent);
                if c.display != Disp::None {
                    collect_inline(child, sheet, &c, out, p, link);
                }
            }
        }
    }
}

/// Register the form control `el` (an `<input>` or `<button>`) in the current
/// form and queue its box. Returns `true` when the element was consumed (a
/// control, or something to skip), `false` to let it render as ordinary
/// content (a `<button type=button>` shows its label as text).
fn collect_control(el: &Element, parent: &Computed, out: &mut Vec<Word>, p: &mut Painter) -> bool {
    let attr = |n: &str| el.attrs.get(n).map(String::as_str);
    let ty = attr("type").unwrap_or("").trim().to_ascii_lowercase();
    let name = decode_attr(attr("name").unwrap_or(""));
    let value = decode_attr(attr("value").unwrap_or(""));
    let (kind, label, chars) = if el.tag == "button" {
        match ty.as_str() {
            "" | "submit" => {
                let t = fold_display(text_content(&el.children).trim());
                (
                    FieldKind::Submit,
                    if t.is_empty() { "Enviar".into() } else { t },
                    0,
                )
            }
            _ => return false,
        }
    } else {
        match ty.as_str() {
            "hidden" => (FieldKind::Hidden, String::new(), 0),
            "" | "text" | "search" | "url" | "email" | "tel" | "number" => {
                let n = attr("size")
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .filter(|&n| n > 0)
                    .unwrap_or(20)
                    .min(80);
                (FieldKind::Text, String::new(), n)
            }
            "password" => (FieldKind::Password, String::new(), 20),
            "submit" => {
                let l = if value.is_empty() {
                    "Enviar".into()
                } else {
                    fold_display(&value)
                };
                (FieldKind::Submit, l, 0)
            }
            // checkbox, radio, file, image, reset, button, ...: not supported, not drawn.
            _ => return true,
        }
    };
    let Some(form) = p.cur_form else {
        return true; // a control outside any <form> has nowhere to go
    };
    let f = &mut p.forms[form];
    if f.fields.len() >= super::form::MAX_FIELDS {
        return true;
    }
    let field = f.fields.len();
    f.fields.push(FieldInfo {
        name,
        kind,
        value,
        size: chars,
        label: label.clone(),
    });
    if kind != FieldKind::Hidden {
        out.push(Word {
            text: String::new(),
            color: parent.color,
            scale: p.zs(2),
            bold: false,
            link: None,
            obj: Some(Obj::Field {
                form,
                field,
                kind,
                chars,
                label,
            }),
        });
    }
    true
}

fn push_words(out: &mut Vec<Word>, text: &str, c: &Computed, link: Option<usize>, p: &Painter) {
    for w in text.split(' ') {
        if w.is_empty() {
            continue;
        }
        out.push(Word {
            text: w.into(),
            color: c.color,
            scale: p.zs(c.scale),
            bold: c.bold,
            link,
            obj: None,
        });
    }
}

/// Width and height of an image box, given what the page declared, what is
/// known of the picture and the room on the line. Integer math throughout,
/// never zero, never wider than `avail`.
pub(crate) fn img_box(dw: i32, dh: i32, state: ImgState, avail: i32, zoom: i32) -> (i32, i32) {
    let nat = match state {
        ImgState::Ready { w, h } if w > 0 && h > 0 => Some((
            (i64::from(w) * i64::from(zoom) / 100).max(1),
            (i64::from(h) * i64::from(zoom) / 100).max(1),
        )),
        _ => None,
    };
    let (dw, dh) = (i64::from(dw), i64::from(dh));
    let (mut w, mut h) = match (dw > 0, dh > 0, nat) {
        (true, true, _) => (dw, dh),
        (true, false, Some((nw, nh))) => (dw, (dw * nh / nw).max(1)),
        (false, true, Some((nw, nh))) => ((dh * nw / nh).max(1), dh),
        (true, false, None) => (dw, (dw * 3 / 4).max(1)),
        (false, true, None) => ((dh * 4 / 3).max(1), dh),
        (false, false, Some((nw, nh))) => (nw, nh),
        (false, false, None) => match state {
            // A picture that will not come: a compact message box.
            ImgState::Pending => (160 * i64::from(zoom) / 100, 120 * i64::from(zoom) / 100),
            _ => (240 * i64::from(zoom) / 100, 34 * i64::from(zoom) / 100),
        },
    };
    let avail = i64::from(avail.max(8));
    if w > avail {
        h = (h * avail / w).max(1);
        w = avail;
    }
    (w.clamp(1, 20_000) as i32, h.clamp(1, 20_000) as i32)
}

/// Draw the message box of an image that has no picture: border, alt text
/// and the reason.
fn paint_missing(p: &mut Painter, x: i32, y: i32, w: i32, h: i32, alt: &str, state: ImgState) {
    p.cmds.push(Cmd::Rect {
        x,
        y,
        w,
        h,
        color: Rgb(0xB8, 0xC0, 0xCE),
    });
    p.cmds.push(Cmd::Rect {
        x: x + 1,
        y: y + 1,
        w: (w - 2).max(0),
        h: (h - 2).max(0),
        color: Rgb(0xEC, 0xEF, 0xF5),
    });
    let sc = p.zs(1).max(1);
    let cw = char_w(sc);
    let cols = ((w - 8) / cw).max(0) as usize;
    let lh = line_h(sc);
    let mut ty = y + 4.max(p.z(4));
    let alt = alt.trim();
    if !alt.is_empty() && cols > 0 && ty + lh <= y + h {
        let t: String = alt.chars().take(cols).collect();
        p.cmds.push(Cmd::Text {
            x: x + 4,
            y: ty,
            text: t,
            color: Rgb(0x33, 0x40, 0x55),
            scale: sc,
            bold: false,
        });
        ty += lh + 2;
    }
    if let Some(m) = state.message()
        && cols > 0
        && ty + lh <= y + h
    {
        let t: String = m.chars().take(cols).collect();
        p.cmds.push(Cmd::Text {
            x: x + 4,
            y: ty,
            text: t,
            color: Rgb(0x8A, 0x1C, 0x1C),
            scale: sc,
            bold: false,
        });
    }
}

/// Place a laid-out object at `(x, y)` (top-left of its box) and record hit
/// boxes. `link` makes a picture clickable.
#[allow(clippy::too_many_arguments)]
fn emit_obj(
    p: &mut Painter,
    obj: &Obj,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    scale: u8,
    link: Option<usize>,
) {
    match obj {
        Obj::Img {
            idx, state, alt, ..
        } => {
            match state {
                ImgState::Ready { .. } => p.cmds.push(Cmd::Image {
                    x,
                    y,
                    w,
                    h,
                    idx: *idx,
                }),
                ImgState::Pending => p.cmds.push(Cmd::Rect {
                    x,
                    y,
                    w,
                    h,
                    color: Rgb(0xDD, 0xE1, 0xE8),
                }),
                other => paint_missing(p, x, y, w, h, alt, *other),
            }
            if let Some(l) = link {
                p.hits.push(LinkHit {
                    x,
                    y,
                    w,
                    h,
                    link: l,
                });
            }
        }
        Obj::Field {
            form,
            field,
            kind,
            label,
            ..
        } => {
            let pad_x = p.z(6) + 1;
            let pad_y = p.z(5) + 1;
            match kind {
                FieldKind::Submit => {
                    p.cmds.push(Cmd::Rect {
                        x,
                        y,
                        w,
                        h,
                        color: Rgb(0x15, 0x65, 0xC0),
                    });
                    let lw = char_w(scale) * label.chars().count() as i32;
                    p.cmds.push(Cmd::Text {
                        x: x + ((w - lw) / 2).max(0),
                        y: y + pad_y,
                        text: label.clone(),
                        color: Rgb(255, 255, 255),
                        scale,
                        bold: false,
                    });
                }
                _ => {
                    p.cmds.push(Cmd::Rect {
                        x,
                        y,
                        w,
                        h,
                        color: Rgb(0x9A, 0xA6, 0xBC),
                    });
                    p.cmds.push(Cmd::Rect {
                        x: x + 1,
                        y: y + 1,
                        w: (w - 2).max(0),
                        h: (h - 2).max(0),
                        color: Rgb(255, 255, 255),
                    });
                }
            }
            p.fields.push(FieldBox {
                form: *form,
                field: *field,
                kind: *kind,
                x,
                y,
                w,
                h,
                scale,
                pad_x,
                pad_y,
            });
        }
    }
}

/// The (width, height) of an object on a line with `avail` pixels.
fn obj_size(p: &Painter, obj: &Obj, scale: u8, avail: i32) -> (i32, i32) {
    match obj {
        Obj::Img { dw, dh, state, .. } => img_box(*dw, *dh, *state, avail, p.zoom),
        Obj::Field {
            kind, chars, label, ..
        } => {
            let cw = char_w(scale);
            let h = line_h(scale) + 2 * (p.z(5) + 1);
            let w = match kind {
                FieldKind::Submit => cw * label.chars().count() as i32 + 2 * p.z(12),
                _ => cw * *chars as i32 + 2 * (p.z(6) + 1),
            };
            (w.min(avail.max(20)).max(8), h)
        }
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
    // Size every object once, now that the line width is known.
    let sizes: Vec<(i32, i32)> = words
        .iter()
        .map(|w| match &w.obj {
            Some(o) => obj_size(p, o, w.scale, width),
            None => (
                char_w(w.scale) * w.text.chars().count() as i32,
                line_h(w.scale),
            ),
        })
        .collect();
    // Group consecutive words into lines that fit `width`.
    let mut line: Vec<usize> = Vec::new();
    let mut line_w = 0;
    let mut max_scale = 1u8;
    let mut line_height = 0;

    let flush_line = |line: &mut Vec<usize>,
                      line_w: &mut i32,
                      max_scale: &mut u8,
                      line_height: &mut i32,
                      y: &mut i32,
                      p: &mut Painter| {
        if line.is_empty() {
            return;
        }
        let lh = (*line_height).max(line_h(*max_scale));
        let mut lx = x;
        if align == Align::Center && *line_w < width {
            lx += (width - *line_w) / 2;
        }
        for &i in line.iter() {
            let w = &words[i];
            let (ww, wh) = sizes[i];
            if let Some(obj) = &w.obj {
                // Objects sit on the baseline (the bottom of the line box).
                emit_obj(p, obj, lx, *y + lh - wh, ww, wh, w.scale, w.link);
            } else {
                let ty = *y + lh - line_h(w.scale);
                if let Some(link) = w.link {
                    p.hits.push(LinkHit {
                        x: lx,
                        y: ty,
                        w: ww,
                        h: line_h(w.scale),
                        link,
                    });
                }
                p.cmds.push(Cmd::Text {
                    x: lx,
                    y: ty,
                    text: w.text.clone(),
                    color: w.color,
                    scale: w.scale,
                    bold: w.bold,
                });
            }
            lx += ww + char_w(w.scale);
        }
        *y += lh;
        line.clear();
        *line_w = 0;
        *max_scale = 1;
        *line_height = 0;
    };

    for (i, w) in words.iter().enumerate() {
        if w.obj.is_none() && w.text == "\n" {
            flush_line(
                &mut line,
                &mut line_w,
                &mut max_scale,
                &mut line_height,
                &mut y,
                p,
            );
            continue;
        }
        let ww = sizes[i].0;
        let space = if line.is_empty() { 0 } else { char_w(w.scale) };
        if line_w + space + ww > width && !line.is_empty() {
            flush_line(
                &mut line,
                &mut line_w,
                &mut max_scale,
                &mut line_height,
                &mut y,
                p,
            );
        }
        line_w += if line.is_empty() { ww } else { space + ww };
        max_scale = max_scale.max(w.scale);
        line_height = line_height.max(sizes[i].1);
        line.push(i);
    }
    flush_line(
        &mut line,
        &mut line_w,
        &mut max_scale,
        &mut line_height,
        &mut y,
        p,
    );
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
                Cmd::Image { x, y, w, h, .. } => assert!(*x >= 0 && *y >= 0 && *w >= 0 && *h >= 0),
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
