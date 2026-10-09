//! Painting a laid-out web page: the kernel half of the `web` engine.
//!
//! The engine produces a display list in page coordinates and asks for its text
//! widths through [`osjeff_core::web::TextMetrics`]; [`KernelMetrics`] answers with
//! the same glyph engine that draws the text here (Inter for text, JetBrains Mono for
//! `<pre>` and `<code>`), so a line that was wrapped to fit really does fit.
//!
//! Italic has no face of its own: runs are slanted by 12 degrees at draw time (the
//! metrics do not change). Bold is Inter Semibold. A character the faces do not have
//! (CJK, emoji, most non-Latin scripts) is drawn as a hollow box instead of the `?`
//! stand-in, and measured as one.
//!
//! The page area keeps the page's own colours in both appearances: nothing here reads
//! the system palette except the accent (selection, caret, focus ring).

use super::*;
use crate::text::{self, Weight};
use osjeff_core::web::{
    Cmd as WebCmd, DECO_STRIKE, DECO_UNDERLINE, Font, Page, Rgb, TextMetrics, form::FieldKind,
    textops::Span,
};

/// Text metrics backed by the kernel's glyph engine.
pub(crate) struct KernelMetrics;

/// The face and size the engine's [`Font`] stands for.
pub(crate) fn face_of(f: Font) -> (u16, Weight) {
    let w = if f.mono {
        Weight::Mono
    } else if f.bold {
        Weight::Semibold
    } else {
        Weight::Regular
    };
    (f.size.clamp(4, 400), w)
}

/// Width of a character without a glyph, as a box: 7/10 em.
fn tofu_q8(px: u16) -> i32 {
    i32::from(px) * 256 * 7 / 10
}

/// Is `c` drawn by the face (as opposed to a box or nothing)?
fn drawable(c: char, w: Weight) -> bool {
    text::has_glyph(c, w)
}

impl TextMetrics for KernelMetrics {
    fn width(&self, t: &str, f: Font) -> i32 {
        (self.width_q8(t, f) + 255) / 256
    }

    fn width_q8(&self, t: &str, f: Font) -> i32 {
        let (px, w) = face_of(f);
        // The usual case: text the face has entirely.
        if t.bytes().all(|b| (0x20..0x7f).contains(&b)) {
            return text::measure_q8(t, px, w);
        }
        let mut total = 0i32;
        let mut start = 0;
        let mut in_run = true;
        for (i, ch) in t.char_indices() {
            let zero = osjeff_core::web::metrics::is_zero_width(ch);
            let ok = zero || drawable(ch, w);
            if ok != in_run {
                if in_run {
                    total = total.saturating_add(text::measure_q8(&t[start..i], px, w));
                }
                start = i;
                in_run = ok;
            }
            if !ok {
                total = total.saturating_add(tofu_q8(px));
                start = i + ch.len_utf8();
                in_run = true;
            }
        }
        if in_run && start < t.len() {
            total = total.saturating_add(text::measure_q8(&t[start..], px, w));
        }
        total
    }

    fn line_height(&self, f: Font) -> i32 {
        let (px, w) = face_of(f);
        text::vmetrics(px, w).line_height
    }

    fn ascent(&self, f: Font) -> i32 {
        let (px, w) = face_of(f);
        text::vmetrics(px, w).ascent
    }

    fn has_glyph(&self, c: char, f: Font) -> bool {
        drawable(c, face_of(f).1)
    }
}

/// Convert a `web` engine color to a framebuffer color.
fn rgb(c: Rgb) -> Color {
    Color::rgb(c.0, c.1, c.2)
}

/// `a` moved towards black by `t`/256 (a darker link on hover).
fn darker(a: Color, t: u16) -> Color {
    a.lerp(Color::rgb(0, 0, 0), t)
}

/// Draw one run of page text with its top at `y`: slanted when italic, boxes for the
/// characters the face lacks. Returns the pen advance.
#[allow(clippy::too_many_arguments)]
fn draw_run(c: &mut Canvas, x: i32, y: i32, t: &str, f: Font, color: Color) -> i32 {
    let (px, w) = face_of(f);
    let put = |c: &mut Canvas, x: i32, s: &str| -> i32 {
        if f.mono && !f.italic {
            text::draw(c, x, y, s, px, w, color)
        } else if f.italic {
            text::draw_slanted(c, x, y, s, px, w, color, 256)
        } else {
            text::draw(c, x, y, s, px, w, color)
        }
    };
    if t.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return put(c, x, t);
    }
    let mut pen = x;
    let mut seg_start = 0;
    let flush = |c: &mut Canvas, pen: &mut i32, from: usize, to: usize| {
        if to > from {
            *pen += put(c, *pen, &t[from..to]);
        }
    };
    for (i, ch) in t.char_indices() {
        if osjeff_core::web::metrics::is_zero_width(ch) {
            flush(c, &mut pen, seg_start, i);
            seg_start = i + ch.len_utf8();
        } else if !drawable(ch, w) {
            flush(c, &mut pen, seg_start, i);
            seg_start = i + ch.len_utf8();
            // A hollow box the height of a capital.
            let v = text::vmetrics(px, w);
            let bw = (tofu_q8(px) + 255) / 256;
            let bx = pen + 1.max(px as i32 / 12);
            let by = y + v.ascent - v.cap_height;
            let r = Rect::new(
                bx,
                by,
                (bw - 2 * 1.max(px as i32 / 12)).max(2),
                v.cap_height,
            );
            c.stroke_rrect(r, 1, Corner::Circle, color, 200);
            pen += bw;
        }
    }
    flush(c, &mut pen, seg_start, t.len());
    pen - x
}

/// Copy `img` into the box `(x, y, w, h)` (screen coordinates), clipped to `clip`. A picture whose
/// size is not the box's (the layout moved on, an image just arrived) is sampled nearest-neighbour.
fn paint_picture(
    c: &mut Canvas,
    img: &osjeff_core::image::Image,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    clip: Rect,
) {
    let (iw, ih) = (img.width() as i64, img.height() as i64);
    if iw == 0 || ih == 0 {
        return;
    }
    let (w, h) = (i64::from(w.max(1)), i64::from(h.max(1)));
    let px = img.pixels();
    let y0 = y.max(clip.y);
    let y1 = (y + h as i32).min(clip.bottom());
    let x0 = x.max(clip.x);
    let x1 = (x + w as i32).min(clip.right());
    for sy in y0..y1 {
        let iy = ((i64::from(sy - y) * ih / h).min(ih - 1)) as usize;
        let row = &px[iy * iw as usize..(iy + 1) * iw as usize];
        for sx in x0..x1 {
            let ix = ((i64::from(sx - x) * iw / w).min(iw - 1)) as usize;
            let p = row[ix];
            c.put(
                sx as usize,
                sy as usize,
                Color::rgb((p >> 16) as u8, (p >> 8) as u8, p as u8),
            );
        }
    }
}

/// Stroke a border of per-side widths inside `r`.
fn paint_border(c: &mut Canvas, r: Rect, widths: [i32; 4], radius: i32, color: Color) {
    let [t, rt, b, l] = widths;
    let radius = radius.clamp(0, 48).min(r.w / 2).min(r.h / 2);
    if radius > 0 && t == rt && rt == b && b == l {
        for k in 0..t.max(1) {
            c.stroke_rrect(
                r.inflated(-k),
                (radius - k).max(0),
                Corner::Circle,
                color,
                256,
            );
        }
        return;
    }
    let fill = |c: &mut Canvas, rr: Rect| {
        if rr.w > 0 && rr.h > 0 {
            c.fill_rect(
                rr.x.max(0) as usize,
                rr.y.max(0) as usize,
                rr.w as usize,
                rr.h as usize,
                color,
            );
        }
    };
    fill(c, Rect::new(r.x, r.y, r.w, t));
    fill(c, Rect::new(r.x, r.bottom() - b, r.w, b));
    fill(c, Rect::new(r.x, r.y + t, l, r.h - t - b));
    fill(c, Rect::new(r.right() - rt, r.y + t, rt, r.h - t - b));
}

impl Desktop {
    /// The pointer moved: which link of the browser page is under it? Returns whether that
    /// changed (the page repaints the underline).
    pub(crate) fn browser_hover_update(&mut self, cx: i32, cy: i32) -> bool {
        let Some(id) = self.browser_id() else {
            return false;
        };
        let over =
            self.drag.is_none() && !self.overlay_open() && self.topmost_at(cx, cy) == Some(id);
        let Some(rect) = self.wm.get(id).map(|w| w.rect) else {
            return false;
        };
        let content = BrowserChrome::of(rect).content;
        let Some(b) = self.browser_state_mut(id) else {
            return false;
        };
        let new = if over && content.contains(cx, cy) && !b.browser.is_home() {
            b.page
                .as_ref()
                .and_then(|p| p.link_index_at(cx - content.x, cy - content.y + b.scroll))
        } else {
            None
        };
        let changed = new != b.hover_link;
        b.hover_link = new;
        changed
    }

    /// Rasterize a `web` engine display list into the content box, offset by the
    /// current scroll and clipped to the visible area.
    pub(crate) fn paint_web_page(
        &self,
        c: &mut Canvas,
        page: &Page,
        content: Rect,
        scroll: i32,
        bs: &BrowserState,
    ) {
        let Some(clip) = content.intersection(&c.clip_rect()) else {
            return;
        };
        let saved = c.set_clip(clip);
        let m = KernelMetrics;
        let (top, bottom) = (scroll, scroll + content.h);
        let (ox, oy) = (content.x, content.y - scroll);
        // The page's own background, whatever the system appearance.
        c.fill_rect(
            content.x.max(0) as usize,
            content.y.max(0) as usize,
            content.w.max(0) as usize,
            content.h.max(0) as usize,
            rgb(page.background),
        );
        let visible = |y: i32, h: i32| y + h >= top - 2 && y <= bottom + 2;

        // Boxes and pictures first, in document order.
        for cmd in &page.cmds {
            match cmd {
                WebCmd::Rect {
                    x,
                    y,
                    w,
                    h,
                    color,
                    radius,
                } if visible(*y, *h) => {
                    let r = Rect::new(ox + x, oy + y, *w, *h);
                    if *radius > 0 {
                        let rad = (*radius).clamp(0, 48).min(r.w / 2).min(r.h / 2);
                        c.fill_rrect(r, rad, Corner::Circle, rgb(*color), 256);
                    } else {
                        c.fill_rect(
                            r.x.max(0) as usize,
                            r.y.max(0) as usize,
                            r.w.max(0) as usize,
                            r.h.max(0) as usize,
                            rgb(*color),
                        );
                    }
                }
                WebCmd::Image { x, y, w, h, idx } if visible(*y, *h) => {
                    let key = bs.img_keys.get(*idx).and_then(|k| k.as_deref());
                    if let Some(img) = key.and_then(|k| bs.images.image(k)) {
                        paint_picture(c, img, ox + *x, oy + *y, *w, *h, clip);
                    }
                }
                _ => {}
            }
        }

        // Under the text: find matches, then the mouse selection.
        let accent = theme::accent();
        let mark = |c: &mut Canvas, s: &Span, col: Color, alpha: u16| {
            if !visible(s.y, s.h) {
                return;
            }
            c.fill_rrect(
                Rect::new(ox + s.x - 1, oy + s.y, s.w + 2, s.h),
                3,
                Corner::Circle,
                col,
                alpha,
            );
        };
        for s in bs.find.other_spans() {
            mark(c, s, Color::rgb(0xFF, 0xD9, 0x5E), 200);
        }
        for s in bs.find.current_spans() {
            mark(c, s, Color::rgb(0xFF, 0x9F, 0x2E), 256);
        }
        if let Some(sel) = &bs.sel {
            for s in &page.selection_spans(sel, &m) {
                mark(c, s, accent, 92);
            }
        }

        // Text.
        for cmd in &page.cmds {
            let WebCmd::Text {
                x,
                y,
                w,
                h,
                text: t,
                color,
                font,
                deco,
                link,
            } = cmd
            else {
                continue;
            };
            if !visible(*y, *h) || *x > content.w {
                continue;
            }
            let hovered = link.is_some() && bs.hover_link == link.map(|l| l as usize);
            let col = if hovered {
                darker(rgb(*color), 70)
            } else {
                rgb(*color)
            };
            let (sx, sy) = (ox + *x, oy + *y);
            draw_run(c, sx, sy, t, *font, col);
            let underline = deco & DECO_UNDERLINE != 0 || hovered;
            if underline || deco & DECO_STRIKE != 0 {
                let (px, wt) = face_of(*font);
                let v = text::vmetrics(px, wt);
                let thick = (px as i32 / 14).max(1);
                if underline {
                    c.fill_rect(
                        sx.max(0) as usize,
                        (sy + v.ascent + 1 + px as i32 / 12).max(0) as usize,
                        (*w).max(0) as usize,
                        thick as usize,
                        col,
                    );
                }
                if deco & DECO_STRIKE != 0 {
                    c.fill_rect(
                        sx.max(0) as usize,
                        (sy + v.ascent - v.x_height / 2).max(0) as usize,
                        (*w).max(0) as usize,
                        thick as usize,
                        col,
                    );
                }
            }
        }

        // Borders over the content of their boxes.
        for cmd in &page.cmds {
            if let WebCmd::Border {
                x,
                y,
                w,
                h,
                widths,
                radius,
                color,
            } = cmd
                && visible(*y, *h)
            {
                paint_border(
                    c,
                    Rect::new(ox + x, oy + y, *w, *h),
                    *widths,
                    *radius,
                    rgb(*color),
                );
            }
        }

        self.paint_page_controls(c, page, content, scroll, bs);
        c.restore_clip(saved);
    }

    /// Form controls: boxes in the page's light colours, the text they hold, the caret and
    /// the focus ring (the accent).
    fn paint_page_controls(
        &self,
        c: &mut Canvas,
        page: &Page,
        content: Rect,
        scroll: i32,
        bs: &BrowserState,
    ) {
        let focus = bs.forms.focus();
        let accent = theme::accent();
        let ink = Color::rgb(0x1D, 0x1D, 0x1F);
        let hint = Color::rgb(0x8E, 0x8E, 0x93);
        let edge = Color::rgb(0xC7, 0xC7, 0xCC);
        let white = Color::rgb(0xFF, 0xFF, 0xFF);
        for f in &page.fields {
            if f.y + f.h < scroll - 4 || f.y > scroll + content.h + 4 {
                continue;
            }
            let r = Rect::new(content.x + f.x, content.y - scroll + f.y, f.w, f.h);
            let focused = focus == Some((f.form, f.field));
            let info = page.forms.get(f.form).and_then(|x| x.fields.get(f.field));
            let font = Font::new(f.size);
            let (px, wt) = face_of(font);
            let radius = (f.h / 5).clamp(4, 12);
            match f.kind {
                FieldKind::Text | FieldKind::Password => {
                    c.fill_rrect(r, radius, Corner::Circle, white, 256);
                    if focused {
                        c.stroke_rrect(r.inflated(2), radius + 2, Corner::Circle, accent, 90);
                        c.stroke_rrect(r, radius, Corner::Circle, accent, 256);
                    } else {
                        c.stroke_rrect(r, radius, Corner::Circle, edge, 256);
                    }
                    let inner = Rect::new(r.x + f.pad_x, r.y, r.w - 2 * f.pad_x, r.h);
                    let value = bs.forms.value(f.form, f.field);
                    let ty = text::center_y(r.y, r.h, px, wt);
                    let saved = c.set_clip(
                        inner
                            .intersection(&c.clip_rect())
                            .unwrap_or(Rect::new(0, 0, 0, 0)),
                    );
                    let shown: alloc::borrow::Cow<'_, str> = if f.kind == FieldKind::Password {
                        alloc::borrow::Cow::Owned("\u{2022}".repeat(value.chars().count()))
                    } else {
                        alloc::borrow::Cow::Borrowed(value)
                    };
                    if shown.is_empty() {
                        if let Some(ph) = info.map(|i| i.placeholder.as_str()) {
                            text::draw(c, inner.x, ty, ph, px, wt, hint);
                        }
                    } else {
                        // The tail of a value wider than the box, so the caret stays visible.
                        let caret_byte = if focused {
                            bs.forms.caret().min(value.len())
                        } else {
                            value.len()
                        };
                        let caret_chars = value[..caret_byte].chars().count();
                        let caret_text: alloc::borrow::Cow<'_, str> =
                            if f.kind == FieldKind::Password {
                                alloc::borrow::Cow::Owned("\u{2022}".repeat(caret_chars))
                            } else {
                                alloc::borrow::Cow::Borrowed(&value[..caret_byte])
                            };
                        let caret_x = text::measure(&caret_text, px, wt);
                        let shift = (caret_x - inner.w + 2).max(0);
                        text::draw(c, inner.x - shift, ty, &shown, px, wt, ink);
                    }
                    if focused {
                        let value_caret = bs.forms.caret().min(value.len());
                        let before: alloc::borrow::Cow<'_, str> = if f.kind == FieldKind::Password {
                            alloc::borrow::Cow::Owned(
                                "\u{2022}".repeat(value[..value_caret].chars().count()),
                            )
                        } else {
                            alloc::borrow::Cow::Borrowed(&value[..value_caret])
                        };
                        let cx = text::measure(&before, px, wt);
                        let shift = (cx - inner.w + 2).max(0);
                        c.fill_rect(
                            (inner.x + cx - shift).max(0) as usize,
                            (r.y + f.h / 5).max(0) as usize,
                            1,
                            (f.h - 2 * (f.h / 5)).max(1) as usize,
                            accent,
                        );
                    }
                    c.restore_clip(saved);
                }
                FieldKind::Submit | FieldKind::PushButton => {
                    let primary = f.kind == FieldKind::Submit;
                    let fill = if primary {
                        accent
                    } else {
                        Color::rgb(0xF2, 0xF2, 0xF7)
                    };
                    c.fill_rrect(r, radius, Corner::Circle, fill, 256);
                    if !primary {
                        c.stroke_rrect(r, radius, Corner::Circle, edge, 256);
                    }
                    if focused {
                        c.stroke_rrect(r.inflated(2), radius + 2, Corner::Circle, accent, 110);
                    }
                    let label = info.map_or("", |i| i.label.as_str());
                    let w = if primary { Weight::Medium } else { wt };
                    text::draw_centered(c, r, label, px, w, if primary { white } else { ink });
                }
                FieldKind::Checkbox | FieldKind::Radio => {
                    let on = bs.forms.is_checked(f.form, f.field);
                    let round = f.kind == FieldKind::Radio;
                    let rad = if round { f.h / 2 } else { (f.h / 4).max(3) };
                    if on {
                        c.fill_rrect(r, rad, Corner::Circle, accent, 256);
                        if round {
                            c.fill_rrect(r.inflated(-(f.h / 3)), f.h, Corner::Circle, white, 256);
                        } else {
                            ui::draw_glyph(
                                c,
                                osjeff_core::iconart::Glyph::Check,
                                r.x,
                                r.y,
                                f.h,
                                0xFFFF_FFFF,
                            );
                        }
                    } else {
                        c.fill_rrect(r, rad, Corner::Circle, white, 256);
                        c.stroke_rrect(r, rad, Corner::Circle, edge, 256);
                    }
                    if focused {
                        c.stroke_rrect(r.inflated(2), rad + 2, Corner::Circle, accent, 110);
                    }
                }
                FieldKind::Hidden => {}
            }
        }
    }
}
