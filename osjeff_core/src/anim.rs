//! Window enter/exit animation state. Pure easing math, frame-driven.
//!
//! The kernel advances each animation by a small `dt` per rendered frame and
//! uses [`Anim::alpha`] (fade) and [`Anim::slide`] (vertical offset) to draw the
//! transition. No timers or floats-in-hardware concerns — just `f32` arithmetic.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Window appearing: visibility goes 0 -> 1.
    Opening,
    /// Window disappearing: visibility goes 1 -> 0.
    Closing,
}

#[derive(Clone, Copy, Debug)]
pub struct Anim {
    phase: Phase,
    /// Raw timeline position in `0.0..=1.0` (always advances forward).
    t: f32,
}

impl Anim {
    pub fn open() -> Self {
        Self {
            phase: Phase::Opening,
            t: 0.0,
        }
    }

    pub fn close() -> Self {
        Self {
            phase: Phase::Closing,
            t: 0.0,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn is_closing(&self) -> bool {
        self.phase == Phase::Closing
    }

    /// Advance the timeline by `dt` (clamped at the end).
    pub fn step(&mut self, dt: f32) {
        self.t = (self.t + dt).min(1.0);
    }

    /// True once the transition has fully played out.
    pub fn finished(&self) -> bool {
        self.t >= 1.0
    }

    /// Eased visibility in `0.0..=1.0` (0 = gone, 1 = fully present).
    pub fn visibility(&self) -> f32 {
        let p = smoothstep(self.t);
        match self.phase {
            Phase::Opening => p,
            Phase::Closing => 1.0 - p,
        }
    }

    /// Fade factor for the window (same as visibility), `0.0..=1.0`.
    pub fn alpha(&self) -> f32 {
        self.visibility()
    }

    /// Vertical slide offset in pixels: the window rises into place as it opens
    /// and sinks as it closes. `max` is the peak offset.
    pub fn slide(&self, max: f32) -> f32 {
        (1.0 - self.visibility()) * max
    }
}

/// Classic smoothstep easing: flat at both ends, `3t² - 2t³`.
fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn smoothstep_endpoints() {
        assert!(approx(smoothstep(0.0), 0.0));
        assert!(approx(smoothstep(1.0), 1.0));
        assert!(approx(smoothstep(0.5), 0.5));
    }

    #[test]
    fn smoothstep_clamps_out_of_range() {
        assert!(approx(smoothstep(-1.0), 0.0));
        assert!(approx(smoothstep(2.0), 1.0));
    }

    #[test]
    fn smoothstep_is_monotonic() {
        let mut prev = -1.0;
        let mut t = 0.0;
        while t <= 1.0 {
            let v = smoothstep(t);
            assert!(v >= prev);
            prev = v;
            t += 0.05;
        }
    }

    #[test]
    fn open_starts_invisible_ends_visible() {
        let mut a = Anim::open();
        assert!(approx(a.visibility(), 0.0));
        assert!(!a.is_closing());
        a.step(1.0);
        assert!(a.finished());
        assert!(approx(a.visibility(), 1.0));
        assert!(approx(a.alpha(), 1.0));
    }

    #[test]
    fn close_starts_visible_ends_gone() {
        let mut a = Anim::close();
        assert!(a.is_closing());
        assert_eq!(a.phase(), Phase::Closing);
        assert!(approx(a.visibility(), 1.0));
        a.step(1.0);
        assert!(a.finished());
        assert!(approx(a.visibility(), 0.0));
    }

    #[test]
    fn step_clamps_at_one() {
        let mut a = Anim::open();
        a.step(0.7);
        a.step(0.7);
        assert!(a.finished());
        assert!(approx(a.visibility(), 1.0));
    }

    #[test]
    fn slide_offset_decreases_while_opening() {
        let mut a = Anim::open();
        let start = a.slide(24.0);
        a.step(0.5);
        let mid = a.slide(24.0);
        a.step(0.5);
        let end = a.slide(24.0);
        assert!(approx(start, 24.0));
        assert!(mid < start);
        assert!(approx(end, 0.0));
    }

    #[test]
    fn slide_offset_grows_while_closing() {
        let mut a = Anim::close();
        let start = a.slide(24.0);
        a.step(1.0);
        let end = a.slide(24.0);
        assert!(approx(start, 0.0));
        assert!(approx(end, 24.0));
    }
}
