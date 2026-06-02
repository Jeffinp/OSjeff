//! IDT, CPU exception handlers, 8259 PIC remap, and an 8253/8254 PIT timer.
//!
//! Only IRQ0 (the timer) is unmasked; the keyboard/mouse keep using polling.
//! Fatal exceptions halt with a frozen screen instead of triple-faulting
//! (silent reboot), which makes bugs visible during development.

use crate::io::outb;
use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

/// Monotonic timer tick count (incremented at `TIMER_HZ`).
pub static TICKS: AtomicU64 = AtomicU64::new(0);
pub const TIMER_HZ: u32 = 100;

const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;
const PIC_EOI: u8 = 0x20;

const TIMER_VECTOR: u8 = 32; // IRQ0 after remap to offset 32

static mut IDT: InterruptDescriptorTable = InterruptDescriptorTable::new();

pub fn init() {
    unsafe {
        IDT.breakpoint.set_handler_fn(breakpoint);
        IDT.general_protection_fault
            .set_handler_fn(general_protection);
        IDT.page_fault.set_handler_fn(page_fault);
        IDT.double_fault.set_handler_fn(double_fault);
        IDT[TIMER_VECTOR].set_handler_fn(timer);
        IDT.load();
    }
    remap_pic();
    init_pit(TIMER_HZ);
    x86_64::instructions::interrupts::enable();
}

// ---- ISRs ----

extern "x86-interrupt" fn timer(_f: InterruptStackFrame) {
    TICKS.fetch_add(1, Ordering::Relaxed);
    outb(PIC1_CMD, PIC_EOI); // end-of-interrupt to master PIC
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

/// Remap the PICs to vectors 0x20/0x28 and mask everything except IRQ0.
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
    // Masks: only IRQ0 (timer) enabled on the master.
    outb(PIC1_DATA, 0xFE);
    outb(PIC2_DATA, 0xFF);
}

// ---- 8253/8254 PIT ----

fn init_pit(hz: u32) {
    let divisor = (1_193_182 / hz) as u16;
    outb(0x43, 0x36); // channel 0, lo/hi byte, mode 3 (square wave)
    outb(0x40, (divisor & 0xFF) as u8);
    outb(0x40, (divisor >> 8) as u8);
}
