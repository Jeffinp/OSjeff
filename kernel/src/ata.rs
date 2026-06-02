//! Minimal ATA PIO (28-bit LBA) driver for the filesystem disk.
//!
//! Targets the **secondary IDE channel master** (ports `0x170`/`0x376`), kept
//! separate from the boot disk on the primary channel. Polled (no IRQ/DMA),
//! which is plenty for flushing a ~17 KiB filesystem image. Every wait is
//! bounded: a missing or wedged drive returns `false` instead of hanging, so the
//! caller can fall back to a RAM-only filesystem.

use crate::io::{inb, inw, outb, outw};

const BASE: u16 = 0x170; // secondary channel I/O base
const CTRL: u16 = 0x376; // secondary channel control / alternate status

const REG_DATA: u16 = BASE;
const REG_SECCOUNT: u16 = BASE + 2;
const REG_LBA0: u16 = BASE + 3;
const REG_LBA1: u16 = BASE + 4;
const REG_LBA2: u16 = BASE + 5;
const REG_DRIVE: u16 = BASE + 6;
const REG_STATUS: u16 = BASE + 7;
const REG_CMD: u16 = BASE + 7;

const SR_BSY: u8 = 0x80;
const SR_DRQ: u8 = 0x08;
const SR_ERR: u8 = 0x01;

const CMD_READ: u8 = 0x20;
const CMD_WRITE: u8 = 0x30;
const CMD_FLUSH: u8 = 0xE7;

const SECTOR: usize = 512;
const SPIN: u32 = 1_000_000; // bounded poll budget

/// ~400ns settle: the spec says read the alternate status four times after
/// selecting a drive before trusting BSY.
fn settle() {
    for _ in 0..4 {
        let _ = inb(CTRL);
    }
}

fn wait_not_busy() -> bool {
    for _ in 0..SPIN {
        let s = inb(REG_STATUS);
        if s == 0xFF {
            return false; // floating bus: no drive present
        }
        if s & SR_BSY == 0 {
            return true;
        }
    }
    false
}

/// Wait until the drive is ready to transfer a data word (DRQ set, BSY clear).
fn wait_drq() -> bool {
    for _ in 0..SPIN {
        let s = inb(REG_STATUS);
        if s == 0xFF || s & SR_ERR != 0 {
            return false;
        }
        if s & SR_BSY == 0 && s & SR_DRQ != 0 {
            return true;
        }
    }
    false
}

/// Select the master drive in LBA mode and program the starting LBA + count.
fn setup(lba: u32, sectors: u8) -> bool {
    if !wait_not_busy() {
        return false;
    }
    outb(REG_DRIVE, 0xE0 | (((lba >> 24) & 0x0F) as u8));
    settle();
    outb(REG_SECCOUNT, sectors);
    outb(REG_LBA0, (lba & 0xFF) as u8);
    outb(REG_LBA1, ((lba >> 8) & 0xFF) as u8);
    outb(REG_LBA2, ((lba >> 16) & 0xFF) as u8);
    true
}

/// Read `buf` (a multiple of 512 bytes) from LBA 0. Returns `false` if no drive
/// responds or a transfer stalls.
pub fn read_image(buf: &mut [u8]) -> bool {
    let sectors = buf.len() / SECTOR;
    if sectors == 0 || sectors > 255 {
        return false;
    }
    if !setup(0, sectors as u8) {
        return false;
    }
    outb(REG_CMD, CMD_READ);
    for s in 0..sectors {
        if !wait_drq() {
            return false;
        }
        let off = s * SECTOR;
        for w in 0..(SECTOR / 2) {
            let word = inw(REG_DATA);
            buf[off + w * 2] = (word & 0xFF) as u8;
            buf[off + w * 2 + 1] = (word >> 8) as u8;
        }
    }
    true
}

/// Write `buf` (a multiple of 512 bytes) to LBA 0 and flush the drive cache.
pub fn write_image(buf: &[u8]) -> bool {
    let sectors = buf.len() / SECTOR;
    if sectors == 0 || sectors > 255 {
        return false;
    }
    if !setup(0, sectors as u8) {
        return false;
    }
    outb(REG_CMD, CMD_WRITE);
    for s in 0..sectors {
        if !wait_drq() {
            return false;
        }
        let off = s * SECTOR;
        for w in 0..(SECTOR / 2) {
            let lo = buf[off + w * 2] as u16;
            let hi = buf[off + w * 2 + 1] as u16;
            outw(REG_DATA, lo | (hi << 8));
        }
    }
    if !wait_not_busy() {
        return false;
    }
    outb(REG_CMD, CMD_FLUSH);
    wait_not_busy()
}
