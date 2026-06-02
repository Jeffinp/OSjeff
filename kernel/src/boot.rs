//! Animated boot splash drawn before the desktop starts.

use crate::fb::{Canvas, Color};
use crate::font;
use crate::icons::{self, Icon};

fn centered_x(c: &Canvas, text: &str, scale: usize) -> usize {
    (c.width().saturating_sub(font::text_width(text, scale))) / 2
}

/// Renders one frame of the splash. `progress` in `0.0..=1.0` fills the bar.
pub fn draw_splash(c: &mut Canvas, progress: f32) {
    let w = c.width();
    let h = c.height();

    // Dark vertical gradient backdrop.
    let top = Color::rgb(0x07, 0x0C, 0x1A);
    let bottom = Color::rgb(0x12, 0x22, 0x42);
    for y in 0..h {
        let t = ((y * 255) / h.max(1)) as u16;
        c.fill_rect(0, y, w, 1, top.lerp(bottom, t));
    }

    // Soft glow behind the logo.
    let cx = w / 2;
    let logo = (h / 7).max(56);
    let gy = h * 30 / 100;
    let glow = (logo as i32 * 5 / 2) as usize;
    c.fill_round_rect_alpha(
        cx - glow / 2,
        gy + logo / 2 - glow / 2,
        glow,
        glow,
        glow / 2,
        Color::rgb(0x3B, 0x82, 0xF6),
        26,
    );

    icons::draw(c, Icon::Start, cx - logo / 2, gy, logo);

    // Wordmark + subtitle.
    let title = "OSJEFF";
    font::draw_text(
        c,
        centered_x(c, title, 6),
        gy + logo + 24,
        title,
        Color::rgb(0xF2, 0xF6, 0xFF),
        6,
    );
    let sub = "Rust Operating System";
    font::draw_text(
        c,
        centered_x(c, sub, 2),
        gy + logo + 88,
        sub,
        Color::rgb(0x7E, 0x8C, 0xA8),
        2,
    );

    // Progress bar.
    let bar_w = (w / 3).max(220);
    let bar_x = (w - bar_w) / 2;
    let bar_y = h * 72 / 100;
    let bar_h = 10usize;
    c.fill_round_rect(
        bar_x,
        bar_y,
        bar_w,
        bar_h,
        bar_h / 2,
        Color::rgb(0x1E, 0x2A, 0x44),
    );
    let p = if progress < 0.0 {
        0.0
    } else if progress > 1.0 {
        1.0
    } else {
        progress
    };
    let fill = (bar_w as f32 * p) as usize;
    if fill > bar_h {
        c.fill_round_rect(
            bar_x,
            bar_y,
            fill,
            bar_h,
            bar_h / 2,
            Color::rgb(0x3B, 0x82, 0xF6),
        );
    }

    let status = "Carregando o sistema...";
    font::draw_text(
        c,
        centered_x(c, status, 2),
        bar_y + 26,
        status,
        Color::rgb(0x9A, 0xA6, 0xBD),
        2,
    );
}
