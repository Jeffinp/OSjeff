//! Cooperative round-robin kernel scheduler with real context switching.
//!
//! Each thread owns a heap-allocated stack and a saved stack pointer. A thread
//! gives up the CPU with [`yield_now`], which performs a real context switch
//! (save callee-saved registers, swap `rsp`, restore) implemented in assembly.
//! The PIT timer credits CPU time to whichever thread is current
//! ([`on_tick`]), so the Task Manager shows real per-thread CPU usage.
//!
//! Switching is done from normal thread context (never inside an interrupt),
//! so only callee-saved registers must be preserved — robust and simple. True
//! preemption (switching inside the timer ISR) is the natural next step.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const STACK_SIZE: usize = 64 * 1024;
const MAX_THREADS: usize = 8;

/// Index of the running thread (read by the timer ISR; tiny + lock-free).
static CURRENT: AtomicUsize = AtomicUsize::new(0);
/// CPU ticks credited per thread slot.
static TICKS: [AtomicU64; MAX_THREADS] = [const { AtomicU64::new(0) }; MAX_THREADS];

struct Thread {
    name: &'static str,
    rsp: u64,
    _stack: Box<[u8]>, // owns the thread's stack (empty for the boot thread)
}

struct Scheduler {
    threads: Vec<Thread>,
    current: usize,
}

static mut SCHED: Option<Scheduler> = None;

/// Initialize the scheduler, registering the current (boot) context as the
/// first thread. Must run after the heap is available.
pub fn init() {
    let boot = Thread {
        name: "compositor",
        rsp: 0, // filled in by the first switch away from this thread
        _stack: Vec::new().into_boxed_slice(),
    };
    unsafe {
        SCHED = Some(Scheduler {
            threads: vec![boot].into_iter().collect(),
            current: 0,
        });
    }
}

/// Spawn a new kernel thread that begins executing `entry` (which must never
/// return). Returns the thread's slot index.
pub fn spawn(name: &'static str, entry: extern "C" fn() -> !) -> usize {
    let sched = scheduler();
    assert!(sched.threads.len() < MAX_THREADS, "too many threads");

    let mut stack = vec![0u8; STACK_SIZE].into_boxed_slice();
    let top = stack.as_mut_ptr() as u64 + STACK_SIZE as u64;

    // Build the initial stack so the first switch "returns" into `entry`:
    //   [rsp -> r15=0, r14=0, r13=0, r12=0, rbx=0, rbp=0, entry]
    // The switch routine pops the six callee-saved regs then `ret`s.
    let entry_slot = (top & !0xF) - 8; // ret leaves rsp 16-aligned
    let rsp = entry_slot - 8 * 6;
    unsafe {
        (entry_slot as *mut u64).write(entry as usize as u64);
        // The six register slots are already zero (stack is zeroed).
    }

    sched.threads.push(Thread {
        name,
        rsp,
        _stack: stack,
    });
    sched.threads.len() - 1
}

/// Yield the CPU to the next ready thread (round-robin).
pub fn yield_now() {
    let sched = scheduler();
    let n = sched.threads.len();
    if n < 2 {
        return;
    }
    let cur = sched.current;
    let next = (cur + 1) % n;

    sched.current = next;
    CURRENT.store(next, Ordering::Relaxed);

    let new_rsp = sched.threads[next].rsp;
    let old_rsp = &mut sched.threads[cur].rsp as *mut u64;
    unsafe { context_switch(old_rsp, new_rsp) };
}

/// Credit one CPU tick to the running thread. Called from the timer ISR; only
/// touches atomics so it is safe from interrupt context.
pub fn on_tick() {
    let c = CURRENT.load(Ordering::Relaxed);
    if c < MAX_THREADS {
        TICKS[c].fetch_add(1, Ordering::Relaxed);
    }
}

// ---- introspection for the Task Manager ----

pub fn thread_count() -> usize {
    match unsafe { SCHED.as_ref() } {
        Some(s) => s.threads.len(),
        None => 0,
    }
}

pub fn thread_name(i: usize) -> &'static str {
    unsafe { SCHED.as_ref() }
        .and_then(|s| s.threads.get(i))
        .map(|t| t.name)
        .unwrap_or("")
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

/// Save callee-saved registers, switch stacks, restore. `*old_rsp` receives the
/// outgoing stack pointer; `new_rsp` is the incoming one.
#[unsafe(naked)]
unsafe extern "C" fn context_switch(old_rsp: *mut u64, new_rsp: u64) {
    core::arch::naked_asm!(
        "push rbp",
        "push rbx",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        "mov [rdi], rsp", // *old_rsp = rsp
        "mov rsp, rsi",   // rsp = new_rsp
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbx",
        "pop rbp",
        "ret",
    );
}
