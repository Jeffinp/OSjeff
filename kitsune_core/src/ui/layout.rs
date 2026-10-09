//! Desktop geometry: dock, context menu, start panel, calculator keypad,
//! browser chrome and file-manager hit zones.
//!
//! Pure integer layout shared by the kernel's drawing code and its hit-testing,
//! so the two can never disagree. Nothing here touches pixels.

use crate::windowing::window::{Rect, TITLE_H};
use alloc::vec::Vec;

/// Calculator keypad: the input byte for each cell, six rows of four. The first row is the
/// memory keys (`MC MR M- M+`, see `calc::KEY_*`), then `C`, backspace (`0x08`), percent and
/// divide; the digit rows each end in their operator; the last row is sign, `0`, the point
/// and equals.
pub const CALC_KEYS: [[u8; 4]; 6] = [
    [0x01, 0x02, 0x03, 0x04],
    [b'C', 0x08, b'%', b'/'],
    *b"789*",
    *b"456-",
    *b"123+",
    *b"n0.=",
];

/// Where everything sits in a calculator window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalcGeom {
    /// The strip with the pending expression and the last results.
    pub history: Rect,
    /// The big number.
    pub display: Rect,
    /// The copy button, at the right end of the history strip.
    pub copy: Rect,
    /// Every key, row by row.
    pub keys: [[Rect; 4]; 6],
}

/// Height of the memory row; the other rows share the rest.
pub const CALC_MEM_H: i32 = 28;
/// Gap between keys.
pub const CALC_GAP: i32 = 8;

/// Geometry of calculator window `r` (the window rectangle, title bar included).
pub fn calc_geom(r: Rect) -> CalcGeom {
    let pad = 16;
    let x = r.x + pad;
    let w = (r.w - 2 * pad).max(0);
    let history = Rect::new(x, r.y + TITLE_H + 8, w, 36);
    let display = Rect::new(x, history.bottom(), w, 64);
    let copy = Rect::new(history.right() - 28, history.y + 2, 28, 24);
    let gy = display.bottom() + 8;
    let cw = ((w - CALC_GAP * 3) / 4).max(0);
    let grid_h = r.bottom() - pad - gy;
    let ch = ((grid_h - CALC_MEM_H - CALC_GAP * 5) / 5).max(0);
    let mut keys = [[Rect::new(0, 0, 0, 0); 4]; 6];
    let mut y = gy;
    for (row, cells) in keys.iter_mut().enumerate() {
        let h = if row == 0 { CALC_MEM_H } else { ch };
        for (col, cell) in cells.iter_mut().enumerate() {
            *cell = Rect::new(x + col as i32 * (cw + CALC_GAP), y, cw, h);
        }
        y += h + CALC_GAP;
    }
    CalcGeom {
        history,
        display,
        copy,
        keys,
    }
}

/// What a click in a calculator window hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalcHit {
    /// A key, by its input byte.
    Key(u8),
    /// The copy button, or the display itself.
    Copy,
}

/// The key (or the copy action) under `(px, py)` in calculator window `r`, if any.
pub fn calc_hit(r: Rect, px: i32, py: i32) -> Option<CalcHit> {
    let g = calc_geom(r);
    if g.copy.contains(px, py) || g.display.contains(px, py) {
        return Some(CalcHit::Copy);
    }
    for (row, cells) in g.keys.iter().enumerate() {
        for (col, cell) in cells.iter().enumerate() {
            if cell.w > 0 && cell.h > 0 && cell.contains(px, py) {
                return Some(CalcHit::Key(CALC_KEYS[row][col]));
            }
        }
    }
    None
}

/// The keypad byte under `(px, py)` in calculator window `r`, if any.
pub fn calc_button_at(r: Rect, px: i32, py: i32) -> Option<u8> {
    match calc_hit(r, px, py)? {
        CalcHit::Key(k) => Some(k),
        CalcHit::Copy => None,
    }
}

// --------------------------------------------------------------------- browser

/// Height of the toolbar band under the title bar.
pub const BROWSER_TOOLBAR_H: i32 = 48;
/// Height of the tab strip (shown from two tabs on).
pub const BROWSER_STRIP_H: i32 = 36;
/// Side of the toolbar buttons.
pub const BROWSER_BTN: i32 = 32;
/// Height of a tab.
pub const BROWSER_TAB_H: i32 = 28;
/// Widest and narrowest a tab gets.
pub const BROWSER_TAB_MAX_W: i32 = 208;
pub const BROWSER_TAB_MIN_W: i32 = 72;

/// Browser chrome geometry, shared by drawing and hit-testing so the toolbar
/// buttons, omnibox, tabs and content area always agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrowserChrome {
    /// The band under the title bar that holds the buttons and the omnibox.
    pub toolbar: Rect,
    pub back: Rect,
    pub forward: Rect,
    pub reload: Rect,
    /// The omnibox.
    pub bar: Rect,
    /// The security indicator, inside the left end of the omnibox (empty when there is none).
    pub shield: Rect,
    /// The favourite star, inside the right end of the omnibox.
    pub star: Rect,
    /// The "new tab" button right of the omnibox.
    pub newtab: Rect,
    /// The loading progress track under the omnibox.
    pub progress: Rect,
    /// The tab strip (zero height with a single tab).
    pub strip: Rect,
    /// The page area, edge to edge below the chrome.
    pub content: Rect,
}

impl BrowserChrome {
    /// Geometry for window `r` with `tabs` tabs; `shield_w` is the width of the security
    /// indicator (0 for none: the start page and internal pages).
    pub fn of(r: Rect, tabs: usize, shield_w: i32) -> Self {
        Self::with_strip(r, shield_w, if tabs >= 2 { BROWSER_STRIP_H } else { 0 })
    }

    /// [`BrowserChrome::of`] with the strip `strip_h` pixels tall (it grows and shrinks while a
    /// second tab opens or the second to last closes).
    pub fn with_strip(r: Rect, shield_w: i32, strip_h: i32) -> Self {
        let pad = 12;
        let y0 = r.y + TITLE_H;
        let toolbar = Rect::new(r.x, y0, r.w, BROWSER_TOOLBAR_H);
        let by = y0 + (BROWSER_TOOLBAR_H - BROWSER_BTN) / 2;
        let back = Rect::new(r.x + pad, by, BROWSER_BTN, BROWSER_BTN);
        let forward = Rect::new(back.right() + 4, by, BROWSER_BTN, BROWSER_BTN);
        let reload = Rect::new(forward.right() + 8, by, BROWSER_BTN, BROWSER_BTN);
        let newtab = Rect::new(r.right() - pad - BROWSER_BTN, by, BROWSER_BTN, BROWSER_BTN);
        let bar_x = reload.right() + 12;
        let bar = Rect::new(bar_x, by, (newtab.x - 8 - bar_x).max(120), BROWSER_BTN);
        let shield = if shield_w > 0 {
            Rect::new(bar.x + 4, bar.y + 4, shield_w.min(bar.w / 2), 24)
        } else {
            Rect::new(bar.x + 4, bar.y + 4, 0, 24)
        };
        let star = Rect::new(bar.right() - 4 - 24, bar.y + 4, 24, 24);
        let progress = Rect::new(bar.x + 12, bar.bottom() + 3, (bar.w - 24).max(0), 2);
        let strip = Rect::new(
            r.x,
            toolbar.bottom(),
            r.w,
            strip_h.clamp(0, BROWSER_STRIP_H),
        );
        let cy = strip.bottom();
        let content = Rect::new(r.x, cy, r.w, (r.bottom() - cy).max(0));
        Self {
            toolbar,
            back,
            forward,
            reload,
            bar,
            shield,
            star,
            newtab,
            progress,
            strip,
            content,
        }
    }

    /// Left edge of the address text inside the omnibox.
    pub fn text_x(&self) -> i32 {
        if self.shield.w > 0 {
            self.shield.right() + 8
        } else {
            self.bar.x + 36
        }
    }
}

/// The tabs of a strip: one rectangle per entry of `weights` (0..=256, how far each tab
/// has grown while it opens or closes), left to right with 4 px between them. Tabs
/// share the strip equally up to [`BROWSER_TAB_MAX_W`] and never get below
/// [`BROWSER_TAB_MIN_W`] (the strip then overflows and the caller scrolls or clips).
pub fn browser_tab_rects(strip: Rect, weights: &[i32]) -> Vec<Rect> {
    let n = weights.len() as i32;
    if n == 0 {
        return Vec::new();
    }
    let total: i32 = weights.iter().map(|w| (*w).clamp(0, 256)).sum();
    let avail = (strip.w - 24 - 4 * (n - 1)).max(0);
    // One full tab's width: the room shared by the full tabs (a growing one counts less).
    let slot = if total == 0 {
        BROWSER_TAB_MAX_W
    } else {
        (avail * 256 / total).clamp(BROWSER_TAB_MIN_W, BROWSER_TAB_MAX_W)
    };
    let mut x = strip.x + 12;
    let y = strip.y + (strip.h - BROWSER_TAB_H) / 2;
    weights
        .iter()
        .map(|w| {
            let w = (*w).clamp(0, 256);
            let tw = slot * w / 256;
            let r = Rect::new(x, y, tw, BROWSER_TAB_H);
            x += tw + if w > 0 { 4 * w / 256 } else { 0 };
            r
        })
        .collect()
}

/// The close button inside tab `tab`.
pub fn browser_tab_close(tab: Rect) -> Rect {
    Rect::new(tab.right() - 6 - 20, tab.y + (tab.h - 20) / 2, 20, 20)
}

/// The index of the tab under `(x, y)`.
pub fn browser_tab_at(rects: &[Rect], x: i32, y: i32) -> Option<usize> {
    rects.iter().position(|r| r.w > 0 && r.contains(x, y))
}

/// Rows of the suggestion list are 32 px tall.
pub const BROWSER_SUGGEST_ROW: i32 = 32;

/// The panel of `n` suggestions hanging under the omnibox `bar`.
pub fn browser_suggest_panel(bar: Rect, n: usize) -> Rect {
    Rect::new(
        bar.x,
        bar.bottom() + 6,
        bar.w,
        n as i32 * BROWSER_SUGGEST_ROW + 12,
    )
}

/// Rect of suggestion row `i` under the omnibox.
pub fn browser_suggestion_row(bar: Rect, i: usize) -> Rect {
    let p = browser_suggest_panel(bar, 0);
    Rect::new(
        p.x + 6,
        p.y + 6 + i as i32 * BROWSER_SUGGEST_ROW,
        bar.w - 12,
        BROWSER_SUGGEST_ROW,
    )
}

/// Row of the suggestion list under point `(px, py)` when `n` rows are shown.
pub fn browser_suggestion_at(bar: Rect, n: usize, px: i32, py: i32) -> Option<usize> {
    (0..n).find(|&i| browser_suggestion_row(bar, i).contains(px, py))
}

/// The popover that explains the security indicator, `h` tall, under the omnibox's left end,
/// kept inside window `win`.
pub fn browser_popover(bar: Rect, win: Rect, h: i32) -> Rect {
    let w = 328.min(win.w - 16).max(120);
    let x = bar.x.clamp(win.x + 8, (win.right() - w - 8).max(win.x + 8));
    Rect::new(x, bar.bottom() + 8, w, h)
}

/// Where everything of the find bar goes: a slim bar in the top right of the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindLayout {
    pub bar: Rect,
    pub field: Rect,
    pub count: Rect,
    pub prev: Rect,
    pub next: Rect,
    pub close: Rect,
}

/// Layout of the find bar in page area `content`.
pub fn browser_find_layout(content: Rect) -> FindLayout {
    let w = 372.min(content.w - 24).max(200);
    let bar = Rect::new(
        (content.right() - 12 - w).max(content.x + 4),
        content.y + 12,
        w,
        40,
    );
    let close = Rect::new(bar.right() - 8 - 24, bar.y + 8, 24, 24);
    let next = Rect::new(close.x - 4 - 24, bar.y + 8, 24, 24);
    let prev = Rect::new(next.x - 24, bar.y + 8, 24, 24);
    let count = Rect::new(prev.x - 8 - 64, bar.y + 8, 64, 24);
    let field = Rect::new(
        bar.x + 8,
        bar.y + 6,
        (count.x - 8 - (bar.x + 8)).max(40),
        28,
    );
    FindLayout {
        bar,
        field,
        count,
        prev,
        next,
        close,
    }
}

/// The zoom pill at the bottom right of the page.
pub fn browser_zoom_pill(content: Rect) -> Rect {
    Rect::new(
        content.right() - 16 - 72,
        content.bottom() - 16 - 28,
        72,
        28,
    )
}

/// Where the parts of an error page go, centred in the page area.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrorLayout {
    /// The illustration.
    pub art: Rect,
    pub title: Rect,
    pub cause: Rect,
    pub retry: Rect,
    /// "Continue anyway" (certificate errors only).
    pub proceed: Rect,
    /// The one-line warning under it.
    pub note: Rect,
}

/// Layout of an error page in `content`; `cert` adds the "continue anyway" row.
pub fn browser_error_layout(content: Rect, cert: bool) -> ErrorLayout {
    let cw = (content.w - 48).clamp(120, 520);
    let cx = content.x + content.w / 2;
    // The block is centred vertically a little above the middle.
    let block_h = 96 + 24 + 32 + 8 + 24 + 28 + 40 + if cert { 12 + 32 + 8 + 20 } else { 0 };
    let top = content.y + ((content.h - block_h) / 2 - 12).max(16);
    let art = Rect::new(cx - 48, top, 96, 96);
    let title = Rect::new(cx - cw / 2, art.bottom() + 24, cw, 32);
    let cause = Rect::new(cx - cw / 2, title.bottom() + 8, cw, 24);
    let retry = Rect::new(cx - 88, cause.bottom() + 28, 176, 40);
    let pw = (cw).min(300);
    let proceed = Rect::new(cx - pw / 2, retry.bottom() + 12, pw, 32);
    let note = Rect::new(cx - cw / 2, proceed.bottom() + 8, cw, 20);
    ErrorLayout {
        art,
        title,
        cause,
        retry,
        proceed,
        note,
    }
}

/// Where the parts of the new-tab page go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartLayout {
    /// The big search field.
    pub search: Rect,
    /// Heading rows of the tile grid and of the recent list.
    pub tiles_heading: Rect,
    pub tiles: Vec<Rect>,
    pub recent_heading: Rect,
    pub recents: Vec<Rect>,
}

/// Layout of the new-tab page in `content` for up to `nt` tiles and `nr` recent rows; what
/// does not fit is left out.
pub fn browser_start_layout(content: Rect, nt: usize, nr: usize) -> StartLayout {
    let cw = (content.w - 48).clamp(160, 600);
    let cx = content.x + content.w / 2;
    let left = cx - cw / 2;
    let search = Rect::new(left, content.y + (content.h / 8).clamp(24, 72), cw, 48);
    let tiles_heading = Rect::new(left, search.bottom() + 28, cw, 20);
    // Tiles: as many columns of 104 px as fit (at most 5), 16 px apart.
    let cols = (((cw + 16) / (104 + 16)).clamp(1, 5)) as usize;
    let tile_w = (cw - 16 * (cols as i32 - 1)) / cols as i32;
    let tile_h = 96;
    let mut tiles = Vec::new();
    let mut y = tiles_heading.bottom() + 12;
    let room_for_rows = |y: i32, extra: i32| y + tile_h + extra <= content.bottom() - 8;
    let mut i = 0;
    while i < nt && room_for_rows(y, 0) {
        let row = (nt - i).min(cols);
        for k in 0..row {
            tiles.push(Rect::new(
                left + k as i32 * (tile_w + 16),
                y,
                tile_w,
                tile_h,
            ));
        }
        i += row;
        y += tile_h + 16;
    }
    let tiles_bottom = if tiles.is_empty() {
        tiles_heading.bottom()
    } else {
        y - 16
    };
    let recent_heading = Rect::new(left, tiles_bottom + 24, cw, 20);
    let mut recents = Vec::new();
    let mut ry = recent_heading.bottom() + 8;
    for _ in 0..nr {
        if ry + 40 > content.bottom() - 8 {
            break;
        }
        recents.push(Rect::new(left, ry, cw, 40));
        ry += 40;
    }
    StartLayout {
        search,
        tiles_heading,
        tiles,
        recent_heading,
        recents,
    }
}

// ------------------------------------------------------------- work area & fit

/// Gap kept between the work area and the floating taskbar.
pub const WORK_DOCK_GAP: i32 = 8;

/// The rectangle windows maximize and snap into: the screen below the top panel, edge to edge,
/// down to the taskbar (plus a small gap).
pub fn work_area(sw: i32, sh: i32) -> Rect {
    let dock_top = sh - crate::windowing::taskbar::BOTTOM - crate::windowing::taskbar::H;
    let top = crate::windowing::window::MENUBAR_H;
    Rect::new(0, top, sw.max(0), (dock_top - WORK_DOCK_GAP - top).max(0))
}

/// Largest integer text scale in `base..=max` at which a text grid fits an
/// `avail_w x avail_h` area. The grid measures `grid_w x grid_h` pixels at
/// scale 1. When even `base` does not fit, `base` is returned (the caller
/// clips). Apps with a fixed logical grid (terminal, editor) use this so a
/// maximized window shows bigger, undistorted text instead of empty space.
pub fn fit_scale(avail_w: i32, avail_h: i32, grid_w: i32, grid_h: i32, base: i32, max: i32) -> i32 {
    let mut best = base;
    let mut s = base + 1;
    while s <= max {
        if grid_w * s <= avail_w && grid_h * s <= avail_h {
            best = s;
        }
        s += 1;
    }
    best
}

// ---------------------------------------------------------------- file manager

/// Width of the file manager's sidebar.
pub const FILES_SIDEBAR_W: i32 = 176;
/// Height of one file-list row.
pub const FILES_ROW_H: i32 = 30;
/// Offset of the first list row below the title bar.
const FILES_LIST_TOP: i32 = 78;

/// What a click inside the file manager landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilesHit {
    /// A sidebar entry: switch to this view.
    View(u8),
    /// A row of the main list (0-based, not yet bounds-checked against the
    /// number of entries).
    Row(usize),
}

/// Resolves a click at `(px, py)` in a file-manager window `rect` currently
/// showing `view` (0 = files, 1 = trash; rows only exist in those views).
pub fn files_hit(rect: Rect, view: u8, px: i32, py: i32) -> Option<FilesHit> {
    let cy0 = rect.y + TITLE_H;
    if px < rect.x + FILES_SIDEBAR_W {
        let zones = [
            (34, 64, 0u8),
            (66, 96, 1),
            (132, 162, 2),
            (164, 194, 3),
            (208, 238, 4),
        ];
        return zones
            .iter()
            .find(|&&(a, b, _)| (cy0 + a..cy0 + b).contains(&py))
            .map(|&(_, _, v)| FilesHit::View(v));
    }
    if view <= 1 {
        let list_y = cy0 + FILES_LIST_TOP;
        if py >= list_y {
            return Some(FilesHit::Row(((py - list_y) / FILES_ROW_H) as usize));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SW: i32 = 1280;
    const SH: i32 = 800;

    // ---- work area & fit ----

    #[test]
    fn work_area_sits_between_the_panel_and_the_taskbar() {
        let w = work_area(SW, SH);
        let bar = crate::windowing::taskbar::layout(SW, SH, 9).panel;
        assert_eq!(w.y, crate::ui::style::PANEL_H);
        assert_eq!(w.bottom() + WORK_DOCK_GAP, bar.y);
        assert_eq!((w.x, w.right()), (0, SW));
        // BIOS 1280x720 too.
        let b = work_area(1280, 720);
        assert!(b.h > 400);
    }

    #[test]
    fn work_area_on_a_tiny_screen_is_not_negative() {
        let w = work_area(100, 60);
        assert!(w.w >= 0 && w.h >= 0);
    }

    #[test]
    fn fit_scale_picks_the_largest_that_fits() {
        // Terminal-like grid: 240 x 135 at scale 1.
        assert_eq!(fit_scale(488, 282, 240, 135, 2, 4), 2);
        assert_eq!(fit_scale(740, 420, 240, 135, 2, 4), 3);
        assert_eq!(fit_scale(2000, 2000, 240, 135, 2, 4), 4); // capped
        // Width limits even when height would allow more.
        assert_eq!(fit_scale(500, 2000, 240, 135, 2, 4), 2);
    }

    #[test]
    fn fit_scale_falls_back_to_base_when_nothing_fits() {
        assert_eq!(fit_scale(10, 10, 240, 135, 2, 4), 2);
        assert_eq!(fit_scale(0, 0, 240, 135, 2, 4), 2);
    }

    // ---- context menu ----

    // ---- dock ----

    // ---- calculator ----

    #[test]
    fn calc_keypad_maps_every_cell() {
        let r = Rect::new(100, 100, 320, 520);
        let g = calc_geom(r);
        for (row, cells) in g.keys.iter().enumerate() {
            for (col, cell) in cells.iter().enumerate() {
                assert!(cell.w > 0 && cell.h > 0, "row {row} col {col}");
                assert_eq!(
                    calc_button_at(r, cell.x + cell.w / 2, cell.y + cell.h / 2),
                    Some(CALC_KEYS[row][col]),
                    "row {row} col {col}"
                );
            }
        }
    }

    #[test]
    fn calc_gaps_and_outside_are_misses() {
        let r = Rect::new(100, 100, 320, 520);
        let g = calc_geom(r);
        let k = g.keys[2][1];
        assert_eq!(calc_button_at(r, k.right(), k.y), None); // horizontal gap
        assert_eq!(calc_button_at(r, k.x, k.bottom()), None); // vertical gap
        assert_eq!(calc_button_at(r, g.keys[2][0].x - 1, k.y), None);
        assert_eq!(calc_button_at(r, r.x + 2, r.bottom() - 2), None);
    }

    #[test]
    fn calc_regions_stack_inside_the_window() {
        let r = Rect::new(0, 0, 320, 520);
        let g = calc_geom(r);
        assert!(g.history.y >= TITLE_H);
        assert_eq!(g.display.y, g.history.bottom());
        assert!(g.keys[0][0].y > g.display.bottom());
        assert!(g.keys[5][3].bottom() <= r.bottom());
        // The memory row is the short one and the rest are equal.
        assert_eq!(g.keys[0][0].h, CALC_MEM_H);
        assert!(g.keys[1][0].h > CALC_MEM_H);
        assert!(g.keys[1..].iter().all(|row| row[0].h == g.keys[1][0].h));
        // Copy sits in the history strip; clicking the display copies too.
        assert_eq!(calc_hit(r, g.copy.x + 2, g.copy.y + 2), Some(CalcHit::Copy));
        assert_eq!(
            calc_hit(r, g.display.x + 10, g.display.y + 10),
            Some(CalcHit::Copy)
        );
    }

    #[test]
    fn calc_operators_are_in_the_last_column() {
        for (row, expect) in [(1, b'/'), (2, b'*'), (3, b'-'), (4, b'+'), (5, b'=')] {
            assert_eq!(CALC_KEYS[row][3], expect);
        }
    }

    #[test]
    fn calc_degenerate_window_has_no_buttons() {
        let tiny = Rect::new(0, 0, 40, 40);
        assert_eq!(calc_button_at(tiny, 20, 20), None);
        let negative = Rect::new(0, 0, 10, 10);
        assert_eq!(calc_button_at(negative, 5, 5), None);
        // Even a tiny window never yields a negative-sized key.
        let g = calc_geom(tiny);
        assert!(g.keys.iter().flatten().all(|k| k.w >= 0 && k.h >= 0));
    }

    // ---- browser ----

    #[test]
    fn browser_chrome_toolbar_is_ordered_and_inside_the_window() {
        let r = Rect::new(80, 60, 700, 500);
        let c = BrowserChrome::of(r, 1, 0);
        assert!(c.back.right() <= c.forward.x);
        assert!(c.forward.right() < c.reload.x);
        assert!(c.reload.right() < c.bar.x);
        assert!(c.bar.right() < c.newtab.x);
        assert!(c.star.x >= c.bar.x && c.star.right() <= c.bar.right());
        assert_eq!(c.newtab.right(), r.right() - 12);
        assert_eq!(c.toolbar.y, r.y + TITLE_H);
        assert_eq!(c.toolbar.h, BROWSER_TOOLBAR_H);
        // Every control is vertically centred in the toolbar and on the 4 px grid.
        for b in [c.back, c.forward, c.reload, c.bar, c.newtab] {
            assert_eq!(b.y - c.toolbar.y, (BROWSER_TOOLBAR_H - b.h) / 2);
            assert_eq!(b.h % 4, 0);
        }
        // The page is edge to edge below the chrome.
        assert_eq!(c.content.x, r.x);
        assert_eq!(c.content.w, r.w);
        assert_eq!(c.content.y, c.toolbar.bottom());
        assert_eq!(c.content.bottom(), r.bottom());
        assert!(c.progress.y > c.bar.bottom() && c.progress.bottom() <= c.toolbar.bottom());
    }

    #[test]
    fn the_tab_strip_appears_from_two_tabs_and_pushes_the_page_down() {
        let r = Rect::new(0, 0, 700, 500);
        let one = BrowserChrome::of(r, 1, 0);
        let two = BrowserChrome::of(r, 2, 0);
        assert_eq!(one.strip.h, 0);
        assert_eq!(two.strip.h, BROWSER_STRIP_H);
        assert_eq!(two.content.y, one.content.y + BROWSER_STRIP_H);
        assert_eq!(two.strip.y, two.toolbar.bottom());
    }

    #[test]
    fn the_shield_takes_room_from_the_address_text() {
        let r = Rect::new(0, 0, 700, 500);
        let none = BrowserChrome::of(r, 1, 0);
        let some = BrowserChrome::of(r, 1, 120);
        assert_eq!(none.shield.w, 0);
        assert_eq!(some.shield.w, 120);
        assert!(some.text_x() > none.text_x());
        assert!(some.shield.x >= some.bar.x && some.shield.right() <= some.bar.right());
        // Never wider than half the omnibox.
        let wide = BrowserChrome::of(Rect::new(0, 0, 400, 300), 1, 5000);
        assert!(wide.shield.w <= wide.bar.w / 2);
    }

    #[test]
    fn tab_rects_share_the_strip_and_animate_with_their_weights() {
        let strip = Rect::new(0, 100, 700, 36);
        let full = browser_tab_rects(strip, &[256, 256, 256]);
        assert_eq!(full.len(), 3);
        for r in &full {
            assert_eq!(r.h, BROWSER_TAB_H);
            assert!(r.w >= BROWSER_TAB_MIN_W && r.w <= BROWSER_TAB_MAX_W);
            assert_eq!(r.y, strip.y + 4);
        }
        assert_eq!(full[1].x - full[0].right(), 4);
        // Eight tabs in a narrow strip bottom out at the minimum.
        let tiny = browser_tab_rects(Rect::new(0, 0, 300, 36), &[256; 8]);
        assert!(tiny.iter().all(|r| r.w == BROWSER_TAB_MIN_W));
        // A tab that is opening is narrower and the others close ranks around it.
        let half = browser_tab_rects(strip, &[256, 128, 256]);
        assert!(half[1].w < full[1].w);
        assert!(half[1].w > 0);
        let zero = browser_tab_rects(strip, &[256, 0, 256]);
        assert_eq!(zero[1].w, 0);
        assert_eq!(zero[2].x, zero[0].right() + 4);
        assert!(browser_tab_rects(strip, &[]).is_empty());
        // Two tabs in a wide strip stop at the maximum width.
        let wide = browser_tab_rects(Rect::new(0, 0, 1280, 36), &[256, 256]);
        assert!(wide.iter().all(|r| r.w == BROWSER_TAB_MAX_W));
    }

    #[test]
    fn tab_hit_testing_and_close_button() {
        let rects = browser_tab_rects(Rect::new(0, 0, 700, 36), &[256, 256, 0]);
        assert_eq!(
            browser_tab_at(&rects, rects[1].x + 5, rects[1].y + 5),
            Some(1)
        );
        assert_eq!(browser_tab_at(&rects, 2, 2), None);
        // A closed (zero width) tab cannot be hit.
        assert_eq!(browser_tab_at(&rects, rects[2].x, rects[2].y + 5), None);
        let x = browser_tab_close(rects[0]);
        assert!(x.x >= rects[0].x && x.right() <= rects[0].right());
        assert!(x.y >= rects[0].y && x.bottom() <= rects[0].bottom());
    }

    #[test]
    fn suggestion_rows_stack_under_the_bar() {
        let bar = Rect::new(100, 50, 400, 32);
        let panel = browser_suggest_panel(bar, 3);
        let r0 = browser_suggestion_row(bar, 0);
        let r1 = browser_suggestion_row(bar, 1);
        assert!(panel.x == bar.x && panel.w == bar.w && panel.y > bar.bottom());
        assert!(r0.x > panel.x && r0.right() < panel.right());
        assert!(r0.y > panel.y);
        assert_eq!(r1.y - r0.y, BROWSER_SUGGEST_ROW);
        assert!(browser_suggestion_row(bar, 2).bottom() < panel.bottom());
        assert_eq!(browser_suggestion_at(bar, 3, 120, r1.y + 3), Some(1));
        assert_eq!(browser_suggestion_at(bar, 1, 120, r1.y + 3), None);
        assert_eq!(browser_suggestion_at(bar, 3, 5, r0.y), None);
    }

    #[test]
    fn popover_stays_inside_the_window() {
        let win = Rect::new(100, 100, 500, 400);
        let bar = Rect::new(380, 150, 200, 32);
        let p = browser_popover(bar, win, 200);
        assert!(p.x >= win.x && p.right() <= win.right());
        assert_eq!(p.y, bar.bottom() + 8);
        let narrow = browser_popover(bar, Rect::new(0, 0, 200, 300), 100);
        assert!(narrow.right() <= 200);
    }

    #[test]
    fn find_bar_sits_in_the_top_right_and_its_parts_fit() {
        let content = Rect::new(0, 80, 700, 400);
        let f = browser_find_layout(content);
        assert!(f.bar.x >= content.x && f.bar.right() <= content.right());
        assert_eq!(f.bar.y, content.y + 12);
        for r in [f.field, f.count, f.prev, f.next, f.close] {
            assert!(r.x >= f.bar.x && r.right() <= f.bar.right(), "{r:?}");
            assert!(r.y >= f.bar.y && r.bottom() <= f.bar.bottom());
        }
        assert!(f.field.right() <= f.count.x);
        assert!(f.count.right() <= f.prev.x);
        assert!(f.prev.right() <= f.next.x && f.next.right() <= f.close.x);
        // Tiny windows do not push it out to the left.
        let t = browser_find_layout(Rect::new(0, 0, 150, 100));
        assert!(t.bar.x >= 0);
    }

    #[test]
    fn error_page_parts_are_stacked_and_inside_the_page() {
        let content = Rect::new(0, 80, 800, 480);
        for cert in [false, true] {
            let e = browser_error_layout(content, cert);
            for r in [e.art, e.title, e.cause, e.retry] {
                assert!(r.x >= content.x && r.right() <= content.right(), "{r:?}");
                assert!(r.y >= content.y && r.bottom() <= content.bottom(), "{r:?}");
            }
            assert!(e.art.bottom() < e.title.y && e.title.bottom() <= e.cause.y);
            assert!(e.cause.bottom() < e.retry.y);
            assert_eq!(e.art.x + e.art.w / 2, content.x + content.w / 2);
            assert_eq!(e.retry.x + e.retry.w / 2, content.x + content.w / 2);
            if cert {
                assert!(e.proceed.y > e.retry.bottom());
                assert!(e.note.bottom() <= content.bottom());
            }
        }
    }

    #[test]
    fn start_page_has_a_search_field_tiles_and_recents_that_fit() {
        let content = Rect::new(0, 80, 900, 480);
        let s = browser_start_layout(content, 8, 6);
        assert_eq!(s.search.x + s.search.w / 2, content.x + content.w / 2);
        assert!(s.search.h == 48);
        assert!(!s.tiles.is_empty() && s.tiles.len() <= 8);
        for t in s.tiles.iter().chain(&s.recents) {
            assert!(t.x >= content.x && t.right() <= content.right());
            assert!(t.y >= content.y && t.bottom() <= content.bottom(), "{t:?}");
        }
        // Tiles are in a grid: the second is right of the first, 16 px apart.
        assert_eq!(s.tiles[1].x - s.tiles[0].right(), 16);
        assert!(s.tiles_heading.bottom() <= s.tiles[0].y);
        assert!(s.recent_heading.y >= s.tiles.last().unwrap().bottom());
        // A short window drops what does not fit.
        let short = browser_start_layout(Rect::new(0, 0, 900, 260), 8, 6);
        assert!(short.tiles.len() < s.tiles.len() || short.recents.len() < s.recents.len());
        for t in short.tiles.iter().chain(&short.recents) {
            assert!(t.bottom() <= 260);
        }
        // Zero items, narrow window: still sane.
        let n = browser_start_layout(Rect::new(0, 0, 200, 300), 0, 0);
        assert!(n.tiles.is_empty() && n.recents.is_empty());
        assert!(n.search.x >= 0 && n.search.right() <= 200);
    }

    #[test]
    fn zoom_pill_is_at_the_bottom_right() {
        let content = Rect::new(0, 80, 700, 400);
        let z = browser_zoom_pill(content);
        assert!(z.right() < content.right() && z.bottom() < content.bottom());
    }

    #[test]
    fn browser_chrome_survives_tiny_windows() {
        let c = BrowserChrome::of(Rect::new(0, 0, 50, 50), 2, 80);
        assert_eq!(c.bar.w, 120); // the omnibox never collapses below 120
        assert_eq!(c.content.h, 0); // negative height clamps to empty
    }

    // ---- start panel ----

    // ---- file manager ----

    #[test]
    fn files_sidebar_zones() {
        let r = Rect::new(50, 50, 600, 400);
        let cy0 = r.y + TITLE_H;
        let x = r.x + 10;
        assert_eq!(files_hit(r, 0, x, cy0 + 34), Some(FilesHit::View(0)));
        assert_eq!(files_hit(r, 0, x, cy0 + 63), Some(FilesHit::View(0)));
        assert_eq!(files_hit(r, 0, x, cy0 + 64), None);
        assert_eq!(files_hit(r, 0, x, cy0 + 66), Some(FilesHit::View(1)));
        assert_eq!(files_hit(r, 0, x, cy0 + 132), Some(FilesHit::View(2)));
        assert_eq!(files_hit(r, 0, x, cy0 + 164), Some(FilesHit::View(3)));
        assert_eq!(files_hit(r, 0, x, cy0 + 194), None);
        assert_eq!(files_hit(r, 0, x, cy0 + 208), Some(FilesHit::View(4)));
        assert_eq!(files_hit(r, 0, x, cy0 + 237), Some(FilesHit::View(4)));
        assert_eq!(files_hit(r, 0, x, cy0 + 238), None);
        assert_eq!(files_hit(r, 0, x, cy0 + 10), None);
    }

    #[test]
    fn files_rows_only_in_file_and_trash_views() {
        let r = Rect::new(50, 50, 600, 400);
        let list_y = r.y + TITLE_H + 78;
        let x = r.x + FILES_SIDEBAR_W + 5;
        assert_eq!(files_hit(r, 0, x, list_y), Some(FilesHit::Row(0)));
        assert_eq!(files_hit(r, 0, x, list_y + 29), Some(FilesHit::Row(0)));
        assert_eq!(files_hit(r, 1, x, list_y + 30), Some(FilesHit::Row(1)));
        assert_eq!(files_hit(r, 0, x, list_y + 95), Some(FilesHit::Row(3)));
        assert_eq!(files_hit(r, 0, x, list_y - 1), None); // header area
        assert_eq!(files_hit(r, 2, x, list_y), None); // disk view has no rows
        assert_eq!(files_hit(r, 3, x, list_y), None);
        assert_eq!(files_hit(r, 4, x, list_y), None); // the Apps view has its own rows
    }

    #[test]
    fn files_sidebar_edge_belongs_to_the_main_list() {
        let r = Rect::new(0, 0, 600, 400);
        let list_y = TITLE_H + 78;
        assert_eq!(
            files_hit(r, 0, FILES_SIDEBAR_W - 1, TITLE_H + 40),
            Some(FilesHit::View(0))
        );
        assert_eq!(
            files_hit(r, 0, FILES_SIDEBAR_W, list_y),
            Some(FilesHit::Row(0))
        );
    }
}
