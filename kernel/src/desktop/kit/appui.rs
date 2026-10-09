//! Drawing helpers shared by the redesigned app interiors (Arquivos, Imagens, Editor,
//! Terminal): toolbar buttons with glyphs, the path-bar pill, a text field with a selection
//! and an eased caret, the window-attached sheet, empty states and a few time helpers.
//!
//! They sit next to the toolkit in `ui.rs` rather than inside it, so the toolkit stays as the
//! wave-1 API; everything reads the palette of the current appearance and measures text.

// Not every helper is used by every app.
#![allow(dead_code)]

use crate::desktop::kit::ui::{self, ButtonKind, Control};
use crate::desktop::*;
use crate::text::{self, BODY, CALLOUT, FOOTNOTE, Weight};
use kitsune_core::anim::{self, Tween};
use kitsune_core::appart::{FileKind, Tool};
use kitsune_core::style::R_CONTROL;

/// `0xRRGGBB` of a colour.
pub(crate) fn rgb_of(c: Color) -> u32 {
    ((c.r as u32) << 16) | ((c.g as u32) << 8) | c.b as u32
}

/// Timer ticks (250 Hz).
pub(crate) fn ticks() -> u64 {
    crate::interrupts::ticks()
}

/// Milliseconds since boot (wraps after 49 days, which `ScrollbarFade` copes with).
pub(crate) fn now_ms() -> u32 {
    (ticks().wrapping_mul(4)) as u32
}

// ---------------------------------------------------------------------------- caret

/// Milliseconds since the last input at tick `last_input` (`None`: 0 means never).
fn since_input(last_input: u64) -> Option<u64> {
    (last_input != 0).then(|| ticks().saturating_sub(last_input) * 4)
}

/// Opacity (0..=256) of a caret whose owner last saw input at tick `last_input` (0 = never): the
/// eased blink of `kitsune_core::anim::caret_alpha`.
pub(crate) fn caret_alpha(last_input: u64) -> u32 {
    anim::caret_alpha(since_input(last_input))
}

/// Whether the caret of an owner with input at `last_input` still animates (needs frames).
pub(crate) fn caret_animating(last_input: u64) -> bool {
    anim::caret_animating(since_input(last_input))
}

// -------------------------------------------------------------------------- buttons

/// A glyph button of a toolbar: soft wash on hover, a stronger one while pressed, the accent
/// tint when `active` (a toggle that is on), dim glyph when disabled. `hover` is 0..=256.
pub(crate) fn tool_button(
    c: &mut Canvas,
    r: Rect,
    tool: Tool,
    enabled: bool,
    hover: u32,
    pressed: bool,
    active: bool,
) {
    let p = theme::pal();
    if active {
        let (col, a) = theme::tint(0x30_00_00_00 | rgb_of(theme::accent()));
        c.fill_rrect(r, R_CONTROL, Corner::Circle, col, a);
    }
    if enabled && (hover > 0 || pressed) {
        let (col, a) = theme::tint(p.hover);
        let a = if pressed {
            a as u32 * 2
        } else {
            a as u32 * hover / 256
        } as u16;
        c.fill_rrect(r, R_CONTROL, Corner::Circle, col, a);
    }
    let argb = if !enabled {
        p.text_tertiary
    } else if active {
        0xFF00_0000 | rgb_of(theme::accent())
    } else {
        p.text_secondary
    };
    let size = 16;
    let (gx, gy) = (r.x + (r.w - size) / 2, r.y + (r.h - size) / 2);
    c.blit_surface(
        super::appart::tool(tool, size, 0xFF00_0000 | (argb & 0xFF_FFFF)),
        gx,
        gy,
        if enabled { 256 } else { 120 },
    );
}

/// A segmented control whose segments are glyphs; `selected` is raised.
pub(crate) fn tool_segmented(c: &mut Canvas, r: Rect, tools: &[Tool], selected: usize) {
    let p = theme::pal();
    ui::fill_token(
        c,
        r,
        R_CONTROL,
        if theme::dark() {
            0x1FFF_FFFF
        } else {
            0x1400_0000
        },
    );
    let segs = kitsune_core::widgets::segmented_rects(r, tools.len());
    for (i, (s, t)) in segs.iter().zip(tools).enumerate() {
        let on = i == selected;
        if on {
            c.draw_shadow(
                *s,
                Shadow {
                    blur: 3,
                    dy: 1,
                    alpha: 60,
                },
                *s,
            );
            let bg = if theme::dark() {
                Color::rgb(0x63, 0x63, 0x66)
            } else {
                Color::rgb(0xFF, 0xFF, 0xFF)
            };
            c.fill_rrect(*s, R_CONTROL - 1, Corner::Circle, bg, 256);
        }
        let col = if on { p.text } else { p.text_secondary };
        let size = 16;
        c.blit_surface(
            super::appart::tool(*t, size, 0xFF00_0000 | (col & 0xFF_FFFF)),
            s.x + (s.w - size) / 2,
            s.y + (s.h - size) / 2,
            256,
        );
    }
}

/// The path-bar pill: a faint recessed capsule.
pub(crate) fn pill(c: &mut Canvas, r: Rect) {
    if r.w <= 0 {
        return;
    }
    ui::fill_token(
        c,
        r,
        R_CONTROL + 2,
        if theme::dark() {
            0x14FF_FFFF
        } else {
            0x0F00_0000
        },
    );
}

// ----------------------------------------------------------------------- text field

/// What a [`field`] shows: the text, the caret as a byte offset and the selected byte range.
pub(crate) struct FieldText<'a> {
    pub text: &'a str,
    pub caret: usize,
    pub selection: Option<(usize, usize)>,
}

/// Draw a one-line text field with a magnifier-style leading glyph (optional), the text
/// scrolled so the caret stays in view, a selection and a caret of opacity `caret_a`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn field(
    c: &mut Canvas,
    r: Rect,
    ft: &FieldText<'_>,
    placeholder: &str,
    focused: bool,
    caret_a: u32,
    lead: Option<Tool>,
    clear_button: bool,
) {
    let p = theme::pal();
    ui::text_field_frame(c, r, R_CONTROL, focused);
    let mut x0 = r.x + 8;
    if let Some(t) = lead {
        c.blit_surface(
            super::appart::tool(t, 14, 0xFF00_0000 | (p.text_tertiary & 0xFF_FFFF)),
            x0,
            r.y + (r.h - 14) / 2,
            256,
        );
        x0 += 20;
    }
    let mut x1 = r.right() - 8;
    if clear_button {
        x1 -= 16;
        let b = Rect::new(r.right() - 8 - 14, r.y + (r.h - 14) / 2, 14, 14);
        c.blit_surface(
            super::appart::tool(
                Tool::Cancel,
                14,
                0xFF00_0000 | (p.text_tertiary & 0xFF_FFFF),
            ),
            b.x,
            b.y,
            256,
        );
    }
    let inner = Rect::new(x0, r.y, (x1 - x0).max(0), r.h);
    let saved = c.set_clip(
        inner
            .intersection(&c.clip_rect())
            .unwrap_or(Rect::new(0, 0, 0, 0)),
    );
    let ty = text::center_y(r.y, r.h, BODY, Weight::Regular);
    if ft.text.is_empty() {
        text::draw(
            c,
            inner.x,
            ty,
            placeholder,
            BODY,
            Weight::Regular,
            theme::solid(p.text_tertiary),
        );
        if focused && caret_a > 0 {
            caret_bar(c, inner.x, r, caret_a);
        }
        c.restore_clip(saved);
        return;
    }
    let caret_at = ft.caret.min(ft.text.len());
    let caret_x = text::measure(&ft.text[..caret_at], BODY, Weight::Regular);
    let total = text::measure(ft.text, BODY, Weight::Regular);
    // Scroll so the caret is visible; show the end of long text.
    let _ = total;
    let shift = (caret_x - (inner.w - 2)).max(0);
    if let Some((a, b)) = ft.selection {
        let (a, b) = (a.min(ft.text.len()), b.min(ft.text.len()));
        let xa = text::measure(&ft.text[..a], BODY, Weight::Regular);
        let xb = text::measure(&ft.text[..b], BODY, Weight::Regular);
        let sel = Rect::new(inner.x + xa - shift, r.y + 4, xb - xa, r.h - 8);
        let (col, al) = theme::tint(0x55_00_00_00 | rgb_of(theme::accent()));
        c.fill_rrect(sel, 3, Corner::Circle, col, al);
    }
    text::draw(
        c,
        inner.x - shift,
        ty,
        ft.text,
        BODY,
        Weight::Regular,
        theme::solid(p.text),
    );
    if focused && caret_a > 0 && ft.selection.is_none() {
        caret_bar(c, inner.x + caret_x - shift, r, caret_a);
    }
    c.restore_clip(saved);
}

fn caret_bar(c: &mut Canvas, x: i32, r: Rect, alpha: u32) {
    let bar = Rect::new(x, r.y + 5, 2, r.h - 10);
    c.fill_rrect(
        bar,
        1,
        Corner::Circle,
        theme::accent(),
        alpha.min(256) as u16,
    );
}

/// Hit area of the clear (x) button of a [`field`] drawn with `clear_button`.
pub(crate) fn field_clear_rect(r: Rect) -> Rect {
    Rect::new(r.right() - 8 - 14 - 4, r.y, 22, r.h)
}

// ------------------------------------------------------------------------- sheets

/// Where a sheet of `size` rests in window `win` (hit testing and drawing share it).
pub(crate) fn sheet_rect(win: Rect, size: (i32, i32)) -> Rect {
    let body = Rect::new(win.x, win.y + TITLE_H, win.w, (win.h - TITLE_H).max(0));
    let w = size.0.min(body.w - 24).max(120);
    let h = size.1.min(body.h - 8).max(40);
    Rect::new(body.x + (body.w - w) / 2, body.y, w, h)
}

/// A sheet attached to the top of a window: a dim over the content and a panel hanging from
/// the title bar that slides down. `t` is the transition (0..=256). Returns the panel
/// rectangle (its final place, shifted while sliding) for the caller to fill.
pub(crate) fn sheet(c: &mut Canvas, win: Rect, size: (i32, i32), t: u32) -> Rect {
    let p = theme::pal();
    let body = Rect::new(win.x, win.y + TITLE_H, win.w, (win.h - TITLE_H).max(0));
    let saved = c.set_clip(
        body.intersection(&c.clip_rect())
            .unwrap_or(Rect::new(0, 0, 0, 0)),
    );
    // The dim.
    let dim = if theme::dark() { 120u32 } else { 72 };
    c.blend_rect(body, Color::rgb(0, 0, 0), (dim * t / 256) as u16);
    let rest = sheet_rect(win, size);
    let slide = ((256 - t as i32) * (rest.h + 16)) / 256;
    let panel = Rect::new(rest.x, rest.y - slide, rest.w, rest.h);
    let hole = Rect::new(panel.x, panel.y, panel.w, panel.h - 12);
    c.draw_shadow(
        Rect::new(panel.x, panel.y - 12, panel.w, panel.h + 12),
        Shadow {
            blur: 18,
            dy: 8,
            alpha: 110,
        },
        hole,
    );
    // The panel is drawn 12 px taller upwards so only its bottom corners show round.
    let full = Rect::new(panel.x, panel.y - 12, panel.w, panel.h + 12);
    c.fill_rrect(full, 12, Corner::Circle, theme::solid(p.window_bg), 256);
    ui::stroke_token(c, full, 12, p.separator);
    c.restore_clip(saved);
    panel
}

/// Standard sheet metrics: padding and the height of a button row.
pub(crate) const SHEET_PAD: i32 = 20;
pub(crate) const BUTTON_H: i32 = 28;

/// Lay out `labels` as a right-aligned row of buttons ending at `right`, `y` the top.
/// Returns the rectangles left to right.
pub(crate) fn button_row(right: i32, y: i32, labels: &[&str]) -> Vec<Rect> {
    let mut x = right;
    let mut out = Vec::new();
    for l in labels.iter().rev() {
        let w = (text::measure(l, BODY, Weight::Medium) + 32).max(76);
        x -= w;
        out.push(Rect::new(x, y, w, BUTTON_H));
        x -= 8;
    }
    out.reverse();
    out
}

/// A sheet's title and message lines. Returns the y just below them.
pub(crate) fn sheet_text(
    c: &mut Canvas,
    panel: Rect,
    title: &str,
    message: &str,
    danger_title: bool,
) -> i32 {
    let p = theme::pal();
    let inner_w = panel.w - 2 * SHEET_PAD;
    let ty = panel.y + SHEET_PAD;
    let tcol = if danger_title {
        theme::danger()
    } else {
        theme::solid(p.text)
    };
    text::draw_ellipsis(
        c,
        panel.x + SHEET_PAD,
        ty,
        inner_w,
        title,
        text::TITLE3,
        Weight::Semibold,
        tcol,
    );
    let mut y = ty + text::line_height(text::TITLE3) + 6;
    for (a, b) in text::wrap(message, BODY, Weight::Regular, inner_w, 4) {
        text::draw(
            c,
            panel.x + SHEET_PAD,
            y,
            &message[a..b],
            BODY,
            Weight::Regular,
            theme::solid(p.text_secondary),
        );
        y += text::line_height(BODY) + 2;
    }
    y
}

/// A standard sheet button.
pub(crate) fn sheet_button(
    c: &mut Canvas,
    r: Rect,
    label: &str,
    kind: ButtonKind,
    hovered: bool,
    pressed: bool,
) {
    let st = if pressed {
        Control::Pressed
    } else if hovered {
        Control::Hover
    } else {
        Control::Normal
    };
    ui::push_button(c, r, label, kind, st);
}

// ----------------------------------------------------------------------- empty state

/// An empty state centred in `area`: a faded icon, a title and an optional second line.
pub(crate) fn empty_state(
    c: &mut Canvas,
    area: Rect,
    icon: EmptyIcon,
    title: &str,
    subtitle: &str,
) {
    let p = theme::pal();
    let has_sub = !subtitle.is_empty();
    let block_h = 64 + 12 + 20 + if has_sub { 20 } else { 0 };
    let top = area.y + ((area.h - block_h) / 2).max(8) - 8;
    let cx = area.x + area.w / 2;
    match icon {
        EmptyIcon::File(k) => {
            c.blit_surface(super::appart::file_icon(k, 64), cx - 32, top, 150);
        }
        EmptyIcon::Tool(t) => {
            c.blit_surface(
                super::appart::tool(t, 48, 0xFF00_0000 | (p.text_tertiary & 0xFF_FFFF)),
                cx - 24,
                top + 8,
                256,
            );
        }
    }
    let ty = top + 64 + 12;
    let tw = text::measure(title, CALLOUT, Weight::Semibold);
    text::draw(
        c,
        cx - tw / 2,
        ty,
        title,
        CALLOUT,
        Weight::Semibold,
        theme::solid(p.text_secondary),
    );
    if has_sub {
        let room = (area.w - 32).max(40);
        let s = text::ellipsize(subtitle, FOOTNOTE, Weight::Regular, room);
        let sw = text::measure(&s, FOOTNOTE, Weight::Regular);
        text::draw(
            c,
            cx - sw / 2,
            ty + 22,
            &s,
            FOOTNOTE,
            Weight::Regular,
            theme::solid(p.text_tertiary),
        );
    }
}

/// What an empty state draws as its picture.
#[derive(Clone, Copy)]
pub(crate) enum EmptyIcon {
    File(FileKind),
    Tool(Tool),
}

// -------------------------------------------------------------------------- misc

/// The 0..=256 value of a tween.
pub(crate) fn level(t: &Tween) -> u32 {
    (t.value().clamp(0.0, 1.0) * 256.0) as u32
}

/// A selection colour for a list pill: the accent in a focused window, a neutral grey in a
/// window that is not.
pub(crate) fn selection_fill(focused: bool) -> Color {
    if focused {
        theme::accent()
    } else if theme::dark() {
        Color::rgb(0x5A, 0x5A, 0x60)
    } else {
        Color::rgb(0xC9, 0xC9, 0xD0)
    }
}

/// Text on a [`selection_fill`].
pub(crate) fn selection_text(focused: bool) -> Color {
    if focused || theme::dark() {
        theme::ACCENT_TEXT
    } else {
        theme::text()
    }
}

/// A short decorative hairline under a header (full width).
pub(crate) fn hairline(c: &mut Canvas, x: i32, y: i32, w: i32) {
    if w > 0 {
        ui::fill_token(c, Rect::new(x, y, w, 1), 0, theme::pal().separator);
    }
}

/// Same as `appart::blit_tool` (the name the apps use).
pub(crate) fn blit_tool_dim(
    c: &mut Canvas,
    t: Tool,
    x: i32,
    y: i32,
    size: i32,
    rgb: u32,
    opacity: u32,
) {
    super::appart::blit_tool(c, t, x, y, size, rgb, opacity);
}

/// Draw `text` cut with an ellipsis to `max_w`, at opacity `alpha` (0..=256).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_ellipsis_a(
    c: &mut Canvas,
    x: i32,
    y: i32,
    max_w: i32,
    s: &str,
    px: u16,
    w: Weight,
    color: Color,
    alpha: u16,
) {
    let t = text::ellipsize(s, px, w, max_w.max(0));
    text::draw_a(c, x, y, &t, px, w, color, alpha);
}

/// `v` rounded to the nearest integer (the kernel has no `f32::round`).
pub(crate) fn round(v: f32) -> i32 {
    if v >= 0.0 {
        (v + 0.5) as i32
    } else {
        (v - 0.5) as i32
    }
}
