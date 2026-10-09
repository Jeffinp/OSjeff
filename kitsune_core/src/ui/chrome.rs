//! Geometry of the system chrome: the top panel, menu panels, the taskbar, the Apps grid,
//! Busca, popovers (Quick Settings, the calendar and notification centre), toasts and the
//! calendar.
//!
//! Pure integer layout shared by the kernel's drawing code and its hit testing so the two can
//! never disagree. All sizes follow the 4 px grid of the design spec
//! (`docs/design/ui-identity.md`, tokens in `docs/design/ui-macos.md`).

use crate::windowing::window::Rect;
use alloc::vec::Vec;

pub use crate::ui::style::{MENUBAR_H, PANEL_H};

// ---------------------------------------------------------------------- panel

/// Horizontal padding inside a panel item (each side).
pub const PANEL_PAD: i32 = 10;
/// Margin between the screen edge and the first / last item.
pub const PANEL_EDGE: i32 = 6;
/// Gap between neighbouring panel items.
pub const PANEL_GAP: i32 = 2;
/// Padding of the status pill's icons and their pitch.
pub const PILL_ICON: i32 = 16;
pub const PILL_PITCH: i32 = 26;
pub const PILL_PAD: i32 = 8;

/// Where the panel's items sit: `left` ones flow from the left edge, the `center` one is centred on
/// the screen and the `right` ones flow to the right edge. Every item is its content width plus
/// [`PANEL_PAD`] on both sides and spans the whole panel height (a generous hit area).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelRects {
    pub left: Vec<Rect>,
    pub center: Rect,
    pub right: Vec<Rect>,
}

pub fn panel_layout(sw: i32, left: &[i32], center_w: i32, right: &[i32]) -> PanelRects {
    let mut l = Vec::with_capacity(left.len());
    let mut x = PANEL_EDGE;
    for &w in left {
        let iw = w + 2 * PANEL_PAD;
        l.push(Rect::new(x, 0, iw, PANEL_H));
        x += iw + PANEL_GAP;
    }
    let cw = center_w + 2 * PANEL_PAD;
    let center = Rect::new(sw / 2 - cw / 2, 0, cw, PANEL_H);
    let total: i32 = right.iter().map(|w| w + 2 * PANEL_PAD).sum::<i32>()
        + PANEL_GAP * right.len().saturating_sub(1) as i32;
    let mut r = Vec::with_capacity(right.len());
    let mut x = sw - PANEL_EDGE - total;
    for &w in right {
        let iw = w + 2 * PANEL_PAD;
        r.push(Rect::new(x, 0, iw, PANEL_H));
        x += iw + PANEL_GAP;
    }
    PanelRects {
        left: l,
        center,
        right: r,
    }
}

/// The workspace indicator: one dot per workspace, the current one a longer pill.
pub const WS_DOT: i32 = 8;
pub const WS_CUR_W: i32 = 22;
pub const WS_GAP: i32 = 6;

/// Content width of the indicator for `n` workspaces (the same whichever is current).
pub fn workspace_width(n: u8) -> i32 {
    let n = n as i32;
    if n == 0 {
        return 0;
    }
    n * WS_DOT + (n - 1) * WS_GAP + (WS_CUR_W - WS_DOT)
}

/// The rectangle of dot `i` inside the panel item `r` (which includes [`PANEL_PAD`]) when `cur` is
/// the current workspace.
pub fn workspace_dot(r: Rect, i: u8, cur: u8) -> Rect {
    let mut x = r.x + PANEL_PAD;
    for k in 0..i {
        x += if k == cur { WS_CUR_W } else { WS_DOT } + WS_GAP;
    }
    let w = if i == cur { WS_CUR_W } else { WS_DOT };
    Rect::new(x, r.y + (r.h - WS_DOT) / 2, w, WS_DOT)
}

/// The workspace under `x` in the indicator `r` of `n` dots (the gaps belong to the nearer dot, the
/// whole panel height is the target).
pub fn workspace_at(r: Rect, n: u8, cur: u8, x: i32, y: i32) -> Option<u8> {
    if !r.contains(x, y) {
        return None;
    }
    (0..n).find(|&i| {
        let d = workspace_dot(r, i, cur);
        x >= d.x - WS_GAP / 2 && x < d.right() + WS_GAP / 2
    })
}

/// Content width of the status pill with `n` icons.
pub fn pill_width(n: usize) -> i32 {
    (n as i32 * PILL_PITCH - (PILL_PITCH - PILL_ICON)).max(0)
}

/// The rectangle of icon `i` inside the pill `r` (which already includes [`PANEL_PAD`]).
pub fn pill_icon(r: Rect, i: usize) -> Rect {
    Rect::new(
        r.x + PANEL_PAD + i as i32 * PILL_PITCH,
        r.y + (r.h - PILL_ICON) / 2,
        PILL_ICON,
        PILL_ICON,
    )
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

// ------------------------------------------------------------------- launcher

pub const LP_CELL_W: i32 = 136;
pub const LP_CELL_H: i32 = 128;
pub const LP_ICON: i32 = 72;
/// The rail's width and its rows.
pub const RAIL_W: i32 = 196;
pub const RAIL_ROW_H: i32 = 40;
pub const RAIL_ROWS: usize = 5;
/// Recent apps shown in their row: a compact cell each.
pub const RECENT_W: i32 = 120;
pub const RECENT_H: i32 = 64;

/// The Apps launcher: the category rail at the left, the search field and the *Recentes* row on
/// top of the content, and the grid below.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchGrid {
    pub rail: Rect,
    pub rail_rows: [Rect; RAIL_ROWS],
    pub field: Rect,
    /// "Recentes" caption and the compact cells (empty when there is nothing to show).
    pub recents_label: Rect,
    pub recents: Vec<Rect>,
    /// One rectangle per item, row-major, already shifted by the scroll.
    pub cells: Vec<Rect>,
    pub cols: usize,
    /// Top of the grid area and the left edge of the content.
    pub top: i32,
    pub left: i32,
    /// Rows the screen shows at once.
    pub visible_rows: usize,
    /// Rows needed for all the items.
    pub total_rows: usize,
}

/// Lay the launcher out on a `sw x sh` screen for `n` items, `scroll` rows scrolled off the top
/// and `n_recents` compact recent cells (0 hides the row).
pub fn launcher_grid(sw: i32, sh: i32, n: usize, scroll: usize, n_recents: usize) -> LaunchGrid {
    let rail = Rect::new(32, PANEL_H + 40, RAIL_W, RAIL_ROWS as i32 * RAIL_ROW_H + 16);
    let mut rail_rows = [Rect::new(0, 0, 0, 0); RAIL_ROWS];
    for (i, r) in rail_rows.iter_mut().enumerate() {
        *r = Rect::new(
            rail.x + 8,
            rail.y + 8 + i as i32 * RAIL_ROW_H,
            rail.w - 16,
            RAIL_ROW_H,
        );
    }
    let left = rail.right() + 36;
    let right = sw - 40;
    let field = Rect::new(left, PANEL_H + 40, 360.min((right - left).max(120)), 36);
    let mut y = field.bottom() + 20;
    let (recents_label, recents) = if n_recents > 0 {
        let label = Rect::new(left, y, 160, 18);
        let ry = label.bottom() + 8;
        let cells = (0..n_recents.min(crate::windowing::launcher::RECENTS))
            .map(|i| Rect::new(left + i as i32 * (RECENT_W + 8), ry, RECENT_W, RECENT_H))
            .collect();
        y = ry + RECENT_H + 20;
        (label, cells)
    } else {
        (Rect::new(0, 0, 0, 0), Vec::new())
    };
    let top = y;
    let cols = (((right - left) / LP_CELL_W).clamp(2, 8)) as usize;
    let visible_rows = (((sh - top - 24) / LP_CELL_H).max(1)) as usize;
    let total_rows = n.div_ceil(cols).max(1);
    let scroll = scroll.min(total_rows.saturating_sub(visible_rows));
    let mut cells = Vec::with_capacity(n);
    for i in 0..n {
        let (row, col) = (i / cols, i % cols);
        // Rows scrolled out of view sit off-screen (negative y): never hit.
        let cy = top + (row as i32 - scroll as i32) * LP_CELL_H;
        cells.push(Rect::new(
            left + col as i32 * LP_CELL_W,
            cy,
            LP_CELL_W,
            LP_CELL_H,
        ));
    }
    LaunchGrid {
        rail,
        rail_rows,
        field,
        recents_label,
        recents,
        cells,
        cols,
        top,
        left,
        visible_rows,
        total_rows,
    }
}

/// The item under `(x, y)` among the rows currently visible.
pub fn launcher_cell_at(g: &LaunchGrid, sh: i32, x: i32, y: i32) -> Option<usize> {
    let bottom = g.top + g.visible_rows as i32 * LP_CELL_H;
    if y < g.top || y >= bottom.min(sh) {
        return None;
    }
    g.cells.iter().position(|r| r.contains(x, y))
}

/// The rail row under `(x, y)`.
pub fn launcher_rail_at(g: &LaunchGrid, x: i32, y: i32) -> Option<usize> {
    g.rail_rows.iter().position(|r| r.contains(x, y))
}

/// The recent cell under `(x, y)`.
pub fn launcher_recent_at(g: &LaunchGrid, x: i32, y: i32) -> Option<usize> {
    g.recents.iter().position(|r| r.contains(x, y))
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
    Rect::new(x, PANEL_H + 6, w, h)
}

// ---------------------------------------------------------------- Quick Settings

pub const QUICK_W: i32 = 344;
pub const QUICK_H: i32 = 360;
/// Tiles of the Quick Settings grid, in reading order.
pub const QUICK_TILES: usize = 6;
pub const QUICK_TILE_H: i32 = 56;

/// What a Quick Settings tile does (the order of [`QuickGeom::tiles`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuickTile {
    Network,
    Appearance,
    ReduceMotion,
    DoNotDisturb,
    Clock24,
    Settings,
}

pub const QUICK_ORDER: [QuickTile; QUICK_TILES] = [
    QuickTile::Network,
    QuickTile::Appearance,
    QuickTile::ReduceMotion,
    QuickTile::DoNotDisturb,
    QuickTile::Clock24,
    QuickTile::Settings,
];

/// Rectangles inside the Quick Settings popover `r`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuickGeom {
    pub title: Rect,
    /// Two columns by three rows of tiles.
    pub tiles: [Rect; QUICK_TILES],
    pub accent_label: Rect,
    pub swatches: [Rect; 8],
    /// Restart and shut down.
    pub restart: Rect,
    pub shutdown: Rect,
}

pub fn quick_geom(r: Rect) -> QuickGeom {
    let pad = 16;
    let w = r.w - 2 * pad;
    let title = Rect::new(r.x + pad, r.y + 14, w, 24);
    let gap = 8;
    let tw = (w - gap) / 2;
    let top = title.bottom() + 10;
    let mut tiles = [Rect::new(0, 0, 0, 0); QUICK_TILES];
    for (i, t) in tiles.iter_mut().enumerate() {
        let (row, col) = (i as i32 / 2, i as i32 % 2);
        *t = Rect::new(
            r.x + pad + col * (tw + gap),
            top + row * (QUICK_TILE_H + gap),
            if col == 1 { w - tw - gap } else { tw },
            QUICK_TILE_H,
        );
    }
    let accent_label = Rect::new(r.x + pad, tiles[QUICK_TILES - 1].bottom() + 14, w, 18);
    let sw = 24;
    let sgap = (w - 8 * sw) / 7;
    let mut swatches = [Rect::new(0, 0, 0, 0); 8];
    for (i, s) in swatches.iter_mut().enumerate() {
        *s = Rect::new(
            r.x + pad + i as i32 * (sw + sgap),
            accent_label.bottom() + 6,
            sw,
            sw,
        );
    }
    let by = swatches[0].bottom() + 16;
    let bw = (w - gap) / 2;
    QuickGeom {
        title,
        tiles,
        accent_label,
        swatches,
        restart: Rect::new(r.x + pad, by, bw, 32),
        shutdown: Rect::new(r.x + pad + bw + gap, by, w - bw - gap, 32),
    }
}

/// The tile under `(x, y)`.
pub fn quick_tile_at(g: &QuickGeom, x: i32, y: i32) -> Option<QuickTile> {
    g.tiles
        .iter()
        .position(|t| t.contains(x, y))
        .map(|i| QUICK_ORDER[i])
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

// ------------------------------------------------- calendar and notification centre

pub const CENTRE_W: i32 = 640;
pub const CENTRE_H: i32 = 364;
/// Notification rows shown at once.
pub const CENTRE_ROWS: usize = 4;
pub const CENTRE_ROW_H: i32 = 48;

/// Rectangles inside the calendar and notification centre popover `r`: notifications at the
/// left (GNOME's arrangement), the calendar at the right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CentreGeom {
    /// Big weekday and the date line under it.
    pub day: Rect,
    pub date: Rect,
    pub notif_title: Rect,
    pub clear: Rect,
    pub rows: [Rect; CENTRE_ROWS],
    /// Shown instead of the rows when there are no notifications.
    pub empty: Rect,
    pub dnd_label: Rect,
    pub dnd_switch: Rect,
    /// The calendar's own rectangle (feed it to [`calendar_geom`]).
    pub calendar: Rect,
}

pub fn centre_geom(r: Rect) -> CentreGeom {
    let pad = 16;
    let left_w = r.w - CAL_W - 3 * pad;
    let day = Rect::new(r.x + pad, r.y + 14, left_w, 28);
    let date = Rect::new(r.x + pad, day.bottom(), left_w, 18);
    let notif_title = Rect::new(r.x + pad, date.bottom() + 16, left_w - 72, 24);
    let clear = Rect::new(r.x + pad + left_w - 68, notif_title.y, 68, 24);
    let mut rows = [Rect::new(0, 0, 0, 0); CENTRE_ROWS];
    for (i, row) in rows.iter_mut().enumerate() {
        *row = Rect::new(
            r.x + pad,
            notif_title.bottom() + 6 + i as i32 * CENTRE_ROW_H,
            left_w,
            CENTRE_ROW_H - 4,
        );
    }
    let empty = Rect::new(
        r.x + pad,
        notif_title.bottom() + 6,
        left_w,
        CENTRE_ROWS as i32 * CENTRE_ROW_H - 4,
    );
    let dnd_y = r.bottom() - pad - 28;
    let dnd_label = Rect::new(r.x + pad, dnd_y, left_w - 56, 28);
    let dnd_switch = crate::ui::widgets::switch_rect(
        r.x + pad + left_w - crate::ui::widgets::SWITCH_W,
        dnd_y + 2,
    );
    CentreGeom {
        day,
        date,
        notif_title,
        clear,
        rows,
        empty,
        dnd_label,
        dnd_switch,
        calendar: Rect::new(r.right() - CAL_W - pad, r.y + 8, CAL_W, CAL_H),
    }
}

/// A popover of size `w x h` centred under the panel item `anchor` (kept on screen).
pub fn popover_centered(anchor: Rect, w: i32, h: i32, sw: i32) -> Rect {
    let x = (anchor.x + anchor.w / 2 - w / 2).clamp(8, (sw - w - 8).max(8));
    Rect::new(x, PANEL_H + 6, w, h)
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
    let days = crate::format::unixtime::days_from_civil(year, month, day);
    // 1970-01-01 was a Thursday (4).
    (days + 4).rem_euclid(7) as u8
}

/// The weeks of a month: 6 rows of 7 day numbers (0 = blank), Sunday first.
pub fn month_grid(year: i32, month: u8) -> [[u8; 7]; 6] {
    let first = weekday(year, month, 1) as usize;
    let n = crate::format::unixtime::days_in_month(year, month) as usize;
    let mut g = [[0u8; 7]; 6];
    for d in 0..n {
        let slot = first + d;
        g[slot / 7][slot % 7] = d as u8 + 1;
    }
    g
}

#[cfg(test)]
mod tests;
