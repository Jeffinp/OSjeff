//! System log: a byte ring of variable-length records, plus the pure logic of
//! the log viewer (level filter, text search, scroll, line formatting, text
//! dump).
//!
//! # The ring
//!
//! [`LogRing`] is a fixed array used as a circular byte buffer. Every record is
//! a 12-byte header followed by the message text (at most [`MAX_MSG`] bytes,
//! longer text is cut):
//!
//! ```text
//! seq:u32  ts_ms:u32  level:u8  origin:u8  len:u16  text[len]
//! ```
//!
//! Writing never allocates and never fails: when the free space is smaller than
//! the new record, the *oldest* whole records are dropped (counted in
//! [`LogRing::dropped`]). Because the buffer is a plain array and a record is
//! written with a handful of `memcpy`s, the kernel can push from an interrupt
//! handler (it only has to keep interrupts off around the call; there is no
//! lock to spin on and no heap).
//!
//! Readers do not walk the live ring. [`LogRing::copy_out`] copies the used
//! bytes, oldest first, into a linear buffer owned by the reader (a short
//! critical section), and [`records`] decodes that snapshot, where each text is
//! contiguous.
//!
//! # The view
//!
//! [`LogView`] turns a snapshot into the lines a window shows: it indexes the
//! records that pass a [`Filter`] (minimum level + case-insensitive substring)
//! and keeps a scroll position that sticks to the newest line until the user
//! scrolls up.

use alloc::vec::Vec;

/// Longest message kept in a record (longer text is truncated).
pub const MAX_MSG: usize = 200;
/// Bytes of the record header.
pub const HDR: usize = 12;
/// Size of the kernel's log ring.
pub const RING_BYTES: usize = 64 * 1024;

/// Severity of a record, lowest first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(u8)]
pub enum Level {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
    Fatal = 5,
}

impl Level {
    pub const ALL: [Level; 6] = [
        Level::Trace,
        Level::Debug,
        Level::Info,
        Level::Warn,
        Level::Error,
        Level::Fatal,
    ];

    /// The level stored as `v` (anything above `Fatal` reads as `Fatal`).
    pub const fn from_u8(v: u8) -> Level {
        match v {
            0 => Level::Trace,
            1 => Level::Debug,
            2 => Level::Info,
            3 => Level::Warn,
            4 => Level::Error,
            _ => Level::Fatal,
        }
    }

    /// Five-letter tag (`"INFO "`).
    pub const fn tag(self) -> &'static str {
        match self {
            Level::Trace => "TRACE",
            Level::Debug => "DEBUG",
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
            Level::Fatal => "FATAL",
        }
    }

    /// Catalog key of the severity's title in notifications and banners
    /// (`Information`, `Warning`, `Error`, `Critical failure`).
    pub const fn title_key(self) -> &'static str {
        match self {
            Level::Trace | Level::Debug | Level::Info => crate::tk!("level.info"),
            Level::Warn => crate::tk!("level.warn"),
            Level::Error => crate::tk!("level.error"),
            Level::Fatal => crate::tk!("level.fatal"),
        }
    }

    /// One-letter tag for the compact viewer line.
    pub const fn letter(self) -> u8 {
        match self {
            Level::Trace => b'T',
            Level::Debug => b'D',
            Level::Info => b'I',
            Level::Warn => b'W',
            Level::Error => b'E',
            Level::Fatal => b'F',
        }
    }

    /// The next level up (saturating at `Fatal`), for the filter button.
    pub const fn next(self) -> Level {
        Level::from_u8(self as u8 + 1)
    }
}

/// One decoded record. `text` borrows the snapshot it was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    pub seq: u32,
    pub ts_ms: u32,
    pub level: Level,
    /// Scheduler slot of the thread that logged it (255 = unknown).
    pub origin: u8,
    pub text: &'a [u8],
}

/// Circular byte buffer of log records (see the module docs).
pub struct LogRing<const N: usize> {
    buf: [u8; N],
    /// Offset of the oldest record.
    head: usize,
    /// Bytes in use.
    used: usize,
    next_seq: u32,
    dropped: u32,
}

impl<const N: usize> Default for LogRing<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> LogRing<N> {
    /// An empty ring. `N` must hold at least one maximal record.
    pub const fn new() -> Self {
        assert!(N > HDR + MAX_MSG);
        Self {
            buf: [0; N],
            head: 0,
            used: 0,
            next_seq: 0,
            dropped: 0,
        }
    }

    /// Sequence number the next record will get (= records ever pushed).
    pub const fn next_seq(&self) -> u32 {
        self.next_seq
    }

    /// Records dropped to make room.
    pub const fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Bytes currently in use.
    pub const fn used(&self) -> usize {
        self.used
    }

    /// Drop every record (the sequence keeps counting).
    pub fn clear(&mut self) {
        self.head = 0;
        self.used = 0;
    }

    fn wrap(pos: usize) -> usize {
        if pos >= N { pos - N } else { pos }
    }

    fn read_byte(&self, off: usize) -> u8 {
        self.buf[Self::wrap(self.head + off)]
    }

    /// Length of the record at logical offset `off` from the head.
    fn rec_len_at(&self, off: usize) -> usize {
        let l = u16::from_le_bytes([self.read_byte(off + 10), self.read_byte(off + 11)]);
        HDR + l as usize
    }

    fn write_at(&mut self, pos: usize, data: &[u8]) {
        let first = (N - pos).min(data.len());
        self.buf[pos..pos + first].copy_from_slice(&data[..first]);
        let rest = data.len() - first;
        if rest > 0 {
            self.buf[..rest].copy_from_slice(&data[first..]);
        }
    }

    /// Append a record, dropping the oldest ones if the ring is full. Returns
    /// its sequence number. Allocation-free and total (never panics).
    pub fn push(&mut self, ts_ms: u32, level: Level, origin: u8, text: &[u8]) -> u32 {
        let text = &text[..text.len().min(MAX_MSG)];
        let need = HDR + text.len();
        while N - self.used < need && self.used > 0 {
            let l = self.rec_len_at(0);
            self.head = Self::wrap(self.head + l);
            self.used -= l;
            self.dropped = self.dropped.wrapping_add(1);
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        let mut hdr = [0u8; HDR];
        hdr[0..4].copy_from_slice(&seq.to_le_bytes());
        hdr[4..8].copy_from_slice(&ts_ms.to_le_bytes());
        hdr[8] = level as u8;
        hdr[9] = origin;
        hdr[10..12].copy_from_slice(&(text.len() as u16).to_le_bytes());
        let tail = Self::wrap(self.head + self.used);
        self.write_at(tail, &hdr);
        self.write_at(Self::wrap(tail + HDR), text);
        self.used += need;
        seq
    }

    /// Copy the used bytes, oldest record first, into `out` and return how many
    /// were copied (`0` if `out` is smaller than [`used`](Self::used)).
    pub fn copy_out(&self, out: &mut [u8]) -> usize {
        if out.len() < self.used {
            return 0;
        }
        let first = (N - self.head).min(self.used);
        out[..first].copy_from_slice(&self.buf[self.head..self.head + first]);
        out[first..self.used].copy_from_slice(&self.buf[..self.used - first]);
        self.used
    }

    /// Visit, oldest first, every record with `seq >= from_seq` and at least
    /// `min` level. The text handed to `f` is only valid for the call (a
    /// wrapped record is rebuilt in a small stack buffer).
    pub fn for_each_since(&self, from_seq: u32, min: Level, mut f: impl FnMut(Entry<'_>)) {
        let mut off = 0;
        let mut tmp = [0u8; MAX_MSG];
        while off + HDR <= self.used {
            let len = self.rec_len_at(off);
            if len < HDR || off + len > self.used {
                break; // corrupt (cannot happen through `push`)
            }
            let mut hdr = [0u8; HDR];
            for (i, b) in hdr.iter_mut().enumerate() {
                *b = self.read_byte(off + i);
            }
            let seq = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]);
            let level = Level::from_u8(hdr[8]);
            // `seq` wraps after 2^32 records; compare in wrapping distance.
            let newer = self.next_seq.wrapping_sub(seq) <= self.next_seq.wrapping_sub(from_seq);
            if newer && level >= min {
                let n = len - HDR;
                for (i, b) in tmp[..n].iter_mut().enumerate() {
                    *b = self.read_byte(off + HDR + i);
                }
                f(Entry {
                    seq,
                    ts_ms: u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]),
                    level,
                    origin: hdr[9],
                    text: &tmp[..n],
                });
            }
            off += len;
        }
    }
}

/// Iterator over the records of a linear snapshot (see [`LogRing::copy_out`]).
pub struct Records<'a> {
    data: &'a [u8],
    off: usize,
}

/// Decode a snapshot. Stops at the first malformed record, never panics.
pub fn records(data: &[u8]) -> Records<'_> {
    Records { data, off: 0 }
}

/// Decode the record at byte offset `off` of a snapshot.
pub fn record_at(data: &[u8], off: usize) -> Option<(Entry<'_>, usize)> {
    let h = data.get(off..off.checked_add(HDR)?)?;
    let len = u16::from_le_bytes([h[10], h[11]]) as usize;
    let text = data.get(off + HDR..off.checked_add(HDR + len)?)?;
    Some((
        Entry {
            seq: u32::from_le_bytes([h[0], h[1], h[2], h[3]]),
            ts_ms: u32::from_le_bytes([h[4], h[5], h[6], h[7]]),
            level: Level::from_u8(h[8]),
            origin: h[9],
            text,
        },
        HDR + len,
    ))
}

impl<'a> Iterator for Records<'a> {
    type Item = Entry<'a>;
    fn next(&mut self) -> Option<Entry<'a>> {
        let (e, n) = record_at(self.data, self.off)?;
        self.off += n;
        Some(e)
    }
}

/// A fixed-capacity `core::fmt::Write` sink that silently cuts overflow, so a
/// message can be formatted without the heap.
pub struct FixedBuf<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> Default for FixedBuf<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> FixedBuf<N> {
    pub const fn new() -> Self {
        Self {
            buf: [0; N],
            len: 0,
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// Append raw bytes, turning control characters (newlines, tabs) into
    /// spaces so the text stays on one line; overflow is cut.
    pub fn push_flat(&mut self, bytes: &[u8]) {
        for &b in bytes.iter().take(N - self.len) {
            self.buf[self.len] = if b < 0x20 { b' ' } else { b };
            self.len += 1;
        }
    }
}

impl<const N: usize> core::fmt::Display for FixedBuf<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        use core::fmt::Write as _;
        // UTF-8 text (the Portuguese UI strings) prints as itself; bytes that are not
        // UTF-8 (a log line in Latin-1, or text cut in the middle of a letter) map 1:1.
        if let Ok(s) = core::str::from_utf8(self.as_bytes()) {
            return f.write_str(s);
        }
        for &c in self.as_bytes() {
            f.write_char(c as char)?;
        }
        Ok(())
    }
}

impl<const N: usize> core::fmt::Write for FixedBuf<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let n = s.len().min(N - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&s.as_bytes()[..n]);
        self.len += n;
        Ok(())
    }
}

/// Milliseconds for a timer tick count at `hz` ticks per second (saturates at
/// `u32::MAX`; the clock wraps after about 49 days of uptime).
pub fn ticks_to_ms(ticks: u64, hz: u32) -> u32 {
    (ticks.saturating_mul(1000) / hz.max(1) as u64).min(u32::MAX as u64) as u32
}

// ------------------------------------------------------------- serial capture

/// Rebuilds lines from the byte stream that goes to the serial port, so every
/// `serial_println!` in the kernel also reaches the ring (see
/// `kernel/src/klog.rs`). Pure: the kernel feeds it and stores what it emits.
pub struct LineAsm {
    buf: [u8; MAX_MSG],
    len: usize,
}

impl Default for LineAsm {
    fn default() -> Self {
        Self::new()
    }
}

impl LineAsm {
    pub const fn new() -> Self {
        Self {
            buf: [0; MAX_MSG],
            len: 0,
        }
    }

    /// Feed bytes; `emit` is called once per finished line (at `\n`, or when
    /// the line reaches [`MAX_MSG`]). `\r` is dropped and empty lines skipped.
    pub fn feed(&mut self, bytes: &[u8], mut emit: impl FnMut(&[u8])) {
        for &b in bytes {
            match b {
                b'\n' => {
                    if self.len > 0 {
                        emit(&self.buf[..self.len]);
                        self.len = 0;
                    }
                }
                b'\r' => {}
                _ => {
                    self.buf[self.len] = b;
                    self.len += 1;
                    if self.len == MAX_MSG {
                        emit(&self.buf[..self.len]);
                        self.len = 0;
                    }
                }
            }
        }
    }
}

/// Severity guessed for a plain serial line, or `None` for lines that must not
/// enter the log (the `[trace]` statistics of perf-trace builds, a per-second
/// flood that would push everything else out of the ring).
pub fn classify(line: &[u8]) -> Option<Level> {
    if line.starts_with(b"[trace]") {
        return None;
    }
    if contains_ci(line, b"FATAL") || contains_ci(line, b"KERNEL PANIC") {
        return Some(Level::Fatal);
    }
    if contains_ci(line, b" died") || contains_ci(line, b"stack overflow") {
        return Some(Level::Error);
    }
    if contains_ci(line, b"failed")
        || contains_ci(line, b"refus")
        || contains_ci(line, b"fallback")
        || contains_ci(line, b"truncated")
        || contains_ci(line, b"expired")
        || contains_ci(line, b"unavailable")
    {
        return Some(Level::Warn);
    }
    Some(Level::Info)
}

// ---------------------------------------------------------------- filter + view

/// What the viewer shows: records at or above `min` whose text contains the
/// needle (ASCII case-insensitive; an empty needle matches everything).
#[derive(Clone, Debug)]
pub struct Filter {
    pub min: Level,
    needle: [u8; 24],
    needle_len: usize,
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
    index: Vec<u32>,
    /// Index of the first visible line.
    top: usize,
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

    fn max_top(&self, rows: usize) -> usize {
        self.index.len().saturating_sub(rows)
    }

    fn clamp(&mut self, rows: usize) {
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

// ------------------------------------------------------------------ formatting

/// Longest [`format_prefix`] output: `"99999.999 I "`.
pub const PREFIX_LEN: usize = 12;

/// Write the compact line prefix `"  12.345 I "` (seconds.millis, level letter,
/// space) into `out` and return its length (always [`PREFIX_LEN`]).
pub fn format_prefix(e: &Entry<'_>, out: &mut [u8; PREFIX_LEN]) -> usize {
    let secs = e.ts_ms / 1000;
    let ms = e.ts_ms % 1000;
    let mut s = [b' '; 5];
    let mut v = secs;
    for slot in s.iter_mut().rev() {
        *slot = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    // Replace leading zeros (except the last digit) with spaces.
    for slot in s.iter_mut().take(4) {
        if *slot == b'0' {
            *slot = b' ';
        } else {
            break;
        }
    }
    out[..5].copy_from_slice(&s);
    out[5] = b'.';
    out[6] = b'0' + (ms / 100) as u8;
    out[7] = b'0' + ((ms / 10) % 10) as u8;
    out[8] = b'0' + (ms % 10) as u8;
    out[9] = b' ';
    out[10] = e.level.letter();
    out[11] = b' ';
    PREFIX_LEN
}

/// Render every record of `snapshot` that passes `filter` as text, one
/// `"<prefix><thread> <text>\n"` line each, into `out`. `thread_name` maps a
/// scheduler slot to its name. Used by "save to file".
pub fn render_text<'n>(
    snapshot: &[u8],
    filter: &Filter,
    thread_name: impl Fn(u8) -> &'n str,
    out: &mut Vec<u8>,
) {
    for e in records(snapshot) {
        if !filter.matches(&e) {
            continue;
        }
        let mut p = [0u8; PREFIX_LEN];
        let n = format_prefix(&e, &mut p);
        out.extend_from_slice(&p[..n]);
        out.extend_from_slice(thread_name(e.origin).as_bytes());
        out.push(b' ');
        out.extend_from_slice(e.text);
        out.push(b'\n');
    }
}

/// The whole log of `snapshot` as text (every level, the same line format as "save
/// to file"), cut to its newest whole lines if it exceeds `cap` bytes. Returns the
/// text and whether older lines were dropped. This is what the boot-time flush
/// writes to `/var/log/boot.log`: the result never exceeds `cap`.
pub fn dump_bounded<'n>(
    snapshot: &[u8],
    thread_name: impl Fn(u8) -> &'n str,
    cap: usize,
) -> (Vec<u8>, bool) {
    let mut text = Vec::new();
    render_text(snapshot, &Filter::new(), thread_name, &mut text);
    let (body, cut) = tail_lines(&text, cap);
    (body.to_vec(), cut)
}

/// The newest part of a text dump that fits `cap` bytes, starting at a line
/// start (so no line is cut in half), and whether anything was dropped. For
/// file systems with a small file size limit.
pub fn tail_lines(data: &[u8], cap: usize) -> (&[u8], bool) {
    if data.len() <= cap {
        return (data, false);
    }
    let mut start = data.len() - cap;
    // Skip to just after the next newline unless the cut already fell on one.
    if data[start - 1] != b'\n' {
        match data[start..].iter().position(|&b| b == b'\n') {
            Some(i) => start += i + 1,
            None => start = data.len(),
        }
    }
    (&data[start..], true)
}

#[cfg(test)]
mod tests;
