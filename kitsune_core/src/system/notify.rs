//! Toast notifications: the pure model behind the corner overlay.
//!
//! [`Toasts`] shows at most [`MAX_VISIBLE`] banners stacked down from the top-right
//! corner (under the menu bar), each for [`LIFETIME_MS`]; more wait in a short queue and take a slot as
//! soon as one frees up. A repeat of a message already on screen only restarts
//! its timer and bumps a counter, so a loop that logs the same warning cannot
//! flood the desktop. A click on a toast dismisses it.
//!
//! The model never draws and has no allocation. The compositor asks
//! [`Toasts::tick`] whether anything changed and repaints [`Toasts::bounds`]
//! only then; with no toast on screen there is nothing to do at all
//! ([`Toasts::is_idle`]).

use crate::system::klog::Level;
use crate::windowing::window::Rect;

/// Toasts on screen at once.
pub const MAX_VISIBLE: usize = 3;
/// Messages waiting for a free slot.
pub const QUEUE_CAP: usize = 8;
/// How long a toast stays by default, in milliseconds (the settings change it with
/// [`Toasts::set_lifetime_secs`]).
pub const LIFETIME_MS: u32 = 4000;
/// The bounds of a chosen lifetime, in seconds.
pub const LIFETIME_SECS_MIN: u32 = 2;
pub const LIFETIME_SECS_MAX: u32 = 15;
/// Longest message (bytes); the rest is cut.
pub const TEXT_CAP: usize = 64;
pub use crate::ui::chrome::{TOAST_GAP as GAP, TOAST_H, TOAST_W};
/// Milliseconds a banner takes to slide in from the right edge, and to slide out
/// again before it expires.
pub const SLIDE_IN_MS: u32 = 260;
pub const SLIDE_OUT_MS: u32 = 220;

/// One notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Toast {
    pub level: Level,
    text: [u8; TEXT_CAP],
    len: u8,
    /// Times the same message arrived while it was on screen (1 = once).
    pub count: u8,
    born_ms: u32,
    /// How long it stays, in milliseconds.
    life_ms: u32,
}

impl Toast {
    fn new(level: Level, text: &[u8], now_ms: u32, life_ms: u32) -> Toast {
        let n = text.len().min(TEXT_CAP);
        let mut t = [0u8; TEXT_CAP];
        t[..n].copy_from_slice(&text[..n]);
        Toast {
            level,
            text: t,
            len: n as u8,
            count: 1,
            born_ms: now_ms,
            life_ms,
        }
    }

    /// Time left, as 256 (just appeared) down to 0 (gone): the auto-dismiss line.
    pub fn remaining(&self, now_ms: u32) -> u32 {
        let left = self.life_ms.saturating_sub(self.age_ms(now_ms)) as u64;
        (left * 256 / self.life_ms.max(1) as u64) as u32
    }

    pub fn text(&self) -> &[u8] {
        &self.text[..self.len as usize]
    }

    fn expired(&self, now_ms: u32) -> bool {
        now_ms.wrapping_sub(self.born_ms) >= self.life_ms
    }

    /// Milliseconds since it appeared (or was last repeated).
    pub fn age_ms(&self, now_ms: u32) -> u32 {
        now_ms.wrapping_sub(self.born_ms)
    }

    /// How far off its resting place the banner is, 0..=256 (256 = fully outside the
    /// screen to the right): it slides in when it appears and out before it expires.
    pub fn slide(&self, now_ms: u32) -> u32 {
        let age = self.age_ms(now_ms);
        let left = self.life_ms.saturating_sub(age);
        if age < SLIDE_IN_MS {
            // Ease-out cubic.
            let t = 256 - age * 256 / SLIDE_IN_MS;
            t * t / 256 * t / 256
        } else if left < SLIDE_OUT_MS {
            // Ease-in.
            let t = 256 - left * 256 / SLIDE_OUT_MS;
            t * t / 256
        } else {
            0
        }
    }
}

/// The visible stack plus the waiting queue.
#[derive(Clone, Debug)]
pub struct Toasts {
    vis: [Option<Toast>; MAX_VISIBLE],
    queue: [Option<Toast>; QUEUE_CAP],
    /// Messages thrown away because the queue was full.
    pub dropped: u32,
    /// Lifetime given to the toasts that appear from now on.
    life_ms: u32,
}

impl Default for Toasts {
    fn default() -> Self {
        Self::new()
    }
}

impl Toasts {
    pub const fn new() -> Self {
        Self {
            vis: [None; MAX_VISIBLE],
            queue: [None; QUEUE_CAP],
            dropped: 0,
            life_ms: LIFETIME_MS,
        }
    }

    /// Toasts that appear from now on stay `secs` seconds (clamped to 2..=15).
    pub fn set_lifetime_secs(&mut self, secs: u32) {
        self.life_ms = secs.clamp(LIFETIME_SECS_MIN, LIFETIME_SECS_MAX) * 1000;
    }

    /// Nothing visible and nothing waiting: the compositor can skip all work.
    pub fn is_idle(&self) -> bool {
        self.vis[0].is_none()
    }

    pub fn visible_count(&self) -> usize {
        self.vis.iter().flatten().count()
    }

    /// The visible toasts, top of the stack first.
    pub fn iter(&self) -> impl Iterator<Item = &Toast> {
        self.vis.iter().flatten()
    }

    /// Show a message. Returns whether the picture changed (always, unless the
    /// message had to wait in the queue or was dropped).
    pub fn push(&mut self, level: Level, text: &[u8], now_ms: u32) -> bool {
        let text = &text[..text.len().min(TEXT_CAP)];
        // A repeat of something on screen restarts it instead of stacking.
        for t in self.vis.iter_mut().flatten() {
            if t.level == level && t.text() == text {
                t.count = t.count.saturating_add(1);
                t.born_ms = now_ms;
                return true;
            }
        }
        let toast = Toast::new(level, text, now_ms, self.life_ms);
        if let Some(slot) = self.vis.iter_mut().find(|s| s.is_none()) {
            *slot = Some(toast);
            return true;
        }
        // Same message already waiting: count it, do not queue twice.
        for t in self.queue.iter_mut().flatten() {
            if t.level == level && t.text() == text {
                t.count = t.count.saturating_add(1);
                return false;
            }
        }
        match self.queue.iter_mut().find(|s| s.is_none()) {
            Some(slot) => *slot = Some(toast),
            None => self.dropped += 1,
        }
        false
    }

    /// Remove toasts whose time is up and promote queued ones into the freed
    /// slots. Returns whether the picture changed.
    pub fn tick(&mut self, now_ms: u32) -> bool {
        let mut changed = false;
        for s in self.vis.iter_mut() {
            if s.is_some_and(|t| t.expired(now_ms)) {
                *s = None;
                changed = true;
            }
        }
        changed |= self.compact_and_fill(now_ms);
        changed
    }

    /// Slide the stack down over holes and move queued messages in.
    fn compact_and_fill(&mut self, now_ms: u32) -> bool {
        let before = self.vis;
        let mut n = 0;
        for i in 0..MAX_VISIBLE {
            if self.vis[i].is_some() {
                self.vis[n] = self.vis[i];
                n += 1;
            }
        }
        for s in self.vis.iter_mut().skip(n) {
            *s = None;
        }
        while n < MAX_VISIBLE {
            let Some(mut t) = self.pop_queue() else { break };
            t.born_ms = now_ms;
            self.vis[n] = Some(t);
            n += 1;
        }
        before != self.vis
    }

    fn pop_queue(&mut self) -> Option<Toast> {
        let first = self.queue.iter().position(Option::is_some)?;
        let t = self.queue[first].take();
        // Keep the queue dense and FIFO.
        self.queue[first..].rotate_left(1);
        t
    }

    /// Dismiss every toast and forget the queue.
    pub fn clear(&mut self) {
        self.vis = [None; MAX_VISIBLE];
        self.queue = [None; QUEUE_CAP];
    }

    /// Screen rect of visible slot `i` (0 = topmost), at rest.
    pub fn rect(i: usize, sw: i32, _sh: i32) -> Rect {
        crate::ui::chrome::toast_rect(i, sw)
    }

    /// Does any banner move (slide in or out) at `now_ms`? It then needs a frame.
    pub fn sliding(&self, now_ms: u32) -> bool {
        self.iter().any(|t| t.slide(now_ms) != 0)
    }

    /// Everything the toasts may touch (all slots, the drop shadow and the strip
    /// they slide across), for restoring the scene under them; empty when idle.
    pub fn bounds(&self, sw: i32, sh: i32) -> Rect {
        let n = self.visible_count();
        if n == 0 {
            return Rect::new(0, 0, 0, 0);
        }
        let r = Self::rect(0, sw, sh)
            .union(&Self::rect(n - 1, sw, sh))
            .inflated(30);
        // Banners come from beyond the right edge: the strip reaches it.
        Rect::new(r.x, r.y, (sw - r.x).max(r.w), r.h).clamped_to(sw, sh)
    }

    /// A left click at `(px, py)`: dismiss the toast under it. Returns whether
    /// one was hit (the click is then consumed).
    pub fn click(&mut self, px: i32, py: i32, sw: i32, sh: i32, now_ms: u32) -> bool {
        for i in 0..self.visible_count() {
            if Self::rect(i, sw, sh).contains(px, py) {
                self.vis[i] = None;
                self.compact_and_fill(now_ms);
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests;
