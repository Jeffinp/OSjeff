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
        // Only ASCII is ever formatted into these buffers; map bytes 1:1.
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
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use alloc::string::String;
    use alloc::vec;
    use core::fmt::Write;

    type Ring = LogRing<512>;

    fn snap<const N: usize>(r: &LogRing<N>) -> Vec<u8> {
        let mut v = vec![0u8; r.used()];
        assert_eq!(r.copy_out(&mut v), r.used());
        v
    }

    #[test]
    fn push_and_read_back() {
        let mut r = Ring::new();
        assert_eq!(r.push(10, Level::Info, 0, b"hello"), 0);
        assert_eq!(r.push(20, Level::Error, 2, b"world!"), 1);
        let s = snap(&r);
        let v: Vec<_> = records(&s).collect();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].text, b"hello");
        assert_eq!((v[0].ts_ms, v[0].level, v[0].origin), (10, Level::Info, 0));
        assert_eq!(v[1].text, b"world!");
        assert_eq!(v[1].level, Level::Error);
        assert_eq!(r.next_seq(), 2);
    }

    #[test]
    fn long_text_is_cut() {
        let mut r = Ring::new();
        r.push(0, Level::Info, 0, &[b'x'; 500]);
        let s = snap(&r);
        assert_eq!(records(&s).next().unwrap().text.len(), MAX_MSG);
    }

    #[test]
    fn oldest_records_are_dropped_and_order_is_kept() {
        let mut r = Ring::new();
        for i in 0..100u32 {
            let mut m = String::new();
            let _ = write!(m, "message number {i}");
            r.push(i, Level::Info, 0, m.as_bytes());
        }
        assert!(r.dropped() > 0);
        let s = snap(&r);
        let v: Vec<_> = records(&s).collect();
        // Strictly consecutive sequence numbers ending at the newest.
        for w in v.windows(2) {
            assert_eq!(w[1].seq, w[0].seq + 1);
        }
        assert_eq!(v.last().unwrap().seq, 99);
        assert_eq!(r.dropped() as usize + v.len(), 100);
        for e in &v {
            let mut m = String::new();
            let _ = write!(m, "message number {}", e.seq);
            assert_eq!(e.text, m.as_bytes());
        }
    }

    #[test]
    fn hundred_thousand_messages_never_corrupt_the_ring() {
        // A 64 KiB ring like the kernel's, hammered with variable-size texts
        // that straddle the wrap point many times over.
        let mut r: Box<LogRing<RING_BYTES>> = Box::default();
        let mut seed = 0x1234_5678u32;
        for i in 0..100_000u32 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let len = (seed >> 24) as usize % 120;
            let mut text = vec![b'a' + (i % 26) as u8; len];
            if len >= 4 {
                text[..4].copy_from_slice(&i.to_le_bytes());
            }
            r.push(i, Level::from_u8((i % 6) as u8), (i % 8) as u8, &text);
            // Spot-check the invariants cheaply, and fully now and then.
            if i % 9973 == 0 {
                let s = snap(&*r);
                let mut prev: Option<u32> = None;
                let mut bytes = 0;
                for e in records(&s) {
                    if let Some(p) = prev {
                        assert_eq!(e.seq, p + 1);
                    }
                    prev = Some(e.seq);
                    assert_eq!(e.ts_ms, e.seq);
                    if e.text.len() >= 4 {
                        assert_eq!(&e.text[..4], &e.seq.to_le_bytes());
                    }
                    bytes += HDR + e.text.len();
                }
                assert_eq!(bytes, s.len());
                assert_eq!(prev, Some(i));
            }
        }
        assert_eq!(r.next_seq(), 100_000);
        let s = snap(&*r);
        assert_eq!(records(&s).last().unwrap().seq, 99_999);
        assert_eq!(r.dropped() as usize + records(&s).count(), 100_000);
    }

    #[test]
    fn copy_out_needs_room() {
        let mut r = Ring::new();
        r.push(0, Level::Info, 0, b"abc");
        let mut small = [0u8; 4];
        assert_eq!(r.copy_out(&mut small), 0);
    }

    #[test]
    fn clear_empties_but_keeps_counting() {
        let mut r = Ring::new();
        r.push(0, Level::Info, 0, b"a");
        r.clear();
        assert_eq!(r.used(), 0);
        assert_eq!(r.push(0, Level::Info, 0, b"b"), 1);
        let s = snap(&r);
        assert_eq!(records(&s).count(), 1);
    }

    #[test]
    fn for_each_since_filters_by_seq_and_level() {
        let mut r = Ring::new();
        for i in 0..10u32 {
            r.push(i, Level::from_u8((i % 6) as u8), 0, b"m");
        }
        let mut got = Vec::new();
        r.for_each_since(5, Level::Warn, |e| got.push((e.seq, e.level)));
        assert_eq!(got, vec![(5, Level::Fatal), (9, Level::Warn)]);
    }

    #[test]
    fn for_each_since_sees_wrapped_records() {
        let mut r: LogRing<256> = LogRing::new();
        for i in 0..40u32 {
            r.push(i, Level::Info, 0, b"wrapping text");
        }
        let mut n = 0;
        r.for_each_since(0, Level::Trace, |e| {
            assert_eq!(e.text, b"wrapping text");
            n += 1;
        });
        assert_eq!(n as u32 + r.dropped(), 40);
    }

    #[test]
    fn level_helpers() {
        assert!(Level::Warn > Level::Info);
        assert_eq!(Level::from_u8(99), Level::Fatal);
        assert_eq!(Level::Fatal.next(), Level::Fatal);
        assert_eq!(Level::Info.tag(), "INFO ");
        for (i, l) in Level::ALL.iter().enumerate() {
            assert_eq!(*l as usize, i);
        }
    }

    #[test]
    fn contains_ci_cases() {
        assert!(contains_ci(b"Hello World", b"o w"));
        assert!(contains_ci(b"Hello", b""));
        assert!(contains_ci(b"TSC calibrated", b"tsc"));
        assert!(!contains_ci(b"abc", b"abcd"));
        assert!(!contains_ci(b"abc", b"x"));
    }

    #[test]
    fn filter_level_and_text() {
        let mut r = Ring::new();
        r.push(0, Level::Info, 0, b"net: lease");
        r.push(1, Level::Warn, 0, b"disk slow");
        r.push(2, Level::Error, 0, b"net: down");
        let s = snap(&r);
        let mut f = Filter::new();
        let mut v = LogView::new();
        v.rebuild(&s, &f, 10);
        assert_eq!(v.len(), 3);
        f.min = Level::Warn;
        v.rebuild(&s, &f, 10);
        assert_eq!(v.len(), 2);
        for c in b"NET" {
            assert!(f.push_char(*c));
        }
        v.rebuild(&s, &f, 10);
        assert_eq!(v.len(), 1);
        assert_eq!(v.visible(&s, 10).next().unwrap().text, b"net: down");
        assert!(f.backspace());
        assert_eq!(f.needle(), b"NE");
        f.clear_needle();
        assert!(!f.backspace());
        f.min = Level::Fatal;
        f.cycle_level();
        assert_eq!(f.min, Level::Trace);
    }

    #[test]
    fn filter_rejects_control_and_overflow() {
        let mut f = Filter::new();
        assert!(!f.push_char(7));
        assert!(!f.push_char(0x80));
        for _ in 0..24 {
            assert!(f.push_char(b'a'));
        }
        assert!(!f.push_char(b'a'));
    }

    #[test]
    fn view_follows_and_scrolls() {
        let mut r = Ring::new();
        for i in 0..20u32 {
            r.push(i, Level::Info, 0, b"line");
        }
        let s = snap(&r);
        let f = Filter::new();
        let mut v = LogView::new();
        v.rebuild(&s, &f, 5);
        assert_eq!(v.top(), 15); // following: bottom
        v.scroll(-3, 5);
        assert_eq!(v.top(), 12);
        assert!(!v.follow);
        // New data does not move a scrolled-up view.
        r.push(99, Level::Info, 0, b"new");
        let s = snap(&r);
        v.rebuild(&s, &f, 5);
        assert_eq!(v.top(), 12);
        v.scroll(100, 5);
        assert!(v.follow);
        assert_eq!(v.top(), v.len() - 5);
        v.home();
        assert_eq!(v.top(), 0);
        v.end(5);
        assert!(v.follow);
        let vis: Vec<_> = v.visible(&s, 5).collect();
        assert_eq!(vis.len(), 5);
        assert_eq!(vis.last().unwrap().text, b"new");
    }

    #[test]
    fn view_with_fewer_lines_than_rows() {
        let mut r = Ring::new();
        r.push(0, Level::Info, 0, b"only");
        let s = snap(&r);
        let mut v = LogView::new();
        v.rebuild(&s, &Filter::new(), 8);
        assert_eq!(v.top(), 0);
        v.scroll(5, 8);
        assert_eq!(v.top(), 0);
        v.scroll(-5, 8);
        assert_eq!(v.top(), 0);
    }

    #[test]
    fn prefix_format() {
        let e = Entry {
            seq: 0,
            ts_ms: 12_345,
            level: Level::Warn,
            origin: 0,
            text: b"",
        };
        let mut p = [0u8; PREFIX_LEN];
        let n = format_prefix(&e, &mut p);
        assert_eq!(&p[..n], b"   12.345 W ");
        let e = Entry { ts_ms: 7, ..e };
        format_prefix(&e, &mut p);
        assert_eq!(&p[..], b"    0.007 W ");
        let e = Entry {
            ts_ms: 99_999_999,
            ..e
        };
        format_prefix(&e, &mut p);
        assert_eq!(&p[..], b"99999.999 W ");
    }

    #[test]
    fn render_text_dump() {
        let mut r = Ring::new();
        r.push(1500, Level::Info, 0, b"boot");
        r.push(2500, Level::Debug, 1, b"noise");
        let s = snap(&r);
        let mut f = Filter::new();
        f.min = Level::Info;
        let mut out = Vec::new();
        render_text(
            &s,
            &f,
            |o| if o == 0 { "kernel" } else { "other" },
            &mut out,
        );
        assert_eq!(out, b"    1.500 I kernel boot\n");
    }

    #[test]
    fn fixed_buf_cuts_silently() {
        let mut b: FixedBuf<8> = FixedBuf::new();
        let _ = write!(b, "{}-{}", String::from("abcdef"), String::from("ghijkl"));
        assert_eq!(b.as_bytes(), b"abcdef-g");
    }

    #[test]
    fn push_flat_removes_newlines_and_cuts() {
        let mut b: FixedBuf<10> = FixedBuf::new();
        b.push_flat(b"a\nb\tc");
        b.push_flat(b"\r\n12345678");
        assert_eq!(b.as_bytes(), b"a b c  123");
        assert_eq!(b.as_bytes().len(), 10);
    }

    #[test]
    fn ticks_convert() {
        assert_eq!(ticks_to_ms(250, 250), 1000);
        assert_eq!(ticks_to_ms(1, 250), 4);
        assert_eq!(ticks_to_ms(u64::MAX, 250), u32::MAX);
    }

    #[test]
    fn line_assembler_splits_and_cuts() {
        let mut a = LineAsm::new();
        let mut got: Vec<Vec<u8>> = Vec::new();
        a.feed(b"hello\r\nwor", |l| got.push(l.to_vec()));
        a.feed(b"ld\n\n\nx", |l| got.push(l.to_vec()));
        assert_eq!(got, vec![b"hello".to_vec(), b"world".to_vec()]);
        let mut long = Vec::new();
        a.feed(&[b'y'; 450], |l| long.push(l.len()));
        assert_eq!(long, vec![MAX_MSG, MAX_MSG]);
    }

    #[test]
    fn classify_lines() {
        assert_eq!(classify(b"[trace] frame stats"), None);
        assert_eq!(classify(b"TSC calibrated: 1000 kHz"), Some(Level::Info));
        assert_eq!(classify(b"thread 'fetcher' died: x"), Some(Level::Error));
        assert_eq!(classify(b"KERNEL PANIC: boom"), Some(Level::Fatal));
        assert_eq!(classify(b"FATAL EXCEPTION #GP"), Some(Level::Fatal));
        assert_eq!(
            classify(b"net: static fallback (no DHCP offer)"),
            Some(Level::Warn)
        );
        assert_eq!(
            classify(b"OJFS: disk read failed; RAM-only"),
            Some(Level::Warn)
        );
        assert_eq!(classify(b"first desktop frame"), Some(Level::Info));
    }

    #[test]
    fn tail_keeps_whole_lines() {
        let d = b"aaaa\nbbbb\ncccc\n";
        assert_eq!(tail_lines(d, 100), (&d[..], false));
        // The cut lands exactly on a line start: that line is kept.
        assert_eq!(tail_lines(d, 10), (&b"bbbb\ncccc\n"[..], true));
        // The cut lands inside "bbbb": skip to the next line.
        assert_eq!(tail_lines(d, 9), (&b"cccc\n"[..], true));
        assert_eq!(tail_lines(d, 5), (&b"cccc\n"[..], true));
        assert_eq!(tail_lines(d, 4), (&b""[..], true));
        assert_eq!(tail_lines(b"no newline at all", 4), (&b""[..], true));
    }

    #[test]
    fn view_top_for_and_set_top() {
        let mut r = Ring::new();
        for i in 0..20u32 {
            r.push(i, Level::Info, 0, b"line");
        }
        let s = snap(&r);
        let mut v = LogView::new();
        v.rebuild(&s, &Filter::new(), 5);
        assert_eq!(v.top_for(5), 15);
        // A taller window (more rows) while following: still the newest page.
        assert_eq!(v.top_for(10), 10);
        assert_eq!(v.visible(&s, 10).count(), 10);
        v.set_top(3, 5);
        assert!(!v.follow);
        assert_eq!(v.top_for(5), 3);
        assert_eq!(v.top_for(19), 1); // clamped for a very tall window
        v.set_top(999, 5);
        assert!(v.follow);
        assert_eq!(v.top_for(5), 15);
    }

    #[test]
    fn record_at_rejects_truncated_data() {
        let mut r = Ring::new();
        r.push(0, Level::Info, 0, b"abcdef");
        let s = snap(&r);
        assert!(record_at(&s[..s.len() - 1], 0).is_none());
        assert!(record_at(&s, usize::MAX).is_none());
        assert!(record_at(&[], 0).is_none());
    }
}
