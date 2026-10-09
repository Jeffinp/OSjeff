//! ATA `IDENTIFY DEVICE` parsing and PIO command arithmetic.
//!
//! The kernel driver talks to the I/O ports; everything that merely *interprets*
//! the 256-word IDENTIFY block, validates and slices transfers, or computes
//! register values lives here.

use crate::storage::blockdev::IoError;

/// Bytes per ATA sector.
pub const SECTOR: usize = 512;

/// Largest sector count one PIO command can carry (the count register is 8 bits;
/// 0 would mean 256 and is deliberately never used).
pub const MAX_SECTORS_PER_CMD: usize = 255;

/// What `IDENTIFY DEVICE` tells us about a drive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskInfo {
    /// ATA model string (ASCII, space-padded; `model_len` trims trailing spaces).
    pub model: [u8; 40],
    pub model_len: usize,
    /// Total addressable 512-byte sectors.
    pub sectors: u64,
    /// True when the drive reports a non-rotating medium (rotation rate == 1).
    pub ssd: bool,
    /// Nominal rotation rate in RPM, or 0 when not reported / SSD.
    pub rpm: u16,
}

impl DiskInfo {
    /// Capacity in whole mebibytes.
    pub fn mib(&self) -> u64 {
        // 512-byte sectors: 2048 per MiB. Dividing (rather than multiplying by
        // 512 first) cannot overflow for any 48-bit sector count.
        self.sectors / (1024 * 1024 / SECTOR as u64)
    }

    /// The model string without padding, or `"?"` if it is not valid UTF-8.
    pub fn model_name(&self) -> &str {
        let end = self.model_len.min(self.model.len());
        core::str::from_utf8(&self.model[..end])
            .unwrap_or("?")
            .trim()
    }
}

/// Parses the 256-word IDENTIFY block.
pub fn parse_identify(id: &[u16; 256]) -> DiskInfo {
    // Model: words 27..=46, big-endian within each word (byte-swapped).
    let mut model = [b' '; 40];
    for (i, &w) in id[27..47].iter().enumerate() {
        model[i * 2] = (w >> 8) as u8;
        model[i * 2 + 1] = (w & 0xFF) as u8;
    }
    let model_len = model
        .iter()
        .rposition(|&b| b != b' ' && b != 0)
        .map_or(0, |p| p + 1);

    // Sector count: 48-bit (words 100..=103) if present, else 28-bit (words 60/61).
    let lba48 = (id[100] as u64)
        | ((id[101] as u64) << 16)
        | ((id[102] as u64) << 32)
        | ((id[103] as u64) << 48);
    let lba28 = (id[60] as u64) | ((id[61] as u64) << 16);
    let sectors = if lba48 != 0 { lba48 } else { lba28 };

    // Word 217: nominal media rotation rate. 1 = non-rotating (SSD).
    let rot = id[217];
    let ssd = rot == 1;
    let rpm = if (0x0401..=0xFFFE).contains(&rot) {
        rot
    } else {
        0
    };

    DiskInfo {
        model,
        model_len,
        sectors,
        ssd,
        rpm,
    }
}

/// Number of whole sectors in a transfer buffer, if one PIO command can carry
/// it (1..=255 sectors). `buf_len` must be a multiple of 512 to be exact;
/// a partial trailing sector is ignored, matching the driver.
pub fn sector_count(buf_len: usize) -> Option<u8> {
    let n = buf_len / SECTOR;
    if n == 0 || n > MAX_SECTORS_PER_CMD {
        None
    } else {
        Some(n as u8)
    }
}

/// Register values for an LBA28 command on the master drive:
/// `[drive_select, lba_low, lba_mid, lba_high]`. Bits 28..32 of `lba` do not fit
/// in 28-bit addressing and are dropped.
pub fn lba28_regs(lba: u32) -> [u8; 4] {
    [
        0xE0 | (((lba >> 24) & 0x0F) as u8), // 0xE0 = master, LBA mode
        (lba & 0xFF) as u8,
        ((lba >> 8) & 0xFF) as u8,
        ((lba >> 16) & 0xFF) as u8,
    ]
}

/// Status register: BSY (drive busy, the other bits are meaningless).
pub const SR_BSY: u8 = 0x80;
/// Status register: DF (device fault).
pub const SR_DF: u8 = 0x20;
/// Status register: DRQ (data request: a sector can be transferred).
pub const SR_DRQ: u8 = 0x08;
/// Status register: ERR (the command failed).
pub const SR_ERR: u8 = 0x01;

/// Sectors addressable with 28-bit LBA (2^28, 128 GiB). The driver only issues
/// `READ SECTORS`/`WRITE SECTORS`, never the EXT forms, so a larger disk is
/// treated as if it ended here.
pub const LBA28_SECTORS: u64 = 1 << 28;

/// What one status-register sample means to the driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// `0xFF`: nothing drives the bus (no drive, or a dead controller).
    Floating,
    /// BSY set: keep waiting.
    Busy,
    /// ERR or DF set while not busy: the command failed.
    Fault,
    /// Not busy, DRQ set: the drive wants or offers a data sector.
    Drq,
    /// Not busy, no DRQ, no error: command complete.
    Ready,
}

/// Classify one status sample. A floating bus is checked first, then BSY
/// (the other bits are undefined while the drive is busy), then faults (a failed
/// command may leave DRQ up), then DRQ.
pub fn classify_status(s: u8) -> Status {
    if s == 0xFF {
        Status::Floating
    } else if s & SR_BSY != 0 {
        Status::Busy
    } else if s & (SR_ERR | SR_DF) != 0 {
        Status::Fault
    } else if s & SR_DRQ != 0 {
        Status::Drq
    } else {
        Status::Ready
    }
}

/// Sectors the driver may address on a drive that identified itself with
/// `identified` sectors (capped at the LBA28 limit).
pub fn usable_sectors(identified: u64) -> u64 {
    identified.min(LBA28_SECTORS)
}

/// Validate a transfer of `buf_len` bytes at `lba` on a device of `total`
/// sectors and return its length in sectors (0 for an empty buffer, which is a
/// no-op). `BadLength` when `buf_len` is not a multiple of 512, `OutOfRange` when
/// the range passes `total` or the 28-bit LBA limit.
pub fn check_transfer(total: u64, lba: u64, buf_len: usize) -> Result<usize, IoError> {
    if !buf_len.is_multiple_of(SECTOR) {
        return Err(IoError::BadLength);
    }
    let n = buf_len / SECTOR;
    let end = lba.checked_add(n as u64).ok_or(IoError::OutOfRange)?;
    if end > total || end > LBA28_SECTORS {
        return Err(IoError::OutOfRange);
    }
    Ok(n)
}

/// One PIO command of a sliced transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chunk {
    /// First sector (28-bit LBA).
    pub lba: u32,
    /// Sectors in this command (1..=255).
    pub sectors: u8,
    /// Byte offset of this chunk inside the caller's buffer.
    pub offset: usize,
}

/// Iterator over the commands that carry a transfer, each at most
/// [`MAX_SECTORS_PER_CMD`] sectors. The range must already have passed
/// [`check_transfer`]; a longer one simply stops at the 28-bit limit.
#[derive(Clone, Debug)]
pub struct Chunks {
    lba: u64,
    left: usize,
    offset: usize,
}

/// Slice `sectors` sectors starting at `lba` into ATA commands of at most 255.
pub fn chunks(lba: u64, sectors: usize) -> Chunks {
    Chunks {
        lba,
        left: sectors,
        offset: 0,
    }
}

impl Iterator for Chunks {
    type Item = Chunk;
    fn next(&mut self) -> Option<Chunk> {
        if self.left == 0 || self.lba >= LBA28_SECTORS {
            return None;
        }
        let n = self.left.min(MAX_SECTORS_PER_CMD);
        let c = Chunk {
            lba: self.lba as u32,
            sectors: n as u8,
            offset: self.offset,
        };
        self.lba += n as u64;
        self.left -= n;
        self.offset += n * SECTOR;
        Some(c)
    }
}

#[cfg(test)]
mod tests;
