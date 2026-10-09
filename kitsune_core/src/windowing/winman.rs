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
mod tests;
