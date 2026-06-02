//! Animated boot splash drawn before the desktop starts. OSJeff identity:
//! dark indigo mesh, brand mark, teal progress with glow.

use crate::fb::Canvas;
use crate::font;
use crate::logo;
use crate::theme;

fn centered_x(c: &Canvas, text: &str, scale: usize) -> usize {
    (c.width().saturating_sub(font::text_width(text, scale))) / 2
}

/// Renders one frame of the splash. `progress` in `0.0..=1.0` fills the bar.
pub fn draw_splash(c: &mut Canvas, progress: f32) {
    let w = c.width();
    let h = c.height();

    // Indigo gradient + two soft accent "mesh" blobs.
    for y in 0..h {
        let t = ((y * 255) / h.max(1)) as u16;
        c.fill_rect(0, y, w, 1, theme::BG_TOP.lerp(theme::BG_BOTTOM, t));
    }
    let blob = (w / 3).max(320);
    c.fill_round_rect_alpha(
        0usize.saturating_sub(0),
        0,
        blob,
        blob,
        blob / 2,
        theme::GLOW_TEAL,
        20,
    );
    c.fill_round_rect_alpha(
        w - blob,
        h - blob,
        blob,
        blob,
        blob / 2,
        theme::GLOW_VIOLET,
        20,
    );

    // Brand logo (real RGBA) + glow.
    let cx = w / 2;
    let mark = logo::SIZE_128;
    let gy = h * 30 / 100;
    let glow = mark * 2;
    c.fill_round_rect_alpha(
        cx - glow / 2,
        gy + mark / 2 - glow / 2,
        glow,
        glow,
        glow / 2,
        theme::ACCENT,
        22,
    );
    c.draw_rgba(logo::ICON_128, mark, mark, cx - mark / 2, gy);

    // Wordmark + tagline.
    let title = "OSJEFF";
    font::draw_text(
        c,
        centered_x(c, title, 6),
        gy + mark + 24,
        title,
        theme::WHITE,
        6,
    );
    let sub = "Sistema Operacional";
    font::draw_text(
        c,
        centered_x(c, sub, 2),
        gy + mark + 88,
        sub,
        theme::TEXT_MUTED,
        2,
    );

    // Progress bar with glow under the fill.
    let bar_w = (w / 3).max(240);
    let bar_x = (w - bar_w) / 2;
    let bar_y = h * 72 / 100;
    let bar_h = 8usize;
    c.fill_round_rect(bar_x, bar_y, bar_w, bar_h, bar_h / 2, theme::DOCK_EDGE);
    let p = progress.clamp(0.0, 1.0);
    let fill = (bar_w as f32 * p) as usize;
    if fill > bar_h {
        c.fill_round_rect_alpha(
            bar_x,
            bar_y - 3,
            fill,
            bar_h + 6,
            (bar_h + 6) / 2,
            theme::ACCENT,
            60,
        );
        c.fill_round_rect(bar_x, bar_y, fill, bar_h, bar_h / 2, theme::ACCENT);
    }

    let status = "Carregando o sistema...";
    font::draw_text(
        c,
        centered_x(c, status, 2),
        bar_y + 24,
        status,
        theme::TEXT_MUTED,
        2,
    );
}
