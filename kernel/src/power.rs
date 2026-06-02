//! System power control: reboot and shutdown through platform I/O ports.
//!
//! There is no firmware/ACPI driver here, so these use the simple legacy and
//! virtual-machine mechanisms. On QEMU (how this OS is normally run) both work;
//! on bare metal without these ports they fall through to a halt.

use crate::io::{outb, outw};

/// Reboot the machine: pulse the 8042 keyboard-controller reset line, then the
/// PCI reset register. Halts if neither resets the CPU.
pub fn reboot() -> ! {
    outb(0x64, 0xFE); // 8042: assert CPU reset
    outb(0xCF9, 0x06); // PCI reset control: full reset
    halt()
}

/// Power off via the ACPI/legacy poweroff ports exposed by QEMU, Bochs, and
/// common virtual machines. Halts if none of them power the system down.
pub fn shutdown() -> ! {
    outw(0x604, 0x2000); // QEMU (modern)
    outw(0xB004, 0x2000); // Bochs / older QEMU
    outw(0x4004, 0x3400); // cloud-hypervisor / virt
    halt()
}

fn halt() -> ! {
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)) };
    }
}
