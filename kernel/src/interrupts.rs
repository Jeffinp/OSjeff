//! IDT, CPU exception handlers, 8259 PIC remap, 8253/8254 PIT timer, and
//! IRQ-driven PS/2 input.
//!
//! The keyboard (IRQ1) and mouse (IRQ12) ISRs push tagged bytes into a
//! single-producer/single-consumer ring buffer that the main loop drains, so
//! no busy-polling is needed. Fatal CPU exceptions halt with a frozen screen
//! instead of triple-faulting (silent reboot), making bugs visible.

use crate::io::{inb, outb};
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

/// Monotonic timer tick count (incremented at `TIMER_HZ`).
pub static TICKS: AtomicU64 = AtomicU64::new(0);
pub const TIMER_HZ: u32 = 100;

/// Tags on bytes in the input ring (high byte of each entry).
pub const SRC_KEYBOARD: u16 = 0;
pub const SRC_MOUSE: u16 = 1;

const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;
const PIC_EOI: u8 = 0x20;
const PS2_DATA: u16 = 0x60;

const TIMER_VECTOR: u8 = 32; // IRQ0
const KEYBOARD_VECTOR: u8 = 33; // IRQ1
const MOUSE_VECTOR: u8 = 44; // IRQ12 (slave PIC)

static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

pub fn init() {
    unsafe {
        IDT.breakpoint.set_handler_fn(breakpoint);
        IDT.general_protection_fault
            .set_handler_fn(general_protection);
        IDT.page_fault.set_handler_fn(page_fault);
        IDT.double_fault.set_handler_fn(double_fault);
        IDT[TIMER_VECTOR].set_handler_fn(timer);
        IDT[KEYBOARD_VECTOR].set_handler_fn(keyboard);
        IDT[MOUSE_VECTOR].set_handler_fn(mouse);
        IDT.load();
    }
    remap_pic();
    init_pit(TIMER_HZ);
    x86_64::instructions::interrupts::enable();
}

// ---- input ring (SPSC: ISR produces, main loop consumes) ----

const RING_CAP: usize = 512;

struct InputRing {
    buf: UnsafeCell<[u16; RING_CAP]>,
    head: AtomicUsize, // consumer
    tail: AtomicUsize, // producer
}

// Safe: ISRs run with interrupts disabled (no producer re-entrancy) and the
// main loop is the only consumer; head/tail use acquire/release ordering.
unsafe impl Sync for InputRing {}

static RING: InputRing = InputRing {
    buf: UnsafeCell::new([0; RING_CAP]),
    head: AtomicUsize::new(0),
    tail: AtomicUsize::new(0),
};

fn ring_push(value: u16) {
    let tail = RING.tail.load(Ordering::Relaxed);
    let next = (tail + 1) % RING_CAP;
    if next == RING.head.load(Ordering::Acquire) {
        return; // full: drop the byte
    }
    unsafe { (*RING.buf.get())[tail] = value };
    RING.tail.store(next, Ordering::Release);
}

/// Current monotonic timer tick.
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Pop one tagged input byte (`SRC_* << 8 | byte`), or `None` if empty.
pub fn read_input() -> Option<u16> {
    let head = RING.head.load(Ordering::Relaxed);
    if head == RING.tail.load(Ordering::Acquire) {
        return None;
    }
    let value = unsafe { (*RING.buf.get())[head] };
    RING.head.store((head + 1) % RING_CAP, Ordering::Release);
    Some(value)
}

// ---- ISRs ----

extern "x86-interrupt" fn timer(_f: InterruptStackFrame) {
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::sched::on_tick(); // credit CPU time to the running thread
    outb(PIC1_CMD, PIC_EOI);
}

extern "x86-interrupt" fn keyboard(_f: InterruptStackFrame) {
    let byte = inb(PS2_DATA);
    ring_push((SRC_KEYBOARD << 8) | byte as u16);
    outb(PIC1_CMD, PIC_EOI);
}

extern "x86-interrupt" fn mouse(_f: InterruptStackFrame) {
    let byte = inb(PS2_DATA);
    ring_push((SRC_MOUSE << 8) | byte as u16);
    // IRQ12 is on the slave PIC: EOI to both slave and master.
    outb(PIC2_CMD, PIC_EOI);
    outb(PIC1_CMD, PIC_EOI);
}

extern "x86-interrupt" fn breakpoint(_f: InterruptStackFrame) {}

extern "x86-interrupt" fn double_fault(_f: InterruptStackFrame, _code: u64) -> ! {
    halt()
}

extern "x86-interrupt" fn general_protection(_f: InterruptStackFrame, _code: u64) {
    halt()
}

extern "x86-interrupt" fn page_fault(_f: InterruptStackFrame, _code: PageFaultErrorCode) {
    halt()
}

fn halt() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

// ---- 8259 PIC ----

/// Remap the PICs to 0x20/0x28 and unmask the timer, keyboard, and mouse.
fn remap_pic() {
    // ICW1: begin init (cascade, ICW4 needed).
    outb(PIC1_CMD, 0x11);
    outb(PIC2_CMD, 0x11);
    // ICW2: vector offsets.
    outb(PIC1_DATA, 0x20);
    outb(PIC2_DATA, 0x28);
    // ICW3: master/slave wiring (slave on IRQ2).
    outb(PIC1_DATA, 0x04);
    outb(PIC2_DATA, 0x02);
    // ICW4: 8086 mode.
    outb(PIC1_DATA, 0x01);
    outb(PIC2_DATA, 0x01);
    // Master: enable IRQ0 (timer), IRQ1 (keyboard), IRQ2 (cascade) -> 0xF8.
    outb(PIC1_DATA, 0xF8);
    // Slave: enable IRQ12 (mouse), bit 4 -> 0xEF.
    outb(PIC2_DATA, 0xEF);
}

// ---- 8253/8254 PIT ----

fn init_pit(hz: u32) {
    let divisor = (1_193_182 / hz) as u16;
    outb(0x43, 0x36); // channel 0, lo/hi byte, mode 3 (square wave)
    outb(0x40, (divisor & 0xFF) as u8);
    outb(0x40, (divisor >> 8) as u8);
}
