//! Small time-driven pieces of the browser's motion that do not belong to the page:
//! the loading bar and values that show for a moment and fade. Pure (`dt` in seconds), so
//! they are tested on the host; `f32` only, a handful of values per frame.

use crate::ui::anim::reduce_motion;

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
mod tests;
