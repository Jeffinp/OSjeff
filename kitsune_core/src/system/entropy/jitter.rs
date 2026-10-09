//! Credit estimation for timing sources.
//!
//! A timing source (the TSC value at an interrupt, at a received frame, ...)
//! delivers a 64-bit timestamp per event. What is unpredictable is not the
//! timestamp but its *variation*: the difference between consecutive timestamps
//! is mostly set by the schedule (the timer period), and only the low-order
//! noise is physical. [`TimingEstimator`] looks at that variation and decides,
//! per event, between "credit a small fixed amount" and "credit nothing".
//!
//! The tests are the stuck/repetition tests of jitter-entropy collectors and
//! NIST SP 800-90B's repetition-count idea, applied to the first, second and
//! third differences of the timestamps:
//!
//! * a zero first, second or third difference means the source is stuck or
//!   perfectly periodic (a deterministic emulator, an idle constant timer);
//! * a second difference below [`MIN_VARIATION`] cycles is quantization, not noise;
//! * the same low byte of the first difference repeating [`RCT_CUTOFF`] times in
//!   a row is a pattern.
//!
//! **What this is not.** It does not measure entropy. It makes a fixed,
//! deliberately low claim (half a bit, or a tenth for CPU-execution jitter)
//! per event that passed. An adversary who can observe or control the host
//! clock tick-for-tick can predict more than that, which is why timing-only
//! generators never rate as `Strong` (see [`super::Quality`]).

/// Smallest second difference (in timestamp units) that counts as noise.
pub const MIN_VARIATION: u64 = 4;
/// Identical low bytes in a row that mark a repeating pattern.
pub const RCT_CUTOFF: u8 = 5;
/// Credit per accepted event for interrupt/arrival timestamps (0.5 bit).
pub const IRQ_MILLIBITS: u32 = 500;
/// Credit per accepted event for CPU execution-time jitter (0.1 bit): the timing of a short
/// loop depends on the same host scheduling that every other sample sees, so it is worth less.
pub const EXEC_MILLIBITS: u32 = 100;

/// Verdict for one timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Not enough history yet (the first three events) or the event was refused.
    Rejected,
    /// Credit this many millibits.
    Accepted(u32),
}

/// Per-source state for the tests above.
#[derive(Clone, Copy, Debug)]
pub struct TimingEstimator {
    credit_mbits: u32,
    seen: u8,
    last_ts: u64,
    last_d1: u64,
    last_d2: u64,
    last_low: u8,
    run: u8,
}

impl TimingEstimator {
    /// A fresh estimator that credits `credit_mbits` per accepted event.
    pub const fn new(credit_mbits: u32) -> TimingEstimator {
        TimingEstimator {
            credit_mbits,
            seen: 0,
            last_ts: 0,
            last_d1: 0,
            last_d2: 0,
            last_low: 0,
            run: 0,
        }
    }

    /// Judge the next timestamp.
    pub fn observe(&mut self, ts: u64) -> Verdict {
        let d1 = ts.wrapping_sub(self.last_ts);
        let d2 = d1.abs_diff(self.last_d1);
        let d3 = d2.abs_diff(self.last_d2);
        let low = d1 as u8;
        let history = self.seen;
        let repeat = history >= 2 && low == self.last_low;
        self.run = if repeat {
            self.run.saturating_add(1)
        } else {
            0
        };
        self.last_ts = ts;
        self.last_d1 = d1;
        self.last_d2 = d2;
        self.last_low = low;
        self.seen = self.seen.saturating_add(1);
        // d1 needs one previous timestamp, d2 two, d3 three.
        if history < 3 {
            return Verdict::Rejected;
        }
        if d1 == 0 || d2 < MIN_VARIATION || d3 == 0 || self.run >= RCT_CUTOFF {
            return Verdict::Rejected;
        }
        Verdict::Accepted(self.credit_mbits)
    }
}
