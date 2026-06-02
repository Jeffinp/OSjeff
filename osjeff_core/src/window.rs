//! Window geometry & hit-testing. Pure integer math, no rendering.

/// Title bar height in pixels.
pub const TITLE_H: i32 = 30;
/// Close-button square side.
pub const CLOSE: i32 = 18;
/// Inset of the close button from the title bar's top-right corner.
pub const CLOSE_INSET: i32 = 8;
const CLOSE_TOP: i32 = 6;

/// An axis-aligned rectangle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// Half-open containment: `[x, x+w) × [y, y+h)`.
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    /// The close button rectangle for this window.
    pub fn close_rect(&self) -> Rect {
        Rect::new(
            self.x + self.w - CLOSE - CLOSE_INSET,
            self.y + CLOSE_TOP,
            CLOSE,
            CLOSE,
        )
    }

    /// True when the point is on the close button.
    pub fn on_close(&self, px: i32, py: i32) -> bool {
        self.close_rect().contains(px, py)
    }

    /// True when the point is on the draggable title area (excludes close button).
    pub fn on_title(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && px < self.x + self.w
            && py >= self.y
            && py < self.y + TITLE_H
            && !self.on_close(px, py)
    }

    /// The content area below the title bar.
    pub fn body(&self) -> Rect {
        Rect::new(self.x, self.y + TITLE_H, self.w, (self.h - TITLE_H).max(0))
    }

    /// Clamp a proposed top-left so the title bar stays on a `sw × sh` screen.
    pub fn clamped_pos(&self, sw: i32, sh: i32) -> (i32, i32) {
        let x = self.x.clamp(0, (sw - self.w).max(0));
        let y = self.y.clamp(0, (sh - TITLE_H).max(0));
        (x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win() -> Rect {
        Rect::new(100, 100, 400, 300)
    }

    #[test]
    fn contains_is_half_open() {
        let r = win();
        assert!(r.contains(100, 100));
        assert!(r.contains(499, 399));
        assert!(!r.contains(500, 400));
        assert!(!r.contains(99, 100));
    }

    #[test]
    fn close_button_top_right() {
        let r = win();
        let c = r.close_rect();
        assert_eq!(c, Rect::new(100 + 400 - 18 - 8, 106, 18, 18));
        assert!(r.on_close(c.x + 1, c.y + 1));
        assert!(!r.on_close(r.x + 5, r.y + 5));
    }

    #[test]
    fn title_excludes_close_button() {
        let r = win();
        assert!(r.on_title(r.x + 10, r.y + 10));
        let c = r.close_rect();
        assert!(!r.on_title(c.x + 1, c.y + 1));
    }

    #[test]
    fn title_band_height() {
        let r = win();
        assert!(r.on_title(r.x + 5, r.y + TITLE_H - 1));
        assert!(!r.on_title(r.x + 5, r.y + TITLE_H));
    }

    #[test]
    fn body_is_below_title() {
        let r = win();
        let b = r.body();
        assert_eq!(b, Rect::new(100, 130, 400, 270));
    }

    #[test]
    fn clamp_keeps_window_on_screen() {
        let r = Rect::new(-50, -20, 400, 300);
        assert_eq!(r.clamped_pos(1280, 800), (0, 0));
        let r2 = Rect::new(2000, 2000, 400, 300);
        assert_eq!(r2.clamped_pos(1280, 800), (1280 - 400, 800 - TITLE_H));
    }

    #[test]
    fn clamp_handles_oversized_window() {
        // Width exceeds screen -> x pinned to 0. Height never constrains y
        // (only the title bar must stay visible), so y is unchanged.
        let r = Rect::new(10, 10, 2000, 2000);
        assert_eq!(r.clamped_pos(1280, 800), (0, 10));
    }
}
