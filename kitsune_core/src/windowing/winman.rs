//! Dynamic window table: any number of windows (up to a configurable limit),
//! each owning an application payload `A`, with z-order, focus, hit-testing,
//! open / close / minimize / maximize / restore, cascading placement,
//! move / resize and most-recently-used focus cycling (Alt+Tab). What is on screen is
//! planned by [`crate::windowing::compositor`].
//!
//! The table is pure logic: it never draws and knows nothing about what `A`
//! is. The kernel instantiates it with its app-instance type; the tests below
//! use plain integers. Windows are stored back-to-front, so the z-order *is*
//! the vector order and the last window is the topmost.
//!
//! The older free functions in [`crate::windowing::wm`] (which work over a fixed `order`
//! slice) keep their meaning; this module is the dynamic generalisation.

use alloc::vec::Vec;

use crate::ui::anim::{Anim, Zoom};
use crate::windowing::snap::{self, SnapZone};
use crate::windowing::window::{Rect, ResizeEdge, WindowId};

/// Default cap on simultaneously open windows.
pub const DEFAULT_MAX_WINDOWS: usize = 32;
/// Offset between consecutive cascaded windows (both axes).
pub const CASCADE_STEP: i32 = 28;
/// Cascade positions before the sequence wraps back to the base position.
pub const CASCADE_WRAP: usize = 8;

/// Placement of a new window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WindowSpec {
    pub rect: Rect,
    pub min_w: i32,
    pub min_h: i32,
    /// Whether the window can be resized / maximized.
    pub resizable: bool,
}

/// High-level state of a window, derived from its flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinState {
    Normal,
    Minimized,
    Maximized,
}

/// What a *closing* animation does when it finishes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Leaving {
    /// Remove the window (and drop its app).
    Destroy,
    /// Hide it but keep it (and its app) alive.
    Minimize,
    /// It slides away to another workspace.
    Workspace,
}

/// Most workspaces there can be.
pub const MAX_WORKSPACES: u8 = 4;
/// Workspaces always shown (the switcher never offers fewer).
pub const MIN_WORKSPACES: u8 = 2;

/// One window record.
pub struct Window<A> {
    pub id: WindowId,
    /// Current on-screen rectangle (the work area while maximized).
    pub rect: Rect,
    /// The rectangle to return to when un-maximizing.
    pub restore: Rect,
    pub minimized: bool,
    pub maximized: bool,
    /// Tiled to a half or a quarter of the work area (never `Maximize`: that is `maximized`).
    pub snap: Option<SnapZone>,
    pub anim: Option<Anim>,
    /// A maximise / restore in flight: the rectangle to draw travels from the old
    /// one to `rect` (see [`Zoom`]).
    pub zoom: Option<Zoom>,
    pub leaving: Leaving,
    pub min_w: i32,
    pub min_h: i32,
    pub resizable: bool,
    /// The workspace the window lives on, and whether it is hidden because that is not the
    /// current one.
    pub ws: u8,
    pub off_ws: bool,
    pub app: A,
}

impl<A> Window<A> {
    pub fn state(&self) -> WinState {
        if self.minimized {
            WinState::Minimized
        } else if self.maximized {
            WinState::Maximized
        } else {
            WinState::Normal
        }
    }

    /// Where the window sits in the snap model: `None` = free, `Some(Maximize)` = maximised,
    /// otherwise the half or quarter it is tiled to.
    pub fn snap_state(&self) -> Option<SnapZone> {
        if self.maximized {
            Some(SnapZone::Maximize)
        } else {
            self.snap
        }
    }

    /// Animating out (closing or minimizing): no longer takes focus or clicks.
    pub fn is_leaving(&self) -> bool {
        matches!(self.anim, Some(a) if a.is_closing())
    }

    /// Being destroyed (as opposed to merely minimized).
    pub fn is_closing(&self) -> bool {
        self.is_leaving() && self.leaving == Leaving::Destroy
    }

    /// On screen: not minimized and on the current workspace (an animating-out window still is).
    pub fn shown(&self) -> bool {
        !self.minimized && !self.off_ws
    }

    /// Accepts focus and clicks: shown and not animating out.
    pub fn active(&self) -> bool {
        self.shown() && !self.is_leaving()
    }

    /// The rectangle the window occupies on screen right now: `rect`, or the
    /// in-flight rectangle of a maximise / restore.
    pub fn visual_rect(&self) -> Rect {
        self.zoom.as_ref().map_or(self.rect, Zoom::rect)
    }

    /// Begin (or redirect) the maximise / restore travel from `from` to `rect`.
    fn start_zoom(&mut self, from: Rect) {
        if !self.shown() || from == self.rect {
            self.zoom = None;
            return;
        }
        match self.zoom.as_mut() {
            Some(z) => z.retarget(self.rect),
            None => self.zoom = Some(Zoom::new(from, self.rect)),
        }
    }
}

/// The dynamic window table. See the module docs.
pub struct WindowManager<A> {
    wins: Vec<Window<A>>,
    /// Most recently used first.
    mru: Vec<WindowId>,
    next_id: u32,
    limit: usize,
    /// The workspace on screen.
    cur_ws: u8,
}

impl<A> Default for WindowManager<A> {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_WINDOWS)
    }
}

impl<A> WindowManager<A> {
    /// An empty table holding at most `limit` windows.
    pub fn new(limit: usize) -> Self {
        Self {
            wins: Vec::new(),
            mru: Vec::new(),
            next_id: 1,
            cur_ws: 0,
            limit,
        }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn len(&self) -> usize {
        self.wins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.wins.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.wins.len() >= self.limit
    }

    /// All windows, back to front.
    pub fn windows(&self) -> &[Window<A>] {
        &self.wins
    }

    /// Z-order position of `id` (0 = backmost).
    pub fn z_index(&self, id: WindowId) -> Option<usize> {
        self.wins.iter().position(|w| w.id == id)
    }

    pub fn get(&self, id: WindowId) -> Option<&Window<A>> {
        self.wins.iter().find(|w| w.id == id)
    }

    pub fn get_mut(&mut self, id: WindowId) -> Option<&mut Window<A>> {
        self.wins.iter_mut().find(|w| w.id == id)
    }

    // ---------------------------------------------------------- open / close

    /// Opens a window on top (opening animation, focused). Gives the app back
    /// when the table is full.
    pub fn open(&mut self, spec: WindowSpec, app: A) -> Result<WindowId, A> {
        if self.is_full() {
            return Err(app);
        }
        let id = WindowId::from_raw(self.next_id);
        self.next_id += 1;
        self.wins.push(Window {
            id,
            rect: spec.rect,
            restore: spec.rect,
            minimized: false,
            maximized: false,
            snap: None,
            anim: Some(Anim::open()),
            zoom: None,
            leaving: Leaving::Destroy,
            min_w: spec.min_w,
            min_h: spec.min_h,
            resizable: spec.resizable,
            ws: self.cur_ws,
            off_ws: false,
            app,
        });
        self.mru.insert(0, id);
        Ok(id)
    }

    /// Starts the closing animation. `false` if the window is unknown or is
    /// already being destroyed. Closing a minimizing window turns the
    /// minimize into a close.
    pub fn request_close(&mut self, id: WindowId) -> bool {
        let Some(w) = self.get_mut(id) else {
            return false;
        };
        if w.is_closing() {
            return false;
        }
        // A minimized window stays hidden: its closing animation runs unseen
        // and the next `step` destroys it.
        w.leaving = Leaving::Destroy;
        w.anim = Some(Anim::close());
        true
    }

    /// Removes a window immediately (no animation) and returns it.
    pub fn remove(&mut self, id: WindowId) -> Option<Window<A>> {
        let i = self.z_index(id)?;
        self.mru.retain(|&m| m != id);
        Some(self.wins.remove(i))
    }

    /// Advances every animation by `dt`. Returns whether any is still running
    /// and the windows whose closing animation finished (already removed from
    /// the table: the caller releases their resources). A finished minimize
    /// hides the window instead.
    pub fn step(&mut self, dt: f32) -> (bool, Vec<Window<A>>) {
        let mut active = false;
        let mut gone: Vec<WindowId> = Vec::new();
        for w in self.wins.iter_mut() {
            if let Some(z) = w.zoom.as_mut() {
                if z.step(dt) {
                    active = true;
                } else {
                    w.zoom = None;
                }
            }
            let Some(a) = w.anim.as_mut() else { continue };
            a.step(dt);
            if !a.finished() {
                active = true;
                continue;
            }
            let closing = a.is_closing();
            w.anim = None;
            if closing {
                match w.leaving {
                    Leaving::Destroy => gone.push(w.id),
                    Leaving::Minimize => {
                        w.minimized = true;
                        w.leaving = Leaving::Destroy;
                    }
                    Leaving::Workspace => {
                        w.off_ws = true;
                        w.leaving = Leaving::Destroy;
                    }
                }
            }
        }
        let mut removed = Vec::new();
        for id in gone {
            if let Some(w) = self.remove(id) {
                removed.push(w);
            }
        }
        (active, removed)
    }

    // --------------------------------------------------------------- focus

    /// The focused window: the topmost one that takes focus.
    pub fn focused(&self) -> Option<WindowId> {
        self.wins.iter().rev().find(|w| w.active()).map(|w| w.id)
    }

    /// Moves `id` to the front and marks it most recently used.
    pub fn raise(&mut self, id: WindowId) -> bool {
        let Some(i) = self.z_index(id) else {
            return false;
        };
        self.wins[i..].rotate_left(1);
        self.touch(id);
        true
    }

    fn touch(&mut self, id: WindowId) {
        self.mru.retain(|&m| m != id);
        self.mru.insert(0, id);
    }

    /// Brings a window to the user: un-minimizes it (with the opening
    /// animation), cancels a pending close or minimize, then raises it.
    pub fn activate(&mut self, id: WindowId) -> bool {
        // A window of another workspace brings its workspace with it.
        if let Some(ws) = self.get(id).map(|w| w.ws).filter(|&ws| ws != self.cur_ws) {
            self.switch_workspace(ws);
        }
        let Some(w) = self.get_mut(id) else {
            return false;
        };
        if w.minimized {
            w.minimized = false;
            w.anim = Some(Anim::restore());
        } else if w.is_leaving() {
            w.anim = Some(Anim::open());
        }
        w.leaving = Leaving::Destroy;
        self.raise(id)
    }

    /// The frontmost active window whose rect contains `(px, py)`.
    pub fn topmost_at(&self, px: i32, py: i32) -> Option<WindowId> {
        self.wins
            .iter()
            .rev()
            .find(|w| w.active() && w.rect.contains(px, py))
            .map(|w| w.id)
    }

    /// Windows in focus-cycle order (Alt+Tab): the focused window first, then
    /// the rest by most recent use. Windows being destroyed are skipped;
    /// minimized ones are included.
    pub fn switch_list(&self) -> Vec<WindowId> {
        let mut out = Vec::with_capacity(self.wins.len());
        let ok = |id: WindowId| self.get(id).is_some_and(|w| !w.is_closing());
        if let Some(f) = self.focused() {
            out.push(f);
        }
        for &id in &self.mru {
            if ok(id) && !out.contains(&id) {
                out.push(id);
            }
        }
        out
    }

    // ----------------------------------------------------------- workspaces

    /// The workspace on screen (0-based).
    pub fn workspace(&self) -> u8 {
        self.cur_ws
    }

    /// How many workspaces the switcher shows: at least [`MIN_WORKSPACES`], one more than the
    /// last one that holds a window (an empty one to move to), the current one included, at
    /// most [`MAX_WORKSPACES`].
    pub fn visible_workspaces(&self) -> u8 {
        let last_used = self
            .wins
            .iter()
            .filter(|w| !w.is_closing())
            .map(|w| w.ws)
            .max()
            .map_or(0, |m| m + 1);
        (last_used + 1)
            .max(self.cur_ws + 1)
            .clamp(MIN_WORKSPACES, MAX_WORKSPACES)
    }

    /// How many live windows are on workspace `ws`.
    pub fn windows_on(&self, ws: u8) -> usize {
        self.wins
            .iter()
            .filter(|w| w.ws == ws && !w.is_closing())
            .count()
    }

    /// Shows workspace `to`: the windows of the one on screen slide away (toward the side `to`
    /// is on, in the opposite direction) and those of `to` slide in. `false` when `to` is out of
    /// range or already current.
    pub fn switch_workspace(&mut self, to: u8) -> bool {
        if to >= MAX_WORKSPACES || to == self.cur_ws {
            return false;
        }
        let dir: i8 = if to > self.cur_ws { 1 } else { -1 };
        for w in self.wins.iter_mut() {
            if w.is_closing() {
                continue;
            }
            if w.ws == to {
                let was_hidden = w.off_ws || w.leaving == Leaving::Workspace;
                w.off_ws = false;
                if w.leaving == Leaving::Workspace {
                    w.leaving = Leaving::Destroy;
                }
                if !w.minimized && was_hidden {
                    w.anim = Some(Anim::slide_in(dir));
                }
            } else if w.ws == self.cur_ws && !w.minimized && !w.off_ws {
                w.leaving = Leaving::Workspace;
                w.anim = Some(Anim::slide_out(dir));
            } else if w.ws != to {
                w.off_ws = true;
            }
        }
        self.cur_ws = to;
        true
    }

    /// Moves window `id` to workspace `to`. When that is not the one on screen the window slides
    /// away (toward the side `to` is on). `false` for an unknown window or an out-of-range `to`.
    pub fn move_to_workspace(&mut self, id: WindowId, to: u8) -> bool {
        if to >= MAX_WORKSPACES {
            return false;
        }
        let cur = self.cur_ws;
        let Some(w) = self.get_mut(id) else {
            return false;
        };
        if w.ws == to {
            return true;
        }
        w.ws = to;
        if to != cur && !w.minimized && !w.off_ws && !w.is_closing() {
            let dir: i8 = if to > cur { 1 } else { -1 };
            w.leaving = Leaving::Workspace;
            w.anim = Some(Anim::slide_out(-dir));
        } else if to != cur {
            w.off_ws = true;
        }
        true
    }

    // ------------------------------------------------- minimize / maximize

    /// Starts minimizing (fade out, then hidden). `false` if the window is not
    /// active.
    pub fn minimize(&mut self, id: WindowId) -> bool {
        let Some(w) = self.get_mut(id) else {
            return false;
        };
        if !w.active() {
            return false;
        }
        w.leaving = Leaving::Minimize;
        w.anim = Some(Anim::minimize());
        true
    }

    /// Maximizes to `work`, remembering the current rect. No-op (`false`) for
    /// a non-resizable or already maximized window.
    pub fn maximize(&mut self, id: WindowId, work: Rect) -> bool {
        self.snap_to(id, SnapZone::Maximize, work)
    }

    /// Puts the window in `zone` of `work` (a half, a quarter or the whole area), remembering the
    /// free rectangle it came from (an already tiled or maximised window keeps the one it had).
    /// The move animates like a maximise. `false` for a non-resizable window or when it is
    /// already there.
    pub fn snap_to(&mut self, id: WindowId, zone: SnapZone, work: Rect) -> bool {
        let Some(w) = self.get_mut(id) else {
            return false;
        };
        if !w.resizable || w.snap_state() == Some(zone) {
            return false;
        }
        let target = snap::zone_rect_min(zone, work, w.min_w, w.min_h);
        if w.maximized || w.snap.is_some() {
            // Keep the original free rectangle.
        } else {
            w.restore = w.rect;
        }
        let from = w.visual_rect();
        w.rect = target;
        w.maximized = zone == SnapZone::Maximize;
        w.snap = (zone != SnapZone::Maximize).then_some(zone);
        w.start_zoom(from);
        true
    }

    /// Leaves the maximized or tiled state, returning to the remembered rect.
    pub fn unmaximize(&mut self, id: WindowId) -> bool {
        let Some(w) = self.get_mut(id) else {
            return false;
        };
        if !w.maximized && w.snap.is_none() {
            return false;
        }
        let from = w.visual_rect();
        w.rect = w.restore;
        w.maximized = false;
        w.snap = None;
        w.start_zoom(from);
        true
    }

    /// Maximize if free, restore if maximized or tiled.
    pub fn toggle_maximize(&mut self, id: WindowId, work: Rect) -> bool {
        match self.get(id).map(|w| w.snap_state().is_some()) {
            Some(true) => self.unmaximize(id),
            Some(false) => self.maximize(id, work),
            None => false,
        }
    }

    /// A drag grabbed the title of a maximised or tiled window at `(px, py)`: restore it
    /// *without animation* to the size it had, put under the pointer so the grab keeps its
    /// proportional place on the bar, and return the new grab offset `(dx, dy)` for the move that
    /// follows. `None` when the window is free (nothing to restore).
    pub fn restore_for_drag(&mut self, id: WindowId, px: i32, py: i32) -> Option<(i32, i32)> {
        let w = self.get_mut(id)?;
        if !w.maximized && w.snap.is_none() {
            return None;
        }
        let cur = w.rect;
        let size = w.restore;
        let frac = ((px - cur.x).clamp(0, cur.w.max(1)) * 256) / cur.w.max(1);
        let dx = (size.w * frac / 256).clamp(0, size.w);
        let dy = (py - cur.y).clamp(0, crate::windowing::window::TITLE_H - 1);
        w.rect = Rect::new(px - dx, py - dy, size.w, size.h);
        w.maximized = false;
        w.snap = None;
        w.zoom = None;
        Some((dx, dy))
    }

    // ------------------------------------------------------ move / resize

    /// Moves a window's top-left to `(x, y)`, keeping its title bar on the
    /// `sw x sh` screen. Maximized windows do not move.
    pub fn move_to(&mut self, id: WindowId, x: i32, y: i32, sw: i32, sh: i32) -> bool {
        let Some(w) = self.get_mut(id) else {
            return false;
        };
        if w.maximized {
            return false;
        }
        let (cx, cy) = Rect::new(x, y, w.rect.w, w.rect.h).clamped_pos(sw, sh);
        w.snap = None;
        w.rect.x = cx;
        w.rect.y = cy;
        true
    }

    /// Resizes a window by dragging `edge` of `start` by `(dx, dy)` (use the
    /// rect captured when the drag began, so the result does not drift).
    pub fn resize(
        &mut self,
        id: WindowId,
        edge: ResizeEdge,
        start: Rect,
        delta: (i32, i32),
        screen: (i32, i32),
    ) -> bool {
        let Some(w) = self.get_mut(id) else {
            return false;
        };
        if !w.resizable || w.maximized {
            return false;
        }
        w.rect = start.resized(edge, delta, (w.min_w, w.min_h), screen);
        w.snap = None;
        true
    }
}

// ------------------------------------------------------------ free helpers

/// Placement of the `k`-th (0-based) window of a kind: `base` shifted down and
/// right by [`CASCADE_STEP`] per index, wrapping after [`CASCADE_WRAP`] (each
/// wrap also nudges the row 12 px right so wrapped windows do not sit exactly on
/// top of the first ones), then moved so it lies inside `work`.
pub fn cascade_rect(base: Rect, k: usize, work: Rect) -> Rect {
    let off = (k % CASCADE_WRAP) as i32 * CASCADE_STEP;
    let nudge = (k / CASCADE_WRAP) as i32 * 12;
    let w = base.w.min(work.w);
    let h = base.h.min(work.h);
    let x = (base.x + off + nudge).clamp(work.x, (work.right() - w).max(work.x));
    let y = (base.y + off).clamp(work.y, (work.bottom() - h).max(work.y));
    Rect::new(x, y, w, h)
}

/// Writes `base` — followed by a space and the number for instances after the
/// first (`shell`, `shell 2`, `shell 3`...) — into `out`, truncating to its
/// length. Returns the byte count. With an empty `base` it yields just `" N"`,
/// the suffix of a numbered window title.
pub fn numbered_name(base: &str, index: u8, out: &mut [u8]) -> usize {
    let mut n = 0;
    let mut put = |b: u8| {
        if n < out.len() {
            out[n] = b;
            n += 1;
        }
    };
    for &b in base.as_bytes() {
        put(b);
    }
    if index > 1 {
        put(b' ');
        if index >= 100 {
            put(b'0' + index / 100);
        }
        if index >= 10 {
            put(b'0' + (index / 10) % 10);
        }
        put(b'0' + index % 10);
    }
    n
}

/// The next entry when cycling a list of `len` items from `cur`, forwards or
/// backwards, wrapping around.
pub fn cycle_index(cur: usize, len: usize, backwards: bool) -> usize {
    if len == 0 {
        0
    } else if backwards {
        (cur + len - 1) % len
    } else {
        (cur + 1) % len
    }
}

/// The Alt+Tab switcher: a snapshot of the focus-cycle list and a selection.
/// Opening it selects the *previous* window (index 1), like every desktop;
/// Alt+Shift+Tab starts from the last one.
pub struct Switcher {
    list: Vec<WindowId>,
    sel: usize,
}

impl Switcher {
    /// `None` when there is nothing to switch to.
    pub fn start(list: Vec<WindowId>, backwards: bool) -> Option<Self> {
        if list.is_empty() {
            return None;
        }
        let sel = if list.len() == 1 {
            0
        } else if backwards {
            list.len() - 1
        } else {
            1
        };
        Some(Self { list, sel })
    }

    pub fn advance(&mut self, backwards: bool) {
        self.sel = cycle_index(self.sel, self.list.len(), backwards);
    }

    pub fn list(&self) -> &[WindowId] {
        &self.list
    }

    pub fn selected_index(&self) -> usize {
        self.sel
    }

    pub fn selected(&self) -> WindowId {
        self.list[self.sel]
    }
}

/// Detects a double click: two presses on the same target, close together in
/// time and space.
pub struct ClickTracker {
    max_gap: u64,
    last: Option<(u64, i32, i32, WindowId)>,
}

/// Max distance (per axis) between the two presses of a double click.
pub const DOUBLE_CLICK_SLOP: i32 = 6;

impl ClickTracker {
    /// `max_gap` is the longest time between presses, in the caller's ticks.
    pub const fn new(max_gap: u64) -> Self {
        Self {
            max_gap,
            last: None,
        }
    }

    /// Registers a press on `target` at `tick`. Returns `true` when it
    /// completes a double click (the sequence then resets, so a triple click
    /// is a double followed by a single).
    pub fn press(&mut self, tick: u64, x: i32, y: i32, target: WindowId) -> bool {
        if let Some((t, lx, ly, lt)) = self.last
            && lt == target
            && tick.saturating_sub(t) <= self.max_gap
            && (x - lx).abs() <= DOUBLE_CLICK_SLOP
            && (y - ly).abs() <= DOUBLE_CLICK_SLOP
        {
            self.last = None;
            return true;
        }
        self.last = Some((tick, x, y, target));
        false
    }

    /// Forgets the pending click (e.g. after a drag).
    pub fn reset(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::anim::Anim;

    const WORK: Rect = Rect::new(12, 76, 1256, 552);

    fn spec(x: i32, y: i32) -> WindowSpec {
        WindowSpec {
            rect: Rect::new(x, y, 300, 200),
            min_w: 120,
            min_h: 80,
            resizable: true,
        }
    }

    fn table() -> (WindowManager<u32>, [WindowId; 3]) {
        let mut m = WindowManager::new(8);
        let a = m.open(spec(10, 10), 1).unwrap();
        let b = m.open(spec(50, 50), 2).unwrap();
        let c = m.open(spec(90, 90), 3).unwrap();
        (m, [a, b, c])
    }

    /// Finish whatever animations are running.
    fn settle(m: &mut WindowManager<u32>) -> Vec<Window<u32>> {
        m.step(2.0).1
    }

    #[test]
    fn open_stacks_on_top_and_focuses_the_newest() {
        let (m, [a, _b, c]) = table();
        assert_eq!(m.len(), 3);
        assert_eq!(m.focused(), Some(c));
        assert_eq!(m.z_index(a), Some(0));
        assert_eq!(m.z_index(c), Some(2));
        assert_eq!(m.windows().last().unwrap().app, 3);
    }

    #[test]
    fn ids_are_unique_and_never_reused() {
        let mut m = WindowManager::new(4);
        let a = m.open(spec(0, 0), 0).unwrap();
        m.remove(a);
        let b = m.open(spec(0, 0), 0).unwrap();
        assert_ne!(a, b);
        assert!(m.get(a).is_none());
    }

    #[test]
    fn open_beyond_the_limit_returns_the_app() {
        let mut m = WindowManager::new(2);
        m.open(spec(0, 0), 10).unwrap();
        m.open(spec(0, 0), 11).unwrap();
        assert!(m.is_full());
        assert_eq!(m.open(spec(0, 0), 12).err(), Some(12));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn there_is_no_fixed_seven_window_cap() {
        let mut m = WindowManager::new(DEFAULT_MAX_WINDOWS);
        for i in 0..DEFAULT_MAX_WINDOWS {
            assert!(m.open(spec(0, 0), i as u32).is_ok());
        }
        assert_eq!(m.len(), 32);
        assert!(m.open(spec(0, 0), 99).is_err());
    }

    #[test]
    fn raise_reorders_only_the_tail_and_updates_focus() {
        let (mut m, [a, b, c]) = table();
        assert!(m.raise(a));
        let order: Vec<_> = m.windows().iter().map(|w| w.id).collect();
        assert_eq!(order, [b, c, a]);
        assert_eq!(m.focused(), Some(a));
        assert!(!m.raise(WindowId::from_raw(999)));
    }

    #[test]
    fn hit_test_prefers_the_front_and_skips_inactive() {
        let (mut m, [a, b, c]) = table();
        // (100,100) lies inside all three.
        assert_eq!(m.topmost_at(100, 100), Some(c));
        m.raise(a);
        assert_eq!(m.topmost_at(100, 100), Some(a));
        // Only `a` covers (12, 12).
        assert_eq!(m.topmost_at(12, 12), Some(a));
        assert_eq!(m.topmost_at(5, 5), None);
        // Minimize the front window: the next one down takes the click.
        assert!(m.minimize(a));
        settle(&mut m);
        assert_eq!(m.topmost_at(100, 100), Some(c));
        let _ = b;
    }

    #[test]
    fn closing_runs_an_animation_then_removes_and_returns_the_window() {
        let (mut m, [a, b, c]) = table();
        assert!(m.request_close(b));
        assert!(!m.request_close(b)); // already closing
        assert!(m.get(b).unwrap().is_closing());
        // A closing window is not focusable or clickable.
        assert_eq!(m.topmost_at(60, 60), Some(a)); // not b
        let (active, gone) = m.step(0.1);
        assert!(active);
        assert!(gone.is_empty());
        let gone = settle(&mut m);
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0].id, b);
        assert_eq!(gone[0].app, 2);
        assert_eq!(m.len(), 2);
        assert_eq!(m.focused(), Some(c));
    }

    #[test]
    fn opening_animation_finishes_without_removing() {
        let (mut m, _) = table();
        assert!(m.windows().iter().all(|w| w.anim.is_some()));
        let (active, gone) = m.step(2.0);
        assert!(!active && gone.is_empty());
        assert!(m.windows().iter().all(|w| w.anim.is_none()));
    }

    #[test]
    fn minimize_hides_after_the_animation_and_keeps_the_app() {
        let (mut m, [a, _b, c]) = table();
        settle(&mut m);
        assert!(m.minimize(c));
        assert!(!m.minimize(c)); // already leaving
        assert!(!m.get(c).unwrap().active());
        assert!(m.get(c).unwrap().shown()); // still drawn while fading
        assert_eq!(m.get(c).unwrap().state(), WinState::Normal);
        settle(&mut m);
        let w = m.get(c).unwrap();
        assert_eq!(w.state(), WinState::Minimized);
        assert!(!w.shown());
        assert_eq!(w.app, 3);
        // Focus fell through to the next window.
        assert_eq!(m.focused(), m.windows().iter().rev().nth(1).map(|w| w.id));
        let _ = a;
    }

    #[test]
    fn activate_restores_a_minimized_window_and_raises_it() {
        let (mut m, [a, _b, c]) = table();
        settle(&mut m);
        m.minimize(c);
        settle(&mut m);
        assert!(m.activate(c));
        let w = m.get(c).unwrap();
        assert!(w.shown() && w.anim.is_some() && !w.is_leaving());
        assert_eq!(m.focused(), Some(c));
        let _ = a;
    }

    #[test]
    fn activate_cancels_a_pending_close() {
        let (mut m, [_a, _b, c]) = table();
        settle(&mut m);
        m.request_close(c);
        assert!(m.activate(c));
        assert!(!m.get(c).unwrap().is_closing());
        let gone = settle(&mut m);
        assert!(gone.is_empty());
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn closing_a_minimized_window_still_destroys_it() {
        let (mut m, [_a, _b, c]) = table();
        settle(&mut m);
        m.minimize(c);
        settle(&mut m);
        assert!(m.request_close(c));
        let gone = settle(&mut m);
        assert_eq!(gone.len(), 1);
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn maximize_fills_the_work_area_and_restore_goes_back() {
        let (mut m, [a, ..]) = table();
        let before = m.get(a).unwrap().rect;
        assert!(m.maximize(a, WORK));
        assert_eq!(m.get(a).unwrap().rect, WORK);
        assert_eq!(m.get(a).unwrap().state(), WinState::Maximized);
        assert!(!m.maximize(a, WORK)); // already
        assert!(m.unmaximize(a));
        assert_eq!(m.get(a).unwrap().rect, before);
        assert!(!m.unmaximize(a));
    }

    #[test]
    fn toggle_maximize_round_trips() {
        let (mut m, [a, ..]) = table();
        let before = m.get(a).unwrap().rect;
        assert!(m.toggle_maximize(a, WORK));
        assert!(m.get(a).unwrap().maximized);
        assert!(m.toggle_maximize(a, WORK));
        assert_eq!(m.get(a).unwrap().rect, before);
        assert!(!m.toggle_maximize(WindowId::from_raw(77), WORK));
    }

    #[test]
    fn non_resizable_windows_refuse_maximize_and_resize() {
        let mut m = WindowManager::new(4);
        let id = m
            .open(
                WindowSpec {
                    resizable: false,
                    ..spec(10, 10)
                },
                0,
            )
            .unwrap();
        assert!(!m.maximize(id, WORK));
        let start = m.get(id).unwrap().rect;
        assert!(!m.resize(id, ResizeEdge::SE, start, (50, 50), (1280, 720)));
        assert_eq!(m.get(id).unwrap().rect, start);
    }

    #[test]
    fn maximized_windows_do_not_move_or_resize() {
        let (mut m, [a, ..]) = table();
        m.maximize(a, WORK);
        assert!(!m.move_to(a, 0, 0, 1280, 720));
        assert!(!m.resize(a, ResizeEdge::E, WORK, (10, 0), (1280, 720)));
        assert_eq!(m.get(a).unwrap().rect, WORK);
    }

    #[test]
    fn snapping_tiles_the_work_area_and_restore_goes_back() {
        let (mut m, [a, ..]) = table();
        let before = m.get(a).unwrap().rect;
        assert!(m.snap_to(a, SnapZone::Left, WORK));
        let w = m.get(a).unwrap();
        assert_eq!(w.rect, snap::zone_rect(SnapZone::Left, WORK));
        assert_eq!(w.snap_state(), Some(SnapZone::Left));
        assert!(!w.maximized);
        assert!(!m.snap_to(a, SnapZone::Left, WORK)); // already there
        // Left -> top-left quarter keeps the ORIGINAL free rectangle.
        assert!(m.snap_to(a, SnapZone::TopLeft, WORK));
        assert!(m.unmaximize(a));
        let w = m.get(a).unwrap();
        assert_eq!((w.rect, w.snap_state()), (before, None));
        assert!(!m.unmaximize(a));
    }

    #[test]
    fn snapping_from_maximized_and_to_maximized_keeps_one_restore_rect() {
        let (mut m, [a, ..]) = table();
        let before = m.get(a).unwrap().rect;
        assert!(m.maximize(a, WORK));
        assert!(m.snap_to(a, SnapZone::Right, WORK));
        let w = m.get(a).unwrap();
        assert!(!w.maximized);
        assert_eq!(w.snap_state(), Some(SnapZone::Right));
        assert!(m.snap_to(a, SnapZone::Maximize, WORK));
        assert_eq!(m.get(a).unwrap().snap, None);
        assert!(m.toggle_maximize(a, WORK));
        assert_eq!(m.get(a).unwrap().rect, before);
        // toggle on a tiled window restores it too.
        assert!(m.snap_to(a, SnapZone::BottomLeft, WORK));
        assert!(m.toggle_maximize(a, WORK));
        assert_eq!(m.get(a).unwrap().rect, before);
    }

    #[test]
    fn a_tiled_window_respects_its_minimum_and_non_resizable_ones_refuse() {
        let mut m = WindowManager::new(4);
        let id = m
            .open(
                WindowSpec {
                    min_w: 800,
                    ..spec(10, 10)
                },
                0,
            )
            .unwrap();
        assert!(m.snap_to(id, SnapZone::Right, WORK));
        let r = m.get(id).unwrap().rect;
        assert_eq!((r.w, r.right()), (800, WORK.right()));
        let fixed = m
            .open(
                WindowSpec {
                    resizable: false,
                    ..spec(10, 10)
                },
                1,
            )
            .unwrap();
        assert!(!m.snap_to(fixed, SnapZone::Left, WORK));
        assert_eq!(m.get(fixed).unwrap().snap_state(), None);
    }

    #[test]
    fn snapping_animates_through_the_zoom() {
        let (mut m, [a, ..]) = table();
        settle(&mut m);
        assert!(m.snap_to(a, SnapZone::TopRight, WORK));
        assert!(m.get(a).unwrap().zoom.is_some());
        m.step(2.0);
        assert!(m.get(a).unwrap().zoom.is_none());
        // Tiled windows still resize (from any edge) and then are free again.
        let start = m.get(a).unwrap().rect;
        assert!(m.resize(a, ResizeEdge::W, start, (-40, 0), (1280, 720)));
        assert_eq!(m.get(a).unwrap().snap, None);
    }

    #[test]
    fn dragging_a_maximised_or_tiled_title_restores_under_the_pointer() {
        let (mut m, [a, ..]) = table();
        let free = m.get(a).unwrap().rect; // 300 x 200
        assert_eq!(m.restore_for_drag(a, 5, 5), None); // free: nothing to do
        assert!(m.maximize(a, WORK));
        settle(&mut m);
        // Grab at the middle of the 1256 px bar: the restored window centres on the pointer.
        let (dx, dy) = m
            .restore_for_drag(a, WORK.x + WORK.w / 2, WORK.y + 10)
            .unwrap();
        let w = m.get(a).unwrap();
        assert!(!w.maximized && w.zoom.is_none());
        assert_eq!((w.rect.w, w.rect.h), (free.w, free.h));
        assert_eq!(dx, free.w / 2);
        assert_eq!(dy, 10);
        assert_eq!(w.rect.x + dx, WORK.x + WORK.w / 2);
        // Grabbing near the left edge keeps the grab near the left edge.
        m.snap_to(a, SnapZone::Left, WORK);
        settle(&mut m);
        let (dx, _) = m.restore_for_drag(a, 2, WORK.y + 4).unwrap();
        assert!(dx <= 2);
        assert_eq!(m.get(a).unwrap().snap, None);
    }

    #[test]
    fn moving_a_tiled_window_frees_it() {
        let (mut m, [a, ..]) = table();
        m.snap_to(a, SnapZone::Left, WORK);
        assert!(m.move_to(a, 300, 300, 1280, 720));
        assert_eq!(m.get(a).unwrap().snap, None);
    }

    // ------------------------------------------------------------ workspaces

    #[test]
    fn windows_open_on_the_current_workspace_and_a_switch_slides_them() {
        let (mut m, [a, b, c]) = table();
        settle(&mut m);
        assert_eq!(m.workspace(), 0);
        assert_eq!(m.windows_on(0), 3);
        assert!(m.switch_workspace(1));
        assert!(!m.switch_workspace(1)); // already there
        assert!(!m.switch_workspace(MAX_WORKSPACES)); // out of range
        // The windows of workspace 0 slide away (still drawn during the slide, then hidden).
        for id in [a, b, c] {
            let w = m.get(id).unwrap();
            assert!(w.shown() && w.is_leaving() && !w.active());
        }
        // Nothing is focusable meanwhile; once the slide ends they are hidden.
        assert_eq!(m.focused(), None);
        settle(&mut m);
        for id in [a, b, c] {
            let w = m.get(id).unwrap();
            assert!(!w.shown() && w.off_ws && w.anim.is_none());
        }
        assert_eq!(m.topmost_at(100, 100), None);
        // A window opened now lives on workspace 1.
        let d = m.open(spec(20, 20), 4).unwrap();
        assert_eq!(m.get(d).unwrap().ws, 1);
        assert_eq!((m.windows_on(0), m.windows_on(1)), (3, 1));
    }

    #[test]
    fn going_back_brings_the_windows_back_with_a_slide_in() {
        let (mut m, [a, ..]) = table();
        settle(&mut m);
        m.switch_workspace(1);
        settle(&mut m);
        assert!(m.switch_workspace(0));
        let w = m.get(a).unwrap();
        assert!(w.shown() && !w.off_ws);
        let an = w.anim.unwrap();
        assert!(!an.is_closing());
        assert_eq!(an.flavor(), crate::ui::anim::Flavor::Slide(-1));
        settle(&mut m);
        assert!(m.get(a).unwrap().active());
        assert!(m.focused().is_some());
    }

    #[test]
    fn a_window_moves_between_workspaces_and_activating_it_follows() {
        let (mut m, [a, b, c]) = table();
        settle(&mut m);
        assert!(m.move_to_workspace(c, 1));
        assert_eq!(m.get(c).unwrap().ws, 1);
        // It slides away toward the workspace it went to (opposite to the slide of a switch).
        assert!(m.get(c).unwrap().is_leaving());
        settle(&mut m);
        assert!(m.get(c).unwrap().off_ws);
        assert_eq!(m.focused(), Some(b));
        assert!(!m.move_to_workspace(c, MAX_WORKSPACES));
        assert!(!m.move_to_workspace(WindowId::from_raw(999), 0));
        // Activating a window on another workspace switches to it.
        assert!(m.activate(c));
        assert_eq!(m.workspace(), 1);
        settle(&mut m);
        assert_eq!(m.focused(), Some(c));
        assert!(m.get(a).unwrap().off_ws && m.get(b).unwrap().off_ws);
        // Moving a window to the workspace on screen is a no-op that keeps it shown.
        assert!(m.move_to_workspace(c, 1));
        assert!(m.get(c).unwrap().shown());
    }

    #[test]
    fn minimised_windows_are_not_animated_across_workspaces() {
        let (mut m, [a, ..]) = table();
        settle(&mut m);
        m.minimize(a);
        settle(&mut m);
        m.switch_workspace(1);
        let w = m.get(a).unwrap();
        assert!(w.minimized && w.off_ws && w.anim.is_none());
        // Back on workspace 0 it is still minimised (not shown).
        m.switch_workspace(0);
        let w = m.get(a).unwrap();
        assert!(!w.off_ws && w.minimized && !w.shown());
    }

    #[test]
    fn the_switcher_offers_one_empty_workspace_up_to_the_cap() {
        let mut m = WindowManager::new(8);
        assert_eq!(m.visible_workspaces(), MIN_WORKSPACES);
        let a = m.open(spec(0, 0), 1).unwrap();
        assert_eq!(m.visible_workspaces(), 2); // workspace 0 used, 1 empty
        m.move_to_workspace(a, 1);
        assert_eq!(m.visible_workspaces(), 3);
        m.move_to_workspace(a, 3);
        assert_eq!(m.visible_workspaces(), MAX_WORKSPACES);
        // Standing on an empty workspace keeps it in the list.
        m.switch_workspace(2);
        assert!(m.visible_workspaces() >= 3);
        // A closing window no longer counts as using its workspace.
        m.request_close(a);
        assert_eq!(m.visible_workspaces(), 3);
    }

    #[test]
    fn minimized_maximized_window_returns_maximized() {
        let (mut m, [a, ..]) = table();
        settle(&mut m);
        m.maximize(a, WORK);
        m.minimize(a);
        settle(&mut m);
        m.activate(a);
        settle(&mut m);
        let w = m.get(a).unwrap();
        assert!(w.maximized);
        assert_eq!(w.rect, WORK);
    }

    #[test]
    fn move_clamps_to_keep_the_title_bar_on_screen() {
        let (mut m, [a, ..]) = table();
        assert!(m.move_to(a, -500, -500, 1280, 720));
        let r = m.get(a).unwrap().rect;
        assert_eq!((r.x, r.y), (0, crate::windowing::window::MENUBAR_H));
        assert!(m.move_to(a, 5000, 5000, 1280, 720));
        let r = m.get(a).unwrap().rect;
        assert_eq!(r.x, 1280 - r.w);
        assert_eq!(r.y, 720 - crate::windowing::window::TITLE_H);
        assert_eq!((r.w, r.h), (300, 200)); // size untouched
    }

    #[test]
    fn resize_uses_the_drag_start_rect_and_the_window_minimum() {
        let (mut m, [a, ..]) = table();
        let start = m.get(a).unwrap().rect; // 10,10 300x200
        assert!(m.resize(a, ResizeEdge::SE, start, (100, 50), (1280, 720)));
        assert_eq!(m.get(a).unwrap().rect, Rect::new(10, 10, 400, 250));
        // Dragging back by a different delta is relative to `start`, no drift.
        assert!(m.resize(a, ResizeEdge::SE, start, (10, 10), (1280, 720)));
        assert_eq!(m.get(a).unwrap().rect, Rect::new(10, 10, 310, 210));
        // Shrinking stops at the minimum size (120 x 80).
        m.resize(a, ResizeEdge::SE, start, (-999, -999), (1280, 720));
        let r = m.get(a).unwrap().rect;
        assert_eq!((r.w, r.h), (120, 80));
        assert_eq!((r.x, r.y), (10, 10));
    }

    #[test]
    fn switch_list_is_most_recently_used_with_focus_first() {
        let (mut m, [a, b, c]) = table();
        assert_eq!(m.switch_list(), [c, b, a]);
        m.raise(a);
        assert_eq!(m.switch_list(), [a, c, b]);
        m.raise(b);
        assert_eq!(m.switch_list(), [b, a, c]);
    }

    #[test]
    fn switch_list_includes_minimized_and_skips_closing() {
        let (mut m, [a, b, c]) = table();
        settle(&mut m);
        m.minimize(c);
        settle(&mut m);
        m.request_close(a);
        // Focus is `b`; `c` (minimized) is listed, `a` (closing) is not.
        assert_eq!(m.switch_list(), [b, c]);
    }

    #[test]
    fn removing_a_window_drops_it_from_the_cycle() {
        let (mut m, [a, b, c]) = table();
        m.remove(b);
        assert_eq!(m.switch_list(), [c, a]);
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn switcher_selects_the_previous_window_first() {
        let (m, [a, b, c]) = table();
        let mut s = Switcher::start(m.switch_list(), false).unwrap();
        assert_eq!(s.selected(), b);
        s.advance(false);
        assert_eq!(s.selected(), a);
        s.advance(false);
        assert_eq!(s.selected(), c); // wrapped
        s.advance(true);
        assert_eq!(s.selected(), a);
    }

    #[test]
    fn switcher_backwards_starts_at_the_last_and_handles_small_lists() {
        let (m, [a, ..]) = table();
        let s = Switcher::start(m.switch_list(), true).unwrap();
        assert_eq!(s.selected(), a);
        let one = Switcher::start(alloc::vec![a], false).unwrap();
        assert_eq!(one.selected(), a);
        assert!(Switcher::start(Vec::new(), false).is_none());
    }

    #[test]
    fn cycle_index_wraps_both_ways() {
        assert_eq!(cycle_index(0, 3, false), 1);
        assert_eq!(cycle_index(2, 3, false), 0);
        assert_eq!(cycle_index(0, 3, true), 2);
        assert_eq!(cycle_index(0, 0, true), 0);
    }

    #[test]
    fn cascade_offsets_each_window_and_wraps() {
        let base = Rect::new(100, 100, 400, 300);
        assert_eq!(cascade_rect(base, 0, WORK), base);
        assert_eq!(cascade_rect(base, 1, WORK), Rect::new(128, 128, 400, 300));
        assert_eq!(cascade_rect(base, 2, WORK), Rect::new(156, 156, 400, 300));
        // After a full lap the row restarts, nudged 12 px right.
        assert_eq!(
            cascade_rect(base, CASCADE_WRAP, WORK),
            Rect::new(112, 100, 400, 300)
        );
    }

    #[test]
    fn cascade_positions_stay_distinct_for_a_full_table() {
        let base = Rect::new(70, 80, 512, 320);
        let mut seen: Vec<(i32, i32)> = Vec::new();
        for k in 0..DEFAULT_MAX_WINDOWS {
            let r = cascade_rect(base, k, WORK);
            assert!(!seen.contains(&(r.x, r.y)), "k={k}");
            seen.push((r.x, r.y));
        }
    }

    #[test]
    fn numbered_names_follow_the_instance_index() {
        let mut buf = [0u8; 16];
        let n = numbered_name("shell", 1, &mut buf);
        assert_eq!(&buf[..n], b"shell");
        let n = numbered_name("shell", 2, &mut buf);
        assert_eq!(&buf[..n], b"shell 2");
        let n = numbered_name("shell", 12, &mut buf);
        assert_eq!(&buf[..n], b"shell 12");
        let n = numbered_name("", 7, &mut buf);
        assert_eq!(&buf[..n], b" 7");
        let n = numbered_name("", 1, &mut buf);
        assert_eq!(n, 0);
    }

    #[test]
    fn numbered_names_truncate_to_the_buffer() {
        let mut buf = [0u8; 6];
        let n = numbered_name("compositor", 3, &mut buf);
        assert_eq!(&buf[..n], b"compos");
    }

    #[test]
    fn cascade_always_fits_inside_the_work_area() {
        let base = Rect::new(250, 120, 780, 520);
        for k in 0..20 {
            let r = cascade_rect(base, k, WORK);
            assert!(r.x >= WORK.x && r.y >= WORK.y, "k={k}");
            assert!(
                r.right() <= WORK.right() && r.bottom() <= WORK.bottom(),
                "k={k}"
            );
            assert_eq!((r.w, r.h), (780, 520));
        }
        // A window bigger than the work area is shrunk to it.
        let huge = cascade_rect(Rect::new(0, 0, 4000, 4000), 3, WORK);
        assert_eq!(huge, WORK);
    }

    #[test]
    fn cascade_gives_neighbours_distinct_positions_while_room_remains() {
        let base = Rect::new(70, 80, 512, 320);
        let mut seen: Vec<(i32, i32)> = Vec::new();
        for k in 0..CASCADE_WRAP {
            let r = cascade_rect(base, k, WORK);
            assert!(!seen.contains(&(r.x, r.y)), "k={k}");
            seen.push((r.x, r.y));
        }
    }

    #[test]
    fn click_tracker_detects_a_double_click() {
        let id = WindowId::from_raw(1);
        let mut t = ClickTracker::new(100);
        assert!(!t.press(1000, 50, 50, id));
        assert!(t.press(1050, 52, 49, id));
        // The sequence reset: a third quick click is a fresh first click.
        assert!(!t.press(1060, 52, 49, id));
    }

    #[test]
    fn click_tracker_rejects_slow_far_or_foreign_clicks() {
        let (a, b) = (WindowId::from_raw(1), WindowId::from_raw(2));
        let mut t = ClickTracker::new(100);
        t.press(1000, 50, 50, a);
        assert!(!t.press(1101, 50, 50, a)); // too slow
        t.press(2000, 50, 50, a);
        assert!(!t.press(2010, 80, 50, a)); // too far
        t.press(3000, 50, 50, a);
        assert!(!t.press(3010, 50, 50, b)); // different window
        t.press(4000, 50, 50, a);
        t.reset();
        assert!(!t.press(4010, 50, 50, a));
    }

    #[test]
    fn empty_table_is_safe() {
        let mut m: WindowManager<u32> = WindowManager::default();
        assert_eq!(m.limit(), DEFAULT_MAX_WINDOWS);
        assert!(m.is_empty());
        assert_eq!(m.focused(), None);
        assert_eq!(m.topmost_at(0, 0), None);
        assert!(m.switch_list().is_empty());
        let (active, gone) = m.step(1.0);
        assert!(!active && gone.is_empty());
        assert!(!m.minimize(WindowId::from_raw(1)));
        assert!(!m.activate(WindowId::from_raw(1)));
        assert!(!m.request_close(WindowId::from_raw(1)));
    }

    #[test]
    fn open_close_cycles_do_not_leak_table_entries() {
        let mut m = WindowManager::new(4);
        for i in 0..200u32 {
            let id = m.open(spec(0, 0), i).unwrap();
            m.request_close(id);
            let gone = settle(&mut m);
            assert_eq!(gone.len(), 1);
        }
        assert!(m.is_empty());
        assert!(m.switch_list().is_empty());
    }

    #[test]
    fn maximize_zooms_the_rect_and_settles() {
        let (mut m, [a, ..]) = table();
        settle(&mut m);
        let before = m.get(a).unwrap().rect;
        assert!(m.maximize(a, WORK));
        let w = m.get(a).unwrap();
        // The final rect is set at once (hit testing, layout); the drawn one travels.
        assert_eq!(w.rect, WORK);
        assert_eq!(w.visual_rect(), before);
        assert!(w.zoom.is_some());
        let (active, _) = m.step(0.03);
        assert!(active);
        let mid = m.get(a).unwrap().visual_rect();
        assert!(mid != before && mid != WORK, "{mid:?}");
        let mut n = 0;
        while m.step(0.016).0 {
            n += 1;
            assert!(n < 400);
        }
        let w = m.get(a).unwrap();
        assert!(w.zoom.is_none());
        assert_eq!(w.visual_rect(), WORK);
        // Restoring zooms back.
        assert!(m.unmaximize(a));
        assert_eq!(m.get(a).unwrap().visual_rect(), WORK);
        settle(&mut m);
        assert_eq!(m.get(a).unwrap().visual_rect(), before);
    }

    #[test]
    fn zoom_is_interruptible() {
        let (mut m, [a, ..]) = table();
        settle(&mut m);
        m.maximize(a, WORK);
        m.step(0.02);
        let now = m.get(a).unwrap().visual_rect();
        m.unmaximize(a); // change of mind mid-flight
        assert_eq!(m.get(a).unwrap().visual_rect(), now);
        settle(&mut m);
    }

    #[test]
    fn minimize_and_restore_fly_to_and_from_the_dock() {
        use crate::ui::anim::Flavor;
        let (mut m, [a, ..]) = table();
        settle(&mut m);
        assert!(m.minimize(a));
        assert_eq!(m.get(a).unwrap().anim.unwrap().flavor(), Flavor::Dock);
        settle(&mut m);
        assert!(m.get(a).unwrap().minimized);
        assert!(m.activate(a));
        let an = m.get(a).unwrap().anim.unwrap();
        assert_eq!(an.flavor(), Flavor::Dock);
        assert!(!an.is_closing());
        // A plain close is a pop, not a dock trip.
        let (mut m, [a, ..]) = table();
        settle(&mut m);
        m.request_close(a);
        assert_eq!(m.get(a).unwrap().anim.unwrap().flavor(), Flavor::Pop);
    }

    #[test]
    fn reduce_motion_skips_the_zoom() {
        let (mut m, [a, ..]) = table();
        settle(&mut m);
        crate::ui::anim::set_reduce_motion(true);
        m.maximize(a, WORK);
        let done = m.get(a).unwrap().zoom.is_none_or(|z| z.finished());
        crate::ui::anim::set_reduce_motion(false);
        assert!(done);
    }

    #[test]
    fn leaving_anim_is_the_existing_close_animation() {
        // The table reuses `Anim::close`, so the kernel's fade/slide code is
        // shared with the old fixed table.
        let (mut m, [a, ..]) = table();
        settle(&mut m);
        m.minimize(a);
        assert!(
            m.get(a)
                .unwrap()
                .anim
                .map(|x: Anim| x.is_closing())
                .unwrap()
        );
    }
}
