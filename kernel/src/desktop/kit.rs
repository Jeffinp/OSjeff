//! Toolkit extras for the wave-2 system apps (Tarefas, Ajustes, Registro, Calculadora):
//! a smooth history chart with a hover value, a usage bar and a pressure gauge, chips,
//! search field, cards and a few drawing helpers. Everything follows the palette of the
//! current appearance and measures its text with the real font.
//!
//! Like `ui.rs`, each function draws at a rectangle the caller computed; geometry that
//! hit testing shares (`plot_of`) is a plain function.

#![allow(dead_code)]

use super::ui;
use super::*;
use crate::text::{self, BODY, CAPTION, FOOTNOTE, TITLE2, Weight};
use kitsune_core::activity::{self, slice_at};
use kitsune_core::iconart::Glyph;
use kitsune_core::sysmon::HIST;

/// The text of a formatting buffer.
pub(crate) fn fb_str<const N: usize>(b: &kitsune_core::klog::FixedBuf<N>) -> &str {
    activity::text(b)
}

/// Linear interpolation of two integers (`t` 0..=256).
pub(crate) fn lerp(a: i64, b: i64, t: i64) -> i64 {
    a + (b - a) * t.clamp(0, 256) / 256
}

/// Ease-out cubic of `t` (0..=256), result 0..=256: the curve every 1 Hz update uses.
pub(crate) fn ease_out(t: i64) -> i64 {
    let u = 256 - t.clamp(0, 256);
    256 - (u * u / 256) * u / 256
}

/// Intersect the canvas clip with `r`; give the old state back to `restore_clip`.
pub(crate) fn clip_to(c: &mut Canvas, r: Rect) -> crate::fb::ClipState {
    let cur = c.clip_rect();
    c.set_clip(r.intersection(&cur).unwrap_or(Rect::new(0, 0, 0, 0)))
}

/// A card: the content colour on the window colour, with a hairline.
pub(crate) fn card(c: &mut Canvas, r: Rect) {
    let p = theme::pal();
    ui::fill_token(c, r, 12, p.content_bg);
    ui::stroke_token(c, r, 12, p.separator);
}

/// The semantic colours of the monitors.
pub(crate) fn green() -> Color {
    Color::rgb(0x34, 0xC7, 0x59)
}
pub(crate) fn amber() -> Color {
    Color::rgb(0xFF, 0x9F, 0x0A)
}
pub(crate) fn red() -> Color {
    Color::rgb(0xFF, 0x45, 0x3A)
}
pub(crate) fn blue() -> Color {
    Color::rgb(0x0A, 0x84, 0xFF)
}

/// Text in the primary / secondary / tertiary colour.
pub(crate) fn ink() -> Color {
    theme::text()
}
pub(crate) fn ink2() -> Color {
    theme::text_muted()
}
pub(crate) fn ink3() -> Color {
    theme::solid(theme::pal().text_tertiary)
}

/// A caption-sized label in the secondary colour at `(x, y)` (top of the line box).
pub(crate) fn label(c: &mut Canvas, x: i32, y: i32, max_w: i32, t: &str) {
    text::draw_ellipsis(c, x, y, max_w, t, FOOTNOTE, Weight::Regular, ink2());
}

/// A big number with a unit-less label under it, as the stat cards show.
pub(crate) fn big_number(c: &mut Canvas, x: i32, y: i32, max_w: i32, value: &str) {
    text::draw_ellipsis(c, x, y, max_w, value, TITLE2, Weight::Semibold, ink());
}

/// A label / value pair stacked: the label (footnote, secondary) over the value (body).
pub(crate) fn stat(c: &mut Canvas, x: i32, y: i32, w: i32, name: &str, value: &str) {
    label(c, x, y, w, name);
    text::draw_ellipsis(c, x, y + 17, w, value, BODY, Weight::Medium, ink());
}

/// A `name .... value` line: name left (secondary), value right (primary), `h` high.
pub(crate) fn kv(c: &mut Canvas, r: Rect, name: &str, value: &str) {
    let vw = text::measure(value, BODY, Weight::Regular).min(r.w * 3 / 4);
    text::draw_left(
        c,
        Rect::new(r.x, r.y, r.w - vw - 12, r.h),
        name,
        BODY,
        Weight::Regular,
        ink2(),
    );
    text::draw_right(
        c,
        Rect::new(r.right() - vw - 2, r.y, vw + 2, r.h),
        value,
        BODY,
        Weight::Regular,
        ink(),
    );
}

/// A track with a rounded fill: `v_q8` is the fill in thousandths times 256 (so a bar can
/// glide between samples). `color` fills; the track is a faint wash.
pub(crate) fn bar(c: &mut Canvas, r: Rect, v_q8: i64, color: Color) {
    let rad = r.h / 2;
    ui::fill_token(
        c,
        r,
        rad,
        if theme::dark() {
            0x33FF_FFFF
        } else {
            0x1A00_0000
        },
    );
    let w = (r.w as i64 * v_q8.clamp(0, 1000 * 256) / (1000 * 256)) as i32;
    if w > 0 {
        c.fill_rrect(
            Rect::new(r.x, r.y, w.max(r.h), r.h),
            rad,
            Corner::Circle,
            color,
            256,
        );
    }
}

/// The memory-pressure gauge: three zones (normal, attention, critical) with a knob at
/// `pm_q8` (thousandths times 256).
pub(crate) fn pressure_gauge(c: &mut Canvas, r: Rect, pm_q8: i64) {
    let gap = 3;
    let zones = [(0, 600, green()), (600, 850, amber()), (850, 1000, red())];
    for (i, (a, b, col)) in zones.iter().enumerate() {
        let x0 = r.x + (r.w as i64 * *a / 1000) as i32 + if i > 0 { gap / 2 } else { 0 };
        let x1 = r.x + (r.w as i64 * *b / 1000) as i32 - if i < 2 { gap / 2 } else { 0 };
        let seg = Rect::new(x0, r.y, (x1 - x0).max(2), r.h);
        c.fill_rrect(seg, r.h / 2, Corner::Circle, *col, 230);
    }
    let kx = r.x + (r.w as i64 * pm_q8.clamp(0, 1000 * 256) / (1000 * 256)) as i32;
    let knob = Rect::new(kx - 8, r.y + r.h / 2 - 8, 16, 16);
    c.draw_shadow(
        knob,
        Shadow {
            blur: 4,
            dy: 1,
            alpha: 90,
        },
        Rect::new(knob.x, knob.y + 3, knob.w, knob.h - 6),
    );
    c.fill_rrect(knob, 8, Corner::Circle, Color::rgb(0xFF, 0xFF, 0xFF), 256);
    c.stroke_rrect(knob, 8, Corner::Circle, Color::rgb(0, 0, 0), 40);
}

/// A small coloured pill with `t` inside (log levels, link state). Returns its width.
pub(crate) fn chip(c: &mut Canvas, x: i32, y: i32, h: i32, t: &str, color: Color) -> i32 {
    let w = text::measure(t, CAPTION, Weight::Semibold) + 14;
    let r = Rect::new(x, y, w, h);
    c.fill_rrect(
        r,
        h / 2,
        Corner::Circle,
        color,
        if theme::dark() { 56 } else { 40 },
    );
    let fg = if theme::dark() {
        color.lerp(Color::rgb(0xFF, 0xFF, 0xFF), 70)
    } else {
        color.lerp(Color::rgb(0, 0, 0), 90)
    };
    text::draw_centered(c, r, t, CAPTION, Weight::Semibold, fg);
    w
}

/// A search field: rounded frame, magnifier, text or placeholder, caret.
pub(crate) fn search_field(
    c: &mut Canvas,
    r: Rect,
    t: &str,
    placeholder: &str,
    focused: bool,
    caret: bool,
) {
    ui::text_field_frame(c, r, r.h / 2, focused);
    ui::draw_glyph(
        c,
        Glyph::Search,
        r.x + 9,
        r.y + (r.h - 14) / 2,
        14,
        0xFF00_0000 | pack(ink3()),
    );
    let tx = r.x + 30;
    let tw = r.w - 30 - 10;
    let ty = text::center_y(r.y, r.h, BODY, Weight::Regular);
    if t.is_empty() {
        text::draw_ellipsis(c, tx, ty, tw, placeholder, BODY, Weight::Regular, ink3());
        if caret {
            ui::fill(c, Rect::new(tx, r.y + 6, 1, r.h - 12), theme::accent());
        }
        return;
    }
    // The tail of a long query stays visible.
    let mut start = 0usize;
    while text::measure(&t[start..], BODY, Weight::Regular) > tw && start < t.len() {
        start += t[start..].chars().next().map_or(1, char::len_utf8);
    }
    let w = text::draw(c, tx, ty, &t[start..], BODY, Weight::Regular, ink());
    if caret {
        ui::fill(
            c,
            Rect::new(tx + w + 1, r.y + 6, 1, r.h - 12),
            theme::accent(),
        );
    }
}

fn pack(c: Color) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
}

/// Pack a colour as an opaque ARGB value for [`ui::draw_glyph`].
pub(crate) fn argb(c: Color) -> u32 {
    0xFF00_0000 | pack(c)
}

/// Is a button of the toolkit being pressed? (`hover` and `down` come from the window's
/// hover key.)
pub(crate) fn control_state(hover: bool, down: bool, enabled: bool) -> ui::Control {
    if !enabled {
        ui::Control::Disabled
    } else if hover && down {
        ui::Control::Pressed
    } else if hover {
        ui::Control::Hover
    } else {
        ui::Control::Normal
    }
}

// ------------------------------------------------------------------ chart

/// One curve of a [`Chart`]: samples oldest first, already smoothed.
pub(crate) struct Curve<'a> {
    pub data: &'a [u32],
    pub color: Color,
}

/// What a chart shows.
pub(crate) struct Chart<'a> {
    pub curves: &'a [Curve<'a>],
    /// Value at the top of the axis.
    pub ceiling: u64,
    /// Axis labels, top / middle / bottom.
    pub y_labels: [&'a str; 3],
    /// Progress (0..=256) of the 1 Hz scroll animation; 256 at rest.
    pub t_q8: i32,
    /// Sample index under the pointer, and the text for its bubble.
    pub hover: Option<usize>,
    pub tip: &'a str,
}

/// The plot area inside a chart card: room for the axis labels on the left and the time
/// labels below.
pub(crate) fn plot_of(card: Rect) -> Rect {
    Rect::new(card.x + 68, card.y + 14, card.w - 68 - 16, card.h - 14 - 26)
}

/// Vertical position (Q8 pixels from the top of the plot) of a value.
fn y_q8(v: u32, ceiling: u64, h: i32) -> i64 {
    let span = ((h - 1).max(1) as i64) << 8;
    let v = (v as u64).min(ceiling.max(1)) as i64;
    span - span * v / ceiling.max(1) as i64
}

/// Position, in Q8 sample indices, that the right edge of the plot shows.
fn right_edge_q8(n: usize, t_q8: i32) -> i64 {
    if n < 2 {
        return 0;
    }
    if t_q8 >= 256 {
        ((n - 1) as i64) << 8
    } else {
        ((n - 2) as i64) * 256 + t_q8.clamp(0, 256) as i64
    }
}

/// Column `px` of a plot `w` wide: the (Q8) sample position it shows.
fn col_pos(px: i32, w: i32, right: i64) -> i64 {
    let span = ((HIST - 1) as i64) << 8;
    right - ((w - 1 - px) as i64 * span) / (w - 1).max(1) as i64
}

/// Draw a vertical run of one pixel column from Q8 `lo` to `hi` (exclusive), edges
/// anti-aliased by their coverage.
fn vrun(c: &mut Canvas, x: i32, lo: i64, hi: i64, plot: Rect, color: Color, alpha: u32) {
    let (lo, hi) = (lo.max(0), hi.min((plot.h as i64) << 8));
    if hi <= lo {
        return;
    }
    let r0 = (lo >> 8) as i32;
    let r1 = ((hi - 1) >> 8) as i32;
    let px = |c: &mut Canvas, row: i32, cover: i64| {
        let a = (alpha as i64 * cover / 256) as u16;
        if a > 0 {
            c.blend_rect(Rect::new(x, plot.y + row, 1, 1), color, a);
        }
    };
    if r0 == r1 {
        px(c, r0, hi - lo);
        return;
    }
    px(c, r0, 256 - (lo & 255));
    if r1 > r0 + 1 {
        c.blend_rect(
            Rect::new(x, plot.y + r0 + 1, 1, r1 - r0 - 1),
            color,
            alpha as u16,
        );
    }
    px(c, r1, ((hi - 1) & 255) + 1);
}

fn draw_curve(c: &mut Canvas, plot: Rect, cv: &Curve<'_>, ceiling: u64, t_q8: i32) {
    let n = cv.data.len();
    if n < 2 || plot.w < 4 || plot.h < 4 {
        return;
    }
    let right = right_edge_q8(n, t_q8);
    let h = plot.h;
    let bands = 5i32;
    let base_alpha: i64 = if theme::dark() { 120 } else { 96 };
    let mut y_prev: Option<i64> = None;
    for px in 0..plot.w {
        let pos = col_pos(px, plot.w, right);
        let Some(v) = (pos >= 0).then(|| slice_at(cv.data, pos)).flatten() else {
            continue;
        };
        let y = y_q8(v, ceiling, h);
        let x = plot.x + px;
        // Area under the curve, in a few absolute bands that fade downwards.
        let first = (y >> 8) as i32 + 1;
        for b in 0..bands {
            let b0 = h * b / bands;
            let b1 = h * (b + 1) / bands;
            let (s, e) = (first.max(b0), b1);
            if e > s {
                let a = base_alpha * (bands - b) as i64 / bands as i64 * 3 / 5;
                c.blend_rect(Rect::new(x, plot.y + s, 1, e - s), cv.color, a as u16);
            }
        }
        // The line: a 1.5 px stroke joining this column to the previous one.
        let ya = y_prev.unwrap_or(y);
        let (lo, hi) = (ya.min(y) - 256, ya.max(y) + 256);
        vrun(c, x, lo, hi, plot, cv.color, 256);
        y_prev = Some(y);
    }
}

/// A chart card: grid, axis labels, the curves (smoothly scrolling), a hover guide and
/// bubble. Returns the plot rectangle.
pub(crate) fn chart(c: &mut Canvas, r: Rect, ch: &Chart<'_>) -> Rect {
    card(c, r);
    let p = theme::pal();
    let plot = plot_of(r);
    if plot.w < 8 || plot.h < 8 {
        return plot;
    }
    // Grid and the axis labels.
    for i in 0..3i32 {
        let y = plot.y + (plot.h - 1) * i / 2;
        ui::fill_token(c, Rect::new(plot.x, y, plot.w, 1), 0, p.separator);
        let ly = text::center_y(y - 8, 16, CAPTION, Weight::Regular);
        let lab = ch.y_labels[i as usize];
        let lw = text::measure(lab, CAPTION, Weight::Regular);
        text::draw(
            c,
            plot.x - 8 - lw,
            ly,
            lab,
            CAPTION,
            Weight::Regular,
            ink3(),
        );
    }
    let ty = r.bottom() - 20;
    text::draw(c, plot.x, ty, "60 s", CAPTION, Weight::Regular, ink3());
    let now = "agora";
    let nw = text::measure(now, CAPTION, Weight::Regular);
    text::draw(
        c,
        plot.right() - nw,
        ty,
        now,
        CAPTION,
        Weight::Regular,
        ink3(),
    );

    let saved = clip_to(c, plot.inflated(0));
    for cv in ch.curves {
        draw_curve(c, plot, cv, ch.ceiling, ch.t_q8);
    }
    c.restore_clip(saved);

    // Hover: a guide line, a dot on every curve and the bubble.
    if let Some(i) = ch.hover {
        let n = ch.curves.first().map_or(0, |cv| cv.data.len());
        if i < n {
            let right = right_edge_q8(n, 256);
            let span = ((HIST - 1) as i64) << 8;
            let x = plot.x
                + ((plot.w - 1) as i64 - ((right - ((i as i64) << 8)) * (plot.w - 1) as i64) / span)
                    as i32;
            ui::fill_token(
                c,
                Rect::new(x, plot.y, 1, plot.h),
                0,
                if theme::dark() {
                    0x66FF_FFFF
                } else {
                    0x5500_0000
                },
            );
            let mut top_y = plot.bottom();
            for cv in ch.curves {
                let Some(&v) = cv.data.get(i) else { continue };
                let y = plot.y + (y_q8(v, ch.ceiling, plot.h) >> 8) as i32;
                top_y = top_y.min(y);
                c.fill_rrect(
                    Rect::new(x - 5, y - 5, 10, 10),
                    5,
                    Corner::Circle,
                    Color::rgb(0xFF, 0xFF, 0xFF),
                    256,
                );
                c.fill_rrect(
                    Rect::new(x - 3, y - 3, 6, 6),
                    3,
                    Corner::Circle,
                    cv.color,
                    256,
                );
            }
            if !ch.tip.is_empty() {
                let below = top_y - 12 - 24 < r.y + 2;
                let bottom = if below { top_y + 12 + 24 } else { top_y - 12 };
                ui::tooltip(c, x, bottom, ch.tip);
            }
        }
    }
    plot
}

/// A push button with a glyph in front of its label.
pub(crate) fn icon_button(
    c: &mut Canvas,
    r: Rect,
    g: Glyph,
    label: &str,
    kind: ui::ButtonKind,
    st: ui::Control,
) {
    ui::push_button(c, r, "", kind, st);
    let fg = if kind == ui::ButtonKind::Secondary {
        ink()
    } else {
        theme::ACCENT_TEXT
    };
    let w = text::measure(label, BODY, Weight::Regular);
    let x = r.x + (r.w - (16 + 6 + w)) / 2;
    let a = if st == ui::Control::Disabled {
        110
    } else {
        256
    };
    let s = crate::glyphs::get(g, 16, argb(fg));
    c.blit_surface(s, x, r.y + (r.h - 16) / 2, a);
    text::draw_a(
        c,
        x + 22,
        text::center_y(r.y, r.h, BODY, Weight::Regular),
        label,
        BODY,
        Weight::Regular,
        fg,
        a as u16,
    );
}

/// A tiny up / down arrow for a sorted table column.
pub(crate) fn sort_arrow(c: &mut Canvas, x: i32, y: i32, descending: bool, color: Color) {
    ui::draw_glyph(
        c,
        if descending {
            Glyph::ChevronDown
        } else {
            Glyph::ChevronUp
        },
        x,
        y,
        10,
        argb(color),
    );
}

/// A hairline across `r`'s width at its vertical middle.
pub(crate) fn hairline(c: &mut Canvas, x: i32, y: i32, w: i32) {
    ui::separator(c, Rect::new(x, y, w, 1));
}
