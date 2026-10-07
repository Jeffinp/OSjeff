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
    Disabled,
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
        Btn::Disabled => (Color::rgb(0xEC, 0xEF, 0xF6), Color::rgb(0xA6, 0xAE, 0xC0)),
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

/// One line of a graph; a non-empty `label` adds it to the legend.
pub(crate) struct Line<'a> {
    pub series: &'a osjeff_core::sysmon::Series,
    pub color: Color,
    pub label: &'a [u8],
}

/// What a [`graph`] panel shows.
pub(crate) struct Graph<'a> {
    pub title: &'a [u8],
    /// Headline value, right-aligned on the title row (accent colour).
    pub value: &'a [u8],
    /// Dim text under the title.
    pub sub: &'a [u8],
    pub lines: &'a [Line<'a>],
    /// Top of the Y axis (the value drawn at the panel's plot top).
    pub ceiling: u64,
    /// Text printed at the top of the axis.
    pub ceiling_label: &'a [u8],
}

/// A dark panel: `title` (left) and `value` (right, accent) on the first row,
/// `sub` (dim) under it, then a faint grid with one line per series on a
/// `0..ceiling` axis (`ceiling_label` is printed at the top of the axis), and
/// the legend along the bottom.
pub(crate) fn graph(c: &mut Canvas, r: Rect, g: &Graph<'_>) {
    let (title, value, sub, lines, ceiling, ceiling_label) =
        (g.title, g.value, g.sub, g.lines, g.ceiling, g.ceiling_label);
    use osjeff_core::sysmon::{HIST, scale_to};
    fill_round(c, r, 10, PANEL_DARK);
    let inner = r.w - 20;
    text(c, r.x + 10, r.y + 8, inner, title, theme::HEADER_TEXT);
    text_right(
        c,
        Rect::new(r.x, r.y + 8, r.w - 10, CELL_H),
        value,
        theme::accent(),
    );
    text(c, r.x + 10, r.y + 26, inner, sub, DIM_TEXT);
    let plot = Rect::new(r.x + 10, r.y + 46, r.w - 20, r.h - 46 - 26);
    if plot.w <= 4 || plot.h <= 4 {
        return;
    }
    for i in 0..=4 {
        let y = plot.y + plot.h * i / 4;
        fill(
            c,
            Rect::new(plot.x, y.min(plot.bottom() - 1), plot.w, 1),
            GRID,
        );
    }
    text_right(
        c,
        Rect::new(plot.x, plot.y + 2, plot.w - 2, CELL_H),
        ceiling_label,
        DIM_TEXT,
    );
    for line in lines {
        let n = line.series.len();
        if n < 2 {
            continue;
        }
        let den = (HIST - 1) as i32;
        let mut prev: Option<(i32, i32)> = None;
        for i in 0..n {
            let v = line.series.get(i).unwrap_or(0) as u64;
            // The newest sample sits at the right edge; a short history is
            // right-aligned, so the line grows leftwards like a task manager.
            let slot = (HIST - n + i) as i32;
            let x = plot.x + (plot.w - 2) * slot / den;
            let h = scale_to(v, ceiling, plot.h as u32) as i32;
            let y = plot.bottom() - 2 - h.min(plot.h - 2);
            if let Some((px, py)) = prev {
                segment(c, px, py, x, y, line.color);
            }
            prev = Some((x, y));
        }
    }
    // Legend.
    let mut lx = r.x + 10;
    let ly = r.bottom() - 20;
    for line in lines.iter().filter(|l| !l.label.is_empty()) {
        // Scale-1 text (6 px per character) keeps four or five labels on one row.
        let w = line.label.len() as i32 * 6 + 14;
        if lx + w > r.right() - 6 {
            break;
        }
        fill_round(c, Rect::new(lx, ly + 3, 8, 8), 4, line.color);
        font::draw_bytes(c, us(lx + 12), us(ly + 3), line.label, DIM_TEXT, 1);
        lx += w + 10;
    }
}

/// A 2-pixel-thick line between two points (graph segments are near-horizontal,
/// so a column-wise walk is enough and never divides by zero).
fn segment(c: &mut Canvas, x0: i32, y0: i32, x1: i32, y1: i32, color: Color) {
    let dx = (x1 - x0).max(1);
    let (ylo, yhi) = (y0.min(y1), y0.max(y1));
    for x in x0..=x1 {
        let y = y0 + (y1 - y0) * (x - x0) / dx;
        // Steep segments get a vertical run so the line stays connected.
        let next = y0 + (y1 - y0) * (x + 1 - x0) / dx;
        let (a, b) = if x == x1 {
            (y, y)
        } else {
            (y.min(next), y.max(next))
        };
        c.fill_rect(
            us(x),
            us(a.clamp(ylo, yhi)),
            2,
            (b.clamp(ylo, yhi) - a.clamp(ylo, yhi) + 2) as usize,
            color,
        );
    }
}

/// A horizontal usage bar: `permille` (0..=1000) of `track` filled.
pub(crate) fn usage_bar(c: &mut Canvas, track: Rect, permille: u32, color: Color) {
    fill_round(c, track, track.h / 2, GRID);
    let w = (track.w as i64 * permille.min(1000) as i64 / 1000) as i32;
    if w > 0 {
        fill_round(
            c,
            Rect::new(track.x, track.y, w.max(track.h), track.h),
            track.h / 2,
            color,
        );
    }
}
