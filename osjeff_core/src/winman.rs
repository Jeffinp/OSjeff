//! Dynamic window table: any number of windows (up to a configurable limit),
//! each owning an application payload `A`, with z-order, focus, hit-testing,
//! open / close / minimize / maximize / restore, cascading placement,
//! move / resize, most-recently-used focus cycling (Alt+Tab) and a signature
//! of the scene for the compositor's cached static layer.
//!
//! The table is pure logic: it never draws and knows nothing about what `A`
//! is. The kernel instantiates it with its app-instance type; the tests below
//! use plain integers. Windows are stored back-to-front, so the z-order *is*
//! the vector order and the last window is the topmost.
//!
//! The older free functions in [`crate::wm`] (which work over a fixed `order`
//! slice) keep their meaning; this module is the dynamic generalisation.

use alloc::vec::Vec;

use crate::anim::Anim;
use crate::window::{Rect, ResizeEdge, WindowId};

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
}

/// One window record.
pub struct Window<A> {
    pub id: WindowId,
    /// Current on-screen rectangle (the work area while maximized).
    pub rect: Rect,
    /// The rectangle to return to when un-maximizing.
    pub restore: Rect,
    pub minimized: bool,
    pub maximized: bool,
    pub anim: Option<Anim>,
    pub leaving: Leaving,
    pub min_w: i32,
    pub min_h: i32,
    pub resizable: bool,
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

    /// Animating out (closing or minimizing): no longer takes focus or clicks.
    pub fn is_leaving(&self) -> bool {
        matches!(self.anim, Some(a) if a.is_closing())
    }

    /// Being destroyed (as opposed to merely minimized).
    pub fn is_closing(&self) -> bool {
        self.is_leaving() && self.leaving == Leaving::Destroy
    }

    /// On screen: not minimized (an animating-out window still is).
    pub fn shown(&self) -> bool {
        !self.minimized
    }

    /// Accepts focus and clicks: shown and not animating out.
    pub fn active(&self) -> bool {
        self.shown() && !self.is_leaving()
    }
}

/// The dynamic window table. See the module docs.
pub struct WindowManager<A> {
    wins: Vec<Window<A>>,
    /// Most recently used first.
    mru: Vec<WindowId>,
    next_id: u32,
    limit: usize,
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
            anim: Some(Anim::open()),
            leaving: Leaving::Destroy,
            min_w: spec.min_w,
            min_h: spec.min_h,
            resizable: spec.resizable,
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
        let Some(w) = self.get_mut(id) else {
            return false;
        };
        if w.minimized {
            w.minimized = false;
            w.anim = Some(Anim::open());
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
        w.anim = Some(Anim::close());
        true
    }

    /// Maximizes to `work`, remembering the current rect. No-op (`false`) for
    /// a non-resizable or already maximized window.
    pub fn maximize(&mut self, id: WindowId, work: Rect) -> bool {
        let Some(w) = self.get_mut(id) else {
            return false;
        };
        if !w.resizable || w.maximized {
            return false;
        }
        w.restore = w.rect;
        w.rect = work;
        w.maximized = true;
        true
    }

    /// Leaves the maximized state, returning to the remembered rect.
    pub fn unmaximize(&mut self, id: WindowId) -> bool {
        let Some(w) = self.get_mut(id) else {
            return false;
        };
        if !w.maximized {
            return false;
        }
        w.rect = w.restore;
        w.maximized = false;
        true
    }

    /// Maximize if normal, restore if maximized.
    pub fn toggle_maximize(&mut self, id: WindowId, work: Rect) -> bool {
        match self.get(id).map(|w| w.maximized) {
            Some(true) => self.unmaximize(id),
            Some(false) => self.maximize(id, work),
            None => false,
        }
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
        true
    }

    // ---------------------------------------------------------- signature

    /// Compact hash of the *static* scene: for every window (back to front)
    /// its id, rect, flags and whether it animates, plus the drag target. When
    /// it changes the compositor rebuilds its cached static layer. Unlike the
    /// fixed-slot [`crate::wm::scene_signature`] it covers geometry, so a
    /// maximize or a finished move/resize invalidates the cache; the rect of
    /// the `drag` window itself is left out so dragging stays on the cheap
    /// damage path.
    pub fn signature(&self, drag: Option<WindowId>) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |v: u32| {
            for b in v.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        for w in &self.wins {
            feed(w.id.raw());
            // The window being dragged / resized is drawn on top of the cached
            // layer every frame, so its moving rect must not invalidate it.
            if drag != Some(w.id) {
                for v in [w.rect.x, w.rect.y, w.rect.w, w.rect.h] {
                    feed(v as u32);
                }
            }
            feed(
                (w.shown() as u32)
                    | ((w.anim.is_some() as u32) << 1)
                    | ((w.is_leaving() as u32) << 2)
                    | ((w.maximized as u32) << 3),
            );
        }
        // +1 so "no drag" and "dragging window 0" differ.
        feed(drag.map_or(0, |d| d.raw().wrapping_add(1)));
        h
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
    use crate::anim::Anim;

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
        assert_eq!((r.x, r.y), (0, 0));
        assert!(m.move_to(a, 5000, 5000, 1280, 720));
        let r = m.get(a).unwrap().rect;
        assert_eq!(r.x, 1280 - r.w);
        assert_eq!(r.y, 720 - crate::window::TITLE_H);
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

    fn sig(m: &WindowManager<u32>, drag: Option<WindowId>) -> u64 {
        m.signature(drag)
    }

    #[test]
    fn signature_is_deterministic_and_tracks_every_ingredient() {
        let (mut m, [a, b, c]) = table();
        settle(&mut m);
        let base = sig(&m, None);
        assert_eq!(base, sig(&m, None));
        assert_ne!(base, sig(&m, Some(a))); // drag target
        assert_ne!(sig(&m, Some(a)), sig(&m, Some(b)));

        m.raise(a); // z-order
        let raised = sig(&m, None);
        assert_ne!(base, raised);

        m.move_to(c, 200, 200, 1280, 720); // geometry
        let moved = sig(&m, None);
        assert_ne!(raised, moved);

        m.maximize(b, WORK); // maximize
        let maxed = sig(&m, None);
        assert_ne!(moved, maxed);

        m.request_close(c); // animation
        assert_ne!(maxed, sig(&m, None));
    }

    #[test]
    fn dragging_a_window_does_not_change_the_signature() {
        let (mut m, [a, b, _c]) = table();
        settle(&mut m);
        let before = sig(&m, Some(a));
        m.move_to(a, 300, 200, 1280, 720);
        assert_eq!(before, sig(&m, Some(a)));
        let start = m.get(a).unwrap().rect;
        m.resize(a, ResizeEdge::SE, start, (40, 40), (1280, 720));
        assert_eq!(before, sig(&m, Some(a)));
        // Moving a *different* window while `a` is dragged does change it.
        m.move_to(b, 400, 300, 1280, 720);
        assert_ne!(before, sig(&m, Some(a)));
        // And once the drag ends the new rect is part of the signature.
        assert_ne!(sig(&m, None), sig(&m, Some(a)));
    }

    #[test]
    fn signature_distinguishes_minimized_from_shown() {
        let (mut m, [a, ..]) = table();
        settle(&mut m);
        let shown = sig(&m, None);
        m.minimize(a);
        settle(&mut m);
        assert_ne!(shown, sig(&m, None));
    }

    #[test]
    fn signature_distinguishes_every_swap_with_many_windows() {
        let mut m = WindowManager::new(32);
        let ids: Vec<_> = (0..20)
            .map(|i| m.open(spec(i, i), i as u32).unwrap())
            .collect();
        settle(&mut m);
        let mut seen = alloc::vec![sig(&m, None)];
        // Every successive raise yields a z-order never seen before.
        for &id in &ids[..19] {
            m.raise(id);
            let s = sig(&m, None);
            assert!(!seen.contains(&s));
            seen.push(s);
        }
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
        let _ = sig(&m, None);
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
