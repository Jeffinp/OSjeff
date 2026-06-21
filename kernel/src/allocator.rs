//! Kernel heap: a linked-list free allocator behind a spin lock, exposed as the
//! global allocator so `alloc` (Vec/String/Box) works. The fiddly alignment and
//! region-fit math is in `osjeff_core::heap` (unit-tested); this file is the
//! thin `unsafe` glue that threads free nodes through the heap memory.

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::mem;
use core::ptr;
use core::sync::atomic::{AtomicBool, Ordering};
use osjeff_core::heap::{adjust_request, fit_region, regions_adjacent};

// ---- minimal spin lock ----

pub struct SpinLock<T> {
    locked: AtomicBool,
    data: UnsafeCell<T>,
}

// Safe: access is serialized by the lock; the kernel is single-core and our
// ISRs never allocate, so there is no re-entrancy.
unsafe impl<T: Send> Sync for SpinLock<T> {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            data: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> SpinGuard<'_, T> {
        // Disable interrupts while the lock is held so a timer preemption can't
        // switch to a thread that then deadlocks waiting on the same lock.
        let ints_enabled = x86_64::instructions::interrupts::are_enabled();
        x86_64::instructions::interrupts::disable();
        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        SpinGuard {
            lock: self,
            ints_enabled,
        }
    }
}

pub struct SpinGuard<'a, T> {
    lock: &'a SpinLock<T>,
    ints_enabled: bool,
}

impl<T> core::ops::Deref for SpinGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> core::ops::DerefMut for SpinGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for SpinGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
        if self.ints_enabled {
            x86_64::instructions::interrupts::enable();
        }
    }
}

// ---- linked-list allocator ----

struct FreeNode {
    size: usize,
    next: Option<&'static mut FreeNode>,
}

impl FreeNode {
    const fn new(size: usize) -> Self {
        FreeNode { size, next: None }
    }
    fn start_addr(&self) -> usize {
        self as *const Self as usize
    }
    fn end_addr(&self) -> usize {
        self.start_addr() + self.size
    }
}

fn node_size() -> usize {
    mem::size_of::<FreeNode>()
}
fn node_align() -> usize {
    mem::align_of::<FreeNode>()
}

pub struct LinkedListAllocator {
    head: FreeNode,
}

impl LinkedListAllocator {
    pub const fn new() -> Self {
        Self {
            head: FreeNode::new(0),
        }
    }

    /// # Safety
    /// `start..start+size` must be valid, unused, writable memory that lives for
    /// the rest of the program.
    pub unsafe fn init(&mut self, start: usize, size: usize) {
        unsafe { self.add_free_region(start, size) };
    }

    /// Insert a freed region into the address-sorted free list and coalesce it
    /// with any physically adjacent neighbours. Without coalescing, repeated
    /// alloc/free of mixed sizes would fragment the heap permanently — large
    /// requests would fail even with plenty of (scattered) free memory.
    unsafe fn add_free_region(&mut self, addr: usize, size: usize) {
        debug_assert_eq!(osjeff_core::heap::align_up(addr, node_align()), addr);
        debug_assert!(size >= node_size());

        unsafe {
            let node_ptr = addr as *mut FreeNode;
            node_ptr.write(FreeNode::new(size));

            // Walk the sorted list to the last node whose start is below `addr`
            // (the head is a sentinel with start = &head, size 0, so it never
            // coalesces with a real region).
            let mut prev: *mut FreeNode = &mut self.head;
            while let Some(next) = (*prev).next.as_ref() {
                if next.start_addr() < addr {
                    prev = (*prev).next.as_deref_mut().unwrap() as *mut FreeNode;
                } else {
                    break;
                }
            }

            // Link the new node in between `prev` and `prev.next`.
            (*node_ptr).next = (*prev).next.take();
            (*prev).next = Some(&mut *node_ptr);

            // Merge forward (node + successor), then backward (prev + node).
            // Order matters: collapsing the successor first closes a three-way gap.
            Self::merge_with_next(node_ptr);
            Self::merge_with_next(prev);
        }
    }

    /// If `node` ends exactly where its successor begins, absorb the successor.
    unsafe fn merge_with_next(node: *mut FreeNode) {
        let n = unsafe { &mut *node };
        let adjacent = match n.next.as_deref() {
            Some(next) => regions_adjacent(n.start_addr(), n.size, next.start_addr()),
            None => false,
        };
        if adjacent {
            let next = n.next.take().unwrap();
            n.size += next.size;
            n.next = next.next.take();
        }
    }

    /// Remove and return the first region that fits, plus the chosen start.
    fn find_region(&mut self, size: usize, align: usize) -> Option<(&'static mut FreeNode, usize)> {
        let mut current = &mut self.head;
        while let Some(ref mut region) = current.next {
            if let Some((alloc_start, _excess)) =
                fit_region(region.start_addr(), region.size, size, align, node_size())
            {
                let next = region.next.take();
                let region = current.next.take().unwrap();
                current.next = next;
                return Some((region, alloc_start));
            }
            current = current.next.as_mut().unwrap();
        }
        None
    }

    fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let (size, align) =
            adjust_request(layout.size(), layout.align(), node_size(), node_align());
        match self.find_region(size, align) {
            Some((region, alloc_start)) => {
                let alloc_end = alloc_start + size;
                let excess = region.end_addr() - alloc_end;
                if excess > 0 {
                    unsafe { self.add_free_region(alloc_end, excess) };
                }
                alloc_start as *mut u8
            }
            None => ptr::null_mut(),
        }
    }

    fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        let (size, _align) =
            adjust_request(layout.size(), layout.align(), node_size(), node_align());
        unsafe { self.add_free_region(ptr as usize, size) };
    }
}

impl Default for LinkedListAllocator {
    fn default() -> Self {
        Self::new()
    }
}

// ---- global allocator ----

pub struct LockedHeap(SpinLock<LinkedListAllocator>);

impl LockedHeap {
    pub const fn new() -> Self {
        Self(SpinLock::new(LinkedListAllocator::new()))
    }

    /// # Safety
    /// See [`LinkedListAllocator::init`].
    pub unsafe fn init(&self, start: usize, size: usize) {
        unsafe { self.0.lock().init(start, size) };
    }
}

unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.0.lock().alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.0.lock().dealloc(ptr, layout);
    }
}
