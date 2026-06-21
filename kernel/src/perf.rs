//! Performance metrics + an on-screen HUD (FPS, frame time, heap, threads).
//!
//! Frame time needs real time units, but the kernel never calibrated the TSC, so
//! [`calibrate_khz`] measures the CPU's cycle rate against the PIT once at boot.
//! Everything else is cheap counters updated from the compositor loop.

use crate::fb::{Canvas, Color};
use crate::{font, interrupts, io, theme};

/// Measure the TSC frequency (in kHz = cycles/ms) by counting cycles across a
/// known number of PIT ticks. Requires the timer to be running.
pub fn calibrate_khz() -> u64 {
    let t0 = interrupts::ticks();
    while interrupts::ticks() == t0 {} // align to a tick edge
    let start_tick = interrupts::ticks();
    let start = io::rdtsc();
    // 25 ticks at TIMER_HZ. Span in ms = 25 / TIMER_HZ * 1000.
    while interrupts::ticks() < start_tick + 25 {}
    let cycles = io::rdtsc().wrapping_sub(start);
    let span_ms = 25u64 * 1000 / interrupts::TIMER_HZ as u64;
    (cycles / span_ms).max(1)
}

/// Rolling compositor metrics.
pub struct Perf {
    khz: u64,          // TSC cycles per millisecond
    pub frame_us: u64, // last render duration, microseconds
    pub max_us: u64,   // worst render this second
    pub fps: u32,      // renders in the last full second
    count: u32,        // renders so far this second
}

impl Perf {
    pub fn new(khz: u64) -> Self {
        Self {
            khz: khz.max(1),
            frame_us: 0,
            max_us: 0,
            fps: 0,
            count: 0,
        }
    }

    /// Record one rendered frame given its duration in TSC cycles.
    pub fn record(&mut self, tsc_delta: u64) {
        self.frame_us = tsc_delta.saturating_mul(1000) / self.khz;
        self.max_us = self.max_us.max(self.frame_us);
        self.count += 1;
    }

    /// Roll the per-second counters (call on each wall-clock second).
    pub fn second_tick(&mut self) {
        self.fps = self.count;
        self.count = 0;
        self.max_us = self.frame_us;
    }

    /// Screen rect of the HUD panel (top-right corner).
    pub fn rect(width: i32) -> osjeff_core::Rect {
        osjeff_core::Rect::new(width - HUD_W - 12, 12, HUD_W, HUD_H)
    }

    /// Draw the HUD panel and its metric lines. `heap_pct` is heap usage 0..100.
    pub fn draw(&self, c: &mut Canvas, heap_pct: u32, threads: usize) {
        let r = Self::rect(c.width() as i32);
        let (x, y) = (r.x as usize, r.y as usize);
        c.fill_round_rect_alpha(
            x,
            y + 4,
            HUD_W as usize,
            HUD_H as usize,
            8,
            theme::SHADOW,
            60,
        );
        c.fill_round_rect(x, y, HUD_W as usize, HUD_H as usize, 8, theme::DOCK);

        let mut line = [0u8; 32];
        // "FPS 60  4.2ms"
        let mut n = put(&mut line, 0, b"FPS ");
        n = put_u32(&mut line, n, self.fps);
        n = put(&mut line, n, b"  ");
        n = put_ms(&mut line, n, self.frame_us);
        text(c, x + 10, y + 8, &line[..n], theme::ACCENT);

        // "max 9.1ms"
        let mut n = put(&mut line, 0, b"max ");
        n = put_ms(&mut line, n, self.max_us);
        text(c, x + 10, y + 24, &line[..n], theme::HEADER_TEXT);

        // "heap 12%  thr 3"
        let mut n = put(&mut line, 0, b"heap ");
        n = put_u32(&mut line, n, heap_pct);
        n = put(&mut line, n, b"%  thr ");
        n = put_u32(&mut line, n, threads as u32);
        text(c, x + 10, y + 40, &line[..n], theme::TEXT_MUTED);
    }
}

const HUD_W: i32 = 212;
const HUD_H: i32 = 60;

fn text(c: &mut Canvas, x: usize, y: usize, bytes: &[u8], color: Color) {
    // The buffer is always ASCII we built ourselves.
    let s = unsafe { core::str::from_utf8_unchecked(bytes) };
    font::draw_text(c, x, y, s, color, 2);
}

/// Append raw bytes, return new position.
fn put(buf: &mut [u8], pos: usize, src: &[u8]) -> usize {
    let n = src.len().min(buf.len() - pos);
    buf[pos..pos + n].copy_from_slice(&src[..n]);
    pos + n
}

/// Append a decimal u32, return new position.
fn put_u32(buf: &mut [u8], pos: usize, v: u32) -> usize {
    if v == 0 {
        return put(buf, pos, b"0");
    }
    let mut digits = [0u8; 10];
    let mut i = 0;
    let mut x = v;
    while x > 0 {
        digits[i] = b'0' + (x % 10) as u8;
        x /= 10;
        i += 1;
    }
    let mut p = pos;
    while i > 0 && p < buf.len() {
        i -= 1;
        buf[p] = digits[i];
        p += 1;
    }
    p
}

/// Append microseconds as "M.mms" with one decimal of milliseconds.
fn put_ms(buf: &mut [u8], pos: usize, us: u64) -> usize {
    let tenths = (us / 100) as u32; // milliseconds * 10
    let mut p = put_u32(buf, pos, tenths / 10);
    p = put(buf, p, b".");
    p = put_u32(buf, p, tenths % 10);
    put(buf, p, b"ms")
}
