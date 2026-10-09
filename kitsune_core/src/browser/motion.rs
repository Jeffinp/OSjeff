//! Small time-driven pieces of the browser's motion that do not belong to the page:
//! the loading bar and values that show for a moment and fade. Pure (`dt` in seconds), so
//! they are tested on the host; `f32` only, a handful of values per frame.

use crate::anim::reduce_motion;

/// How far the bar creeps while a page is still loading.
const CREEP_LIMIT: f32 = 0.9;
/// Seconds the bar takes to run to the end once the page is in.
const FINISH_SECS: f32 = 0.18;
/// Seconds the finished bar takes to fade away.
const FADE_SECS: f32 = 0.28;

/// The progress bar under the omnibox. The network gives no total, so while loading the bar
/// eases towards 90 %; when the page arrives it runs to the end and fades.
#[derive(Clone, Copy, Debug)]
pub struct LoadBar {
    p: f32,
    fade: f32,
    loading: bool,
    finishing: bool,
}

impl Default for LoadBar {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadBar {
    pub const fn new() -> Self {
        LoadBar {
            p: 0.0,
            fade: 0.0,
            loading: false,
            finishing: false,
        }
    }

    /// A load began: the bar appears at a small head start.
    pub fn start(&mut self) {
        self.loading = true;
        self.finishing = false;
        self.p = self.p.clamp(0.06, 0.1);
        self.fade = 1.0;
    }

    /// The load ended (the page, an error or a stop): run to the end and fade.
    pub fn finish(&mut self) {
        if self.loading {
            self.loading = false;
            self.finishing = true;
            if reduce_motion() {
                self.p = 1.0;
            }
        }
    }

    /// Is a load running?
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Advance; `true` while anything still moves (the window needs frames).
    pub fn step(&mut self, dt: f32) -> bool {
        if self.loading {
            // Exponential approach to the limit: quick at first, slower and slower.
            self.p += (CREEP_LIMIT - self.p) * (1.0 - (-dt * 1.4_f32).exp_approx());
            return true;
        }
        if self.finishing {
            if self.p < 1.0 {
                self.p = (self.p + dt / FINISH_SECS).min(1.0);
                return true;
            }
            self.fade -= dt / FADE_SECS;
            if self.fade <= 0.0 || reduce_motion() {
                self.fade = 0.0;
                self.finishing = false;
                self.p = 0.0;
                return false;
            }
            return true;
        }
        false
    }

    /// Filled part in thousandths.
    pub fn permille(&self) -> i32 {
        (self.p.clamp(0.0, 1.0) * 1000.0) as i32
    }

    /// Opacity 0..=256 (0 = nothing to draw).
    pub fn alpha(&self) -> u32 {
        if !self.loading && !self.finishing {
            return 0;
        }
        (self.fade.clamp(0.0, 1.0) * 256.0) as u32
    }
}

/// `exp(-x)` without libm: `f32::exp` is not available in `core`. A rational approximation
/// good to a few percent on 0..4, which is all an ease needs.
trait ExpApprox {
    fn exp_approx(self) -> f32;
}

impl ExpApprox for f32 {
    /// `self` is `-x` with `x >= 0`: returns about `e^(-x)`.
    fn exp_approx(self) -> f32 {
        let x = (-self).clamp(0.0, 8.0);
        // e^-x ~ 1 / (1 + x + x^2/2 + x^3/6 + x^4/24)
        let x2 = x * x;
        1.0 / (1.0 + x + x2 / 2.0 + x2 * x / 6.0 + x2 * x2 / 24.0)
    }
}

/// A value that appears at once, stays a moment and fades: the zoom pill, the notice.
#[derive(Clone, Copy, Debug, Default)]
pub struct Flash {
    hold: f32,
    fade: f32,
}

/// Seconds a [`Flash`] fades over.
const FLASH_FADE: f32 = 0.3;

impl Flash {
    pub const fn new() -> Self {
        Flash {
            hold: 0.0,
            fade: 0.0,
        }
    }

    /// Show for `secs` (then the fade).
    pub fn show(&mut self, secs: f32) {
        self.hold = secs.max(0.0);
        self.fade = FLASH_FADE;
    }

    /// Hide at once.
    pub fn hide(&mut self) {
        self.hold = 0.0;
        self.fade = 0.0;
    }

    /// Advance; `true` while visible or fading.
    pub fn step(&mut self, dt: f32) -> bool {
        let mut dt = dt;
        if self.hold > 0.0 {
            if self.hold > dt {
                self.hold -= dt;
                return true;
            }
            // The rest of this step already counts towards the fade.
            dt -= self.hold;
            self.hold = 0.0;
        }
        if self.fade > 0.0 {
            if reduce_motion() {
                self.fade = 0.0;
                return false;
            }
            self.fade = (self.fade - dt).max(0.0);
            return self.fade > 0.0;
        }
        false
    }

    /// Opacity 0..=256.
    pub fn alpha(&self) -> u32 {
        if self.hold > 0.0 {
            256
        } else {
            (self.fade / FLASH_FADE * 256.0).clamp(0.0, 256.0) as u32
        }
    }

    pub fn active(&self) -> bool {
        self.hold > 0.0 || self.fade > 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bar_creeps_but_never_reaches_the_end_while_loading() {
        let mut b = LoadBar::new();
        assert_eq!(b.alpha(), 0);
        b.start();
        assert!(b.is_loading());
        assert!(b.alpha() > 0 && b.permille() > 0);
        let mut last = b.permille();
        for _ in 0..600 {
            assert!(b.step(0.016));
            assert!(b.permille() >= last);
            last = b.permille();
        }
        assert!(last > 800 && last <= 900, "{last}");
    }

    #[test]
    fn finishing_runs_to_the_end_then_fades_and_goes_idle() {
        let mut b = LoadBar::new();
        b.start();
        for _ in 0..30 {
            b.step(0.016);
        }
        b.finish();
        assert!(!b.is_loading());
        let mut frames = 0;
        while b.step(0.016) {
            frames += 1;
            assert!(frames < 200, "never settles");
        }
        assert_eq!(b.alpha(), 0);
        assert_eq!(b.permille(), 0);
        // Idle really is idle.
        assert!(!b.step(0.016));
    }

    #[test]
    fn a_finish_without_a_start_does_nothing() {
        let mut b = LoadBar::new();
        b.finish();
        assert!(!b.step(0.016));
        assert_eq!(b.alpha(), 0);
    }

    #[test]
    fn a_new_load_restarts_a_fading_bar() {
        let mut b = LoadBar::new();
        b.start();
        b.step(0.5);
        b.finish();
        b.step(0.3);
        b.start();
        assert!(b.is_loading());
        assert_eq!(b.alpha(), 256);
        assert!(b.permille() <= 100);
    }

    #[test]
    fn exp_approximation_is_close_enough() {
        for k in 0..40 {
            let x = k as f32 * 0.1;
            let real = {
                // Taylor to high order as the reference.
                let mut term = 1.0f32;
                let mut sum = 1.0f32;
                for n in 1..40 {
                    term *= -x / n as f32;
                    sum += term;
                }
                sum
            };
            let a = (-x).exp_approx();
            assert!((a - real).abs() < 0.05, "x={x} {a} vs {real}");
        }
    }

    #[test]
    fn a_flash_holds_then_fades_then_stops() {
        let mut f = Flash::new();
        assert!(!f.active());
        assert_eq!(f.alpha(), 0);
        f.show(1.0);
        assert_eq!(f.alpha(), 256);
        assert!(f.step(0.5));
        assert_eq!(f.alpha(), 256);
        assert!(f.step(0.6)); // into the fade
        let mid = f.alpha();
        assert!(mid > 0 && mid < 256, "{mid}");
        let mut n = 0;
        while f.step(0.016) {
            n += 1;
            assert!(n < 100);
        }
        assert_eq!(f.alpha(), 0);
        assert!(!f.active());
        f.show(0.0);
        f.hide();
        assert!(!f.active());
    }
}
