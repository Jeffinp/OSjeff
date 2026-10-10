//! selection (split out of `mod.rs`).

use super::*;

impl Editor {
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

    pub(super) fn after_move(&mut self, keep_pref: bool) {
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
}
