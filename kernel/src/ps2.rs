//! PS/2 mouse driver (polling mode, no IRQ). Returns movement deltas + button state.

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

static mut CYCLE: u8 = 0;
static mut BUF: [u8; 3] = [0; 3];

/// Non-blocking poll. Returns a packet only when a full 3-byte frame arrives.
pub fn poll() -> Option<Packet> {
    let status = inb(STATUS);
    if status & 0x01 == 0 {
        return None; // output buffer empty
    }
    if status & 0x20 == 0 {
        let _ = inb(DATA); // keyboard byte; discard
        return None;
    }
    let data = inb(DATA);

    unsafe {
        match CYCLE {
            0 => {
                if data & 0x08 == 0 {
                    return None; // out of sync; bit3 of byte0 is always 1
                }
                BUF[0] = data;
                CYCLE = 1;
            }
            1 => {
                BUF[1] = data;
                CYCLE = 2;
            }
            _ => {
                BUF[2] = data;
                CYCLE = 0;
                let flags = BUF[0];
                let mut dx = BUF[1] as i32;
                let mut dy = BUF[2] as i32;
                if flags & 0x10 != 0 {
                    dx -= 256;
                }
                if flags & 0x20 != 0 {
                    dy -= 256;
                }
                return Some(Packet {
                    dx,
                    dy,
                    left: flags & 0x01 != 0,
                });
            }
        }
    }
    None
}
