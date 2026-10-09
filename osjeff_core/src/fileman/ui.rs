//! The pixels of Arquivos as numbers: window regions, the sidebar, the breadcrumb bar, the
//! list and icon-grid geometry (hit testing, visible range, rubber band), the smooth scroller,
//! the drag-and-drop rules and the preview helpers.
//!
//! The kernel draws and routes input; everything it needs to *decide* lives here so it is
//! tested on the host and the renderer and the mouse handler cannot disagree. Text widths are
//! never guessed: callers pass measured widths in pixels.

use super::{APPS_PATH, Place, SortKey, TRASH_PATH};
use crate::anim::Spring;
use crate::window::Rect;
use alloc::string::String;
use alloc::vec::Vec;

pub const TITLE_H: i32 = crate::window::TITLE_H;
pub const SIDEBAR_W: i32 = 188;
pub const TOOLBAR_H: i32 = 44;
pub const HEADER_H: i32 = 28;
pub const ROW_H: i32 = 28;
pub const STATUS_H: i32 = 28;
pub const PREVIEW_W: i32 = 252;
/// Side of a square toolbar button.
pub const BTN: i32 = 28;
/// Padding above the first row and below the last.
pub const LIST_PAD: i32 = 4;
/// Horizontal inset of the selection pill of a row.
pub const ROW_INSET: i32 = 8;
/// Icon grid: cell size and outer padding.
pub const CELL_W: i32 = 104;
pub const CELL_H: i32 = 96;
pub const GRID_PAD: i32 = 12;
/// Pixels the pointer must travel with the button down before a press becomes a drag.
pub const DRAG_THRESHOLD: i32 = 5;
/// Width of the size and date columns of the list.
pub const SIZE_COL_W: i32 = 84;
pub const DATE_COL_W: i32 = 152;
/// Below this list width the date column is dropped.
pub const NARROW_LIST: i32 = 420;

/// How the folder is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    List,
    Icons,
}

impl ViewMode {
    pub fn index(self) -> usize {
        match self {
            ViewMode::List => 0,
            ViewMode::Icons => 1,
        }
    }

    pub fn from_index(i: usize) -> ViewMode {
        if i == 1 {
            ViewMode::Icons
        } else {
            ViewMode::List
        }
    }
}

// ---------------------------------------------------------------------------
// Window regions
// ---------------------------------------------------------------------------

/// Geometry of a file-manager window, all in screen coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub window: Rect,
    /// Everything below the title bar.
    pub body: Rect,
    pub sidebar: Rect,
    /// The column right of the sidebar (toolbar, list, status bar).
    pub main: Rect,
    pub toolbar: Rect,
    pub back: Rect,
    pub forward: Rect,
    /// The path bar (breadcrumbs).
    pub path: Rect,
    /// The two-segment view switch.
    pub view_switch: Rect,
    pub sort: Rect,
    pub search: Rect,
    pub preview_btn: Rect,
    /// Column titles of the list (empty height in icon mode).
    pub header: Rect,
    /// The scrolling viewport of the rows or icons.
    pub list: Rect,
    /// The preview pane, when open.
    pub preview: Option<Rect>,
    pub status: Rect,
}

impl Layout {
    /// Geometry for a window at `r`. `search_open` keeps the search field expanded even in a
    /// narrow window.
    pub fn of(r: Rect, mode: ViewMode, preview_open: bool, search_open: bool) -> Layout {
        let top = r.y + TITLE_H;
        let body = Rect::new(r.x, top, r.w, (r.h - TITLE_H).max(0));
        let sidebar = Rect::new(r.x, top, SIDEBAR_W.min(r.w), body.h);
        let main = Rect::new(r.x + SIDEBAR_W, top, (r.w - SIDEBAR_W).max(0), body.h);
        let toolbar = Rect::new(main.x, main.y, main.w, TOOLBAR_H.min(main.h));
        let by = toolbar.y + (TOOLBAR_H - BTN) / 2;
        let back = Rect::new(main.x + 12, by, BTN, BTN);
        let forward = Rect::new(back.right() + 2, by, BTN, BTN);
        // Right cluster, laid out from the right edge.
        let preview_btn = Rect::new(main.right() - 12 - BTN, by, BTN, BTN);
        let wide = main.w >= 620;
        let search_w = if search_open || wide {
            if main.w >= 820 { 188 } else { 148 }
        } else {
            BTN
        };
        let path_x = forward.right() + 12;
        let (path, view_switch, sort, search);
        if search_open && !wide {
            // Not enough room for everything: the field takes the place of the path bar and
            // of the view and sort buttons while it is open.
            let right = preview_btn.x - 8;
            search = Rect::new(path_x, by, (right - path_x).max(BTN), BTN);
            path = Rect::new(path_x, by, 0, BTN);
            view_switch = Rect::new(path_x, by, 0, BTN);
            sort = Rect::new(path_x, by, 0, BTN);
        } else {
            search = Rect::new(preview_btn.x - 8 - search_w, by, search_w, BTN);
            sort = Rect::new(search.x - 8 - BTN, by, BTN, BTN);
            view_switch = Rect::new(sort.x - 8 - 64, by, 64, BTN);
            path = Rect::new(path_x, by, (view_switch.x - 8 - path_x).max(0), BTN);
        }
        let status = Rect::new(
            main.x,
            (main.bottom() - STATUS_H).max(main.y),
            main.w,
            STATUS_H.min(main.h),
        );
        let content_top = toolbar.bottom();
        let pane_w = if preview_open {
            PREVIEW_W.min(main.w / 2)
        } else {
            0
        };
        let preview = (pane_w > 0).then(|| {
            Rect::new(
                main.right() - pane_w,
                content_top,
                pane_w,
                (status.y - content_top).max(0),
            )
        });
        let content_w = main.w - pane_w;
        let header_h = if mode == ViewMode::List { HEADER_H } else { 0 };
        let header = Rect::new(main.x, content_top, content_w, header_h);
        let list_y = content_top + header_h;
        let list = Rect::new(main.x, list_y, content_w, (status.y - list_y).max(0));
        Layout {
            window: r,
            body,
            sidebar,
            main,
            toolbar,
            back,
            forward,
            path,
            view_switch,
            sort,
            search,
            preview_btn,
            header,
            list,
            preview,
            status,
        }
    }

    /// Whether the search field is wide enough to be a field (else it is a magnifier button).
    pub fn search_is_field(&self) -> bool {
        self.search.w > BTN
    }

    /// The columns of the list for its current width.
    pub fn columns(&self) -> Columns {
        Columns::of(self.list.x, self.list.w)
    }
}

/// X positions of the list columns.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Columns {
    /// Left edge of the name column's content (icon starts here).
    pub name_x: i32,
    /// Left edge of the size column and of the date column; the date column is absent when
    /// `date_x == right`.
    pub size_x: i32,
    pub date_x: i32,
    pub right: i32,
}

impl Columns {
    pub fn of(x: i32, w: i32) -> Columns {
        let right = x + w;
        let date_x = if w < NARROW_LIST {
            right
        } else {
            right - DATE_COL_W - 12
        };
        let size_x = date_x - SIZE_COL_W;
        Columns {
            name_x: x + ROW_INSET + 8,
            size_x,
            date_x,
            right,
        }
    }

    pub fn has_date(&self) -> bool {
        self.date_x < self.right
    }

    /// The sort key under header x `px`.
    pub fn key_at(&self, px: i32) -> SortKey {
        if self.has_date() && px >= self.date_x {
            SortKey::Modified
        } else if px >= self.size_x {
            SortKey::Size
        } else {
            SortKey::Name
        }
    }
}

/// A sidebar entry and where it sits.
#[derive(Clone, Copy, Debug)]
pub struct SideLayout {
    pub favorites_title: Rect,
    pub places_title: Rect,
    pub items: [(Place, Rect); 6],
}

impl SideLayout {
    pub fn of(sidebar: Rect) -> SideLayout {
        let x = sidebar.x + 8;
        let w = (sidebar.w - 16).max(0);
        let mut y = sidebar.y + 12;
        let favorites_title = Rect::new(x + 8, y, w - 8, 24);
        y += 24;
        let mut items = [(Place::Home, Rect::new(0, 0, 0, 0)); 6];
        for (i, p) in [
            Place::Home,
            Place::Documents,
            Place::Images,
            Place::Apps,
            Place::Trash,
        ]
        .into_iter()
        .enumerate()
        {
            items[i] = (p, Rect::new(x, y, w, 28));
            y += 30;
        }
        y += 8;
        let places_title = Rect::new(x + 8, y, w - 8, 24);
        y += 24;
        items[5] = (Place::Disk, Rect::new(x, y, w, 56));
        SideLayout {
            favorites_title,
            places_title,
            items,
        }
    }

    pub fn place_at(&self, px: i32, py: i32) -> Option<Place> {
        self.items
            .iter()
            .find(|(_, r)| r.contains(px, py))
            .map(|&(p, _)| p)
    }
}

// ---------------------------------------------------------------------------
// The path bar
// ---------------------------------------------------------------------------

/// Gap between two crumbs (the chevron is drawn centred in it).
pub const CRUMB_GAP: i32 = 12;
/// Horizontal padding inside a crumb's hit area.
pub const CRUMB_PAD: i32 = 6;
/// Width of the folded-crumbs marker.
pub const CRUMB_FOLD_W: i32 = 28;
/// Padding of the path bar's ends.
pub const PATH_PAD: i32 = 8;

/// Where the crumbs of the path bar go.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CrumbLayout {
    /// Index of the first crumb shown.
    pub first: usize,
    /// `(crumb index, hit rectangle)`; the text starts [`CRUMB_PAD`] inside the rectangle.
    pub spans: Vec<(usize, Rect)>,
    /// The `...` marker standing for the folded crumbs.
    pub fold: Option<Rect>,
}

/// Lay the crumbs (whose measured text widths are `widths`) into the path bar `bar`. The last
/// crumb is always shown (cut to the room left when it alone is too wide); leading crumbs fold
/// into a marker when they do not fit.
pub fn crumb_layout(bar: Rect, widths: &[i32]) -> CrumbLayout {
    let n = widths.len();
    if n == 0 {
        return CrumbLayout {
            first: 0,
            spans: Vec::new(),
            fold: None,
        };
    }
    let avail = (bar.w - 2 * PATH_PAD).max(0);
    let item = |i: usize| widths[i].max(0) + 2 * CRUMB_PAD;
    let total = |from: usize| -> i32 {
        let body: i32 = (from..n).map(item).sum::<i32>() + CRUMB_GAP * (n - from - 1) as i32;
        if from > 0 {
            body + CRUMB_FOLD_W + CRUMB_GAP
        } else {
            body
        }
    };
    let mut first = 0;
    while first + 1 < n && total(first) > avail {
        first += 1;
    }
    let mut x = bar.x + PATH_PAD;
    let mut fold = None;
    let hit_h = bar.h - 8;
    let y = bar.y + 4;
    if first > 0 {
        fold = Some(Rect::new(x, y, CRUMB_FOLD_W, hit_h));
        x += CRUMB_FOLD_W + CRUMB_GAP;
    }
    let mut spans = Vec::new();
    for i in first..n {
        let room = (bar.right() - PATH_PAD - x).max(0);
        let w = item(i).min(room);
        spans.push((i, Rect::new(x, y, w, hit_h)));
        x += w + CRUMB_GAP;
    }
    CrumbLayout { first, spans, fold }
}

impl CrumbLayout {
    /// The crumb whose hit area contains `(px, py)`.
    pub fn crumb_at(&self, px: i32, py: i32) -> Option<usize> {
        self.spans
            .iter()
            .find(|(_, r)| r.contains(px, py))
            .map(|&(i, _)| i)
    }

    /// The folded marker was hit.
    pub fn fold_hit(&self, px: i32, py: i32) -> bool {
        self.fold.is_some_and(|r| r.contains(px, py))
    }
}

// ---------------------------------------------------------------------------
// What a press landed on
// ---------------------------------------------------------------------------

/// What a click landed on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    Back,
    Forward,
    /// Crumb index (into the full crumb list).
    Crumb(usize),
    /// The `...` marker of folded crumbs (goes to the parent of the first visible crumb).
    CrumbFold,
    /// Empty part of the path bar.
    PathBlank,
    View(ViewMode),
    SortButton,
    Search,
    PreviewButton,
    Place(Place),
    Header(SortKey),
    /// A row or icon (absolute index).
    Item(usize),
    /// Empty space in the list or icon area (a rubber band can start here).
    Blank,
    PreviewPane,
    /// Toolbar, sidebar or status space that does nothing.
    Dead,
}

/// What the hit test needs besides the layout.
pub struct HitCtx<'a> {
    pub mode: ViewMode,
    /// Scroll offset in pixels.
    pub scroll: i32,
    /// Number of rows shown.
    pub count: usize,
    pub crumbs: &'a CrumbLayout,
}

impl Layout {
    /// Resolve a press at `(px, py)`; `None` outside the window.
    pub fn hit(&self, px: i32, py: i32, ctx: &HitCtx<'_>) -> Option<Hit> {
        if !self.window.contains(px, py) {
            return None;
        }
        if self.sidebar.contains(px, py) {
            let side = SideLayout::of(self.sidebar);
            return Some(side.place_at(px, py).map_or(Hit::Dead, Hit::Place));
        }
        if self.back.contains(px, py) {
            return Some(Hit::Back);
        }
        if self.forward.contains(px, py) {
            return Some(Hit::Forward);
        }
        if self.path.contains(px, py) {
            if ctx.crumbs.fold_hit(px, py) {
                return Some(Hit::CrumbFold);
            }
            return Some(
                ctx.crumbs
                    .crumb_at(px, py)
                    .map_or(Hit::PathBlank, Hit::Crumb),
            );
        }
        if self.view_switch.contains(px, py) {
            let seg = if px < self.view_switch.x + self.view_switch.w / 2 {
                0
            } else {
                1
            };
            return Some(Hit::View(ViewMode::from_index(seg)));
        }
        if self.sort.contains(px, py) {
            return Some(Hit::SortButton);
        }
        if self.search.contains(px, py) {
            return Some(Hit::Search);
        }
        if self.preview_btn.contains(px, py) {
            return Some(Hit::PreviewButton);
        }
        if self.toolbar.contains(px, py) || self.status.contains(px, py) {
            return Some(Hit::Dead);
        }
        if self.header.h > 0 && self.header.contains(px, py) {
            return Some(Hit::Header(self.columns().key_at(px)));
        }
        if self.list.contains(px, py) {
            let (lx, ly) = (px - self.list.x, py - self.list.y);
            return Some(
                item_at(ctx.mode, self.list.w, ctx.scroll, lx, ly, ctx.count)
                    .map_or(Hit::Blank, Hit::Item),
            );
        }
        if self.preview.is_some_and(|p| p.contains(px, py)) {
            return Some(Hit::PreviewPane);
        }
        Some(Hit::Dead)
    }
}

// ---------------------------------------------------------------------------
// Rows and icons
// ---------------------------------------------------------------------------

/// Items per row of the view (1 for the list).
pub fn columns_of(mode: ViewMode, vw: i32) -> usize {
    match mode {
        ViewMode::List => 1,
        ViewMode::Icons => (((vw - 2 * GRID_PAD) / CELL_W).max(1)) as usize,
    }
}

/// Left edge of the icon grid (it is centred in the viewport).
fn grid_x0(vw: i32, cols: usize) -> i32 {
    GRID_PAD + ((vw - 2 * GRID_PAD - cols as i32 * CELL_W) / 2).max(0)
}

/// Height of all `n` items.
pub fn content_height(mode: ViewMode, vw: i32, n: usize) -> i32 {
    if n == 0 {
        return 0;
    }
    match mode {
        ViewMode::List => 2 * LIST_PAD + n as i32 * ROW_H,
        ViewMode::Icons => {
            let rows = n.div_ceil(columns_of(mode, vw)) as i32;
            2 * GRID_PAD + rows * CELL_H
        }
    }
}

/// The largest scroll offset.
pub fn max_scroll(mode: ViewMode, vw: i32, vh: i32, n: usize) -> i32 {
    (content_height(mode, vw, n) - vh).max(0)
}

/// The hit rectangle of item `i`, in content coordinates (relative to the viewport's top left
/// with scroll 0). The selection pill of a list row and the highlight of an icon use it.
pub fn item_rect(mode: ViewMode, vw: i32, i: usize) -> Rect {
    match mode {
        ViewMode::List => Rect::new(
            ROW_INSET,
            LIST_PAD + i as i32 * ROW_H,
            (vw - 2 * ROW_INSET).max(0),
            ROW_H,
        ),
        ViewMode::Icons => {
            let cols = columns_of(mode, vw);
            let (r, c) = ((i / cols) as i32, (i % cols) as i32);
            let x0 = grid_x0(vw, cols);
            Rect::new(
                x0 + c * CELL_W + 4,
                GRID_PAD + r * CELL_H + 2,
                CELL_W - 8,
                CELL_H - 4,
            )
        }
    }
}

/// The items (first, one past the last) that intersect a viewport of height `vh` scrolled by
/// `scroll`, for `n` items.
pub fn visible_range(mode: ViewMode, vw: i32, vh: i32, scroll: i32, n: usize) -> (usize, usize) {
    if n == 0 || vh <= 0 {
        return (0, 0);
    }
    let cols = columns_of(mode, vw);
    let (pitch, pad) = match mode {
        ViewMode::List => (ROW_H, LIST_PAD),
        ViewMode::Icons => (CELL_H, GRID_PAD),
    };
    let first_row = ((scroll - pad).max(0) / pitch) as usize;
    let last_row = ((scroll + vh - pad - 1).max(0) / pitch) as usize;
    let first = (first_row * cols).min(n);
    let end = ((last_row + 1) * cols).min(n);
    (first, end)
}

/// The item under viewport-relative `(x, y)` when scrolled by `scroll`.
pub fn item_at(mode: ViewMode, vw: i32, scroll: i32, x: i32, y: i32, n: usize) -> Option<usize> {
    if x < 0 || y < 0 || x >= vw {
        return None;
    }
    let cy = y + scroll;
    let cols = columns_of(mode, vw);
    let (pitch, pad) = match mode {
        ViewMode::List => (ROW_H, LIST_PAD),
        ViewMode::Icons => (CELL_H, GRID_PAD),
    };
    if cy < pad {
        return None;
    }
    let row = ((cy - pad) / pitch) as usize;
    let col = match mode {
        ViewMode::List => 0,
        ViewMode::Icons => {
            let x0 = grid_x0(vw, cols);
            if x < x0 {
                return None;
            }
            let c = ((x - x0) / CELL_W) as usize;
            if c >= cols {
                return None;
            }
            c
        }
    };
    let i = row * cols + col;
    if i >= n {
        return None;
    }
    item_rect(mode, vw, i).contains(x, cy).then_some(i)
}

/// The items touched by `band` (content coordinates): what a rubber band selects.
pub fn items_in_rect(mode: ViewMode, vw: i32, n: usize, band: Rect) -> Vec<usize> {
    if n == 0 || band.w <= 0 || band.h <= 0 {
        return Vec::new();
    }
    let cols = columns_of(mode, vw);
    let (pitch, pad) = match mode {
        ViewMode::List => (ROW_H, LIST_PAD),
        ViewMode::Icons => (CELL_H, GRID_PAD),
    };
    let r0 = ((band.y - pad).max(0) / pitch) as usize;
    let r1 = ((band.bottom() - 1 - pad).max(0) / pitch) as usize;
    let mut out = Vec::new();
    for row in r0..=r1 {
        for col in 0..cols {
            let i = row * cols + col;
            if i >= n {
                break;
            }
            if item_rect(mode, vw, i).intersection(&band).is_some() {
                out.push(i);
            }
        }
    }
    out
}

/// The scroll offset that brings item `i` fully into a viewport of height `vh` with the least
/// movement (unchanged when it is already visible).
pub fn reveal(mode: ViewMode, vw: i32, vh: i32, scroll: i32, i: usize, n: usize) -> i32 {
    let r = item_rect(mode, vw, i);
    // Include the padding when the item is in the first or last row.
    let top = if r.y <= GRID_PAD.max(LIST_PAD) + 4 {
        0
    } else {
        r.y - 4
    };
    let bottom = {
        let b = r.bottom() + 4;
        if b + GRID_PAD.max(LIST_PAD) >= content_height(mode, vw, n) {
            content_height(mode, vw, n)
        } else {
            b
        }
    };
    let s = if top < scroll {
        top
    } else if bottom > scroll + vh {
        bottom - vh
    } else {
        scroll
    };
    s.clamp(0, max_scroll(mode, vw, vh, n))
}

/// An arrow-key direction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Where an arrow key moves the cursor from `i` among `n` items.
pub fn step_index(mode: ViewMode, vw: i32, i: usize, dir: Dir, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let cols = columns_of(mode, vw);
    let last = n - 1;
    let i = i.min(last);
    match (mode, dir) {
        (ViewMode::List, Dir::Up | Dir::Left) => i.saturating_sub(1),
        (ViewMode::List, Dir::Down | Dir::Right) => (i + 1).min(last),
        (ViewMode::Icons, Dir::Left) => i.saturating_sub(1),
        (ViewMode::Icons, Dir::Right) => (i + 1).min(last),
        (ViewMode::Icons, Dir::Up) => {
            if i >= cols {
                i - cols
            } else {
                i
            }
        }
        (ViewMode::Icons, Dir::Down) => {
            if i + cols <= last {
                i + cols
            } else if i / cols < last / cols {
                // A short last row: land on its final item.
                last
            } else {
                i
            }
        }
    }
}

/// Items a page-up or page-down jumps over for a viewport of height `vh`.
pub fn page_items(mode: ViewMode, vw: i32, vh: i32) -> usize {
    let per_row = columns_of(mode, vw);
    let pitch = match mode {
        ViewMode::List => ROW_H,
        ViewMode::Icons => CELL_H,
    };
    (((vh / pitch) - 1).max(1) as usize) * per_row
}

// ---------------------------------------------------------------------------
// Rubber band and drags
// ---------------------------------------------------------------------------

/// The normalised rectangle spanned by two corners (both inclusive).
pub fn band_rect(a: (i32, i32), b: (i32, i32)) -> Rect {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
    Rect::new(x0, y0, x1 - x0 + 1, y1 - y0 + 1)
}

/// Whether the pointer has moved far enough from where it was pressed to start a drag.
pub fn drag_started(press: (i32, i32), now: (i32, i32)) -> bool {
    let (dx, dy) = ((now.0 - press.0).abs(), (now.1 - press.1).abs());
    dx.max(dy) >= DRAG_THRESHOLD
}

/// Pixels to scroll per frame while a drag holds the pointer at `y` over a viewport spanning
/// `top..bottom`: negative above, positive below, zero in the middle.
pub fn edge_scroll(y: i32, top: i32, bottom: i32) -> i32 {
    const ZONE: i32 = 28;
    const MAX: i32 = 18;
    if y < top + ZONE {
        -(((top + ZONE - y).min(ZONE + 24) * MAX) / ZONE).min(MAX)
    } else if y > bottom - ZONE {
        (((y - (bottom - ZONE)).min(ZONE + 24) * MAX) / ZONE).min(MAX)
    } else {
        0
    }
}

/// The selection a rubber band produces: the items it touches, added to `base` with Ctrl (and
/// toggled off when both), else alone. Ascending, no duplicates.
pub fn band_selection(base: &[usize], touched: &[usize], additive: bool) -> Vec<usize> {
    let mut out: Vec<usize> = if additive { base.to_vec() } else { Vec::new() };
    for &i in touched {
        if !out.contains(&i) {
            out.push(i);
        }
    }
    out.sort_unstable();
    out
}

// ---------------------------------------------------------------------------
// Drag and drop
// ---------------------------------------------------------------------------

/// What a drop would do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropOp {
    Move,
    Copy,
    Trash,
}

/// Where the pointer is while dragging items.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DropTarget<'a> {
    /// A folder (absolute path).
    Folder(&'a [u8]),
    /// The bin.
    Trash,
    /// Nothing that accepts a drop.
    None,
}

/// The place a sidebar entry stands for as a drop target.
pub fn place_target(p: Place) -> DropTarget<'static> {
    match p {
        Place::Trash => DropTarget::Trash,
        Place::Apps => DropTarget::None,
        other => DropTarget::Folder(other.path()),
    }
}

fn parent_of(path: &[u8]) -> &[u8] {
    match path.iter().rposition(|&b| b == b'/') {
        Some(0) | None => b"/",
        Some(i) => &path[..i],
    }
}

fn is_inside(path: &[u8], folder: &[u8]) -> bool {
    if folder == b"/" {
        return true;
    }
    path == folder || (path.starts_with(folder) && path.get(folder.len()) == Some(&b'/'))
}

/// Whether dropping `sources` into the folder `dest` is meaningful: it is not a source itself or
/// inside one, and at least one source would actually change folders.
pub fn can_drop_into(sources: &[Vec<u8>], dest: &[u8]) -> bool {
    if sources.is_empty() || dest == TRASH_PATH || dest == APPS_PATH {
        return false;
    }
    if sources.iter().any(|s| is_inside(dest, s)) {
        return false;
    }
    sources.iter().any(|s| parent_of(s) != dest)
}

/// The operation a drop of `sources` onto `target` performs, or `None` when it would do nothing
/// useful. `copy` is the Ctrl key: a copy instead of a move. The bin always moves to the bin.
pub fn plan_drop(sources: &[Vec<u8>], target: DropTarget<'_>, copy: bool) -> Option<DropOp> {
    match target {
        DropTarget::Trash => (!sources.is_empty()).then_some(DropOp::Trash),
        DropTarget::Folder(dest) => {
            // A copy into the folder the items already live in would duplicate them, which
            // the paste command does; a drop is for going somewhere else.
            can_drop_into(sources, dest).then_some(if copy { DropOp::Copy } else { DropOp::Move })
        }
        DropTarget::None => None,
    }
}

// ---------------------------------------------------------------------------
// Smooth scrolling
// ---------------------------------------------------------------------------

/// Pixels one wheel notch scrolls.
pub const WHEEL_STEP: i32 = 3 * ROW_H;

/// A scroll offset that follows its target with a critically damped spring: wheel notches and
/// keyboard moves change the target, the position glides there.
#[derive(Clone, Copy, Debug)]
pub struct Scroller {
    spring: Spring,
    max: i32,
}

impl Default for Scroller {
    fn default() -> Self {
        Self::new()
    }
}

impl Scroller {
    pub const fn new() -> Self {
        Scroller {
            spring: Spring::pixels(0.0, 300.0, 34.6),
            max: 0,
        }
    }

    /// The offset to draw with.
    pub fn pos(&self) -> i32 {
        let v = self.spring.value();
        (v + 0.5) as i32
    }

    /// Where it is heading.
    pub fn target(&self) -> i32 {
        (self.spring.target() + 0.5) as i32
    }

    pub fn max(&self) -> i32 {
        self.max
    }

    /// The content or viewport changed: the largest offset is now `max`.
    pub fn set_max(&mut self, max: i32) {
        self.max = max.max(0);
        let t = self.spring.target().clamp(0.0, self.max as f32);
        self.spring.set_target(t);
        if self.spring.value() > self.max as f32 {
            self.spring.jump(t);
        }
    }

    /// Move the target by `delta` pixels (a wheel notch, a key).
    pub fn scroll_by(&mut self, delta: i32) {
        let t = (self.spring.target() + delta as f32).clamp(0.0, self.max as f32);
        self.spring.set_target(t);
    }

    /// Aim at an absolute offset.
    pub fn scroll_to(&mut self, to: i32) {
        self.spring.set_target(to.clamp(0, self.max) as f32);
    }

    /// Jump with no animation (a new folder, a drag that scrolls).
    pub fn jump(&mut self, to: i32) {
        self.spring.jump(to.clamp(0, self.max) as f32);
    }

    /// Advance by `dt` seconds; `true` while moving.
    pub fn step(&mut self, dt: f32) -> bool {
        self.spring.step(dt)
    }

    pub fn at_rest(&self) -> bool {
        self.spring.at_rest()
    }
}

// ---------------------------------------------------------------------------
// Names, kinds and dates
// ---------------------------------------------------------------------------

/// A name folded for searching: ASCII lowercase with accents removed, so `acao` finds `Ação`.
pub fn search_key(name: &[u8]) -> String {
    let folded = super::display_ascii(name);
    folded
        .iter()
        .map(|&b| b.to_ascii_lowercase() as char)
        .collect()
}

/// The folded, trimmed form of a search query, ready for [`matches_key`].
pub fn query_key(query: &[u8]) -> String {
    String::from(search_key(query).trim())
}

/// Whether `name` contains the folded query `key` (an empty key matches everything).
pub fn matches_key(name: &[u8], key: &str) -> bool {
    key.is_empty() || search_key(name).contains(key)
}

/// Whether `name` matches the search `query` (an empty query matches everything).
pub fn matches_query(name: &[u8], query: &[u8]) -> bool {
    matches_key(name, &query_key(query))
}

/// How a row is described in the preview pane and the file icons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PreviewKind {
    Image,
    Text,
    Folder,
    App,
    Other,
}

/// What a name (and whether it is a folder) previews as.
pub fn preview_kind(name: &[u8], is_dir: bool) -> PreviewKind {
    use super::FileClass;
    if is_dir {
        return PreviewKind::Folder;
    }
    match super::classify(name) {
        FileClass::Image => PreviewKind::Image,
        FileClass::Wasm => PreviewKind::App,
        FileClass::Text => PreviewKind::Text,
        FileClass::Other => PreviewKind::Other,
    }
}

/// The file icon a name uses.
pub fn icon_kind(name: &[u8], is_dir: bool) -> crate::appart::FileKind {
    use crate::appart::FileKind;
    match preview_kind(name, is_dir) {
        PreviewKind::Folder => FileKind::Folder,
        PreviewKind::Image => FileKind::Image,
        PreviewKind::App => FileKind::App,
        PreviewKind::Text => FileKind::Text,
        PreviewKind::Other => FileKind::Generic,
    }
}

/// A human description of a file by its name: `Pasta`, `Imagem PNG`, `Texto`, `Aplicativo`...
pub fn kind_label(name: &[u8], is_dir: bool) -> String {
    if is_dir {
        return String::from("Pasta");
    }
    let ext = super::extension(name);
    let upper: String = ext
        .iter()
        .map(|&b| (b as char).to_ascii_uppercase())
        .collect();
    match preview_kind(name, false) {
        PreviewKind::Image => alloc::format!("Imagem {upper}"),
        PreviewKind::App => String::from("Aplicativo"),
        PreviewKind::Text if ext.is_empty() => String::from("Texto"),
        PreviewKind::Text => alloc::format!("Texto {upper}"),
        _ if ext.is_empty() => String::from("Arquivo"),
        _ => alloc::format!("Arquivo {upper}"),
    }
}

/// Largest file the preview decodes as an image.
pub const PREVIEW_MAX_IMAGE: u64 = 3 * 1024 * 1024;
/// Bytes read from the start of a text file for the preview.
pub const PREVIEW_TEXT_BYTES: usize = 6 * 1024;

/// The first lines of a text for the preview: invalid UTF-8 shown as replacement characters,
/// tabs as four spaces, other control characters as spaces, at most `max_lines` lines of
/// `max_chars` characters each. A file that is not text gives no lines.
pub fn text_preview(bytes: &[u8], max_lines: usize, max_chars: usize) -> Vec<String> {
    if !super::looks_like_text(bytes) {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    for raw in text.split('\n') {
        if out.len() >= max_lines {
            break;
        }
        let mut line = String::new();
        let mut n = 0;
        for ch in raw.trim_end_matches('\r').chars() {
            if n >= max_chars {
                break;
            }
            match ch {
                '\t' => {
                    line.push_str("    ");
                    n += 4;
                }
                c if c.is_control() => {
                    line.push(' ');
                    n += 1;
                }
                c => {
                    line.push(c);
                    n += 1;
                }
            }
        }
        out.push(line);
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out
}

/// The modified time as people say it: `Hoje, 14:32`, `Ontem, 09:10`, else `dd/mm/aaaa hh:mm`.
/// `0` (the clock was not set when the file was written) shows `--`.
pub fn format_modified(unix: u64, now: u64, tz_secs: i32) -> String {
    if unix == 0 {
        return String::from("--");
    }
    let day = |t: u64| (t as i64 + tz_secs as i64).div_euclid(86_400);
    let secs = (unix as i64 + tz_secs as i64).rem_euclid(86_400);
    let hm = alloc::format!("{:02}:{:02}", secs / 3600, (secs % 3600) / 60);
    match day(now) - day(unix) {
        0 => alloc::format!("Hoje, {hm}"),
        1 => alloc::format!("Ontem, {hm}"),
        _ => super::format_datetime(unix, tz_secs),
    }
}

#[cfg(test)]
mod tests;
