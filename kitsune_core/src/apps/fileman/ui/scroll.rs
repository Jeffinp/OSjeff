//! scroll (split out of `ui.rs`).

use super::*;

/// Pixels one wheel notch scrolls.
pub const WHEEL_STEP: i32 = 3 * ROW_H;

/// A scroll offset that follows its target with a critically damped spring: wheel notches and
/// keyboard moves change the target, the position glides there.
#[derive(Clone, Copy, Debug)]
pub struct Scroller {
    pub(super) spring: Spring,
    pub(super) max: i32,
}

impl Default for Scroller {
    fn default() -> Self {
        Self::new()
    }
}

impl Scroller {
    pub const fn new() -> Self {
        Scroller {
            spring: Spring::pixels(0.0, 300.0, 34.6),
            max: 0,
        }
    }

    /// The offset to draw with.
    pub fn pos(&self) -> i32 {
        let v = self.spring.value();
        (v + 0.5) as i32
    }

    /// Where it is heading.
    pub fn target(&self) -> i32 {
        (self.spring.target() + 0.5) as i32
    }

    pub fn max(&self) -> i32 {
        self.max
    }

    /// The content or viewport changed: the largest offset is now `max`.
    pub fn set_max(&mut self, max: i32) {
        self.max = max.max(0);
        let t = self.spring.target().clamp(0.0, self.max as f32);
        self.spring.set_target(t);
        if self.spring.value() > self.max as f32 {
            self.spring.jump(t);
        }
    }

    /// Move the target by `delta` pixels (a wheel notch, a key).
    pub fn scroll_by(&mut self, delta: i32) {
        let t = (self.spring.target() + delta as f32).clamp(0.0, self.max as f32);
        self.spring.set_target(t);
    }

    /// Aim at an absolute offset.
    pub fn scroll_to(&mut self, to: i32) {
        self.spring.set_target(to.clamp(0, self.max) as f32);
    }

    /// Jump with no animation (a new folder, a drag that scrolls).
    pub fn jump(&mut self, to: i32) {
        self.spring.jump(to.clamp(0, self.max) as f32);
    }

    /// Advance by `dt` seconds; `true` while moving.
    pub fn step(&mut self, dt: f32) -> bool {
        self.spring.step(dt)
    }

    pub fn at_rest(&self) -> bool {
        self.spring.at_rest()
    }
}
