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
}

pub fn draw(c: &mut Canvas, icon: Icon, x: usize, y: usize, size: usize) {
    match icon {
        Icon::Brand => brand(c, x, y, size),
        Icon::Terminal => terminal(c, x, y, size),
        Icon::Editor => editor(c, x, y, size),
        Icon::TaskMgr => taskmgr(c, x, y, size),
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
