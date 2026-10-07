//! Window geometry & hit-testing. Pure integer math, no rendering.

/// Title bar height in pixels.
pub const TITLE_H: i32 = 30;
/// Close-button square side.
pub const CLOSE: i32 = 18;
/// Inset of the close button from the title bar's top-right corner.
pub const CLOSE_INSET: i32 = 8;
const CLOSE_TOP: i32 = 6;
/// Horizontal gap between adjacent title-bar buttons.
pub const BTN_GAP: i32 = 6;
/// Thickness of the invisible resize band along a window's border.
pub const RESIZE_BAND: i32 = 5;
/// Length of the corner zone along each border (a bit larger than the band so
/// corners are easy to hit).
pub const RESIZE_CORNER: i32 = 14;

/// A strongly typed window handle. Ids are handed out by
/// [`crate::winman::WindowManager`], increase monotonically and are never
/// reused, so a stale id can only miss, never alias another window.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct WindowId(u32);

impl WindowId {
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }
}

/// Which border or corner of a window a resize drag grabbed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResizeEdge {
    N,
    S,
    E,
    W,
    NE,
    NW,
    SE,
    SW,
}

impl ResizeEdge {
    /// `(dx, dy)` sign pair: `-1` means the low (left/top) side moves, `1` the
    /// high (right/bottom) side, `0` the axis is untouched.
    pub const fn axes(self) -> (i32, i32) {
        match self {
            Self::N => (0, -1),
            Self::S => (0, 1),
            Self::E => (1, 0),
            Self::W => (-1, 0),
            Self::NE => (1, -1),
            Self::NW => (-1, -1),
            Self::SE => (1, 1),
            Self::SW => (-1, 1),
        }
    }

    const fn from_axes(x: i32, y: i32) -> Option<Self> {
        Some(match (x, y) {
            (0, -1) => Self::N,
            (0, 1) => Self::S,
            (1, 0) => Self::E,
            (-1, 0) => Self::W,
            (1, -1) => Self::NE,
            (-1, -1) => Self::NW,
            (1, 1) => Self::SE,
            (-1, 1) => Self::SW,
            _ => return None,
        })
    }
}

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

    /// The maximize/restore button: left of the close button.
    pub fn max_rect(&self) -> Rect {
        let c = self.close_rect();
        Rect::new(c.x - CLOSE - BTN_GAP, c.y, CLOSE, CLOSE)
    }

    /// The minimize button: left of the maximize button.
    pub fn min_rect(&self) -> Rect {
        let m = self.max_rect();
        Rect::new(m.x - CLOSE - BTN_GAP, m.y, CLOSE, CLOSE)
    }

    pub fn on_max(&self, px: i32, py: i32) -> bool {
        self.max_rect().contains(px, py)
    }

    pub fn on_min(&self, px: i32, py: i32) -> bool {
        self.min_rect().contains(px, py)
    }

    /// True when the point is on the draggable title area (excludes the three
    /// title-bar buttons).
    pub fn on_title(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && px < self.x + self.w
            && py >= self.y
            && py < self.y + TITLE_H
            && !self.on_close(px, py)
            && !self.on_max(px, py)
            && !self.on_min(px, py)
    }

    /// The border/corner under `(px, py)` for a resize drag: a
    /// [`RESIZE_BAND`]-thick band inside the rect, widened to
    /// [`RESIZE_CORNER`] along the border next to a corner.
    pub fn resize_edge_at(&self, px: i32, py: i32) -> Option<ResizeEdge> {
        if !self.contains(px, py) {
            return None;
        }
        let (l, r) = (px - self.x, self.right() - 1 - px);
        let (t, b) = (py - self.y, self.bottom() - 1 - py);
        let mut ex = if l < RESIZE_BAND {
            -1
        } else if r < RESIZE_BAND {
            1
        } else {
            0
        };
        let mut ey = if t < RESIZE_BAND {
            -1
        } else if b < RESIZE_BAND {
            1
        } else {
            0
        };
        // Along a border, the stretch next to a corner counts as the corner.
        if ex == 0 && ey != 0 {
            ex = if l < RESIZE_CORNER {
                -1
            } else if r < RESIZE_CORNER {
                1
            } else {
                0
            };
        } else if ey == 0 && ex != 0 {
            ey = if t < RESIZE_CORNER {
                -1
            } else if b < RESIZE_CORNER {
                1
            } else {
                0
            };
        }
        ResizeEdge::from_axes(ex, ey)
    }

    /// The rect after dragging `edge` of `self`: the grabbed
    /// side(s) follow the pointer by `(dx, dy)`, the opposite side stays put, the
    /// size never drops below `min` nor leaves the `screen` (`sw x sh`).
    pub fn resized(
        &self,
        edge: ResizeEdge,
        (dx, dy): (i32, i32),
        (min_w, min_h): (i32, i32),
        (sw, sh): (i32, i32),
    ) -> Rect {
        let (ax, ay) = edge.axes();
        let (mut left, mut right) = (self.x, self.right());
        let (mut top, mut bottom) = (self.y, self.bottom());
        if ax < 0 {
            left = (left + dx).clamp(0, right - min_w);
        } else if ax > 0 {
            right = (right + dx).clamp(left + min_w, sw.max(left + min_w));
        }
        if ay < 0 {
            top = (top + dy).clamp(0, bottom - min_h);
        } else if ay > 0 {
            bottom = (bottom + dy).clamp(top + min_h, sh.max(top + min_h));
        }
        Rect::new(left, top, right - left, bottom - top)
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

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    /// Smallest rectangle covering both `self` and `other` (damage-region
    /// merge: keep a single extents rect instead of many fragments).
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());
        Rect::new(x, y, right - x, bottom - y)
    }

    /// Overlap of two rectangles, or `None` if they don't intersect.
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right > x && bottom > y {
            Some(Rect::new(x, y, right - x, bottom - y))
        } else {
            None
        }
    }

    /// Clip the rectangle to the `0..sw × 0..sh` screen bounds.
    pub fn clamped_to(&self, sw: i32, sh: i32) -> Rect {
        let x = self.x.max(0);
        let y = self.y.max(0);
        let right = self.right().min(sw);
        let bottom = self.bottom().min(sh);
        Rect::new(x, y, (right - x).max(0), (bottom - y).max(0))
    }

    /// Grow the rectangle by `m` pixels on every side.
    pub fn inflated(&self, m: i32) -> Rect {
        Rect::new(self.x - m, self.y - m, self.w + 2 * m, self.h + 2 * m)
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
    fn union_covers_both() {
        let a = Rect::new(10, 10, 20, 20); // [10,30)x[10,30)
        let b = Rect::new(40, 5, 10, 40); // [40,50)x[5,45)
        let u = a.union(&b);
        assert_eq!(u, Rect::new(10, 5, 40, 40)); // [10,50)x[5,45)
    }

    #[test]
    fn union_with_empty_is_identity() {
        let a = Rect::new(10, 10, 20, 20);
        let empty = Rect::new(0, 0, 0, 0);
        assert_eq!(a.union(&empty), a);
        assert_eq!(empty.union(&a), a);
    }

    #[test]
    fn intersection_overlap() {
        let a = Rect::new(0, 0, 30, 30);
        let b = Rect::new(20, 20, 30, 30);
        assert_eq!(a.intersection(&b), Some(Rect::new(20, 20, 10, 10)));
    }

    #[test]
    fn intersection_disjoint_is_none() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(20, 20, 10, 10);
        assert_eq!(a.intersection(&b), None);
        // touching edges (not overlapping) -> None
        assert_eq!(a.intersection(&Rect::new(10, 0, 5, 10)), None);
    }

    #[test]
    fn clamp_to_screen() {
        let r = Rect::new(-5, -5, 20, 20); // [-5,15)
        assert_eq!(r.clamped_to(1280, 800), Rect::new(0, 0, 15, 15));
        let off = Rect::new(1270, 0, 100, 50);
        assert_eq!(off.clamped_to(1280, 800), Rect::new(1270, 0, 10, 50));
        // fully off-screen -> zero area
        assert!(Rect::new(2000, 0, 10, 10).clamped_to(1280, 800).is_empty());
    }

    #[test]
    fn inflate_grows_all_sides() {
        let r = Rect::new(10, 10, 20, 20);
        assert_eq!(r.inflated(5), Rect::new(5, 5, 30, 30));
    }

    #[test]
    fn right_bottom_empty() {
        let r = Rect::new(10, 20, 30, 40);
        assert_eq!(r.right(), 40);
        assert_eq!(r.bottom(), 60);
        assert!(!r.is_empty());
        assert!(Rect::new(0, 0, 0, 5).is_empty());
    }

    #[test]
    fn buttons_sit_left_of_close_without_overlap() {
        let r = win();
        let (c, m, n) = (r.close_rect(), r.max_rect(), r.min_rect());
        assert_eq!(m.right() + BTN_GAP, c.x);
        assert_eq!(n.right() + BTN_GAP, m.x);
        assert!(r.on_max(m.x + 1, m.y + 1));
        assert!(r.on_min(n.x + 1, n.y + 1));
        assert!(!r.on_max(c.x + 1, c.y + 1));
        assert!(!r.on_min(m.x + 1, m.y + 1));
    }

    #[test]
    fn title_excludes_all_three_buttons() {
        let r = win();
        for b in [r.close_rect(), r.max_rect(), r.min_rect()] {
            assert!(!r.on_title(b.x + 2, b.y + 2));
        }
        // The stretch between the title text and the buttons is still title.
        assert!(r.on_title(r.min_rect().x - 3, r.y + 10));
    }

    #[test]
    fn resize_edges_cover_borders_and_corners() {
        let r = win(); // [100,500) x [100,400)
        assert_eq!(r.resize_edge_at(300, 250), None); // interior
        assert_eq!(r.resize_edge_at(100, 250), Some(ResizeEdge::W));
        assert_eq!(r.resize_edge_at(499, 250), Some(ResizeEdge::E));
        assert_eq!(r.resize_edge_at(300, 100), Some(ResizeEdge::N));
        assert_eq!(r.resize_edge_at(300, 399), Some(ResizeEdge::S));
        assert_eq!(r.resize_edge_at(499, 399), Some(ResizeEdge::SE));
        assert_eq!(r.resize_edge_at(100, 399), Some(ResizeEdge::SW));
        assert_eq!(r.resize_edge_at(100, 100), Some(ResizeEdge::NW));
        assert_eq!(r.resize_edge_at(499, 100), Some(ResizeEdge::NE));
        // Bottom border next to the corner counts as the corner.
        assert_eq!(r.resize_edge_at(490, 399), Some(ResizeEdge::SE));
        assert_eq!(r.resize_edge_at(499, 390), Some(ResizeEdge::SE));
        // Outside the band, and outside the window: no edge.
        assert_eq!(r.resize_edge_at(100 + RESIZE_BAND, 250), None);
        assert_eq!(r.resize_edge_at(500, 250), None);
        assert_eq!(r.resize_edge_at(50, 50), None);
    }

    const SCREEN: (i32, i32) = (1280, 720);

    #[test]
    fn resized_follows_the_grabbed_side() {
        let r = win();
        let se = r.resized(ResizeEdge::SE, (30, 20), (100, 100), SCREEN);
        assert_eq!(se, Rect::new(100, 100, 430, 320));
        let nw = r.resized(ResizeEdge::NW, (20, 10), (100, 100), SCREEN);
        assert_eq!(nw, Rect::new(120, 110, 380, 290));
        let e = r.resized(ResizeEdge::E, (-50, 99), (100, 100), SCREEN);
        assert_eq!(e, Rect::new(100, 100, 350, 300)); // dy ignored on E
    }

    #[test]
    fn resized_enforces_the_minimum_size_anchoring_the_far_side() {
        let r = win();
        let shrunk = r.resized(ResizeEdge::SE, (-1000, -1000), (200, 150), SCREEN);
        assert_eq!(shrunk, Rect::new(100, 100, 200, 150));
        let nw = r.resized(ResizeEdge::NW, (1000, 1000), (200, 150), SCREEN);
        assert_eq!(nw, Rect::new(300, 250, 200, 150)); // right/bottom fixed
    }

    #[test]
    fn resized_stays_on_screen() {
        let r = win();
        let big = r.resized(ResizeEdge::SE, (5000, 5000), (100, 100), SCREEN);
        assert_eq!(big.right(), 1280);
        assert_eq!(big.bottom(), 720);
        let nw = r.resized(ResizeEdge::NW, (-5000, -5000), (100, 100), SCREEN);
        assert_eq!((nw.x, nw.y), (0, 0));
        assert_eq!((nw.right(), nw.bottom()), (500, 400));
    }

    #[test]
    fn window_id_roundtrips_and_orders() {
        let a = WindowId::from_raw(3);
        assert_eq!(a.raw(), 3);
        assert!(a < WindowId::from_raw(4));
    }

    #[test]
    fn clamp_handles_oversized_window() {
        // Width exceeds screen -> x pinned to 0. Height never constrains y
        // (only the title bar must stay visible), so y is unchanged.
        let r = Rect::new(10, 10, 2000, 2000);
        assert_eq!(r.clamped_pos(1280, 800), (0, 10));
    }
}
