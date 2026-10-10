//! input (split out of `fileman.rs`).

use super::*;

/// A one-line text field (new name, rename, save-as, search) with an optional selection.
#[derive(Clone, Debug)]
pub struct TextInput {
    buf: Vec<u8>,
    /// Byte offset of the caret, always on a UTF-8 boundary.
    cur: usize,
    /// The other end of the selection (the caret is the moving end).
    anchor: Option<usize>,
    max: usize,
}

impl TextInput {
    /// A field holding `initial`, caret at the end, at most `max` bytes.
    pub fn new(initial: &[u8], max: usize) -> Self {
        let mut end = initial.len().min(max);
        while end > 0 && end < initial.len() && initial[end] & 0xC0 == 0x80 {
            end -= 1;
        }
        let buf = initial[..end].to_vec();
        TextInput {
            cur: buf.len(),
            buf,
            anchor: None,
            max,
        }
    }

    pub fn text(&self) -> &[u8] {
        &self.buf
    }

    /// The text as a string (invalid UTF-8 shown as replacement characters).
    pub fn to_string_lossy(&self) -> String {
        String::from_utf8_lossy(&self.buf).into_owned()
    }

    /// The caret's byte offset.
    pub fn caret(&self) -> usize {
        self.cur
    }

    /// The selected byte range `(start, end)`, if any.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.cur).then(|| (a.min(self.cur), a.max(self.cur)))
    }

    /// Select everything.
    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cur = self.buf.len();
    }

    /// Select the name without its extension (what renaming starts with): up to the last dot,
    /// unless the dot starts the name; everything when there is no extension.
    pub fn select_stem(&mut self) {
        let end = match self.buf.iter().rposition(|&b| b == b'.') {
            Some(i) if i > 0 => i,
            _ => self.buf.len(),
        };
        self.anchor = Some(0);
        self.cur = end;
    }

    /// Delete the selected text, leaving the caret at its start. `true` when there was some.
    pub(super) fn delete_selection(&mut self) -> bool {
        match self.selection() {
            Some((a, b)) => {
                self.buf.drain(a..b);
                self.cur = a;
                self.anchor = None;
                true
            }
            None => {
                self.anchor = None;
                false
            }
        }
    }

    /// Insert a printable byte at the caret, replacing the selection (control bytes and `/`
    /// are ignored). A byte above 127 is a Latin-1 character and is stored as UTF-8.
    pub fn insert(&mut self, b: u8) {
        if b < 0x20 || b == 0x7F || b == b'/' {
            return;
        }
        let mut tmp = [0u8; 4];
        let enc: &[u8] = if b < 0x80 {
            tmp[0] = b;
            &tmp[..1]
        } else {
            char::from(b).encode_utf8(&mut tmp).as_bytes()
        };
        let removed = self.selection().map_or(0, |(a, z)| z - a);
        if self.buf.len() - removed + enc.len() > self.max {
            return;
        }
        self.delete_selection();
        for (k, &x) in enc.iter().enumerate() {
            self.buf.insert(self.cur + k, x);
        }
        self.cur += enc.len();
    }

    pub(super) fn prev_boundary(&self, mut i: usize) -> usize {
        while i > 0 {
            i -= 1;
            if self.buf[i] & 0xC0 != 0x80 {
                break;
            }
        }
        i
    }

    pub(super) fn next_boundary(&self, mut i: usize) -> usize {
        while i < self.buf.len() {
            i += 1;
            if i >= self.buf.len() || self.buf[i] & 0xC0 != 0x80 {
                break;
            }
        }
        i
    }

    /// Delete the selection, else the character before the caret.
    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        let p = self.prev_boundary(self.cur);
        self.buf.drain(p..self.cur);
        self.cur = p;
    }

    /// Delete the selection, else the character at the caret.
    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        let n = self.next_boundary(self.cur);
        self.buf.drain(self.cur..n);
    }

    /// Move left; over a selection, collapse to its start.
    pub fn left(&mut self) {
        if let Some((a, _)) = self.selection() {
            self.cur = a;
        } else {
            self.cur = self.prev_boundary(self.cur);
        }
        self.anchor = None;
    }

    /// Move right; over a selection, collapse to its end.
    pub fn right(&mut self) {
        if let Some((_, b)) = self.selection() {
            self.cur = b;
        } else {
            self.cur = self.next_boundary(self.cur);
        }
        self.anchor = None;
    }

    pub fn home(&mut self) {
        self.anchor = None;
        self.cur = 0;
    }

    pub fn end(&mut self) {
        self.anchor = None;
        self.cur = self.buf.len();
    }

    /// Empty the field.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.cur = 0;
        self.anchor = None;
    }

    /// Replace the whole text.
    pub fn set(&mut self, text: &[u8]) {
        *self = TextInput::new(text, self.max);
    }

    /// The caret's column in the folded display text ([`display_ascii`]).
    pub fn caret_column(&self) -> usize {
        display_ascii(&self.buf[..self.cur]).len()
    }
}
