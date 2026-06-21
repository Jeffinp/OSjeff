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
    mouse_command(0xF4); // enable data reporting
}

pub struct Packet {
    pub dx: i32,
    pub dy: i32,
    pub left: bool,
    pub right: bool,
}

pub struct KeyEvent {
    pub scan_code: u8,
    pub pressed: bool,
    /// Set when the scancode followed an `0xE0` prefix (arrows, Del, etc.).
    pub extended: bool,
}

pub enum Event {
    Mouse(Packet),
    Key(KeyEvent),
}

/// Incremental decoder state for the scancode/mouse-packet state machines.
/// Touched only by [`poll`] on the main loop (the sole consumer of the IRQ
/// ring), so a single non-reentrant owner — see [`RacyCell`].
struct DecoderState {
    mouse_cycle: u8,
    mouse_buf: [u8; 3],
    key_extended: bool,
}

static DECODER: RacyCell<DecoderState> = RacyCell::new(DecoderState {
    mouse_cycle: 0,
    mouse_buf: [0; 3],
    key_extended: false,
});

/// Drains the IRQ ring until a complete event is decoded, or it empties.
pub fn poll() -> Option<Event> {
    loop {
        let raw = interrupts::read_input()?;
        let source = raw >> 8;
        let data = (raw & 0xFF) as u8;

        let event = if source == SRC_KEYBOARD {
            decode_keyboard(data)
        } else {
            decode_mouse(data)
        };
        if event.is_some() {
            return event;
        }
        // Otherwise the byte was a prefix / mid-packet: keep draining.
    }
}

fn decode_keyboard(data: u8) -> Option<Event> {
    let d = unsafe { &mut *DECODER.get() };
    if data == 0xE0 {
        d.key_extended = true;
        return None; // prefix; real scancode is the next byte
    }
    let extended = d.key_extended;
    d.key_extended = false;
    Some(Event::Key(KeyEvent {
        scan_code: data & 0x7F,
        pressed: data & 0x80 == 0,
        extended,
    }))
}

fn decode_mouse(data: u8) -> Option<Event> {
    let d = unsafe { &mut *DECODER.get() };
    match d.mouse_cycle {
        0 => {
            if data & 0x08 == 0 {
                return None; // out of sync; bit3 of byte0 is always 1
            }
            d.mouse_buf[0] = data;
            d.mouse_cycle = 1;
            None
        }
        1 => {
            d.mouse_buf[1] = data;
            d.mouse_cycle = 2;
            None
        }
        _ => {
            d.mouse_buf[2] = data;
            d.mouse_cycle = 0;
            let flags = d.mouse_buf[0];
            let mut dx = d.mouse_buf[1] as i32;
            let mut dy = d.mouse_buf[2] as i32;
            if flags & 0x10 != 0 {
                dx -= 256;
            }
            if flags & 0x20 != 0 {
                dy -= 256;
            }
            Some(Event::Mouse(Packet {
                dx,
                dy,
                left: flags & 0x01 != 0,
                right: flags & 0x02 != 0,
            }))
        }
    }
}
