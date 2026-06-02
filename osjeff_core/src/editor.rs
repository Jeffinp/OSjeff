//! Multi-line text editor model. Fixed-capacity grid, 2D cursor, no allocation.
//!
//! Pure logic only: insertion, line splitting/joining, cursor navigation. The
//! kernel renders the grid and draws the caret.

use crate::keymap::Key;

/// Columns per line.
pub const COLS: usize = 44;
/// Maximum number of lines.
pub const ROWS: usize = 18;

pub struct Editor {
    text: [[u8; COLS]; ROWS],
    len: [usize; ROWS],
    rows: usize,
    cx: usize,
    cy: usize,
    dirty: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    pub fn new() -> Self {
        Self {
            text: [[0; COLS]; ROWS],
            len: [0; ROWS],
            rows: 1,
            cx: 0,
            cy: 0,
            dirty: false,
        }
    }

    /// Number of lines in use (always `>= 1`).
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Borrow line `i` (`0..rows`).
    pub fn line(&self, i: usize) -> &[u8] {
        &self.text[i][..self.len[i]]
    }

    /// Cursor as `(col, row)`.
    pub fn cursor(&self) -> (usize, usize) {
        (self.cx, self.cy)
    }

    /// Whether the buffer changed since creation.
    pub fn dirty(&self) -> bool {
        self.dirty
    }

    /// Dispatch a key to the matching edit operation.
    pub fn on_key(&mut self, key: Key) {
        match key {
            Key::Char(c) => self.insert(c),
            Key::Enter => self.newline(),
            Key::Backspace => self.backspace(),
            Key::Delete => self.delete(),
            Key::Left => self.move_left(),
            Key::Right => self.move_right(),
            Key::Up => self.move_up(),
            Key::Down => self.move_down(),
            Key::Home => self.cx = 0,
            Key::End => self.cx = self.len[self.cy],
            Key::Tab => {
                for _ in 0..2 {
                    self.insert(b' ');
                }
            }
            Key::Esc => {}
        }
    }

    fn insert(&mut self, ch: u8) {
        let l = self.cy;
        let n = self.len[l];
        if n >= COLS {
            return; // line full; ignore (no auto-wrap)
        }
        let mut i = n;
        while i > self.cx {
            self.text[l][i] = self.text[l][i - 1];
            i -= 1;
        }
        self.text[l][self.cx] = ch;
        self.len[l] = n + 1;
        self.cx += 1;
        self.dirty = true;
    }

    fn newline(&mut self) {
        if self.rows >= ROWS {
            return;
        }
        let l = self.cy;
        let n = self.len[l];
        let cx = self.cx;

        // Shift lines below down by one to open a slot at l+1.
        let mut i = self.rows;
        while i > l + 1 {
            self.text[i] = self.text[i - 1];
            self.len[i] = self.len[i - 1];
            i -= 1;
        }

        // Move the tail (cx..n) of the current line onto the new line.
        let tail = n - cx;
        let mut new_line = [0u8; COLS];
        new_line[..tail].copy_from_slice(&self.text[l][cx..n]);
        self.text[l + 1] = new_line;
        self.len[l + 1] = tail;
        self.len[l] = cx;

        self.rows += 1;
        self.cy = l + 1;
        self.cx = 0;
        self.dirty = true;
    }

    fn backspace(&mut self) {
        if self.cx > 0 {
            let l = self.cy;
            for i in (self.cx - 1)..(self.len[l] - 1) {
                self.text[l][i] = self.text[l][i + 1];
            }
            self.len[l] -= 1;
            self.cx -= 1;
            self.dirty = true;
        } else if self.cy > 0 {
            let prev = self.cy - 1;
            let join_at = self.len[prev];
            self.join_into(prev, self.cy);
            self.cy = prev;
            self.cx = join_at;
        }
    }

    fn delete(&mut self) {
        let l = self.cy;
        if self.cx < self.len[l] {
            for i in self.cx..(self.len[l] - 1) {
                self.text[l][i] = self.text[l][i + 1];
            }
            self.len[l] -= 1;
            self.dirty = true;
        } else if self.cy + 1 < self.rows {
            self.join_into(l, l + 1);
        }
    }

    /// Append line `src` onto the end of line `dst` (capped at COLS), then
    /// remove `src` by shifting the rows above it up.
    fn join_into(&mut self, dst: usize, src: usize) {
        let dlen = self.len[dst];
        let slen = self.len[src];
        let capacity = COLS - dlen;
        let copy = slen.min(capacity);
        for j in 0..copy {
            self.text[dst][dlen + j] = self.text[src][j];
        }
        self.len[dst] = dlen + copy;

        let mut i = src;
        while i + 1 < self.rows {
            self.text[i] = self.text[i + 1];
            self.len[i] = self.len[i + 1];
            i += 1;
        }
        self.rows -= 1;
        self.dirty = true;
    }

    fn move_left(&mut self) {
        if self.cx > 0 {
            self.cx -= 1;
        } else if self.cy > 0 {
            self.cy -= 1;
            self.cx = self.len[self.cy];
        }
    }

    fn move_right(&mut self) {
        if self.cx < self.len[self.cy] {
            self.cx += 1;
        } else if self.cy + 1 < self.rows {
            self.cy += 1;
            self.cx = 0;
        }
    }

    fn move_up(&mut self) {
        if self.cy > 0 {
            self.cy -= 1;
            self.cx = self.cx.min(self.len[self.cy]);
        }
    }

    fn move_down(&mut self) {
        if self.cy + 1 < self.rows {
            self.cy += 1;
            self.cx = self.cx.min(self.len[self.cy]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_str(e: &mut Editor, s: &str) {
        for &b in s.as_bytes() {
            e.on_key(Key::Char(b));
        }
    }

    #[test]
    fn starts_with_one_empty_line() {
        let e = Editor::new();
        assert_eq!(e.rows(), 1);
        assert_eq!(e.line(0), b"");
        assert_eq!(e.cursor(), (0, 0));
        assert!(!e.dirty());
    }

    #[test]
    fn insert_advances_cursor_and_marks_dirty() {
        let mut e = Editor::new();
        type_str(&mut e, "hello");
        assert_eq!(e.line(0), b"hello");
        assert_eq!(e.cursor(), (5, 0));
        assert!(e.dirty());
    }

    #[test]
    fn insert_in_middle() {
        let mut e = Editor::new();
        type_str(&mut e, "ac");
        e.on_key(Key::Left);
        e.on_key(Key::Char(b'b'));
        assert_eq!(e.line(0), b"abc");
        assert_eq!(e.cursor(), (2, 0));
    }

    #[test]
    fn line_full_blocks_insert() {
        let mut e = Editor::new();
        for _ in 0..(COLS + 5) {
            e.on_key(Key::Char(b'x'));
        }
        assert_eq!(e.line(0).len(), COLS);
    }

    #[test]
    fn newline_splits_line() {
        let mut e = Editor::new();
        type_str(&mut e, "abcdef");
        e.on_key(Key::Home);
        e.on_key(Key::Right);
        e.on_key(Key::Right);
        e.on_key(Key::Right); // caret after "abc"
        e.on_key(Key::Enter);
        assert_eq!(e.rows(), 2);
        assert_eq!(e.line(0), b"abc");
        assert_eq!(e.line(1), b"def");
        assert_eq!(e.cursor(), (0, 1));
    }

    #[test]
    fn newline_at_end_creates_empty_line() {
        let mut e = Editor::new();
        type_str(&mut e, "hi");
        e.on_key(Key::Enter);
        assert_eq!(e.rows(), 2);
        assert_eq!(e.line(1), b"");
        assert_eq!(e.cursor(), (0, 1));
    }

    #[test]
    fn newline_respects_row_cap() {
        let mut e = Editor::new();
        for _ in 0..(ROWS + 3) {
            e.on_key(Key::Enter);
        }
        assert_eq!(e.rows(), ROWS);
    }

    #[test]
    fn backspace_within_line() {
        let mut e = Editor::new();
        type_str(&mut e, "abc");
        e.on_key(Key::Backspace);
        assert_eq!(e.line(0), b"ab");
        assert_eq!(e.cursor(), (2, 0));
    }

    #[test]
    fn backspace_at_line_start_joins_previous() {
        let mut e = Editor::new();
        type_str(&mut e, "ab");
        e.on_key(Key::Enter);
        type_str(&mut e, "cd");
        e.on_key(Key::Home); // caret at start of line 1
        e.on_key(Key::Backspace);
        assert_eq!(e.rows(), 1);
        assert_eq!(e.line(0), b"abcd");
        assert_eq!(e.cursor(), (2, 0));
    }

    #[test]
    fn backspace_at_origin_is_noop() {
        let mut e = Editor::new();
        e.on_key(Key::Backspace);
        assert_eq!(e.rows(), 1);
        assert_eq!(e.cursor(), (0, 0));
    }

    #[test]
    fn delete_within_line() {
        let mut e = Editor::new();
        type_str(&mut e, "abc");
        e.on_key(Key::Home);
        e.on_key(Key::Delete);
        assert_eq!(e.line(0), b"bc");
        assert_eq!(e.cursor(), (0, 0));
    }

    #[test]
    fn delete_at_line_end_joins_next() {
        let mut e = Editor::new();
        type_str(&mut e, "ab");
        e.on_key(Key::Enter);
        type_str(&mut e, "cd");
        e.on_key(Key::Up);
        e.on_key(Key::End); // end of line 0
        e.on_key(Key::Delete);
        assert_eq!(e.rows(), 1);
        assert_eq!(e.line(0), b"abcd");
    }

    #[test]
    fn delete_at_buffer_end_is_noop() {
        let mut e = Editor::new();
        type_str(&mut e, "x");
        e.on_key(Key::Delete); // caret already at end, last line
        assert_eq!(e.line(0), b"x");
        assert_eq!(e.rows(), 1);
    }

    #[test]
    fn left_wraps_to_previous_line_end() {
        let mut e = Editor::new();
        type_str(&mut e, "ab");
        e.on_key(Key::Enter);
        type_str(&mut e, "cd");
        e.on_key(Key::Home);
        e.on_key(Key::Left);
        assert_eq!(e.cursor(), (2, 0));
    }

    #[test]
    fn right_wraps_to_next_line_start() {
        let mut e = Editor::new();
        type_str(&mut e, "ab");
        e.on_key(Key::Enter);
        type_str(&mut e, "cd");
        e.on_key(Key::Up);
        e.on_key(Key::End); // (2,0)
        e.on_key(Key::Right);
        assert_eq!(e.cursor(), (0, 1));
    }

    #[test]
    fn up_down_clamp_column() {
        let mut e = Editor::new();
        type_str(&mut e, "longline");
        e.on_key(Key::Enter);
        type_str(&mut e, "x"); // line1 = "x"
        e.on_key(Key::End); // (1,1)
        e.on_key(Key::Up); // to line0, clamp col to ... line0 len 8, col was 1
        assert_eq!(e.cursor(), (1, 0));
    }

    #[test]
    fn down_clamps_to_shorter_line() {
        let mut e = Editor::new();
        type_str(&mut e, "abcdef");
        e.on_key(Key::Enter);
        type_str(&mut e, "xy");
        e.on_key(Key::Up);
        e.on_key(Key::End); // (6,0)
        e.on_key(Key::Down); // clamp to len 2
        assert_eq!(e.cursor(), (2, 1));
    }

    #[test]
    fn up_at_top_and_down_at_bottom_noop() {
        let mut e = Editor::new();
        e.on_key(Key::Up);
        assert_eq!(e.cursor(), (0, 0));
        e.on_key(Key::Down);
        assert_eq!(e.cursor(), (0, 0));
    }

    #[test]
    fn tab_inserts_two_spaces() {
        let mut e = Editor::new();
        e.on_key(Key::Tab);
        assert_eq!(e.line(0), b"  ");
        assert_eq!(e.cursor(), (2, 0));
    }

    #[test]
    fn esc_is_noop() {
        let mut e = Editor::new();
        e.on_key(Key::Esc);
        assert_eq!(e.cursor(), (0, 0));
        assert!(!e.dirty());
    }

    #[test]
    fn join_caps_at_cols() {
        // Build line0 near full, line1 with content; backspace-join must cap.
        let mut e = Editor::new();
        for _ in 0..COLS {
            e.on_key(Key::Char(b'a'));
        }
        e.on_key(Key::Enter); // row cap fine
        type_str(&mut e, "bb");
        e.on_key(Key::Home);
        e.on_key(Key::Backspace); // join line1 into full line0
        assert_eq!(e.line(0).len(), COLS); // capped, not overflowed
    }
}
