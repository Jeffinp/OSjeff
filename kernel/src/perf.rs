//! Performance metrics + an on-screen HUD (FPS, frame time, heap, threads).
//!
//! Frame time needs real time units, but the kernel never calibrated the TSC, so
//! [`calibrate_khz`] measures the CPU's cycle rate against the PIT once at boot.
//! Everything else is cheap counters updated from the compositor loop.

use crate::fb::{Canvas, Color};
use crate::{font, interrupts, io, theme};
use osjeff_core::hw::perf::{LINE_LEN, Perf as CorePerf, heap_line, khz_from_calibration};

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
    khz_from_calibration(cycles, 25, interrupts::TIMER_HZ as u64)
}

/// Rolling compositor metrics (the arithmetic lives in `osjeff_core::hw::perf`).
pub struct Perf(CorePerf);

impl Perf {
    pub fn new(khz: u64) -> Self {
        Self(CorePerf::new(khz))
    }

    /// Record one rendered frame given its duration in TSC cycles.
    pub fn record(&mut self, tsc_delta: u64) {
        self.0.record(tsc_delta);
    }

    /// Roll the per-second counters (call on each wall-clock second).
    pub fn second_tick(&mut self) {
        self.0.second_tick();
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

        let mut line = [0u8; LINE_LEN];
        // "0.2ms  ~5000fps" — frame time is the real smoothness metric; the
        // "possible fps" (1000/ms) shows the headroom even when idle.
        let n = self.0.frame_line(&mut line);
        text(c, x + 10, y + 8, &line[..n], theme::accent());

        // "draws/s 60 0.4ms" — how often we actually redraw (= activity,
        // low when idle by design) and the worst frame this second.
        let n = self.0.draws_line(&mut line);
        text(c, x + 10, y + 24, &line[..n], theme::HEADER_TEXT);

        // "heap 12%  thr 3"
        let n = heap_line(&mut line, heap_pct, threads);
        text(c, x + 10, y + 40, &line[..n], theme::TEXT_MUTED);
    }
}

const HUD_W: i32 = 212;
const HUD_H: i32 = 60;

fn text(c: &mut Canvas, x: usize, y: usize, bytes: &[u8], color: Color) {
    // The buffer is always ASCII we built ourselves.
    // SAFETY: `bytes` is a prefix of `line`, built only from ASCII literals and decimal digits
    // (`put`/`put_u32`/`put_ms`); truncation cannot split a multi-byte char, so it is valid UTF-8.
    let s = unsafe { core::str::from_utf8_unchecked(bytes) };
    font::draw_text(c, x, y, s, color, 2);
}
