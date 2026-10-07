//! The terminal's scrollback: lines of text with bounded memory, soft wrapping
//! to the window width and a scroll offset, all independent of any pixels.
//!
//! [`Screen`] keeps *logical* lines (what the commands printed); the window
//! width only matters when a [`View`] is built, so resizing the window re-wraps
//! the whole history for free. Memory is bounded three ways: lines kept
//! ([`MAX_LINES`]), characters kept ([`MAX_CHARS`]) and the length of one
//! logical line ([`MAX_LINE_CHARS`], longer text continues on a new line), so a
//! `yes` or a binary file cannot grow the heap without limit.
//!
//! [`Screen::print`] understands what a terminal receives from a plain text
//! command: UTF-8 (invalid bytes show as U+FFFD), `\n` and `\r\n` as line
//! ends, tabs to the next multiple of 8, backspace, and it swallows ANSI
//! escape sequences instead of showing their bytes.

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

/// Most logical lines kept.
pub const MAX_LINES: usize = 5000;
/// Most characters kept in all lines together.
pub const MAX_CHARS: usize = 1 << 20;
/// Longest logical line; more text goes on a following line.
pub const MAX_LINE_CHARS: usize = 2048;
/// Tab stop width.
pub const TAB: usize = 8;

/// What to draw: the visible rows (oldest first) and where the caret goes.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct View {
    /// At most `rows` rows of at most `cols` characters each.
    pub rows: Vec<String>,
    /// `(row, column)` of the caret inside `rows`, when the live line is visible.
    pub cursor: Option<(usize, usize)>,
    /// Wrapped rows hidden above the window (scrollback left to read).
    pub above: usize,
    /// Wrapped rows hidden below the window (zero when at the bottom).
    pub below: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Esc {
    None,
    /// Saw ESC.
    Start,
    /// Inside `ESC [` ... until a final byte `@..~`.
    Csi,
    /// Inside `ESC ]` (OSC) until BEL or `ESC \`.
    Osc,
}

/// Scrollback of a terminal.
#[derive(Clone, Debug)]
pub struct Screen {
    /// Finished lines.
    lines: VecDeque<Vec<char>>,
    /// The line being written (no `\n` yet).
    cur: Vec<char>,
    chars: usize,
    /// Rows scrolled up from the bottom (0 = following the newest output).
    offset: usize,
    esc: Esc,
    /// Bytes of an unfinished UTF-8 sequence carried to the next `print`.
    pending: Vec<u8>,
    /// Last `\r` seen without a `\n` right after it.
    cr: bool,
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}

/// Wrapped rows a line of `len` characters takes in `cols` columns (at least 1).
fn rows_of(len: usize, cols: usize) -> usize {
    len.div_ceil(cols.max(1)).max(1)
}

impl Screen {
    pub fn new() -> Self {
        Self {
            lines: VecDeque::new(),
            cur: Vec::new(),
            chars: 0,
            offset: 0,
            esc: Esc::None,
            pending: Vec::new(),
            cr: false,
        }
    }

    /// Forget everything (the `clear` command, Ctrl+L).
    pub fn clear(&mut self) {
        self.lines.clear();
        self.cur.clear();
        self.chars = 0;
        self.offset = 0;
        self.esc = Esc::None;
        self.pending.clear();
        self.cr = false;
    }

    /// Finished lines plus the partial one if it has text.
    pub fn line_count(&self) -> usize {
        self.lines.len() + usize::from(!self.cur.is_empty())
    }

    /// Characters held (for tests and the memory bound).
    pub fn char_count(&self) -> usize {
        self.chars + self.cur.len()
    }

    fn finish_line(&mut self) {
        let line = core::mem::take(&mut self.cur);
        self.chars += line.len();
        self.lines.push_back(line);
        while self.lines.len() > MAX_LINES || self.chars > MAX_CHARS {
            match self.lines.pop_front() {
                Some(l) => self.chars -= l.len(),
                None => break,
            }
        }
    }

    fn put(&mut self, c: char) {
        if self.cur.len() >= MAX_LINE_CHARS {
            self.finish_line();
        }
        self.cur.push(c);
    }

    /// Append program output. Never panics, whatever the bytes.
    pub fn print(&mut self, bytes: &[u8]) {
        let mut data: Vec<u8>;
        let input: &[u8] = if self.pending.is_empty() {
            bytes
        } else {
            data = core::mem::take(&mut self.pending);
            data.extend_from_slice(bytes);
            &data
        };
        let mut i = 0;
        while i < input.len() {
            let b = input[i];
            if b < 0x80 {
                i += 1;
                self.ascii(b);
                continue;
            }
            // A multi-byte sequence: decode one character.
            let need = match b {
                0xC2..=0xDF => 2,
                0xE0..=0xEF => 3,
                0xF0..=0xF4 => 4,
                _ => 0,
            };
            if need == 0 {
                i += 1;
                self.glyph('\u{FFFD}');
                continue;
            }
            let end = (i + need).min(input.len());
            match core::str::from_utf8(&input[i..end]) {
                Ok(s) if end - i == need => {
                    for c in s.chars() {
                        self.glyph(c);
                    }
                    i = end;
                }
                Err(e) if e.error_len().is_none() => {
                    // Cut by the end of this chunk: finish it with the next one.
                    self.pending = input[i..].to_vec();
                    return;
                }
                Err(e) => {
                    // One U+FFFD for the whole maximal invalid prefix, as std does.
                    i += e.error_len().unwrap_or(1).max(1);
                    self.glyph('\u{FFFD}');
                }
                Ok(_) => {
                    i += 1;
                    self.glyph('\u{FFFD}');
                }
            }
        }
    }

    /// One non-ASCII character (never part of an escape sequence's control bytes).
    fn glyph(&mut self, c: char) {
        match self.esc {
            Esc::None => {
                self.cr = false;
                self.put(c);
            }
            // Inside a sequence everything up to its end is swallowed.
            Esc::Start => self.esc = Esc::None,
            Esc::Csi | Esc::Osc => {}
        }
    }

    fn ascii(&mut self, b: u8) {
        match self.esc {
            Esc::Start => {
                self.esc = match b {
                    b'[' => Esc::Csi,
                    b']' => Esc::Osc,
                    _ => Esc::None,
                };
                return;
            }
            Esc::Csi => {
                if (0x40..=0x7E).contains(&b) {
                    self.esc = Esc::None;
                }
                return;
            }
            Esc::Osc => {
                if b == 0x07 || b == b'\\' {
                    self.esc = Esc::None;
                }
                return;
            }
            Esc::None => {}
        }
        match b {
            0x1B => self.esc = Esc::Start,
            b'\n' => {
                self.cr = false;
                self.finish_line();
            }
            b'\r' => self.cr = true,
            b'\t' => {
                self.cr = false;
                let n = TAB - self.cur.len() % TAB;
                for _ in 0..n {
                    self.put(' ');
                }
            }
            0x08 => {
                self.cur.pop();
            }
            0x20..=0x7E => {
                if self.cr {
                    // A lone carriage return: the new text replaces the line (a
                    // progress bar redraws itself this way).
                    self.cur.clear();
                    self.cr = false;
                }
                self.put(char::from(b));
            }
            // Other control bytes (NUL, BEL, DEL...) show nothing.
            _ => {}
        }
    }

    /// Make sure the next text starts on a fresh line.
    pub fn ensure_newline(&mut self) {
        if !self.cur.is_empty() {
            self.finish_line();
        }
        self.cr = false;
    }

    // ---- scrolling ------------------------------------------------------------

    /// Total wrapped rows of the history plus a live line of `live_len` characters
    /// (caret included).
    fn total_rows(&self, cols: usize, live_len: usize) -> usize {
        let mut t = self
            .lines
            .iter()
            .map(|l| rows_of(l.len(), cols))
            .sum::<usize>();
        if !self.cur.is_empty() {
            t += rows_of(self.cur.len(), cols);
        }
        t + rows_of(live_len, cols)
    }

    /// Scroll back by `delta` rows (positive = towards older output), within the
    /// history for a window of `cols` x `rows` and a live line of `live_len`.
    pub fn scroll(&mut self, delta: isize, cols: usize, rows: usize, live_len: usize) {
        let total = self.total_rows(cols, live_len);
        let max = total.saturating_sub(rows.max(1));
        let off = self.offset.min(max);
        self.offset = if delta >= 0 {
            off.saturating_add(delta as usize).min(max)
        } else {
            off.saturating_sub(delta.unsigned_abs())
        };
    }

    /// Follow the newest output again.
    pub fn to_bottom(&mut self) {
        self.offset = 0;
    }

    /// Is the window showing older output?
    pub fn is_scrolled(&self) -> bool {
        self.offset > 0
    }

    // ---- drawing ----------------------------------------------------------------

    /// The rows to draw in a window of `cols` x `rows` characters. `live` is the
    /// line being edited (prompt included) and `live_cursor` the caret position
    /// in characters; it is always the last line.
    pub fn view(&self, cols: usize, rows: usize, live: &str, live_cursor: usize) -> View {
        self.build(cols, rows, Some((live, live_cursor)))
    }

    /// Like [`Screen::view`] with no live line (a command is running).
    pub fn view_history(&self, cols: usize, rows: usize) -> View {
        self.build(cols, rows, None)
    }

    fn build(&self, cols: usize, rows: usize, live: Option<(&str, usize)>) -> View {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let has_live = live.is_some();
        let (live, live_cursor) = live.unwrap_or(("", 0));
        let live: Vec<char> = live.chars().take(MAX_LINE_CHARS * 4).collect();
        let caret = live_cursor.min(live.len());
        let live_rows = if has_live {
            rows_of(live.len().max(caret + 1), cols)
        } else {
            0
        };
        let hist_rows = self.total_rows(cols, 0) - rows_of(0, cols);
        let total = hist_rows + live_rows;
        let max = total.saturating_sub(rows);
        let offset = self.offset.min(max);
        // Row range [first, first + rows) of the whole wrapped text.
        let first = total - rows.min(total) - offset;
        let end = first + rows.min(total);
        let mut out: Vec<String> = Vec::with_capacity(rows);
        let mut cursor = None;
        let mut row0 = 0usize; // wrapped-row index of the line being visited
        let partial = (!self.cur.is_empty()).then_some(self.cur.as_slice());
        let hist = self.lines.iter().map(Vec::as_slice).chain(partial);
        for line in hist {
            let n = rows_of(line.len(), cols);
            if row0 + n > first && row0 < end {
                for k in 0..n {
                    let r = row0 + k;
                    if r >= first && r < end {
                        let a = k * cols;
                        let b = (a + cols).min(line.len());
                        out.push(line.get(a..b).unwrap_or(&[]).iter().collect());
                    }
                }
            }
            row0 += n;
            if row0 >= end {
                break;
            }
        }
        if row0 < end {
            // The live line.
            for k in 0..live_rows {
                let r = hist_rows + k;
                if r >= first && r < end {
                    let a = k * cols;
                    let b = (a + cols).min(live.len());
                    out.push(live.get(a..b).unwrap_or(&[]).iter().collect());
                    if caret / cols == k {
                        cursor = Some((out.len() - 1, caret % cols));
                    }
                }
            }
        }
        View {
            rows: out,
            cursor,
            above: first,
            below: total - end,
        }
    }
}

/// `names` laid out in columns for a window `cols` wide, like `ls` does: column
/// major, two spaces between columns, no trailing spaces. Empty for no names.
pub fn columnize(names: &[String], cols: usize) -> String {
    if names.is_empty() {
        return String::new();
    }
    let widths: Vec<usize> = names.iter().map(|n| n.chars().count()).collect();
    let longest = widths.iter().copied().max().unwrap_or(0);
    let cell = longest + 2;
    let per_row = (cols.max(1) / cell).max(1);
    let nrows = names.len().div_ceil(per_row);
    let mut out = String::new();
    for r in 0..nrows {
        let mut line = String::new();
        for c in 0..per_row {
            let Some(name) = names.get(c * nrows + r) else {
                break;
            };
            line.push_str(name);
            let last = names.get((c + 1) * nrows + r).is_none();
            if !last {
                for _ in widths[c * nrows + r]..cell {
                    line.push(' ');
                }
            }
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;
    use alloc::vec;

    fn rows(s: &Screen, cols: usize, h: usize, live: &str) -> Vec<String> {
        s.view(cols, h, live, live.chars().count()).rows
    }

    #[test]
    fn lines_and_the_live_line() {
        let mut s = Screen::new();
        s.print(b"one\ntwo\n");
        assert_eq!(rows(&s, 20, 10, "$ ls"), ["one", "two", "$ ls"]);
        let v = s.view(20, 10, "$ ls", 4);
        assert_eq!(v.cursor, Some((2, 4)));
        assert_eq!((v.above, v.below), (0, 0));
    }

    #[test]
    fn a_partial_line_shows_before_the_live_line() {
        let mut s = Screen::new();
        s.print(b"abc");
        assert_eq!(rows(&s, 20, 10, "$ "), ["abc", "$ "]);
        s.print(b"def\n");
        assert_eq!(rows(&s, 20, 10, "$ "), ["abcdef", "$ "]);
        s.print(b"x");
        s.ensure_newline();
        s.ensure_newline();
        assert_eq!(s.line_count(), 2);
    }

    #[test]
    fn long_lines_wrap_and_rewrap_on_resize() {
        let mut s = Screen::new();
        s.print(b"abcdefghij\n");
        assert_eq!(rows(&s, 4, 10, "$"), ["abcd", "efgh", "ij", "$"]);
        assert_eq!(rows(&s, 5, 10, "$"), ["abcde", "fghij", "$"]);
        assert_eq!(rows(&s, 100, 10, "$"), ["abcdefghij", "$"]);
    }

    #[test]
    fn the_window_shows_the_newest_rows_and_scrolls() {
        let mut s = Screen::new();
        for i in 0..20 {
            s.print(format!("line {i}\n").as_bytes());
        }
        let v = s.view(20, 5, "$", 1);
        assert_eq!(v.rows, ["line 16", "line 17", "line 18", "line 19", "$"]);
        assert_eq!((v.above, v.below), (16, 0));
    }

    #[test]
    fn scrolling_moves_by_rows_and_clamps() {
        let mut s = Screen::new();
        for i in 0..20 {
            s.print(format!("l{i}\n").as_bytes());
        }
        // 20 lines + the live one = 21 rows; the window holds 5.
        assert_eq!(rows(&s, 20, 5, "$"), ["l16", "l17", "l18", "l19", "$"]);
        s.scroll(3, 20, 5, 1);
        assert!(s.is_scrolled());
        let v = s.view(20, 5, "$", 1);
        assert_eq!(v.rows, ["l13", "l14", "l15", "l16", "l17"]);
        assert_eq!((v.above, v.below), (13, 3));
        assert_eq!(v.cursor, None, "the live line is below the window");
        s.scroll(1000, 20, 5, 1);
        let v = s.view(20, 5, "$", 1);
        assert_eq!(v.rows[0], "l0");
        assert_eq!(v.above, 0);
        s.scroll(-2, 20, 5, 1);
        assert_eq!(s.view(20, 5, "$", 1).rows[0], "l2");
        s.scroll(-1000, 20, 5, 1);
        assert!(!s.is_scrolled());
        s.scroll(4, 20, 5, 1);
        s.to_bottom();
        assert_eq!(rows(&s, 20, 5, "$")[4], "$");
    }

    #[test]
    fn a_short_history_is_not_padded() {
        let mut s = Screen::new();
        s.print(b"a\n");
        let v = s.view(10, 8, "$", 1);
        assert_eq!(v.rows, ["a", "$"]);
        s.scroll(5, 10, 8, 1);
        assert_eq!(
            s.view(10, 8, "$", 1).rows,
            ["a", "$"],
            "nothing to scroll to"
        );
    }

    #[test]
    fn the_caret_wraps_with_the_live_line() {
        let s = Screen::new();
        let v = s.view(4, 5, "abcdefg", 7);
        assert_eq!(v.rows, ["abcd", "efg"]);
        assert_eq!(v.cursor, Some((1, 3)));
        // A caret at the very end of a full row sits on a new, empty row.
        let v = s.view(4, 5, "abcd", 4);
        assert_eq!(v.rows, ["abcd", ""]);
        assert_eq!(v.cursor, Some((1, 0)));
        // In the middle of the text.
        let v = s.view(4, 5, "abcdefg", 5);
        assert_eq!(v.cursor, Some((1, 1)));
    }

    #[test]
    fn crlf_tabs_backspace_and_controls() {
        let mut s = Screen::new();
        s.print(b"a\r\nb\tc\x08d\x07\x00e\n");
        assert_eq!(rows(&s, 40, 5, ""), ["a", "b       de", ""]);
        // A lone CR rewrites the line (progress bars).
        let mut s = Screen::new();
        s.print(b"10%\r50%\r100%\n");
        assert_eq!(rows(&s, 40, 5, "")[0], "100%");
    }

    #[test]
    fn escape_sequences_are_swallowed() {
        let mut s = Screen::new();
        s.print(b"\x1b[31mred\x1b[0m \x1b]0;title\x07ok\x1b[2");
        s.print(b"Jx\n");
        assert_eq!(rows(&s, 40, 5, "")[0], "red okx");
        // A lone ESC and an unknown two-byte sequence.
        let mut s = Screen::new();
        s.print(b"a\x1bcb\x1b");
        assert_eq!(rows(&s, 40, 5, "")[0], "ab");
    }

    #[test]
    fn utf8_is_decoded_even_when_split_between_prints() {
        let mut s = Screen::new();
        let bytes = "ação €".as_bytes();
        for chunk in bytes.chunks(1) {
            s.print(chunk);
        }
        s.print(b"\n");
        assert_eq!(rows(&s, 40, 5, "")[0], "ação €");
        // Invalid bytes become U+FFFD and never stall the decoder.
        let mut s = Screen::new();
        s.print(b"a\xffb\xc3(c\xe2\x82");
        s.print(b"d\n");
        assert_eq!(rows(&s, 40, 5, "")[0], "a\u{FFFD}b\u{FFFD}(c\u{FFFD}d");
    }

    #[test]
    fn memory_is_bounded_by_lines_chars_and_line_length() {
        let mut s = Screen::new();
        for i in 0..(MAX_LINES + 100) {
            s.print(format!("n{i}\n").as_bytes());
        }
        assert_eq!(s.line_count(), MAX_LINES);
        assert_eq!(rows(&s, 40, 3, "")[0], format!("n{}", MAX_LINES + 98));
        // One enormous line is cut into pieces of MAX_LINE_CHARS.
        let mut s = Screen::new();
        s.print(&vec![b'x'; MAX_LINE_CHARS * 3 + 5]);
        assert_eq!(s.line_count(), 4);
        assert_eq!(s.char_count(), MAX_LINE_CHARS * 3 + 5);
        // Many long lines hit the character cap.
        let mut s = Screen::new();
        for _ in 0..(MAX_CHARS / 1000 + 500) {
            s.print(&vec![b'y'; 1000]);
            s.print(b"\n");
        }
        assert!(s.char_count() <= MAX_CHARS + 1000);
    }

    #[test]
    fn a_hostile_flood_stays_fast_and_bounded() {
        let mut s = Screen::new();
        let mut junk = Vec::new();
        let mut x: u32 = 12345;
        for _ in 0..200_000 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            junk.push(x as u8);
        }
        s.print(&junk);
        assert!(s.char_count() <= MAX_CHARS + MAX_LINE_CHARS);
        let v = s.view(80, 24, "$ ", 2);
        assert!(v.rows.len() <= 24);
        s.scroll(10_000_000, 80, 24, 2);
        s.scroll(-3, 80, 24, 2);
        let _ = s.view(1, 1, "", 0);
    }

    #[test]
    fn clear_forgets_everything() {
        let mut s = Screen::new();
        s.print(b"a\nb");
        s.scroll(1, 10, 1, 0);
        s.clear();
        assert_eq!(s.line_count(), 0);
        assert!(!s.is_scrolled());
        assert_eq!(rows(&s, 10, 3, "$"), ["$"]);
    }

    #[test]
    fn degenerate_window_sizes_do_not_panic() {
        let mut s = Screen::new();
        s.print(b"hello world\n");
        for (c, r) in [(0, 0), (1, 1), (0, 5), (5, 0), (1, 100)] {
            let v = s.view(c, r, "$ x", 3);
            assert!(v.rows.len() <= r.max(1));
        }
    }

    #[test]
    fn columns_fill_down_then_across() {
        let names: Vec<String> = ["a", "bb", "ccc", "d", "e"].map(String::from).to_vec();
        // cell = 3 + 2 = 5; 12 columns fit 2 per row -> 3 rows.
        assert_eq!(columnize(&names, 12), "a    d\nbb   e\nccc\n");
        assert_eq!(columnize(&names, 4), "a\nbb\nccc\nd\ne\n");
        assert_eq!(columnize(&[], 80), "");
        let one = vec!["only".to_string()];
        assert_eq!(columnize(&one, 80), "only\n");
        // Wide enough for everything: one row, no trailing spaces.
        assert_eq!(columnize(&names, 80), "a    bb   ccc  d    e\n");
    }
}
