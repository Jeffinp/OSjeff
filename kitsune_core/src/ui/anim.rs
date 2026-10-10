//! Motion: easing curves, springs, retargetable tweens and the window
//! transitions, all driven by real elapsed seconds (`dt`), never by frame counts.
//!
//! Everything is pure state advanced by [`step`](Spring::step) / [`Anim::step`]:
//! the compositor feeds it the time since the last frame, asks `active()` to decide
//! whether another frame is needed (an idle desktop schedules nothing), and reads
//! values to draw. Animations are interruptible: [`Spring::set_target`] and
//! [`Tween::retarget`] keep the current value (and velocity) and continue toward
//! the new goal. A global *reduce motion* switch makes every step land on its end
//! value immediately.
//!
//! `f32` is fine here: a handful of values per frame, never per pixel (the kernel
//! emulates floats in software).

use crate::windowing::window::Rect;
#[cfg(not(test))]
use core::sync::atomic::{AtomicBool, Ordering};

#[cfg(not(test))]
static REDUCE_MOTION: AtomicBool = AtomicBool::new(false);

// Unit tests run in parallel threads: give each its own switch so a test that turns
// reduce motion on cannot disturb the others.
#[cfg(test)]
std::thread_local! {
    static REDUCE_MOTION_TL: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
}

/// Turn the global *reduce motion* switch on or off (the Settings toggle).
pub fn set_reduce_motion(on: bool) {
    #[cfg(not(test))]
    REDUCE_MOTION.store(on, Ordering::Relaxed);
    #[cfg(test)]
    REDUCE_MOTION_TL.with(|c| c.set(on));
}

/// Whether *reduce motion* is on.
pub fn reduce_motion() -> bool {
    #[cfg(not(test))]
    return REDUCE_MOTION.load(Ordering::Relaxed);
    #[cfg(test)]
    return REDUCE_MOTION_TL.with(|c| c.get());
}

fn clamp01(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

// ----------------------------------------------------------------------- bezier

/// A CSS-style `cubic-bezier(x1, y1, x2, y2)` easing curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bezier {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl Bezier {
    pub const fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Bezier {
        Bezier { x1, y1, x2, y2 }
    }

    /// Eased value for linear progress `t` in `0..=1` (0 at 0, 1 at 1).
    pub fn ease(&self, t: f32) -> f32 {
        let t = clamp01(t);
        if t == 0.0 || t == 1.0 {
            return t;
        }
        // Solve x(s) = t for the curve parameter s: Newton steps, then bisection.
        let x = |s: f32| {
            let u = 1.0 - s;
            3.0 * u * u * s * self.x1 + 3.0 * u * s * s * self.x2 + s * s * s
        };
        let dx = |s: f32| {
            let u = 1.0 - s;
            3.0 * u * u * self.x1
                + 6.0 * u * s * (self.x2 - self.x1)
                + 3.0 * s * s * (1.0 - self.x2)
        };
        let mut s = t;
        for _ in 0..4 {
            let d = dx(s);
            if d.abs() < 1e-5 {
                break;
            }
            s = clamp01(s - (x(s) - t) / d);
        }
        if (x(s) - t).abs() > 1e-4 {
            let (mut lo, mut hi) = (0.0f32, 1.0f32);
            for _ in 0..24 {
                s = (lo + hi) / 2.0;
                if x(s) < t {
                    lo = s;
                } else {
                    hi = s;
                }
            }
        }
        let u = 1.0 - s;
        3.0 * u * u * s * self.y1 + 3.0 * u * s * s * self.y2 + s * s * s
    }
}

/// Easing presets (the design spec's motion curves).
pub mod curves {
    use super::Bezier;
    /// Decelerating entrance: windows opening, popovers, overlays.
    pub const ENTER: Bezier = Bezier::new(0.2, 0.8, 0.2, 1.0);
    /// Accelerating exit.
    pub const EXIT: Bezier = Bezier::new(0.4, 0.0, 1.0, 1.0);
    /// Fast departure, soft landing.
    pub const SWOOP: Bezier = Bezier::new(0.3, 0.7, 0.2, 1.0);
    /// Plain ease in/out.
    pub const STANDARD: Bezier = Bezier::new(0.4, 0.0, 0.2, 1.0);
    pub const LINEAR: Bezier = Bezier::new(0.0, 0.0, 1.0, 1.0);
}

// ----------------------------------------------------------------------- spring

/// A damped spring (semi-implicit Euler with fixed sub-steps: stable for any
/// frame time). Interruptible: change the target at any moment.
#[derive(Clone, Copy, Debug)]
pub struct Spring {
    pos: f32,
    vel: f32,
    target: f32,
    pub stiffness: f32,
    pub damping: f32,
    /// Resting tolerance on position and velocity.
    pub epsilon: f32,
}

impl Spring {
    /// A spring at rest at `pos`.
    pub const fn new(pos: f32, stiffness: f32, damping: f32) -> Spring {
        Spring {
            pos,
            vel: 0.0,
            target: pos,
            stiffness,
            damping,
            epsilon: 0.002,
        }
    }

    /// Pixel-scale springs rest within a twentieth of a pixel.
    pub const fn pixels(pos: f32, stiffness: f32, damping: f32) -> Spring {
        let mut s = Spring::new(pos, stiffness, damping);
        s.epsilon = 0.05;
        s
    }

    pub fn value(&self) -> f32 {
        self.pos
    }

    pub fn velocity(&self) -> f32 {
        self.vel
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    /// Aim at a new target, keeping position and velocity.
    pub fn set_target(&mut self, t: f32) {
        self.target = t;
    }

    /// Snap to `v` at rest.
    pub fn jump(&mut self, v: f32) {
        self.pos = v;
        self.target = v;
        self.vel = 0.0;
    }

    /// Give the spring a kick (`v` units per second).
    pub fn impulse(&mut self, v: f32) {
        self.vel += v;
    }

    pub fn at_rest(&self) -> bool {
        (self.pos - self.target).abs() <= self.epsilon && self.vel.abs() <= self.epsilon * 10.0
    }

    /// Advance by `dt` seconds. Returns `true` while it still moves.
    pub fn step(&mut self, dt: f32) -> bool {
        if self.at_rest() {
            self.pos = self.target;
            self.vel = 0.0;
            return false;
        }
        if reduce_motion() || dt >= 2.0 {
            self.jump(self.target);
            return false;
        }
        // Sub-steps of at most 4 ms keep the integration stable for stiff springs.
        let n = ((dt / 0.004) as u32 + 1).min(64);
        let h = dt / n as f32;
        for _ in 0..n {
            let a = -self.stiffness * (self.pos - self.target) - self.damping * self.vel;
            self.vel += a * h;
            self.pos += self.vel * h;
        }
        if self.at_rest() {
            self.pos = self.target;
            self.vel = 0.0;
            return false;
        }
        true
    }
}

// ------------------------------------------------------------------------ tween

/// A time-based interpolation between two values along a [`Bezier`], which can be
/// retargeted mid-flight (it then starts from the value it shows right now).
#[derive(Clone, Copy, Debug)]
pub struct Tween {
    from: f32,
    to: f32,
    t: f32,
    dur: f32,
    curve: Bezier,
}

impl Tween {
    /// A finished tween resting at `v`.
    pub const fn at(v: f32) -> Tween {
        Tween {
            from: v,
            to: v,
            t: 1.0,
            dur: 1.0,
            curve: curves::LINEAR,
        }
    }

    /// Start moving from the current value to `to` over `dur` seconds.
    pub fn retarget(&mut self, to: f32, dur: f32, curve: Bezier) {
        if to == self.to && !self.finished() {
            return;
        }
        self.from = self.value();
        self.to = to;
        self.t = 0.0;
        self.dur = dur.max(0.001);
        self.curve = curve;
        if reduce_motion() {
            self.t = self.dur;
        }
    }

    pub fn value(&self) -> f32 {
        if self.t >= self.dur {
            self.to
        } else {
            lerp(self.from, self.to, self.curve.ease(self.t / self.dur))
        }
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn finished(&self) -> bool {
        self.t >= self.dur
    }

    /// Advance; `true` while still running.
    pub fn step(&mut self, dt: f32) -> bool {
        if self.finished() {
            return false;
        }
        self.t = if reduce_motion() {
            self.dur
        } else {
            self.t + dt
        };
        !self.finished()
    }
}

// ------------------------------------------------------------- window transition

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Window appearing: visibility goes 0 -> 1.
    Opening,
    /// Window disappearing: visibility goes 1 -> 0.
    Closing,
}

/// What the window travels to or from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flavor {
    /// Scale about its centre (new window, close).
    Pop,
    /// Toward / from its dock icon (minimise, restore).
    Dock,
    /// Sideways with a fade (a workspace change): the sign is the direction of the change, +1
    /// when the new workspace is to the right. Entering windows come from that side, leaving
    /// ones go to the other.
    Slide(i8),
}

/// How far, in pixels, a window travels sideways in a workspace change.
pub const SLIDE_PX: f32 = 420.0;
/// Seconds a workspace change takes.
pub const SLIDE_SECS: f32 = 0.30;

/// Scale of a window at the start of an open and the end of a close.
pub const POP_SCALE: f32 = 0.92;

/// Seconds an open takes.
pub const OPEN_SECS: f32 = 0.22;
/// Seconds a close takes.
pub const CLOSE_SECS: f32 = 0.16;
/// Seconds minimise / restore take.
pub const DOCK_SECS: f32 = 0.30;

/// The window enter/exit transition.
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    phase: Phase,
    flavor: Flavor,
    /// Elapsed seconds.
    t: f32,
    dur: f32,
}

/// Where a window is drawn this frame and how opaque.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub rect: Rect,
    /// 0..=256.
    pub alpha: u16,
}

impl Anim {
    pub fn open() -> Self {
        Self {
            phase: Phase::Opening,
            flavor: Flavor::Pop,
            t: 0.0,
            dur: OPEN_SECS,
        }
    }

    pub fn close() -> Self {
        Self {
            phase: Phase::Closing,
            flavor: Flavor::Pop,
            t: 0.0,
            dur: CLOSE_SECS,
        }
    }

    /// Minimise: the window flies into its dock icon.
    pub fn minimize() -> Self {
        Self {
            phase: Phase::Closing,
            flavor: Flavor::Dock,
            t: 0.0,
            dur: DOCK_SECS,
        }
    }

    /// A window of the new workspace slides in (`dir` = +1 when the workspace is to the right).
    pub fn slide_in(dir: i8) -> Self {
        Self {
            phase: Phase::Opening,
            flavor: Flavor::Slide(dir),
            t: 0.0,
            dur: SLIDE_SECS,
        }
    }

    /// A window of the old workspace slides out.
    pub fn slide_out(dir: i8) -> Self {
        Self {
            phase: Phase::Closing,
            flavor: Flavor::Slide(dir),
            t: 0.0,
            dur: SLIDE_SECS,
        }
    }

    /// Restore from the dock.
    pub fn restore() -> Self {
        Self {
            phase: Phase::Opening,
            flavor: Flavor::Dock,
            t: 0.0,
            dur: DOCK_SECS,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn flavor(&self) -> Flavor {
        self.flavor
    }

    pub fn is_closing(&self) -> bool {
        self.phase == Phase::Closing
    }

    /// Advance by `dt` seconds (clamped at the end).
    pub fn step(&mut self, dt: f32) {
        self.t = if reduce_motion() {
            self.dur
        } else {
            (self.t + dt).min(self.dur)
        };
    }

    /// True once the transition has fully played out.
    pub fn finished(&self) -> bool {
        self.t >= self.dur
    }

    /// Linear progress `0..=1` through the transition.
    pub fn progress(&self) -> f32 {
        clamp01(self.t / self.dur)
    }

    /// Eased visibility in `0.0..=1.0` (0 = gone, 1 = fully present).
    pub fn visibility(&self) -> f32 {
        let p = self.progress();
        match (self.phase, self.flavor) {
            (Phase::Opening, Flavor::Pop | Flavor::Slide(_)) => curves::ENTER.ease(p),
            (Phase::Closing, Flavor::Pop | Flavor::Slide(_)) => 1.0 - curves::EXIT.ease(p),
            (Phase::Opening, Flavor::Dock) => curves::SWOOP.ease(p),
            (Phase::Closing, Flavor::Dock) => 1.0 - curves::SWOOP.ease(1.0 - (1.0 - p)),
        }
    }

    /// Fade factor for the window (same as visibility), `0.0..=1.0`.
    pub fn alpha(&self) -> f32 {
        self.visibility()
    }

    /// Where `rect` (the window's resting rectangle) is drawn now. `dock` is the
    /// screen rectangle of its dock icon, used by the [`Flavor::Dock`] transitions
    /// (without it they behave like [`Flavor::Pop`]).
    pub fn frame(&self, rect: Rect, dock: Option<Rect>) -> Frame {
        let v = self.visibility();
        match (self.flavor, dock) {
            (Flavor::Dock, Some(d)) => {
                // Position follows the eased value; the size shrinks a little
                // faster than the position travels (a lazy genie).
                let q = v;
                let qs = q * q;
                let lerp_i = |a: i32, b: i32, t: f32| (a as f32 + (b - a) as f32 * t + 0.5) as i32;
                let w = lerp_i(d.w, rect.w, qs.max(0.0)).max(1);
                let h = lerp_i(d.h, rect.h, qs.max(0.0)).max(1);
                // Keep the horizontal centre on the straight line between the centres.
                let cx = lerp_i(d.x + d.w / 2, rect.x + rect.w / 2, q);
                let cy = lerp_i(d.y + d.h / 2, rect.y + rect.h / 2, q);
                Frame {
                    rect: Rect::new(cx - w / 2, cy - h / 2, w, h),
                    alpha: (clamp01(v * 1.8) * 256.0) as u16,
                }
            }
            (Flavor::Slide(dir), _) => {
                // Entering windows come from the side of the change, leaving ones go away from it.
                let sign = if self.phase == Phase::Opening {
                    dir as f32
                } else {
                    -(dir as f32)
                };
                let off = (sign * (1.0 - v) * SLIDE_PX) as i32;
                Frame {
                    rect: Rect::new(rect.x + off, rect.y, rect.w, rect.h),
                    alpha: (v * 256.0) as u16,
                }
            }
            _ => {
                let s = lerp(POP_SCALE, 1.0, v);
                let w = (rect.w as f32 * s + 0.5) as i32;
                let h = (rect.h as f32 * s + 0.5) as i32;
                Frame {
                    rect: Rect::new(
                        rect.x + (rect.w - w) / 2,
                        rect.y + (rect.h - h) / 2,
                        w.max(1),
                        h.max(1),
                    ),
                    alpha: (v * 256.0) as u16,
                }
            }
        }
    }

    /// Vertical slide offset in pixels (kept for callers that only slide).
    pub fn slide(&self, max: f32) -> f32 {
        (1.0 - self.visibility()) * max
    }
}

// ------------------------------------------------------------------ zoom (rect)

/// Maximise / restore: the window rectangle travels from `from` to `to` on a
/// spring (progress 0 -> 1, slightly under-damped so it settles with a hint of
/// life). The content is drawn stretched while it runs and re-laid-out at the end.
#[derive(Clone, Copy, Debug)]
pub struct Zoom {
    from: Rect,
    to: Rect,
    p: Spring,
}

impl Zoom {
    pub fn new(from: Rect, to: Rect) -> Zoom {
        let mut p = Spring::new(0.0, 300.0, 30.0);
        p.set_target(1.0);
        if reduce_motion() {
            p.jump(1.0);
        }
        Zoom { from, to, p }
    }

    /// Re-aim at `to` from wherever the window is now (interrupting a zoom).
    pub fn retarget(&mut self, to: Rect) {
        let now = self.rect();
        self.from = now;
        self.to = to;
        self.p.jump(0.0);
        self.p.set_target(1.0);
        if reduce_motion() {
            self.p.jump(1.0);
        }
    }

    pub fn step(&mut self, dt: f32) -> bool {
        self.p.step(dt)
    }

    pub fn finished(&self) -> bool {
        self.p.at_rest()
    }

    /// The rectangle to draw now.
    pub fn rect(&self) -> Rect {
        let t = self.p.value();
        let l = |a: i32, b: i32| (a as f32 + (b - a) as f32 * t + 0.5) as i32;
        Rect::new(
            l(self.from.x, self.to.x),
            l(self.from.y, self.to.y),
            l(self.from.w, self.to.w).max(1),
            l(self.from.h, self.to.h).max(1),
        )
    }

    pub fn from(&self) -> Rect {
        self.from
    }

    pub fn to(&self) -> Rect {
        self.to
    }
}

// ------------------------------------------------------------------ dock bounce

/// The launch bounce of a dock icon: hops of decreasing height, each a parabola.
/// `height` is the peak of the first hop in pixels; `t` is seconds since launch.
/// Returns the upward offset in pixels (0 when done) and whether it still runs.
pub fn bounce(t: f32, height: f32) -> (f32, bool) {
    const HOP: f32 = 0.32;
    const HOPS: u32 = 3;
    if reduce_motion() || t >= HOP * HOPS as f32 || t < 0.0 {
        return (0.0, false);
    }
    let k = (t / HOP) as u32;
    let u = (t - k as f32 * HOP) / HOP; // 0..1 within the hop
    let peak = height * (1.0 - 0.38 * k as f32);
    (4.0 * peak * u * (1.0 - u), true)
}

// ---------------------------------------------------------------------------
// Caret blink
// ---------------------------------------------------------------------------

/// Length of one caret blink, in milliseconds.
pub const BLINK_MS: u64 = 1060;
/// A caret keeps blinking this long after the last key or click, then rests solid (an idle
/// desktop must not repaint forever).
pub const BLINK_ACTIVE_MS: u64 = 12_000;
/// The caret stays solid this long after input.
pub const BLINK_HOLD_MS: u64 = 500;

fn smoothstep256(x: u32) -> u32 {
    let x = x.min(256);
    (x * x * (768 - 2 * x)) >> 16
}

/// The caret opacity (0..=256) `ms` milliseconds into the blink, eased: solid, a quick fade
/// out, dark, a quick fade in.
pub fn caret_curve(ms: u64) -> u32 {
    let t = ms % BLINK_MS;
    match t {
        0..=419 => 256,
        420..=579 => 256 - smoothstep256(((t - 420) * 256 / 160) as u32),
        580..=899 => 0,
        _ => smoothstep256(((t - 900) * 256 / 160) as u32),
    }
}

/// The opacity of a caret whose owner saw input `since_ms` ago (`None`: never, so a fresh
/// window keeps a solid caret): solid for [`BLINK_HOLD_MS`], blinking until [`BLINK_ACTIVE_MS`],
/// solid afterwards.
pub fn caret_alpha(since_ms: Option<u64>) -> u32 {
    match since_ms {
        Some(s) if (BLINK_HOLD_MS..BLINK_ACTIVE_MS).contains(&s) => caret_curve(s - BLINK_HOLD_MS),
        _ => 256,
    }
}

/// Whether such a caret still changes (the window needs frames).
pub fn caret_animating(since_ms: Option<u64>) -> bool {
    since_ms.is_some_and(|s| s < BLINK_ACTIVE_MS)
}

#[cfg(test)]
mod tests;
