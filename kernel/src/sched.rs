//! Preemptive round-robin kernel scheduler.
//!
//! Each thread owns a heap-allocated stack and a saved stack pointer. The PIT
//! timer fires a naked ISR (see `interrupts`) that saves the full interrupted
//! context, calls [`switch_current`] to pick the next thread, and restores +
//! `iretq`s into it — so any thread is preempted on a timer tick without
//! cooperating. New threads are launched by fabricating an initial stack frame
//! that the ISR epilogue + `iretq` "resume" into the entry function.

use crate::sync::RacyCell;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use x86_64::registers::segmentation::{CS, SS, Segment};

const STACK_SIZE: usize = 64 * 1024;
const MAX_THREADS: usize = 8;

/// Sentinel written to the lowest 8 bytes of every spawned stack. A stack
/// overflow grows *downward* past the usable region and clobbers this word
/// before running off the end of the heap allocation into unrelated data — so a
/// mismatch on context switch means "this thread overflowed its stack" and is
/// caught here instead of silently corrupting the heap (the worst failure mode:
/// a wild write with no fault, surfacing as nondeterministic garbage later).
const STACK_CANARY: u64 = 0xDEAD_C0DE_CAFE_F00D;

/// 512-byte, 16-byte-aligned save area for `fxsave`/`fxrstor` (x87 + SSE state:
/// xmm0..15, MXCSR, FPU control/status). One per thread so float/SSE registers
/// survive preemption — without this, a thread preempted mid-SSE would have its
/// `xmm`/`MXCSR` clobbered by another thread.
#[repr(C, align(16))]
struct FxArea([u8; 512]);

impl FxArea {
    /// Allocate a save area seeded with the *current* (valid) FPU/SSE state, so
    /// a thread's first `fxrstor` loads a sane MXCSR/control word rather than
    /// garbage (a bad MXCSR reserved bit would `#GP` on restore).
    fn seeded() -> Box<FxArea> {
        let mut area = Box::new(FxArea([0u8; 512]));
        unsafe {
            core::arch::asm!(
                "fxsave [{}]", in(reg) area.0.as_mut_ptr(),
                options(nostack, preserves_flags),
            );
        }
        area
    }
}

/// Index of the running thread (read by the Task Manager).
static CURRENT: AtomicUsize = AtomicUsize::new(0);
/// CPU ticks credited per thread slot.
static TICKS: [AtomicU64; MAX_THREADS] = [const { AtomicU64::new(0) }; MAX_THREADS];

struct Thread {
    name: &'static str,
    rsp: u64,
    stack: Box<[u8]>, // owns the stack (empty for the boot thread)
    fpu: Box<FxArea>, // x87/SSE save area, swapped on context switch
}

impl Thread {
    #[inline]
    fn fpu_ptr(&self) -> *mut u8 {
        // Box gives a stable, 16-byte-aligned address (FxArea is align(16)).
        &*self.fpu as *const FxArea as *mut u8
    }

    /// `true` if this thread's stack canary is intact (or it has no canary, like
    /// the boot thread). A `false` means the stack overflowed its bounds.
    #[inline]
    fn stack_intact(&self) -> bool {
        match self.stack.first_chunk::<8>() {
            Some(bytes) => u64::from_ne_bytes(*bytes) == STACK_CANARY,
            None => true, // boot thread: no owned stack, nothing to check
        }
    }
}

struct Scheduler {
    threads: Vec<Thread>,
    current: usize,
}

static SCHED: RacyCell<Option<Scheduler>> = RacyCell::new(None);

/// Register the current (boot) context as thread 0. Run before interrupts.
pub fn init() {
    let boot = Thread {
        name: "compositor",
        rsp: 0, // captured on the first preemption
        stack: Vec::new().into_boxed_slice(),
        fpu: FxArea::seeded(),
    };
    unsafe {
        *SCHED.get() = Some(Scheduler {
            threads: vec![boot],
            current: 0,
        });
    }
}

/// Spawn a preemptible kernel thread starting at `entry` (must never return).
pub fn spawn(name: &'static str, entry: extern "C" fn() -> !) {
    let s = scheduler();
    assert!(s.threads.len() < MAX_THREADS, "too many threads");

    let mut stack = vec![0u8; STACK_SIZE].into_boxed_slice();
    // Plant the overflow tripwire in the lowest 8 bytes (see `STACK_CANARY`).
    stack[..8].copy_from_slice(&STACK_CANARY.to_ne_bytes());
    let top = stack.as_ptr() as u64 + STACK_SIZE as u64;

    // Running stack pointer once the thread is live (≡ 8 mod 16, as if just
    // called, per the SysV ABI).
    let thread_rsp = (top & !0xF) - 8;

    let cs = CS::get_reg().0 as u64;
    let ss = SS::get_reg().0 as u64;

    // Build, from `thread_rsp` downward: an `iretq` frame, then a 15-register
    // block of zeros. The timer ISR epilogue pops the 15 regs then `iretq`s.
    let mut p = thread_rsp;
    let mut push = |val: u64| {
        p -= 8;
        unsafe { (p as *mut u64).write(val) };
    };
    push(ss); // SS
    push(thread_rsp); // RSP after iretq
    push(0x202); // RFLAGS: IF set + reserved bit
    push(cs); // CS
    push(entry as usize as u64); // RIP
    for _ in 0..15 {
        push(0); // rax..r15 (already zero, but advance the pointer)
    }
    let rsp = p;

    s.threads.push(Thread {
        name,
        rsp,
        stack,
        fpu: FxArea::seeded(),
    });
}

/// Called from the timer ISR: save the outgoing `rsp`, credit a CPU tick to the
/// thread that just ran, advance round-robin, and return the next thread's
/// `rsp`. Touches only scheduler state (no allocation, no other locks).
pub extern "C" fn switch_current(rsp: u64) -> u64 {
    let s = match unsafe { (*SCHED.get()).as_mut() } {
        Some(s) => s,
        None => return rsp,
    };
    let cur = s.current;

    // Catch a stack overflow the instant the offending thread is preempted,
    // before its wild writes corrupt the heap and detonate elsewhere.
    if !s.threads[cur].stack_intact() {
        panic!("stack overflow in thread '{}'", s.threads[cur].name);
    }

    // Save the outgoing thread's x87/SSE state. This must run before any xmm
    // use: the path from ISR entry to here (GP-reg pushes, integer scheduler
    // glue) touches no SSE register, so the interrupted thread's xmm/MXCSR are
    // still live here and captured intact.
    unsafe {
        core::arch::asm!(
            "fxsave [{}]", in(reg) s.threads[cur].fpu_ptr(),
            options(nostack, preserves_flags),
        );
    }

    s.threads[cur].rsp = rsp;
    if cur < MAX_THREADS {
        TICKS[cur].fetch_add(1, Ordering::Relaxed);
    }

    let n = s.threads.len();
    if n < 2 {
        return rsp; // single thread: nothing to restore, state unchanged
    }
    let next = (cur + 1) % n;
    s.current = next;
    CURRENT.store(next, Ordering::Relaxed);

    // Load the incoming thread's x87/SSE state. Nothing below uses xmm before
    // the ISR `iretq`s into that thread, so its registers resume correctly.
    unsafe {
        core::arch::asm!(
            "fxrstor [{}]", in(reg) s.threads[next].fpu_ptr(),
            options(nostack, readonly, preserves_flags),
        );
    }

    s.threads[next].rsp
}

// ---- introspection for the Task Manager ----

pub fn thread_count() -> usize {
    unsafe { (*SCHED.get()).as_ref() }.map_or(0, |s| s.threads.len())
}

pub fn thread_name(i: usize) -> &'static str {
    unsafe { (*SCHED.get()).as_ref() }
        .and_then(|s| s.threads.get(i))
        .map_or("", |t| t.name)
}

pub fn thread_ticks(i: usize) -> u64 {
    if i < MAX_THREADS {
        TICKS[i].load(Ordering::Relaxed)
    } else {
        0
    }
}

fn scheduler() -> &'static mut Scheduler {
    unsafe { (*SCHED.get()).as_mut().expect("scheduler not initialized") }
}
