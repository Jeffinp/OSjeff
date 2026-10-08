//! Desktop geometry: dock, context menu, start panel, calculator keypad,
//! browser chrome and file-manager hit zones.
//!
//! Pure integer layout shared by the kernel's drawing code and its hit-testing,
//! so the two can never disagree. Nothing here touches pixels.

use crate::window::{Rect, TITLE_H};

/// Calculator keypad: the input byte for each cell (`0x08` = backspace).
/// Duplicate cells (`0` spanning two columns, `=` spanning two rows) map to the
/// same byte; the draw code merges them visually.
pub const CALC_KEYS: [[u8; 4]; 5] = [*b"C\x08/*", *b"789-", *b"456+", *b"123=", *b"00.="];

// ------------------------------------------------------------------ calculator

/// Keypad geometry for a calculator window: grid origin x/y, cell width/height
/// and gap. Shared by drawing and hit-testing so they always agree.
pub fn calc_layout(r: Rect) -> (i32, i32, i32, i32, i32) {
    let pad = 14;
    let gap = 8;
    let disp_h = 48;
    let gx = r.x + pad;
    let gy = r.y + TITLE_H + 12 + disp_h + 12;
    let grid_w = r.w - pad * 2;
    let grid_h = r.bottom() - gy - pad;
    let cw = (grid_w - gap * 3) / 4;
    let ch = (grid_h - gap * 4) / 5;
    (gx, gy, cw, ch, gap)
}

/// The keypad byte under `(px, py)` in calculator window `r`, if any. Spanning
/// buttons map through their duplicate cells in [`CALC_KEYS`].
pub fn calc_button_at(r: Rect, px: i32, py: i32) -> Option<u8> {
    let (gx, gy, cw, ch, gap) = calc_layout(r);
    if cw <= 0 || ch <= 0 {
        return None;
    }
    for (row, keys) in CALC_KEYS.iter().enumerate() {
        for (col, &k) in keys.iter().enumerate() {
            let bx = gx + col as i32 * (cw + gap);
            let by = gy + row as i32 * (ch + gap);
            if px >= bx && px < bx + cw && py >= by && py < by + ch {
                return Some(k);
            }
        }
    }
    None
}

// --------------------------------------------------------------------- browser

/// Browser chrome geometry, shared by drawing and hit-testing so the toolbar
/// buttons, address bar and content area always agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrowserChrome {
    pub back: Rect,
    pub forward: Rect,
    pub reload: Rect,
    pub home: Rect,
    pub go: Rect,
    pub bar: Rect,
    /// The favourite star, inside the right end of the address bar.
    pub star: Rect,
    pub content: Rect,
}

impl BrowserChrome {
    pub fn of(r: Rect) -> Self {
        let pad = 14;
        let btn = 36;
        let gap = 8;
        let ty = r.y + TITLE_H + 12;
        let back = Rect::new(r.x + pad, ty, btn, btn);
        let forward = Rect::new(back.right() + gap, ty, btn, btn);
        let reload = Rect::new(forward.right() + gap, ty, btn, btn);
        let home = Rect::new(reload.right() + gap, ty, btn, btn);
        let go = Rect::new(r.right() - pad - btn, ty, btn, btn);
        let bar_x = home.right() + gap;
        let bar = Rect::new(bar_x, ty, (go.x - gap - bar_x).max(60), btn);
        let star = Rect::new(bar.right() - 34, ty + 6, 24, 24);
        let cy = ty + btn + 16;
        let content = Rect::new(r.x + pad, cy, r.w - pad * 2, (r.bottom() - 14 - cy).max(0));
        Self {
            back,
            forward,
            reload,
            home,
            go,
            bar,
            star,
            content,
        }
    }
}

/// Rect of suggestion row `i` under the address bar (rows are 26 px tall).
pub fn browser_suggestion_row(bar: Rect, i: usize) -> Rect {
    Rect::new(bar.x, bar.bottom() + 2 + i as i32 * 26, bar.w, 26)
}

/// Row of the suggestion list under page-space point `(px, py)` when `n` rows are shown.
pub fn browser_suggestion_at(bar: Rect, n: usize, px: i32, py: i32) -> Option<usize> {
    (0..n).find(|&i| browser_suggestion_row(bar, i).contains(px, py))
}

/// Rect of the error page's "continue anyway (insecure)" button, offered only for
/// certificate errors. Inside the content box, below the message lines.
pub fn browser_continue_button(content: Rect) -> Rect {
    Rect::new(
        content.x + 8,
        content.y + 104,
        (content.w - 16).clamp(0, 420),
        34,
    )
}

/// Start-page layout: the brand logo rect and the four shortcut-tile rects,
/// centered in the content box.
pub fn browser_home_layout(content: Rect) -> (Rect, [Rect; 4]) {
    let cx = content.x + content.w / 2;
    let logo_sz = 84;
    let logo = Rect::new(cx - logo_sz / 2, content.y + 30, logo_sz, logo_sz);
    let tile_w = 150;
    let tile_h = 96;
    let gap = 18;
    let total = 4 * tile_w + 3 * gap;
    let sx = cx - total / 2;
    let ty = logo.bottom() + 108;
    let mut tiles = [Rect::new(0, 0, 0, 0); 4];
    for (i, t) in tiles.iter_mut().enumerate() {
        *t = Rect::new(sx + i as i32 * (tile_w + gap), ty, tile_w, tile_h);
    }
    (logo, tiles)
}

// ------------------------------------------------------------- work area & fit

/// Gap kept between the work area and the floating dock panel.
pub const WORK_DOCK_GAP: i32 = 8;

/// The rectangle windows maximize into: the screen below the menu bar, edge to
/// edge, down to the dock panel (plus a small gap).
pub fn work_area(sw: i32, sh: i32) -> Rect {
    let dock_top = sh - crate::chrome::DOCK_BOTTOM - crate::chrome::DOCK_H;
    let top = crate::window::MENUBAR_H;
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
    fn work_area_sits_between_the_menu_bar_and_the_dock() {
        let w = work_area(SW, SH);
        let (dock, _) = crate::chrome::dock_rest(SW, SH, 9, None);
        assert_eq!(w.y, crate::window::MENUBAR_H);
        assert!(w.bottom() + WORK_DOCK_GAP <= dock.y);
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
        let r = Rect::new(100, 100, 320, 420);
        let (gx, gy, cw, ch, gap) = calc_layout(r);
        assert!(cw > 0 && ch > 0);
        for (row, keys) in CALC_KEYS.iter().enumerate() {
            for (col, &k) in keys.iter().enumerate() {
                let px = gx + col as i32 * (cw + gap) + cw / 2;
                let py = gy + row as i32 * (ch + gap) + ch / 2;
                assert_eq!(calc_button_at(r, px, py), Some(k), "row {row} col {col}");
            }
        }
    }

    #[test]
    fn calc_gaps_and_outside_are_misses() {
        let r = Rect::new(100, 100, 320, 420);
        let (gx, gy, cw, ch, gap) = calc_layout(r);
        assert_eq!(calc_button_at(r, gx + cw, gy), None); // horizontal gap
        assert_eq!(calc_button_at(r, gx, gy + ch), None); // vertical gap
        assert_eq!(calc_button_at(r, gx - 1, gy), None);
        assert_eq!(calc_button_at(r, gx, gy - 1), None);
        assert_eq!(calc_button_at(r, gx + 4 * (cw + gap), gy), None);
        assert_eq!(calc_button_at(r, gx, gy + 5 * (ch + gap)), None);
    }

    #[test]
    fn calc_spanning_buttons_share_a_byte() {
        let r = Rect::new(0, 0, 320, 420);
        let (gx, gy, cw, ch, gap) = calc_layout(r);
        let at = |row: i32, col: i32| {
            calc_button_at(r, gx + col * (cw + gap) + 1, gy + row * (ch + gap) + 1)
        };
        assert_eq!(at(4, 0), at(4, 1)); // '0' spans two columns
        assert_eq!(at(3, 3), at(4, 3)); // '=' spans two rows
        assert_eq!(at(3, 3), Some(b'='));
    }

    #[test]
    fn calc_degenerate_window_has_no_buttons() {
        let tiny = Rect::new(0, 0, 40, 40);
        assert_eq!(calc_button_at(tiny, 20, 20), None);
        let negative = Rect::new(0, 0, 10, 10);
        assert_eq!(calc_button_at(negative, 5, 5), None);
    }

    // ---- browser ----

    #[test]
    fn browser_chrome_toolbar_is_ordered_and_inside_the_window() {
        let r = Rect::new(80, 60, 700, 500);
        let c = BrowserChrome::of(r);
        assert!(c.back.right() < c.forward.x);
        assert!(c.forward.right() < c.reload.x);
        assert!(c.reload.right() < c.home.x);
        assert!(c.home.right() < c.bar.x);
        assert!(c.bar.right() < c.go.x);
        assert!(c.star.x >= c.bar.x && c.star.right() <= c.bar.right());
        assert_eq!(c.go.right(), r.right() - 14);
        assert_eq!(c.back.y, r.y + TITLE_H + 12);
        assert!(c.content.y > c.home.bottom());
        assert_eq!(c.content.bottom(), r.bottom() - 14);
    }

    #[test]
    fn suggestion_rows_stack_under_the_bar() {
        let bar = Rect::new(100, 50, 400, 36);
        let r0 = browser_suggestion_row(bar, 0);
        let r1 = browser_suggestion_row(bar, 1);
        assert_eq!((r0.x, r0.w), (bar.x, bar.w));
        assert!(r0.y > bar.bottom());
        assert_eq!(r1.y - r0.y, 26);
        assert_eq!(browser_suggestion_at(bar, 3, 120, r1.y + 3), Some(1));
        assert_eq!(browser_suggestion_at(bar, 1, 120, r1.y + 3), None);
        assert_eq!(browser_suggestion_at(bar, 3, 5, r0.y), None);
    }

    #[test]
    fn continue_button_is_inside_the_content_box() {
        let content = Rect::new(100, 200, 600, 300);
        let b = browser_continue_button(content);
        assert!(b.x >= content.x && b.right() <= content.right());
        assert!(b.y > content.y + 60 && b.bottom() <= content.bottom());
        assert_eq!(b.w, 420);
        // Narrow windows shrink it instead of overflowing.
        let narrow = browser_continue_button(Rect::new(0, 0, 200, 300));
        assert_eq!(narrow.w, 184);
    }

    #[test]
    fn browser_chrome_survives_tiny_windows() {
        let c = BrowserChrome::of(Rect::new(0, 0, 50, 50));
        assert_eq!(c.bar.w, 60); // address bar never collapses below 60
        assert_eq!(c.content.h, 0); // negative height clamps to empty
    }

    #[test]
    fn browser_home_tiles_are_centered_and_disjoint() {
        let content = Rect::new(100, 200, 600, 300);
        let (logo, tiles) = browser_home_layout(content);
        assert_eq!(logo.x + logo.w / 2, content.x + content.w / 2);
        assert_eq!(logo.y, content.y + 30);
        for pair in tiles.windows(2) {
            assert_eq!(pair[1].x - pair[0].right(), 18);
        }
        let left_margin = tiles[0].x - content.x;
        let right_margin = content.right() - tiles[3].right();
        assert!((left_margin - right_margin).abs() <= 1);
        assert!(tiles.iter().all(|t| t.y == logo.bottom() + 108));
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
