//! Gap buffer plus a line-start index, with UTF-8 helpers that never panic.
//!
//! The text is stored as raw bytes. Bytes that are not valid UTF-8 are kept
//! verbatim (so saving never corrupts a file) and each one behaves as a single
//! character of width 1 that renders as U+FFFD.
//!
//! Line structure is *derived* from the bytes: a line ends at `\n`, and a `\r`
//! directly before that `\n` belongs to the terminator, not to the content.
//! Positions handed out by [`TextBuf`] are byte offsets.

use alloc::vec::Vec;

/// Replacement character shown for bytes that are not valid UTF-8.
pub const REPLACEMENT: char = '\u{FFFD}';

/// A gap buffer over bytes: O(distance) cursor moves, O(1) amortised typing.
#[derive(Clone, Default)]
pub struct GapBuffer {
    buf: Vec<u8>,
    gap_start: usize,
    gap_end: usize,
}

impl GapBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from `data`, with the gap at the end.
    pub fn from_bytes(data: &[u8]) -> Self {
        Self {
            buf: data.to_vec(),
            gap_start: data.len(),
            gap_end: data.len(),
        }
    }

    pub fn len(&self) -> usize {
        self.buf.len() - (self.gap_end - self.gap_start)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Byte at logical index `i` (`0` past the end; callers bound-check).
    #[inline]
    pub fn get(&self, i: usize) -> Option<u8> {
        if i < self.gap_start {
            self.buf.get(i).copied()
        } else {
            self.buf.get(i + (self.gap_end - self.gap_start)).copied()
        }
    }

    /// Move the gap so it starts at logical position `pos` (clamped).
    pub fn move_gap(&mut self, pos: usize) {
        let pos = pos.min(self.len());
        if pos < self.gap_start {
            let n = self.gap_start - pos;
            self.buf.copy_within(pos..self.gap_start, self.gap_end - n);
            self.gap_start = pos;
            self.gap_end -= n;
        } else if pos > self.gap_start {
            let n = pos - self.gap_start;
            self.buf
                .copy_within(self.gap_end..self.gap_end + n, self.gap_start);
            self.gap_start += n;
            self.gap_end += n;
        }
    }

    fn reserve_gap(&mut self, need: usize) {
        let gap = self.gap_end - self.gap_start;
        if gap >= need {
            return;
        }
        let extra = (need - gap).max(256).max(self.len() / 8);
        let old_len = self.buf.len();
        self.buf.resize(old_len + extra, 0);
        self.buf
            .copy_within(self.gap_end..old_len, self.gap_end + extra);
        self.gap_end += extra;
    }

    /// Insert `data` at logical position `pos` (clamped).
    pub fn insert(&mut self, pos: usize, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.move_gap(pos);
        self.reserve_gap(data.len());
        self.buf[self.gap_start..self.gap_start + data.len()].copy_from_slice(data);
        self.gap_start += data.len();
    }

    /// Delete `n` bytes starting at `pos`, returning them. Out-of-range parts
    /// are ignored.
    pub fn delete(&mut self, pos: usize, n: usize) -> Vec<u8> {
        let len = self.len();
        let pos = pos.min(len);
        let n = n.min(len - pos);
        let out = self.copy_range(pos, pos + n);
        self.move_gap(pos);
        self.gap_end += n;
        out
    }

    /// Copy logical bytes `[a, b)` (clamped) into a new `Vec`.
    pub fn copy_range(&self, a: usize, b: usize) -> Vec<u8> {
        let len = self.len();
        let b = b.min(len);
        let a = a.min(b);
        let mut out = Vec::with_capacity(b - a);
        let (s1, s2) = self.as_slices();
        if a < s1.len() {
            out.extend_from_slice(&s1[a..b.min(s1.len())]);
        }
        if b > s1.len() {
            out.extend_from_slice(&s2[a.max(s1.len()) - s1.len()..b - s1.len()]);
        }
        out
    }

    /// The text as two slices (before / after the gap), without copying.
    pub fn as_slices(&self) -> (&[u8], &[u8]) {
        (&self.buf[..self.gap_start], &self.buf[self.gap_end..])
    }

    /// Move the gap to the end and return the text as one slice.
    pub fn make_contiguous(&mut self) -> &[u8] {
        let len = self.len();
        self.move_gap(len);
        &self.buf[..self.gap_start]
    }

    /// The whole text as a new `Vec`.
    pub fn to_vec(&self) -> Vec<u8> {
        self.copy_range(0, self.len())
    }
}

/// Decode the UTF-8 character at the start of `b`. Returns `(char, byte_len)`;
/// an invalid or truncated sequence decodes as one [`REPLACEMENT`] byte.
#[inline]
pub fn decode(b: &[u8]) -> (char, usize) {
    let Some(&b0) = b.first() else {
        return (REPLACEMENT, 1);
    };
    if b0 < 0x80 {
        return (char::from(b0), 1);
    }
    let need = match b0 {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => return (REPLACEMENT, 1),
    };
    if b.len() < need {
        return (REPLACEMENT, 1);
    }
    match core::str::from_utf8(&b[..need]) {
        Ok(s) => match s.chars().next() {
            Some(c) => (c, need),
            None => (REPLACEMENT, 1),
        },
        Err(_) => (REPLACEMENT, 1),
    }
}

/// True for a UTF-8 continuation byte.
#[inline]
pub fn is_cont(b: u8) -> bool {
    b & 0xC0 == 0x80
}

/// The text plus its line index.
#[derive(Clone)]
pub struct TextBuf {
    gap: GapBuffer,
    /// Byte offset of the first byte of each line; `lines[0] == 0`.
    lines: Vec<usize>,
}

impl Default for TextBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl TextBuf {
    pub fn new() -> Self {
        Self {
            gap: GapBuffer::new(),
            lines: alloc::vec![0],
        }
    }

    pub fn from_bytes(data: &[u8]) -> Self {
        let mut t = Self {
            gap: GapBuffer::from_bytes(data),
            lines: Vec::new(),
        };
        t.rebuild_lines();
        t
    }

    fn rebuild_lines(&mut self) {
        self.lines.clear();
        self.lines.push(0);
        let (a, b) = self.gap.as_slices();
        let mut off = 0;
        for s in [a, b] {
            for (i, &c) in s.iter().enumerate() {
                if c == b'\n' {
                    self.lines.push(off + i + 1);
                }
            }
            off += s.len();
        }
    }

    pub fn len(&self) -> usize {
        self.gap.len()
    }

    pub fn is_empty(&self) -> bool {
        self.gap.is_empty()
    }

    pub fn byte(&self, i: usize) -> Option<u8> {
        self.gap.get(i)
    }

    pub fn as_slices(&self) -> (&[u8], &[u8]) {
        self.gap.as_slices()
    }

    pub fn make_contiguous(&mut self) -> &[u8] {
        self.gap.make_contiguous()
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.gap.to_vec()
    }

    pub fn copy_range(&self, a: usize, b: usize) -> Vec<u8> {
        self.gap.copy_range(a, b)
    }

    /// Number of lines (always `>= 1`).
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Byte offset where line `l` starts (clamped to the last line).
    pub fn line_start(&self, l: usize) -> usize {
        self.lines[l.min(self.lines.len() - 1)]
    }

    /// Line containing byte offset `pos`.
    pub fn line_of(&self, pos: usize) -> usize {
        self.lines.partition_point(|&s| s <= pos).saturating_sub(1)
    }

    /// Where the content of line `l` ends (before `\r\n` / `\n`).
    pub fn line_end(&self, l: usize) -> usize {
        let l = l.min(self.lines.len() - 1);
        match self.lines.get(l + 1) {
            None => self.len(),
            Some(&next) => {
                // `next - 1` is the `\n`.
                let nl = next - 1;
                if nl > self.lines[l] && self.byte(nl - 1) == Some(b'\r') {
                    nl - 1
                } else {
                    nl
                }
            }
        }
    }

    /// Bytes of the terminator of line `l` (0, 1 for `\n`, 2 for `\r\n`).
    pub fn eol_len(&self, l: usize) -> usize {
        let l = l.min(self.lines.len() - 1);
        match self.lines.get(l + 1) {
            None => 0,
            Some(&next) => next - self.line_end(l),
        }
    }

    /// Start of the line after `l`, or the buffer end for the last line.
    pub fn next_line_start(&self, l: usize) -> usize {
        self.lines.get(l + 1).copied().unwrap_or(self.len())
    }

    /// Character at `pos` and its byte length. At the end it returns
    /// `(REPLACEMENT, 0)`.
    #[inline]
    pub fn char_at(&self, pos: usize) -> (char, usize) {
        let Some(b0) = self.byte(pos) else {
            return (REPLACEMENT, 0);
        };
        if b0 < 0x80 {
            return (char::from(b0), 1);
        }
        let mut tmp = [0u8; 4];
        let mut n = 0;
        while n < 4 {
            match self.byte(pos + n) {
                Some(b) => tmp[n] = b,
                None => break,
            }
            n += 1;
        }
        decode(&tmp[..n])
    }

    /// The next character boundary after `pos` (`len` at the end).
    pub fn next_boundary(&self, pos: usize) -> usize {
        let (_, n) = self.char_at(pos);
        (pos + n).min(self.len())
    }

    /// The previous character boundary before `pos` (`0` at the start).
    pub fn prev_boundary(&self, pos: usize) -> usize {
        let pos = pos.min(self.len());
        if pos == 0 {
            return 0;
        }
        let last = self.byte(pos - 1).unwrap_or(0);
        if !is_cont(last) {
            return pos - 1;
        }
        // Nearest lead byte within 3 bytes back; accept it only when its
        // sequence is valid and ends exactly at `pos`.
        let mut s = pos - 1;
        let mut back = 0;
        while back < 3 && s > 0 && is_cont(self.byte(s).unwrap_or(0)) {
            s -= 1;
            back += 1;
        }
        if !is_cont(self.byte(s).unwrap_or(0)) {
            // `char_at` only returns n > 1 for a valid sequence.
            let (_, n) = self.char_at(s);
            if n > 1 && s + n == pos {
                return s;
            }
        }
        pos - 1
    }

    /// Apply one edit: remove `del` bytes at `pos`, then insert `ins` there.
    /// Returns the removed bytes. `pos` and `del` are clamped.
    pub fn replace(&mut self, pos: usize, del: usize, ins: &[u8]) -> Vec<u8> {
        let len = self.len();
        let pos = pos.min(len);
        let del = del.min(len - pos);
        let removed = self.gap.delete(pos, del);
        self.gap.insert(pos, ins);

        let l0 = self.line_of(pos);
        let first = l0 + 1;
        let last = self.lines.partition_point(|&s| s <= pos + del);
        let last = last.max(first);
        let new_starts: Vec<usize> = ins
            .iter()
            .enumerate()
            .filter(|&(_, &c)| c == b'\n')
            .map(|(i, _)| pos + i + 1)
            .collect();
        let n_new = new_starts.len();
        self.lines.splice(first..last, new_starts);
        if ins.len() != del {
            for s in &mut self.lines[first + n_new..] {
                *s = *s - del + ins.len();
            }
        }
        removed
    }

    /// Hook for tests and fuzzing: the incremental line index equals a fresh
    /// rebuild (O(n)).
    pub fn lines_consistent(&self) -> bool {
        let fresh = TextBuf::from_bytes(&self.to_vec());
        fresh.lines == self.lines
    }

    /// Replace the whole content.
    pub fn set(&mut self, data: &[u8]) {
        self.gap = GapBuffer::from_bytes(data);
        self.rebuild_lines();
    }

    /// Character index of `pos` within its line.
    pub fn col_of(&self, pos: usize) -> usize {
        let l = self.line_of(pos);
        let mut p = self.lines[l];
        let mut col = 0;
        while p < pos {
            p = self.next_boundary(p);
            col += 1;
        }
        col
    }

    /// Byte offset of character column `col` of line `l` (clamped to the line).
    pub fn pos_of(&self, l: usize, col: usize) -> usize {
        let l = l.min(self.lines.len() - 1);
        let end = self.line_end(l);
        let mut p = self.lines[l];
        let mut c = 0;
        while c < col && p < end {
            p = self.next_boundary(p).min(end);
            c += 1;
        }
        p
    }

    /// Display column of `pos` (tabs advance to the next multiple of `tab`).
    pub fn dc_of(&self, pos: usize, tab: usize) -> usize {
        let l = self.line_of(pos);
        let mut p = self.lines[l];
        let mut dc = 0;
        while p < pos {
            let (c, n) = self.char_at(p);
            dc += width(c, dc, tab);
            p += n.max(1);
        }
        dc
    }

    /// Largest position in line `l` whose display column is `<= target`.
    pub fn pos_at_dc(&self, l: usize, target: usize, tab: usize) -> usize {
        let l = l.min(self.lines.len() - 1);
        let end = self.line_end(l);
        let mut p = self.lines[l];
        let mut dc = 0;
        while p < end {
            let (c, n) = self.char_at(p);
            let w = width(c, dc, tab);
            if dc + w > target {
                break;
            }
            dc += w;
            p += n.max(1);
        }
        p.min(end)
    }

    /// Total display width of line `l`.
    pub fn line_width(&self, l: usize, tab: usize) -> usize {
        self.dc_of(self.line_end(l), tab)
    }

    /// Number of characters of line `l`.
    pub fn line_chars(&self, l: usize) -> usize {
        self.col_of(self.line_end(l))
    }
}

/// Display width of `c` placed at display column `dc`.
#[inline]
pub fn width(c: char, dc: usize, tab: usize) -> usize {
    if c == '\t' {
        let t = tab.max(1);
        t - dc % t
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_insert_and_read() {
        let mut g = GapBuffer::new();
        g.insert(0, b"hello");
        g.insert(5, b" world");
        g.insert(5, b",");
        assert_eq!(g.to_vec(), b"hello, world");
        assert_eq!(g.len(), 12);
    }

    #[test]
    fn gap_delete_returns_removed() {
        let mut g = GapBuffer::from_bytes(b"abcdef");
        assert_eq!(g.delete(1, 3), b"bcd");
        assert_eq!(g.to_vec(), b"aef");
        assert_eq!(g.delete(10, 3), b"");
        assert_eq!(g.delete(1, 99), b"ef");
        assert_eq!(g.to_vec(), b"a");
    }

    #[test]
    fn gap_get_across_gap() {
        let mut g = GapBuffer::from_bytes(b"abcdef");
        g.move_gap(3);
        assert_eq!(g.get(2), Some(b'c'));
        assert_eq!(g.get(3), Some(b'd'));
        assert_eq!(g.get(6), None);
    }

    #[test]
    fn gap_copy_range_spanning_gap() {
        let mut g = GapBuffer::from_bytes(b"0123456789");
        g.move_gap(5);
        assert_eq!(g.copy_range(3, 8), b"34567");
        assert_eq!(g.copy_range(0, 100), b"0123456789");
        assert_eq!(g.copy_range(8, 2), b"");
    }

    #[test]
    fn gap_grows_for_large_inserts() {
        let mut g = GapBuffer::new();
        let big = alloc::vec![b'x'; 10_000];
        g.insert(0, &big);
        g.insert(5000, &big);
        assert_eq!(g.len(), 20_000);
    }

    #[test]
    fn gap_make_contiguous() {
        let mut g = GapBuffer::from_bytes(b"abc");
        g.insert(1, b"ZZ");
        assert_eq!(g.make_contiguous(), b"aZZbc");
    }

    #[test]
    fn decode_ascii_and_multibyte() {
        assert_eq!(decode(b"a"), ('a', 1));
        assert_eq!(decode("é".as_bytes()), ('é', 2));
        assert_eq!(decode("€".as_bytes()), ('€', 3));
        assert_eq!(decode("😀".as_bytes()), ('😀', 4));
    }

    #[test]
    fn decode_invalid_never_panics() {
        assert_eq!(decode(&[0xFF]), (REPLACEMENT, 1));
        assert_eq!(decode(&[0x80]), (REPLACEMENT, 1));
        assert_eq!(decode(&[0xE2, 0x82]), (REPLACEMENT, 1));
        assert_eq!(decode(&[0xC0, 0x80]), (REPLACEMENT, 1));
        assert_eq!(decode(&[0xED, 0xA0, 0x80]), (REPLACEMENT, 1));
        assert_eq!(decode(&[]), (REPLACEMENT, 1));
    }

    #[test]
    fn line_index_basic() {
        let t = TextBuf::from_bytes(b"ab\ncd\n\nefg");
        assert_eq!(t.line_count(), 4);
        assert_eq!(t.line_start(1), 3);
        assert_eq!(t.line_end(0), 2);
        assert_eq!(t.line_end(2), 6);
        assert_eq!(t.line_end(3), 10);
        assert_eq!(t.line_of(4), 1);
        assert_eq!(t.line_of(10), 3);
    }

    #[test]
    fn trailing_newline_makes_empty_last_line() {
        let t = TextBuf::from_bytes(b"a\n");
        assert_eq!(t.line_count(), 2);
        assert_eq!(t.line_end(1), 2);
    }

    #[test]
    fn crlf_terminator_is_not_content() {
        let t = TextBuf::from_bytes(b"ab\r\ncd");
        assert_eq!(t.line_end(0), 2);
        assert_eq!(t.eol_len(0), 2);
        assert_eq!(t.eol_len(1), 0);
        assert_eq!(t.next_line_start(0), 4);
    }

    #[test]
    fn lone_cr_stays_content() {
        let t = TextBuf::from_bytes(b"a\rb\n");
        assert_eq!(t.line_end(0), 3);
        assert_eq!(t.eol_len(0), 1);
    }

    #[test]
    fn replace_keeps_index_consistent() {
        let mut t = TextBuf::from_bytes(b"one\ntwo\nthree");
        t.replace(4, 4, b"X\nY\nZ\n");
        let fresh = TextBuf::from_bytes(&t.to_vec());
        assert_eq!(t.lines, fresh.lines);
        assert_eq!(t.to_vec(), b"one\nX\nY\nZ\nthree");
    }

    #[test]
    fn replace_deleting_newlines_merges_lines() {
        let mut t = TextBuf::from_bytes(b"a\nb\nc\nd");
        t.replace(1, 4, b"");
        assert_eq!(t.to_vec(), b"a\nd");
        assert_eq!(t.line_count(), 2);
    }

    #[test]
    fn replace_at_end_and_start() {
        let mut t = TextBuf::new();
        t.replace(0, 0, b"x\n");
        t.replace(2, 0, b"y");
        t.replace(0, 0, b"\n");
        assert_eq!(t.to_vec(), b"\nx\ny");
        assert_eq!(t.line_count(), 3);
    }

    #[test]
    fn boundaries_utf8() {
        let t = TextBuf::from_bytes("a€b😀".as_bytes());
        assert_eq!(t.next_boundary(0), 1);
        assert_eq!(t.next_boundary(1), 4);
        assert_eq!(t.prev_boundary(4), 1);
        assert_eq!(t.prev_boundary(5), 4);
        assert_eq!(t.prev_boundary(9), 5);
        assert_eq!(t.next_boundary(9), 9);
        assert_eq!(t.prev_boundary(0), 0);
    }

    #[test]
    fn boundaries_invalid_bytes_are_single_chars() {
        let t = TextBuf::from_bytes(&[b'a', 0xFF, 0x80, 0x80, b'b']);
        assert_eq!(t.next_boundary(1), 2);
        assert_eq!(t.prev_boundary(4), 3);
        assert_eq!(t.prev_boundary(3), 2);
        assert_eq!(t.col_of(5), 5);
    }

    #[test]
    fn boundaries_truncated_sequence() {
        let t = TextBuf::from_bytes(&[0xE2, 0x82]);
        assert_eq!(t.next_boundary(0), 1);
        assert_eq!(t.prev_boundary(2), 1);
        assert_eq!(t.prev_boundary(1), 0);
    }

    #[test]
    fn forward_backward_boundaries_agree_on_garbage() {
        // Deterministic pseudo-random bytes: walking forward then backward over
        // every boundary must visit the same positions.
        let mut x: u32 = 12345;
        let mut data = Vec::new();
        for _ in 0..4000 {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            data.push((x >> 8) as u8);
        }
        let t = TextBuf::from_bytes(&data);
        let mut fwd = alloc::vec![0usize];
        let mut p = 0;
        while p < t.len() {
            p = t.next_boundary(p);
            fwd.push(p);
        }
        let mut p = t.len();
        let mut i = fwd.len() - 1;
        while p > 0 {
            p = t.prev_boundary(p);
            i -= 1;
            assert_eq!(fwd[i], p);
        }
    }

    #[test]
    fn col_and_pos_roundtrip() {
        let t = TextBuf::from_bytes("é€x\nabc".as_bytes());
        assert_eq!(t.col_of(5), 2);
        assert_eq!(t.pos_of(0, 2), 5);
        assert_eq!(t.pos_of(0, 99), t.line_end(0));
        assert_eq!(t.pos_of(1, 2), t.line_start(1) + 2);
    }

    #[test]
    fn display_columns_expand_tabs() {
        let t = TextBuf::from_bytes(b"a\tb\t\tc");
        assert_eq!(t.dc_of(2, 4), 4);
        assert_eq!(t.dc_of(4, 4), 8);
        assert_eq!(t.dc_of(5, 4), 12);
        assert_eq!(t.line_width(0, 4), 13);
        assert_eq!(t.pos_at_dc(0, 5, 4), 3);
        assert_eq!(t.pos_at_dc(0, 3, 4), 1);
        assert_eq!(t.pos_at_dc(0, 4, 4), 2);
    }

    #[test]
    fn set_replaces_content() {
        let mut t = TextBuf::from_bytes(b"a\nb");
        t.set(b"x\ny\nz");
        assert_eq!(t.line_count(), 3);
        assert_eq!(t.to_vec(), b"x\ny\nz");
    }
}
