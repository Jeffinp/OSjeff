//! rates (split out of `activity.rs`).

/// Bytes (or packets) moved between two reads of a counter that is `bits` wide and may
/// have wrapped around once. A counter that merely went *backwards* by less than half
/// its range is treated as a reset (0), not as a near-full wrap.
pub fn wrapping_delta(prev: u64, now: u64, bits: u32) -> u64 {
    let bits = bits.clamp(1, 64);
    let mask = if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let (prev, now) = (prev & mask, now & mask);
    let d = now.wrapping_sub(prev) & mask;
    // Behind the previous read by more than half the range: a reset, not a wrap.
    if now < prev && d > mask / 2 { 0 } else { d }
}

/// Per-second rate from a cumulative counter, tolerant of wrap-around and of an
/// irregular interval (milliseconds). The first read reports 0.
#[derive(Clone, Copy, Debug, Default)]
pub struct Rate {
    pub(super) last: Option<(u64, u64)>,
    pub(super) bits: u8,
}

impl Rate {
    /// A meter for a `bits`-wide counter.
    pub const fn new(bits: u8) -> Self {
        Self { last: None, bits }
    }

    /// Feed the counter `now` read at `t_ms`. Returns units per second.
    pub fn feed(&mut self, now: u64, t_ms: u64) -> u64 {
        let r = match self.last {
            Some((prev, t0)) if t_ms > t0 => {
                let d = wrapping_delta(prev, now, self.bits as u32);
                (d as u128 * 1000 / (t_ms - t0) as u128).min(u64::MAX as u128) as u64
            }
            _ => 0,
        };
        self.last = Some((now, t_ms));
        r
    }
}
