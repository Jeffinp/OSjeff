//! Window-manager rules over a z-order list: focus, raise, hit-testing, process
//! lookup.
//!
//! The kernel owns the window records (rect, visibility, animation); these
//! functions only need to *ask* about them, so they take closures and stay pure.
//! `order` is always back-to-front: the last entry is the topmost window.

use crate::windowing::window::Rect;

/// The topmost window for which `active` holds (the one that has focus).
pub fn focused(order: &[usize], active: impl Fn(usize) -> bool) -> Option<usize> {
    order.iter().rev().copied().find(|&w| active(w))
}

/// Moves `win` to the end (front) of `order`, keeping everyone else's relative
/// order. Returns `false` and leaves `order` untouched if `win` is not in it.
pub fn bring_to_front(order: &mut [usize], win: usize) -> bool {
    let Some(pos) = order.iter().position(|&w| w == win) else {
        return false;
    };
    order[pos..].rotate_left(1);
    true
}

/// The frontmost window whose *active* rect contains `(px, py)`. `rect_of`
/// returns a window's rect only while it can take clicks (shown and not
/// animating out).
pub fn topmost_at(
    order: &[usize],
    px: i32,
    py: i32,
    rect_of: impl Fn(usize) -> Option<Rect>,
) -> Option<usize> {
    order
        .iter()
        .rev()
        .copied()
        .find(|&w| rect_of(w).is_some_and(|r| r.contains(px, py)))
}

/// Index of the window whose live process id is `pid`. Pid 0 means "no
/// process" and never matches.
pub fn window_of_pid(pids: &[u16], pid: u16) -> Option<usize> {
    if pid == 0 {
        return None;
    }
    pids.iter().position(|&p| p == pid)
}

#[cfg(test)]
mod tests;
