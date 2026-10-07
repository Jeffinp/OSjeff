//! File contents: offsets, holes, truncate, append, extents, big directories,
//! full disks and space reuse.

use super::*;

fn read_all(fs: &mut Fs3<RamDisk>, ino: Ino) -> Vec<u8> {
    let size = fs.stat_ino(ino).unwrap().size as usize;
    let mut v = alloc::vec![0xEEu8; size];
    assert_eq!(fs.read_at(ino, 0, &mut v).unwrap(), size);
    v
}

/// Apply a write to a model buffer (zero-filling gaps).
fn model_write(m: &mut Vec<u8>, off: usize, d: &[u8]) {
    if m.len() < off + d.len() {
        m.resize(off + d.len(), 0);
    }
    m[off..off + d.len()].copy_from_slice(d);
}

#[test]
fn write_and_read_back_across_block_boundaries() {
    let mut fs = fresh(4);
    let ino = fs.create("/f", 0).unwrap();
    let mut rng = Rng(7);
    let mut model = Vec::new();
    for off in [0usize, 1, 4095, 4096, 4097, 8191, 10_000, 3] {
        let d = rng.bytes(5000);
        fs.write_at(ino, off as u64, &d, 1).unwrap();
        model_write(&mut model, off, &d);
        assert_eq!(read_all(&mut fs, ino), model, "after write at {off}");
    }
    assert_clean(&mut fs);
}

#[test]
fn read_at_offsets_and_eof() {
    let mut fs = fresh(2);
    let ino = fs.create("/f", 0).unwrap();
    let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
    fs.write_at(ino, 0, &data, 0).unwrap();
    let mut b = [0u8; 100];
    assert_eq!(fs.read_at(ino, 9_950, &mut b).unwrap(), 50);
    assert_eq!(&b[..50], &data[9_950..]);
    assert_eq!(fs.read_at(ino, 10_000, &mut b).unwrap(), 0);
    assert_eq!(fs.read_at(ino, 99_999, &mut b).unwrap(), 0);
    assert_eq!(fs.read_at(ino, 4090, &mut b).unwrap(), 100);
    assert_eq!(&b[..], &data[4090..4190]);
    assert_eq!(fs.read_at(ino, 0, &mut []).unwrap(), 0);
    assert_eq!(fs.read_at(ino, u64::MAX, &mut b).unwrap(), 0);
}

#[test]
fn write_past_the_end_leaves_a_hole_of_zeros() {
    let mut fs = fresh(2);
    let ino = fs.create("/f", 0).unwrap();
    fs.write_at(ino, 0, b"head", 0).unwrap();
    fs.write_at(ino, 40_000, b"tail", 0).unwrap();
    let all = read_all(&mut fs, ino);
    assert_eq!(all.len(), 40_004);
    assert_eq!(&all[..4], b"head");
    assert!(all[4..40_000].iter().all(|&b| b == 0));
    assert_eq!(&all[40_000..], b"tail");
    // Only the two end blocks exist: the hole costs nothing.
    assert_eq!(fs.stat_ino(ino).unwrap().blocks, 2);
    assert_clean(&mut fs);
}

#[test]
fn write_into_an_empty_file_at_a_big_offset() {
    let mut fs = fresh(2);
    let ino = fs.create("/f", 0).unwrap();
    fs.write_at(ino, 1 << 31, b"x", 0).unwrap();
    assert_eq!(fs.stat_ino(ino).unwrap().size, (1 << 31) + 1);
    assert_eq!(fs.stat_ino(ino).unwrap().blocks, 1);
    let mut b = [9u8; 4];
    assert_eq!(fs.read_at(ino, (1 << 31) - 2, &mut b).unwrap(), 3);
    assert_eq!(&b[..3], &[0, 0, b'x']);
    assert_eq!(fs.read_at(ino, 12345, &mut b).unwrap(), 4);
    assert_eq!(b, [0; 4]);
    assert_clean(&mut fs);
}

#[test]
fn file_size_limit_is_enforced() {
    let mut fs = fresh(2);
    let ino = fs.create("/f", 0).unwrap();
    assert_eq!(
        fs.write_at(ino, MAX_FILE_BYTES, b"x", 0),
        Err(FsError::TooBig)
    );
    assert_eq!(fs.write_at(ino, u64::MAX, b"x", 0), Err(FsError::TooBig));
    assert_eq!(
        fs.write_at(ino, u64::MAX - 1, b"xyz", 0),
        Err(FsError::TooBig)
    );
    assert_eq!(
        fs.truncate(ino, MAX_FILE_BYTES + 1, 0),
        Err(FsError::TooBig)
    );
    fs.truncate(ino, MAX_FILE_BYTES, 0).unwrap();
    assert_eq!(fs.read_file("/f"), Err(FsError::TooBig)); // refuses to allocate 16 TiB
    assert_clean(&mut fs);
}

#[test]
fn zero_length_write_changes_nothing() {
    let mut fs = fresh(2);
    let ino = fs.create("/f", 1).unwrap();
    fs.write_at(ino, 5000, &[], 99).unwrap();
    let s = fs.stat_ino(ino).unwrap();
    assert_eq!((s.size, s.mtime, s.blocks), (0, 1, 0));
}

#[test]
fn overwrite_in_place_keeps_neighbouring_bytes() {
    let mut fs = fresh(2);
    fs.write_file("/f", &[b'a'; 12_000], 0).unwrap();
    let ino = fs.lookup("/f").unwrap();
    fs.write_at(ino, 4000, &[b'B'; 300], 5).unwrap();
    let all = read_all(&mut fs, ino);
    assert!(all[..4000].iter().all(|&b| b == b'a'));
    assert!(all[4000..4300].iter().all(|&b| b == b'B'));
    assert!(all[4300..].iter().all(|&b| b == b'a'));
    assert_eq!(fs.stat_ino(ino).unwrap().mtime, 5);
    assert_clean(&mut fs);
}

#[test]
fn overwriting_does_not_leak_space() {
    let mut fs = fresh(2);
    fs.write_file("/f", &[1u8; 20_000], 0).unwrap();
    let free = fs.statfs().free_blocks;
    let ino = fs.lookup("/f").unwrap();
    for i in 0..30u8 {
        fs.write_at(ino, 1000, &[i; 9_000], 0).unwrap();
    }
    assert_eq!(fs.statfs().free_blocks, free);
    assert_clean(&mut fs);
}

#[test]
fn write_file_replaces_content_and_can_shrink_or_grow() {
    let mut fs = fresh(2);
    fs.write_file("/f", &[1u8; 30_000], 1).unwrap();
    fs.write_file("/f", b"short", 2).unwrap();
    assert_eq!(fs.read_file("/f").unwrap(), b"short");
    assert_eq!(fs.stat("/f").unwrap().blocks, 1);
    fs.write_file("/f", &[2u8; 50_000], 3).unwrap();
    assert_eq!(fs.read_file("/f").unwrap(), alloc::vec![2u8; 50_000]);
    fs.write_file("/f", b"", 4).unwrap();
    assert_eq!(fs.read_file("/f").unwrap(), b"");
    assert_eq!(fs.stat("/f").unwrap().blocks, 0);
    assert_eq!(fs.write_file("/", b"x", 0), Err(FsError::InvalidPath));
    fs.mkdir("/d", 0).unwrap();
    assert_eq!(fs.write_file("/d", b"x", 0), Err(FsError::IsDir));
    assert_clean(&mut fs);
}

#[test]
fn append_grows_the_file() {
    let mut fs = fresh(2);
    let ino = fs.create("/log", 0).unwrap();
    let mut model = Vec::new();
    for i in 0..200u32 {
        let line = alloc::format!("line {i}\n");
        fs.append(ino, line.as_bytes(), i as u64).unwrap();
        model.extend_from_slice(line.as_bytes());
    }
    assert_eq!(read_all(&mut fs, ino), model);
    assert_eq!(fs.stat_ino(ino).unwrap().mtime, 199);
    assert_clean(&mut fs);
}

#[test]
fn append_crossing_a_block_boundary_keeps_the_old_tail() {
    let mut fs = fresh(2);
    let ino = fs.create("/f", 0).unwrap();
    fs.append(ino, &[1u8; 4090], 0).unwrap();
    fs.append(ino, &[2u8; 20], 0).unwrap();
    let all = read_all(&mut fs, ino);
    assert_eq!(&all[4085..4095], &[1, 1, 1, 1, 1, 2, 2, 2, 2, 2]);
    assert_eq!(all.len(), 4110);
}

#[test]
fn truncate_shrinks_and_frees_blocks() {
    let mut fs = fresh(2);
    let s0 = fs.statfs().free_blocks;
    fs.write_file("/f", &[5u8; 40_000], 0).unwrap();
    let ino = fs.lookup("/f").unwrap();
    assert_eq!(fs.stat_ino(ino).unwrap().blocks, 10);
    fs.truncate(ino, 4097, 1).unwrap();
    assert_eq!(fs.stat_ino(ino).unwrap().blocks, 2);
    assert_eq!(read_all(&mut fs, ino), alloc::vec![5u8; 4097]);
    fs.truncate(ino, 0, 2).unwrap();
    assert_eq!(fs.stat_ino(ino).unwrap().blocks, 0);
    assert_eq!(fs.statfs().free_blocks, s0);
    assert_clean(&mut fs);
}

#[test]
fn truncate_then_extend_exposes_zeros_not_stale_bytes() {
    let mut fs = fresh(2);
    fs.write_file("/f", &[0xFFu8; 10_000], 0).unwrap();
    let ino = fs.lookup("/f").unwrap();
    fs.truncate(ino, 5_000, 1).unwrap(); // mid-block
    fs.truncate(ino, 9_000, 2).unwrap(); // grow back
    let all = read_all(&mut fs, ino);
    assert!(all[..5_000].iter().all(|&b| b == 0xFF));
    assert!(
        all[5_000..].iter().all(|&b| b == 0),
        "stale bytes resurfaced"
    );
    // Same through a write past the old end.
    fs.truncate(ino, 100, 3).unwrap();
    fs.write_at(ino, 6_000, b"z", 4).unwrap();
    let all = read_all(&mut fs, ino);
    assert!(all[100..6_000].iter().all(|&b| b == 0));
    assert_clean(&mut fs);
}

#[test]
fn truncate_grow_is_sparse() {
    let mut fs = fresh(2);
    let ino = fs.create("/f", 0).unwrap();
    fs.truncate(ino, 1_000_000, 7).unwrap();
    let s = fs.stat_ino(ino).unwrap();
    assert_eq!((s.size, s.blocks, s.mtime), (1_000_000, 0, 7));
    assert!(read_all(&mut fs, ino).iter().all(|&b| b == 0));
    fs.truncate(ino, 1_000_000, 8).unwrap(); // same size: no-op
    assert_eq!(fs.stat_ino(ino).unwrap().mtime, 7);
}

#[test]
fn data_ops_on_directories_and_missing_inodes_fail_cleanly() {
    let mut fs = fresh(2);
    let d = fs.mkdir("/d", 0).unwrap();
    let mut b = [0u8; 4];
    assert_eq!(fs.read_at(d, 0, &mut b), Err(FsError::IsDir));
    assert_eq!(fs.write_at(d, 0, b"x", 0), Err(FsError::IsDir));
    assert_eq!(fs.append(d, b"x", 0), Err(FsError::IsDir));
    assert_eq!(fs.truncate(d, 0, 0), Err(FsError::IsDir));
    assert_eq!(fs.read_at(77, 0, &mut b), Err(FsError::NotFound));
    assert_eq!(fs.write_at(77, 0, b"x", 0), Err(FsError::NotFound));
    assert_eq!(fs.write_at(0, 0, b"x", 0), Err(FsError::NotFound));
    assert_eq!(fs.read_file("/d"), Err(FsError::IsDir));
    assert_clean(&mut fs);
}

#[test]
fn multi_megabyte_file_roundtrip() {
    let mut fs = fresh(16);
    let mut rng = Rng(99);
    let data = rng.bytes(5 * 1024 * 1024 + 123);
    fs.write_file("/big", &data, 0).unwrap();
    assert_eq!(fs.read_file("/big").unwrap(), data);
    // Sequential chunked read.
    let ino = fs.lookup("/big").unwrap();
    let mut got = Vec::new();
    let mut buf = alloc::vec![0u8; 100_003];
    let mut off = 0u64;
    loop {
        let n = fs.read_at(ino, off, &mut buf).unwrap();
        if n == 0 {
            break;
        }
        got.extend_from_slice(&buf[..n]);
        off += n as u64;
    }
    assert_eq!(got, data);
    // A single contiguous extent when the disk is fresh.
    let node = fs.read_inode(ino).unwrap();
    assert_eq!(node.nextents, 1);
    assert_clean(&mut fs);
}

#[test]
fn fragmented_files_use_the_indirect_extent_chain() {
    let mut fs = fresh(8);
    let a = fs.create("/a", 0).unwrap();
    let b = fs.create("/b", 0).unwrap();
    let mut ma = Vec::new();
    let mut mb = Vec::new();
    // Alternate one-block appends so neither file can stay contiguous.
    for i in 0..400u32 {
        let da = alloc::vec![(i % 251) as u8; 4096];
        let db = alloc::vec![(i % 241) as u8 ^ 0x55; 4096];
        fs.append(a, &da, 0).unwrap();
        fs.append(b, &db, 0).unwrap();
        ma.extend_from_slice(&da);
        mb.extend_from_slice(&db);
    }
    let na = fs.read_inode(a).unwrap();
    assert!(na.nextents > 339 + 12, "{} extents", na.nextents);
    assert_ne!(na.ext_chain, 0);
    assert_eq!(read_all(&mut fs, a), ma);
    assert_eq!(read_all(&mut fs, b), mb);
    assert_clean(&mut fs);
    // Punch holes through the middle, then free one of them entirely.
    fs.write_at(a, 100 * 4096 + 17, &[9u8; 50_000], 0).unwrap();
    model_write(&mut ma, 100 * 4096 + 17, &[9u8; 50_000]);
    assert_eq!(read_all(&mut fs, a), ma);
    fs.remove("/a").unwrap();
    assert_eq!(read_all(&mut fs, b), mb);
    assert_clean(&mut fs);
    fs.remove("/b").unwrap();
    assert_clean(&mut fs);
}

#[test]
fn shrinking_a_fragmented_file_releases_chain_blocks() {
    let mut fs = fresh(8);
    let a = fs.create("/a", 0).unwrap();
    let b = fs.create("/b", 0).unwrap();
    for _ in 0..400 {
        fs.append(a, &[1u8; 4096], 0).unwrap();
        fs.append(b, &[2u8; 4096], 0).unwrap();
    }
    let free = fs.statfs().free_blocks;
    fs.truncate(a, 10 * 4096, 0).unwrap();
    let na = fs.read_inode(a).unwrap();
    assert!(na.nextents <= 12 + 339);
    assert!(fs.statfs().free_blocks > free + 380);
    fs.truncate(a, 0, 0).unwrap();
    assert_eq!(fs.read_inode(a).unwrap().ext_chain, 0);
    assert_clean(&mut fs);
}

#[test]
fn disk_full_is_reported_and_leaves_old_data_intact() {
    let mut fs = fresh(1); // the smallest disk
    let mut i = 0;
    let err = loop {
        match fs.write_file(&alloc::format!("/f{i}"), &[i as u8; 20_000], 0) {
            Ok(()) => i += 1,
            Err(e) => break e,
        }
    };
    assert_eq!(err, FsError::NoSpace);
    assert!(i > 5);
    assert_clean(&mut fs);
    for k in 0..i {
        assert_eq!(
            fs.read_file(&alloc::format!("/f{k}")).unwrap(),
            alloc::vec![k as u8; 20_000]
        );
    }
    // The failed file must not exist and must not have leaked blocks or inodes.
    assert_eq!(fs.lookup(&alloc::format!("/f{i}")), Err(FsError::NotFound));
    // Rewriting an existing file needs room for the copy-on-write: refused,
    // and the old content is untouched.
    let big = alloc::vec![0xCCu8; 20_000];
    let free = fs.statfs().free_blocks as usize;
    if free < 5 {
        let r = fs.write_file("/f0", &big, 0);
        assert_eq!(r, Err(FsError::NoSpace));
        assert_eq!(fs.read_file("/f0").unwrap(), alloc::vec![0u8; 20_000]);
    }
    assert_clean(&mut fs);
}

#[test]
fn space_is_reused_after_removal() {
    let mut fs = fresh(1);
    let mut count = 0;
    while fs
        .write_file(&alloc::format!("/f{count}"), &[1u8; 16_384], 0)
        .is_ok()
    {
        count += 1;
    }
    let free_full = fs.statfs().free_blocks;
    for k in 0..count {
        fs.remove(&alloc::format!("/f{k}")).unwrap();
    }
    assert!(fs.statfs().free_blocks >= free_full + count as u32 * 4);
    // The same number of files fits again.
    for k in 0..count {
        fs.write_file(&alloc::format!("/g{k}"), &[2u8; 16_384], 0)
            .unwrap();
    }
    assert_clean(&mut fs);
}

#[test]
fn no_space_error_leaves_statfs_unchanged() {
    let mut fs = fresh(1);
    let mut n = 0;
    while fs
        .write_file(&alloc::format!("/f{n}"), &[1u8; 30_000], 0)
        .is_ok()
    {
        n += 1;
    }
    let s = fs.statfs();
    assert!(fs.write_file("/another", &[1u8; 200_000], 0).is_err());
    assert_eq!(fs.statfs(), s);
    assert_clean(&mut fs);
}

#[test]
fn directory_with_thousands_of_entries() {
    let mut fs = fresh_with_inodes(24, 8192);
    fs.mkdir("/big", 0).unwrap();
    let n = 5_300u32;
    for i in 0..n {
        fs.create(&alloc::format!("/big/entry-number-{i:05}"), i as u64)
            .unwrap();
    }
    let list = fs.readdir("/big").unwrap();
    assert_eq!(list.len(), n as usize);
    let st = fs.stat("/big").unwrap();
    assert!(st.blocks >= 15, "{} dir blocks", st.blocks);
    for i in (0..n).step_by(97) {
        assert!(
            fs.lookup(&alloc::format!("/big/entry-number-{i:05}"))
                .is_ok()
        );
    }
    assert_eq!(fs.lookup("/big/entry-number-99999"), Err(FsError::NotFound));
    assert_eq!(
        fs.create("/big/entry-number-00042", 0),
        Err(FsError::Exists)
    );
    assert_clean(&mut fs);
    // Remove every other entry; blocks are compacted/reused, the rest survive.
    for i in (0..n).step_by(2) {
        fs.remove(&alloc::format!("/big/entry-number-{i:05}"))
            .unwrap();
    }
    assert_eq!(fs.readdir("/big").unwrap().len(), (n / 2) as usize);
    assert!(fs.lookup("/big/entry-number-00001").is_ok());
    assert_eq!(fs.lookup("/big/entry-number-00002"), Err(FsError::NotFound));
    assert_clean(&mut fs);
    // And the survivors can be re-filled.
    for i in (0..n).step_by(2) {
        fs.create(&alloc::format!("/big/entry-number-{i:05}"), 0)
            .unwrap();
    }
    assert_eq!(fs.readdir("/big").unwrap().len(), n as usize);
    assert_clean(&mut fs);
}

#[test]
fn longest_names_fill_directory_blocks_correctly() {
    let mut fs = fresh(4);
    // 261 bytes per entry: 15 fit in a block; 40 entries span 3 blocks.
    for i in 0..40u32 {
        let name = alloc::format!("{i:03}{}", "z".repeat(252));
        fs.create(&alloc::format!("/{name}"), 0).unwrap();
    }
    assert_eq!(fs.readdir("/").unwrap().len(), 40);
    assert!(fs.stat("/").unwrap().blocks >= 3);
    for i in 0..40u32 {
        let name = alloc::format!("{i:03}{}", "z".repeat(252));
        fs.remove(&alloc::format!("/{name}")).unwrap();
    }
    assert!(fs.readdir("/").unwrap().is_empty());
    assert_clean(&mut fs);
}

#[test]
fn works_with_a_tiny_block_cache() {
    let dev = Fs3::format(RamDisk::new(4 * 2048), &FormatOptions::new(UUID, 0))
        .unwrap()
        .into_device();
    let mut fs = Fs3::mount_with(dev, 3).unwrap();
    let mut rng = Rng(5);
    let data = rng.bytes(300_000);
    fs.mkdir("/d", 0).unwrap();
    fs.write_file("/d/f", &data, 0).unwrap();
    for i in 0..40 {
        fs.create(&alloc::format!("/d/x{i}"), 0).unwrap();
    }
    assert_eq!(fs.read_file("/d/f").unwrap(), data);
    assert!(fs.cache_stats().evictions > 0);
    assert_clean(&mut fs);
}

#[test]
fn state_survives_unmount_and_remount() {
    let mut fs = fresh(4);
    fs.mkdir("/a", 1).unwrap();
    fs.write_file("/a/f", &[3u8; 70_000], 2).unwrap();
    fs.write_file("/g", b"gg", 3).unwrap();
    fs.trash("/g", 4).unwrap();
    let st = fs.statfs();
    let mut fs = Fs3::mount_verified(fs.into_device()).unwrap();
    assert_eq!(fs.statfs(), st);
    assert_eq!(fs.read_file("/a/f").unwrap(), alloc::vec![3u8; 70_000]);
    assert_eq!(fs.trash_list().unwrap().len(), 1);
    assert_eq!(fs.stat("/a/f").unwrap().mtime, 2);
}

#[test]
fn sync_is_a_harmless_barrier() {
    let mut fs = fresh(2);
    fs.sync().unwrap();
    fs.write_file("/f", b"x", 0).unwrap();
    fs.sync().unwrap();
    assert_clean(&mut fs);
}

#[test]
fn directory_blocks_shrink_back_to_nothing() {
    let mut fs = fresh(4);
    fs.mkdir("/d", 0).unwrap();
    let s0 = fs.statfs().free_blocks;
    for i in 0..200 {
        fs.create(&alloc::format!("/d/{i:040}"), 0).unwrap();
    }
    assert!(fs.statfs().free_blocks < s0);
    for i in 0..200 {
        fs.remove(&alloc::format!("/d/{i:040}")).unwrap();
    }
    // Entries removed in insertion order empty the blocks front to back; the
    // trailing empty blocks are returned as they become the last block.
    assert_clean(&mut fs);
    assert!(
        fs.statfs().free_blocks >= s0 - 40,
        "directory kept too many empty blocks"
    );
}
