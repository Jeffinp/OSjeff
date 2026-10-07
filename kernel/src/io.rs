//! Raw x86 port I/O. Thin wrappers over `in`/`out` instructions.

use core::arch::asm;

#[inline]
pub fn inb(port: u16) -> u8 {
    let value: u8;
    // SAFETY: `in` from an I/O port is legal in ring 0 and touches no Rust memory.
    // NOTE: not guaranteed by the type: this safe fn lets any caller read any port; side effects
    // are the device's (docs/audit/05-qualidade-rust.md A-07).
    unsafe {
        asm!("in al, dx", out("al") value, in("dx") port, options(nomem, nostack, preserves_flags));
    }
    value
}

#[inline]
pub fn outb(port: u16, value: u8) {
    // SAFETY: `out` to an I/O port is legal in ring 0 and touches no Rust memory; the device-side
    // effect is the caller's responsibility (see `inb`).
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
    }
}

#[inline]
pub fn outw(port: u16, value: u16) {
    // SAFETY: 16-bit port write, ring 0, no Rust memory touched (see `outb`).
    unsafe {
        asm!("out dx, ax", in("dx") port, in("ax") value, options(nomem, nostack, preserves_flags));
    }
}

#[inline]
pub fn inw(port: u16) -> u16 {
    let value: u16;
    // SAFETY: 16-bit port read, ring 0, no Rust memory touched (see `inb`).
    unsafe {
        asm!("in ax, dx", out("ax") value, in("dx") port, options(nomem, nostack, preserves_flags));
    }
    value
}

#[inline]
pub fn outl(port: u16, value: u32) {
    // SAFETY: 32-bit port write, ring 0, no Rust memory touched (see `outb`).
    unsafe {
        asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack, preserves_flags));
    }
}

#[inline]
pub fn inl(port: u16) -> u32 {
    let value: u32;
    // SAFETY: 32-bit port read, ring 0, no Rust memory touched (see `inb`).
    unsafe {
        asm!("in eax, dx", out("eax") value, in("dx") port, options(nomem, nostack, preserves_flags));
    }
    value
}

/// Read the CPU timestamp counter (cycle count since reset).
#[inline]
pub fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    // SAFETY: `rdtsc` is always allowed in ring 0, reads no memory and clobbers only eax/edx,
    // which are declared as outputs.
    unsafe {
        asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack, preserves_flags));
    }
    ((hi as u64) << 32) | lo as u64
}

/// Busy-wait roughly `cycles` TSC ticks. Used to pace animation frames without
/// a timer interrupt. Approximate (TSC frequency is unknown), good enough for
/// ~60fps visuals.
#[inline]
pub fn delay_cycles(cycles: u64) {
    let start = rdtsc();
    while rdtsc().wrapping_sub(start) < cycles {
        core::hint::spin_loop();
    }
}
