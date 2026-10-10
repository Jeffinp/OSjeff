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

mod capture;
mod format;
mod view;
pub use capture::*;
pub use format::*;
pub use view::*;

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

// ---------------------------------------------------------------- filter + view

// ------------------------------------------------------------------ formatting

#[cfg(test)]
mod tests;
