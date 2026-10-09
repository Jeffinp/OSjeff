//! Window geometry & hit-testing. Pure integer math, no rendering.

/// Title bar height in pixels (unified title bar: same colour as the body).
pub const TITLE_H: i32 = 32;
/// Height of the top panel: no window may cover it.
pub use crate::style::{MENUBAR_H, PANEL_H};
/// Width of the minimise, maximise and close buttons at the right of the title bar. The hit area
/// is the whole 40 x 32 cell (the close button reaches the very corner of the window).
pub const BTN_W: i32 = 40;
/// Width of the menu button that sits left of the three buttons.
pub const MENU_W: i32 = 32;
/// Side of the small app icon at the left of the title.
pub const TITLE_ICON: i32 = 16;
/// Padding before the app icon and between the icon and the title text.
pub const TITLE_PAD: i32 = 10;
/// Gap between the title text and the first button, kept free of text.
pub const TITLE_GAP: i32 = 8;
/// Thickness of the invisible resize band along a window's border.
pub const RESIZE_BAND: i32 = 5;
/// Length of the corner zone along each border (a bit larger than the band so
/// corners are easy to hit).
pub const RESIZE_CORNER: i32 = 14;

/// A button of the title bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TitleBtn {
    Menu,
    Minimize,
    Maximize,
    Close,
}

/// Where the parts of a title bar sit (see [`Rect::title_layout`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TitleLayout {
    /// The small app icon at the left.
    pub icon: Rect,
    /// Left edge of the title text and the right limit it must be cut at.
    pub title_x: i32,
    pub title_right: i32,
    pub menu: Option<Rect>,
    pub min: Rect,
    pub max: Option<Rect>,
    pub close: Rect,
}

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

    /// The close button: the right-most cell of the title bar.
    pub fn close_rect(&self) -> Rect {
        Rect::new(self.right() - BTN_W, self.y, BTN_W, TITLE_H)
    }

    /// The maximise / restore button, left of close.
    pub fn max_rect(&self) -> Rect {
        let c = self.close_rect();
        Rect::new(c.x - BTN_W, c.y, BTN_W, TITLE_H)
    }

    /// The minimise button, left of maximise.
    pub fn min_rect(&self) -> Rect {
        let m = self.max_rect();
        Rect::new(m.x - BTN_W, m.y, BTN_W, TITLE_H)
    }

    /// The title-bar layout of a window with this rectangle. A window that cannot be resized has
    /// no maximise button (minimise slides over to take its place); `menu` asks for the menu
    /// button left of the buttons.
    pub fn title_layout(&self, resizable: bool, menu: bool) -> TitleLayout {
        let close = self.close_rect();
        let (max, min) = if resizable {
            let m = self.max_rect();
            (Some(m), Rect::new(m.x - BTN_W, m.y, BTN_W, TITLE_H))
        } else {
            (None, Rect::new(close.x - BTN_W, close.y, BTN_W, TITLE_H))
        };
        let menu = menu.then(|| Rect::new(min.x - MENU_W, min.y, MENU_W, TITLE_H));
        let icon = Rect::new(
            self.x + TITLE_PAD,
            self.y + (TITLE_H - TITLE_ICON) / 2,
            TITLE_ICON,
            TITLE_ICON,
        );
        let title_x = icon.right() + TITLE_PAD - 2;
        let controls_left = menu.map_or(min.x, |m| m.x);
        TitleLayout {
            icon,
            title_x,
            title_right: (controls_left - TITLE_GAP).max(title_x),
            menu,
            min,
            max,
            close,
        }
    }

    /// The title-bar button under `(px, py)`.
    pub fn title_button_at(
        &self,
        resizable: bool,
        menu: bool,
        px: i32,
        py: i32,
    ) -> Option<TitleBtn> {
        if py < self.y || py >= self.y + TITLE_H {
            return None;
        }
        let l = self.title_layout(resizable, menu);
        if l.close.contains(px, py) {
            Some(TitleBtn::Close)
        } else if l.max.is_some_and(|m| m.contains(px, py)) {
            Some(TitleBtn::Maximize)
        } else if l.min.contains(px, py) {
            Some(TitleBtn::Minimize)
        } else if l.menu.is_some_and(|m| m.contains(px, py)) {
            Some(TitleBtn::Menu)
        } else {
            None
        }
    }

    /// True when the point is on the draggable title area: the title bar minus its buttons (every
    /// button counts, so a right click on the bar never lands on the app under it).
    pub fn on_title(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && px < self.x + self.w
            && py >= self.y
            && py < self.y + TITLE_H
            && self.title_button_at(true, true, px, py).is_none()
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
            top = (top + dy).clamp(MENUBAR_H, (bottom - min_h).max(MENUBAR_H));
        } else if ay > 0 {
            bottom = (bottom + dy).clamp(top + min_h, sh.max(top + min_h));
        }
        Rect::new(left, top, right - left, bottom - top)
    }

    /// The content area below the title bar.
    pub fn body(&self) -> Rect {
        Rect::new(self.x, self.y + TITLE_H, self.w, (self.h - TITLE_H).max(0))
    }

    /// Clamp a proposed top-left so the title bar stays on a `sw × sh` screen and
    /// below the menu bar.
    pub fn clamped_pos(&self, sw: i32, sh: i32) -> (i32, i32) {
        let x = self.x.clamp(0, (sw - self.w).max(0));
        let y = self.y.clamp(MENUBAR_H, (sh - TITLE_H).max(MENUBAR_H));
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
    fn buttons_sit_at_the_right_edge_close_outermost() {
        let r = win();
        let (c, m, n) = (r.close_rect(), r.max_rect(), r.min_rect());
        assert_eq!(c, Rect::new(r.right() - BTN_W, r.y, BTN_W, TITLE_H));
        assert_eq!(c.right(), r.right());
        assert_eq!((m.right(), n.right()), (c.x, m.x));
        // The corner pixel of the window is the close button (Fitts: infinite target).
        assert_eq!(
            r.title_button_at(true, true, r.right() - 1, r.y),
            Some(TitleBtn::Close)
        );
        assert_eq!(
            r.title_button_at(true, true, m.x + 3, m.y + 3),
            Some(TitleBtn::Maximize)
        );
        assert_eq!(
            r.title_button_at(true, true, n.x + 3, n.y + 30),
            Some(TitleBtn::Minimize)
        );
        // Below the bar nothing is a button.
        assert_eq!(r.title_button_at(true, true, c.x, r.y + TITLE_H), None);
    }

    #[test]
    fn the_menu_button_sits_left_of_the_buttons_and_is_optional() {
        let r = win();
        let l = r.title_layout(true, true);
        let menu = l.menu.unwrap();
        assert_eq!(menu.right(), l.min.x);
        assert_eq!(menu.w, MENU_W);
        assert_eq!(
            r.title_button_at(true, true, menu.x + 2, menu.y + 2),
            Some(TitleBtn::Menu)
        );
        let none = r.title_layout(true, false);
        assert!(none.menu.is_none());
        assert_eq!(r.title_button_at(true, false, menu.x + 2, menu.y + 2), None);
        // The title text stops before the first button.
        assert_eq!(l.title_right, menu.x - TITLE_GAP);
        assert_eq!(none.title_right, none.min.x - TITLE_GAP);
    }

    #[test]
    fn a_window_that_cannot_be_resized_has_no_maximise_button() {
        let r = win();
        let l = r.title_layout(false, true);
        assert!(l.max.is_none());
        assert_eq!(l.min.right(), l.close.x);
        let x = r.max_rect().x + 3;
        assert_eq!(
            r.title_button_at(false, true, x, r.y + 3),
            Some(TitleBtn::Minimize)
        );
        assert_eq!(
            r.title_button_at(true, true, x, r.y + 3),
            Some(TitleBtn::Maximize)
        );
    }

    #[test]
    fn the_icon_and_title_start_at_the_left_and_never_reach_the_buttons() {
        let r = win();
        let l = r.title_layout(true, true);
        assert_eq!(l.icon.x, r.x + TITLE_PAD);
        assert_eq!(l.icon.w, TITLE_ICON);
        // The icon is vertically centred in the bar.
        assert_eq!(l.icon.y - r.y, TITLE_H - (l.icon.bottom() - r.y));
        assert!(l.title_x > l.icon.right() && l.title_x < l.title_right);
        // A tiny window keeps a non-negative text width.
        let tiny = Rect::new(0, 0, 60, 100).title_layout(true, true);
        assert!(tiny.title_right >= tiny.title_x);
    }

    #[test]
    fn title_excludes_the_buttons() {
        let r = win();
        assert!(r.on_title(r.x + 100, r.y + 10));
        for b in [r.close_rect(), r.max_rect(), r.min_rect()] {
            assert!(!r.on_title(b.x + 1, b.y + 1));
        }
        let menu = r.title_layout(true, true).menu.unwrap();
        assert!(!r.on_title(menu.x + 1, menu.y + 1));
        assert!(r.on_title(menu.x - 4, menu.y + 1));
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
        assert_eq!(b, Rect::new(100, 100 + TITLE_H, 400, 300 - TITLE_H));
    }

    #[test]
    fn clamp_keeps_window_on_screen() {
        let r = Rect::new(-50, -20, 400, 300);
        assert_eq!(r.clamped_pos(1280, 800), (0, MENUBAR_H));
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
        // The top edge stops under the menu bar.
        assert_eq!((nw.x, nw.y), (0, MENUBAR_H));
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
        let r = Rect::new(10, 40, 2000, 2000);
        assert_eq!(r.clamped_pos(1280, 800), (0, 40));
    }
}
