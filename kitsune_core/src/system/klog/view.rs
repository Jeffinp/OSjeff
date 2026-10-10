//! view (split out of `klog.rs`).

use super::*;

/// What the viewer shows: records at or above `min` whose text contains the
/// needle (ASCII case-insensitive; an empty needle matches everything).
#[derive(Clone, Debug)]
pub struct Filter {
    pub min: Level,
    pub(super) needle: [u8; 24],
    pub(super) needle_len: usize,
}

impl Default for Filter {
    fn default() -> Self {
        Self::new()
    }
}

impl Filter {
    pub const fn new() -> Self {
        Self {
            min: Level::Trace,
            needle: [0; 24],
            needle_len: 0,
        }
    }

    pub fn needle(&self) -> &[u8] {
        &self.needle[..self.needle_len]
    }

    pub fn push_char(&mut self, b: u8) -> bool {
        if self.needle_len < self.needle.len() && (0x20..0x7F).contains(&b) {
            self.needle[self.needle_len] = b;
            self.needle_len += 1;
            true
        } else {
            false
        }
    }

    pub fn backspace(&mut self) -> bool {
        if self.needle_len > 0 {
            self.needle_len -= 1;
            true
        } else {
            false
        }
    }

    pub fn clear_needle(&mut self) {
        self.needle_len = 0;
    }

    /// Cycle the minimum level Trace -> ... -> Fatal -> Trace.
    pub fn cycle_level(&mut self) {
        self.min = if self.min == Level::Fatal {
            Level::Trace
        } else {
            self.min.next()
        };
    }

    pub fn matches(&self, e: &Entry<'_>) -> bool {
        e.level >= self.min && contains_ci(e.text, self.needle())
    }
}

/// ASCII case-insensitive substring test (`needle` empty = true).
pub fn contains_ci(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > hay.len() {
        return false;
    }
    hay.windows(needle.len())
        .any(|w| w.iter().zip(needle).all(|(a, b)| a.eq_ignore_ascii_case(b)))
}

/// Indexed, scrollable window onto a snapshot.
#[derive(Default)]
pub struct LogView {
    /// Byte offsets (in the snapshot) of the records passing the filter.
    pub(super) index: Vec<u32>,
    /// Index of the first visible line.
    pub(super) top: usize,
    /// Follow the newest line.
    pub follow: bool,
}

impl LogView {
    pub fn new() -> Self {
        Self {
            index: Vec::new(),
            top: 0,
            follow: true,
        }
    }

    /// Rebuild the index for `snapshot` and `filter`; `rows` is the number of
    /// lines that fit. Keeps the scroll position unless following.
    pub fn rebuild(&mut self, snapshot: &[u8], filter: &Filter, rows: usize) {
        self.index.clear();
        let mut off = 0usize;
        while let Some((e, n)) = record_at(snapshot, off) {
            if filter.matches(&e) {
                self.index.push(off as u32);
            }
            off += n;
        }
        self.clamp(rows);
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    pub(super) fn max_top(&self, rows: usize) -> usize {
        self.index.len().saturating_sub(rows)
    }

    pub(super) fn clamp(&mut self, rows: usize) {
        let max = self.max_top(rows);
        if self.follow {
            self.top = max;
        } else {
            self.top = self.top.min(max);
        }
    }

    /// Index of the first visible line (as last stored; see [`top_for`](Self::top_for)).
    pub fn top(&self) -> usize {
        self.top
    }

    /// The first visible line for a window of `rows` lines *right now*: the
    /// newest page while following, else the stored position clamped. Drawing
    /// uses this so a window resized since the last rebuild still shows a full page.
    pub fn top_for(&self, rows: usize) -> usize {
        let max = self.max_top(rows);
        if self.follow { max } else { self.top.min(max) }
    }

    /// Jump to line `top` (clamped); reaching the bottom resumes following.
    pub fn set_top(&mut self, top: usize, rows: usize) {
        let max = self.max_top(rows);
        self.top = top.min(max);
        self.follow = self.top == max;
    }

    /// Scroll by `delta` lines (negative = up). Scrolling up stops following;
    /// reaching the bottom resumes it.
    pub fn scroll(&mut self, delta: i32, rows: usize) {
        let max = self.max_top(rows) as i64;
        let t = (self.top as i64 + delta as i64).clamp(0, max);
        self.top = t as usize;
        self.follow = t == max;
    }

    pub fn home(&mut self) {
        self.top = 0;
        self.follow = self.index.len() <= 1;
    }

    pub fn end(&mut self, rows: usize) {
        self.follow = true;
        self.top = self.max_top(rows);
    }

    /// The visible records (at most `rows`), oldest first.
    pub fn visible<'a>(
        &'a self,
        snapshot: &'a [u8],
        rows: usize,
    ) -> impl Iterator<Item = Entry<'a>> + 'a {
        self.index
            .iter()
            .skip(self.top_for(rows))
            .take(rows)
            .filter_map(move |&o| record_at(snapshot, o as usize).map(|(e, _)| e))
    }
}

impl LogView {
    /// `count` records starting at filtered line `first` (clamped), oldest first: for a
    /// view that scrolls by pixels and draws a partial line at each end.
    pub fn visible_from<'a>(
        &'a self,
        snapshot: &'a [u8],
        first: usize,
        count: usize,
    ) -> impl Iterator<Item = Entry<'a>> + 'a {
        self.index
            .iter()
            .skip(first)
            .take(count)
            .filter_map(move |&o| record_at(snapshot, o as usize).map(|(e, _)| e))
    }

    /// Largest first line for a window of `rows` lines.
    pub fn max_top_for(&self, rows: usize) -> usize {
        self.max_top(rows)
    }
}
