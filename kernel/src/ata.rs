//! Minimal ATA PIO (28-bit LBA) driver for the filesystem disk.
//!
//! Targets the **secondary IDE channel master** (ports `0x170`/`0x376`), kept
//! separate from the boot disk on the primary channel. Polled (no IRQ/DMA),
//! which is plenty for flushing a ~49 KiB (99-sector) filesystem image. Every wait is
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
const CMD_IDENTIFY: u8 = 0xEC;

/// Standard IDE channels: `(io_base, control, label)`. In our QEMU setup the
/// primary master is the boot/OS disk and the secondary master holds the
/// filesystem image (see `run.ps1`).
pub const CHANNELS: [(u16, u16, &str); 2] = [
    (0x1F0, 0x3F6, "primario (boot)"),
    (0x170, 0x376, "secundario (FS)"),
];

pub use osjeff_core::hw::ata::DiskInfo;
use osjeff_core::hw::ata::{SECTOR, lba28_regs, parse_identify, sector_count};

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
    let [drive, lba0, lba1, lba2] = lba28_regs(lba);
    outb(REG_DRIVE, drive);
    settle();
    outb(REG_SECCOUNT, sectors);
    outb(REG_LBA0, lba0);
    outb(REG_LBA1, lba1);
    outb(REG_LBA2, lba2);
    true
}

/// Bounded wait for BSY to clear on an arbitrary channel's status port.
fn wait_bsy_at(status: u16) -> bool {
    for _ in 0..SPIN {
        let s = inb(status);
        if s == 0xFF {
            return false;
        }
        if s & SR_BSY == 0 {
            return true;
        }
    }
    false
}

/// Run `IDENTIFY DEVICE` on `(base, ctrl)` master/slave and parse the result.
/// `None` when no ATA drive answers (floating bus, ATAPI/SATA signature, or a
/// stalled transfer). Read-only: safe to probe every channel at boot.
pub fn identify(base: u16, ctrl: u16, slave: bool) -> Option<DiskInfo> {
    let status = base + 7;
    if !wait_bsy_at(status) {
        return None;
    }
    outb(base + 6, if slave { 0xB0 } else { 0xA0 });
    for _ in 0..4 {
        let _ = inb(ctrl);
    }
    // Zero the sector-count/LBA registers, then issue IDENTIFY.
    outb(base + 2, 0);
    outb(base + 3, 0);
    outb(base + 4, 0);
    outb(base + 5, 0);
    outb(base + 7, CMD_IDENTIFY);

    if inb(status) == 0 {
        return None; // no drive on this slot
    }
    if !wait_bsy_at(status) {
        return None;
    }
    // A non-zero LBA1/LBA2 here is an ATAPI/SATA signature, not a plain ATA disk.
    if inb(base + 4) != 0 || inb(base + 5) != 0 {
        return None;
    }
    // Wait for DRQ (or error).
    let mut ok = false;
    for _ in 0..SPIN {
        let s = inb(status);
        if s & SR_ERR != 0 || s == 0xFF {
            return None;
        }
        if s & SR_DRQ != 0 {
            ok = true;
            break;
        }
    }
    if !ok {
        return None;
    }

    let mut id = [0u16; 256];
    for w in id.iter_mut() {
        *w = inw(base);
    }

    Some(parse_identify(&id))
}

/// Probe every standard channel and log a one-line summary per present drive.
/// This is the "detect where the OS is installed + HD vs SSD" report.
pub fn detect_and_log() {
    crate::serial_println!("ATA: scanning disks");
    for (base, ctrl, label) in CHANNELS {
        match identify(base, ctrl, false) {
            Some(d) => {
                let model = d.model_name();
                let kind = if d.ssd {
                    "SSD"
                } else if d.rpm > 0 {
                    "HD"
                } else {
                    "HD/desconhecido"
                };
                crate::serial_println!(
                    "  {} :: {} | {} MiB | {} (rot={})",
                    label,
                    model,
                    d.mib(),
                    kind,
                    d.rpm
                );
            }
            None => crate::serial_println!("  {} :: ausente", label),
        }
    }
}

/// Read `buf` (a multiple of 512 bytes) from LBA 0. Returns `false` if no drive
/// responds or a transfer stalls.
pub fn read_image(buf: &mut [u8]) -> bool {
    let t0 = crate::trace::t();
    let ok = read_image_inner(buf);
    if crate::trace::ON {
        use core::sync::atomic::Ordering::Relaxed;
        crate::trace::ATA_R_N.fetch_add(1, Relaxed);
        crate::trace::ATA_R_CYC.fetch_add(crate::io::rdtsc().wrapping_sub(t0), Relaxed);
    }
    ok
}

fn read_image_inner(buf: &mut [u8]) -> bool {
    let Some(count) = sector_count(buf.len()) else {
        return false;
    };
    let sectors = count as usize;
    if !setup(0, count) {
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
    let t0 = crate::trace::t();
    let ok = write_image_inner(buf);
    if crate::trace::ON {
        use core::sync::atomic::Ordering::Relaxed;
        crate::trace::ATA_W_N.fetch_add(1, Relaxed);
        crate::trace::ATA_W_CYC.fetch_add(crate::io::rdtsc().wrapping_sub(t0), Relaxed);
    }
    ok
}

fn write_image_inner(buf: &[u8]) -> bool {
    let Some(count) = sector_count(buf.len()) else {
        return false;
    };
    let sectors = count as usize;
    if !setup(0, count) {
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
