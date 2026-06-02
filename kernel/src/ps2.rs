//! PS/2 driver for mouse + keyboard in polling mode.

use crate::io::{inb, outb};

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

/// Enables the auxiliary device and turns on data reporting.
pub fn init() {
    wait_write();
    outb(CMD, 0xA8); // enable aux device

    // Read controller config, enable mouse clock + IRQ12.
    wait_write();
    outb(CMD, 0x20);
    wait_read();
    let mut config = inb(DATA);
    config |= 0x02; // IRQ12 (harmless under polling)
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

static mut MOUSE_CYCLE: u8 = 0;
static mut MOUSE_BUF: [u8; 3] = [0; 3];
static mut KEY_EXTENDED: bool = false;

/// Non-blocking poll. Returns one PS/2 event at a time.
pub fn poll() -> Option<Event> {
    let status = inb(STATUS);
    if status & 0x01 == 0 {
        return None; // output buffer empty
    }

    let data = inb(DATA);
    if status & 0x20 == 0 {
        // Keyboard data.
        if data == 0xE0 {
            unsafe { KEY_EXTENDED = true };
            return None; // prefix only; the real code is the next byte
        }

        let extended = unsafe { KEY_EXTENDED };
        unsafe { KEY_EXTENDED = false };

        let pressed = data & 0x80 == 0;
        let scan_code = data & 0x7F;
        return Some(Event::Key(KeyEvent {
            scan_code,
            pressed,
            extended,
        }));
    }

    // Mouse data
    unsafe {
        match MOUSE_CYCLE {
            0 => {
                if data & 0x08 == 0 {
                    return None; // out of sync; bit3 of byte0 is always 1
                }
                MOUSE_BUF[0] = data;
                MOUSE_CYCLE = 1;
            }
            1 => {
                MOUSE_BUF[1] = data;
                MOUSE_CYCLE = 2;
            }
            _ => {
                MOUSE_BUF[2] = data;
                MOUSE_CYCLE = 0;
                let flags = MOUSE_BUF[0];
                let mut dx = MOUSE_BUF[1] as i32;
                let mut dy = MOUSE_BUF[2] as i32;
                if flags & 0x10 != 0 {
                    dx -= 256;
                }
                if flags & 0x20 != 0 {
                    dy -= 256;
                }
                return Some(Event::Mouse(Packet {
                    dx,
                    dy,
                    left: flags & 0x01 != 0,
                }));
            }
        }
    }
    None
}
