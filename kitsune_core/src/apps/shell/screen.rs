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
    /// Index in `rows` of the first row of the live line (prompt and typed text), if shown.
    pub live_first: Option<usize>,
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
        let mut live_first = None;
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
                    live_first.get_or_insert(out.len());
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
            live_first,
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
mod tests;
