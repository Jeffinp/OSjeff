//! Keys, clicks, hover and per-frame state of the calculator.

use super::state::COPIED_MS;
use super::state::FLASH_MS;
use super::state::H_COPY;
use super::state::H_DOWN;
use crate::desktop::*;
use kitsune_core::layout::{CALC_KEYS, CalcHit, calc_hit};

impl Desktop {
    /// Feed key `k` to calculator `id` (a click or a typed character) and light its key.
    pub(crate) fn calc_input(&mut self, id: WindowId, k: u8) {
        let k = match k {
            b'x' | b'X' => b'*',
            b':' => b'/',
            b'\r' | b'\n' => b'=',
            other => other,
        };
        if let Some(App::Calculator(c)) = self.app_mut(id) {
            if k == 0x08 {
                c.calc.backspace();
            } else {
                c.calc.input(k);
            }
            c.flash.set((k, FLASH_MS));
        }
    }

    /// Put the display on the clipboard and say so.
    fn calc_copy(&mut self, id: WindowId) {
        let text = match self.app_mut(id) {
            Some(App::Calculator(c)) => {
                c.copied_ms.set(COPIED_MS);
                c.calc.display().to_vec()
            }
            _ => return,
        };
        self.clipboard.set(&text);
    }

    pub(crate) fn calc_click(&mut self, id: WindowId, rect: Rect, px: i32, py: i32) {
        match calc_hit(rect, px, py) {
            Some(CalcHit::Key(k)) => self.calc_input(id, k),
            Some(CalcHit::Copy) => self.calc_copy(id),
            None => {}
        }
    }

    pub(crate) fn calc_hover(&mut self, cx: i32, cy: i32, down: bool) -> bool {
        let top = if self.overlay_open() {
            None
        } else {
            self.topmost_at(cx, cy)
        };
        let mut changed = false;
        let mut dirty = Vec::new();
        for w in self.wm.windows() {
            let App::Calculator(c) = &w.app.app else {
                continue;
            };
            if !w.shown() {
                continue;
            }
            let mut key = 0;
            if Some(w.id) == top {
                key = match calc_hit(w.rect, cx, cy) {
                    Some(CalcHit::Key(k)) => CALC_KEYS
                        .iter()
                        .flatten()
                        .position(|&x| x == k)
                        .map_or(0, |i| i as u32 + 1),
                    Some(CalcHit::Copy) => H_COPY,
                    None => 0,
                };
                if key != 0 && down {
                    key |= H_DOWN;
                }
            }
            if key != c.hover.get() {
                c.hover.set(key);
                changed = true;
                dirty.push(self.window_box(w));
            }
        }
        for r in dirty {
            self.mark_dirty(r);
        }
        changed
    }

    pub(crate) fn calc_step(&mut self, dt_ms: u32) {
        for w in self.wm.windows() {
            if let App::Calculator(c) = &w.app.app {
                let (k, ms) = c.flash.get();
                c.flash.set((k, ms.saturating_sub(dt_ms)));
                c.copied_ms.set(c.copied_ms.get().saturating_sub(dt_ms));
            }
        }
    }

    pub(crate) fn calc_busy_one(&self, w: &Win) -> bool {
        let App::Calculator(c) = &w.app.app else {
            return false;
        };
        w.shown() && (c.flash.get().1 > 0 || c.copied_ms.get() > 0)
    }
}
