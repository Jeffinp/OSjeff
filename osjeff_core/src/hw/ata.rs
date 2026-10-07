//! ATA `IDENTIFY DEVICE` parsing and PIO command arithmetic.
//!
//! The kernel driver talks to the I/O ports; everything that merely *interprets*
//! the 256-word IDENTIFY block or computes register values lives here.

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

#[cfg(test)]
mod tests {
    use super::*;

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
