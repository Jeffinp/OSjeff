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

/// One byte from the data port, or `None` if the device stays silent.
fn read_timeout() -> Option<u8> {
    for _ in 0..100_000 {
        if inb(STATUS) & 0x01 == 1 {
            return Some(inb(DATA));
        }
    }
    None
}

/// Send `cmd` (or a command argument) to the mouse. Returns whether it answered ACK (`0xFA`).
fn mouse_command(cmd: u8) -> bool {
    wait_write();
    outb(CMD, 0xD4); // next byte goes to the mouse
    wait_write();
    outb(DATA, cmd);
    read_timeout() == Some(0xFA)
}

/// Try the IntelliMouse handshake: sample rates 200, 100, 80, then "get device id". A mouse with a
/// wheel answers 3 and from then on sends 4-byte packets; anything else (id 0, no ACK, silence)
/// keeps the 3-byte framing. Reporting is off while this runs, so the replies are not mixed with
/// motion packets. Polled: interrupts are still off here.
fn negotiate_wheel() -> MouseMode {
    for rate in WHEEL_MAGIC {
        if !(mouse_command(0xF3) && mouse_command(rate)) {
            return MouseMode::Basic;
        }
    }
    if !mouse_command(0xF2) {
        return MouseMode::Basic;
    }
    MouseMode::from_id(read_timeout())
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

    mouse_command(0xF5); // disable data reporting while we probe
    mouse_command(0xF6); // set defaults
    let mode = if WHEEL_ENABLED {
        negotiate_wheel()
    } else {
        MouseMode::Basic
    };
    crate::serial_println!(
        "ps2: mouse {}",
        match mode {
            MouseMode::Wheel => "id 3 (wheel, 4-byte packets)",
            MouseMode::Basic => "id 0 (no wheel, 3-byte packets)",
        }
    );
    // SAFETY: `init` runs once from `kernel_main` before interrupts are enabled and before the
    // compositor's `poll` loop starts, so nothing else touches DECODER yet.
    unsafe {
        (*DECODER.get()).set_mode(mode);
    }
    mouse_command(0xF4); // enable data reporting
}

/// Whether to attempt the IntelliMouse handshake (kept as a switch so the 3-byte fallback can be
/// exercised on a mouse that does have a wheel).
const WHEEL_ENABLED: bool = true;

pub use kitsune_core::hw::ps2::{Decoder, Event, MouseMode, WHEEL_MAGIC};

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
