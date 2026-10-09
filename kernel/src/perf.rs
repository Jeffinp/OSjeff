//! Performance metrics + an on-screen HUD (FPS, frame time, heap, threads).
//!
//! Frame time needs real time units, but the kernel never calibrated the TSC, so
//! [`calibrate_khz`] measures the CPU's cycle rate against the PIT once at boot.
//! Everything else is cheap counters updated from the compositor loop.

use crate::fb::{Canvas, Color};
use crate::{interrupts, io, theme};
use kitsune_core::hw::perf::{LINE_LEN, Perf as CorePerf, heap_line, khz_from_calibration};

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

/// Rolling compositor metrics (the arithmetic lives in `kitsune_core::hw::perf`).
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

    /// `(frames drawn in the last second, last frame time in microseconds, worst
    /// frame time this second)`: what the resource monitor graphs.
    pub fn stats(&self) -> (u32, u64, u64) {
        (self.0.fps, self.0.frame_us, self.0.max_us)
    }

    /// Screen rect of the HUD panel (top-left, under the menu bar).
    pub fn rect(_width: i32) -> kitsune_core::Rect {
        kitsune_core::Rect::new(
            12,
            kitsune_core::window::MENUBAR_H + 10,
            HUD_W + 16,
            HUD_H + 16,
        )
    }

    /// Draw the HUD panel and its metric lines. `heap_pct` is heap usage 0..100.
    pub fn draw(&self, c: &mut Canvas, heap_pct: u32, threads: usize) {
        use crate::fb::Corner;
        let r = Self::rect(c.width() as i32);
        let panel = kitsune_core::Rect::new(r.x + 8, r.y + 4, HUD_W, HUD_H);
        let hole = kitsune_core::Rect::new(panel.x, panel.y + 10, panel.w, panel.h - 20);
        c.draw_shadow(
            panel,
            crate::fb::Shadow {
                blur: 8,
                dy: 4,
                alpha: 70,
            },
            hole,
        );
        c.fill_rrect(panel, 10, Corner::Circle, Color::rgb(0x16, 0x16, 0x1A), 235);
        c.stroke_rrect(panel, 10, Corner::Circle, Color::rgb(0xFF, 0xFF, 0xFF), 30);
        let (x, y) = (panel.x + 12, panel.y + 8);

        let mut line = [0u8; LINE_LEN];
        // "0.2ms  ~5000fps" — frame time is the real smoothness metric; the
        // "possible fps" (1000/ms) shows the headroom even when idle.
        let n = self.0.frame_line(&mut line);
        text(
            c,
            x,
            y,
            &line[..n],
            theme::accent().lerp(Color::rgb(255, 255, 255), 90),
        );

        // "draws/s 60 0.4ms" — how often we actually redraw (= activity,
        // low when idle by design) and the worst frame this second.
        let n = self.0.draws_line(&mut line);
        text(c, x, y + 20, &line[..n], Color::rgb(0xF5, 0xF5, 0xF7));

        // "heap 12%  thr 3"
        let n = heap_line(&mut line, heap_pct, threads);
        text(c, x, y + 40, &line[..n], Color::rgb(0xA1, 0xA1, 0xA6));
    }
}

const HUD_W: i32 = 232;
const HUD_H: i32 = 68;

fn text(c: &mut Canvas, x: i32, y: i32, bytes: &[u8], color: Color) {
    // The buffer is always ASCII we built ourselves.
    // SAFETY: `bytes` is a prefix of `line`, built only from ASCII literals and decimal digits
    // (`put`/`put_u32`/`put_ms`); truncation cannot split a multi-byte char, so it is valid UTF-8.
    let s = unsafe { core::str::from_utf8_unchecked(bytes) };
    crate::text::draw_mono(c, x, y, s, 13, color);
}
