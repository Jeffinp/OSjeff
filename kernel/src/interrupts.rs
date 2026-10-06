//! IDT, CPU exception handlers, 8259 PIC remap, 8253/8254 PIT timer, and
//! IRQ-driven PS/2 input.
//!
//! The keyboard (IRQ1) and mouse (IRQ12) ISRs push tagged bytes into a
//! single-producer/single-consumer ring buffer that the main loop drains, so
//! no busy-polling is needed. Fatal CPU exceptions halt with a frozen screen
//! instead of triple-faulting (silent reboot), making bugs visible.

use crate::io::{inb, outb};
use crate::sync::RacyCell;
use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use x86_64::VirtAddr;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

/// Monotonic timer tick count (incremented at `TIMER_HZ`).
pub static TICKS: AtomicU64 = AtomicU64::new(0);
/// Timer / scheduler quantum frequency. Higher = smoother time-slicing.
pub const TIMER_HZ: u32 = 250;

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

static IDT: RacyCell<InterruptDescriptorTable> = RacyCell::new(InterruptDescriptorTable::new());

pub fn init() {
    unsafe {
        let idt = &mut *IDT.get();
        idt.breakpoint.set_handler_fn(breakpoint);
        idt.general_protection_fault
            .set_handler_fn(general_protection);
        idt.page_fault.set_handler_fn(page_fault);
        idt.double_fault.set_handler_fn(double_fault);
        // Without these, #DE/#UD/#NP/#SS escalate to a silent #DF.
        idt.divide_error.set_handler_fn(divide_error);
        idt.invalid_opcode.set_handler_fn(invalid_opcode);
        idt.segment_not_present.set_handler_fn(segment_not_present);
        idt.stack_segment_fault.set_handler_fn(stack_segment_fault);
        // The timer uses a naked ISR that performs a full preemptive context
        // switch, so it's installed by raw address instead of `set_handler_fn`.
        idt[TIMER_VECTOR].set_handler_addr(VirtAddr::from_ptr(timer_isr as *const ()));
        idt[KEYBOARD_VECTOR].set_handler_fn(keyboard);
        idt[MOUSE_VECTOR].set_handler_fn(mouse);
        // `load` needs `&'static self`; the table lives in a `static`, so a
        // reference derived from its raw pointer is genuinely `'static`.
        (*IDT.get()).load();
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

// The preemptive context switch is hand-written x86_64 assembly in `switch.s`,
// assembled here via `global_asm!`. It defines the `timer_isr` symbol installed
// in the IDT for IRQ0 and tail-calls `timer_schedule` (the Rust half below).
core::arch::global_asm!(
    include_str!("switch.s"),
    schedule = sym timer_schedule,
);

unsafe extern "C" {
    /// Naked timer ISR entry (see `switch.s`): saves the interrupted context,
    /// switches stacks to the next thread, and `iretq`s into it.
    fn timer_isr();
}

/// Rust half of the timer ISR: advance the clock, round-robin to the next
/// thread, and acknowledge the interrupt. Returns the next thread's `rsp`.
extern "C" fn timer_schedule(rsp: u64) -> u64 {
    let t0 = crate::trace::t();
    TICKS.fetch_add(1, Ordering::Relaxed);
    crate::trace::timer_sample(rsp, crate::sched::current());
    let next = crate::sched::switch_current(rsp);
    outb(PIC1_CMD, PIC_EOI); // EOI before iretq re-enables interrupts
    if crate::trace::ON {
        let d = crate::io::rdtsc().wrapping_sub(t0);
        crate::trace::ISR_N.fetch_add(1, Ordering::Relaxed);
        crate::trace::ISR_CYC.fetch_add(d, Ordering::Relaxed);
        crate::trace::max_to(&crate::trace::ISR_MAX, d);
    }
    next
}

extern "x86-interrupt" fn keyboard(_f: InterruptStackFrame) {
    let byte = inb(PS2_DATA);
    crate::trace::input_irq();
    ring_push((SRC_KEYBOARD << 8) | byte as u16);
    outb(PIC1_CMD, PIC_EOI);
}

extern "x86-interrupt" fn mouse(_f: InterruptStackFrame) {
    let byte = inb(PS2_DATA);
    crate::trace::input_irq();
    ring_push((SRC_MOUSE << 8) | byte as u16);
    // IRQ12 is on the slave PIC: EOI to both slave and master.
    outb(PIC2_CMD, PIC_EOI);
    outb(PIC1_CMD, PIC_EOI);
}

extern "x86-interrupt" fn breakpoint(_f: InterruptStackFrame) {}

/// Report a fatal CPU exception on COM1 (the only channel that still works
/// when the compositor is dead), then freeze. Uses no allocation or locks.
fn fatal(name: &str, f: &InterruptStackFrame, code: Option<u64>) -> ! {
    crate::serial_println!(
        "FATAL EXCEPTION: {name} code={code:?} rip={:#x} rsp={:#x} rflags={:#x}",
        f.instruction_pointer.as_u64(),
        f.stack_pointer.as_u64(),
        f.cpu_flags.bits()
    );
    halt()
}

extern "x86-interrupt" fn double_fault(f: InterruptStackFrame, code: u64) -> ! {
    fatal("#DF double fault", &f, Some(code))
}

extern "x86-interrupt" fn general_protection(f: InterruptStackFrame, code: u64) {
    fatal("#GP general protection", &f, Some(code))
}

extern "x86-interrupt" fn page_fault(f: InterruptStackFrame, code: PageFaultErrorCode) {
    crate::serial_println!(
        "FATAL EXCEPTION: #PF cr2={:#x} err={:?}",
        x86_64::registers::control::Cr2::read_raw(),
        code
    );
    fatal("#PF page fault", &f, Some(code.bits()))
}

extern "x86-interrupt" fn divide_error(f: InterruptStackFrame) {
    fatal("#DE divide error", &f, None)
}

extern "x86-interrupt" fn invalid_opcode(f: InterruptStackFrame) {
    fatal("#UD invalid opcode", &f, None)
}

extern "x86-interrupt" fn segment_not_present(f: InterruptStackFrame, code: u64) {
    fatal("#NP segment not present", &f, Some(code))
}

extern "x86-interrupt" fn stack_segment_fault(f: InterruptStackFrame, code: u64) {
    fatal("#SS stack segment fault", &f, Some(code))
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
