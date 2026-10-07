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

/// Current date and time as seconds since the Unix epoch, **UTC** (no
/// [`TZ_OFFSET_HOURS`] shift: that is a display concern), for filesystem
/// timestamps. The registers are read until two consecutive reads agree, so a
/// rollover between them cannot produce a torn value. Returns 0 if the RTC holds
/// an impossible date (dead battery, unset clock).
pub fn now_unix() -> u64 {
    use osjeff_core::hw::rtc::{RawDateTime, decode_unix};
    let read = || {
        // The update flag is set for ~2 ms once a second; bounded so a stuck
        // register cannot hang the caller.
        for _ in 0..100_000 {
            if !update_in_progress() {
                break;
            }
        }
        (
            RawDateTime {
                sec: read_reg(0x00),
                min: read_reg(0x02),
                hour: read_reg(0x04),
                day: read_reg(0x07),
                month: read_reg(0x08),
                year: read_reg(0x09),
                century: read_reg(0x32),
            },
            read_reg(0x0B),
        )
    };
    let mut cur = read();
    for _ in 0..4 {
        let next = read();
        if next == cur {
            break;
        }
        cur = next;
    }
    decode_unix(cur.0, cur.1).unwrap_or(0)
}
