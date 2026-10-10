//! Support crate for the benches: pulls the kernel's framebuffer primitives in
//! by path (so the benchmark runs the *real* source, not a rewrite), plus a
//! frozen copy of the pre-audit versions for A/B comparison.
#![allow(dead_code, clippy::all)]

extern crate alloc;

/// Stand-in for `kernel::sync` (only `RacyCell`; the kernel's locks need the scheduler).
pub mod sync {
    pub struct RacyCell<T>(core::cell::UnsafeCell<T>);
    unsafe impl<T> Sync for RacyCell<T> {}
    impl<T> RacyCell<T> {
        pub const fn new(value: T) -> Self {
            Self(core::cell::UnsafeCell::new(value))
        }
        pub const fn get(&self) -> *mut T {
            self.0.get()
        }
    }
}

/// Stand-in for `kernel::trace` (measurement hooks compile to nothing).
pub mod trace {
    pub const ON: bool = false;
    #[inline(always)]
    pub fn t() -> u64 {
        0
    }
    #[derive(Clone, Copy)]
    pub enum Prim {
        Glyph,
        FillRect,
        RoundRect,
        Alpha,
        BgCopy,
        Fade,
    }
    #[inline(always)]
    pub fn prim(_p: Prim, _t0: u64) {}
}

#[path = "../../kernel/src/fb.rs"]
pub mod fb;
#[path = "../../kernel/src/font.rs"]
pub mod font;

#[path = "../old/fb_old.rs"]
pub mod fb_old;
#[path = "../old/font_old.rs"]
pub mod font_old;
