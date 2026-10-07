//! The toast overlay: notifications stacked above the clock pill.
//!
//! The model (three visible, a short queue, 4 s each, merging repeats) is
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
use osjeff_core::notify::{TOAST_H, TOAST_W, Toasts};

/// Milliseconds since boot (the clock the toasts are timed with).
pub(crate) fn now_ms() -> u32 {
    klog::ticks_to_ms_now()
}

fn level_color(l: Level) -> Color {
    match l {
        Level::Warn => Color::rgb(0xFF, 0xC1, 0x4D),
        Level::Error => theme::CLOSE,
        Level::Fatal => Color::rgb(0xFF, 0x4D, 0x9D),
        _ => theme::accent(),
    }
}

fn level_title(l: Level) -> &'static [u8] {
    match l {
        Level::Trace | Level::Debug | Level::Info => b"INFO",
        Level::Warn => b"AVISO",
        Level::Error => b"ERRO",
        Level::Fatal => b"FATAL",
    }
}

impl Desktop {
    /// Show a toast (the notifications setting permitting).
    pub(crate) fn push_toast(&mut self, level: Level, text: &[u8]) {
        if crate::settings::toasts_enabled() && self.toasts.push(level, text, now_ms()) {
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

    /// Draw every visible toast onto `c` (the framebuffer).
    pub fn draw_toasts(&self, c: &mut Canvas) {
        for (i, t) in self.toasts.iter().enumerate() {
            let r = Toasts::rect(i, self.sw, self.sh);
            // Soft shadow, body, level colour bar.
            c.fill_round_rect_alpha(
                (r.x + 3) as usize,
                (r.y + 5) as usize,
                TOAST_W as usize,
                TOAST_H as usize,
                12,
                theme::SHADOW,
                60,
            );
            c.fill_round_rect(
                r.x as usize,
                r.y as usize,
                TOAST_W as usize,
                TOAST_H as usize,
                12,
                theme::DOCK,
            );
            let col = level_color(t.level);
            c.fill_round_rect(
                (r.x + 8) as usize,
                (r.y + 8) as usize,
                5,
                (TOAST_H - 16) as usize,
                2,
                col,
            );
            text(c, r.x + 22, r.y + 7, 120, level_title(t.level), col);
            if t.count > 1 {
                let mut n = osjeff_core::klog::FixedBuf::<6>::new();
                let _ = core::fmt::Write::write_fmt(&mut n, format_args!("x{}", t.count));
                text_right(
                    c,
                    Rect::new(r.x, r.y + 7 - (CELL_H - 14) / 2, TOAST_W - 12, CELL_H),
                    n.as_bytes(),
                    DIM_TEXT,
                );
            }
            // Body at scale 1: two lines of up to 55 characters.
            let body = t.text();
            let cols = ((TOAST_W - 34) / 6) as usize;
            let (l1, l2) = body.split_at(body.len().min(cols));
            font::draw_bytes(
                c,
                (r.x + 22) as usize,
                (r.y + 26) as usize,
                l1,
                theme::HEADER_TEXT,
                1,
            );
            font::draw_bytes(
                c,
                (r.x + 22) as usize,
                (r.y + 36) as usize,
                &l2[..l2.len().min(cols)],
                theme::HEADER_TEXT,
                1,
            );
        }
    }
}
