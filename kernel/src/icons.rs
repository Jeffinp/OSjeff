//! Hand-drawn app icons built from framebuffer primitives. Each renders a
//! rounded tile plus a recognizable glyph, sized to fit `size x size`.

use crate::fb::{Canvas, Color};

/// Which app an icon represents.
#[derive(Clone, Copy)]
pub enum Icon {
    Start,
    Terminal,
    Editor,
    TaskMgr,
}

pub fn draw(c: &mut Canvas, icon: Icon, x: usize, y: usize, size: usize) {
    match icon {
        Icon::Start => start(c, x, y, size),
        Icon::Terminal => terminal(c, x, y, size),
        Icon::Editor => editor(c, x, y, size),
        Icon::TaskMgr => taskmgr(c, x, y, size),
    }
}

/// Windows-style four squares.
fn start(c: &mut Canvas, x: usize, y: usize, size: usize) {
    let blue = Color::rgb(0x2D, 0x7D, 0xF6);
    let gap = size / 11;
    let s = (size - gap) / 2;
    for (dx, dy) in [(0, 0), (s + gap, 0), (0, s + gap), (s + gap, s + gap)] {
        c.fill_round_rect(x + dx, y + dy, s, s, 2, blue);
    }
}

/// Dark terminal tile with a `>` prompt and a blinking-style underscore.
fn terminal(c: &mut Canvas, x: usize, y: usize, size: usize) {
    c.fill_round_rect(x, y, size, size, size / 5, Color::rgb(0x14, 0x1F, 0x33));
    c.fill_round_rect(
        x + 1,
        y + 1,
        size - 2,
        size / 5,
        size / 6,
        Color::rgb(0x24, 0x32, 0x4D),
    );
    let green = Color::rgb(0x4A, 0xD9, 0x7B);
    let u = (size / 10).max(2); // stroke unit
    let px = x + size / 4;
    let py = y + size / 3;
    // ">" chevron from two strokes.
    for i in 0..3 {
        c.fill_rect(px + i * u, py + i * u, u, u, green);
    }
    for i in 0..3 {
        c.fill_rect(px + i * u, py + (4 - i) * u, u, u, green);
    }
    // Prompt underscore.
    c.fill_rect(x + size / 2, y + size - size / 3, size / 3, u, green);
}

/// White page with text lines and a folded corner.
fn editor(c: &mut Canvas, x: usize, y: usize, size: usize) {
    c.fill_round_rect(x, y, size, size, size / 5, Color::rgb(0x2E, 0x6B, 0xE6));
    let pad = size / 6;
    let pw = size - pad * 2;
    let ph = size - pad * 2;
    c.fill_round_rect(x + pad, y + pad, pw, ph, 3, Color::rgb(0xFA, 0xFB, 0xFE));
    // Text lines.
    let line = Color::rgb(0x9A, 0xA6, 0xBD);
    let lh = ph / 5;
    let lx = x + pad + pw / 6;
    for i in 0..3 {
        let ly = y + pad + lh + i * lh;
        let w = if i == 2 { pw / 2 } else { pw - pw / 3 };
        c.fill_rect(lx, ly, w, (lh / 3).max(1), line);
    }
    // Folded corner.
    let f = size / 5;
    c.fill_round_rect(
        x + size - pad - f,
        y + pad,
        f,
        f,
        2,
        Color::rgb(0x2E, 0x6B, 0xE6),
    );
}

/// Dark tile with an ascending bar chart.
fn taskmgr(c: &mut Canvas, x: usize, y: usize, size: usize) {
    c.fill_round_rect(x, y, size, size, size / 5, Color::rgb(0x23, 0x2B, 0x3D));
    let pad = size / 5;
    let bw = (size - pad * 2) / 4;
    let base = y + size - pad;
    let colors = [
        Color::rgb(0x3B, 0x82, 0xF6),
        Color::rgb(0x22, 0xC5, 0x5E),
        Color::rgb(0xF5, 0x9E, 0x0B),
    ];
    for (i, color) in colors.iter().enumerate() {
        let bh = (size - pad * 2) * (i + 2) / 4;
        let bx = x + pad + i * (bw + bw / 3);
        c.fill_round_rect(bx, base - bh, bw, bh, 2, *color);
    }
}
