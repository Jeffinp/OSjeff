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
