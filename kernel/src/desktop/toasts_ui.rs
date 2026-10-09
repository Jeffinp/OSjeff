//! The toast overlay: notification banners stacked down from the top-right corner.
//!
//! The model (three visible, a short queue, a chosen time each, merging repeats) is
//! `osjeff_core::notify::Toasts`. The desktop feeds it from two places:
//! WARN-and-above records of the system log (`klog::take_warnings`, checked with
//! one atomic load per compositor pass) and the queue behind `notify::notify`.
//! The compositor repaints the toasts straight onto the framebuffer after a
//! frame (restoring the scene under them from the back buffer, like the HUD), and
//! only while some toast is on screen or has just left: an idle desktop with no
//! toast does nothing here beyond those loads.

use super::ui::*;
use super::*;
use crate::klog::{self, Level};
use osjeff_core::notify::{TOAST_W, Toasts};

/// Milliseconds since boot (the clock the toasts are timed with).
pub(crate) fn now_ms() -> u32 {
    klog::ticks_to_ms_now()
}

fn level_color(l: Level) -> Color {
    match l {
        Level::Warn => Color::rgb(0xF5, 0xA6, 0x23),
        Level::Error => theme::CLOSE,
        Level::Fatal => Color::rgb(0xFF, 0x4D, 0x9D),
        _ => theme::accent(),
    }
}

fn level_title(l: Level) -> &'static str {
    osjeff_core::i18n::tr(l.title_key())
}

impl Desktop {
    /// Show a toast (the notifications setting permitting).
    pub(crate) fn push_toast(&mut self, level: Level, text: &[u8]) {
        if !crate::settings::toasts_enabled() {
            return;
        }
        self.toasts
            .set_lifetime_secs(crate::settings::get().toast_secs as u32);
        if self.toasts.push(level, text, now_ms()) {
            self.toast_dirty = true;
        }
    }

    /// Gather new events (WARN+ log records, queued `notify` calls), expire old
    /// toasts. Returns whether the picture changed. Cheap when idle: two atomic loads.
    pub fn poll_toasts(&mut self, now_ms: u32) -> bool {
        let mut changed = core::mem::take(&mut self.toast_dirty);
        let mut warns = [None; 4];
        let n = klog::take_warnings(&mut self.toast_seen, &mut warns);
        for w in warns.iter().take(n).flatten() {
            self.push_toast(w.level, w.text());
        }
        if crate::notify::pending() {
            let mut queued: [Option<(Level, [u8; 64], usize)>; 8] = [None; 8];
            let mut k = 0;
            crate::notify::drain(|level, text| {
                if k < queued.len() {
                    let mut t = [0u8; 64];
                    let l = text.len().min(64);
                    t[..l].copy_from_slice(&text[..l]);
                    queued[k] = Some((level, t, l));
                    k += 1;
                }
            });
            for (level, t, l) in queued.iter().flatten() {
                self.push_toast(*level, &t[..*l]);
            }
        }
        changed |= self.toasts.tick(now_ms);
        changed
    }

    /// True when no toast is visible (nothing to repaint).
    pub fn toasts_idle(&self) -> bool {
        self.toasts.is_idle()
    }

    /// Screen area the toasts occupy (empty when idle).
    pub fn toast_bounds(&self) -> Rect {
        self.toasts.bounds(self.sw, self.sh)
    }

    /// Does a banner need frames: one is sliding, or its dismiss line is running down (the
    /// line steps once a second with reduced motion).
    pub(crate) fn toasts_sliding(&self) -> bool {
        if osjeff_core::anim::reduce_motion() {
            self.toasts.sliding(now_ms())
        } else {
            !self.toasts.is_idle()
        }
    }

    /// Draw every visible banner onto `c` (the framebuffer): a rounded card with the level's
    /// icon, a title, up to two lines of text, a repeat counter, a close button while the
    /// pointer is over it and a hairline at the bottom that runs down as the banner nears
    /// its end. Banners slide in from the right edge and out again before they expire.
    pub fn draw_toasts(&self, c: &mut Canvas) {
        use crate::text::{self, BODY, FOOTNOTE, Weight};
        let p = theme::pal();
        let now = now_ms();
        for (i, t) in self.toasts.iter().enumerate() {
            let mut r = Toasts::rect(i, self.sw, self.sh);
            let off = (t.slide(now) as i64 * (TOAST_W + 32) as i64 / 256) as i32;
            r.x += off;
            let hole = Rect::new(r.x, r.y + 14, r.w, r.h - 28);
            c.draw_shadow(
                r,
                Shadow {
                    blur: 14,
                    dy: 8,
                    alpha: 80,
                },
                hole,
            );
            fill_token(
                c,
                r,
                14,
                if theme::dark() {
                    0xF22E_2E32
                } else {
                    0xF2FA_FAFC
                },
            );
            stroke_token(c, r, 14, p.separator);
            let hovered = r.contains(self.cursor_x, self.cursor_y);
            // Level icon: a disc in the level's colour with its glyph.
            let col = level_color(t.level);
            let disc = Rect::new(r.x + 16, r.y + (r.h - 32) / 2, 32, 32);
            c.fill_rrect(disc, 16, Corner::Circle, col, 256);
            match t.level {
                Level::Trace | Level::Debug | Level::Info => draw_glyph(
                    c,
                    iconart::Glyph::Info,
                    disc.x + 6,
                    disc.y + 6,
                    20,
                    0xFFFF_FFFF,
                ),
                Level::Warn => {
                    text::draw_centered(c, disc, "!", 20, Weight::Semibold, theme::WHITE)
                }
                Level::Error | Level::Fatal => draw_glyph(
                    c,
                    iconart::Glyph::Close,
                    disc.x + 6,
                    disc.y + 6,
                    20,
                    0xFFFF_FFFF,
                ),
            }
            let tx = disc.right() + 12;
            let right_pad = if t.count > 1 { 44 } else { 16 };
            let tw = r.right() - tx - right_pad;
            text::draw_left(
                c,
                Rect::new(tx, r.y + 10, tw, 20),
                level_title(t.level),
                BODY,
                Weight::Semibold,
                theme::solid(p.text),
            );
            if t.count > 1 {
                let n = osjeff_core::t!("toast.repeat", n = t.count);
                text::draw_right(
                    c,
                    Rect::new(r.x, r.y + 10, r.w - 14, 20),
                    &n,
                    FOOTNOTE,
                    Weight::Medium,
                    theme::solid(p.text_secondary),
                );
            }
            let body = crate::text::from_bytes(t.text());
            let lines = text::wrap(&body, FOOTNOTE, Weight::Regular, r.right() - tx - 16, 2);
            for (k, (a, b)) in lines.into_iter().enumerate() {
                text::draw(
                    c,
                    tx,
                    r.y + 31 + k as i32 * 15,
                    &body[a..b],
                    FOOTNOTE,
                    Weight::Regular,
                    theme::solid(p.text_secondary),
                );
            }
            // The auto-dismiss line along the bottom edge, inside the rounded corners.
            let left = t.remaining(now) as i64;
            let line_w = ((r.w - 28) as i64 * left / 256) as i32;
            if line_w > 0 {
                c.fill_rrect(
                    Rect::new(r.x + 14, r.bottom() - 4, line_w, 2),
                    1,
                    Corner::Circle,
                    col,
                    if theme::dark() { 170 } else { 150 },
                );
            }
            // The close button, over the top-left corner, while the pointer is on the banner.
            if hovered {
                let b = Rect::new(r.x - 7, r.y - 7, 20, 20);
                c.draw_shadow(
                    b,
                    Shadow {
                        blur: 4,
                        dy: 1,
                        alpha: 80,
                    },
                    Rect::new(b.x, b.y + 4, b.w, b.h - 8),
                );
                c.fill_rrect(
                    b,
                    10,
                    Corner::Circle,
                    if theme::dark() {
                        Color::rgb(0x4A, 0x4A, 0x4E)
                    } else {
                        Color::rgb(0xFF, 0xFF, 0xFF)
                    },
                    256,
                );
                stroke_token(c, b, 10, p.separator);
                draw_glyph(
                    c,
                    iconart::Glyph::Close,
                    b.x + 4,
                    b.y + 4,
                    12,
                    0xFF00_0000 | (p.text_secondary & 0xFF_FFFF),
                );
            }
        }
    }
}
