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
