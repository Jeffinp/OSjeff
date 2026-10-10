//! Window geometry & hit-testing. Pure integer math, no rendering.

/// Title bar height in pixels (unified title bar: same colour as the body).
pub const TITLE_H: i32 = 32;
/// Height of the top panel: no window may cover it.
pub use crate::ui::style::{MENUBAR_H, PANEL_H};
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
/// [`crate::windowing::winman::WindowManager`], increase monotonically and are never
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
    /// below the panel.
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
mod tests;
