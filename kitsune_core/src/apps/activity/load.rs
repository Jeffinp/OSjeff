//! load (split out of `activity.rs`).

/// Exponentially averaged busy share over 1, 5 and 15 minutes, in thousandths of a
/// full CPU (`1000` = one CPU busy all the time). Fed once per second.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadAvg {
    // Q8 thousandths.
    pub(super) v: [i64; 3],
    pub(super) primed: bool,
}

impl LoadAvg {
    pub const fn new() -> Self {
        Self {
            v: [0; 3],
            primed: false,
        }
    }

    /// One second elapsed with the CPU `busy_pm` thousandths busy.
    pub fn feed(&mut self, busy_pm: u32) {
        let x = (busy_pm.min(1000) as i64) << 8;
        if !self.primed {
            self.v = [x; 3];
            self.primed = true;
            return;
        }
        for (v, tau) in self.v.iter_mut().zip([60i64, 300, 900]) {
            *v += (x - *v) / tau;
        }
    }

    /// Averages in thousandths: (1 min, 5 min, 15 min).
    pub fn get(&self) -> (u32, u32, u32) {
        let f = |v: i64| ((v + 128) >> 8).clamp(0, 1000) as u32;
        (f(self.v[0]), f(self.v[1]), f(self.v[2]))
    }
}
