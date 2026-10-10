//! helpers (split out of `winman.rs`).

use super::*;

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
    pub(super) list: Vec<WindowId>,
    pub(super) sel: usize,
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
    pub(super) max_gap: u64,
    pub(super) last: Option<(u64, i32, i32, WindowId)>,
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
