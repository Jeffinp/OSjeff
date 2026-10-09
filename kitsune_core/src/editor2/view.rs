//! Viewport: scrolling, soft wrap, row iteration for drawing, and mouse
//! hit-testing. Nothing here copies the document: [`VisibleRows`] borrows the
//! editor and yields [`Cell`]s lazily.

use super::Editor;
use super::buffer::{TextBuf, width};

/// Scroll state and window size.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub(crate) rows: usize,
    pub(crate) cols: usize,
    /// First visible logical line.
    pub(crate) top: usize,
    /// Wrapped sub-row of `top` (always 0 without soft wrap).
    pub(crate) top_sub: usize,
    /// First visible display column (always 0 with soft wrap).
    pub(crate) left: usize,
}

impl View {
    pub(crate) fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows: rows.max(1),
            cols: cols.max(1),
            top: 0,
            top_sub: 0,
            left: 0,
        }
    }

    pub(crate) fn reset_scroll(&mut self) {
        self.top = 0;
        self.top_sub = 0;
        self.left = 0;
    }

    pub(crate) fn reset_scroll_x(&mut self) {
        self.left = 0;
        self.top_sub = 0;
    }
}

/// One character cell to draw.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cell {
    /// The glyph: tabs are already expanded to spaces, control characters show
    /// as `·` and invalid UTF-8 bytes as U+FFFD.
    pub ch: char,
    pub selected: bool,
}

/// Lazily decodes the cells of one screen row.
pub struct Cells<'a> {
    text: &'a TextBuf,
    pos: usize,
    end: usize,
    dc: usize,
    end_dc: usize,
    tab: usize,
    pending: usize,
    tab_sel: bool,
    sel: (usize, usize),
}

impl Iterator for Cells<'_> {
    type Item = Cell;

    fn next(&mut self) -> Option<Cell> {
        if self.dc >= self.end_dc {
            return None;
        }
        if self.pending > 0 {
            self.pending -= 1;
            self.dc += 1;
            return Some(Cell {
                ch: ' ',
                selected: self.tab_sel,
            });
        }
        if self.pos >= self.end {
            return None;
        }
        let (c, n) = self.text.char_at(self.pos);
        let selected = self.pos >= self.sel.0 && self.pos < self.sel.1;
        self.pos += n.max(1);
        let w = width(c, self.dc, self.tab);
        self.dc += 1;
        if c == '\t' {
            self.pending = w - 1;
            self.tab_sel = selected;
            return Some(Cell { ch: ' ', selected });
        }
        let ch = if c.is_control() { '\u{B7}' } else { c };
        Some(Cell { ch, selected })
    }
}

/// One screen row of the text area.
pub struct RowView<'a> {
    ed: &'a Editor,
    /// 0-based logical line shown on this row.
    pub line: usize,
    /// True for the second and later screen rows of a wrapped line.
    pub continuation: bool,
    /// 1-based line number to print in the gutter (`None` on continuation
    /// rows or when line numbers are off).
    pub line_number: Option<usize>,
    start_dc: usize,
    end_dc: usize,
}

impl<'a> RowView<'a> {
    /// The characters of this row, left to right (at most `text_cols`).
    pub fn cells(&self) -> Cells<'a> {
        let t = &self.ed.text;
        let tab = self.ed.cfg.tab_width;
        let mut it = Cells {
            text: t,
            pos: t.line_start(self.line),
            end: t.line_end(self.line),
            dc: 0,
            end_dc: usize::MAX,
            tab,
            pending: 0,
            tab_sel: false,
            sel: self.ed.sel_range().unwrap_or((0, 0)),
        };
        for _ in 0..self.start_dc {
            if it.next().is_none() {
                break;
            }
        }
        it.end_dc = self.end_dc;
        it
    }
}

/// Iterator over the screen rows of the text area (at most `rows`, fewer when
/// the document ends first).
pub struct VisibleRows<'a> {
    ed: &'a Editor,
    line: usize,
    sub: usize,
    left: usize,
    remaining: usize,
    cached: Option<(usize, usize)>,
}

impl<'a> Iterator for VisibleRows<'a> {
    type Item = RowView<'a>;

    fn next(&mut self) -> Option<RowView<'a>> {
        if self.remaining == 0 || self.line >= self.ed.text.line_count() {
            return None;
        }
        self.remaining -= 1;
        let w = self.ed.text_cols();
        let wrap = self.ed.cfg.soft_wrap;
        let (start, end) = if wrap {
            (self.sub * w, (self.sub + 1) * w)
        } else {
            (self.left, self.left + w)
        };
        let row = RowView {
            ed: self.ed,
            line: self.line,
            continuation: self.sub > 0,
            line_number: (self.ed.cfg.line_numbers && self.sub == 0).then_some(self.line + 1),
            start_dc: start,
            end_dc: end,
        };
        // Advance: a line has `width / w + 1` rows; the width is computed
        // once per line and cached.
        let more = wrap && {
            let lw = match self.cached {
                Some((l, lw)) if l == self.line => lw,
                _ => {
                    let lw = self.ed.line_width(self.line);
                    self.cached = Some((self.line, lw));
                    lw
                }
            };
            self.sub < lw / w
        };
        if more {
            self.sub += 1;
        } else {
            self.line += 1;
            self.sub = 0;
        }
        Some(row)
    }
}

impl Editor {
    /// Resize the window (rows x columns, including the gutter). Zero is
    /// treated as one.
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.view.rows = rows.max(1);
        self.view.cols = cols.max(1);
        self.ensure_visible();
    }

    /// Window size as `(rows, cols)`.
    pub fn viewport(&self) -> (usize, usize) {
        (self.view.rows, self.view.cols)
    }

    /// Width of the line-number gutter in columns (0 when off).
    pub fn gutter_width(&self) -> usize {
        if !self.cfg.line_numbers {
            return 0;
        }
        let mut n = self.text.line_count();
        let mut digits = 1;
        while n >= 10 {
            n /= 10;
            digits += 1;
        }
        (digits + 1).min(self.view.cols.saturating_sub(1))
    }

    /// Columns available to text.
    pub fn text_cols(&self) -> usize {
        (self.view.cols - self.gutter_width()).max(1)
    }

    /// First visible logical line.
    pub fn top_line(&self) -> usize {
        self.view.top
    }

    /// First visible display column.
    pub fn left_col(&self) -> usize {
        self.view.left
    }

    pub(crate) fn line_width(&self, l: usize) -> usize {
        self.text.line_width(l, self.cfg.tab_width)
    }

    /// Screen rows taken by line `l` (1 without soft wrap).
    pub fn rows_of_line(&self, l: usize) -> usize {
        if self.cfg.soft_wrap {
            self.line_width(l) / self.text_cols() + 1
        } else {
            1
        }
    }

    /// The rows to draw, top to bottom.
    pub fn visible_rows(&self) -> VisibleRows<'_> {
        VisibleRows {
            ed: self,
            line: self.view.top.min(self.text.line_count() - 1),
            sub: self.view.top_sub,
            left: self.view.left,
            remaining: self.view.rows,
            cached: None,
        }
    }

    /// Visual row (line, sub-row) holding byte `pos`.
    fn visual_row_of(&self, pos: usize) -> (usize, usize) {
        let l = self.text.line_of(pos);
        if self.cfg.soft_wrap {
            let dc = self.text.dc_of(pos, self.cfg.tab_width);
            (l, dc / self.text_cols())
        } else {
            (l, 0)
        }
    }

    fn prev_row(&self, (l, s): (usize, usize)) -> Option<(usize, usize)> {
        if s > 0 {
            Some((l, s - 1))
        } else if l > 0 {
            Some((l - 1, self.rows_of_line(l - 1) - 1))
        } else {
            None
        }
    }

    fn next_row(&self, (l, s): (usize, usize)) -> Option<(usize, usize)> {
        if s + 1 < self.rows_of_line(l) {
            Some((l, s + 1))
        } else if l + 1 < self.text.line_count() {
            Some((l + 1, 0))
        } else {
            None
        }
    }

    /// Screen rows from `from` down to `to`, or `None` when farther than `cap`.
    fn row_distance(&self, from: (usize, usize), to: (usize, usize), cap: usize) -> Option<usize> {
        if to < from {
            return None;
        }
        let (mut l, mut s) = from;
        let mut dist = 0;
        while l < to.0 {
            dist += self.rows_of_line(l).saturating_sub(s);
            if dist > cap {
                return None;
            }
            l += 1;
            s = 0;
        }
        dist += to.1.saturating_sub(s);
        (dist <= cap).then_some(dist)
    }

    /// Scroll the minimum needed to show the cursor.
    pub fn ensure_visible(&mut self) {
        let rows = self.view.rows;
        let w = self.text_cols();
        let last = self.text.line_count() - 1;
        self.view.top = self.view.top.min(last);
        self.view.top_sub = if self.cfg.soft_wrap {
            self.view.top_sub.min(self.rows_of_line(self.view.top) - 1)
        } else {
            0
        };
        let (cl, cs) = self.visual_row_of(self.cursor);
        if self.cfg.soft_wrap {
            self.view.left = 0;
            let top = (self.view.top, self.view.top_sub);
            if (cl, cs) < top {
                self.view.top = cl;
                self.view.top_sub = cs;
            } else if self.row_distance(top, (cl, cs), rows - 1).is_none() {
                let mut r = (cl, cs);
                for _ in 0..rows - 1 {
                    match self.prev_row(r) {
                        Some(p) => r = p,
                        None => break,
                    }
                }
                self.view.top = r.0;
                self.view.top_sub = r.1;
            }
        } else {
            self.view.top_sub = 0;
            if cl < self.view.top {
                self.view.top = cl;
            } else if cl >= self.view.top + rows {
                self.view.top = cl + 1 - rows;
            }
            let dc = self.text.dc_of(self.cursor, self.cfg.tab_width);
            if dc < self.view.left {
                self.view.left = dc;
            } else if dc >= self.view.left + w {
                self.view.left = dc + 1 - w;
            }
        }
    }

    /// Scroll so the cursor row sits mid-window.
    pub fn center_cursor(&mut self) {
        let rows = self.view.rows;
        let mut r = self.visual_row_of(self.cursor);
        for _ in 0..rows / 2 {
            match self.prev_row(r) {
                Some(p) => r = p,
                None => break,
            }
        }
        self.view.top = r.0;
        self.view.top_sub = if self.cfg.soft_wrap { r.1 } else { 0 };
        let w = self.text_cols();
        if !self.cfg.soft_wrap {
            let dc = self.text.dc_of(self.cursor, self.cfg.tab_width);
            if dc < self.view.left {
                self.view.left = dc;
            } else if dc >= self.view.left + w {
                self.view.left = dc + 1 - w;
            }
        }
    }

    /// Scroll the window by `delta` screen rows without moving the cursor
    /// (mouse wheel). Clamped to the document.
    pub fn scroll_by(&mut self, delta: isize) {
        let mut r = (self.view.top, self.view.top_sub);
        for _ in 0..delta.unsigned_abs() {
            let n = if delta < 0 {
                self.prev_row(r)
            } else {
                self.next_row(r)
            };
            match n {
                Some(n) => r = n,
                None => break,
            }
        }
        self.view.top = r.0;
        self.view.top_sub = r.1;
    }

    /// Scroll horizontally by `delta` columns (no effect with soft wrap).
    pub fn scroll_x_by(&mut self, delta: isize) {
        if self.cfg.soft_wrap {
            return;
        }
        let max = (0..self.text.line_count())
            .take(4096)
            .map(|l| self.line_width(l))
            .max()
            .unwrap_or(0);
        let nl = (self.view.left as isize + delta).clamp(0, max as isize);
        self.view.left = nl as usize;
    }

    /// Cursor position in window coordinates `(row, col)`, or `None` when it
    /// is scrolled out of view.
    pub fn cursor_screen(&self) -> Option<(usize, usize)> {
        let gutter = self.gutter_width();
        let w = self.text_cols();
        let dc = self.text.dc_of(self.cursor, self.cfg.tab_width);
        let (cl, cs) = self.visual_row_of(self.cursor);
        let top = (self.view.top, self.view.top_sub);
        let row = self.row_distance(top, (cl, cs), self.view.rows - 1)?;
        if self.cfg.soft_wrap {
            Some((row, gutter + dc % w))
        } else if dc >= self.view.left && dc < self.view.left + w {
            Some((row, gutter + dc - self.view.left))
        } else {
            None
        }
    }

    /// Byte offset under window coordinates `(row, col)`. Clicks past the end
    /// of a line land at its end; below the text, at the end of the document.
    pub fn pos_at_screen(&self, row: usize, col: usize) -> usize {
        let gutter = self.gutter_width();
        let w = self.text_cols();
        let x = col.saturating_sub(gutter).min(w - 1);
        let mut r = (
            self.view.top.min(self.text.line_count() - 1),
            self.view.top_sub,
        );
        for _ in 0..row {
            match self.next_row(r) {
                Some(n) => r = n,
                None => return self.text.len(),
            }
        }
        let target = if self.cfg.soft_wrap {
            r.1 * w + x
        } else {
            self.view.left + x
        };
        self.text.pos_at_dc(r.0, target, self.cfg.tab_width)
    }

    /// Where Up/Down (`dir`) would put the cursor. At the first/last row it
    /// goes to the start/end of the document.
    pub(crate) fn vertical_target(&self, dir: i32) -> usize {
        let tab = self.cfg.tab_width;
        let w = self.text_cols();
        let cur = self.visual_row_of(self.cursor);
        let next = if dir < 0 {
            self.prev_row(cur)
        } else {
            self.next_row(cur)
        };
        match next {
            None => {
                if dir < 0 {
                    0
                } else {
                    self.text.len()
                }
            }
            Some((l, s)) => {
                let target = if self.cfg.soft_wrap {
                    s * w + self.pref_dc % w
                } else {
                    self.pref_dc
                };
                self.text.pos_at_dc(l, target, tab)
            }
        }
    }

    // ---- mouse -----------------------------------------------------------

    /// Mouse press at window coordinates. `clicks` is 1, 2 (word) or 3 (line);
    /// `shift` extends the selection.
    pub fn mouse_down(&mut self, row: usize, col: usize, clicks: u8, shift: bool) {
        let p = self.pos_at_screen(row, col);
        match clicks {
            0 | 1 => self.move_to(p, shift, false),
            2 => self.select_word_at(p),
            _ => self.select_line_at(p),
        }
    }

    /// Mouse moved with the button held: extend the selection.
    pub fn mouse_drag(&mut self, row: usize, col: usize) {
        let p = self.pos_at_screen(row, col);
        self.move_to(p, true, false);
    }
}
