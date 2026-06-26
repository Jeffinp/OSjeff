//! Desktop geometry, widget layout and chrome drawing helpers (free
//! functions shared across the app/input/render modules).

use super::*;

/// Index of the context-menu item under `(px, py)`, if any.
pub(crate) fn menu_item_at(mx: i32, my: i32, px: i32, py: i32) -> Option<usize> {
    if px < mx + MENU_PAD || px >= mx + MENU_W - MENU_PAD {
        return None;
    }
    let rel = py - (my + MENU_PAD);
    if rel < 0 {
        return None;
    }
    let i = (rel / MENU_ITEM_H) as usize;
    (i < MENU_ITEMS.len()).then_some(i)
}

/// The floating dock panel rect and its `DOCK_COUNT` icon slots.
pub(crate) fn dock_layout(sw: i32, sh: i32) -> (Rect, [Rect; DOCK_COUNT as usize]) {
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

/// Keypad geometry for a calculator window: grid origin, cell size and gap.
/// Shared by drawing and hit-testing so they always agree.
pub(crate) fn calc_layout(r: Rect) -> (i32, i32, i32, i32, i32) {
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
pub(crate) fn calc_button_at(r: Rect, px: i32, py: i32) -> Option<u8> {
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

/// Browser chrome geometry, shared by drawing and hit-testing so the toolbar
/// buttons, address bar and content area always agree.
pub(crate) struct BrowserChrome {
    pub home: Rect,
    pub reload: Rect,
    pub go: Rect,
    pub bar: Rect,
    pub content: Rect,
}

impl BrowserChrome {
    pub(crate) fn of(r: Rect) -> Self {
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

/// Start-page layout: the brand logo rect and the four shortcut-tile rects,
/// centered in the content box.
pub(crate) fn browser_home_layout(content: Rect) -> (Rect, [Rect; 4]) {
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

/// Label bytes for a keypad cell (`<` for the backspace sentinel).
pub(crate) fn key_label(k: &u8) -> &[u8] {
    if *k == 0x08 {
        b"<"
    } else {
        core::slice::from_ref(k)
    }
}

/// Background/foreground for a keypad button; the pending operator is inverted.
pub(crate) fn key_style(k: u8, pending: Option<u8>) -> (Color, Color) {
    match k {
        b'=' => (theme::ACCENT_2, theme::WHITE),
        b'C' => (theme::CLOSE, theme::WHITE),
        0x08 => (theme::HEADER, theme::HEADER_TEXT),
        b'+' | b'-' | b'*' | b'/' => {
            if pending == Some(k) {
                (theme::WHITE, theme::ACCENT)
            } else {
                (theme::ACCENT, theme::WHITE)
            }
        }
        _ => (theme::WINDOW_BODY, theme::TEXT), // digits + dot
    }
}

pub(crate) fn start_height() -> i32 {
    START_PAD * 2 + (START_APPS.len() as i32 + 2) * START_ROW_H + START_GAP
}

/// Top-left of the start panel, centered above the dock's system icon.
pub(crate) fn start_origin(sw: i32, sh: i32) -> (i32, i32) {
    let (_dock, icons) = dock_layout(sw, sh);
    let brand = icons[0];
    let x = (brand.x + DOCK_ICON / 2 - START_W / 2).clamp(8, (sw - START_W - 8).max(8));
    let y = brand.y - start_height() - 12;
    (x, y)
}

/// The start-panel item under `(px, py)`, if any.
pub(crate) fn start_item_at(sw: i32, sh: i32, px: i32, py: i32) -> Option<StartItem> {
    let (sx, sy) = start_origin(sw, sh);
    if px < sx + START_PAD || px >= sx + START_W - START_PAD {
        return None;
    }
    let top = sy + START_PAD;
    for (i, (_label, win)) in START_APPS.iter().enumerate() {
        let ry = top + i as i32 * START_ROW_H;
        if py >= ry && py < ry + START_ROW_H {
            return Some(StartItem::App(*win));
        }
    }
    let pwr_top = top + START_APPS.len() as i32 * START_ROW_H + START_GAP;
    for (i, item) in [StartItem::Reboot, StartItem::Shutdown].iter().enumerate() {
        let ry = pwr_top + i as i32 * START_ROW_H;
        if py >= ry && py < ry + START_ROW_H {
            return Some(*item);
        }
    }
    None
}

pub(crate) fn start_row_highlight(c: &mut Canvas, sx: i32, ry: i32, color: Color) {
    c.fill_round_rect_alpha(
        (sx + 5) as usize,
        (ry + 2) as usize,
        (START_W - 10) as usize,
        (START_ROW_H - 4) as usize,
        8,
        color,
        36,
    );
}

pub fn paint_background(c: &mut Canvas) {
    let w = c.width();
    let h = c.height();

    // Indigo gradient backdrop.
    for yy in 0..h {
        let t = ((yy * 255) / h.max(1)) as u16;
        c.fill_rect(0, yy, w, 1, theme::BG_TOP.lerp(theme::BG_BOTTOM, t));
    }
    // Two soft accent "mesh" blobs (teal top-left, violet bottom-right).
    let blob = (w / 3).max(360);
    c.fill_round_rect_alpha(0, 0, blob, blob, blob / 2, theme::GLOW_TEAL, 16);
    c.fill_round_rect_alpha(
        w - blob,
        h - blob,
        blob,
        blob,
        blob / 2,
        theme::GLOW_VIOLET,
        16,
    );

    // Brand wordmark top-left.
    font::draw_text(c, 20, 18, "OSJEFF", theme::HEADER_TEXT, 2);

    // Floating dock: shadow, panel, icons.
    let (dock, icons) = dock_layout(w as i32, h as i32);
    let (dx, dy, dw, dh) = (
        dock.x as usize,
        dock.y as usize,
        dock.w as usize,
        dock.h as usize,
    );
    let radius = dh / 2;
    c.fill_round_rect_alpha(dx, dy + 8, dw, dh, radius, theme::SHADOW, 38);
    c.fill_round_rect(dx, dy, dw, dh, radius, theme::DOCK);

    // Slot 0 = real OSJeff logo; the rest are vector app icons.
    let kinds = [
        Icon::Brand,
        Icon::Terminal,
        Icon::Editor,
        Icon::TaskMgr,
        Icon::Calculator,
        Icon::Browser,
        Icon::WasmApp,
        Icon::Files,
    ];
    for (i, kind) in kinds.iter().enumerate() {
        let r = icons[i];
        if i == 0 && r.w as usize == logo::SIZE_40 {
            c.draw_rgba(
                logo::ICON_40,
                logo::SIZE_40,
                logo::SIZE_40,
                r.x as usize,
                r.y as usize,
            );
        } else {
            icons::draw(c, *kind, r.x as usize, r.y as usize, r.w as usize);
        }
    }
}

pub(crate) fn draw_clock(c: &mut Canvas, t: Time) {
    let w = c.width();
    let h = c.height();
    let mut buf = [b'0'; 8];
    two(&mut buf, 0, t.h);
    buf[2] = b':';
    two(&mut buf, 3, t.m);
    buf[5] = b':';
    two(&mut buf, 6, t.s);
    let clock = unsafe { core::str::from_utf8_unchecked(&buf) };

    // Pill in the bottom-right corner.
    let tw = font::text_width(clock, 2);
    let pad = 14usize;
    let pw = tw + pad * 2;
    let ph = 34usize;
    let px = w - pw - DOCK_MARGIN as usize;
    let py = h - ph - DOCK_MARGIN as usize;
    c.fill_round_rect_alpha(px, py + 6, pw, ph, ph / 2, theme::SHADOW, 34);
    c.fill_round_rect(px, py, pw, ph, ph / 2, theme::DOCK);
    font::draw_text(c, px + pad, py + 9, clock, theme::HEADER_TEXT, 2);
}

pub(crate) fn two(buf: &mut [u8], idx: usize, val: u8) {
    buf[idx] = b'0' + (val / 10) % 10;
    buf[idx + 1] = b'0' + val % 10;
}

/// Mutable view of the window-compositing scratch buffer.
pub(crate) fn scratch_slice() -> &'static mut [u8] {
    unsafe { core::slice::from_raw_parts_mut(SCRATCH.get() as *mut u8, SCRATCH_BYTES) }
}

/// Copy the rectangle `r` from `src` into `dst` (identical framebuffer layout).
pub(crate) fn copy_region(
    dst: &mut [u8],
    src: &[u8],
    info: bootloader_api::info::FrameBufferInfo,
    r: Rect,
) {
    let bpp = info.bytes_per_pixel;
    let stride = info.stride;
    let x = r.x.max(0) as usize;
    let y = r.y.max(0) as usize;
    if x >= info.width || y >= info.height {
        return;
    }
    let x_end = (r.right().max(0) as usize).min(info.width);
    let y_end = (r.bottom().max(0) as usize).min(info.height);
    if x_end <= x {
        return;
    }
    let row_len = (x_end - x) * bpp;
    for row in y..y_end {
        let off = (row * stride + x) * bpp;
        dst[off..off + row_len].copy_from_slice(&src[off..off + row_len]);
    }
}

/// Right-align `v` as decimal digits in `buf[start..start+width]`.
pub(crate) fn write_uint(buf: &mut [u8], start: usize, width: usize, mut v: u32) {
    let mut i = start + width;
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 || i == start {
            break;
        }
    }
}

pub(crate) const CURSOR: [&str; 16] = [
    "#         ",
    "##        ",
    "#.#       ",
    "#..#      ",
    "#...#     ",
    "#....#    ",
    "#.....#   ",
    "#......#  ",
    "#.......# ",
    "#....#####",
    "#..#.#    ",
    "#.# #.#   ",
    "##  #.#   ",
    "#    #.#  ",
    "     #.#  ",
    "      #   ",
];
