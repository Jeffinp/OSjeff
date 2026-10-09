//! Compositor performance statistics and the HUD's text formatting.
//!
//! The kernel measures raw TSC cycles; this module converts them to
//! microseconds, keeps the rolling per-second counters and formats the HUD
//! lines into small fixed buffers (no allocation, never panics on overflow).

/// Size of one HUD text line buffer.
pub const LINE_LEN: usize = 32;

/// TSC frequency in kHz (= cycles per millisecond) from a calibration run:
/// `cycles` elapsed while `ticks` PIT ticks at `timer_hz` went by. Never 0.
pub fn khz_from_calibration(cycles: u64, ticks: u64, timer_hz: u64) -> u64 {
    let span_ms = (ticks * 1000 / timer_hz.max(1)).max(1);
    (cycles / span_ms).max(1)
}

/// Rolling compositor metrics.
#[derive(Clone, Copy, Debug)]
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
        self.count = self.count.saturating_add(1);
    }

    /// Roll the per-second counters (call on each wall-clock second).
    pub fn second_tick(&mut self) {
        self.fps = self.count;
        self.count = 0;
        self.max_us = self.frame_us;
    }

    /// Frames per second the last frame time would allow (headroom), capped at 9999.
    pub fn possible_fps(&self) -> u32 {
        1_000_000u64
            .checked_div(self.frame_us)
            .unwrap_or(0)
            .min(9999) as u32
    }

    /// Writes `"0.2ms  ~5000fps"` and returns its length.
    pub fn frame_line(&self, buf: &mut [u8; LINE_LEN]) -> usize {
        let mut n = put_ms(buf, 0, self.frame_us);
        n = put(buf, n, b"  ~");
        n = put_u32(buf, n, self.possible_fps());
        put(buf, n, b"fps")
    }

    /// Writes `"draws/s 60 0.4ms"` (redraw rate and worst frame this second).
    pub fn draws_line(&self, buf: &mut [u8; LINE_LEN]) -> usize {
        let mut n = put(buf, 0, b"draws/s ");
        n = put_u32(buf, n, self.fps);
        n = put(buf, n, b" ");
        put_ms(buf, n, self.max_us)
    }
}

/// Writes `"heap 12%  thr 3"` and returns its length.
pub fn heap_line(buf: &mut [u8; LINE_LEN], heap_pct: u32, threads: usize) -> usize {
    let mut n = put(buf, 0, b"heap ");
    n = put_u32(buf, n, heap_pct);
    n = put(buf, n, b"%  thr ");
    put_u32(buf, n, threads.min(u32::MAX as usize) as u32)
}

/// Append raw bytes at `pos` (truncating to the buffer), return the new position.
pub fn put(buf: &mut [u8], pos: usize, src: &[u8]) -> usize {
    let pos = pos.min(buf.len());
    let n = src.len().min(buf.len() - pos);
    buf[pos..pos + n].copy_from_slice(&src[..n]);
    pos + n
}

/// Append a decimal u32 at `pos` (truncating to the buffer), return the new position.
pub fn put_u32(buf: &mut [u8], pos: usize, v: u32) -> usize {
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

/// Append microseconds as `"M.mms"` with one decimal of milliseconds.
pub fn put_ms(buf: &mut [u8], pos: usize, us: u64) -> usize {
    // Saturate: `us / 100` can exceed u32 for absurd durations (hours).
    let tenths = (us / 100).min(u32::MAX as u64) as u32; // milliseconds * 10
    let mut p = put_u32(buf, pos, tenths / 10);
    p = put(buf, p, b".");
    p = put_u32(buf, p, tenths % 10);
    put(buf, p, b"ms")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(buf: &[u8], n: usize) -> &str {
        core::str::from_utf8(&buf[..n]).unwrap()
    }

    #[test]
    fn put_u32_formats_decimals() {
        for v in [0u32, 1, 9, 10, 99, 100, 12345, u32::MAX] {
            let mut b = [0u8; 16];
            let n = put_u32(&mut b, 0, v);
            assert_eq!(s(&b, n), v.to_string());
        }
    }

    #[test]
    fn put_u32_truncates_at_buffer_end() {
        let mut b = [0u8; 3];
        let n = put_u32(&mut b, 0, 12345);
        assert_eq!(n, 3);
        assert_eq!(&b, b"123");
        let mut b = [0u8; 4];
        assert_eq!(put_u32(&mut b, 4, 7), 4); // already full: nothing written
    }

    #[test]
    fn put_clamps_position_and_length() {
        let mut b = [0u8; 4];
        assert_eq!(put(&mut b, 0, b"abcdef"), 4);
        assert_eq!(&b, b"abcd");
        assert_eq!(put(&mut b, 9, b"x"), 4); // pos past the end must not panic
        assert_eq!(put(&mut b, 2, b""), 2);
    }

    #[test]
    fn put_ms_rounds_down_to_tenths() {
        let mut b = [0u8; 16];
        let n = put_ms(&mut b, 0, 0);
        assert_eq!(s(&b, n), "0.0ms");
        let n = put_ms(&mut b, 0, 99);
        assert_eq!(s(&b, n), "0.0ms");
        let n = put_ms(&mut b, 0, 200);
        assert_eq!(s(&b, n), "0.2ms");
        let n = put_ms(&mut b, 0, 12_345);
        assert_eq!(s(&b, n), "12.3ms");
    }

    #[test]
    fn put_ms_huge_value_saturates_instead_of_wrapping() {
        let mut b = [0u8; 24];
        let n = put_ms(&mut b, 0, u64::MAX);
        assert_eq!(s(&b, n), "429496729.5ms");
    }

    #[test]
    fn khz_calibration() {
        // 25 ticks at 1000 Hz = 25 ms; 75_000_000 cycles -> 3_000_000 kHz (3 GHz).
        assert_eq!(khz_from_calibration(75_000_000, 25, 1000), 3_000_000);
        assert_eq!(khz_from_calibration(0, 25, 1000), 1); // never zero
        assert_eq!(khz_from_calibration(100_000, 25, 0), 4); // bogus rate: no div-by-zero
        assert_eq!(khz_from_calibration(100, 25, 1_000_000), 100); // span rounds to 1 ms
    }

    #[test]
    fn record_converts_cycles_to_microseconds() {
        let mut p = Perf::new(2_000_000); // 2 GHz -> 2_000_000 cycles/ms
        p.record(2_000_000); // 1 ms
        assert_eq!(p.frame_us, 1000);
        p.record(500_000);
        assert_eq!(p.frame_us, 250);
        assert_eq!(p.max_us, 1000); // worst frame is kept
    }

    #[test]
    fn zero_khz_is_clamped() {
        let mut p = Perf::new(0);
        p.record(5);
        assert_eq!(p.frame_us, 5000);
        let mut p = Perf::new(1);
        p.record(u64::MAX); // saturating_mul: no overflow
        assert_eq!(p.frame_us, u64::MAX);
    }

    #[test]
    fn second_tick_rolls_counters() {
        let mut p = Perf::new(1_000_000);
        for c in [1_000_000, 3_000_000, 2_000_000] {
            p.record(c);
        }
        assert_eq!(p.max_us, 3000);
        p.second_tick();
        assert_eq!(p.fps, 3);
        assert_eq!(p.max_us, 2000); // restarts from the last frame
        p.second_tick();
        assert_eq!(p.fps, 0); // idle second
    }

    #[test]
    fn possible_fps_handles_zero_and_caps() {
        let mut p = Perf::new(1_000_000);
        assert_eq!(p.possible_fps(), 0); // no frame yet
        p.frame_us = 200;
        assert_eq!(p.possible_fps(), 5000);
        p.frame_us = 50;
        assert_eq!(p.possible_fps(), 9999); // capped
        p.frame_us = 1_000_000;
        assert_eq!(p.possible_fps(), 1);
    }

    #[test]
    fn hud_lines_match_expected_text() {
        let mut p = Perf::new(1_000_000);
        p.frame_us = 200;
        p.fps = 60;
        p.max_us = 400;
        let mut b = [0u8; LINE_LEN];
        let n = p.frame_line(&mut b);
        assert_eq!(s(&b, n), "0.2ms  ~5000fps");
        let n = p.draws_line(&mut b);
        assert_eq!(s(&b, n), "draws/s 60 0.4ms");
        let n = heap_line(&mut b, 12, 3);
        assert_eq!(s(&b, n), "heap 12%  thr 3");
    }

    #[test]
    fn hud_lines_never_exceed_the_buffer() {
        let mut p = Perf::new(1);
        p.frame_us = u64::MAX;
        p.max_us = u64::MAX;
        p.fps = u32::MAX;
        let mut b = [0u8; LINE_LEN];
        assert!(p.frame_line(&mut b) <= LINE_LEN);
        assert!(p.draws_line(&mut b) <= LINE_LEN);
        assert!(heap_line(&mut b, u32::MAX, usize::MAX) <= LINE_LEN);
    }
}
