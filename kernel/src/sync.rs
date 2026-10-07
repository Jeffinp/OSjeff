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
// SAFETY: a promise, not a proof: single core, and each cell's users are serialized by
// convention (boot before IRQs, ISR with IF=0, single owner thread, or an atomic state
// machine; see docs/audit/01-memoria-unsafe.md section 3.2).
// NOTE: not guaranteed by the type: there is no `T: Send` bound and nothing stops a third accessor.
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

// ---------------------------------------------------------------------------
// YieldMutex: a blocking lock for thread context (IF=1).
// ---------------------------------------------------------------------------

use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Why [`YieldMutex::lock`] gave up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockError {
    /// The calling thread already holds the lock (taking it again would never return).
    Reentrant,
    /// The holder died (a contained fault killed its thread) with the lock held:
    /// what it protects may be half-updated, so nobody gets it again.
    OwnerDead,
    /// Interrupts are off, so waiting could never let the holder run; the bounded
    /// spin ran out.
    Timeout,
}

const NO_OWNER: usize = usize::MAX;

/// Spin budget for a caller with interrupts disabled (it cannot yield).
const IRQ_OFF_SPINS: u32 = 1_000_000;

/// A mutual-exclusion lock whose holder may be preempted or `yield_now` while
/// holding it (unlike [`crate::allocator::SpinLock`], which masks interrupts and
/// so cannot be held across a long, yielding operation such as disk I/O).
///
/// Waiters give the CPU away instead of burning their slice. It is meant for
/// thread context; it never deadlocks the machine: re-entry by the owner, a dead
/// owner and an interrupts-off wait all come back as a [`LockError`].
pub struct YieldMutex<T> {
    locked: AtomicBool,
    owner: AtomicUsize,
    value: UnsafeCell<T>,
}

// SAFETY: `value` is only reachable through a `YieldGuard`, which exists only while
// `locked` was won with a compare-exchange; so at most one thread touches it at a time.
unsafe impl<T: Send> Sync for YieldMutex<T> {}

impl<T> YieldMutex<T> {
    /// Wrap `value`; `const` so it can initialize a `static`.
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            owner: AtomicUsize::new(NO_OWNER),
            value: UnsafeCell::new(value),
        }
    }

    /// Acquire the lock, yielding the CPU while someone else holds it.
    pub fn lock(&self) -> Result<YieldGuard<'_, T>, LockError> {
        let me = crate::sched::current();
        let mut spins = 0u32;
        loop {
            if self
                .locked
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                self.owner.store(me, Ordering::Relaxed);
                return Ok(YieldGuard { m: self });
            }
            let owner = self.owner.load(Ordering::Relaxed);
            if owner == me {
                return Err(LockError::Reentrant);
            }
            if owner != NO_OWNER && crate::sched::is_dead(owner) {
                return Err(LockError::OwnerDead);
            }
            if x86_64::instructions::interrupts::are_enabled() {
                crate::sched::yield_now();
            } else {
                spins += 1;
                if spins > IRQ_OFF_SPINS {
                    return Err(LockError::Timeout);
                }
                core::hint::spin_loop();
            }
        }
    }
}

/// Proof of holding a [`YieldMutex`]; releases it on drop.
pub struct YieldGuard<'a, T> {
    m: &'a YieldMutex<T>,
}

impl<T> Deref for YieldGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: the guard proves this thread won `locked`, so no other reference exists.
        unsafe { &*self.m.value.get() }
    }
}

impl<T> DerefMut for YieldGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: as in `deref`, and `&mut self` makes this the only reference through the guard.
        unsafe { &mut *self.m.value.get() }
    }
}

impl<T> Drop for YieldGuard<'_, T> {
    fn drop(&mut self) {
        self.m.owner.store(NO_OWNER, Ordering::Relaxed);
        self.m.locked.store(false, Ordering::Release);
    }
}
