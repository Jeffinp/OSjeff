//! Desktop geometry, widget layout and chrome drawing helpers (free
//! functions shared across the app/input/render modules).

use super::*;
use osjeff_core::style::PANEL_H;

pub(crate) use osjeff_core::layout::{BrowserChrome, browser_home_layout};

/// Alt+Tab panel geometry.
pub(crate) const SWITCH_W: i32 = 380;
pub(crate) const SWITCH_PAD: i32 = 10;
pub(crate) const SWITCH_ROW_H: i32 = 38;
/// Most rows shown at once (the list scrolls with the selection).
pub(crate) const SWITCH_ROWS: usize = 8;

/// Paint the wallpaper into `c` (the cached background) and bake what depends only on
/// it: the top panel.
pub fn paint_background(c: &mut Canvas) {
    paint_wallpaper(c, &crate::settings::get());
    let w = c.width() as i32;
    let p = theme::pal();

    // Panel: a lightly blurred wallpaper strip under a mostly opaque tint and a hairline, so it
    // reads as a flat bar (not a glass sheet).
    let bar = Rect::new(0, 0, w, PANEL_H);
    let strip = super::glass::Backdrop::capture(c, bar, 4);
    strip.draw(c, bar, 0, 256);
    let (tc, ta) = theme::tint(p.menubar_tint);
    c.blend_rect(bar, tc, ta);
    let (sc, sa) = theme::tint(p.separator);
    c.blend_rect(Rect::new(0, PANEL_H - 1, w, 1), sc, sa);
    // A faint light line over the hairline: the panel's inner highlight.
    let (gc, ga) = theme::tint(p.glass_edge);
    c.blend_rect(Rect::new(0, PANEL_H - 2, w, 1), gc, ga / 3);
}

/// 24-bit `0xRRGGBB` to a framebuffer colour.
fn rgb24(v: u32) -> Color {
    Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Paint the wallpaper `s` selects: a preset (in the scheme of the current
/// appearance) or the user's image, falling back to the default preset when the
/// image cannot be used.
fn paint_wallpaper(c: &mut Canvas, s: &osjeff_core::settings::Settings) {
    use osjeff_core::settings::WallpaperChoice;
    use osjeff_core::wallpaper::PRESETS;
    match s.wallpaper {
        WallpaperChoice::Image => {
            if !paint_image(c, s.image_path()) {
                paint_preset(c, &PRESETS[0]);
            }
        }
        WallpaperChoice::Preset(n) => {
            paint_preset(c, PRESETS.get(n as usize).unwrap_or(&PRESETS[0]))
        }
    }
}

fn paint_preset(c: &mut Canvas, p: &osjeff_core::wallpaper::Preset) {
    use osjeff_core::wallpaper::{Style, lerp_rgb};
    let (w, h) = (c.width(), c.height());
    let sc = p.scheme(theme::dark());
    match p.style {
        Style::Solid => c.fill_rect(0, 0, w, h, rgb24(sc.top)),
        Style::Gradient | Style::Glow | Style::Shapes => {
            for yy in 0..h {
                let t = ((yy * 255) / h.max(1)) as u32;
                c.fill_rect(0, yy, w, 1, rgb24(lerp_rgb(sc.top, sc.bottom, t)));
            }
        }
    }
    if matches!(p.style, Style::Glow | Style::Shapes) {
        let lut = osjeff_core::raster::glow_lut();
        for b in sc.blobs.iter().filter(|b| b.alpha > 0 && b.r > 0) {
            let cx = (w as i64 * b.x as i64 / 1000) as i32;
            let cy = (h as i64 * b.y as i64 / 1000) as i32;
            let rad = (w as i64 * b.r as i64 / 1000) as i32;
            c.glow(cx, cy, rad, rgb24(b.color), b.alpha as u32, &lut);
        }
    }
    if p.style == Style::Shapes && !p.shapes.is_empty() {
        // The facets / hills / bands: anti-aliased polygons on a transparent layer of the
        // screen's size (freed right after), laid over the gradient and the glows.
        use osjeff_core::glyph::Path;
        use osjeff_core::raster::{Paint, Surface, rgba};
        let mut layer = Surface::new(w, h);
        for sh in p.shapes {
            let (col, a) = sh.look(theme::dark());
            let pts: Vec<(i32, i32)> = sh
                .pts
                .iter()
                .map(|&(x, y)| {
                    (
                        (w as i64 * x as i64 * 256 / 1000) as i32,
                        (h as i64 * y as i64 * 256 / 1000) as i32,
                    )
                })
                .collect();
            let mut path = Path::new();
            path.polygon(&pts);
            layer.fill_path(&path, Paint::Solid(rgba(col, a)));
        }
        c.blit_surface(&layer, 0, 0, 256);
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

/// The two shadow layers of a window (ambient + key), stronger when focused, scaled
/// by `alpha256` (0..=256) while the window fades.
pub(crate) fn window_shadow(focused: bool, alpha256: u32) -> [Shadow; 2] {
    let (a1, a2) = if focused { (62, 54) } else { (36, 32) };
    let s = |v: u32| v * alpha256.min(256) / 256;
    [
        Shadow {
            blur: if focused { 16 } else { 10 },
            dy: if focused { 10 } else { 6 },
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
