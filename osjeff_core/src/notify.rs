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

use crate::klog::Level;
use crate::window::Rect;

/// Toasts on screen at once.
pub const MAX_VISIBLE: usize = 3;
/// Messages waiting for a free slot.
pub const QUEUE_CAP: usize = 8;
/// How long a toast stays, in milliseconds.
pub const LIFETIME_MS: u32 = 4000;
/// Longest message (bytes); the rest is cut.
pub const TEXT_CAP: usize = 64;
pub use crate::chrome::{TOAST_GAP as GAP, TOAST_H, TOAST_W};
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
}

impl Toast {
    fn new(level: Level, text: &[u8], now_ms: u32) -> Toast {
        let n = text.len().min(TEXT_CAP);
        let mut t = [0u8; TEXT_CAP];
        t[..n].copy_from_slice(&text[..n]);
        Toast {
            level,
            text: t,
            len: n as u8,
            count: 1,
            born_ms: now_ms,
        }
    }

    pub fn text(&self) -> &[u8] {
        &self.text[..self.len as usize]
    }

    fn expired(&self, now_ms: u32) -> bool {
        now_ms.wrapping_sub(self.born_ms) >= LIFETIME_MS
    }

    /// Milliseconds since it appeared (or was last repeated).
    pub fn age_ms(&self, now_ms: u32) -> u32 {
        now_ms.wrapping_sub(self.born_ms)
    }

    /// How far off its resting place the banner is, 0..=256 (256 = fully outside the
    /// screen to the right): it slides in when it appears and out before it expires.
    pub fn slide(&self, now_ms: u32) -> u32 {
        let age = self.age_ms(now_ms);
        let left = LIFETIME_MS.saturating_sub(age);
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
        }
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
        let toast = Toast::new(level, text, now_ms);
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
        crate::chrome::toast_rect(i, sw)
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
            .inflated(14);
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
mod tests {
    use super::*;

    const SW: i32 = 1280;
    const SH: i32 = 720;

    fn texts(t: &Toasts) -> alloc::vec::Vec<alloc::vec::Vec<u8>> {
        t.iter().map(|x| x.text().to_vec()).collect()
    }

    #[test]
    fn starts_idle() {
        let t = Toasts::new();
        assert!(t.is_idle());
        assert_eq!(t.visible_count(), 0);
        assert!(t.bounds(SW, SH).is_empty());
    }

    #[test]
    fn shows_up_to_three_and_queues_the_rest() {
        let mut t = Toasts::new();
        for (i, m) in [b"a", b"b", b"c", b"d", b"e"].iter().enumerate() {
            let shown = t.push(Level::Warn, *m, i as u32);
            assert_eq!(shown, i < 3, "{i}");
        }
        assert_eq!(texts(&t), [b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);
        assert!(!t.is_idle());
    }

    #[test]
    fn expires_after_four_seconds_and_promotes_the_queue() {
        let mut t = Toasts::new();
        t.push(Level::Error, b"first", 0);
        t.push(Level::Warn, b"second", 1000);
        t.push(Level::Warn, b"third", 1000);
        t.push(Level::Warn, b"fourth", 1000);
        assert!(!t.tick(3999));
        assert_eq!(t.visible_count(), 3);
        // "first" expires; "fourth" takes the freed slot.
        assert!(t.tick(4000));
        assert_eq!(
            texts(&t),
            [b"second".to_vec(), b"third".to_vec(), b"fourth".to_vec()]
        );
        // The promoted one is timed from when it appeared.
        assert!(t.tick(5000)); // second + third (born 1000) expire
        assert_eq!(texts(&t), [b"fourth".to_vec()]);
        assert!(!t.tick(7999));
        assert!(t.tick(8000));
        assert!(t.is_idle());
        assert!(!t.tick(8001));
    }

    #[test]
    fn repeats_do_not_stack() {
        let mut t = Toasts::new();
        t.push(Level::Warn, b"disk slow", 0);
        t.push(Level::Warn, b"disk slow", 3000);
        t.push(Level::Warn, b"disk slow", 3500);
        assert_eq!(t.visible_count(), 1);
        assert_eq!(t.iter().next().unwrap().count, 3);
        // The timer restarted at the last repeat.
        assert!(!t.tick(7000));
        assert!(t.tick(7500));
        // Same text, different level is a different toast.
        t.push(Level::Warn, b"x", 0);
        t.push(Level::Error, b"x", 0);
        assert_eq!(t.visible_count(), 2);
    }

    #[test]
    fn queue_overflow_is_counted_and_never_panics() {
        let mut t = Toasts::new();
        for i in 0..100u32 {
            let msg = [b'a' + (i % 26) as u8, b'0' + (i / 26) as u8];
            t.push(Level::Warn, &msg, 0);
        }
        assert_eq!(t.visible_count(), MAX_VISIBLE);
        assert_eq!(t.dropped as usize, 100 - MAX_VISIBLE - QUEUE_CAP);
    }

    #[test]
    fn click_dismisses_and_compacts() {
        let mut t = Toasts::new();
        t.push(Level::Warn, b"a", 0);
        t.push(Level::Warn, b"b", 0);
        t.push(Level::Warn, b"c", 0);
        t.push(Level::Warn, b"d", 0);
        let r0 = Toasts::rect(0, SW, SH);
        // Click the middle one (slot 1).
        let r1 = Toasts::rect(1, SW, SH);
        assert!(t.click(r1.x + 5, r1.y + 5, SW, SH, 100));
        assert_eq!(texts(&t), [b"a".to_vec(), b"c".to_vec(), b"d".to_vec()]);
        // A click elsewhere is not consumed.
        assert!(!t.click(5, 5, SW, SH, 100));
        // Slot 0 click.
        assert!(t.click(r0.x + 1, r0.y + 1, SW, SH, 100));
        assert_eq!(texts(&t), [b"c".to_vec(), b"d".to_vec()]);
    }

    #[test]
    fn geometry_stacks_down_from_the_top_right() {
        let r0 = Toasts::rect(0, SW, SH);
        let r1 = Toasts::rect(1, SW, SH);
        assert_eq!(r0.right() + crate::chrome::TOAST_MARGIN, SW);
        // Under the menu bar, the next one below with a gap.
        assert!(r0.y > crate::style::MENUBAR_H);
        assert_eq!(r0.bottom() + GAP, r1.y);
        let mut t = Toasts::new();
        t.push(Level::Warn, b"a", 0);
        t.push(Level::Warn, b"b", 0);
        let b = t.bounds(SW, SH);
        assert!(b.contains(r0.x, r0.y) && b.contains(r1.x, r1.y));
        // The strip reaches the screen edge for the slide.
        assert_eq!(b.right(), SW);
    }

    #[test]
    fn slide_comes_in_from_the_right_and_leaves_before_expiring() {
        let mut t = Toasts::new();
        t.push(Level::Info, b"hello", 1000);
        let toast = *t.iter().next().unwrap();
        // Starts fully outside, ends at rest, and only ever eases one way.
        assert_eq!(toast.slide(1000), 256);
        let mut last = 256;
        for ms in (0..=SLIDE_IN_MS).step_by(10) {
            let v = toast.slide(1000 + ms);
            assert!(v <= last);
            last = v;
        }
        assert_eq!(toast.slide(1000 + SLIDE_IN_MS), 0);
        assert_eq!(toast.slide(1000 + 2000), 0);
        // The exit starts SLIDE_OUT_MS before the end and grows.
        let mut last = 0;
        for ms in (LIFETIME_MS - SLIDE_OUT_MS..LIFETIME_MS).step_by(10) {
            let v = toast.slide(1000 + ms);
            assert!(v >= last);
            last = v;
        }
        assert!(last > 200);
        assert!(t.sliding(1000) && !t.sliding(2500) && t.sliding(1000 + LIFETIME_MS - 50));
    }

    #[test]
    fn long_text_is_cut_and_timer_wraps() {
        let mut t = Toasts::new();
        t.push(Level::Info, &[b'x'; 200], u32::MAX - 100);
        assert_eq!(t.iter().next().unwrap().text().len(), TEXT_CAP);
        // The clock wraps past u32::MAX: still expires on time.
        assert!(!t.tick(u32::MAX));
        assert!(t.tick(LIFETIME_MS - 101));
    }

    #[test]
    fn clear_empties_everything() {
        let mut t = Toasts::new();
        for _ in 0..3 {
            t.push(Level::Warn, b"a", 0);
        }
        t.push(Level::Error, b"queued", 0);
        t.clear();
        assert!(t.is_idle());
        assert!(!t.tick(100));
    }
}
