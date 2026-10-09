//! Geometry of the system chrome: menu bar, menu panels, dock (with
//! magnification), Launchpad grid, Spotlight, popovers, toasts and the calendar.
//!
//! Pure integer layout (plus one `f32` magnification curve evaluated for a dozen
//! dock icons per frame), shared by the kernel's drawing code and its hit
//! testing so the two can never disagree. All sizes follow the 4 px grid of the
//! design spec (`docs/design/ui-macos.md`).

use crate::window::Rect;
use alloc::vec::Vec;

pub use crate::style::MENUBAR_H;

// ------------------------------------------------------------------- menu bar

/// Horizontal padding inside a menu bar item (each side).
pub const BAR_PAD: i32 = 8;
/// Margin between the screen edge and the first / last item.
pub const BAR_EDGE: i32 = 6;

/// Lay the menu bar items out: `left` content widths run from the left edge,
/// `right` ones from the right edge, in the order given (so the first of `right`
/// is the leftmost of the right group). Each item is its content plus
/// [`BAR_PAD`] on both sides and spans the whole bar height (a generous hit area).
pub fn menubar_layout(sw: i32, left: &[i32], right: &[i32]) -> (Vec<Rect>, Vec<Rect>) {
    let mut l = Vec::with_capacity(left.len());
    let mut x = BAR_EDGE;
    for &w in left {
        let iw = w + 2 * BAR_PAD;
        l.push(Rect::new(x, 0, iw, MENUBAR_H));
        x += iw;
    }
    let total: i32 = right.iter().map(|w| w + 2 * BAR_PAD).sum();
    let mut r = Vec::with_capacity(right.len());
    let mut x = sw - BAR_EDGE - total;
    for &w in right {
        let iw = w + 2 * BAR_PAD;
        r.push(Rect::new(x, 0, iw, MENUBAR_H));
        x += iw;
    }
    (l, r)
}

/// Index of the rectangle containing `(x, y)`.
pub fn rect_at(rects: &[Rect], x: i32, y: i32) -> Option<usize> {
    rects.iter().position(|r| r.contains(x, y))
}

// ----------------------------------------------------------------- menu panel

pub const MENU_ROW_H: i32 = 24;
pub const MENU_SEP_H: i32 = 9;
pub const MENU_PAD_Y: i32 = 6;
pub const MENU_PAD_X: i32 = 6;
/// Space reserved at the left of a row for a check mark.
pub const MENU_CHECK_W: i32 = 16;
pub const MENU_MIN_W: i32 = 168;

/// What a menu row needs measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuRow {
    Item { label_w: i32, shortcut_w: i32 },
    Separator,
}

/// Placement of an open menu: the panel and the rectangle of every row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuGeom {
    pub rect: Rect,
    pub rows: Vec<Rect>,
}

/// Lay a menu out with its top-left near `anchor`, kept fully on a `sw x sh`
/// screen and below the menu bar.
pub fn menu_geom(rows: &[MenuRow], anchor: (i32, i32), sw: i32, sh: i32) -> MenuGeom {
    let mut w = MENU_MIN_W;
    let mut h = 2 * MENU_PAD_Y;
    for r in rows {
        match *r {
            MenuRow::Item {
                label_w,
                shortcut_w,
            } => {
                let gap = if shortcut_w > 0 { 24 } else { 0 };
                w = w.max(2 * MENU_PAD_X + MENU_CHECK_W + label_w + gap + shortcut_w + 12);
                h += MENU_ROW_H;
            }
            MenuRow::Separator => h += MENU_SEP_H,
        }
    }
    let x = anchor.0.clamp(4, (sw - w - 4).max(4));
    let y = anchor.1.clamp(MENUBAR_H, (sh - h - 4).max(MENUBAR_H));
    let mut rects = Vec::with_capacity(rows.len());
    let mut ry = y + MENU_PAD_Y;
    for r in rows {
        let rh = match r {
            MenuRow::Item { .. } => MENU_ROW_H,
            MenuRow::Separator => MENU_SEP_H,
        };
        rects.push(Rect::new(x + MENU_PAD_X, ry, w - 2 * MENU_PAD_X, rh));
        ry += rh;
    }
    MenuGeom {
        rect: Rect::new(x, y, w, h),
        rows: rects,
    }
}

/// The row under `(x, y)` (separators never match).
pub fn menu_row_at(g: &MenuGeom, rows: &[MenuRow], x: i32, y: i32) -> Option<usize> {
    g.rows
        .iter()
        .zip(rows)
        .position(|(r, k)| matches!(k, MenuRow::Item { .. }) && r.contains(x, y))
}

// ----------------------------------------------------------------------- dock

/// Unmagnified icon side.
pub const DOCK_ICON: i32 = 48;
/// Most an icon grows to under the pointer.
pub const DOCK_MAX: i32 = 76;
pub const DOCK_GAP: i32 = 8;
pub const DOCK_PAD_X: i32 = 12;
pub const DOCK_PAD_Y: i32 = 8;
pub const DOCK_BOTTOM: i32 = 8;
/// Distance at which magnification fades out completely.
pub const DOCK_REACH: i32 = 104;
/// Extra gap around the separator line.
pub const DOCK_SEP: i32 = 12;
/// Height of the dock panel.
pub const DOCK_H: i32 = DOCK_ICON + 2 * DOCK_PAD_Y;

/// The dock panel and the rectangle of every icon for icon sizes `sizes` (pixels).
/// Icons sit on the panel's baseline (they grow upwards); the row is centred on
/// the screen. `sep_after` inserts the separator after that icon index.
pub fn dock_layout(sw: i32, sh: i32, sizes: &[i32], sep_after: Option<usize>) -> (Rect, Vec<Rect>) {
    let n = sizes.len() as i32;
    let mut total: i32 = sizes.iter().sum::<i32>() + (n - 1).max(0) * DOCK_GAP;
    if sep_after.is_some_and(|i| i + 1 < sizes.len()) {
        total += DOCK_SEP;
    }
    let panel_w = total + 2 * DOCK_PAD_X;
    let panel_y = sh - DOCK_BOTTOM - DOCK_H;
    let panel = Rect::new(sw / 2 - panel_w / 2, panel_y, panel_w, DOCK_H);
    let baseline = panel_y + DOCK_H - DOCK_PAD_Y;
    let mut x = panel.x + DOCK_PAD_X;
    let mut icons = Vec::with_capacity(sizes.len());
    for (i, &s) in sizes.iter().enumerate() {
        icons.push(Rect::new(x, baseline - s, s, s));
        x += s + DOCK_GAP;
        if sep_after == Some(i) && i + 1 < sizes.len() {
            x += DOCK_SEP;
        }
    }
    (panel, icons)
}

/// The resting layout (every icon at [`DOCK_ICON`]).
pub fn dock_rest(sw: i32, sh: i32, n: usize, sep_after: Option<usize>) -> (Rect, Vec<Rect>) {
    dock_layout(sw, sh, &alloc::vec![DOCK_ICON; n], sep_after)
}

/// Target size of every icon for a pointer at `pointer_x` (or none): a smooth bump
/// `(1 - (d / reach)^2)^2` over the distance `d` to each icon's resting centre,
/// scaled between [`DOCK_ICON`] and [`DOCK_MAX`]. Springs chase these values.
pub fn dock_magnify(rest: &[Rect], pointer_x: Option<i32>) -> Vec<f32> {
    dock_magnify_scaled(rest, pointer_x, 100)
}

/// [`dock_magnify`] with the bump scaled to `percent` of its height (`0` keeps every icon at
/// rest; the Ajustes slider).
pub fn dock_magnify_scaled(rest: &[Rect], pointer_x: Option<i32>, percent: u8) -> Vec<f32> {
    let gain = percent.min(100) as f32 / 100.0;
    rest.iter()
        .map(|r| {
            let base = DOCK_ICON as f32;
            let Some(px) = pointer_x else { return base };
            let d = (px - (r.x + r.w / 2)).abs();
            if d >= DOCK_REACH {
                return base;
            }
            let t = d as f32 / DOCK_REACH as f32;
            let w = 1.0 - t * t;
            base + (DOCK_MAX - DOCK_ICON) as f32 * gain * w * w
        })
        .collect()
}

/// The pointer is in the zone where the dock reacts: over the panel or in the band
/// above it that magnified icons occupy.
pub fn dock_zone(panel: Rect) -> Rect {
    Rect::new(
        panel.x - DOCK_PAD_X,
        panel.y - (DOCK_MAX - DOCK_ICON) - 8,
        panel.w + 2 * DOCK_PAD_X,
        panel.h + (DOCK_MAX - DOCK_ICON) + 8 + DOCK_BOTTOM,
    )
}

/// The icon whose column contains `x` (gaps belong to the nearer icon), if the
/// pointer is within the dock zone.
pub fn dock_icon_at(panel: Rect, icons: &[Rect], x: i32, y: i32) -> Option<usize> {
    if !dock_zone(panel).contains(x, y) {
        return None;
    }
    icons.iter().position(|r| {
        x >= r.x - DOCK_GAP / 2 - 1
            && x < r.right() + DOCK_GAP / 2 + 1
            && y >= r.y
            && y < r.bottom() + DOCK_PAD_Y
    })
}

/// The tooltip rectangle for a label `w x h` above `icon`, kept on screen.
pub fn dock_tooltip(icon: Rect, w: i32, h: i32, sw: i32) -> Rect {
    let x = (icon.x + icon.w / 2 - w / 2).clamp(4, (sw - w - 4).max(4));
    Rect::new(x, icon.y - h - 10, w, h)
}

// ------------------------------------------------------------------ Launchpad

pub const LP_CELL_W: i32 = 136;
pub const LP_CELL_H: i32 = 128;
pub const LP_ICON: i32 = 72;

/// Launchpad: the search field and the grid cells.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchGrid {
    pub field: Rect,
    /// One rectangle per item, row-major, already shifted by the scroll.
    pub cells: Vec<Rect>,
    pub cols: usize,
    /// Rows the screen shows at once.
    pub visible_rows: usize,
    /// Rows needed for all the items.
    pub total_rows: usize,
}

/// Lay `n` items out on a `sw x sh` screen, `scroll` rows scrolled off the top.
pub fn launchpad_grid(sw: i32, sh: i32, n: usize, scroll: usize) -> LaunchGrid {
    let field = Rect::new(sw / 2 - 150, 64, 300, 36);
    let cols = (((sw - 160) / LP_CELL_W).clamp(3, 8)) as usize;
    let top = field.bottom() + 56;
    let visible_rows = (((sh - top - 64) / LP_CELL_H).max(1)) as usize;
    let total_rows = n.div_ceil(cols).max(1);
    let scroll = scroll.min(total_rows.saturating_sub(visible_rows));
    let used_cols = n.min(cols).max(1) as i32;
    let left = sw / 2 - used_cols * LP_CELL_W / 2;
    let mut cells = Vec::with_capacity(n);
    for i in 0..n {
        let (row, col) = (i / cols, i % cols);
        // Rows scrolled out of view sit off-screen (negative y): never hit.
        let y = top + (row as i32 - scroll as i32) * LP_CELL_H;
        cells.push(Rect::new(
            left + col as i32 * LP_CELL_W,
            y,
            LP_CELL_W,
            LP_CELL_H,
        ));
    }
    LaunchGrid {
        field,
        cells,
        cols,
        visible_rows,
        total_rows,
    }
}

/// The item under `(x, y)` among the rows currently visible.
pub fn launchpad_cell_at(g: &LaunchGrid, sh: i32, x: i32, y: i32) -> Option<usize> {
    let top = g.field.bottom() + 56;
    let bottom = top + g.visible_rows as i32 * LP_CELL_H;
    if y < top || y >= bottom.min(sh) {
        return None;
    }
    g.cells.iter().position(|r| r.contains(x, y))
}

// ------------------------------------------------------------------ Spotlight

pub const SPOT_W: i32 = 640;
pub const SPOT_FIELD_H: i32 = 56;
pub const SPOT_ROW_H: i32 = 40;
pub const SPOT_MAX_ROWS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpotGeom {
    pub panel: Rect,
    pub field: Rect,
    pub rows: Vec<Rect>,
}

/// Spotlight with `n` result rows (at most [`SPOT_MAX_ROWS`] are shown).
pub fn spotlight_geom(sw: i32, sh: i32, n: usize) -> SpotGeom {
    let n = n.min(SPOT_MAX_ROWS) as i32;
    let results_h = if n > 0 { n * SPOT_ROW_H + 16 } else { 0 };
    let h = SPOT_FIELD_H + results_h;
    let x = sw / 2 - SPOT_W / 2;
    let y = (sh / 5).clamp(MENUBAR_H + 24, (sh - h - 24).max(MENUBAR_H + 24));
    let field = Rect::new(x, y, SPOT_W, SPOT_FIELD_H);
    let mut rows = Vec::new();
    for i in 0..n {
        rows.push(Rect::new(
            x + 8,
            y + SPOT_FIELD_H + 8 + i * SPOT_ROW_H,
            SPOT_W - 16,
            SPOT_ROW_H,
        ));
    }
    SpotGeom {
        panel: Rect::new(x, y, SPOT_W, h),
        field,
        rows,
    }
}

// ------------------------------------------------------------------- popovers

/// A popover of size `w x h` hanging below the bar item `anchor`, its right edge
/// aligned with the item's (clamped to the screen).
pub fn popover_rect(anchor: Rect, w: i32, h: i32, sw: i32) -> Rect {
    let x = (anchor.right() - w).clamp(8, (sw - w - 8).max(8));
    Rect::new(x, MENUBAR_H + 6, w, h)
}

// ------------------------------------------------------------- Control popover

pub const CONTROL_W: i32 = 320;
pub const CONTROL_H: i32 = 352;

/// Rectangles inside the Controls popover `r`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlGeom {
    /// Title row.
    pub title: Rect,
    /// The network card (icon, status, address).
    pub net: Rect,
    /// Appearance label and its segmented control (Auto / Claro / Escuro).
    pub appearance_label: Rect,
    pub appearance: Rect,
    /// Accent label and the eight swatches.
    pub accent_label: Rect,
    pub swatches: [Rect; 8],
    /// The three switch rows: label rect and switch rect (reduce motion, 24 h clock, banners).
    pub rows: [(Rect, Rect); 3],
}

pub fn control_geom(r: Rect) -> ControlGeom {
    let pad = 16;
    let w = r.w - 2 * pad;
    let title = Rect::new(r.x + pad, r.y + 14, w, 24);
    let net = Rect::new(r.x + pad, title.bottom() + 8, w, 56);
    let appearance_label = Rect::new(r.x + pad, net.bottom() + 16, w, 18);
    let appearance = Rect::new(r.x + pad, appearance_label.bottom() + 4, w, 28);
    let accent_label = Rect::new(r.x + pad, appearance.bottom() + 16, w, 18);
    let sw = 24;
    let gap = (w - 8 * sw) / 7;
    let mut swatches = [Rect::new(0, 0, 0, 0); 8];
    for (i, s) in swatches.iter_mut().enumerate() {
        *s = Rect::new(
            r.x + pad + i as i32 * (sw + gap),
            accent_label.bottom() + 6,
            sw,
            sw,
        );
    }
    let mut rows = [(Rect::new(0, 0, 0, 0), Rect::new(0, 0, 0, 0)); 3];
    let mut y = swatches[0].bottom() + 14;
    for row in rows.iter_mut() {
        let label = Rect::new(r.x + pad, y, w - 48, 24);
        let sw_rect =
            crate::widgets::switch_rect(r.right() - pad - crate::widgets::SWITCH_W, y + 1);
        *row = (label, sw_rect);
        y += 30;
    }
    ControlGeom {
        title,
        net,
        appearance_label,
        appearance,
        accent_label,
        swatches,
        rows,
    }
}

// ------------------------------------------------------------- Calendar popover

pub const CAL_W: i32 = 280;
pub const CAL_H: i32 = 300;

/// Rectangles inside the calendar popover `r`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarGeom {
    pub title: Rect,
    pub prev: Rect,
    pub next: Rect,
    /// Weekday letters row.
    pub weekdays: [Rect; 7],
    /// 6 x 7 day cells.
    pub cells: [[Rect; 7]; 6],
}

pub fn calendar_geom(r: Rect) -> CalendarGeom {
    let pad = 16;
    let cw = (r.w - 2 * pad) / 7;
    let title = Rect::new(r.x + pad, r.y + 12, r.w - 2 * pad - 56, 28);
    let prev = Rect::new(r.right() - pad - 52, r.y + 12, 24, 28);
    let next = Rect::new(r.right() - pad - 26, r.y + 12, 24, 28);
    let mut weekdays = [Rect::new(0, 0, 0, 0); 7];
    for (i, w) in weekdays.iter_mut().enumerate() {
        *w = Rect::new(r.x + pad + i as i32 * cw, title.bottom() + 6, cw, 20);
    }
    let mut cells = [[Rect::new(0, 0, 0, 0); 7]; 6];
    for (row, line) in cells.iter_mut().enumerate() {
        for (col, c) in line.iter_mut().enumerate() {
            *c = Rect::new(
                r.x + pad + col as i32 * cw,
                weekdays[0].bottom() + 4 + row as i32 * 34,
                cw,
                34,
            );
        }
    }
    CalendarGeom {
        title,
        prev,
        next,
        weekdays,
        cells,
    }
}

// --------------------------------------------------------------------- toasts

pub const TOAST_W: i32 = 344;
pub const TOAST_H: i32 = 68;
pub const TOAST_GAP: i32 = 8;
pub const TOAST_MARGIN: i32 = 12;

/// Rectangle of the `i`-th (0 = newest, topmost) banner: top-right, under the menu bar.
pub fn toast_rect(i: usize, sw: i32) -> Rect {
    Rect::new(
        sw - TOAST_W - TOAST_MARGIN,
        MENUBAR_H + 8 + i as i32 * (TOAST_H + TOAST_GAP),
        TOAST_W,
        TOAST_H,
    )
}

// ------------------------------------------------------------------- calendar

/// Weekday (0 = Sunday) of `year-month-day`.
pub fn weekday(year: i32, month: u8, day: u8) -> u8 {
    let days = crate::unixtime::days_from_civil(year, month, day);
    // 1970-01-01 was a Thursday (4).
    (days + 4).rem_euclid(7) as u8
}

/// The weeks of a month: 6 rows of 7 day numbers (0 = blank), Sunday first.
pub fn month_grid(year: i32, month: u8) -> [[u8; 7]; 6] {
    let first = weekday(year, month, 1) as usize;
    let n = crate::unixtime::days_in_month(year, month) as usize;
    let mut g = [[0u8; 7]; 6];
    for d in 0..n {
        let slot = first + d;
        g[slot / 7][slot % 7] = d as u8 + 1;
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menubar_items_flow_from_both_edges() {
        let (l, r) = menubar_layout(1280, &[20, 60, 40], &[30, 100]);
        assert_eq!(l[0], Rect::new(BAR_EDGE, 0, 36, MENUBAR_H));
        assert_eq!(l[1].x, l[0].right());
        assert_eq!(l[2].x, l[1].right());
        assert_eq!(r[1].right(), 1280 - BAR_EDGE);
        assert_eq!(r[0].right(), r[1].x);
        assert!(l[2].right() < r[0].x);
        assert!(l.iter().chain(&r).all(|x| x.h == MENUBAR_H && x.y == 0));
        assert_eq!(rect_at(&l, l[1].x + 3, 10), Some(1));
        assert_eq!(rect_at(&l, 600, 10), None);
        assert_eq!(rect_at(&r, r[0].x, 27), Some(0));
        assert_eq!(rect_at(&r, r[0].x, 28), None);
    }

    #[test]
    fn menu_geometry_sizes_to_content_and_stays_on_screen() {
        let rows = [
            MenuRow::Item {
                label_w: 90,
                shortcut_w: 30,
            },
            MenuRow::Separator,
            MenuRow::Item {
                label_w: 200,
                shortcut_w: 0,
            },
        ];
        let g = menu_geom(&rows, (10, 28), 1280, 720);
        assert_eq!(g.rows.len(), 3);
        assert_eq!(g.rect.h, 2 * MENU_PAD_Y + 2 * MENU_ROW_H + MENU_SEP_H);
        assert!(g.rect.w >= MENU_MIN_W);
        assert!(g.rect.w >= 2 * MENU_PAD_X + MENU_CHECK_W + 200 + 12);
        assert_eq!(g.rows[0].y, g.rect.y + MENU_PAD_Y);
        assert_eq!(g.rows[1].y, g.rows[0].bottom());
        assert_eq!(g.rows[2].y, g.rows[1].bottom());
        assert!(
            g.rows
                .iter()
                .all(|r| r.x >= g.rect.x && r.right() <= g.rect.right())
        );
        // Near the corner it shifts back inside; never under the menu bar.
        let c = menu_geom(&rows, (1270, 715), 1280, 720);
        assert!(c.rect.right() <= 1276 && c.rect.bottom() <= 716);
        let top = menu_geom(&rows, (100, 0), 1280, 720);
        assert_eq!(top.rect.y, MENUBAR_H);
        // Hit testing skips the separator.
        assert_eq!(
            menu_row_at(&g, &rows, g.rows[0].x + 4, g.rows[0].y + 4),
            Some(0)
        );
        assert_eq!(
            menu_row_at(&g, &rows, g.rows[1].x + 4, g.rows[1].y + 4),
            None
        );
        assert_eq!(
            menu_row_at(&g, &rows, g.rows[2].x + 4, g.rows[2].y + 4),
            Some(2)
        );
        assert_eq!(menu_row_at(&g, &rows, 0, 0), None);
    }

    #[test]
    fn dock_rest_layout_is_centred_and_on_the_grid() {
        let (panel, icons) = dock_rest(1280, 720, 9, Some(0));
        assert_eq!(icons.len(), 9);
        assert_eq!(panel.h, DOCK_H);
        assert_eq!(panel.bottom(), 720 - DOCK_BOTTOM);
        // Centred within a pixel.
        assert!(((panel.x + panel.w / 2) - 640).abs() <= 1);
        assert_eq!(icons[0].x, panel.x + DOCK_PAD_X);
        assert_eq!(icons[1].x, icons[0].right() + DOCK_GAP + DOCK_SEP);
        assert_eq!(icons[2].x, icons[1].right() + DOCK_GAP);
        assert_eq!(icons.last().unwrap().right(), panel.right() - DOCK_PAD_X);
        for r in &icons {
            assert_eq!((r.w, r.h), (DOCK_ICON, DOCK_ICON));
            assert_eq!(r.bottom(), panel.bottom() - DOCK_PAD_Y);
        }
    }

    #[test]
    fn magnification_peaks_under_the_pointer_and_fades() {
        let (_, rest) = dock_rest(1280, 720, 9, None);
        let none = dock_magnify(&rest, None);
        assert!(none.iter().all(|&s| s == DOCK_ICON as f32));
        let cx = rest[4].x + rest[4].w / 2;
        let s = dock_magnify(&rest, Some(cx));
        assert!((s[4] - DOCK_MAX as f32).abs() < 0.01);
        // Symmetric, decreasing with distance, back to rest at the reach.
        assert!((s[3] - s[5]).abs() < 0.01);
        assert!(s[3] < s[4] && s[2] < s[3] && s[1] <= s[2]);
        assert!(
            s.iter()
                .all(|&v| (DOCK_ICON as f32..=DOCK_MAX as f32 + 0.01).contains(&v))
        );
        let far = dock_magnify(&rest, Some(cx + DOCK_REACH + 200));
        assert!(far[4] <= DOCK_ICON as f32 + 0.01 || far[4] > 0.0);
        let at_reach = dock_magnify(&rest, Some(rest[0].x + rest[0].w / 2 - DOCK_REACH));
        assert_eq!(at_reach[0], DOCK_ICON as f32);
    }

    #[test]
    fn magnified_layout_grows_up_and_keeps_the_baseline() {
        let (_, rest) = dock_rest(1280, 720, 9, None);
        let sizes: Vec<i32> = dock_magnify(&rest, Some(rest[4].x + 24))
            .iter()
            .map(|&f| (f + 0.5) as i32)
            .collect();
        let (panel, icons) = dock_layout(1280, 720, &sizes, None);
        let (rest_panel, _) = dock_rest(1280, 720, 9, None);
        assert!(panel.w > rest_panel.w);
        assert_eq!(panel.h, rest_panel.h);
        for (r, s) in icons.iter().zip(&sizes) {
            assert_eq!(r.w, *s);
            assert_eq!(r.bottom(), panel.bottom() - DOCK_PAD_Y);
        }
        // Neighbours never overlap.
        for w in icons.windows(2) {
            assert_eq!(w[1].x - w[0].right(), DOCK_GAP);
        }
        assert!(icons[4].y < rest_panel.y);
    }

    #[test]
    fn dock_hit_testing_uses_the_columns() {
        let (panel, icons) = dock_rest(1280, 720, 9, None);
        for (i, r) in icons.iter().enumerate() {
            assert_eq!(
                dock_icon_at(panel, &icons, r.x + r.w / 2, r.y + 10),
                Some(i)
            );
        }
        // A gap belongs to a neighbour; far away or above the zone is nothing.
        let gap_x = icons[2].right() + 2;
        assert!(dock_icon_at(panel, &icons, gap_x, icons[2].y + 10).is_some());
        assert_eq!(dock_icon_at(panel, &icons, 10, 700), None);
        assert_eq!(dock_icon_at(panel, &icons, 640, 200), None);
        let z = dock_zone(panel);
        assert!(z.contains(640, panel.y - 20));
        assert!(!z.contains(640, panel.y - 60));
    }

    #[test]
    fn tooltips_stay_on_screen_above_the_icon() {
        let icon = Rect::new(5, 650, 48, 48);
        let t = dock_tooltip(icon, 80, 24, 1280);
        assert!(t.x >= 4 && t.bottom() < icon.y);
        let r = dock_tooltip(Rect::new(1260, 650, 48, 48), 80, 24, 1280);
        assert!(r.right() <= 1276);
    }

    #[test]
    fn launchpad_grid_centres_rows_and_scrolls() {
        let g = launchpad_grid(1280, 720, 21, 0);
        assert_eq!(g.cols, 8);
        assert_eq!(g.total_rows, 3);
        assert_eq!(g.cells.len(), 21);
        // Row-major and aligned.
        assert_eq!(g.cells[1].x, g.cells[0].right());
        assert_eq!(g.cells[8].y, g.cells[0].bottom());
        assert_eq!(g.cells[8].x, g.cells[0].x);
        // Centred.
        let left = g.cells[0].x;
        let right = g.cells[7].right();
        assert!((left + right - 1280).abs() <= 1);
        assert_eq!(g.field.y, 64);
        assert!((g.field.x * 2 + g.field.w - 1280).abs() <= 1);
        // Hit test inside the visible rows only.
        let c = g.cells[3];
        assert_eq!(launchpad_cell_at(&g, 720, c.x + 4, c.y + 4), Some(3));
        assert_eq!(launchpad_cell_at(&g, 720, 5, 5), None);
        // Few items: the single row is centred on its own width.
        let g2 = launchpad_grid(1280, 720, 3, 0);
        assert!((g2.cells[0].x + g2.cells[2].right() - 1280).abs() <= 1);
        // Scrolling moves rows up (clamped).
        let s = launchpad_grid(1280, 300, 40, 99);
        assert!(s.visible_rows >= 1);
        assert!(s.cells[0].y < s.field.bottom());
    }

    #[test]
    fn spotlight_panel_grows_with_results() {
        let a = spotlight_geom(1280, 720, 0);
        let b = spotlight_geom(1280, 720, 5);
        assert_eq!(a.panel.h, SPOT_FIELD_H);
        assert_eq!(b.panel.h, SPOT_FIELD_H + 5 * SPOT_ROW_H + 16);
        assert_eq!(b.rows.len(), 5);
        assert_eq!(a.panel.x, b.panel.x);
        assert_eq!(a.panel.y, b.panel.y);
        assert!(
            b.rows
                .iter()
                .all(|r| b.panel.contains(r.x, r.y) && r.bottom() <= b.panel.bottom())
        );
        // Capped.
        assert_eq!(spotlight_geom(1280, 720, 99).rows.len(), SPOT_MAX_ROWS);
        assert!(a.panel.y >= MENUBAR_H);
    }

    #[test]
    fn popovers_and_toasts_sit_under_the_bar() {
        let anchor = Rect::new(1100, 0, 40, 28);
        let p = popover_rect(anchor, 320, 300, 1280);
        assert_eq!(p.y, MENUBAR_H + 6);
        assert_eq!(p.right(), 1140);
        let edge = popover_rect(Rect::new(1260, 0, 40, 28), 320, 300, 1280);
        assert_eq!(edge.right(), 1272);
        let left = popover_rect(Rect::new(0, 0, 20, 28), 320, 300, 1280);
        assert_eq!(left.x, 8);
        let t0 = toast_rect(0, 1280);
        let t1 = toast_rect(1, 1280);
        assert_eq!(t0.right(), 1280 - TOAST_MARGIN);
        assert!(t0.y >= MENUBAR_H);
        assert_eq!(t1.y, t0.bottom() + TOAST_GAP);
    }

    #[test]
    fn popover_contents_fit_their_popovers() {
        let r = Rect::new(900, 34, CONTROL_W, CONTROL_H);
        let g = control_geom(r);
        let all = [
            g.title,
            g.net,
            g.appearance_label,
            g.appearance,
            g.accent_label,
        ];
        for x in all.iter().chain(g.swatches.iter()) {
            assert!(
                x.x >= r.x && x.right() <= r.right() && x.y >= r.y && x.bottom() <= r.bottom(),
                "{x:?}"
            );
        }
        for (l, s) in g.rows {
            assert!(l.right() <= s.x && s.right() <= r.right() && s.bottom() <= r.bottom());
        }
        // Rows go top to bottom without overlap.
        assert!(
            g.net.bottom() <= g.appearance_label.y && g.appearance.bottom() <= g.accent_label.y
        );
        assert!(g.swatches[0].bottom() <= g.rows[0].0.y);
        for w in g.swatches.windows(2) {
            assert!(w[0].right() <= w[1].x);
        }
        let cr = Rect::new(900, 34, CAL_W, CAL_H);
        let c = calendar_geom(cr);
        assert!(c.cells[5][6].bottom() <= cr.bottom() && c.cells[5][6].right() <= cr.right());
        assert!(c.prev.right() <= c.next.x && c.next.right() <= cr.right());
        assert!(c.title.right() <= c.prev.x);
        assert_eq!(c.cells[0][1].x - c.cells[0][0].x, c.cells[0][0].w);
    }

    #[test]
    fn calendar_weekdays_and_grid() {
        assert_eq!(weekday(1970, 1, 1), 4);
        assert_eq!(weekday(2000, 1, 1), 6); // Saturday
        assert_eq!(weekday(2024, 2, 29), 4); // Thursday
        assert_eq!(weekday(2026, 10, 8), 4); // Thursday
        let g = month_grid(2026, 10);
        assert_eq!(g[0][4], 1); // Oct 1 2026 is a Thursday
        let days: Vec<u8> = g.iter().flatten().copied().filter(|&d| d != 0).collect();
        assert_eq!(days.len(), 31);
        assert_eq!(days, (1..=31).collect::<Vec<u8>>());
        // February in a leap and a common year.
        let n = |y| {
            month_grid(y, 2)
                .iter()
                .flatten()
                .filter(|&&d| d != 0)
                .count()
        };
        assert_eq!((n(2024), n(2025)), (29, 28));
        // Never needs a seventh row.
        for y in 2020..2030 {
            for m in 1..=12 {
                assert_eq!(
                    month_grid(y, m)
                        .iter()
                        .flatten()
                        .filter(|&&d| d != 0)
                        .count(),
                    crate::unixtime::days_in_month(y, m) as usize
                );
            }
        }
    }

    #[test]
    fn dock_zoom_scales_the_bump() {
        let (_, rest) = dock_rest(1280, 720, 9, Some(0));
        let mid = rest[4].x + rest[4].w / 2;
        let full = dock_magnify_scaled(&rest, Some(mid), 100);
        let half = dock_magnify_scaled(&rest, Some(mid), 50);
        let none = dock_magnify_scaled(&rest, Some(mid), 0);
        assert_eq!(full, dock_magnify(&rest, Some(mid)));
        assert!(none.iter().all(|&v| v == DOCK_ICON as f32));
        let bump = |v: &Vec<f32>| v[4] - DOCK_ICON as f32;
        assert!((bump(&half) * 2.0 - bump(&full)).abs() < 0.01);
        // More than 100 is clamped.
        assert_eq!(dock_magnify_scaled(&rest, Some(mid), 250), full);
    }
}
