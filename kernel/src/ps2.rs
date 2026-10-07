//! PS/2 mouse + keyboard. The controller is configured here; bytes arrive via
//! IRQ1/IRQ12 (see `interrupts`) and land in a ring buffer. `poll` drains that
//! ring and decodes scancodes / 3-byte mouse packets into [`Event`]s.

use crate::interrupts::{self, SRC_KEYBOARD};
use crate::io::{inb, outb};
use crate::sync::RacyCell;

const DATA: u16 = 0x60;
const STATUS: u16 = 0x64;
const CMD: u16 = 0x64;

fn wait_write() {
    for _ in 0..100_000 {
        if inb(STATUS) & 0x02 == 0 {
            return;
        }
    }
}

fn wait_read() {
    for _ in 0..100_000 {
        if inb(STATUS) & 0x01 == 1 {
            return;
        }
    }
}

fn mouse_command(cmd: u8) {
    wait_write();
    outb(CMD, 0xD4); // next byte goes to the mouse
    wait_write();
    outb(DATA, cmd);
    wait_read();
    let _ack = inb(DATA);
}

/// Enables the aux device, turns on keyboard + mouse IRQs, and starts the
/// mouse data stream. Called after the IDT/PIC are up.
pub fn init() {
    wait_write();
    outb(CMD, 0xA8); // enable aux (mouse) device

    // Read controller config; enable IRQ1 (keyboard) + IRQ12 (mouse) and the
    // mouse clock.
    wait_write();
    outb(CMD, 0x20);
    wait_read();
    let mut config = inb(DATA);
    config |= 0x01; // keyboard interrupt
    config |= 0x02; // mouse interrupt
    config &= !0x20; // clear "mouse clock disabled"
    wait_write();
    outb(CMD, 0x60);
    wait_write();
    outb(DATA, config);

    mouse_command(0xF6); // set defaults
    // IntelliMouse handshake (sample rates 200, 100, 80), then ask for the device ID:
    // 3 means the mouse has a wheel and sends 4-byte packets. Runs before the IRQs are
    // unmasked, so the replies are read straight from the port.
    for rate in [200u8, 100, 80] {
        mouse_command(0xF3);
        mouse_command(rate);
    }
    mouse_command(0xF2);
    wait_read();
    if inb(DATA) == 3 {
        WHEEL_MODE.store(true, core::sync::atomic::Ordering::Relaxed);
    }
    mouse_command(0xF4); // enable data reporting
}

/// Whether the mouse negotiated the 4-byte (wheel) protocol at boot.
static WHEEL_MODE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub use osjeff_core::hw::ps2::{Decoder, Event};

/// Incremental decoder state for the scancode/mouse-packet state machines.
/// Touched only by [`poll`] on the main loop (the sole consumer of the IRQ
/// ring), so a single non-reentrant owner — see [`RacyCell`].
static DECODER: RacyCell<Decoder> = RacyCell::new(Decoder::new());

/// Drains the IRQ ring until a complete event is decoded, or it empties.
pub fn poll() -> Option<Event> {
    loop {
        let raw = interrupts::read_input()?;
        let source = raw >> 8;
        let data = (raw & 0xFF) as u8;

        // SAFETY: DECODER is only used here, in `poll()` on the compositor thread (ISRs only fill
        // the ring); calls are sequential, so this `&mut` is unique.
        let d = unsafe { &mut *DECODER.get() };
        d.set_wheel_mode(WHEEL_MODE.load(core::sync::atomic::Ordering::Relaxed));
        let event = if source == SRC_KEYBOARD {
            d.keyboard(data)
        } else {
            d.mouse(data)
        };
        if event.is_some() {
            return event;
        }
        // Otherwise the byte was a prefix / mid-packet: keep draining.
    }
}
