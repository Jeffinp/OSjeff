//! Small widgets shared by the system-management apps (log viewer, resource
//! monitor, settings): buttons, clipped text, panels, a text-input box, a
//! scrollbar and a line graph. Pure drawing helpers over [`Canvas`]; each app
//! keeps its own layout function so drawing and hit-testing share one geometry.

use super::*;

pub(crate) const PANEL_DARK: Color = Color::rgb(0x0E, 0x16, 0x28);
pub(crate) const BUTTON: Color = Color::rgb(0xD8, 0xDF, 0xEE);
pub(crate) const BORDER: Color = Color::rgb(0xCE, 0xD6, 0xE6);
pub(crate) const GRID: Color = Color::rgb(0x23, 0x2E, 0x4A);
pub(crate) const DIM_TEXT: Color = Color::rgb(0x8C, 0x9A, 0xB6);

/// Glyph cell of the UI font (scale 2).
pub(crate) const CELL_W: i32 = 12;
pub(crate) const CELL_H: i32 = 14;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Btn {
    Normal,
    /// Selected / toggled on: filled with the accent colour.
    On,
}

fn us(v: i32) -> usize {
    v.max(0) as usize
}

/// Draw `text` (scale 2) at `(x, y)`, cut to `max_w` pixels.
pub(crate) fn text(c: &mut Canvas, x: i32, y: i32, max_w: i32, t: &[u8], color: Color) {
    let n = (max_w.max(0) / CELL_W) as usize;
    font::draw_bytes(c, us(x), us(y), &t[..t.len().min(n)], color, 2);
}

/// Text centered in `r`.
pub(crate) fn text_center(c: &mut Canvas, r: Rect, t: &[u8], color: Color) {
    let n = ((r.w / CELL_W).max(0) as usize).min(t.len());
    let w = n as i32 * CELL_W;
    font::draw_bytes(
        c,
        us(r.x + (r.w - w) / 2),
        us(r.y + (r.h - CELL_H) / 2),
        &t[..n],
        color,
        2,
    );
}

/// Text right-aligned to `r`'s right edge.
pub(crate) fn text_right(c: &mut Canvas, r: Rect, t: &[u8], color: Color) {
    let n = ((r.w / CELL_W).max(0) as usize).min(t.len());
    let w = n as i32 * CELL_W;
    font::draw_bytes(
        c,
        us(r.right() - w),
        us(r.y + (r.h - CELL_H) / 2),
        &t[..n],
        color,
        2,
    );
}

pub(crate) fn fill_round(c: &mut Canvas, r: Rect, radius: i32, color: Color) {
    if r.w > 0 && r.h > 0 {
        c.fill_round_rect(us(r.x), us(r.y), us(r.w), us(r.h), us(radius), color);
    }
}

pub(crate) fn fill(c: &mut Canvas, r: Rect, color: Color) {
    if r.w > 0 && r.h > 0 {
        c.fill_rect(us(r.x), us(r.y), us(r.w), us(r.h), color);
    }
}

/// A push button with a centered label.
pub(crate) fn button(c: &mut Canvas, r: Rect, label: &[u8], state: Btn) {
    let (bg, fg) = match state {
        Btn::Normal => (BUTTON, theme::TEXT),
        Btn::On => (theme::accent(), Color::rgb(0x08, 0x12, 0x1E)),
    };
    fill_round(c, r, 8, bg);
    text_center(c, r, label, fg);
}

/// A single-line text box: white pill with a border, the text (or a muted
/// placeholder) and, when `caret` is set, a caret after the text.
pub(crate) fn input_box(c: &mut Canvas, r: Rect, t: &[u8], placeholder: &[u8], caret: bool) {
    fill_round(c, r, 8, BORDER);
    fill_round(
        c,
        Rect::new(r.x + 1, r.y + 1, r.w - 2, r.h - 2),
        7,
        theme::WHITE,
    );
    let inner = r.w - 16;
    if t.is_empty() {
        text(
            c,
            r.x + 8,
            r.y + (r.h - CELL_H) / 2,
            inner,
            placeholder,
            theme::TEXT_MUTED,
        );
        if caret {
            fill(c, Rect::new(r.x + 8, r.y + 6, 2, r.h - 12), theme::accent());
        }
        return;
    }
    // Show the tail when the text is wider than the box.
    let cols = (inner / CELL_W).max(1) as usize;
    let tail = &t[t.len().saturating_sub(cols)..];
    text(
        c,
        r.x + 8,
        r.y + (r.h - CELL_H) / 2,
        inner,
        tail,
        theme::TEXT,
    );
    if caret {
        let cx = r.x + 8 + tail.len() as i32 * CELL_W;
        fill(c, Rect::new(cx, r.y + 6, 2, r.h - 12), theme::accent());
    }
}

/// A vertical scrollbar in `track`: thumb sized and placed for `top` of
/// `total` lines with `rows` visible.
pub(crate) fn scrollbar(c: &mut Canvas, track: Rect, top: usize, total: usize, rows: usize) {
    fill_round(c, track, 3, GRID);
    if total <= rows || track.h <= 0 {
        return;
    }
    let th = ((track.h as i64 * rows as i64) / total as i64).max(16) as i32;
    let th = th.min(track.h);
    let max_top = (total - rows) as i64;
    let ty = track.y + ((track.h - th) as i64 * top.min(total - rows) as i64 / max_top) as i32;
    fill_round(c, Rect::new(track.x, ty, track.w, th), 3, theme::accent());
}

/// Map a click `y` inside a scrollbar `track` to a first-visible line.
pub(crate) fn scrollbar_pos(track: Rect, y: i32, total: usize, rows: usize) -> usize {
    if total <= rows || track.h <= 0 {
        return 0;
    }
    let frac = (y - track.y).clamp(0, track.h) as i64;
    ((total - rows) as i64 * frac / track.h as i64) as usize
}
