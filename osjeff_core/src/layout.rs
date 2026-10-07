//! Desktop geometry: dock, context menu, start panel, calculator keypad,
//! browser chrome and file-manager hit zones.
//!
//! Pure integer layout shared by the kernel's drawing code and its hit-testing,
//! so the two can never disagree. Nothing here touches pixels.

use crate::window::{Rect, TITLE_H};

// ---- floating dock ----
pub const DOCK_ICON: i32 = 40;
pub const DOCK_GAP: i32 = 14;
pub const DOCK_PAD: i32 = 12;
/// brand + terminal + editor + taskmgr + calculator + browser + wasm + files
pub const DOCK_COUNT: i32 = 8;
/// Gap from the screen bottom.
pub const DOCK_MARGIN: i32 = 16;

// ---- right-click context menu ----
pub const MENU_W: i32 = 220;
pub const MENU_ITEM_H: i32 = 32;
pub const MENU_PAD: i32 = 6;

// ---- start panel ----
pub const START_W: i32 = 240;
pub const START_ROW_H: i32 = 38;
pub const START_PAD: i32 = 10;
/// Divider gap before the power rows.
pub const START_GAP: i32 = 12;
/// Rows after the app list: reboot + shutdown.
const START_POWER_ROWS: i32 = 2;

/// Calculator keypad: the input byte for each cell (`0x08` = backspace).
/// Duplicate cells (`0` spanning two columns, `=` spanning two rows) map to the
/// same byte; the draw code merges them visually.
pub const CALC_KEYS: [[u8; 4]; 5] = [*b"C\x08/*", *b"789-", *b"456+", *b"123=", *b"00.="];

// ---------------------------------------------------------------- context menu

/// Total height of a context menu with `items` entries.
pub fn menu_height(items: usize) -> i32 {
    MENU_PAD * 2 + items as i32 * MENU_ITEM_H
}

/// Top-left of a menu opened at `(x, y)`, shifted so it stays on a `sw x sh`
/// screen (the top-left corner wins when the screen is smaller than the menu).
pub fn clamp_menu(sw: i32, sh: i32, x: i32, y: i32, items: usize) -> (i32, i32) {
    let mx = x.min(sw - MENU_W).max(0);
    let my = y.min(sh - menu_height(items)).max(0);
    (mx, my)
}

/// Index of the context-menu item under `(px, py)` for a menu at `(mx, my)`
/// with `items` entries.
pub fn menu_item_at(mx: i32, my: i32, px: i32, py: i32, items: usize) -> Option<usize> {
    if px < mx + MENU_PAD || px >= mx + MENU_W - MENU_PAD {
        return None;
    }
    let rel = py - (my + MENU_PAD);
    if rel < 0 {
        return None;
    }
    let i = (rel / MENU_ITEM_H) as usize;
    (i < items).then_some(i)
}

// ------------------------------------------------------------------------ dock

/// The floating dock panel rect and its `DOCK_COUNT` icon slots.
pub fn dock_layout(sw: i32, sh: i32) -> (Rect, [Rect; DOCK_COUNT as usize]) {
    let inner = DOCK_COUNT * DOCK_ICON + (DOCK_COUNT - 1) * DOCK_GAP;
    let dock_w = inner + DOCK_PAD * 2;
    let dock_h = DOCK_ICON + DOCK_PAD * 2;
    let dock_x = sw / 2 - dock_w / 2;
    let dock_y = sh - dock_h - DOCK_MARGIN;
    let dock = Rect::new(dock_x, dock_y, dock_w, dock_h);

    let mut icons = [Rect::new(0, 0, DOCK_ICON, DOCK_ICON); DOCK_COUNT as usize];
    let mut x = dock_x + DOCK_PAD;
    for slot in icons.iter_mut() {
        *slot = Rect::new(x, dock_y + DOCK_PAD, DOCK_ICON, DOCK_ICON);
        x += DOCK_ICON + DOCK_GAP;
    }
    (dock, icons)
}

/// Index of the dock icon under `(px, py)`, if any (slot 0 is the brand icon).
pub fn dock_slot_at(sw: i32, sh: i32, px: i32, py: i32) -> Option<usize> {
    let (_, icons) = dock_layout(sw, sh);
    icons.iter().position(|r| r.contains(px, py))
}

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
    pub home: Rect,
    pub reload: Rect,
    pub go: Rect,
    pub bar: Rect,
    pub content: Rect,
}

impl BrowserChrome {
    pub fn of(r: Rect) -> Self {
        let pad = 14;
        let btn = 36;
        let gap = 8;
        let ty = r.y + TITLE_H + 12;
        let home = Rect::new(r.x + pad, ty, btn, btn);
        let reload = Rect::new(home.right() + gap, ty, btn, btn);
        let go = Rect::new(r.right() - pad - btn, ty, btn, btn);
        let bar_x = reload.right() + gap;
        let bar = Rect::new(bar_x, ty, (go.x - gap - bar_x).max(60), btn);
        let cy = ty + btn + 16;
        let content = Rect::new(r.x + pad, cy, r.w - pad * 2, (r.bottom() - 14 - cy).max(0));
        Self {
            home,
            reload,
            go,
            bar,
            content,
        }
    }
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

// ----------------------------------------------------------------- start panel

/// Height of the start panel listing `apps` applications plus the power rows.
pub fn start_height(apps: usize) -> i32 {
    START_PAD * 2 + (apps as i32 + START_POWER_ROWS) * START_ROW_H + START_GAP
}

/// Top-left of the start panel, centered above the dock's system icon and
/// clamped to the screen.
pub fn start_origin(sw: i32, sh: i32, apps: usize) -> (i32, i32) {
    let (_dock, icons) = dock_layout(sw, sh);
    let brand = icons[0];
    let x = (brand.x + DOCK_ICON / 2 - START_W / 2).clamp(8, (sw - START_W - 8).max(8));
    let y = brand.y - start_height(apps) - 12;
    (x, y)
}

/// An entry in the start panel; `App` carries the row index in the app list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartHit {
    App(usize),
    Reboot,
    Shutdown,
}

/// The start-panel item under `(px, py)` for a panel listing `apps` apps.
pub fn start_item_at(sw: i32, sh: i32, apps: usize, px: i32, py: i32) -> Option<StartHit> {
    let (sx, sy) = start_origin(sw, sh, apps);
    if px < sx + START_PAD || px >= sx + START_W - START_PAD {
        return None;
    }
    let top = sy + START_PAD;
    for i in 0..apps {
        let ry = top + i as i32 * START_ROW_H;
        if py >= ry && py < ry + START_ROW_H {
            return Some(StartHit::App(i));
        }
    }
    let pwr_top = top + apps as i32 * START_ROW_H + START_GAP;
    for (i, item) in [StartHit::Reboot, StartHit::Shutdown].iter().enumerate() {
        let ry = pwr_top + i as i32 * START_ROW_H;
        if py >= ry && py < ry + START_ROW_H {
            return Some(*item);
        }
    }
    None
}

// ------------------------------------------------------------- work area & fit

/// Bottom of the perf HUD (top-right corner, 12 px margin + 60 px panel) plus a
/// little air. Maximized windows start below it so their title-bar buttons are
/// never hidden under the HUD.
pub const WORK_TOP: i32 = 76;
/// Side margin of the work area.
pub const WORK_SIDE: i32 = 12;
/// Gap kept between the work area and the floating dock.
pub const WORK_DOCK_GAP: i32 = 12;

/// The rectangle windows maximize into: the screen minus the HUD band on top,
/// a thin side margin and the floating dock at the bottom.
pub fn work_area(sw: i32, sh: i32) -> Rect {
    let (dock, _) = dock_layout(sw, sh);
    let bottom = dock.y - WORK_DOCK_GAP;
    Rect::new(
        WORK_SIDE,
        WORK_TOP,
        (sw - 2 * WORK_SIDE).max(0),
        (bottom - WORK_TOP).max(0),
    )
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
        let zones = [(34, 64, 0u8), (66, 96, 1), (132, 162, 2), (164, 194, 3)];
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
    fn work_area_clears_the_hud_and_the_dock() {
        let w = work_area(SW, SH);
        let (dock, _) = dock_layout(SW, SH);
        assert_eq!(w.y, WORK_TOP);
        assert!(w.bottom() + WORK_DOCK_GAP <= dock.y);
        assert_eq!((w.x, w.right()), (WORK_SIDE, SW - WORK_SIDE));
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

    #[test]
    fn menu_height_counts_padding_and_rows() {
        assert_eq!(menu_height(0), 12);
        assert_eq!(menu_height(7), 12 + 7 * 32);
    }

    #[test]
    fn clamp_menu_keeps_menu_on_screen() {
        assert_eq!(clamp_menu(SW, SH, 100, 100, 7), (100, 100));
        assert_eq!(
            clamp_menu(SW, SH, SW - 1, SH - 1, 7),
            (SW - MENU_W, SH - menu_height(7))
        );
        assert_eq!(clamp_menu(SW, SH, -50, -50, 7), (0, 0));
    }

    #[test]
    fn clamp_menu_on_tiny_screen_pins_to_origin() {
        assert_eq!(clamp_menu(100, 100, 50, 50, 7), (0, 0));
    }

    #[test]
    fn menu_item_hit_rows_and_edges() {
        let (mx, my) = (100, 100);
        let first_y = my + MENU_PAD;
        assert_eq!(menu_item_at(mx, my, mx + MENU_PAD, first_y, 7), Some(0));
        assert_eq!(
            menu_item_at(mx, my, mx + 50, first_y + MENU_ITEM_H - 1, 7),
            Some(0)
        );
        assert_eq!(
            menu_item_at(mx, my, mx + 50, first_y + MENU_ITEM_H, 7),
            Some(1)
        );
        assert_eq!(
            menu_item_at(mx, my, mx + 50, first_y + 6 * MENU_ITEM_H, 7),
            Some(6)
        );
        // Past the last item, above the first, and in the side padding: miss.
        assert_eq!(
            menu_item_at(mx, my, mx + 50, first_y + 7 * MENU_ITEM_H, 7),
            None
        );
        assert_eq!(menu_item_at(mx, my, mx + 50, first_y - 1, 7), None);
        assert_eq!(menu_item_at(mx, my, mx + MENU_PAD - 1, first_y, 7), None);
        assert_eq!(
            menu_item_at(mx, my, mx + MENU_W - MENU_PAD, first_y, 7),
            None
        );
        assert_eq!(
            menu_item_at(mx, my, mx + MENU_W - MENU_PAD - 1, first_y, 7),
            Some(0)
        );
    }

    #[test]
    fn menu_with_no_items_never_hits() {
        assert_eq!(menu_item_at(0, 0, 50, 10, 0), None);
    }

    // ---- dock ----

    #[test]
    fn dock_is_centered_above_the_bottom_margin() {
        let (dock, icons) = dock_layout(SW, SH);
        assert_eq!(dock.w, 8 * 40 + 7 * 14 + 24);
        assert_eq!(dock.x + dock.w / 2, SW / 2);
        assert_eq!(dock.bottom(), SH - DOCK_MARGIN);
        for r in icons {
            assert!(dock.contains(r.x, r.y) && dock.contains(r.right() - 1, r.bottom() - 1));
        }
    }

    #[test]
    fn dock_icons_are_ordered_and_do_not_overlap() {
        let (_, icons) = dock_layout(SW, SH);
        for pair in icons.windows(2) {
            assert_eq!(pair[1].x - pair[0].right(), DOCK_GAP);
            assert_eq!(pair[0].y, pair[1].y);
        }
    }

    #[test]
    fn dock_slot_hit_testing() {
        let (_, icons) = dock_layout(SW, SH);
        for (i, r) in icons.iter().enumerate() {
            assert_eq!(dock_slot_at(SW, SH, r.x, r.y), Some(i));
            assert_eq!(dock_slot_at(SW, SH, r.right() - 1, r.bottom() - 1), Some(i));
            // The gap after the icon is not part of any slot.
            assert_eq!(dock_slot_at(SW, SH, r.right(), r.y), None);
        }
        assert_eq!(dock_slot_at(SW, SH, 0, 0), None);
        assert_eq!(dock_slot_at(SW, SH, -5, -5), None);
    }

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
        assert!(c.home.right() < c.reload.x);
        assert!(c.reload.right() < c.bar.x);
        assert!(c.bar.right() < c.go.x);
        assert_eq!(c.go.right(), r.right() - 14);
        assert_eq!(c.home.y, r.y + TITLE_H + 12);
        assert!(c.content.y > c.home.bottom());
        assert_eq!(c.content.bottom(), r.bottom() - 14);
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

    #[test]
    fn start_height_grows_with_apps() {
        assert_eq!(start_height(7), 20 + 9 * 38 + 12);
        assert_eq!(start_height(0), 20 + 2 * 38 + 12);
    }

    #[test]
    fn start_panel_is_above_the_dock_and_on_screen() {
        let (sx, sy) = start_origin(SW, SH, 7);
        let (dock, _) = dock_layout(SW, SH);
        assert!(sx >= 8 && sx + START_W <= SW - 8);
        assert_eq!(sy + start_height(7) + 12, dock.y + DOCK_PAD);
    }

    #[test]
    fn start_origin_clamps_horizontally_on_narrow_screens() {
        // 200 px wide: the dock overflows, and the panel pins to x = 8.
        let (sx, _) = start_origin(200, SH, 7);
        assert_eq!(sx, 8);
    }

    #[test]
    fn start_hits_apps_then_power_rows() {
        let (sx, sy) = start_origin(SW, SH, 7);
        let x = sx + START_W / 2;
        let top = sy + START_PAD;
        for i in 0..7 {
            let y = top + i * START_ROW_H + 1;
            assert_eq!(
                start_item_at(SW, SH, 7, x, y),
                Some(StartHit::App(i as usize))
            );
        }
        let pwr = top + 7 * START_ROW_H + START_GAP;
        assert_eq!(start_item_at(SW, SH, 7, x, pwr), Some(StartHit::Reboot));
        assert_eq!(
            start_item_at(SW, SH, 7, x, pwr + START_ROW_H),
            Some(StartHit::Shutdown)
        );
    }

    #[test]
    fn start_divider_and_margins_are_misses() {
        let (sx, sy) = start_origin(SW, SH, 7);
        let x = sx + START_W / 2;
        let top = sy + START_PAD;
        let divider = top + 7 * START_ROW_H + START_GAP / 2;
        assert_eq!(start_item_at(SW, SH, 7, x, divider), None);
        assert_eq!(start_item_at(SW, SH, 7, x, top - 1), None);
        assert_eq!(start_item_at(SW, SH, 7, sx + START_PAD - 1, top), None);
        assert_eq!(
            start_item_at(SW, SH, 7, sx + START_W - START_PAD, top),
            None
        );
        let below = top + 7 * START_ROW_H + START_GAP + 2 * START_ROW_H;
        assert_eq!(start_item_at(SW, SH, 7, x, below), None);
    }

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
