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
mod tests {
    use super::*;

    #[test]
    fn status_classification() {
        assert_eq!(classify_status(0xFF), Status::Floating);
        assert_eq!(classify_status(0x80), Status::Busy);
        // BSY hides ERR/DRQ: those bits are undefined while busy.
        assert_eq!(classify_status(0x80 | SR_ERR | SR_DRQ), Status::Busy);
        assert_eq!(classify_status(0x50), Status::Ready); // DRDY | DSC
        assert_eq!(classify_status(0x58), Status::Drq);
        assert_eq!(classify_status(0x51), Status::Fault); // ERR
        assert_eq!(classify_status(0x70), Status::Fault); // DF
        assert_eq!(classify_status(0x59), Status::Fault); // fault wins over DRQ
        assert_eq!(classify_status(0x00), Status::Ready);
    }

    #[test]
    fn usable_sectors_caps_at_lba28() {
        assert_eq!(usable_sectors(0), 0);
        assert_eq!(usable_sectors(131_072), 131_072); // 64 MiB
        assert_eq!(usable_sectors(1 << 28), 1 << 28);
        assert_eq!(usable_sectors(u64::MAX), 1 << 28);
    }

    #[test]
    fn check_transfer_validates_length_and_bounds() {
        assert_eq!(check_transfer(100, 0, 512), Ok(1));
        assert_eq!(check_transfer(100, 99, 512), Ok(1));
        assert_eq!(check_transfer(100, 0, 100 * 512), Ok(100));
        assert_eq!(check_transfer(100, 100, 0), Ok(0)); // empty at the end: fine
        assert_eq!(check_transfer(100, 100, 512), Err(IoError::OutOfRange));
        assert_eq!(check_transfer(100, 99, 1024), Err(IoError::OutOfRange));
        assert_eq!(check_transfer(100, 0, 511), Err(IoError::BadLength));
        assert_eq!(check_transfer(100, 0, 513), Err(IoError::BadLength));
        // BadLength wins when both are wrong, so a caller bug is reported as such.
        assert_eq!(check_transfer(100, 500, 3), Err(IoError::BadLength));
        assert_eq!(check_transfer(100, u64::MAX, 512), Err(IoError::OutOfRange));
        assert_eq!(
            check_transfer(u64::MAX, u64::MAX - 1, 1024),
            Err(IoError::OutOfRange)
        );
    }

    #[test]
    fn check_transfer_respects_the_lba28_ceiling() {
        // A device reporting more than 2^28 sectors still ends at 2^28 for us.
        let big = u64::MAX;
        assert_eq!(check_transfer(big, LBA28_SECTORS - 1, 512), Ok(1));
        assert_eq!(
            check_transfer(big, LBA28_SECTORS, 512),
            Err(IoError::OutOfRange)
        );
        assert_eq!(
            check_transfer(big, LBA28_SECTORS - 1, 1024),
            Err(IoError::OutOfRange)
        );
    }

    fn collect(lba: u64, sectors: usize) -> alloc::vec::Vec<(u32, u8, usize)> {
        chunks(lba, sectors)
            .map(|c| (c.lba, c.sectors, c.offset))
            .collect()
    }

    #[test]
    fn chunks_split_at_255_sectors() {
        assert!(collect(0, 0).is_empty());
        assert_eq!(collect(7, 1), [(7, 1, 0)]);
        assert_eq!(collect(0, 255), [(0, 255, 0)]);
        assert_eq!(collect(0, 256), [(0, 255, 0), (255, 1, 255 * 512)]);
        // 256 blocks of 4 KiB in one cache read.
        assert_eq!(
            collect(1000, 2048),
            [
                (1000, 255, 0),
                (1255, 255, 255 * 512),
                (1510, 255, 510 * 512),
                (1765, 255, 765 * 512),
                (2020, 255, 1020 * 512),
                (2275, 255, 1275 * 512),
                (2530, 255, 1530 * 512),
                (2785, 255, 1785 * 512),
                (3040, 8, 2040 * 512),
            ]
        );
    }

    #[test]
    fn chunks_cover_exactly_and_never_exceed_255() {
        let cases = [
            (0u64, 1usize),
            (128, 8),
            (5, 254),
            (5, 255),
            (5, 509),
            (5, 510),
            (5, 511),
            (3, 10_000),
        ];
        for (lba, n) in cases {
            let cs: alloc::vec::Vec<Chunk> = chunks(lba, n).collect();
            assert_eq!(cs.iter().map(|c| c.sectors as usize).sum::<usize>(), n);
            let mut next = lba;
            let mut off = 0;
            for c in &cs {
                assert!(c.sectors >= 1);
                assert_eq!(c.lba as u64, next, "chunks are contiguous");
                assert_eq!(c.offset, off);
                next += c.sectors as u64;
                off += c.sectors as usize * SECTOR;
            }
        }
    }

    #[test]
    fn chunks_stop_at_the_lba28_limit() {
        let last = LBA28_SECTORS - 2;
        assert_eq!(collect(last, 10), [(last as u32, 10, 0)]);
        // A start already past the limit yields nothing (check_transfer rejects it first).
        assert!(collect(LBA28_SECTORS, 1).is_empty());
    }

    #[test]
    fn chunks_drive_a_ram_model_end_to_end() {
        // Reading through `chunks` reproduces a straight copy: offsets line up.
        let total = 3000usize;
        let disk: alloc::vec::Vec<u8> = (0..total * SECTOR).map(|i| (i % 251) as u8).collect();
        let lba = 17u64;
        let n = 2500usize;
        let mut out = alloc::vec![0u8; n * SECTOR];
        for c in chunks(lba, n) {
            let src = c.lba as usize * SECTOR;
            let len = c.sectors as usize * SECTOR;
            out[c.offset..c.offset + len].copy_from_slice(&disk[src..src + len]);
        }
        assert_eq!(&out[..], &disk[17 * SECTOR..(17 + n) * SECTOR]);
    }

    /// Encodes `s` into IDENTIFY model words (27..=46), byte-swapped, padded.
    fn put_model(id: &mut [u16; 256], s: &[u8]) {
        let mut b = [b' '; 40];
        b[..s.len()].copy_from_slice(s);
        for i in 0..20 {
            id[27 + i] = ((b[i * 2] as u16) << 8) | b[i * 2 + 1] as u16;
        }
    }

    #[test]
    fn model_is_unswapped_and_trimmed() {
        let mut id = [0u16; 256];
        put_model(&mut id, b"QEMU HARDDISK");
        let d = parse_identify(&id);
        assert_eq!(d.model_len, 13);
        assert_eq!(d.model_name(), "QEMU HARDDISK");
        assert_eq!(&d.model[..4], b"QEMU");
    }

    #[test]
    fn model_odd_length_and_full_width() {
        let mut id = [0u16; 256];
        put_model(&mut id, b"ABC");
        assert_eq!(parse_identify(&id).model_name(), "ABC");
        put_model(&mut id, &[b'X'; 40]);
        let d = parse_identify(&id);
        assert_eq!(d.model_len, 40);
        assert_eq!(d.model_name().len(), 40);
    }

    #[test]
    fn empty_or_nul_model_has_zero_length() {
        let id = [0u16; 256]; // all NULs
        let d = parse_identify(&id);
        assert_eq!(d.model_len, 0);
        assert_eq!(d.model_name(), "");
        let mut id = [0u16; 256];
        put_model(&mut id, b"");
        assert_eq!(parse_identify(&id).model_len, 0);
    }

    #[test]
    fn non_utf8_model_reports_question_mark() {
        let mut id = [0u16; 256];
        id[27] = 0xFFFE;
        let d = parse_identify(&id);
        assert_eq!(d.model_len, 2);
        assert_eq!(d.model_name(), "?");
    }

    #[test]
    fn lba28_used_when_lba48_absent() {
        let mut id = [0u16; 256];
        id[60] = 0x5678;
        id[61] = 0x1234;
        assert_eq!(parse_identify(&id).sectors, 0x1234_5678);
    }

    #[test]
    fn lba48_takes_precedence() {
        let mut id = [0u16; 256];
        id[60] = 1;
        id[100] = 0x0002;
        id[101] = 0x0001;
        id[102] = 0x0003;
        id[103] = 0x0000;
        assert_eq!(parse_identify(&id).sectors, 0x0003_0001_0002);
    }

    #[test]
    fn lba48_top_word_is_honoured() {
        let mut id = [0u16; 256];
        id[103] = 0xFFFF;
        assert_eq!(parse_identify(&id).sectors, 0xFFFF << 48);
    }

    #[test]
    fn capacity_in_mib_never_overflows() {
        let mut id = [0u16; 256];
        id[100] = 2048;
        assert_eq!(parse_identify(&id).mib(), 1);
        id[100] = 2047;
        assert_eq!(parse_identify(&id).mib(), 0);
        // Absurd (malformed) 64-bit count: the old `sectors * 512` overflowed.
        id[100] = 0xFFFF;
        id[101] = 0xFFFF;
        id[102] = 0xFFFF;
        id[103] = 0xFFFF;
        assert_eq!(parse_identify(&id).mib(), u64::MAX / 2048);
    }

    #[test]
    fn rotation_rate_classification() {
        let mut id = [0u16; 256];
        let cases = [
            (0u16, false, 0u16), // not reported
            (1, true, 0),        // SSD
            (2, false, 0),       // reserved
            (0x0400, false, 0),  // reserved boundary
            (0x0401, false, 0x0401),
            (7200, false, 7200),
            (0xFFFE, false, 0xFFFE),
            (0xFFFF, false, 0), // reserved
        ];
        for (rot, ssd, rpm) in cases {
            id[217] = rot;
            let d = parse_identify(&id);
            assert_eq!((d.ssd, d.rpm), (ssd, rpm), "rot={rot:#x}");
        }
    }

    #[test]
    fn sector_count_bounds() {
        assert_eq!(sector_count(0), None);
        assert_eq!(sector_count(511), None);
        assert_eq!(sector_count(512), Some(1));
        assert_eq!(sector_count(1023), Some(1)); // partial tail ignored
        assert_eq!(sector_count(255 * 512), Some(255));
        assert_eq!(sector_count(256 * 512), None);
        assert_eq!(sector_count(usize::MAX), None);
    }

    #[test]
    fn lba28_register_encoding() {
        assert_eq!(lba28_regs(0), [0xE0, 0, 0, 0]);
        assert_eq!(lba28_regs(0x0123_4567), [0xE1, 0x67, 0x45, 0x23]);
        assert_eq!(lba28_regs(0x0FFF_FFFF), [0xEF, 0xFF, 0xFF, 0xFF]);
        // Bits above 28 are dropped, never leak into the drive-select bits.
        assert_eq!(lba28_regs(0xF000_0001), [0xE0, 0x01, 0, 0]);
    }
}
