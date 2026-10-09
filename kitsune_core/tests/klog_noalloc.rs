//! Proof that the log write path never touches the heap.
//!
//! The kernel pushes log records from interrupt handlers, where taking the heap
//! lock could deadlock against the interrupted code. `kitsune_core::klog`'s
//! write path (formatting into `FixedBuf`, `LogRing::push`, the serial line
//! assembler and its classifier, reading a snapshot out) must therefore allocate
//! nothing. This test installs a counting global allocator (per thread, so the
//! test harness cannot disturb it) and checks that 100 000 operations of each
//! kind perform zero allocations.

use core::fmt::Write as _;
use kitsune_core::klog::{
    Filter, FixedBuf, Level, LineAsm, LogRing, RING_BYTES, classify, ticks_to_ms,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}

struct Counting;

// SAFETY: forwards every call to the system allocator unchanged; the counter is a
// thread-local `Cell` with const initialisation (no allocation, no destructor).
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static A: Counting = Counting;

fn allocs() -> usize {
    ALLOCS.with(Cell::get)
}

#[test]
fn the_write_path_does_not_allocate() {
    // Everything that may allocate is created first.
    let mut ring: Box<LogRing<RING_BYTES>> = Box::default();
    let mut out = vec![0u8; RING_BYTES];
    let mut line = LineAsm::new();
    let filter = Filter::new();
    let mut sink = 0usize;

    let before = allocs();
    for i in 0..100_000u32 {
        // What `klog::log` does: format into a stack buffer, push.
        let mut b: FixedBuf<200> = FixedBuf::new();
        let _ = write!(b, "message {i} from thread");
        ring.push(
            ticks_to_ms(i as u64, 250),
            Level::from_u8((i % 6) as u8),
            (i % 8) as u8,
            b.as_bytes(),
        );
        // What the serial mirror does: assemble a line, classify it, push it.
        line.feed(b"net: DHCP lease 10.0.2.15\n", |l| {
            if let Some(level) = classify(l) {
                ring.push(0, level, 0, l);
            }
        });
        // What the toast watcher does.
        if i % 1000 == 0 {
            ring.for_each_since(i.saturating_sub(50), Level::Warn, |e| sink += e.text.len());
            // And a viewer snapshot into its own pre-allocated buffer.
            sink += ring.copy_out(&mut out);
        }
        sink += filter.matches(&kitsune_core::klog::Entry {
            seq: 0,
            ts_ms: 0,
            level: Level::Info,
            origin: 0,
            text: b"x",
        }) as usize;
    }
    let after = allocs();
    assert!(sink > 0);
    assert_eq!(
        after - before,
        0,
        "the log write path allocated {} times",
        after - before
    );

    // Control: the counter does see an allocation, so the zero above means something.
    let c0 = allocs();
    let v: Vec<u8> = Vec::with_capacity(16);
    std::hint::black_box(&v);
    assert!(allocs() > c0, "the counting allocator is not live");
}
