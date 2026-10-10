//! rows (split out of `ui.rs`).

use super::*;

/// Items per row of the view (1 for the list).
pub fn columns_of(mode: ViewMode, vw: i32) -> usize {
    match mode {
        ViewMode::List => 1,
        ViewMode::Icons => (((vw - 2 * GRID_PAD) / CELL_W).max(1)) as usize,
    }
}

/// Left edge of the icon grid (it is centred in the viewport).
pub(super) fn grid_x0(vw: i32, cols: usize) -> i32 {
    GRID_PAD + ((vw - 2 * GRID_PAD - cols as i32 * CELL_W) / 2).max(0)
}

/// Height of all `n` items.
pub fn content_height(mode: ViewMode, vw: i32, n: usize) -> i32 {
    if n == 0 {
        return 0;
    }
    match mode {
        ViewMode::List => 2 * LIST_PAD + n as i32 * ROW_H,
        ViewMode::Icons => {
            let rows = n.div_ceil(columns_of(mode, vw)) as i32;
            2 * GRID_PAD + rows * CELL_H
        }
    }
}

/// The largest scroll offset.
pub fn max_scroll(mode: ViewMode, vw: i32, vh: i32, n: usize) -> i32 {
    (content_height(mode, vw, n) - vh).max(0)
}

/// The hit rectangle of item `i`, in content coordinates (relative to the viewport's top left
/// with scroll 0). The selection pill of a list row and the highlight of an icon use it.
pub fn item_rect(mode: ViewMode, vw: i32, i: usize) -> Rect {
    match mode {
        ViewMode::List => Rect::new(
            ROW_INSET,
            LIST_PAD + i as i32 * ROW_H,
            (vw - 2 * ROW_INSET).max(0),
            ROW_H,
        ),
        ViewMode::Icons => {
            let cols = columns_of(mode, vw);
            let (r, c) = ((i / cols) as i32, (i % cols) as i32);
            let x0 = grid_x0(vw, cols);
            Rect::new(
                x0 + c * CELL_W + 4,
                GRID_PAD + r * CELL_H + 2,
                CELL_W - 8,
                CELL_H - 4,
            )
        }
    }
}

/// The items (first, one past the last) that intersect a viewport of height `vh` scrolled by
/// `scroll`, for `n` items.
pub fn visible_range(mode: ViewMode, vw: i32, vh: i32, scroll: i32, n: usize) -> (usize, usize) {
    if n == 0 || vh <= 0 {
        return (0, 0);
    }
    let cols = columns_of(mode, vw);
    let (pitch, pad) = match mode {
        ViewMode::List => (ROW_H, LIST_PAD),
        ViewMode::Icons => (CELL_H, GRID_PAD),
    };
    let first_row = ((scroll - pad).max(0) / pitch) as usize;
    let last_row = ((scroll + vh - pad - 1).max(0) / pitch) as usize;
    let first = (first_row * cols).min(n);
    let end = ((last_row + 1) * cols).min(n);
    (first, end)
}

/// The item under viewport-relative `(x, y)` when scrolled by `scroll`.
pub fn item_at(mode: ViewMode, vw: i32, scroll: i32, x: i32, y: i32, n: usize) -> Option<usize> {
    if x < 0 || y < 0 || x >= vw {
        return None;
    }
    let cy = y + scroll;
    let cols = columns_of(mode, vw);
    let (pitch, pad) = match mode {
        ViewMode::List => (ROW_H, LIST_PAD),
        ViewMode::Icons => (CELL_H, GRID_PAD),
    };
    if cy < pad {
        return None;
    }
    let row = ((cy - pad) / pitch) as usize;
    let col = match mode {
        ViewMode::List => 0,
        ViewMode::Icons => {
            let x0 = grid_x0(vw, cols);
            if x < x0 {
                return None;
            }
            let c = ((x - x0) / CELL_W) as usize;
            if c >= cols {
                return None;
            }
            c
        }
    };
    let i = row * cols + col;
    if i >= n {
        return None;
    }
    item_rect(mode, vw, i).contains(x, cy).then_some(i)
}

/// The items touched by `band` (content coordinates): what a rubber band selects.
pub fn items_in_rect(mode: ViewMode, vw: i32, n: usize, band: Rect) -> Vec<usize> {
    if n == 0 || band.w <= 0 || band.h <= 0 {
        return Vec::new();
    }
    let cols = columns_of(mode, vw);
    let (pitch, pad) = match mode {
        ViewMode::List => (ROW_H, LIST_PAD),
        ViewMode::Icons => (CELL_H, GRID_PAD),
    };
    let r0 = ((band.y - pad).max(0) / pitch) as usize;
    let r1 = ((band.bottom() - 1 - pad).max(0) / pitch) as usize;
    let mut out = Vec::new();
    for row in r0..=r1 {
        for col in 0..cols {
            let i = row * cols + col;
            if i >= n {
                break;
            }
            if item_rect(mode, vw, i).intersection(&band).is_some() {
                out.push(i);
            }
        }
    }
    out
}

/// The scroll offset that brings item `i` fully into a viewport of height `vh` with the least
/// movement (unchanged when it is already visible).
pub fn reveal(mode: ViewMode, vw: i32, vh: i32, scroll: i32, i: usize, n: usize) -> i32 {
    let r = item_rect(mode, vw, i);
    // Include the padding when the item is in the first or last row.
    let top = if r.y <= GRID_PAD.max(LIST_PAD) + 4 {
        0
    } else {
        r.y - 4
    };
    let bottom = {
        let b = r.bottom() + 4;
        if b + GRID_PAD.max(LIST_PAD) >= content_height(mode, vw, n) {
            content_height(mode, vw, n)
        } else {
            b
        }
    };
    let s = if top < scroll {
        top
    } else if bottom > scroll + vh {
        bottom - vh
    } else {
        scroll
    };
    s.clamp(0, max_scroll(mode, vw, vh, n))
}

/// An arrow-key direction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

/// Where an arrow key moves the cursor from `i` among `n` items.
pub fn step_index(mode: ViewMode, vw: i32, i: usize, dir: Dir, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let cols = columns_of(mode, vw);
    let last = n - 1;
    let i = i.min(last);
    match (mode, dir) {
        (ViewMode::List, Dir::Up | Dir::Left) => i.saturating_sub(1),
        (ViewMode::List, Dir::Down | Dir::Right) => (i + 1).min(last),
        (ViewMode::Icons, Dir::Left) => i.saturating_sub(1),
        (ViewMode::Icons, Dir::Right) => (i + 1).min(last),
        (ViewMode::Icons, Dir::Up) => {
            if i >= cols {
                i - cols
            } else {
                i
            }
        }
        (ViewMode::Icons, Dir::Down) => {
            if i + cols <= last {
                i + cols
            } else if i / cols < last / cols {
                // A short last row: land on its final item.
                last
            } else {
                i
            }
        }
    }
}

/// Items a page-up or page-down jumps over for a viewport of height `vh`.
pub fn page_items(mode: ViewMode, vw: i32, vh: i32) -> usize {
    let per_row = columns_of(mode, vw);
    let pitch = match mode {
        ViewMode::List => ROW_H,
        ViewMode::Icons => CELL_H,
    };
    (((vh / pitch) - 1).max(1) as usize) * per_row
}
