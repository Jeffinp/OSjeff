//! Reads and sets the wall-clock time of the CMOS RTC. Polling, no interrupts.
//!
//! The RTC holds UTC. [`now`] returns local time of day, shifted by the zone
//! offset the settings app chose ([`set_tz_minutes`], default -3 h = Brasilia);
//! the date, which only the settings app needs, comes from [`read_utc`] and
//! [`set_utc`]. The BCD / 12-hour / century logic is `kitsune_core::hw::rtc`.

use crate::io::{inb, outb};
use core::sync::atomic::{AtomicI32, Ordering};
use kitsune_core::hw::rtc::{DateTime, RawRtc, decode, decode_datetime, encode_datetime};

const ADDR: u16 = 0x70;
const DATA: u16 = 0x71;

/// Hours to add to UTC for local time before the user picks a zone. Brasilia = -3.
pub const TZ_OFFSET_HOURS: i32 = -3;

/// Current zone offset in minutes east of UTC.
static TZ_MINUTES: AtomicI32 = AtomicI32::new(TZ_OFFSET_HOURS * 60);

/// Choose the zone offset (minutes east of UTC) [`now`] applies.
pub fn set_tz_minutes(m: i32) {
    TZ_MINUTES.store(m, Ordering::Relaxed);
}

/// The zone offset in minutes east of UTC.
pub fn tz_minutes() -> i32 {
    TZ_MINUTES.load(Ordering::Relaxed)
}

fn read_reg(reg: u8) -> u8 {
    outb(ADDR, reg);
    inb(DATA)
}

fn write_reg(reg: u8, v: u8) {
    outb(ADDR, reg);
    outb(DATA, v);
}

fn update_in_progress() -> bool {
    read_reg(0x0A) & 0x80 != 0
}

pub use kitsune_core::hw::rtc::Time;

/// Reads (hours, minutes, seconds) of local time. Handles BCD and 12h formats per RTC reg B.
pub fn now() -> Time {
    while update_in_progress() {}
    let s = read_reg(0x00);
    let m = read_reg(0x02);
    let h = read_reg(0x04);
    let regb = read_reg(0x0B);
    kitsune_core::hw::rtc::shift_time_of_day(decode(s, m, h, regb, 0), tz_minutes())
}

/// Read the date and time registers as UTC. A coherent snapshot: if the RTC
/// ticked while the registers were being read the read is repeated.
pub fn read_utc() -> DateTime {
    loop {
        while update_in_progress() {}
        let raw = RawRtc {
            sec: read_reg(0x00),
            min: read_reg(0x02),
            hour: read_reg(0x04),
            day: read_reg(0x07),
            month: read_reg(0x08),
            year: read_reg(0x09),
            century: read_reg(0x32),
        };
        let regb = read_reg(0x0B);
        // Same seconds after the read: no update slipped in.
        if !update_in_progress() && read_reg(0x00) == raw.sec {
            return decode_datetime(raw, regb);
        }
    }
}

/// Write `utc` to the RTC, keeping the mode (BCD / binary, 12 / 24 hour) the
/// firmware chose. Updates are halted (reg B bit 7, "SET") while the registers
/// are written so a tick cannot land between two of them.
pub fn set_utc(utc: &DateTime) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let regb = read_reg(0x0B);
        let raw = encode_datetime(utc, regb);
        write_reg(0x0B, regb | 0x80);
        write_reg(0x00, raw.sec);
        write_reg(0x02, raw.min);
        write_reg(0x04, raw.hour);
        write_reg(0x07, raw.day);
        write_reg(0x08, raw.month);
        write_reg(0x09, raw.year);
        write_reg(0x32, raw.century);
        write_reg(0x0B, regb & !0x80);
    });
}

/// Current date and time as seconds since the Unix epoch, **UTC** (no
/// [`TZ_OFFSET_HOURS`] shift: that is a display concern), for filesystem
/// timestamps. The registers are read until two consecutive reads agree, so a
/// rollover between them cannot produce a torn value. Returns 0 if the RTC holds
/// an impossible date (dead battery, unset clock).
pub fn now_unix() -> u64 {
    use kitsune_core::hw::rtc::{RawDateTime, decode_unix};
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
