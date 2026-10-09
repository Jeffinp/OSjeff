//! The pixels of the terminal window as numbers: the tab strip, the character grid with the
//! mouse mapped onto it, the text selection (cells of the visible rows) and the prompt split.
//! Pure and host tested; the kernel draws and routes the mouse through these rectangles.

use crate::window::Rect;
use alloc::string::String;
use alloc::vec::Vec;

pub const TITLE_H: i32 = crate::window::TITLE_H;
/// Height of the strip under the title bar that holds the session's tab.
pub const STRIP_H: i32 = 34;
/// Left padding of the text and the room the overlay scrollbar keeps on the right.
pub const PAD: i32 = 14;
pub const BAR_W: i32 = 12;
/// Gap under the strip and above the window's bottom edge.
pub const TOP_PAD: i32 = 6;
pub const BOTTOM_PAD: i32 = 8;

/// The character cell of the monospace face: advance and line height.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Metrics {
    pub cw: i32,
    pub lh: i32,
}

/// Geometry of a terminal window.
#[derive(Clone, Copy, Debug)]
pub struct Grid {
    pub window: Rect,
    /// The strip with the tab, directly under the title bar.
    pub strip: Rect,
    /// Where row 0, column 0 sits.
    pub x: i32,
    pub y: i32,
    pub cols: usize,
    pub rows: usize,
    pub m: Metrics,
}

/// Lay out a window `r` for cells of `m`.
pub fn layout(r: Rect, m: Metrics) -> Grid {
    let strip = Rect::new(r.x, r.y + TITLE_H, r.w, STRIP_H.min((r.h - TITLE_H).max(0)));
    let y = strip.bottom() + TOP_PAD;
    let cols = ((r.w - PAD - BAR_W) / m.cw.max(1)).max(1) as usize;
    let rows = ((r.bottom() - BOTTOM_PAD - y) / m.lh.max(1)).max(1) as usize;
    Grid {
        window: r,
        strip,
        x: r.x + PAD,
        y,
        cols,
        rows,
        m,
    }
}

impl Grid {
    /// The cell under `(px, py)`, clamped to the grid.
    pub fn cell_at(&self, px: i32, py: i32) -> (usize, usize) {
        let row = ((py - self.y).max(0) / self.m.lh.max(1)) as usize;
        let col = ((px - self.x).max(0) / self.m.cw.max(1)) as usize;
        (row.min(self.rows - 1), col.min(self.cols - 1))
    }

    /// Whether `(px, py)` is over the text rows.
    pub fn in_text(&self, px: i32, py: i32) -> bool {
        px >= self.window.x
            && px < self.window.right()
            && py >= self.y
            && py < self.y + self.rows as i32 * self.m.lh
    }

    /// The rectangle of cell `(row, col)`.
    pub fn cell_rect(&self, row: usize, col: usize) -> Rect {
        Rect::new(
            self.x + col as i32 * self.m.cw,
            self.y + row as i32 * self.m.lh,
            self.m.cw,
            self.m.lh,
        )
    }

    /// The scrollbar's track: the right edge of the text rows.
    pub fn track(&self) -> Rect {
        Rect::new(
            self.window.right() - BAR_W,
            self.y,
            BAR_W,
            self.rows as i32 * self.m.lh,
        )
    }
}

/// A text selection over the cells of the visible rows: where the press happened and where the
/// pointer is now, as `(row, column)`. The end cell is included.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Selection {
    pub anchor: (usize, usize),
    pub head: (usize, usize),
}

impl Selection {
    pub fn new(at: (usize, usize)) -> Self {
        Self {
            anchor: at,
            head: at,
        }
    }

    /// The two ends, first in reading order first.
    pub fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    /// A single cell is not a selection (a click).
    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// The columns of row `row` (whose text has `len` characters) that are selected, as
    /// `(first, count)`; a row inside the selection is selected to its end plus one cell, so
    /// the line break shows.
    pub fn span(&self, row: usize, len: usize) -> Option<(usize, usize)> {
        if self.is_empty() {
            return None;
        }
        let ((r0, c0), (r1, c1)) = self.ordered();
        if row < r0 || row > r1 {
            return None;
        }
        let first = if row == r0 { c0 } else { 0 };
        let last = if row == r1 { c1 } else { len.max(first) };
        (last >= first).then_some((first, last - first + 1))
    }
}

/// The word (or run of one kind of character) around column `col` of `row`: `(first, last)`
/// columns, both included. A blank cell selects the run of blanks.
pub fn word_bounds(row: &str, col: usize) -> (usize, usize) {
    let chars: Vec<char> = row.chars().collect();
    if chars.is_empty() {
        return (0, 0);
    }
    let at = col.min(chars.len() - 1);
    let kind = |c: char| -> u8 {
        if c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | '~') {
            2
        } else if c.is_whitespace() {
            0
        } else {
            1
        }
    };
    let k = kind(chars[at]);
    let mut a = at;
    while a > 0 && kind(chars[a - 1]) == k {
        a -= 1;
    }
    let mut b = at;
    while b + 1 < chars.len() && kind(chars[b + 1]) == k {
        b += 1;
    }
    (a, b)
}

/// The text of `sel` over `rows`: each row from its first to its last selected cell, trailing
/// blanks cut, rows joined with line breaks.
pub fn extract(rows: &[String], sel: &Selection) -> String {
    let mut out = String::new();
    if sel.is_empty() {
        return out;
    }
    let ((r0, _), (r1, _)) = sel.ordered();
    for (r, row) in rows.iter().enumerate().take(r1 + 1).skip(r0) {
        let len = row.chars().count();
        let Some((first, n)) = sel.span(r, len) else {
            continue;
        };
        let piece: String = row.chars().skip(first).take(n).collect();
        if r > r0 {
            out.push('\n');
        }
        out.push_str(piece.trim_end());
    }
    out
}

/// Split a prompt such as `~/docs $ ` into its path part (with the blank before the symbol) and
/// the trailing symbol (`$ `, `# ` or `> `), for two colours.
pub fn split_prompt(prompt: &str) -> (&str, &str) {
    let t = prompt.trim_end();
    match t.chars().last() {
        Some('$' | '#' | '>') => {
            let cut = t.len() - 1;
            (&prompt[..cut], &prompt[cut..])
        }
        _ => (prompt, ""),
    }
}

/// The directory a prompt shows, for the tab: the prompt without its trailing symbol, or `~`
/// when it shows nothing more.
pub fn tab_label(prompt: &str) -> String {
    let (path, _) = split_prompt(prompt);
    let p = path.trim();
    if p.is_empty() {
        String::from("~")
    } else {
        String::from(p)
    }
}

#[cfg(test)]
mod tests;
