//! Animated boot splash drawn before the desktop starts: the deep night wallpaper,
//! the brand mark, the name and a slim progress bar.

use crate::fb::{Canvas, Color, Corner};
use crate::text::{self, BODY, Weight};
use crate::theme;
use kitsune_core::Rect;
use kitsune_core::wallpaper::{PRESETS, lerp_rgb};

fn rgb24(v: u32) -> Color {
    Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Renders one frame of the splash. `progress` in `0.0..=1.0` fills the bar.
pub fn draw_splash(c: &mut Canvas, progress: f32) {
    let w = c.width();
    let h = c.height();

    // The dark scheme of the default wallpaper: gradient and soft glows.
    let sc = PRESETS[0].scheme(true);
    for y in 0..h {
        let t = ((y * 255) / h.max(1)) as u32;
        c.fill_rect(0, y, w, 1, rgb24(lerp_rgb(sc.top, sc.bottom, t)));
    }
    let lut = kitsune_core::raster::glow_lut();
    for b in sc.blobs.iter().filter(|b| b.alpha > 0 && b.r > 0) {
        let cx = (w as i64 * b.x as i64 / 1000) as i32;
        let cy = (h as i64 * b.y as i64 / 1000) as i32;
        let rad = (w as i64 * b.r as i64 / 1000) as i32;
        c.glow(cx, cy, rad, rgb24(b.color), b.alpha as u32, &lut);
    }

    // Brand mark with a soft halo, the name below it.
    let cx = w as i32 / 2;
    let mark = 112;
    let gy = h as i32 * 30 / 100;
    c.glow(cx, gy + mark / 2, mark * 2, theme::accent(), 70, &lut);
    c.blit_surface(
        crate::icons::surface(crate::icons::Icon::Brand, mark),
        cx - mark / 2,
        gy,
        256,
    );
    let name = Rect::new(0, gy + mark + 22, w as i32, 48);
    text::draw_centered(
        c,
        name,
        "OSjeff",
        40,
        Weight::Semibold,
        Color::rgb(255, 255, 255),
    );
    let sub = Rect::new(0, name.bottom() + 2, w as i32, 24);
    text::draw_centered(
        c,
        sub,
        "Sistema operacional",
        BODY,
        Weight::Regular,
        Color::rgb(0xC4, 0xC6, 0xE8),
    );

    // Slim progress bar: translucent track, accent fill.
    let bar_w = (w as i32 / 4).max(220);
    let bar = Rect::new((w as i32 - bar_w) / 2, h as i32 * 74 / 100, bar_w, 6);
    c.fill_rrect(bar, 3, Corner::Circle, Color::rgb(255, 255, 255), 46);
    let p = progress.clamp(0.0, 1.0);
    let fill = ((bar_w as f32 * p) as i32).max(0);
    if fill >= 6 {
        c.fill_rrect(
            Rect::new(bar.x, bar.y, fill, bar.h),
            3,
            Corner::Circle,
            theme::accent().lerp(Color::rgb(255, 255, 255), 60),
            256,
        );
    }
}
