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

/// Characters that may break between each other (no spaces in the script).
fn breaks_anywhere(c: char) -> bool {
    matches!(c as u32,
        0x0E00..=0x0EFF
        | 0x2E80..=0x9FFF
        | 0xA960..=0xA97F
        | 0xAC00..=0xD7FF
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFFEF
        | 0x1F000..=0x1FAFF
        | 0x20000..=0x3FFFF)
}

/// Width in q8 pixels (1/256) of `text`.
fn wq(p: &Painter, text: &str, f: Font) -> i32 {
    // The metrics report whole pixels; the kernel's engine is more exact through
    // `width_q8`, which defaults to `width * 256`.
    p.m.width_q8(text, f).clamp(0, 1 << 26)
}

const Q: i32 = 256;

/// Byte length of the longest prefix of `text` (at least one character) that fits
/// in `max_q8`.
fn fit_prefix(p: &Painter, text: &str, f: Font, max_q8: i32) -> usize {
    let bounds: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .skip(1)
        .chain([text.len()])
        .collect();
    if bounds.is_empty() {
        return text.len();
    }
    // Binary search for the largest count of characters whose width fits.
    let (mut lo, mut hi) = (0usize, bounds.len()); // lo fits (0 chars), hi may not
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if wq(p, &text[..bounds[mid - 1]], f) <= max_q8 {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    bounds[lo.max(1) - 1]
}

fn push_word(p: &mut Painter, text: String, st: Style, any: bool, nb: bool) {
    if text.is_empty() {
        return;
    }
    let sp = p.space.take();
    p.items.push(Item::Word {
        text,
        st,
        sp,
        any,
        nb,
    });
}

/// Turn the text of a DOM node into words (collapsing whitespace) or, in
/// preformatted text, lines.
#[allow(unused_assignments)]
fn push_text(p: &mut Painter, text: &str, c: &Computed) {
    if p.chars >= MAX_TEXT_CHARS {
        return;
    }
    let st = p.style(c);
    let transformed;
    let text = if c.transform == super::style::Transform::None {
        text
    } else {
        transformed = transform_text(text, c.transform);
        transformed.as_str()
    };
    if c.ws == Ws::Pre {
        let mut lines = text.split('\n').peekable();
        while let Some(line) = lines.next() {
            if !line.is_empty() {
                let expanded = expand_tabs(line);
                let n = expanded.chars().count().min(MAX_WORD_CHARS * 4);
                p.chars += n;
                let cut: String = expanded.chars().take(n).collect();
                // The leading text belongs to the line, spaces included.
                p.space = None;
                p.items.push(Item::Word {
                    text: cut,
                    st,
                    sp: None,
                    any: true,
                    nb: false,
                });
            }
            if lines.peek().is_some() {
                p.items.push(Item::Br(st));
            }
        }
        return;
    }
    let nowrap = c.ws == Ws::NoWrap;
    // The word being read is `text[start..]` up to the current position.
    let mut start: Option<usize> = None;
    let mut word_any = false;
    let mut emitted = 0usize;
    let mut count = 0usize;
    macro_rules! finish {
        ($end:expr) => {
            if let Some(from) = start.take() {
                p.chars += count;
                count = 0;
                emitted += 1;
                push_word(
                    p,
                    String::from(&text[from..$end]),
                    st,
                    word_any,
                    nowrap && emitted > 1,
                );
            }
        };
    }
    let mut stop = text.len();
    for (i, ch) in text.char_indices() {
        if ch != '\u{a0}' && (ch.is_whitespace() || (ch.is_ascii() && is_html_space(ch as u8))) {
            finish!(i);
            p.space = Some(st.font);
            continue;
        }
        if ch == '\u{200b}' {
            finish!(i);
            continue;
        }
        let any = breaks_anywhere(ch);
        if start.is_some() && (any != word_any || count >= MAX_WORD_CHARS) {
            finish!(i);
        }
        if p.chars + count >= MAX_TEXT_CHARS {
            stop = i;
            break;
        }
        word_any = any;
        if start.is_none() {
            start = Some(i);
        }
        count += 1;
    }
    finish!(stop);
}

fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return String::from(line);
    }
    let mut out = String::with_capacity(line.len() + 8);
    let mut col = 0usize;
    for ch in line.chars() {
        if ch == '\t' {
            let n = 4 - col % 4;
            for _ in 0..n {
                out.push(' ');
            }
            col += n;
        } else {
            out.push(ch);
            col += 1;
        }
    }
    out
}

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

impl Line {
    fn new(strut: (i32, i32)) -> Line {
        Line {
            pieces: Vec::new(),
            w: 0,
            top: strut.0,
            bottom: strut.1,
        }
    }
}

/// `(above, below)` the baseline of the block's own strut.
fn strut_of(p: &Painter, c: &Computed) -> (i32, i32) {
    let f = p.font(c);
    let lh = p.lh(c, f);
    let asc = p.asc(f);
    let nat = p.nat(f);
    let top = asc + (lh - nat) / 2;
    (top, lh - top)
}

/// Extents of a text run: (above baseline, below baseline).
fn text_extents(p: &Painter, st: &Style) -> (i32, i32) {
    let asc = p.asc(st.font);
    let nat = p.nat(st.font);
    let top = asc + (st.lh - nat) / 2;
    ((top + st.shift).max(0), (st.lh - top - st.shift).max(0))
}

/// Emit the buffered inline words as wrapped, aligned line boxes starting at `y`;
/// returns the y below the last line. `align_override` replaces the container's
/// `text-align` (block-level images centred by auto margins).
fn flush_inline(
    p: &mut Painter,
    container: &Computed,
    area: Area,
    mut y: i32,
    align_override: Option<Align>,
) -> i32 {
    if p.items.is_empty() {
        return y;
    }
    // The margin waiting above the first line.
    y = adv(y, core::mem::take(&mut p.pending));
    p.space = None;
    let spare = core::mem::take(&mut p.spare_items);
    let mut items = core::mem::replace(&mut p.items, spare);
    let align = align_override.unwrap_or(container.align);
    let strut = strut_of(p, container);
    let avail = area.w.max(1).saturating_mul(Q);
    let mut line = Line::new(strut);

    for item in items.drain(..) {
        match item {
            Item::Br(st) => {
                if line.pieces.is_empty() {
                    // An empty line still has the height of its strut and of the break.
                    let (t, b) = text_extents(p, &st);
                    y = adv(y, (t.max(strut.0) + b.max(strut.1)).max(1));
                } else {
                    y = emit_line(p, &mut line, area, align, y, strut);
                }
            }
            Item::Obj { obj, st, sp } => {
                let (w, h) = obj_size(p, &obj, area.w);
                let below = obj_below(p, &obj, h);
                let space = match sp {
                    Some(f) if !line.pieces.is_empty() => p.space_q8(f),
                    _ => 0,
                };
                let wq_ = w.saturating_mul(Q);
                if !line.pieces.is_empty()
                    && line.w.saturating_add(space).saturating_add(wq_) > avail
                {
                    y = emit_line(p, &mut line, area, align, y, strut);
                }
                let space = if line.pieces.is_empty() { 0 } else { space };
                let x = line.w.saturating_add(space);
                line.w = x.saturating_add(wq_);
                line.top = line.top.max(h - below);
                line.bottom = line.bottom.max(below);
                line.pieces.push(Piece {
                    kind: PieceKind::Obj(obj, w, h, below),
                    st,
                    x,
                    w: wq_,
                });
            }
            Item::Gap(w, st, sp) => {
                let wq_ = w.max(0).saturating_mul(Q);
                let space = match sp {
                    Some(f) if !line.pieces.is_empty() => p.space_q8(f),
                    _ => 0,
                };
                let x = line.w.saturating_add(space);
                line.w = x.saturating_add(wq_);
                line.pieces.push(Piece {
                    kind: PieceKind::Gap,
                    st,
                    x,
                    w: wq_,
                });
            }
            Item::Word {
                mut text,
                st,
                sp,
                any,
                nb,
            } => {
                let mut sp = sp;
                loop {
                    let ww = wq(p, &text, st.font);
                    let space = match sp {
                        Some(f) if !line.pieces.is_empty() => p.space_q8(f),
                        _ => 0,
                    };
                    if line.w.saturating_add(space).saturating_add(ww) <= avail
                        || (nb && !line.pieces.is_empty())
                    {
                        place_text(p, &mut line, text, st, space, ww);
                        break;
                    }
                    if !line.pieces.is_empty() {
                        // Fill the rest of this line with the head of a word that may break anywhere.
                        if any {
                            let room = avail.saturating_sub(line.w).saturating_sub(space);
                            if room > 0 {
                                let k = fit_prefix(p, &text, st.font, room);
                                if k < text.len() && wq(p, &text[..k], st.font) <= room {
                                    let tail = text.split_off(k);
                                    let hw = wq(p, &text, st.font);
                                    place_text(p, &mut line, text, st, space, hw);
                                    y = emit_line(p, &mut line, area, align, y, strut);
                                    text = tail;
                                    sp = None;
                                    continue;
                                }
                            }
                        }
                        y = emit_line(p, &mut line, area, align, y, strut);
                        sp = None;
                        continue;
                    }
                    // Alone on an empty line and still too wide: cut it.
                    let k = fit_prefix(p, &text, st.font, avail);
                    if k >= text.len() {
                        place_text(p, &mut line, text, st, 0, ww);
                        break;
                    }
                    let tail = text.split_off(k);
                    let hw = wq(p, &text, st.font);
                    place_text(p, &mut line, text, st, 0, hw);
                    y = emit_line(p, &mut line, area, align, y, strut);
                    text = tail;
                    sp = None;
                }
            }
        }
    }
    if !line.pieces.is_empty() {
        y = emit_line(p, &mut line, area, align, y, strut);
    }
    // Keep the list's room for the next flush (nothing was queued while this one ran).
    if p.items.is_empty() {
        p.spare_items = core::mem::replace(&mut p.items, items);
    } else {
        p.spare_items = items;
    }
    y
}

fn place_text(p: &Painter, line: &mut Line, text: String, st: Style, space: i32, w: i32) {
    let x = line.w.saturating_add(space);
    line.w = x.saturating_add(w);
    let (t, b) = text_extents(p, &st);
    line.top = line.top.max(t);
    line.bottom = line.bottom.max(b);
    line.pieces.push(Piece {
        kind: PieceKind::Text(text),
        st,
        x,
        w,
    });
}

/// Place the pieces of `line` and start a new one; returns the y below it.
fn emit_line(
    p: &mut Painter,
    line: &mut Line,
    area: Area,
    align: Align,
    y: i32,
    strut: (i32, i32),
) -> i32 {
    let mut done = core::mem::replace(line, Line::new(strut));
    // The pieces are read out of this vector, which then goes back to the next line empty
    // but with the room it grew to.
    let mut pieces = core::mem::take(&mut done.pieces);
    let line_h = (done.top + done.bottom).max(1);
    let baseline = y.saturating_add(done.top);
    let slack = (area.w.saturating_mul(Q) - done.w).max(0);
    let shift = match align {
        Align::Left => 0,
        Align::Center => slack / 2,
        Align::Right => slack,
    };
    let x0 = area.x.saturating_mul(Q) + shift;

    // A marker belongs to the first line of its item.
    if let Some(mk) = p.marker.take() {
        let f = pieces
            .iter()
            .find_map(|pc| matches!(pc.kind, PieceKind::Text(_)).then_some(pc.st.font))
            .unwrap_or(p.style(&Computed::root()).font);
        emit_marker(p, &mk, baseline, f);
    }

    // Merge neighbouring text pieces that look the same into one run.
    let mut runs: Vec<(i32, i32, String, Style)> = Vec::with_capacity(pieces.len().min(8)); // x q8, w q8, text, style
    let mut objs: Vec<(i32, Obj, i32, i32, i32, Style)> = Vec::new();
    // Backgrounds of inline boxes: (left, right, top, height, colour), joined when they touch.
    let mut bgs: Vec<(i32, i32, i32, i32, Rgb)> = Vec::new();
    let add_bg =
        |bgs: &mut Vec<(i32, i32, i32, i32, Rgb)>, l: i32, r: i32, t: i32, h: i32, c: Rgb| {
            if let Some(last) = bgs.last_mut()
                && last.4 == c
                && last.2 == t
                && last.3 == h
                && l <= last.1 + 1
            {
                last.1 = last.1.max(r);
                return;
            }
            bgs.push((l, r, t, h, c));
        };
    for pc in pieces.drain(..) {
        match pc.kind {
            PieceKind::Text(t) => {
                if let Some(last) = runs.last_mut()
                    && last.3 == pc.st
                    && last.0 + last.1 <= pc.x
                    && pc.x - (last.0 + last.1) <= p.space_q8(pc.st.font) + Q
                {
                    // The gap between the pieces is the collapsed space.
                    if pc.x > last.0 + last.1 {
                        last.2.push(' ');
                    }
                    last.2.push_str(&t);
                    last.1 = pc.x + pc.w - last.0;
                } else {
                    runs.push((pc.x, pc.w, t, pc.st));
                }
            }
            PieceKind::Obj(o, w, h, below) => objs.push((pc.x, o, w, h, below, pc.st)),
            PieceKind::Gap => {
                if let Some(bg) = pc.st.bg {
                    let nat = p.nat(pc.st.font);
                    let ty = baseline - p.asc(pc.st.font) - pc.st.shift;
                    let l = (x0.saturating_add(pc.x).saturating_add(Q / 2)) / Q;
                    let r = (x0
                        .saturating_add(pc.x)
                        .saturating_add(pc.w)
                        .saturating_add(Q / 2))
                        / Q;
                    bgs.push((l, r, ty - 1, nat + 2, bg));
                }
            }
        }
    }
    // Background boxes of the runs (and of the gaps between them), in order.
    let mut run_bgs: Vec<(i32, i32, i32, i32, Rgb)> = Vec::new();
    for (x, w, _, st) in &runs {
        if let Some(bg) = st.bg {
            let nat = p.nat(st.font);
            let ty = baseline - p.asc(st.font) - st.shift;
            let l = (x0.saturating_add(*x).saturating_add(Q / 2)) / Q;
            let r = (x0
                .saturating_add(*x)
                .saturating_add(*w)
                .saturating_add(Q - 1))
                / Q;
            run_bgs.push((l, r, ty - 1, nat + 2, bg));
        }
    }
    bgs.extend(run_bgs);
    bgs.sort_by_key(|b| (b.2, b.0));
    let mut joined: Vec<(i32, i32, i32, i32, Rgb)> = Vec::new();
    for b in bgs {
        add_bg(&mut joined, b.0, b.1, b.2, b.3, b.4);
    }
    for (l, r, t, h, col) in joined {
        p.push_cmd(Cmd::Rect {
            x: l,
            y: t,
            w: (r - l).max(1),
            h,
            color: col,
            radius: 4,
        });
    }
    for (x, text, w, st) in runs.into_iter().map(|(x, w, t, s)| (x, t, w, s)) {
        let px = (x0.saturating_add(x).saturating_add(Q / 2)) / Q;
        let pw = (w + Q - 1) / Q;
        let nat = p.nat(st.font);
        let ty = baseline - p.asc(st.font) - st.shift;
        if let Some(l) = st.link {
            p.hits.push(LinkHit {
                x: px,
                y,
                w: pw,
                h: line_h,
                link: l,
            });
        }
        p.push_cmd(Cmd::Text {
            x: px,
            y: ty,
            w: pw,
            h: nat,
            text,
            color: st.color,
            font: st.font,
            deco: st.deco,
            link: st.link.map(|l| l as u32),
        });
    }
    for (x, obj, w, h, below, st) in objs {
        let px = (x0.saturating_add(x).saturating_add(Q / 2)) / Q;
        emit_obj(p, &obj, px, baseline - (h - below), w, h, st);
    }
    line.pieces = pieces;
    adv(y, line_h)
}

fn emit_marker(p: &mut Painter, mk: &Marker, baseline: i32, f: Font) {
    match mk.kind {
        MarkerKind::Number => {
            let w = (wq(p, &mk.text, f) + Q - 1) / Q;
            let nat = p.nat(f);
            p.push_cmd(Cmd::Text {
                x: (mk.right - w).max(0),
                y: baseline - p.asc(f),
                w,
                h: nat,
                text: mk.text.clone(),
                color: mk.color,
                font: f,
                deco: 0,
                link: None,
            });
        }
        MarkerKind::Bullet(style) => {
            let d = (i32::from(f.size) * 3 / 10).max(3);
            let right = mk.right - p.z(2);
            // A list with no left padding would put its marker off the page.
            let x = (right - d).max(0);
            let y = (baseline - i32::from(f.size) * 3 / 8 - d / 2).max(0);
            match style {
                ListStyle::Circle => p.push_cmd(Cmd::Border {
                    x,
                    y,
                    w: d,
                    h: d,
                    widths: [1; 4],
                    radius: d / 2,
                    color: mk.color,
                }),
                ListStyle::Square => p.push_cmd(Cmd::Rect {
                    x,
                    y,
                    w: d,
                    h: d,
                    color: mk.color,
                    radius: 0,
                }),
                _ => p.push_cmd(Cmd::Rect {
                    x,
                    y,
                    w: d,
                    h: d,
                    color: mk.color,
                    radius: d / 2,
                }),
            }
        }
    }
}

// ---- objects: pictures and controls ----

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
            _ => (240 * i64::from(zoom) / 100, 48 * i64::from(zoom) / 100),
        },
    };
    let avail = i64::from(avail.max(8));
    if w > avail {
        h = (h * avail / w).max(1);
        w = avail;
    }
    (w.clamp(1, 20_000) as i32, h.clamp(1, 20_000) as i32)
}

/// Font size of the text inside controls (CSS px).
const CONTROL_PX: i32 = 14;

fn control_font(p: &Painter) -> Font {
    Font::new(p.z(CONTROL_PX).clamp(6, 200) as u16)
}

/// The (width, height) of an object on a line with `avail` pixels.
fn obj_size(p: &Painter, obj: &Obj, avail: i32) -> (i32, i32) {
    match obj {
        Obj::Img { dw, dh, state, .. } => img_box(*dw, *dh, *state, avail, p.zoom),
        Obj::Field {
            kind, chars, label, ..
        } => {
            let f = control_font(p);
            let h = p.z(30);
            let w = match kind {
                FieldKind::Submit | FieldKind::PushButton => {
                    let tw = (wq(p, label, f) + Q - 1) / Q;
                    (tw + 2 * p.z(16)).max(p.z(64))
                }
                FieldKind::Checkbox | FieldKind::Radio => {
                    // The box and a gap before the label that follows.
                    let s = p.z(16).max(8);
                    return (s + p.z(6), s);
                }
                _ => {
                    let cw = (wq(p, "0", f) + Q - 1) / Q;
                    cw * (*chars as i32).clamp(1, 80) + 2 * p.z(10)
                }
            };
            (w.min(avail.max(20)).max(8), h)
        }
    }
}

/// How far an object hangs below the baseline of its line: nothing for a picture; a control
/// sits with the baseline of its own text on the line's, a toggle roughly centred on the
/// x-height.
fn obj_below(p: &Painter, obj: &Obj, h: i32) -> i32 {
    match obj {
        Obj::Img { .. } => 0,
        Obj::Field { kind, .. } => match kind {
            FieldKind::Checkbox | FieldKind::Radio => h / 4,
            _ => {
                let f = control_font(p);
                let text_top = (h - p.nat(f)) / 2;
                (h - (text_top + p.asc(f))).clamp(0, h)
            }
        },
    }
}

/// Fit `text` in `max_w` pixels, cutting with an ellipsis.
fn ellipsize(p: &Painter, text: &str, f: Font, max_w: i32) -> String {
    let max_q = max_w.max(0).saturating_mul(Q);
    if wq(p, text, f) <= max_q {
        return String::from(text);
    }
    let room = max_q - wq(p, "\u{2026}", f);
    if room <= 0 {
        return String::new();
    }
    let k = fit_prefix(p, text, f, room);
    let mut s = String::from(&text[..k]);
    if wq(p, &s, f) > room {
        s.clear();
    }
    s.push('\u{2026}');
    s
}

/// Draw the message box of an image that has no picture: border, alt text
/// and the reason.
fn paint_missing(p: &mut Painter, x: i32, y: i32, w: i32, h: i32, alt: &str, state: ImgState) {
    p.push_cmd(Cmd::Rect {
        x,
        y,
        w,
        h,
        color: Rgb(0xEE, 0xEF, 0xF3),
        radius: p.z(6),
    });
    p.push_cmd(Cmd::Border {
        x,
        y,
        w,
        h,
        widths: [1; 4],
        radius: p.z(6),
        color: Rgb(0xC9, 0xCC, 0xD6),
    });
    let f = Font::new(p.z(12).clamp(6, 100) as u16);
    let lh = p.nat(f);
    let inner = w - 2 * p.z(8);
    let mut ty = y + p.z(8);
    let alt = alt.trim();
    if !alt.is_empty() && inner > 0 && ty + lh <= y + h {
        let t = ellipsize(p, alt, f, inner);
        let tw = (wq(p, &t, f) + Q - 1) / Q;
        p.push_cmd(Cmd::Text {
            x: x + p.z(8),
            y: ty,
            w: tw,
            h: lh,
            text: t,
            color: Rgb(0x33, 0x40, 0x55),
            font: f,
            deco: 0,
            link: None,
        });
        ty += lh + p.z(2);
    }
    if let Some(m) = state.message()
        && inner > 0
        && ty + lh <= y + h
    {
        let t = ellipsize(p, m, f, inner);
        let tw = (wq(p, &t, f) + Q - 1) / Q;
        p.push_cmd(Cmd::Text {
            x: x + p.z(8),
            y: ty,
            w: tw,
            h: lh,
            text: t,
            color: Rgb(0x9A, 0x2B, 0x2B),
            font: f,
            deco: 0,
            link: None,
        });
    }
}

/// Place a laid-out object at `(x, y)` (top-left of its box) and record hit boxes.
fn emit_obj(p: &mut Painter, obj: &Obj, x: i32, y: i32, w: i32, h: i32, st: Style) {
    match obj {
        Obj::Img {
            idx, state, alt, ..
        } => {
            match state {
                ImgState::Ready { .. } => p.push_cmd(Cmd::Image {
                    x,
                    y,
                    w,
                    h,
                    idx: *idx,
                }),
                ImgState::Pending => p.push_cmd(Cmd::Rect {
                    x,
                    y,
                    w,
                    h,
                    color: Rgb(0xDD, 0xE1, 0xE8),
                    radius: p.z(6),
                }),
                other => paint_missing(p, x, y, w, h, alt, *other),
            }
            if let Some(l) = st.link {
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
            form, field, kind, ..
        } => {
            let f = control_font(p);
            let w = if matches!(kind, FieldKind::Checkbox | FieldKind::Radio) {
                (w - p.z(6)).max(1)
            } else {
                w
            };
            p.fields.push(FieldBox {
                form: *form,
                field: *field,
                kind: *kind,
                x,
                y,
                w,
                h,
                size: f.size,
                pad_x: p.z(10),
            });
        }
    }
}

// ---- flow: blocks and inline content ----

/// Lay out a list of sibling nodes in the formatting context of `container`.
/// Inline content is buffered in the painter; a block child first flushes it.
/// `parent` is the style the text of these nodes inherits (an inline element's
/// own style when recursing through inline elements), `container` the nearest
/// block (its strut and alignment shape the lines). Returns the y after the
/// last line or block emitted (buffered inline content is still waiting).
fn flow<'a>(
    p: &mut Painter<'a>,
    nodes: &'a [Node],
    parent: &Computed,
    container: &Computed,
    area: Area,
    mut y: i32,
) -> i32 {
    for node in nodes {
        match node {
            Node::Text(t) => push_text(p, t, parent),
            Node::Element(el) => {
                if el.ws_before {
                    p.space = Some(p.font(parent));
                }
                let c = compute(el, p.sheet, parent, &p.anc, &mut p.budget);
                match c.display {
                    Disp::None => {}
                    Disp::Inline => {
                        y = inline_element(p, el, &c, area, y);
                    }
                    _ if el.tag == "img" => {
                        // A block-level picture sits on a line of its own; auto side margins
                        // centre it.
                        y = flush_inline(p, container, area, y, None);
                        p.pending = p.pending.max(side(c.margin[0], area.w, p.zoom));
                        image_element(p, el, &c);
                        let centred = c.margin[1] == Len::Auto && c.margin[3] == Len::Auto;
                        y = flush_inline(p, container, area, y, centred.then_some(Align::Center));
                        p.pending = p.pending.max(side(c.margin[2], area.w, p.zoom));
                    }
                    _ => {
                        y = flush_inline(p, container, area, y, None);
                        y = layout_block(p, el, &c, area, y);
                    }
                }
            }
        }
    }
    y
}

/// An inline element: special ones (`br`, `img`, controls), otherwise its
/// children join the inline run.
fn inline_element<'a>(
    p: &mut Painter<'a>,
    el: &'a Element,
    c: &Computed,
    area: Area,
    y: i32,
) -> i32 {
    match el.tag.as_str() {
        "br" => {
            let st = p.style(c);
            p.items.push(Item::Br(st));
            p.space = None;
            return y;
        }
        "wbr" => return y,
        "img" => {
            image_element(p, el, c);
            return y;
        }
        "input" | "button" => {
            if collect_control(p, el, c) {
                return y;
            }
        }
        _ => {}
    }
    let saved_link = p.link;
    if el.tag == "a"
        && let Some(href) = el.attrs.get("href")
        && p.links.len() < MAX_LINKS
    {
        p.link = Some(p.links.len());
        p.links.push(decode_attr(href));
    }
    // Horizontal padding of an inline box: a gap its background shows through.
    let (pl, pr) = (side(c.padding[3], 0, p.zoom), side(c.padding[1], 0, p.zoom));
    let st = p.style(c);
    if pl > 0 {
        // The space before the box goes before its padding, not inside it.
        let sp = p.space.take();
        p.items.push(Item::Gap(pl, st, sp));
    }
    p.anc.push(el);
    let y = flow(p, &el.children, c, c, area, y);
    p.anc.pop();
    if pr > 0 {
        p.items.push(Item::Gap(pr, st, None));
    }
    p.link = saved_link;
    y
}

fn image_element(p: &mut Painter, el: &Element, c: &Computed) {
    if p.images.len() >= MAX_IMAGES {
        return;
    }
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
    // A CSS pixel width overrides the attribute.
    let dw = match c.width {
        Len::Px(w) => p.z(w),
        _ => dw,
    };
    let st = p.style(c);
    let sp = p.space.take();
    p.items.push(Item::Obj {
        obj: Obj::Img {
            idx,
            dw,
            dh,
            state,
            alt,
        },
        st,
        sp,
    });
}

/// Register the form control `el` (an `<input>` or `<button>`) in the current
/// form and queue its box. Returns `true` when the element was consumed (a
/// control, or something to skip), `false` to let it render as ordinary content.
fn collect_control(p: &mut Painter, el: &Element, c: &Computed) -> bool {
    let attr = |n: &str| el.attrs.get(n).map(String::as_str);
    let ty = attr("type").unwrap_or("").trim().to_ascii_lowercase();
    let name = decode_attr(attr("name").unwrap_or(""));
    let value = decode_attr(attr("value").unwrap_or(""));
    let placeholder = fold_display(&decode_attr(attr("placeholder").unwrap_or("")));
    let checked = el.attrs.contains_key("checked");
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
            "button" | "reset" => {
                let t = fold_display(text_content(&el.children).trim());
                (FieldKind::PushButton, t, 0)
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
            "button" | "reset" => (FieldKind::PushButton, fold_display(&value), 0),
            "checkbox" => (FieldKind::Checkbox, String::new(), 0),
            "radio" => (FieldKind::Radio, String::new(), 0),
            // file, image, range, color, date, ...: not supported, not drawn.
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
    let value = if matches!(kind, FieldKind::Checkbox | FieldKind::Radio) && value.is_empty() {
        String::from("on")
    } else {
        value
    };
    f.fields.push(FieldInfo {
        name,
        kind,
        value,
        size: chars,
        label: label.clone(),
        checked,
        placeholder,
    });
    if kind != FieldKind::Hidden {
        let st = p.style(c);
        p.items.push(Item::Obj {
            obj: Obj::Field {
                form,
                field,
                kind,
                chars,
                label,
            },
            st,
            sp: p.space.take(),
        });
    }
    true
}

/// Resolve a margin or padding side against the container width.
fn side(l: Len, base: i32, zoom: i32) -> i32 {
    l.resolve(base, zoom).unwrap_or(0)
}

/// Lay out a single block-level element: margins, border, padding, background and
/// content. Returns the y below its border box (its bottom margin waits in
/// `p.pending` to collapse with what follows).
fn layout_block<'a>(p: &mut Painter<'a>, el: &'a Element, c: &Computed, area: Area, y: i32) -> i32 {
    layout_block_in(p, el, c, area, y, None)
}

/// [`layout_block`], optionally in a box of exactly `fixed = (x, border-box width)`
/// (a table cell: no margins, the width is the column's).
fn layout_block_in<'a>(
    p: &mut Painter<'a>,
    el: &'a Element,
    c: &Computed,
    area: Area,
    mut y: i32,
    fixed: Option<(i32, i32)>,
) -> i32 {
    let zoom = p.zoom;
    let aw = area.w;
    let [mt_l, mr_l, mb_l, ml_l] = c.margin;
    let (mt, mb) = if fixed.is_some() {
        (0, 0)
    } else {
        (side(mt_l, aw, zoom), side(mb_l, aw, zoom))
    };
    let pad_t = side(c.padding[0], aw, zoom);
    let pad_r = side(c.padding[1], aw, zoom);
    let pad_b = side(c.padding[2], aw, zoom);
    let pad_l = side(c.padding[3], aw, zoom);
    let [bt, br, bb, bl] = c.border_w.map(|w| if w > 0 { p.z(w).max(1) } else { 0 });
    let extra = pad_l + pad_r + bl + br;

    // A table is as wide as its columns want to be (or its `width`); read it first.
    let mut table = if c.display == Disp::Table && fixed.is_none() {
        Some(table::collect(p, el, c))
    } else {
        None
    };

    // ---- width and horizontal position ----
    let ml_fixed = if ml_l == Len::Auto {
        0
    } else {
        side(ml_l, aw, zoom)
    };
    let mr_fixed = if mr_l == Len::Auto {
        0
    } else {
        side(mr_l, aw, zoom)
    };
    let room = (aw - ml_fixed - mr_fixed).max(1);
    let mut total = room;
    if let Some(t) = &table
        && c.width == Len::Auto
    {
        total = t.totals().1.saturating_add(extra).min(room);
    }
    if let Some(w) = c.width.resolve(aw, zoom) {
        total = if c.border_box { w } else { w + extra };
    }
    if let Some(mw) = c.max_width.resolve(aw, zoom) {
        let cap = if c.border_box { mw } else { mw + extra };
        total = total.min(cap);
    }
    total = total.clamp(extra.max(1), room.max(extra).max(1));
    let leftover = (aw - ml_fixed - mr_fixed - total).max(0);
    let ml = match (ml_l == Len::Auto, mr_l == Len::Auto) {
        (true, true) => leftover / 2,
        (true, false) => leftover,
        _ => ml_fixed,
    };
    let (bx, bw) = fixed.unwrap_or((area.x + ml, total));
    let cx = bx + bl + pad_l;
    let cw = (bw - extra).max(1);

    // ---- top ----
    p.pending = p.pending.max(mt);
    let is_canvas = el.tag == "html" || el.tag == "body";
    if is_canvas && let Some(bg) = c.bg {
        // The page background belongs to the canvas, not to this box.
        if el.tag == "html" || p.canvas.is_none() {
            p.canvas = Some(bg);
        }
    }
    let paint_bg = c.bg.filter(|_| !is_canvas);
    let boxed_top = paint_bg.is_some() || bt > 0 || pad_t > 0;
    let boxed_bottom = paint_bg.is_some() || bb > 0 || pad_b > 0;
    if boxed_top {
        y = adv(y, core::mem::take(&mut p.pending));
    }
    let top = y;
    let bg_index = p.cmds.len();
    y = adv(y, bt + pad_t);

    // ---- list marker ----
    let mut own_marker = false;
    if c.display == Disp::ListItem {
        let n = match p.lists.last_mut() {
            Some(l) => {
                if let Some(v) = el
                    .attrs
                    .get("value")
                    .and_then(|v| v.trim().parse::<i32>().ok())
                {
                    l.next = v;
                }
                let n = l.next;
                l.next = l.next.saturating_add(1);
                Some((l.ordered, n))
            }
            None => None,
        };
        let (ordered, n) = n.unwrap_or((false, 0));
        let kind = if ordered
            || matches!(
                c.list,
                ListStyle::Decimal
                    | ListStyle::LowerAlpha
                    | ListStyle::UpperAlpha
                    | ListStyle::LowerRoman
                    | ListStyle::UpperRoman
            ) {
            MarkerKind::Number
        } else {
            MarkerKind::Bullet(c.list)
        };
        if c.list != ListStyle::None {
            p.marker = Some(Marker {
                kind,
                text: if kind == MarkerKind::Number {
                    let mut s = counter_text(n, c.list);
                    s.push('.');
                    s
                } else {
                    String::new()
                },
                color: c.color,
                right: cx - p.z(8),
            });
            own_marker = true;
        }
    }

    // ---- scopes opened by this element ----
    let saved_form = p.cur_form;
    if el.tag == "form" && p.forms.len() < super::form::MAX_FORMS {
        p.forms.push(FormInfo::from_attrs(
            el.attrs.get("method").map(String::as_str),
            el.attrs.get("action").map(|a| decode_attr(a)),
        ));
        p.cur_form = Some(p.forms.len() - 1);
    }
    let is_list = matches!(el.tag.as_str(), "ul" | "ol" | "menu" | "dir");
    if is_list {
        let start = el
            .attrs
            .get("start")
            .and_then(|v| v.trim().parse::<i32>().ok())
            .unwrap_or(1);
        if p.lists.len() < 32 {
            p.lists.push(ListCtx {
                ordered: el.tag == "ol",
                next: start,
            });
        }
    }

    // ---- content ----
    p.anc.push(el);
    let inner = Area { x: cx, w: cw };
    if let Some(t) = &mut table {
        y = table::layout(p, t, c, inner, y, c.width != Len::Auto);
    } else {
        y = flow(p, &el.children, c, c, inner, y);
        y = flush_inline(p, c, inner, y, None);
    }
    p.anc.pop();
    if is_list && !p.lists.is_empty() {
        p.lists.pop();
    }
    p.cur_form = saved_form;

    // A marker that never found a line (an empty item) sits on a line of its own.
    if own_marker && let Some(mk) = p.marker.take() {
        let f = p.font(c);
        let strut = strut_of(p, c);
        emit_marker(p, &mk, top + bt + pad_t + strut.0, f);
        y = adv(y, strut.0 + strut.1);
    }

    // ---- bottom ----
    if boxed_bottom {
        y = adv(y, core::mem::take(&mut p.pending));
        y = adv(y, pad_b + bb);
    }
    if c.min_height > 0 {
        y = y.max(top + p.z(c.min_height));
    }
    let height = (y - top).max(0);

    // Background behind the content, border on top of it.
    p.last_box = (None, None);
    if let Some(bg) = paint_bg {
        let rect = Cmd::Rect {
            x: bx,
            y: top,
            w: bw,
            h: height,
            color: bg,
            radius: p.z(c.radius).min(bw / 2).min(height / 2),
        };
        if bg_index <= p.cmds.len() && p.cmds.len() < MAX_CMDS {
            p.cmds.insert(bg_index, rect);
            p.last_box.0 = Some(bg_index);
        }
    }
    if bt + br + bb + bl > 0 {
        p.last_box.1 = Some(p.cmds.len());
        p.push_cmd(Cmd::Border {
            x: bx,
            y: top,
            w: bw,
            h: height,
            widths: [bt, br, bb, bl],
            radius: p.z(c.radius).min(bw / 2).min(height / 2),
            color: c.border_color,
        });
    }
    p.pending = p.pending.max(mb);
    y
}

/// The text of list counter `n`: `1`, `a`, `iv`.
fn counter_text(n: i32, style: ListStyle) -> String {
    match style {
        ListStyle::LowerAlpha | ListStyle::UpperAlpha => {
            if n < 1 {
                return alloc::format!("{n}");
            }
            let mut s = String::new();
            let mut k = n - 1;
            loop {
                let d = (k % 26) as u8;
                s.insert(
                    0,
                    char::from(if style == ListStyle::LowerAlpha {
                        b'a' + d
                    } else {
                        b'A' + d
                    }),
                );
                k = k / 26 - 1;
                if k < 0 || s.len() > 8 {
                    break;
                }
            }
            s
        }
        ListStyle::LowerRoman | ListStyle::UpperRoman => {
            if !(1..4000).contains(&n) {
                return alloc::format!("{n}");
            }
            let table = [
                (1000, "m"),
                (900, "cm"),
                (500, "d"),
                (400, "cd"),
                (100, "c"),
                (90, "xc"),
                (50, "l"),
                (40, "xl"),
                (10, "x"),
                (9, "ix"),
                (5, "v"),
                (4, "iv"),
                (1, "i"),
            ];
            let mut s = String::new();
            let mut k = n;
            for (v, r) in table {
                while k >= v {
                    s.push_str(r);
                    k -= v;
                }
            }
            if style == ListStyle::UpperRoman {
                s.to_uppercase()
            } else {
                s
            }
        }
        _ => alloc::format!("{n}"),
    }
}
