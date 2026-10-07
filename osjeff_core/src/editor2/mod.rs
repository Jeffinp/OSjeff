//! `editor2` — a real text editor model: gap buffer, UTF-8, selection,
//! unlimited undo/redo, search/replace, soft wrap and a scrolling viewport.
//!
//! Pure logic only (no drawing, no I/O): the kernel feeds [`KeyEvent`]s and mouse
//! coordinates in, and draws what [`Editor::visible_rows`] yields. It is the v2
//! replacement for the fixed-grid [`crate::editor::Editor`] (which is untouched).
//!
//! # Buffer choice: gap buffer + line-start index
//!
//! The text is a [`GapBuffer`] (`Vec<u8>` with a movable hole) and a sorted
//! `Vec<usize>` with the byte offset of every line. Typing is O(1) amortised
//! (the gap sits at the cursor), a jump moves the gap in one `memmove`, and the
//! text of a 2 MB file stays contiguous except for the gap, so search runs over a
//! plain slice at memory speed. A piece table gives cheaper undo and huge-file
//! edits, but every read (render, search, UTF-8 decoding) has to walk pieces,
//! and undo is already O(edit size) here because each [`undo::Edit`] stores the
//! exact removed and inserted bytes. The line index costs O(lines after the
//! edit) per keystroke (an integer add per line: ~50 µs for 50 000 lines), which
//! is the one trade-off made for O(log n) line lookup.
//!
//! # Text model
//!
//! * Bytes are kept verbatim. Invalid UTF-8 bytes are never dropped or
//!   rewritten: each is one character of width 1 shown as U+FFFD, so saving is
//!   byte-exact.
//! * A line ends at `\n`; a `\r` right before it is part of the terminator, not
//!   of the content. Mixed `\n`/`\r\n` files therefore round-trip exactly, and
//!   new lines typed with Enter use the file's first terminator style ([`Eol`]).
//! * Cursor and columns count characters (code points); every code point has
//!   display width 1, except Tab which expands to the next multiple of the tab
//!   width.
//! * Invariant: the cursor is always at a character boundary inside the content
//!   of a line (never between `\r` and `\n`), and `0 <= cursor <= len`.

mod buffer;
mod keys;
mod search;
#[cfg(test)]
mod tests;
mod undo;
mod view;

pub use buffer::{GapBuffer, REPLACEMENT, TextBuf};
pub use search::{Notice, PromptKind, PromptView};
pub use undo::{Edit, History, Kind};
pub use view::{Cell, Cells, RowView, VisibleRows};

use crate::clipboard::{self, Clipboard};
use alloc::string::String;
use alloc::vec::Vec;

/// Line terminator style.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Eol {
    Lf,
    Crlf,
}

impl Eol {
    pub const fn bytes(self) -> &'static [u8] {
        match self {
            Eol::Lf => b"\n",
            Eol::Crlf => b"\r\n",
        }
    }
}

/// What the caller should do after a key was handled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    /// The key meant nothing to the editor.
    Ignored,
    /// The editor consumed the key (redraw).
    Handled,
    /// Ctrl+S: the caller should write [`Editor::to_bytes`] and then call
    /// [`Editor::mark_saved`].
    SaveRequested,
    /// Ctrl+Q: the caller should close the editor (check
    /// [`Editor::is_modified`] first).
    QuitRequested,
}

/// Editor options.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Config {
    /// Width of a tab stop (`>= 1`).
    pub tab_width: usize,
    /// Tab inserts spaces instead of `\t`.
    pub use_spaces: bool,
    /// Enter copies the indentation of the current line.
    pub auto_indent: bool,
    /// Show a line-number gutter.
    pub line_numbers: bool,
    /// Wrap long lines at the window width instead of scrolling sideways.
    pub soft_wrap: bool,
    /// Case-sensitive search.
    pub case_sensitive: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            tab_width: 4,
            use_spaces: true,
            auto_indent: true,
            line_numbers: false,
            soft_wrap: false,
            case_sensitive: false,
        }
    }
}

/// Snapshot for a status bar.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Status {
    /// 1-based line of the cursor.
    pub line: usize,
    /// 1-based character column of the cursor.
    pub col: usize,
    pub total_lines: usize,
    pub bytes: usize,
    pub modified: bool,
    pub eol: Eol,
    /// Characters currently selected.
    pub selected_chars: usize,
}

/// The editor.
pub struct Editor {
    pub(crate) text: TextBuf,
    pub(crate) cursor: usize,
    pub(crate) anchor: Option<usize>,
    /// Display column kept while moving vertically.
    pub(crate) pref_dc: usize,
    pub(crate) cfg: Config,
    pub(crate) view: view::View,
    pub(crate) hist: History,
    pub(crate) eol: Eol,
    pub(crate) srch: search::Search,
    pub(crate) prompt: Option<search::Prompt>,
    pub(crate) notice: Notice,
}

impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}

/// Character class used by word movement.
fn class(c: char) -> u8 {
    if c.is_alphanumeric() || c == '_' {
        2
    } else if c.is_whitespace() {
        0
    } else {
        1
    }
}

/// Cut `data` to at most `max` bytes without splitting a UTF-8 sequence.
pub fn truncate_utf8(data: &[u8], max: usize) -> &[u8] {
    if data.len() <= max {
        return data;
    }
    let mut end = max;
    while end > 0 && buffer::is_cont(data[end]) {
        end -= 1;
    }
    &data[..end]
}

impl Editor {
    /// An empty, clean editor with a 24x80 window.
    pub fn new() -> Self {
        Self::from_bytes(b"")
    }

    /// An editor holding `data` (not modified).
    pub fn from_bytes(data: &[u8]) -> Self {
        let mut e = Self {
            text: TextBuf::new(),
            cursor: 0,
            anchor: None,
            pref_dc: 0,
            cfg: Config::default(),
            view: view::View::new(24, 80),
            hist: History::new(),
            eol: Eol::Lf,
            srch: search::Search::default(),
            prompt: None,
            notice: Notice::None,
        };
        e.set_text(data);
        e
    }

    /// Replace the whole buffer (loading a file). Clears history, selection and
    /// scroll, and marks the buffer clean.
    pub fn set_text(&mut self, data: &[u8]) {
        self.text.set(data);
        self.eol = detect_eol(data);
        self.cursor = 0;
        self.anchor = None;
        self.pref_dc = 0;
        self.hist.clear();
        self.prompt = None;
        self.notice = Notice::None;
        self.view.reset_scroll();
    }

    // ---- configuration -------------------------------------------------

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    pub fn set_tab_width(&mut self, w: usize) {
        self.cfg.tab_width = w.clamp(1, 16);
        self.pref_dc = self.text.dc_of(self.cursor, self.cfg.tab_width);
        self.ensure_visible();
    }

    pub fn set_use_spaces(&mut self, on: bool) {
        self.cfg.use_spaces = on;
    }

    pub fn set_auto_indent(&mut self, on: bool) {
        self.cfg.auto_indent = on;
    }

    pub fn set_line_numbers(&mut self, on: bool) {
        self.cfg.line_numbers = on;
        self.ensure_visible();
    }

    pub fn set_soft_wrap(&mut self, on: bool) {
        self.cfg.soft_wrap = on;
        self.view.reset_scroll_x();
        self.ensure_visible();
    }

    pub fn set_case_sensitive(&mut self, on: bool) {
        self.cfg.case_sensitive = on;
    }

    /// Bound the memory used by undo history (`usize::MAX` = unlimited, the
    /// default). The oldest groups are dropped first.
    pub fn set_undo_limit(&mut self, bytes: usize) {
        self.hist.set_limit(bytes);
    }

    pub fn eol(&self) -> Eol {
        self.eol
    }

    pub fn set_eol(&mut self, eol: Eol) {
        self.eol = eol;
    }

    // ---- reading -------------------------------------------------------

    pub fn len_bytes(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn line_count(&self) -> usize {
        self.text.line_count()
    }

    /// The whole buffer, byte-exact (what to write on save).
    pub fn to_bytes(&self) -> Vec<u8> {
        self.text.to_vec()
    }

    /// The buffer as two slices (before/after the gap) without copying; write
    /// both in order to save a large file with no extra allocation.
    pub fn as_slices(&self) -> (&[u8], &[u8]) {
        self.text.as_slices()
    }

    /// Content of line `l` (without its terminator).
    pub fn line_bytes(&self, l: usize) -> Vec<u8> {
        self.text
            .copy_range(self.text.line_start(l), self.text.line_end(l))
    }

    /// Content of line `l` as text (invalid bytes become U+FFFD).
    pub fn line_string(&self, l: usize) -> String {
        String::from_utf8_lossy(&self.line_bytes(l)).into_owned()
    }

    /// Number of characters in line `l`.
    pub fn line_chars(&self, l: usize) -> usize {
        self.text.line_chars(l)
    }

    /// Cursor as `(line, column)`, both 0-based; the column counts characters.
    pub fn cursor(&self) -> (usize, usize) {
        (
            self.text.line_of(self.cursor),
            self.text.col_of(self.cursor),
        )
    }

    /// Cursor as a byte offset.
    pub fn cursor_byte(&self) -> usize {
        self.cursor
    }

    /// Place the cursor at `(line, col)` (clamped); clears the selection.
    pub fn set_cursor(&mut self, line: usize, col: usize) {
        let p = self.text.pos_of(line, col);
        self.move_to(p, false, false);
    }

    pub fn status(&self) -> Status {
        let (line, col) = self.cursor();
        let selected_chars = self.sel_range().map_or(0, |(a, b)| {
            let mut n = 0;
            let mut p = a;
            while p < b {
                p = self.text.next_boundary(p);
                n += 1;
            }
            n
        });
        Status {
            line: line + 1,
            col: col + 1,
            total_lines: self.text.line_count(),
            bytes: self.text.len(),
            modified: self.is_modified(),
            eol: self.eol,
            selected_chars,
        }
    }

    // ---- modified state ------------------------------------------------

    /// True when the text differs from what was last saved/loaded. Undoing
    /// back to the saved state makes it false again.
    pub fn is_modified(&self) -> bool {
        self.hist.modified()
    }

    /// Call after the file was written successfully.
    pub fn mark_saved(&mut self) {
        self.hist.mark_saved();
    }

    pub fn can_undo(&self) -> bool {
        self.hist.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.hist.can_redo()
    }

    // ---- selection -----------------------------------------------------

    /// The selection as `(start, end)` byte offsets, if non-empty.
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.sel_range()
    }

    pub(crate) fn sel_range(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        if a == self.cursor {
            None
        } else {
            Some((a.min(self.cursor), a.max(self.cursor)))
        }
    }

    pub fn has_selection(&self) -> bool {
        self.sel_range().is_some()
    }

    /// The selected bytes (empty without a selection).
    pub fn selected_bytes(&self) -> Vec<u8> {
        match self.sel_range() {
            Some((a, b)) => self.text.copy_range(a, b),
            None => Vec::new(),
        }
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    pub fn select_all(&mut self) {
        self.hist.break_group();
        self.anchor = Some(0);
        self.cursor = self.text.len();
        self.pref_dc = self.text.dc_of(self.cursor, self.cfg.tab_width);
        self.ensure_visible();
    }

    /// Select bytes `[a, b)` (snapped to valid positions), cursor at the end.
    pub fn select_range(&mut self, a: usize, b: usize) {
        self.hist.break_group();
        let a = self.normalize(a);
        let b = self.normalize(b);
        self.anchor = Some(a);
        self.cursor = b;
        self.pref_dc = self.text.dc_of(b, self.cfg.tab_width);
        self.ensure_visible();
    }

    /// Select the word (or run of same-class characters) around `pos`.
    pub fn select_word_at(&mut self, pos: usize) {
        let pos = self.normalize(pos);
        let l = self.text.line_of(pos);
        let (ls, le) = (self.text.line_start(l), self.text.line_end(l));
        if ls == le {
            self.select_range(pos, pos);
            return;
        }
        // On the line end, take the run before it.
        let probe = if pos >= le {
            self.text.prev_boundary(le)
        } else {
            pos
        };
        let k = class(self.text.char_at(probe).0);
        let mut a = probe;
        while a > ls {
            let p = self.text.prev_boundary(a);
            if class(self.text.char_at(p).0) != k {
                break;
            }
            a = p;
        }
        let mut b = self.text.next_boundary(probe);
        while b < le && class(self.text.char_at(b).0) == k {
            b = self.text.next_boundary(b);
        }
        self.select_range(a, b);
    }

    /// Select the whole line holding `pos`, including its terminator.
    pub fn select_line_at(&mut self, pos: usize) {
        let pos = self.normalize(pos);
        let l = self.text.line_of(pos);
        let a = self.text.line_start(l);
        let b = self.text.next_line_start(l);
        self.select_range(a, b);
        // Keep the cursor inside the content invariant.
        self.cursor = self.normalize(self.cursor);
    }

    // ---- positions -----------------------------------------------------

    /// Snap `pos` to a valid cursor position.
    pub(crate) fn normalize(&self, pos: usize) -> usize {
        let mut p = pos.min(self.text.len());
        // Back up out of the middle of a valid multi-byte sequence.
        for k in 0..4usize {
            let Some(s) = p.checked_sub(k) else { break };
            match self.text.byte(s) {
                Some(b) if buffer::is_cont(b) => continue,
                _ => {
                    if s + self.text.char_at(s).1 > p {
                        p = s;
                    }
                    break;
                }
            }
        }
        let l = self.text.line_of(p);
        p.min(self.text.line_end(l))
    }

    fn after_move(&mut self, keep_pref: bool) {
        if !keep_pref {
            self.pref_dc = self.text.dc_of(self.cursor, self.cfg.tab_width);
        }
        self.ensure_visible();
    }

    /// Move the cursor to `pos`, extending the selection when `select`.
    pub(crate) fn move_to(&mut self, pos: usize, select: bool, keep_pref: bool) {
        self.hist.break_group();
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = self.normalize(pos);
        self.after_move(keep_pref);
    }

    // ---- movement ------------------------------------------------------

    pub fn move_left(&mut self, select: bool) {
        if !select && let Some((a, _)) = self.sel_range() {
            self.move_to(a, false, false);
            return;
        }
        let c = self.cursor;
        let l = self.text.line_of(c);
        let p = if c == self.text.line_start(l) {
            if l == 0 { 0 } else { self.text.line_end(l - 1) }
        } else {
            self.text.prev_boundary(c)
        };
        self.move_to(p, select, false);
    }

    pub fn move_right(&mut self, select: bool) {
        if !select && let Some((_, b)) = self.sel_range() {
            self.move_to(b, false, false);
            return;
        }
        let c = self.cursor;
        let l = self.text.line_of(c);
        let p = if c >= self.text.line_end(l) {
            self.text.next_line_start(l).max(c)
        } else {
            self.text.next_boundary(c)
        };
        self.move_to(p, select, false);
    }

    /// Start of the previous word (stops at line boundaries).
    pub(crate) fn word_left_pos(&self, pos: usize) -> usize {
        let l = self.text.line_of(pos);
        let ls = self.text.line_start(l);
        if pos <= ls {
            return if l == 0 { 0 } else { self.text.line_end(l - 1) };
        }
        let mut p = pos;
        while p > ls {
            let q = self.text.prev_boundary(p);
            if class(self.text.char_at(q).0) != 0 {
                break;
            }
            p = q;
        }
        if p > ls {
            let k = class(self.text.char_at(self.text.prev_boundary(p)).0);
            while p > ls {
                let q = self.text.prev_boundary(p);
                if class(self.text.char_at(q).0) != k {
                    break;
                }
                p = q;
            }
        }
        p
    }

    /// Start of the next word (stops at line boundaries).
    pub(crate) fn word_right_pos(&self, pos: usize) -> usize {
        let l = self.text.line_of(pos);
        let le = self.text.line_end(l);
        if pos >= le {
            return self.text.next_line_start(l).max(pos);
        }
        let mut p = pos;
        let k = class(self.text.char_at(p).0);
        if k != 0 {
            while p < le && class(self.text.char_at(p).0) == k {
                p = self.text.next_boundary(p);
            }
        }
        while p < le && class(self.text.char_at(p).0) == 0 {
            p = self.text.next_boundary(p);
        }
        p
    }

    pub fn move_word_left(&mut self, select: bool) {
        let p = self.word_left_pos(self.cursor);
        self.move_to(p, select, false);
    }

    pub fn move_word_right(&mut self, select: bool) {
        let p = self.word_right_pos(self.cursor);
        self.move_to(p, select, false);
    }

    /// Smart Home: first non-blank character, then column 0.
    pub fn move_home(&mut self, select: bool) {
        let l = self.text.line_of(self.cursor);
        let ls = self.text.line_start(l);
        let le = self.text.line_end(l);
        let mut fnb = ls;
        while fnb < le && matches!(self.text.byte(fnb), Some(b' ' | b'\t')) {
            fnb += 1;
        }
        let target = if self.cursor != fnb { fnb } else { ls };
        self.move_to(target, select, false);
    }

    pub fn move_end(&mut self, select: bool) {
        let l = self.text.line_of(self.cursor);
        let p = self.text.line_end(l);
        self.move_to(p, select, false);
    }

    pub fn move_doc_start(&mut self, select: bool) {
        self.move_to(0, select, false);
    }

    pub fn move_doc_end(&mut self, select: bool) {
        let n = self.text.len();
        self.move_to(n, select, false);
    }

    /// One visual row up (`dir < 0`) or down (`dir > 0`), keeping the column.
    pub fn move_vertical(&mut self, dir: i32, select: bool) {
        let target = self.vertical_target(dir);
        self.move_to(target, select, true);
    }

    /// Move by a page (window height minus one row).
    pub fn page(&mut self, dir: i32, select: bool) {
        let steps = self.view.rows.saturating_sub(1).max(1);
        let mut pos = self.cursor;
        let saved = self.cursor;
        for _ in 0..steps {
            self.cursor = pos;
            let next = self.vertical_target(dir);
            if next == pos {
                break;
            }
            pos = next;
        }
        self.cursor = saved;
        self.move_to(pos, select, true);
    }

    /// Jump to 1-based `line` (clamped), centring it in the window.
    pub fn goto_line(&mut self, line: usize) {
        let l = line.saturating_sub(1).min(self.text.line_count() - 1);
        let p = self.text.line_start(l);
        self.move_to(p, false, false);
        self.center_cursor();
    }

    // ---- editing primitives ---------------------------------------------

    /// Apply one edit and record it in the current history group.
    fn raw_edit(&mut self, pos: usize, del: usize, ins: &[u8]) {
        let len = self.text.len();
        let pos = pos.min(len);
        let del = del.min(len - pos);
        if del == 0 && ins.is_empty() {
            return;
        }
        let removed = self.text.replace(pos, del, ins);
        self.hist.push(Edit {
            pos,
            removed,
            inserted: ins.to_vec(),
        });
    }

    fn raw_insert_at_cursor(&mut self, ins: &[u8]) {
        let c = self.cursor;
        self.raw_edit(c, 0, ins);
        self.cursor = c + ins.len();
    }

    fn delete_selection_raw(&mut self) -> bool {
        match self.sel_range() {
            Some((a, b)) => {
                self.raw_edit(a, b - a, b"");
                self.cursor = a;
                self.anchor = None;
                true
            }
            None => false,
        }
    }

    /// Run `f` as one undo group of `kind`, then repair the cursor and scroll.
    fn edit_cmd(&mut self, kind: Kind, ws: bool, f: impl FnOnce(&mut Self)) {
        self.hist.begin(kind, self.cursor, ws);
        f(self);
        self.hist.finish(self.cursor, kind);
        self.cursor = self.normalize(self.cursor);
        // Commands that keep a selection (indent/outdent) restore it after.
        self.anchor = None;
        self.after_move(false);
    }

    // ---- editing commands ----------------------------------------------

    /// Type one character (replaces the selection). `'\n'` is Enter; other
    /// control characters are ignored.
    pub fn insert_char(&mut self, ch: char) {
        if ch == '\n' {
            self.newline();
            return;
        }
        if ch.is_control() && ch != '\t' {
            return;
        }
        let mut b = [0u8; 4];
        let s = ch.encode_utf8(&mut b).as_bytes().to_vec();
        if self.has_selection() {
            // Typing over a selection starts a group that later keystrokes join.
            self.hist.break_group();
        }
        self.edit_cmd(Kind::Typing, ch.is_whitespace(), |e| {
            e.delete_selection_raw();
            e.raw_insert_at_cursor(&s);
        });
    }

    /// Insert text verbatim (paste), replacing the selection. Line breaks in
    /// `data` are kept as they are.
    pub fn insert_bytes(&mut self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.edit_cmd(Kind::Other, false, |e| {
            e.delete_selection_raw();
            e.raw_insert_at_cursor(data);
        });
    }

    pub fn insert_str(&mut self, s: &str) {
        self.insert_bytes(s.as_bytes());
    }

    /// Leading blanks of the line of `pos`, up to `pos`.
    fn indent_prefix(&self, pos: usize) -> Vec<u8> {
        let l = self.text.line_of(pos);
        let mut p = self.text.line_start(l);
        let mut out = Vec::new();
        while p < pos {
            match self.text.byte(p) {
                Some(b @ (b' ' | b'\t')) => out.push(b),
                _ => break,
            }
            p += 1;
        }
        out
    }

    /// Enter: split the line (with auto-indent when enabled).
    pub fn newline(&mut self) {
        self.edit_cmd(Kind::Other, false, |e| {
            e.delete_selection_raw();
            let mut ins = e.eol.bytes().to_vec();
            if e.cfg.auto_indent {
                ins.extend(e.indent_prefix(e.cursor));
            }
            e.raw_insert_at_cursor(&ins);
        });
    }

    /// Backspace: delete the selection, else the previous character (or the
    /// line break when at the start of a line).
    pub fn backspace(&mut self) {
        if self.has_selection() {
            self.edit_cmd(Kind::Other, false, |e| {
                e.delete_selection_raw();
            });
            return;
        }
        let c = self.cursor;
        if c == 0 {
            return;
        }
        let l = self.text.line_of(c);
        let start = if c == self.text.line_start(l) {
            self.text.line_end(l - 1)
        } else {
            self.text.prev_boundary(c)
        };
        self.edit_cmd(Kind::Backspacing, false, |e| {
            e.raw_edit(start, c - start, b"");
            e.cursor = start;
        });
    }

    /// Delete: remove the selection, else the next character (or line break).
    pub fn delete_forward(&mut self) {
        if self.has_selection() {
            self.edit_cmd(Kind::Other, false, |e| {
                e.delete_selection_raw();
            });
            return;
        }
        let c = self.cursor;
        let l = self.text.line_of(c);
        let end = self.text.line_end(l);
        let n = if c < end {
            self.text.next_boundary(c) - c
        } else if l + 1 < self.text.line_count() {
            self.text.eol_len(l)
        } else {
            return;
        };
        self.edit_cmd(Kind::Deleting, false, |e| e.raw_edit(c, n, b""));
    }

    /// Ctrl+Backspace.
    pub fn delete_word_left(&mut self) {
        if self.has_selection() {
            self.backspace();
            return;
        }
        let c = self.cursor;
        let start = self.word_left_pos(c);
        if start == c {
            return;
        }
        self.edit_cmd(Kind::Other, false, |e| {
            e.raw_edit(start, c - start, b"");
            e.cursor = start;
        });
    }

    /// Ctrl+Delete.
    pub fn delete_word_right(&mut self) {
        if self.has_selection() {
            self.delete_forward();
            return;
        }
        let c = self.cursor;
        let end = self.word_right_pos(c);
        if end == c {
            return;
        }
        // Delete a whole line break when crossing it.
        let l = self.text.line_of(c);
        let end = if end > self.text.line_end(l) && end == self.text.next_line_start(l) {
            self.text.line_end(l) + self.text.eol_len(l)
        } else {
            end
        };
        self.edit_cmd(Kind::Other, false, |e| e.raw_edit(c, end - c, b""));
    }

    fn indent_unit(&self) -> Vec<u8> {
        if self.cfg.use_spaces {
            alloc::vec![b' '; self.cfg.tab_width]
        } else {
            alloc::vec![b'\t']
        }
    }

    /// Tab: indent the selected lines, or insert a tab/spaces up to the next
    /// tab stop.
    pub fn tab(&mut self) {
        if let Some((a, b)) = self.sel_range() {
            let la = self.text.line_of(a);
            let mut lb = self.text.line_of(b);
            if lb > la {
                if b == self.text.line_start(lb) {
                    lb -= 1;
                }
                self.indent_lines(la, lb);
                return;
            }
        }
        let ins = if self.cfg.use_spaces {
            let dc = self.text.dc_of(
                self.sel_range().map_or(self.cursor, |r| r.0),
                self.cfg.tab_width,
            );
            let n = self.cfg.tab_width - dc % self.cfg.tab_width;
            alloc::vec![b' '; n]
        } else {
            alloc::vec![b'\t']
        };
        let kind = if self.has_selection() {
            Kind::Other
        } else {
            Kind::Typing
        };
        self.edit_cmd(kind, true, |e| {
            e.delete_selection_raw();
            e.raw_insert_at_cursor(&ins);
        });
    }

    fn indent_lines(&mut self, la: usize, lb: usize) {
        let unit = self.indent_unit();
        let starts: Vec<usize> = (la..=lb).map(|l| self.text.line_start(l)).collect();
        let map = |p: usize| p + unit.len() * starts.iter().filter(|&&s| s < p).count();
        let (cur, anc) = (map(self.cursor), self.anchor.map(map));
        self.edit_cmd(Kind::Other, false, |e| {
            for &s in starts.iter().rev() {
                e.raw_edit(s, 0, &unit);
            }
            e.cursor = cur;
        });
        self.anchor = anc;
    }

    /// Shift+Tab: remove one indentation level from the selected lines (or the
    /// cursor line).
    pub fn outdent(&mut self) {
        let (la, lb) = match self.sel_range() {
            Some((a, b)) => {
                let la = self.text.line_of(a);
                let mut lb = self.text.line_of(b);
                if lb > la && b == self.text.line_start(lb) {
                    lb -= 1;
                }
                (la, lb)
            }
            None => {
                let l = self.text.line_of(self.cursor);
                (l, l)
            }
        };
        let tw = self.cfg.tab_width;
        let mut cuts: Vec<(usize, usize)> = Vec::new();
        for l in la..=lb {
            let s = self.text.line_start(l);
            let e = self.text.line_end(l);
            let n = match self.text.byte(s) {
                Some(b'\t') if s < e => 1,
                Some(b' ') => {
                    let mut k = 0;
                    while k < tw && s + k < e && self.text.byte(s + k) == Some(b' ') {
                        k += 1;
                    }
                    k
                }
                _ => 0,
            };
            if n > 0 {
                cuts.push((s, n));
            }
        }
        if cuts.is_empty() {
            return;
        }
        let map = |p: usize| {
            let mut sub = 0;
            for &(s, n) in &cuts {
                if s < p {
                    sub += n.min(p - s);
                }
            }
            p - sub
        };
        let (cur, anc) = (map(self.cursor), self.anchor.map(map));
        self.edit_cmd(Kind::Other, false, |e| {
            for &(s, n) in cuts.iter().rev() {
                e.raw_edit(s, n, b"");
            }
            e.cursor = cur;
        });
        self.anchor = anc;
    }

    // ---- undo / redo ---------------------------------------------------

    /// Undo the last group. Returns false when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        let text = &mut self.text;
        let r = self.hist.undo_with(|g| {
            for e in g.edits.iter().rev() {
                text.replace(e.pos, e.inserted.len(), &e.removed);
            }
            g.cursor_before
        });
        self.finish_history_move(r)
    }

    /// Redo the last undone group.
    pub fn redo(&mut self) -> bool {
        let text = &mut self.text;
        let r = self.hist.redo_with(|g| {
            for e in &g.edits {
                text.replace(e.pos, e.removed.len(), &e.inserted);
            }
            g.cursor_after
        });
        self.finish_history_move(r)
    }

    fn finish_history_move(&mut self, r: Option<usize>) -> bool {
        match r {
            Some(c) => {
                self.cursor = self.normalize(c);
                self.anchor = None;
                self.after_move(false);
                true
            }
            None => false,
        }
    }

    // ---- clipboard -----------------------------------------------------

    /// Copy the selection into `clip` (truncated to its capacity at a
    /// character boundary). Returns false without a selection.
    pub fn copy(&self, clip: &mut Clipboard) -> bool {
        let sel = self.selected_bytes();
        if sel.is_empty() {
            return false;
        }
        clip.set(truncate_utf8(&sel, clipboard::CAP));
        true
    }

    /// Copy then delete the selection. If the selection is bigger than the
    /// clipboard, nothing is deleted (no silent data loss).
    pub fn cut(&mut self, clip: &mut Clipboard) -> bool {
        let sel = self.selected_bytes();
        if sel.is_empty() || sel.len() > clipboard::CAP {
            return false;
        }
        clip.set(&sel);
        self.backspace();
        true
    }

    /// Paste the clipboard at the cursor (replacing the selection).
    pub fn paste(&mut self, clip: &Clipboard) -> bool {
        if clip.is_empty() {
            return false;
        }
        let data = clip.get().to_vec();
        self.insert_bytes(&data);
        true
    }
}

fn detect_eol(data: &[u8]) -> Eol {
    match data.iter().position(|&b| b == b'\n') {
        Some(i) if i > 0 && data[i - 1] == b'\r' => Eol::Crlf,
        _ => Eol::Lf,
    }
}
