//! editing (split out of `mod.rs`).

use super::*;

impl Editor {
    /// Apply one edit and record it in the current history group.
    pub(super) fn raw_edit(&mut self, pos: usize, del: usize, ins: &[u8]) {
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

    pub(super) fn raw_insert_at_cursor(&mut self, ins: &[u8]) {
        let c = self.cursor;
        self.raw_edit(c, 0, ins);
        self.cursor = c + ins.len();
    }

    pub(super) fn delete_selection_raw(&mut self) -> bool {
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
    pub(super) fn edit_cmd(&mut self, kind: Kind, ws: bool, f: impl FnOnce(&mut Self)) {
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
    pub(super) fn indent_prefix(&self, pos: usize) -> Vec<u8> {
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

    pub(super) fn indent_unit(&self) -> Vec<u8> {
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

    pub(super) fn indent_lines(&mut self, la: usize, lb: usize) {
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
}
