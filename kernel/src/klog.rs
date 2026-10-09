//! System log: the kernel end of `kitsune_core::klog`.
//!
//! One static ring of [`RING_BYTES`] (64 KiB, BSS) holds the last messages with
//! level, millisecond timestamp and the scheduler slot of the thread that logged
//! them. [`klog!`] writes a message to the ring **and** to COM1, byte for byte
//! what `serial_println!` printed before, so scripts that grep the serial log
//! keep working; and every plain `serial_println!` in the kernel (drivers owned
//! by other fronts included) is mirrored into the ring too, through
//! [`capture`], which `serial::write_str` calls.
//!
//! # Interrupt safety
//!
//! A write never allocates and never takes the heap lock: the message is
//! formatted into a 200-byte stack buffer, and the ring is touched only inside
//! [`with_ring`], a short critical section with interrupts off (the kernel is
//! single core, so that is mutual exclusion: no lock to spin on, and an ISR
//! that logs cannot deadlock against the thread it interrupted). The slow part,
//! the UART, runs *outside* the critical section. The ring write itself is a few
//! `memcpy`s of at most 212 bytes (`kitsune_core::klog::LogRing::push`, tested
//! with 100 000 messages over a 64 KiB ring).
//!
//! The panic / fatal-exception path does not depend on any of this: `crash`
//! freezes the capture first and prints through the UART only.

use crate::sync::RacyCell;
use alloc::vec::Vec;
use core::fmt::{self, Write as _};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
pub use kitsune_core::klog::Level;
use kitsune_core::klog::{FixedBuf, LineAsm, LogRing, MAX_MSG, RING_BYTES, classify, ticks_to_ms};

static RING: RacyCell<LogRing<RING_BYTES>> = RacyCell::new(LogRing::new());
static LINE: RacyCell<LineAsm> = RacyCell::new(LineAsm::new());
/// Sequence number the next record will get (mirror of the ring's, readable
/// without entering the critical section).
static SEQ: AtomicU32 = AtomicU32::new(0);
/// `1 + seq` of the newest record at WARN or above (0 = none yet): lets the
/// toast overlay notice a warning with one atomic load per pass.
static WARN_SEQ: AtomicU32 = AtomicU32::new(0);
/// Mirror serial output into the ring. Cleared by [`freeze`] when the kernel is dying.
static CAPTURE: AtomicBool = AtomicBool::new(true);

/// Run `f` on the ring with interrupts off.
fn with_ring<R>(f: impl FnOnce(&mut LogRing<RING_BYTES>) -> R) -> R {
    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: single core and interrupts are off for the whole closure, so no other thread or
        // ISR can touch RING meanwhile and this is the only live reference.
        f(unsafe { &mut *RING.get() })
    })
}

/// `1 + seq` of the last few WARN-or-above records that already have a toast of their own (see
/// [`log_toasted`]): the toast watcher skips them instead of showing the log text twice.
static TOASTED: [AtomicU32; 4] = [
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
];
static TOASTED_NEXT: AtomicU32 = AtomicU32::new(0);

/// Store one record in `ring` and publish the counters. Caller holds the ring. Returns its
/// sequence number.
fn record(ring: &mut LogRing<RING_BYTES>, level: Level, text: &[u8]) -> u32 {
    let ts = ticks_to_ms(crate::interrupts::ticks(), crate::interrupts::TIMER_HZ);
    let origin = u8::try_from(crate::sched::current()).unwrap_or(255);
    let seq = ring.push(ts, level, origin, text);
    SEQ.store(seq.wrapping_add(1), Ordering::Release);
    if level >= Level::Warn {
        WARN_SEQ.store(seq.wrapping_add(1), Ordering::Release);
    }
    seq
}

/// Sink for `core::fmt` that mirrors every piece to the UART (exactly as
/// `serial_println!` did) and keeps the first [`MAX_MSG`] bytes for the ring.
struct Tee {
    buf: FixedBuf<MAX_MSG>,
}

impl fmt::Write for Tee {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        crate::serial::write_str_raw(s);
        // The ring keeps one line per record: a message that contains newlines
        // (a pretty-printed `{:#?}`) becomes one line with spaces.
        self.buf.push_flat(s.as_bytes());
        Ok(())
    }
}

/// Log one message: the serial port gets `args` plus a newline (the same bytes
/// `serial_println!` printed), the ring gets a record of `level`. Prefer the
/// [`klog!`] macro.
pub fn log(level: Level, args: fmt::Arguments<'_>) {
    let mut t = Tee {
        buf: FixedBuf::new(),
    };
    let _ = t.write_fmt(args);
    crate::serial::write_str_raw("\n");
    with_ring(|r| record(r, level, t.buf.as_bytes()));
}

/// [`log`] for a record that already has a toast of its own, shown by `notify::notify_key` in
/// the language of the interface: the serial port and the ring get `args` as usual, and the
/// toast watcher skips the record instead of showing the English text a second time. For a
/// level below WARN there is nothing to skip.
pub fn log_toasted(level: Level, args: fmt::Arguments<'_>) {
    let mut t = Tee {
        buf: FixedBuf::new(),
    };
    let _ = t.write_fmt(args);
    crate::serial::write_str_raw("\n");
    // Record and mark inside one critical section, so the watcher never sees one without the other.
    with_ring(|r| {
        let seq = record(r, level, t.buf.as_bytes());
        if level >= Level::Warn {
            let slot = TOASTED_NEXT.fetch_add(1, Ordering::Relaxed) as usize % TOASTED.len();
            TOASTED[slot].store(seq.wrapping_add(1), Ordering::Release);
        }
    });
}

fn is_toasted(seq: u32) -> bool {
    let tag = seq.wrapping_add(1);
    TOASTED.iter().any(|t| t.load(Ordering::Acquire) == tag)
}

/// Log to the ring only (no serial). For events that must be visible in the
/// viewer but are already reported elsewhere.
pub fn log_quiet(level: Level, args: fmt::Arguments<'_>) {
    let mut buf: FixedBuf<MAX_MSG> = FixedBuf::new();
    let _ = buf.write_fmt(args);
    with_ring(|r| record(r, level, buf.as_bytes()));
}

/// `klog!(Info, "text {}", x)`: write to the system log and to COM1.
///
/// Levels: `Trace`, `Debug`, `Info`, `Warn`, `Error`, `Fatal`.
#[macro_export]
macro_rules! klog {
    ($lvl:ident, $($arg:tt)*) => {
        $crate::klog::log($crate::klog::Level::$lvl, format_args!($($arg)*))
    };
}

/// Mirror serial output into the ring (called by `serial::write_str` for every
/// piece written through `serial_println!` / `serial_print!`). Lines are
/// reassembled and classified by `kitsune_core::klog::classify`.
pub fn capture(s: &str) {
    if !CAPTURE.load(Ordering::Relaxed) {
        return;
    }
    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: interrupts are off on a single core: no other access to LINE or RING is in
        // flight, so these are the only live references.
        let (line, ring) = unsafe { (&mut *LINE.get(), &mut *RING.get()) };
        line.feed(s.as_bytes(), |l| {
            if let Some(level) = classify(l) {
                record(ring, level, l);
            }
        });
    });
}

/// Stop mirroring the serial port into the ring: the kernel is about to report
/// a fatal error through the UART alone.
pub fn freeze() {
    CAPTURE.store(false, Ordering::Relaxed);
}

/// Records pushed so far (also the sequence number of the next one).
pub fn seq() -> u32 {
    SEQ.load(Ordering::Acquire)
}

/// Empty the ring (the viewer's "clear"; the sequence keeps counting).
pub fn clear() {
    with_ring(LogRing::clear);
}

/// Copy the ring, oldest record first, into `out` (replacing its content).
/// The allocation happens before the critical section; `out` only grows
/// to [`RING_BYTES`] once.
pub fn snapshot(out: &mut Vec<u8>) {
    if out.capacity() < RING_BYTES {
        let _ = out.try_reserve_exact(RING_BYTES - out.len());
    }
    out.clear();
    if out.capacity() < RING_BYTES {
        return;
    }
    out.resize(RING_BYTES, 0);
    let n = with_ring(|r| r.copy_out(out));
    out.truncate(n);
}

/// Milliseconds since boot (4 ms resolution), the clock log records use.
pub fn ticks_to_ms_now() -> u32 {
    ticks_to_ms(crate::interrupts::ticks(), crate::interrupts::TIMER_HZ)
}

/// `1 + seq` of the newest WARN-or-above record, 0 if there is none.
pub fn warn_seq() -> u32 {
    WARN_SEQ.load(Ordering::Acquire)
}

/// A WARN-or-above message seen by [`take_warnings`].
#[derive(Clone, Copy)]
pub struct Warning {
    pub level: Level,
    text: [u8; 64],
    len: u8,
}

impl Warning {
    pub fn text(&self) -> &[u8] {
        &self.text[..self.len as usize]
    }
}

/// Collect (up to `out.len()`) WARN-or-above records newer than `*seen`
/// (advancing it to the current sequence) and return how many. One atomic load
/// when nothing new has been logged.
pub fn take_warnings(seen: &mut u32, out: &mut [Option<Warning>]) -> usize {
    let w = warn_seq();
    // A warning exists since `seen` iff its `1 + seq` is above `seen`.
    if w == 0 || (w.wrapping_sub(*seen) as i32) <= 0 {
        return 0;
    }
    let from = *seen;
    let mut n = 0;
    with_ring(|r| {
        r.for_each_since(from, Level::Warn, |e| {
            if n < out.len() && !is_toasted(e.seq) {
                let mut text = [0u8; 64];
                let l = e.text.len().min(64);
                text[..l].copy_from_slice(&e.text[..l]);
                out[n] = Some(Warning {
                    level: e.level,
                    text,
                    len: l as u8,
                });
                n += 1;
            }
        });
        *seen = r.next_seq();
    });
    n
}
