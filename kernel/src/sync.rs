//! Single-core kernel cell for mutable statics.
//!
//! Replaces the `static mut` pattern, which edition 2024 rejects (`static_mut_refs`
//! is a hard error) because taking a `&mut` to a `static mut` is instant UB the
//! moment two such references coexist. [`RacyCell`] instead hands out a raw
//! `*mut T`, so callers form only short-lived references in `unsafe` blocks and
//! never materialize an aliasing `&mut` to the static itself.
//!
//! Soundness is the caller's contract, not the type's: this kernel is
//! single-core and access to each cell is serialized either by running before
//! interrupts are enabled (e.g. the IDT, set up once at boot) or by the
//! interrupt-flag mutual exclusion between an ISR and the main loop (the ISR
//! runs with `IF` clear, so it cannot interleave with the code it preempts).

use core::cell::UnsafeCell;

/// A `Sync` wrapper exposing a `*mut T` to a kernel-global. See the module docs
/// for the soundness contract — the name is a deliberate reminder that the
/// compiler is *not* proving exclusivity here; the kernel's execution model is.
#[repr(transparent)]
pub struct RacyCell<T>(UnsafeCell<T>);

// Safe in this kernel: see module-level soundness contract (single-core +
// interrupt-flag serialization). Access still requires `unsafe`.
unsafe impl<T> Sync for RacyCell<T> {}

impl<T> RacyCell<T> {
    /// Wrap an initial value. `const` so it can initialize a `static`.
    pub const fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    /// Raw pointer to the contained value. Callers must uphold the module's
    /// serialization contract before dereferencing.
    #[inline]
    pub const fn get(&self) -> *mut T {
        self.0.get()
    }
}
