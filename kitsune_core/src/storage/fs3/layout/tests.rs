use super::*;

#[test]
fn geometry_for_minimum_disk_is_sane() {
    let total = ((MIN_DISK_SECTORS - FS_START_LBA) / 8) as u32;
    assert_eq!(total, 240);
    let (j, i) = Geometry::defaults(total);
    assert_eq!((j, i), (24, 64));
    let g = Geometry::compute(total, j, i).unwrap();
    assert_eq!(g.bbitmap_start, 26);
    assert_eq!(g.ibitmap_start, 27);
    assert_eq!(g.itable_start, 28);
    assert_eq!(g.itable_blocks, 8);
    assert_eq!(g.data_start, 36);
    assert_eq!(g.backup_sb(), 239);
}

#[test]
fn geometry_rejects_unusable_parameters() {
    assert!(Geometry::compute(240, 0, 64).is_none());
    assert!(Geometry::compute(240, MAX_JOURNAL_BLOCKS + 1, 64).is_none());
    assert!(Geometry::compute(240, 24, 63).is_none());
    assert!(Geometry::compute(240, 24, 0).is_none());
    assert!(Geometry::compute(40, 24, 64).is_none());
    assert!(Geometry::compute(0, 1, 8).is_none());
    assert!(Geometry::compute(1000, 256, u32::MAX - 7).is_none());
}

#[test]
fn geometry_regions_do_not_overlap_and_grow_with_size() {
    for total in [240u32, 1000, 32_704, 32_705, 100_000, 1_000_000] {
        let (j, i) = Geometry::defaults(total);
        let g = Geometry::compute(total, j, i).unwrap();
        assert_eq!(g.bbitmap_blocks, total.div_ceil(BITS_PER_BITMAP_BLOCK));
        assert!(g.jpayload + g.journal_blocks == g.bbitmap_start);
        assert!(g.bbitmap_start + g.bbitmap_blocks == g.ibitmap_start);
        assert!(g.ibitmap_start + g.ibitmap_blocks == g.itable_start);
        assert!(g.itable_start + g.itable_blocks == g.data_start);
        assert!(g.data_start < g.backup_sb());
    }
}

fn sb() -> Superblock {
    let g = Geometry::compute(240, 24, 64).unwrap();
    Superblock {
        uuid: [7; 16],
        created: 1_700_000_000,
        geo: g,
    }
}

#[test]
fn superblock_roundtrip() {
    let s = sb();
    let e = s.encode();
    assert_eq!(Superblock::decode(&e), Some(s));
    assert_eq!(&e[4..8], b"OJF3");
}

#[test]
fn superblock_rejects_every_single_bit_flip() {
    let e = sb().encode();
    for bit in 0..512 * 8 {
        let mut c = e;
        c[bit / 8] ^= 1 << (bit % 8);
        assert!(Superblock::decode(&c).is_none(), "bit {bit} accepted");
    }
}

#[test]
fn superblock_rejects_inconsistent_geometry_even_with_valid_crc() {
    let mut e = sb().encode();
    wr32(&mut e, 68, 99); // data_start lies
    let c = crc32(&e[4..]);
    wr32(&mut e, 0, c);
    assert!(Superblock::decode(&e).is_none());
    assert!(Superblock::decode(&e[..100]).is_none());
    assert!(Superblock::decode(&[0u8; 512]).is_none());
}

#[test]
fn journal_header_roundtrip_and_crc_covers_payload() {
    let p1 = [1u8; BLOCK_SIZE];
    let p2 = [2u8; BLOCK_SIZE];
    let h = build_journal_header(9, &[40, 41], &[&p1, &p2]);
    let parsed = parse_journal_header(&h, 24).unwrap();
    assert_eq!(parsed.seq, 9);
    assert_eq!(parsed.targets, [40, 41]);
    let mut payload = Vec::new();
    payload.extend_from_slice(&p1);
    payload.extend_from_slice(&p2);
    assert!(journal_crc_ok(&h, &payload));
    payload[5000] ^= 1;
    assert!(!journal_crc_ok(&h, &payload));
    assert!(parse_journal_header(&h, 1).is_none(), "n > max rejected");
}

#[test]
fn empty_journal_header_is_valid_with_n_zero() {
    let h = build_journal_header(3, &[], &[]);
    let p = parse_journal_header(&h, 24).unwrap();
    assert!(p.targets.is_empty());
    assert!(journal_crc_ok(&h, &[]));
    assert!(parse_journal_header(&[0u8; BLOCK_SIZE], 24).is_none());
}
