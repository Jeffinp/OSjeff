//! Preemptive round-robin kernel scheduler.
//!
//! Each thread owns a heap-allocated stack and a saved stack pointer. The PIT
//! timer fires a naked ISR (see `interrupts`) that saves the full interrupted
//! context, calls [`switch_current`] to pick the next thread, and restores +
//! `iretq`s into it — so any thread is preempted on a timer tick without
//! cooperating. New threads are launched by fabricating an initial stack frame
//! that the ISR epilogue + `iretq` "resume" into the entry function.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use x86_64::registers::segmentation::{Segment, CS, SS};

const STACK_SIZE: usize = 64 * 1024;
const MAX_THREADS: usize = 8;

/// Index of the running thread (read by the Task Manager).
static CURRENT: AtomicUsize = AtomicUsize::new(0);
/// CPU ticks credited per thread slot.
static TICKS: [AtomicU64; MAX_THREADS] = [const { AtomicU64::new(0) }; MAX_THREADS];

struct Thread {
    name: &'static str,
    rsp: u64,
    _stack: Box<[u8]>, // owns the stack (empty for the boot thread)
}

struct Scheduler {
    threads: Vec<Thread>,
    current: usize,
}

static mut SCHED: Option<Scheduler> = None;

/// Register the current (boot) context as thread 0. Run before interrupts.
pub fn init() {
    let boot = Thread {
        name: "compositor",
        rsp: 0, // captured on the first preemption
        _stack: Vec::new().into_boxed_slice(),
    };
    unsafe {
        SCHED = Some(Scheduler {
            threads: vec![boot],
            current: 0,
        });
    }
}

/// Spawn a preemptible kernel thread starting at `entry` (must never return).
pub fn spawn(name: &'static str, entry: extern "C" fn() -> !) {
    let s = scheduler();
    assert!(s.threads.len() < MAX_THREADS, "too many threads");

    let stack = vec![0u8; STACK_SIZE].into_boxed_slice();
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
        _stack: stack,
    });
}

/// Called from the timer ISR: save the outgoing `rsp`, credit a CPU tick to the
/// thread that just ran, advance round-robin, and return the next thread's
/// `rsp`. Touches only scheduler state (no allocation, no other locks).
pub extern "C" fn switch_current(rsp: u64) -> u64 {
    let s = match unsafe { SCHED.as_mut() } {
        Some(s) => s,
        None => return rsp,
    };
    let cur = s.current;
    s.threads[cur].rsp = rsp;
    if cur < MAX_THREADS {
        TICKS[cur].fetch_add(1, Ordering::Relaxed);
    }

    let n = s.threads.len();
    if n < 2 {
        return rsp;
    }
    let next = (cur + 1) % n;
    s.current = next;
    CURRENT.store(next, Ordering::Relaxed);
    s.threads[next].rsp
}

// ---- introspection for the Task Manager ----

pub fn thread_count() -> usize {
    unsafe { SCHED.as_ref() }.map_or(0, |s| s.threads.len())
}

pub fn thread_name(i: usize) -> &'static str {
    unsafe { SCHED.as_ref() }
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
    unsafe { SCHED.as_mut().expect("scheduler not initialized") }
}
