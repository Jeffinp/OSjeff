//! Desktop geometry, widget layout and chrome drawing helpers (free
//! functions shared across the app/input/render modules).

use super::*;

pub(crate) use osjeff_core::layout::{
    BrowserChrome, browser_home_layout, calc_button_at, calc_layout, dock_layout, fit_scale,
};

/// Alt+Tab panel geometry.
pub(crate) const SWITCH_W: i32 = 380;
pub(crate) const SWITCH_PAD: i32 = 10;
pub(crate) const SWITCH_ROW_H: i32 = 38;
/// Most rows shown at once (the list scrolls with the selection).
pub(crate) const SWITCH_ROWS: usize = 8;

/// Index of the context-menu item under `(px, py)` in a menu of `items` entries.
pub(crate) fn menu_item_at(mx: i32, my: i32, px: i32, py: i32, items: usize) -> Option<usize> {
    osjeff_core::layout::menu_item_at(mx, my, px, py, items)
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

/// Height of the start panel showing `rows` app rows.
pub(crate) fn start_height(rows: usize) -> i32 {
    osjeff_core::layout::start_height(rows)
}

/// Top-left of the start panel, centered above the dock's system icon.
pub(crate) fn start_origin(sw: i32, sh: i32, rows: usize) -> (i32, i32) {
    osjeff_core::layout::start_origin(sw, sh, rows)
}

/// The start-panel item under `(px, py)`, if any. `rows` is the number of app rows
/// shown and `scroll` the index of the first one.
pub(crate) fn start_item_at(
    sw: i32,
    sh: i32,
    rows: usize,
    scroll: usize,
    px: i32,
    py: i32,
) -> Option<StartItem> {
    use osjeff_core::layout::StartHit;
    Some(
        match osjeff_core::layout::start_item_at(sw, sh, rows, px, py)? {
            StartHit::App(i) => {
                let n = scroll + i;
                match Kind::ALL.get(n) {
                    Some(&k) => StartItem::App(k),
                    None => StartItem::Wasm(n - Kind::ALL.len()),
                }
            }
            StartHit::Reboot => StartItem::Reboot,
            StartHit::Shutdown => StartItem::Shutdown,
        },
    )
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
    // SAFETY: `buf` holds only ASCII digits (written by `two`) and ':', so it is valid UTF-8.
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
    // SAFETY: SCRATCH is `SCRATCH_BYTES` long; the only caller is `draw_animating` (compositor
    // thread), one use at a time.
    // NOTE: not guaranteed by the type: safe fn returning `&'static mut`; a second caller would alias.
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
