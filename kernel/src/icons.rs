//! Hand-drawn app icons built from framebuffer primitives. Each renders a
//! rounded tile plus a recognizable glyph, sized to fit `size x size`.

use crate::fb::{Canvas, Color};
use crate::theme;

/// Which app an icon represents.
#[derive(Clone, Copy)]
pub enum Icon {
    Brand,
    Terminal,
    Editor,
    TaskMgr,
    Calculator,
    Browser,
    Power,
}

pub fn draw(c: &mut Canvas, icon: Icon, x: usize, y: usize, size: usize) {
    match icon {
        Icon::Brand => brand(c, x, y, size),
        Icon::Terminal => terminal(c, x, y, size),
        Icon::Editor => editor(c, x, y, size),
        Icon::TaskMgr => taskmgr(c, x, y, size),
        Icon::Calculator => calculator(c, x, y, size),
        Icon::Browser => browser(c, x, y, size),
        Icon::Power => power(c, x, y, size),
    }
}

/// OSJeff mark: teal squircle, violet diagonal half, white chevron.
pub fn brand(c: &mut Canvas, x: usize, y: usize, size: usize) {
    let r = size / 4;
    c.fill_round_rect(x, y, size, size, r, theme::ACCENT);
    // Violet lower-right triangle.
    for row in 0..size {
        let start = size.saturating_sub(row);
        if start < size {
            c.fill_rect(x + start, y + row, size - start, 1, theme::ACCENT_2);
        }
    }
    // White ">" chevron.
    let u = (size / 8).max(2);
    let px = x + size / 3;
    let py = y + size / 4;
    for i in 0..3 {
        c.fill_rect(px + i * u, py + i * u, u, u, theme::WHITE);
        c.fill_rect(px + i * u, py + (4 - i) * u, u, u, theme::WHITE);
    }
}

fn terminal(c: &mut Canvas, x: usize, y: usize, size: usize) {
    c.fill_round_rect(x, y, size, size, size / 5, Color::rgb(0x10, 0x18, 0x2A));
    c.fill_round_rect(
        x + 1,
        y + 1,
        size - 2,
        size / 5,
        size / 6,
        Color::rgb(0x1D, 0x28, 0x42),
    );
    let green = theme::ACCENT;
    let u = (size / 10).max(2);
    let px = x + size / 4;
    let py = y + size / 3;
    for i in 0..3 {
        c.fill_rect(px + i * u, py + i * u, u, u, green);
        c.fill_rect(px + i * u, py + (4 - i) * u, u, u, green);
    }
    c.fill_rect(x + size / 2, y + size - size / 3, size / 3, u, green);
}

fn editor(c: &mut Canvas, x: usize, y: usize, size: usize) {
    c.fill_round_rect(x, y, size, size, size / 5, theme::ACCENT);
    let pad = size / 6;
    let pw = size - pad * 2;
    let ph = size - pad * 2;
    c.fill_round_rect(x + pad, y + pad, pw, ph, 3, theme::WINDOW_BODY);
    let line = Color::rgb(0x9A, 0xA6, 0xBD);
    let lh = ph / 5;
    let lx = x + pad + pw / 6;
    for i in 0..3 {
        let ly = y + pad + lh + i * lh;
        let w = if i == 2 { pw / 2 } else { pw - pw / 3 };
        c.fill_rect(lx, ly, w, (lh / 3).max(1), line);
    }
    let f = size / 5;
    c.fill_round_rect(x + size - pad - f, y + pad, f, f, 2, theme::ACCENT);
}

fn calculator(c: &mut Canvas, x: usize, y: usize, size: usize) {
    c.fill_round_rect(x, y, size, size, size / 5, Color::rgb(0x1B, 0x24, 0x3A));
    let pad = (size / 6).max(2);
    let gap = (size / 12).max(1);
    // Screen.
    c.fill_round_rect(x + pad, y + pad, size - pad * 2, size / 5, 2, theme::ACCENT);
    // 3x3 keypad.
    let gy = y + pad + size / 5 + gap;
    let cell = ((size - pad * 2).saturating_sub(2 * gap) / 3).max(1);
    for r in 0..3 {
        for col in 0..3 {
            let bx = x + pad + col * (cell + gap);
            let by = gy + r * (cell + gap);
            let fill = if r == 2 && col == 2 {
                theme::ACCENT_2
            } else {
                theme::WINDOW_BODY
            };
            c.fill_round_rect(bx, by, cell, cell, 2, fill);
        }
    }
}

fn power(c: &mut Canvas, x: usize, y: usize, size: usize) {
    c.fill_round_rect(x, y, size, size, size / 5, Color::rgb(0x2A, 0x12, 0x18));
    let col = theme::CLOSE;
    let cx = x + size / 2;
    let cy = y + size / 2;
    let r = (size / 3).max(3);
    // Ring: filled disc punched hollow with the tile color.
    c.fill_round_rect(cx - r, cy - r, 2 * r, 2 * r, r, col);
    let inner = r.saturating_sub((size / 12).max(2));
    c.fill_round_rect(
        cx - inner,
        cy - inner,
        2 * inner,
        2 * inner,
        inner,
        Color::rgb(0x2A, 0x12, 0x18),
    );
    // Top break + power bar.
    let bw = (size / 10).max(2);
    c.fill_rect(cx - bw / 2, y + size / 6, bw, size / 3, col);
    c.fill_rect(
        cx - bw / 2 - 1,
        y + size / 6,
        bw + 2,
        (size / 12).max(2),
        Color::rgb(0x2A, 0x12, 0x18),
    );
}

/// Globe: teal disc, violet "land" wedge, white meridian + equator lines.
fn browser(c: &mut Canvas, x: usize, y: usize, size: usize) {
    c.fill_round_rect(x, y, size, size, size / 5, Color::rgb(0x10, 0x1A, 0x30));
    let cx = x + size / 2;
    let cy = y + size / 2;
    let r = (size / 2).saturating_sub(size / 8).max(3);
    // Ocean disc.
    c.fill_round_rect(cx - r, cy - r, 2 * r, 2 * r, r, theme::ACCENT);
    // A couple of violet "continents".
    let l = (r / 2).max(2);
    c.fill_round_rect(cx - r + r / 4, cy - r / 2, l, l, l / 2, theme::ACCENT_2);
    c.fill_round_rect(
        cx + r / 6,
        cy,
        (l * 3) / 4,
        (l * 3) / 4,
        l / 3,
        theme::ACCENT_2,
    );
    // Equator + meridian in white.
    let t = (size / 16).max(1);
    c.fill_rect(cx - r, cy - t / 2, 2 * r, t, theme::WHITE);
    c.fill_rect(cx - t / 2, cy - r, t, 2 * r, theme::WHITE);
}

fn taskmgr(c: &mut Canvas, x: usize, y: usize, size: usize) {
    c.fill_round_rect(x, y, size, size, size / 5, Color::rgb(0x1A, 0x16, 0x32));
    let pad = size / 5;
    let bw = (size - pad * 2) / 4;
    let base = y + size - pad;
    let colors = [theme::ACCENT, theme::ACCENT_2, Color::rgb(0xF5, 0x9E, 0x0B)];
    for (i, color) in colors.iter().enumerate() {
        let bh = (size - pad * 2) * (i + 2) / 4;
        let bx = x + pad + i * (bw + bw / 3);
        c.fill_round_rect(bx, base - bh, bw, bh, 2, *color);
    }
}
