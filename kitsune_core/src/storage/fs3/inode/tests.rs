use super::*;

fn sample() -> Inode {
    let mut i = Inode::new(Kind::File, 1234, 1);
    i.size = 99_999;
    i.nblocks = 25;
    i.nextents = 2;
    i.inline[0] = Extent {
        lblk: 0,
        len: 20,
        pblk: 100,
    };
    i.inline[1] = Extent {
        lblk: 20,
        len: 5,
        pblk: 300,
    };
    i.trash_name = b"orig.txt".to_vec();
    i.flags = FLAG_TRASHED;
    i.trash_parent = 7;
    i.trash_time = 55;
    i.trash_pctime = 0xABCD;
    i
}

#[test]
fn inode_roundtrip() {
    let i = sample();
    let mut buf = [0xEEu8; INODE_SIZE];
    i.encode(&mut buf);
    assert_eq!(Inode::decode(&buf), Some(i));
}

#[test]
fn every_bit_flip_in_an_inode_is_detected() {
    let mut buf = [0u8; INODE_SIZE];
    sample().encode(&mut buf);
    for bit in 0..INODE_SIZE * 8 {
        let mut c = buf;
        c[bit / 8] ^= 1 << (bit % 8);
        assert!(Inode::decode(&c).is_none(), "bit {bit}");
    }
}

#[test]
fn invalid_kind_or_flags_rejected_even_with_valid_crc() {
    let mut buf = [0u8; INODE_SIZE];
    sample().encode(&mut buf);
    let mut bad = buf;
    bad[4] = 9;
    let c = crc32(&bad[4..]);
    wr32(&mut bad, 0, c);
    assert!(Inode::decode(&bad).is_none());
    let mut bad = buf;
    bad[5] = 0x80;
    let c = crc32(&bad[4..]);
    wr32(&mut bad, 0, c);
    assert!(Inode::decode(&bad).is_none());
    assert!(Inode::decode(&buf[..100]).is_none());
    assert!(Inode::decode(&[0u8; INODE_SIZE]).is_none());
}

#[test]
fn kind_codes_roundtrip() {
    for k in [Kind::File, Kind::Dir] {
        assert_eq!(Kind::from_u8(k.to_u8()), Some(k));
    }
    assert_eq!(Kind::from_u8(0), None);
}

#[test]
fn extent_arithmetic() {
    let e = Extent {
        lblk: u32::MAX - 1,
        len: 1,
        pblk: u32::MAX,
    };
    assert_eq!(e.lend(), u32::MAX as u64);
    assert_eq!(e.pend(), u32::MAX as u64 + 1);
    assert_eq!(EXTENTS_PER_BLOCK, 339);
}

#[test]
fn longest_trash_name_fits() {
    let mut i = sample();
    i.trash_name = alloc::vec![b'x'; MAX_NAME];
    let mut buf = [0u8; INODE_SIZE];
    i.encode(&mut buf);
    assert_eq!(Inode::decode(&buf).unwrap().trash_name.len(), MAX_NAME);
}
