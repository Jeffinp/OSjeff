//! Reads wall-clock time from the CMOS RTC (UTC). Polling, no interrupts.

use crate::io::{inb, outb};

const ADDR: u16 = 0x70;
const DATA: u16 = 0x71;

fn read_reg(reg: u8) -> u8 {
    outb(ADDR, reg);
    inb(DATA)
}

fn update_in_progress() -> bool {
    read_reg(0x0A) & 0x80 != 0
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Time {
    pub h: u8,
    pub m: u8,
    pub s: u8,
}

/// Reads (hours, minutes, seconds). Handles BCD and 12h formats per RTC reg B.
pub fn now() -> Time {
    while update_in_progress() {}
    let mut s = read_reg(0x00);
    let mut m = read_reg(0x02);
    let mut h = read_reg(0x04);
    let regb = read_reg(0x0B);

    if regb & 0x04 == 0 {
        // BCD -> binary.
        s = (s & 0x0F) + (s >> 4) * 10;
        m = (m & 0x0F) + (m >> 4) * 10;
        let pm = h & 0x80 != 0; // 12h mode keeps PM flag in high bit
        h = ((h & 0x0F) + ((h & 0x70) >> 4) * 10) | (if pm { 0x80 } else { 0 });
    }

    // Convert 12h -> 24h when RTC is in 12h mode (reg B bit 1 == 0).
    if regb & 0x02 == 0 {
        let pm = h & 0x80 != 0;
        h &= 0x7F;
        if pm && h != 12 {
            h += 12;
        } else if !pm && h == 12 {
            h = 0;
        }
    }

    Time { h, m, s }
}
