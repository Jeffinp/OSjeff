//! Desktop geometry, widget layout and chrome drawing helpers (free
//! functions shared across the app/input/render modules).

use super::*;

pub(crate) use osjeff_core::layout::{
    BrowserChrome, browser_home_layout, calc_button_at, calc_layout, dock_layout,
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
                (theme::WHITE, theme::accent())
            } else {
                (theme::accent(), theme::WHITE)
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
    paint_wallpaper(c, &crate::settings::get());
    let w = c.width();
    let h = c.height();

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

/// 24-bit `0xRRGGBB` to a framebuffer colour.
fn rgb24(v: u32) -> Color {
    Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// The original wallpaper (preset 0): indigo gradient plus two soft glows.
fn paint_indigo(c: &mut Canvas) {
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
}

/// Paint the wallpaper `s` selects: a preset or the user's image, falling back to
/// the original look when the image cannot be used.
fn paint_wallpaper(c: &mut Canvas, s: &osjeff_core::settings::Settings) {
    use osjeff_core::settings::WallpaperChoice;
    use osjeff_core::wallpaper::{PRESETS, Style};
    match s.wallpaper {
        WallpaperChoice::Image => {
            if !paint_image(c, s.image_path()) {
                paint_indigo(c);
            }
        }
        WallpaperChoice::Preset(n) => {
            let p = PRESETS.get(n as usize).unwrap_or(&PRESETS[0]);
            let (w, h) = (c.width(), c.height());
            match p.style {
                Style::Indigo => paint_indigo(c),
                Style::Solid => c.fill_rect(0, 0, w, h, rgb24(p.top)),
                Style::Gradient => {
                    for yy in 0..h {
                        let t = ((yy * 255) / h.max(1)) as u32;
                        let col = osjeff_core::wallpaper::lerp_rgb(p.top, p.bottom, t);
                        c.fill_rect(0, yy, w, 1, rgb24(col));
                    }
                }
            }
        }
    }
}

/// Decode the image file at `path` (FS v2), cover the screen with it and paint
/// it. Returns `false` (nothing painted) if it is missing or refused.
fn paint_image(c: &mut Canvas, path: &[u8]) -> bool {
    let (w, h) = (c.width(), c.height());
    let Some(bytes) = read_path(path) else {
        crate::klog!(Warn, "wallpaper: file not found, using the default");
        return false;
    };
    let mut img = match osjeff_core::wallpaper::load(&bytes, w, h) {
        Ok(img) => img,
        Err(e) => {
            crate::klog!(Warn, "wallpaper: {e}, using the default");
            return false;
        }
    };
    // Transparent pixels show the default backdrop colour.
    img.flatten(0xFF00_0000 | osjeff_core::wallpaper::PRESETS[0].top);
    for y in 0..h {
        for (x, &p) in img.row(y).iter().enumerate() {
            c.put(x, y, rgb24(p));
        }
    }
    crate::klog!(
        Info,
        "wallpaper: {} painted from the disk",
        core::str::from_utf8(path).unwrap_or("?")
    );
    true
}

pub(crate) fn draw_clock(c: &mut Canvas, t: Time) {
    let w = c.width();
    let h = c.height();
    let mut buf = [0u8; osjeff_core::hw::rtc::CLOCK_LEN];
    let t = osjeff_core::hw::rtc::Time {
        h: t.h,
        m: t.m,
        s: t.s,
    };
    let n = osjeff_core::hw::rtc::format_clock(t, crate::settings::clock24(), &mut buf);
    // `format_clock` writes only ASCII digits, ':', ' ', 'A', 'P' and 'M'.
    let clock = core::str::from_utf8(&buf[..n]).unwrap_or("");

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

/// The two shadow layers of a window (ambient + key), stronger when focused, scaled
/// by `alpha256` (0..=256) while the window fades.
pub(crate) fn window_shadow(focused: bool, alpha256: u32) -> [Shadow; 2] {
    let (a1, a2) = if focused { (74, 62) } else { (44, 38) };
    let s = |v: u32| v * alpha256.min(256) / 256;
    [
        Shadow {
            blur: if focused { 20 } else { 14 },
            dy: if focused { 14 } else { 8 },
            alpha: s(a1),
        },
        Shadow {
            blur: if focused { 6 } else { 4 },
            dy: if focused { 3 } else { 2 },
            alpha: s(a2),
        },
    ]
}

/// Mutable view of the window-animation texture buffer.
pub(crate) fn texture_slice() -> &'static mut [u8] {
    // SAFETY: TEXTURE is `TEXTURE_BYTES` long; the only caller is `draw_animating` (compositor
    // thread), one use at a time.
    // NOTE: not guaranteed by the type: safe fn returning `&'static mut`; a second caller would alias.
    unsafe { core::slice::from_raw_parts_mut(TEXTURE.get() as *mut u8, TEXTURE_BYTES) }
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

/// The pointing hand over links and buttons (same 10x16 box and hot spot as [`CURSOR`]).
pub(crate) const HAND: [&str; 15] = [
    "##        ",
    "#.#       ",
    "#.#       ",
    "#.#       ",
    "#.####    ",
    "#.#..###  ",
    "#.#..#.## ",
    "#.#..#..# ",
    "#.......# ",
    "##......# ",
    " #......# ",
    " #.....#  ",
    "  #....#  ",
    "  #....#  ",
    "  ######  ",
];

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
