//! Block layout with inline text flow, producing the display list.
//!
//! Text is measured, never counted: every width comes from the caller's
//! [`TextMetrics`], so words wrap at their real widths whatever the face. Line
//! boxes follow the CSS model closely enough for ordinary pages: a strut from the
//! block's own font, per-item baselines, `line-height`, `text-align`, collapsing
//! vertical margins, `width`/`max-width` with auto margins, borders, rounded
//! corners and backgrounds, lists with markers, tables, preformatted text.
//!
//! Besides text and boxes the layout places two kinds of *inline objects* in the
//! line boxes: pictures (`<img>`, see [`super::imgcache`]) and form controls
//! (`<input>`, `<button>`, see [`super::form`]). Both are sized here (declared
//! attributes first, then what is known about the picture), so a page lays out the
//! same before and after its images arrive unless a picture's own size differs from
//! what the page declared.
//!
//! Integer zoom (percent) scales every length and the font sizes; no floating
//! point anywhere. Everything a page controls is bounded: nodes, rules, depth,
//! words, characters, commands.

use super::Rgb;
use super::css::{Stylesheet, parse_css};
use super::dom::{
    Element, Node, decode_attr, fold_display, is_html_space, parse_html, text_content,
};
use super::form::{FieldInfo, FieldKind, FormInfo};
use super::imgcache::{ImageLookup, ImgState, NoImages};
use super::metrics::{FixedAdvance, Font, TextMetrics};
use super::style::{
    Align, Computed, Disp, Len, LineH, ListStyle, UA_CSS, Ws, compute, transform_text, zoom_px,
};
use alloc::string::String;
use alloc::vec::Vec;

mod flow;
mod inline;
mod objects;
mod text;
use flow::*;
use inline::*;
pub(super) use objects::*;
use text::*;

mod table;

// ---- the display list ----

/// Underline bit of [`Cmd::Text::deco`].
pub const DECO_UNDERLINE: u8 = 1;
/// Line-through bit of [`Cmd::Text::deco`].
pub const DECO_STRIKE: u8 = 2;

/// A rectangle, a run of text or a picture to paint.
#[derive(Debug, Clone)]
pub enum Cmd {
    /// A filled rectangle with rounded corners (`radius` 0 = square).
    Rect {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        color: Rgb,
        radius: i32,
    },
    /// A border drawn inside the box: widths top, right, bottom, left.
    Border {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        widths: [i32; 4],
        radius: i32,
        color: Rgb,
    },
    /// A run of text on one line. `y` is the top of the font's natural line box
    /// (draw with the face's ascent below it), `h` its height.
    Text {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        text: String,
        color: Rgb,
        font: Font,
        /// [`DECO_UNDERLINE`] | [`DECO_STRIKE`].
        deco: u8,
        /// Index into [`Page::links`] when the run is link text.
        link: Option<u32>,
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
    /// `alt`, entities decoded.
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
    /// Size in pixels of the control's text.
    pub size: u16,
    /// Inset of the text from the box's left edge.
    pub pad_x: i32,
}

/// A laid-out page: a flat display list plus the total content height (for
/// scrolling).
#[derive(Debug)]
pub struct Page {
    pub cmds: Vec<Cmd>,
    pub height: i32,
    /// The canvas colour (`html`/`body` background, white by default).
    pub background: Rgb,
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

impl Default for Page {
    fn default() -> Self {
        Page {
            cmds: Vec::new(),
            height: 0,
            background: Rgb(255, 255, 255),
            links: Vec::new(),
            hits: Vec::new(),
            images: Vec::new(),
            forms: Vec::new(),
            fields: Vec::new(),
        }
    }
}

/// Most links recorded per page (the rest still render, they are just not clickable).
pub const MAX_LINKS: usize = 2000;

/// Most `<img>` elements recorded per page (the rest are not laid out).
pub const MAX_IMAGES: usize = 200;

/// Tallest page, in pixels: every y coordinate saturates here, so a hostile page cannot
/// overflow the arithmetic of the layout, the scroll or the painter.
pub const MAX_PAGE_H: i32 = 1 << 24;

/// `y + d`, saturating at [`MAX_PAGE_H`].
fn adv(y: i32, d: i32) -> i32 {
    y.saturating_add(d).clamp(i32::MIN / 2, MAX_PAGE_H)
}

/// Most characters of text a page may put on screen (the rest is dropped).
pub const MAX_TEXT_CHARS: usize = 1_000_000;

/// Most display commands one layout may emit.
pub const MAX_CMDS: usize = 150_000;

/// Longest unbroken word kept, in characters (a longer one is cut).
pub const MAX_WORD_CHARS: usize = 4096;

/// Budget of ancestor steps the selector matching of one layout may take.
const STYLE_BUDGET: u32 = 4_000_000;

/// The box of one run of link text and the link it belongs to.
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
    /// Index into [`Page::links`] of the link under page coordinates `(x, y)`, if any.
    pub fn link_index_at(&self, x: i32, y: i32) -> Option<usize> {
        self.hits
            .iter()
            .rev()
            .find(|h| x >= h.x && x < h.x + h.w && y >= h.y && y < h.y + h.h)
            .map(|h| h.link)
    }

    /// The `href` of the link under page coordinates `(x, y)`, if any.
    pub fn link_at(&self, x: i32, y: i32) -> Option<&str> {
        self.link_index_at(x, y)
            .and_then(|l| self.links.get(l))
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
    /// Where every text width comes from.
    pub metrics: &'a dyn TextMetrics,
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
        let (mut dom, css) = parse_html(html);
        // Every document has a body: fragments get one so the UA margins and the page's own
        // `body { ... }` rules apply to them too.
        if !has_body(&dom, 0) {
            dom = alloc::vec![Node::Element(Element {
                tag: String::from("body"),
                attrs: Default::default(),
                children: dom,
                ws_before: false,
            })];
        }
        let mut sheet = parse_css(UA_CSS);
        for r in &mut sheet.rules {
            r.author = false;
        }
        sheet.rules.extend(parse_css(&css).rules);
        sheet.build_index();
        let title = find_title(&dom, 0)
            .map(|t| fold_display(&t))
            .unwrap_or_default();
        Doc { dom, sheet, title }
    }

    /// The document's `<title>` text (empty if none).
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Lay the document out.
    pub fn layout(&self, opts: &Layout) -> Page {
        let zoom = i32::from(opts.zoom.clamp(MIN_ZOOM, MAX_ZOOM));
        let root = Computed::root();
        let mut p = Painter {
            cmds: Vec::new(),
            links: Vec::new(),
            hits: Vec::new(),
            images: Vec::new(),
            forms: Vec::new(),
            fields: Vec::new(),
            cur_form: None,
            zoom,
            lookup: opts.images,
            m: opts.metrics,
            sheet: &self.sheet,
            anc: Vec::new(),
            budget: STYLE_BUDGET,
            chars: 0,
            items: Vec::new(),
            spare_items: Vec::new(),
            fm: core::cell::RefCell::new(Vec::new()),
            space: None,
            link: None,
            pending: 0,
            marker: None,
            lists: Vec::new(),
            canvas: None,
            last_box: (None, None),
        };
        let area = Area {
            x: 0,
            w: opts.width.max(40),
        };
        let mut y = flow(&mut p, &self.dom, &root, &root, area, 0);
        y = flush_inline(&mut p, &root, area, y, None);
        y = adv(y, core::mem::take(&mut p.pending));
        Page {
            cmds: p.cmds,
            height: y.clamp(0, MAX_PAGE_H),
            background: p.canvas.unwrap_or(Rgb(255, 255, 255)),
            links: p.links,
            hits: p.hits,
            images: p.images,
            forms: p.forms,
            fields: p.fields,
        }
    }
}

fn has_body(nodes: &[Node], depth: usize) -> bool {
    depth <= 3
        && nodes.iter().any(|n| match n {
            Node::Element(e) => e.tag == "body" || has_body(&e.children, depth + 1),
            Node::Text(_) => false,
        })
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

/// Render an HTML document to a display list laid out for `viewport_w` pixels
/// with the deterministic [`FixedAdvance`] metrics (no images known, zoom 100%).
/// For tests and fuzzing; the kernel calls [`Doc::layout`] with its own metrics.
pub fn render(html: &[u8], viewport_w: i32) -> Page {
    render_with(html, viewport_w, &FixedAdvance)
}

/// [`render`] with the caller's metrics.
pub fn render_with(html: &[u8], viewport_w: i32, metrics: &dyn TextMetrics) -> Page {
    Doc::parse(html).layout(&Layout {
        width: viewport_w,
        zoom: 100,
        images: &NoImages,
        metrics,
    })
}

// ---- the painter: state of one layout pass ----

/// Horizontal room of a block: its left edge and width.
#[derive(Clone, Copy, Debug)]
struct Area {
    x: i32,
    w: i32,
}

/// How a run of text looks.
#[derive(Clone, Copy, PartialEq)]
struct Style {
    font: Font,
    color: Rgb,
    deco: u8,
    bg: Option<Rgb>,
    link: Option<usize>,
    /// Line-height of the run in pixels.
    lh: i32,
    /// Pixels the run is raised above the baseline (negative: lowered).
    shift: i32,
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
        /// Width in characters (text controls).
        chars: usize,
        label: String,
    },
}

/// One piece of inline content waiting for its line.
enum Item {
    Word {
        text: String,
        st: Style,
        /// A collapsible space precedes the word; the font is that of the text holding it.
        sp: Option<Font>,
        /// May break between any two characters (CJK, preformatted lines).
        any: bool,
        /// Must stay on the line of the previous item (`white-space: nowrap`).
        nb: bool,
    },
    Obj {
        obj: Obj,
        st: Style,
        sp: Option<Font>,
    },
    Br(Style),
    /// Horizontal padding of an inline box (its background shows through it).
    Gap(i32, Style, Option<Font>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkerKind {
    Bullet(ListStyle),
    Number,
}

/// A list marker waiting for the first line of its item.
struct Marker {
    kind: MarkerKind,
    text: String,
    color: Rgb,
    /// Right edge of the marker (page x).
    right: i32,
}

struct ListCtx {
    ordered: bool,
    next: i32,
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
    m: &'a dyn TextMetrics,
    sheet: &'a Stylesheet,
    /// Ancestors of the element being styled, outermost first.
    anc: Vec<&'a Element>,
    budget: u32,
    /// Characters of text emitted so far.
    chars: usize,
    /// Inline content waiting for its line boxes.
    items: Vec<Item>,
    /// An empty item list with room kept, swapped in while a flush reads the real one.
    spare_items: Vec<Item>,
    /// What the metrics answered for the fonts seen so far: `(font, space q8, ascent, line)`.
    /// A page uses a handful of fonts and asks for these once per word.
    fm: core::cell::RefCell<Vec<(Font, i32, i32, i32)>>,
    /// A collapsible space precedes the next word (and the font of the text it came from).
    space: Option<Font>,
    /// The `<a href>` the content belongs to.
    link: Option<usize>,
    /// Vertical margin waiting at the current y (it collapses with the next one).
    pending: i32,
    marker: Option<Marker>,
    lists: Vec<ListCtx>,
    /// `html` / `body` background.
    canvas: Option<Rgb>,
    /// Indices of the background and border commands of the block laid out last (the table
    /// code stretches a cell's box to its row).
    last_box: (Option<usize>, Option<usize>),
}

impl Painter<'_> {
    /// Natural line height of `f`, sanitised: the metrics are the caller's, and layout must
    /// stay in range whatever they answer.
    fn nat(&self, f: Font) -> i32 {
        self.metrics_of(f).2
    }

    /// Ascent of `f`, sanitised like [`Painter::nat`].
    fn asc(&self, f: Font) -> i32 {
        self.metrics_of(f).1
    }

    /// Width of one space in `f`, q8.
    fn space_q8(&self, f: Font) -> i32 {
        self.metrics_of(f).0
    }

    /// `(space q8, ascent, line height)` of `f`, asked of the metrics once.
    fn metrics_of(&self, f: Font) -> (i32, i32, i32) {
        let mut c = self.fm.borrow_mut();
        if let Some(e) = c.iter().find(|e| e.0 == f) {
            return (e.1, e.2, e.3);
        }
        let v = (
            self.m.width_q8(" ", f).clamp(0, 1 << 26),
            self.m.ascent(f).clamp(0, 1 << 16),
            self.m.line_height(f).clamp(1, 1 << 16),
        );
        if c.len() < 32 {
            c.push((f, v.0, v.1, v.2));
        }
        v
    }

    /// Scale a length by the zoom (integer arithmetic).
    fn z(&self, v: i32) -> i32 {
        zoom_px(v, self.zoom)
    }

    fn font(&self, c: &Computed) -> Font {
        Font {
            size: self.z(c.font_px).clamp(4, 600) as u16,
            bold: c.bold,
            italic: c.italic,
            mono: c.mono,
        }
    }

    fn lh(&self, c: &Computed, font: Font) -> i32 {
        match c.line_h {
            LineH::Normal => self.nat(font),
            LineH::Mult(pct) => (i32::from(font.size) * pct + 50) / 100,
            LineH::Px(px) => self.z(px),
        }
        .clamp(1, 4000)
    }

    fn style(&self, c: &Computed) -> Style {
        let font = self.font(c);
        Style {
            font,
            color: c.color,
            deco: (if c.underline { DECO_UNDERLINE } else { 0 })
                | (if c.strike { DECO_STRIKE } else { 0 }),
            // Only an inline element paints a background behind its words; a block paints its own box.
            bg: c.bg.filter(|_| c.display == Disp::Inline),
            link: self.link,
            lh: self.lh(c, font),
            shift: i32::from(c.vshift) * (i32::from(font.size) * 2 / 5).max(1),
        }
    }

    fn push_cmd(&mut self, cmd: Cmd) {
        let low = match &cmd {
            Cmd::Rect { y, .. }
            | Cmd::Border { y, .. }
            | Cmd::Text { y, .. }
            | Cmd::Image { y, .. } => *y,
        };
        if self.cmds.len() < MAX_CMDS && low < MAX_PAGE_H {
            self.cmds.push(cmd);
        }
    }
}

// ---- text helpers ----

const Q: i32 = 256;

// ---- inline layout ----

/// One thing placed on a line.
struct Piece {
    kind: PieceKind,
    st: Style,
    /// Left edge from the line start, q8 pixels.
    x: i32,
    /// Width, q8 pixels.
    w: i32,
}

enum PieceKind {
    Text(String),
    /// An object: its box and how far it hangs below the baseline.
    Obj(Obj, i32, i32, i32),
    /// Inline padding.
    Gap,
}

struct Line {
    pieces: Vec<Piece>,
    /// Width so far, q8.
    w: i32,
    top: i32,
    bottom: i32,
}

// ---- objects: pictures and controls ----

/// Font size of the text inside controls (CSS px).
const CONTROL_PX: i32 = 14;

// ---- flow: blocks and inline content ----
