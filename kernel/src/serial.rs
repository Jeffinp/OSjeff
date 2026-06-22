//! Minimal COM1 (16550 UART) serial output for kernel debug logging.
//!
//! QEMU can capture this to a file (`-serial file:serial.log`), giving a text
//! verification channel that is independent of the framebuffer — invaluable for
//! bringing up drivers (PCI, virtio-gpu) where the screen may not show progress.

use crate::io::{inb, outb};

const COM1: u16 = 0x3F8;

/// Program COM1 for 38400 baud, 8N1, FIFO on, interrupts off.
pub fn init() {
    outb(COM1 + 1, 0x00); // disable UART interrupts
    outb(COM1 + 3, 0x80); // enable DLAB (set baud divisor)
    outb(COM1, 0x03); // divisor low  = 3 -> 38400 baud
    outb(COM1 + 1, 0x00); // divisor high = 0
    outb(COM1 + 3, 0x03); // 8 bits, no parity, one stop bit
    outb(COM1 + 2, 0xC7); // enable + clear FIFO, 14-byte threshold
    outb(COM1 + 4, 0x0B); // RTS/DSR set, IRQs off
}

#[inline]
fn write_byte(b: u8) {
    // Spin until the transmit-holding register is empty (LSR bit 5).
    while inb(COM1 + 5) & 0x20 == 0 {}
    outb(COM1, b);
}

/// Write a string, translating `\n` to `\r\n` for terminal-friendly output.
pub fn write_str(s: &str) {
    for b in s.bytes() {
        if b == b'\n' {
            write_byte(b'\r');
        }
        write_byte(b);
    }
}

/// Zero-sized [`core::fmt::Write`] sink so `write!`/`writeln!` work without alloc.
pub struct Serial;

impl core::fmt::Write for Serial {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        write_str(s);
        Ok(())
    }
}

/// `println!`-style logging to COM1.
#[macro_export]
macro_rules! serial_println {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let _ = writeln!($crate::serial::Serial, $($arg)*);
    }};
}

/// `print!`-style logging to COM1 (no trailing newline). Used by the WASM host
/// `log` syscall, which streams raw guest output that may not be line-aligned.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let _ = write!($crate::serial::Serial, $($arg)*);
    }};
}
