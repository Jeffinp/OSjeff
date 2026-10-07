//! Reads wall-clock time from the CMOS RTC. Polling, no interrupts.
//!
//! The RTC under QEMU reports UTC; [`TZ_OFFSET_HOURS`] shifts it to local time
//! (default -3, Brasília).

use crate::io::{inb, outb};

const ADDR: u16 = 0x70;
const DATA: u16 = 0x71;

/// Hours to add to UTC for local time. Brasília = -3.
pub const TZ_OFFSET_HOURS: i32 = -3;

fn read_reg(reg: u8) -> u8 {
    outb(ADDR, reg);
    inb(DATA)
}

fn update_in_progress() -> bool {
    read_reg(0x0A) & 0x80 != 0
}

pub use osjeff_core::hw::rtc::Time;

/// Reads (hours, minutes, seconds). Handles BCD and 12h formats per RTC reg B.
pub fn now() -> Time {
    while update_in_progress() {}
    let s = read_reg(0x00);
    let m = read_reg(0x02);
    let h = read_reg(0x04);
    let regb = read_reg(0x0B);
    osjeff_core::hw::rtc::decode(s, m, h, regb, TZ_OFFSET_HOURS)
}
