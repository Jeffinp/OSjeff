//! Small widgets shared by the system-management apps (log viewer, resource
//! monitor, settings): buttons, clipped text, panels, a text-input box, a
//! scrollbar and a line graph. Pure drawing helpers over [`Canvas`]; each app
//! keeps its own layout function so drawing and hit-testing share one geometry.

// Toolkit: the widget section at the bottom is the API wave 2 builds on; not every widget is used yet.
#![allow(dead_code)]

use crate::desktop::*;
use kitsune_core::iconart;

pub(crate) const PANEL_DARK: Color = Color::rgb(0x0E, 0x16, 0x28);
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

/// Draw `text` at `(x, y)` (the top of a [`CELL_H`]-high row), cut with an ellipsis to
/// `max_w` pixels. Proportional 13 px UI font; `t` is UTF-8 (or Latin-1) bytes.
pub(crate) fn text(c: &mut Canvas, x: i32, y: i32, max_w: i32, t: &[u8], color: Color) {
    let s = crate::text::from_bytes(t);
    let ty = crate::text::center_y(y, CELL_H, crate::text::BODY, crate::text::Weight::Regular);
    crate::text::draw_ellipsis(
        c,
        x,
        ty,
        max_w.max(0),
        &s,
        crate::text::BODY,
        crate::text::Weight::Regular,
        color,
    );
}

/// Text centered in `r`.
pub(crate) fn text_center(c: &mut Canvas, r: Rect, t: &[u8], color: Color) {
    let s = crate::text::from_bytes(t);
    crate::text::draw_centered(
        c,
        r,
        &s,
        crate::text::BODY,
        crate::text::Weight::Regular,
        color,
    );
}

/// Text right-aligned to `r`'s right edge.
pub(crate) fn text_right(c: &mut Canvas, r: Rect, t: &[u8], color: Color) {
    let s = crate::text::from_bytes(t);
    crate::text::draw_right(
        c,
        r,
        &s,
        crate::text::BODY,
        crate::text::Weight::Regular,
        color,
    );
}

pub(crate) fn fill_round(c: &mut Canvas, r: Rect, radius: i32, color: Color) {
    if r.w > 0 && r.h > 0 {
        c.fill_rrect(r, radius, Corner::Circle, color, 256);
    }
}

pub(crate) fn fill(c: &mut Canvas, r: Rect, color: Color) {
    if r.w > 0 && r.h > 0 {
        c.fill_rect(us(r.x), us(r.y), us(r.w), us(r.h), color);
    }
}

/// A push button with a centered label (legacy light-surface look; new code uses
/// [`push_button`], which follows the appearance).
pub(crate) fn button(c: &mut Canvas, r: Rect, label: &[u8], state: Btn) {
    let kind = match state {
        Btn::On => ButtonKind::Primary,
        _ => ButtonKind::Secondary,
    };
    let st = if state == Btn::Disabled {
        Control::Disabled
    } else {
        Control::Normal
    };
    draw_button(
        c,
        r,
        &crate::text::from_bytes(label),
        kind,
        st,
        theme::pal(),
    );
}

/// A single-line text box: white field with a hairline (accent ring when the caret is
/// on), the text (or a muted placeholder) and a caret after the text. Legacy light look.
pub(crate) fn input_box(c: &mut Canvas, r: Rect, t: &[u8], placeholder: &[u8], caret: bool) {
    draw_field(
        c,
        r,
        &crate::text::from_bytes(t),
        &crate::text::from_bytes(placeholder),
        caret,
        caret,
        theme::pal(),
    );
}

/// A vertical scrollbar in `track`: a thin rounded thumb sized and placed for `top`
/// of `total` lines with `rows` visible (legacy signature; always visible).
pub(crate) fn scrollbar(c: &mut Canvas, track: Rect, top: usize, total: usize, rows: usize) {
    if total <= rows || track.h <= 0 {
        return;
    }
    let (off, len) = kitsune_core::widgets::scroll_thumb(track.h, total, rows, top, 24);
    let w = 6.min(track.w);
    let thumb = Rect::new(track.right() - w - 1, track.y + off, w, len);
    c.fill_rrect(
        thumb,
        w / 2,
        Corner::Circle,
        Color::rgb(0x6E, 0x6E, 0x73),
        150,
    );
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
    pub series: &'a kitsune_core::sysmon::Series,
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
    use kitsune_core::sysmon::{HIST, scale_to};
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
        // Caption text keeps four or five labels on one row.
        let label = crate::text::from_bytes(line.label);
        let w =
            crate::text::measure(&label, crate::text::CAPTION, crate::text::Weight::Regular) + 14;
        if lx + w > r.right() - 6 {
            break;
        }
        fill_round(c, Rect::new(lx, ly + 3, 8, 8), 4, line.color);
        let ty = crate::text::center_y(ly, 14, crate::text::CAPTION, crate::text::Weight::Regular);
        crate::text::draw(
            c,
            lx + 12,
            ty,
            &label,
            crate::text::CAPTION,
            crate::text::Weight::Regular,
            DIM_TEXT,
        );
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

// =====================================================================
// The widget toolkit (wave 2 builds on this section).
//
// Every function draws into a `Canvas` at a rectangle the caller computed, reads
// the colours of the *current appearance* (`theme::pal()`), and takes the
// interaction state as an argument: the caller derives `Control::Hover` /
// `Pressed` from the pointer. Geometry shared with hit testing comes from
// `kitsune_core::widgets` and `kitsune_core::chrome`.
// =====================================================================

use crate::text::{self, BODY, CAPTION, FOOTNOTE, Weight};
use kitsune_core::style::Palette;
use kitsune_core::widgets as wg;

/// Interaction state of a control.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Control {
    Normal,
    Hover,
    Pressed,
    Disabled,
}

/// Visual weight of a push button.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ButtonKind {
    /// Plain (white / translucent) button.
    Secondary,
    /// The default action: accent fill.
    Primary,
    /// A destructive action: red fill.
    Destructive,
}

/// Fill `r` with an ARGB token (its own alpha applies).
pub(crate) fn fill_token(c: &mut Canvas, r: Rect, radius: i32, argb: u32) {
    let (col, a) = theme::tint(argb);
    c.fill_rrect(r, radius, Corner::Circle, col, a);
}

/// 1 px border inside `r` in an ARGB token.
pub(crate) fn stroke_token(c: &mut Canvas, r: Rect, radius: i32, argb: u32) {
    let (col, a) = theme::tint(argb);
    c.stroke_rrect(r, radius, Corner::Circle, col, a);
}

fn mix(a: Color, b: Color, t: u16) -> Color {
    a.lerp(b, t)
}

fn draw_button(c: &mut Canvas, r: Rect, label: &str, kind: ButtonKind, st: Control, p: &Palette) {
    let disabled = st == Control::Disabled;
    let acc = theme::accent();
    let (bg, fg, border): (Color, Color, Option<u32>) = match kind {
        ButtonKind::Primary => {
            let base = match st {
                Control::Pressed => mix(acc, Color::rgb(0, 0, 0), 56),
                Control::Hover => mix(acc, Color::rgb(255, 255, 255), 28),
                _ => acc,
            };
            (base, theme::ACCENT_TEXT, None)
        }
        ButtonKind::Destructive => {
            let base = theme::solid(p.danger);
            let base = match st {
                Control::Pressed => mix(base, Color::rgb(0, 0, 0), 56),
                Control::Hover => mix(base, Color::rgb(255, 255, 255), 28),
                _ => base,
            };
            (base, theme::ACCENT_TEXT, None)
        }
        ButtonKind::Secondary => {
            let base = theme::solid(p.control_bg);
            let base = match st {
                Control::Pressed => mix(base, Color::rgb(0x80, 0x80, 0x88), 46),
                Control::Hover => mix(base, Color::rgb(0x80, 0x80, 0x88), 18),
                _ => base,
            };
            (base, theme::solid(p.text), Some(p.control_border))
        }
    };
    let a_scale: u32 = if disabled { 110 } else { 256 };
    let (_, bg_alpha) = if kind == ButtonKind::Secondary {
        theme::tint(p.control_bg)
    } else {
        (bg, 256)
    };
    c.fill_rrect(
        r,
        kitsune_core::style::R_CONTROL,
        Corner::Circle,
        bg,
        (bg_alpha as u32 * a_scale / 256) as u16,
    );
    if let Some(b) = border {
        let (bc, ba) = theme::tint(b);
        c.stroke_rrect(
            r,
            kitsune_core::style::R_CONTROL,
            Corner::Circle,
            bc,
            (ba as u32 * a_scale / 256) as u16,
        );
    }
    let w = if kind == ButtonKind::Secondary {
        Weight::Regular
    } else {
        Weight::Medium
    };
    text::draw_centered_a(c, r, label, BODY, w, fg, a_scale as u16);
}

/// A push button.
pub(crate) fn push_button(c: &mut Canvas, r: Rect, label: &str, kind: ButtonKind, st: Control) {
    draw_button(c, r, label, kind, st, theme::pal());
}

fn draw_field(
    c: &mut Canvas,
    r: Rect,
    t: &str,
    placeholder: &str,
    focused: bool,
    caret: bool,
    p: &Palette,
) {
    let rad = kitsune_core::style::R_CONTROL;
    fill_token(c, r, rad, p.field_bg);
    if focused {
        // Focus ring: the accent, soft outside and crisp inside.
        let acc = theme::accent();
        c.stroke_rrect(r.inflated(2), rad + 2, Corner::Circle, acc, 90);
        c.stroke_rrect(r, rad, Corner::Circle, acc, 256);
    } else {
        stroke_token(c, r, rad, p.control_border);
    }
    let inner = Rect::new(r.x + 8, r.y, r.w - 16, r.h);
    let ty = text::center_y(r.y, r.h, BODY, Weight::Regular);
    if t.is_empty() {
        text::draw_ellipsis(
            c,
            inner.x,
            ty,
            inner.w,
            placeholder,
            BODY,
            Weight::Regular,
            theme::solid(p.text_tertiary),
        );
        if caret {
            fill(c, Rect::new(inner.x, r.y + 5, 1, r.h - 10), theme::accent());
        }
        return;
    }
    // Show the tail of text wider than the field.
    let mut start = 0usize;
    while text::measure(&t[start..], BODY, Weight::Regular) > inner.w && start < t.len() {
        start += t[start..].chars().next().map_or(1, char::len_utf8);
    }
    let tail = &t[start..];
    let tw = text::draw(
        c,
        inner.x,
        ty,
        tail,
        BODY,
        Weight::Regular,
        theme::solid(p.text),
    );
    if caret {
        fill(
            c,
            Rect::new(inner.x + tw + 1, r.y + 5, 1, r.h - 10),
            theme::accent(),
        );
    }
}

/// Just the frame of a text field (fill, hairline, focus ring) of any corner radius.
pub(crate) fn text_field_frame(c: &mut Canvas, r: Rect, rad: i32, focused: bool) {
    let p = theme::pal();
    fill_token(c, r, rad, p.field_bg);
    if focused {
        let acc = theme::accent();
        c.stroke_rrect(r.inflated(2), rad + 2, Corner::Circle, acc, 90);
        c.stroke_rrect(r, rad, Corner::Circle, acc, 256);
    } else {
        stroke_token(c, r, rad, p.control_border);
    }
}

/// A single-line text field with placeholder, focus ring and caret.
pub(crate) fn text_field(
    c: &mut Canvas,
    r: Rect,
    t: &str,
    placeholder: &str,
    focused: bool,
    caret: bool,
) {
    draw_field(c, r, t, placeholder, focused, caret, theme::pal());
}

/// A segmented control with `labels`; segment `selected` is raised.
pub(crate) fn segmented(c: &mut Canvas, r: Rect, labels: &[&str], selected: usize) {
    let p = theme::pal();
    let rad = kitsune_core::style::R_CONTROL;
    fill_token(
        c,
        r,
        rad,
        if theme::dark() {
            0x1FFF_FFFF
        } else {
            0x1400_0000
        },
    );
    let segs = wg::segmented_rects(r, labels.len());
    for (i, (s, l)) in segs.iter().zip(labels).enumerate() {
        if i == selected {
            c.draw_shadow(
                *s,
                Shadow {
                    blur: 3,
                    dy: 1,
                    alpha: 60,
                },
                Rect::new(s.x, s.y, s.w, s.h),
            );
            let bg = if theme::dark() {
                Color::rgb(0x63, 0x63, 0x66)
            } else {
                Color::rgb(0xFF, 0xFF, 0xFF)
            };
            c.fill_rrect(*s, rad - 1, Corner::Circle, bg, 256);
            text::draw_centered(c, *s, l, BODY, Weight::Medium, theme::solid(p.text));
        } else {
            text::draw_centered(
                c,
                *s,
                l,
                BODY,
                Weight::Regular,
                theme::solid(p.text_secondary),
            );
        }
    }
}

/// A toggle switch; `t256` is the animated position (0 = off .. 256 = on).
pub(crate) fn switch(c: &mut Canvas, r: Rect, t256: i32, enabled: bool) {
    let off = if theme::dark() {
        Color::rgb(0x56, 0x56, 0x5A)
    } else {
        Color::rgb(0xD1, 0xD1, 0xD6)
    };
    let track = mix(
        off,
        theme::accent(),
        (t256.clamp(0, 256) * 255 / 256) as u16,
    );
    c.fill_rrect(
        r,
        r.h / 2,
        Corner::Circle,
        track,
        if enabled { 256 } else { 120 },
    );
    let knob = wg::switch_knob(r, t256);
    c.draw_shadow(
        knob,
        Shadow {
            blur: 3,
            dy: 1,
            alpha: 90,
        },
        Rect::new(knob.x, knob.y + 3, knob.w, knob.h - 6),
    );
    c.fill_rrect(
        knob,
        knob.h / 2,
        Corner::Circle,
        Color::rgb(0xFF, 0xFF, 0xFF),
        256,
    );
}

/// A horizontal slider with value `v` in `min..=max`.
pub(crate) fn slider(c: &mut Canvas, r: Rect, v: i32, min: i32, max: i32, enabled: bool) {
    let p = theme::pal();
    let t = wg::slider_track(r);
    let kx = wg::slider_knob_x(r, v, min, max);
    let a = if enabled { 256 } else { 120 };
    fill_token(
        c,
        t,
        2,
        if theme::dark() {
            0x40FF_FFFF
        } else {
            0x2400_0000
        },
    );
    let filled = Rect::new(t.x, t.y, (kx - t.x).max(0), t.h);
    c.fill_rrect(filled, 2, Corner::Circle, theme::accent(), a);
    let knob = Rect::new(
        kx - wg::SLIDER_KNOB / 2,
        r.y + r.h / 2 - wg::SLIDER_KNOB / 2,
        wg::SLIDER_KNOB,
        wg::SLIDER_KNOB,
    );
    c.draw_shadow(
        knob,
        Shadow {
            blur: 3,
            dy: 1,
            alpha: 80,
        },
        Rect::new(knob.x, knob.y + 3, knob.w, knob.h - 6),
    );
    c.fill_rrect(
        knob,
        knob.h / 2,
        Corner::Circle,
        Color::rgb(0xFF, 0xFF, 0xFF),
        a,
    );
    stroke_token(c, knob, knob.h / 2, p.control_border);
}

/// A 16 px check box.
pub(crate) fn checkbox(c: &mut Canvas, x: i32, y: i32, checked: bool) {
    let p = theme::pal();
    let r = Rect::new(x, y, 16, 16);
    if checked {
        c.fill_rrect(r, 4, Corner::Circle, theme::accent(), 256);
        draw_glyph(c, iconart::Glyph::Check, x, y, 16, 0xFFFF_FFFF);
    } else {
        fill_token(c, r, 4, p.field_bg);
        stroke_token(c, r, 4, p.control_border);
    }
}

/// A 16 px radio button.
pub(crate) fn radio(c: &mut Canvas, x: i32, y: i32, selected: bool) {
    let p = theme::pal();
    let r = Rect::new(x, y, 16, 16);
    if selected {
        c.fill_rrect(r, 8, Corner::Circle, theme::accent(), 256);
        c.fill_rrect(
            r.inflated(-5),
            3,
            Corner::Circle,
            Color::rgb(0xFF, 0xFF, 0xFF),
            256,
        );
    } else {
        fill_token(c, r, 8, p.field_bg);
        stroke_token(c, r, 8, p.control_border);
    }
}

/// A thin overlay scrollbar that fades: `alpha` 0..=256 (see `ScrollbarFade`).
pub(crate) fn overlay_scrollbar(
    c: &mut Canvas,
    track: Rect,
    top: usize,
    total: usize,
    rows: usize,
    alpha: u32,
) {
    if total <= rows || track.h <= 0 || alpha == 0 {
        return;
    }
    let (off, len) = wg::scroll_thumb(track.h, total, rows, top, 28);
    let w = 6.min(track.w);
    let thumb = Rect::new(track.right() - w - 2, track.y + off, w, len);
    let base = if theme::dark() { 0xA0u32 } else { 0x80 };
    c.fill_rrect(
        thumb,
        w / 2,
        Corner::Circle,
        if theme::dark() {
            Color::rgb(0xFF, 0xFF, 0xFF)
        } else {
            Color::rgb(0, 0, 0)
        },
        (base * alpha / 256) as u16,
    );
}

/// A list row: accent pill when `selected`, a soft wash when `hovered`.
pub(crate) fn list_row(c: &mut Canvas, r: Rect, selected: bool, hovered: bool, label: &str) {
    let p = theme::pal();
    let inner = Rect::new(r.x + 4, r.y + 1, r.w - 8, r.h - 2);
    let fg = if selected {
        c.fill_rrect(inner, 6, Corner::Circle, theme::accent(), 256);
        theme::ACCENT_TEXT
    } else {
        if hovered {
            fill_token(c, inner, 6, p.hover);
        }
        theme::solid(p.text)
    };
    text::draw_left(
        c,
        Rect::new(r.x + 12, r.y, r.w - 24, r.h),
        label,
        BODY,
        Weight::Regular,
        fg,
    );
}

/// A menu row: accent pill with white text when `hovered`; an optional check mark and
/// a right-aligned shortcut in secondary colour.
pub(crate) fn menu_item(
    c: &mut Canvas,
    r: Rect,
    label: &str,
    shortcut: &str,
    hovered: bool,
    enabled: bool,
    checked: bool,
) {
    let p = theme::pal();
    let (fg, fg2) = if hovered && enabled {
        c.fill_rrect(
            r,
            kitsune_core::style::R_MENU - 3,
            Corner::Circle,
            theme::accent(),
            256,
        );
        (theme::ACCENT_TEXT, theme::ACCENT_TEXT)
    } else if enabled {
        (theme::solid(p.text), theme::solid(p.text_secondary))
    } else {
        (theme::solid(p.text_tertiary), theme::solid(p.text_tertiary))
    };
    if checked {
        let col = if hovered && enabled {
            0xFFFF_FFFFu32
        } else {
            p.text
        };
        draw_glyph(
            c,
            iconart::Glyph::Check,
            r.x + 6,
            r.y + (r.h - 12) / 2,
            12,
            col,
        );
    }
    let lx = r.x + kitsune_core::chrome::MENU_CHECK_W + 4;
    text::draw_left(
        c,
        Rect::new(lx, r.y, r.w - (lx - r.x) - 8, r.h),
        label,
        BODY,
        Weight::Regular,
        fg,
    );
    if !shortcut.is_empty() {
        text::draw_right(
            c,
            Rect::new(r.x, r.y, r.w - 10, r.h),
            shortcut,
            BODY,
            Weight::Regular,
            fg2,
        );
    }
}

/// A tooltip bubble centred on `cx` whose bottom edge is at `bottom`.
pub(crate) fn tooltip(c: &mut Canvas, cx: i32, bottom: i32, label: &str) -> Rect {
    let p = theme::pal();
    let w = text::measure(label, FOOTNOTE, Weight::Medium) + 20;
    let h = 24;
    let r = Rect::new(
        (cx - w / 2).clamp(4, (c.width() as i32 - w - 4).max(4)),
        bottom - h,
        w,
        h,
    );
    c.draw_shadow(
        r,
        Shadow {
            blur: 6,
            dy: 2,
            alpha: 70,
        },
        Rect::new(r.x, r.y + 6, r.w, (r.h - 12).max(0)),
    );
    fill_token(c, r, kitsune_core::style::R_TOOLTIP, p.tooltip_bg);
    text::draw_centered(
        c,
        r,
        label,
        FOOTNOTE,
        Weight::Medium,
        theme::solid(p.tooltip_text),
    );
    r
}

/// A progress bar (`permille` 0..=1000).
pub(crate) fn progress(c: &mut Canvas, r: Rect, permille: u32) {
    fill_token(
        c,
        r,
        r.h / 2,
        if theme::dark() {
            0x40FF_FFFF
        } else {
            0x2400_0000
        },
    );
    let w = (r.w as i64 * permille.min(1000) as i64 / 1000) as i32;
    if w > 0 {
        c.fill_rrect(
            Rect::new(r.x, r.y, w.max(r.h), r.h),
            r.h / 2,
            Corner::Circle,
            theme::accent(),
            256,
        );
    }
}

/// A hairline separator.
pub(crate) fn separator(c: &mut Canvas, r: Rect) {
    fill_token(c, r, 0, theme::pal().separator);
}

/// A rounded group (settings-style card) on the window background.
pub(crate) fn group_box(c: &mut Canvas, r: Rect) {
    let p = theme::pal();
    fill_token(c, r, 10, p.content_bg);
    stroke_token(c, r, 10, p.separator);
}

/// A monochrome glyph (`iconart`) at `(x, y)`, `size` square, in an ARGB colour.
pub(crate) fn draw_glyph(c: &mut Canvas, g: iconart::Glyph, x: i32, y: i32, size: i32, argb: u32) {
    let s = crate::glyphs::get(g, size as usize, argb);
    c.blit_surface(s, x, y, 256);
}

/// Section caption (small, secondary).
pub(crate) fn caption(c: &mut Canvas, x: i32, y: i32, label: &str) {
    text::draw(
        c,
        x,
        y,
        label,
        CAPTION,
        Weight::Medium,
        theme::solid(theme::pal().text_secondary),
    );
}
