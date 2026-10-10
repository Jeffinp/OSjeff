//! smoothing (split out of `activity.rs`).

use super::*;

/// Move `cur` toward `target` (both Q8 fixed point: 256 = 1.0 of whatever the value
/// is) as an exponential approach with time constant `tau_ms` over `dt_ms`. Reaches
/// the target exactly once the remaining gap is below one step, so it always settles.
pub fn ease_toward(cur: i32, target: i32, dt_ms: u32, tau_ms: u32) -> i32 {
    if cur == target {
        return cur;
    }
    // k = dt / (tau + dt), a stable one-pole step for any dt.
    let k = (dt_ms as u64 * 256 / (tau_ms as u64 + dt_ms as u64).max(1)) as i64;
    let gap = (target - cur) as i64;
    let step = gap * k / 256;
    let next = cur as i64 + if step == 0 { gap.signum() } else { step };
    // Never overshoot.
    let next = if gap > 0 {
        next.min(target as i64)
    } else {
        next.max(target as i64)
    };
    next as i32
}

/// A value that glides toward a target (Q8), used for bars and gauges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Glide {
    pub(super) cur: i32,
    pub(super) target: i32,
}

impl Glide {
    pub const fn at(v: i32) -> Self {
        Self {
            cur: v << 8,
            target: v << 8,
        }
    }

    /// Aim at `v` (plain units).
    pub fn set(&mut self, v: i32) {
        self.target = v << 8;
    }

    /// Jump to the target (reduced motion, first sample).
    pub fn snap(&mut self) {
        self.cur = self.target;
    }

    /// Advance by `dt_ms`; `true` while still moving.
    pub fn step(&mut self, dt_ms: u32, tau_ms: u32) -> bool {
        self.cur = ease_toward(self.cur, self.target, dt_ms, tau_ms);
        self.cur != self.target
    }

    /// Current value in plain units (rounded).
    pub fn value(&self) -> i32 {
        (self.cur + 128) >> 8
    }

    /// Where it is heading, in plain units.
    pub fn target(&self) -> i32 {
        self.target >> 8
    }

    /// Current value in Q8.
    pub fn q8(&self) -> i32 {
        self.cur
    }

    pub fn moving(&self) -> bool {
        self.cur != self.target
    }
}

/// Value of the history at fractional position `pos_q8` (Q8 index, `0` = oldest
/// kept), linearly interpolated and clamped to the ends. `None` when empty.
pub fn series_at(s: &Series, pos_q8: i64) -> Option<u32> {
    let mut buf = [0u32; crate::system::sysmon::HIST];
    let n = snapshot(s, &mut buf);
    slice_at(&buf[..n], pos_q8)
}

/// Copy a history into `out` (oldest first); returns how many samples were copied.
pub fn snapshot(s: &Series, out: &mut [u32; crate::system::sysmon::HIST]) -> usize {
    let n = s.len();
    for (i, o) in out.iter_mut().enumerate().take(n) {
        *o = s.get(i).unwrap_or(0);
    }
    n
}

/// [`series_at`] over a plain slice.
pub fn slice_at(v: &[u32], pos_q8: i64) -> Option<u32> {
    if v.is_empty() {
        return None;
    }
    let max = ((v.len() - 1) as i64) << 8;
    let p = pos_q8.clamp(0, max);
    let i = (p >> 8) as usize;
    let f = (p & 255) as u64;
    let a = v[i] as u64;
    let b = v[(i + 1).min(v.len() - 1)] as u64;
    Some(((a * (256 - f) + b * f + 128) >> 8) as u32)
}

/// Light smoothing of a history for drawing, in place: a 3-tap binomial filter
/// (1-2-1) with the ends kept as they are.
pub fn smooth121(v: &mut [u32]) {
    if v.len() < 3 {
        return;
    }
    let mut prev = v[0] as u64;
    for i in 1..v.len() - 1 {
        let cur = v[i] as u64;
        v[i] = ((prev + 2 * cur + v[i + 1] as u64 + 2) / 4) as u32;
        prev = cur;
    }
}

/// The text of a formatting buffer (empty if a cut left it invalid).
pub fn text<const N: usize>(b: &FixedBuf<N>) -> &str {
    core::str::from_utf8(b.as_bytes()).unwrap_or("")
}

/// Which sample of a plot `plot_w` pixels wide (starting at `plot_x`) is under the
/// pointer at `px`, for a history of `len` samples drawn right-aligned in `slots`
/// slots (the newest at the right edge). `None` outside the plot or where no sample
/// exists yet.
pub fn sample_under(px: i32, plot_x: i32, plot_w: i32, slots: usize, len: usize) -> Option<usize> {
    if plot_w <= 0 || len == 0 || slots < 2 || px < plot_x || px >= plot_x + plot_w {
        return None;
    }
    let slot = ((px - plot_x) as i64 * (slots as i64 - 1) * 2 / plot_w.max(1) as i64 + 1) / 2;
    let slot = (slot as usize).min(slots - 1);
    let first = slots.saturating_sub(len);
    slot.checked_sub(first)
}
