//! Hostile and damaged images: mount, fsck and every operation must stay
//! total (no panic, no endless loop), and damage must be *detected*.

use super::*;
use crate::fs3::layout::{rd32, wr32};
use crate::fs3::{Detected, detect};

const BASE: usize = 128 * 512;

fn blk_off(b: u32) -> usize {
    BASE + b as usize * 4096
}

/// A small but varied filesystem: nested dirs, a fragmented file, a trashed file.
fn populated() -> Fs3<RamDisk> {
    let mut fs = fresh(1);
    fs.mkdir("/a", 1).unwrap();
    fs.mkdir("/a/b", 2).unwrap();
    fs.write_file("/a/b/file", &[7u8; 10_000], 3).unwrap();
    fs.write_file("/a/other", &[8u8; 5_000], 4).unwrap();
    fs.write_file("/c", b"tiny", 5).unwrap();
    fs.mkdir("/d", 6).unwrap();
    for i in 0..5 {
        fs.write_file(&alloc::format!("/d/f{i}"), &[i as u8; 3000], 7)
            .unwrap();
    }
    // Fragment one file across a dozen extents so it needs the indirect chain.
    let x = fs.create("/frag", 8).unwrap();
    let y = fs.create("/frag2", 8).unwrap();
    for _ in 0..16 {
        fs.append(x, &[0x11u8; 4096], 9).unwrap();
        fs.append(y, &[0x22u8; 4096], 9).unwrap();
    }
    fs.write_file("/gone", b"to the trash", 10).unwrap();
    fs.trash("/gone", 11).unwrap();
    assert_clean(&mut fs);
    fs
}

fn image() -> (Vec<u8>, Geometry) {
    let fs = populated();
    let geo = fs.geo;
    (fs.into_device().into_bytes(), geo)
}

/// Exercise every public operation; none may panic (errors are fine).
fn hammer(fs: &mut Fs3<RamDisk>) {
    let _ = fs.fsck();
    let _ = fs.statfs();
    let _ = fs.readdir("/");
    let _ = fs.readdir("/.trash");
    let _ = fs.trash_list();
    for p in [
        "/",
        "/a",
        "/a/b",
        "/a/b/file",
        "/c",
        "/d",
        "/frag",
        "/gone",
        "/.trash/gone",
    ] {
        let _ = fs.lookup(p);
        let _ = fs.stat(p);
        let _ = fs.read_file(p);
        let _ = fs.readdir(p);
    }
    for ino in [1u32, 2, 3, 4, 5, 6, 10, 20, 63, 64, 65] {
        let _ = fs.stat_ino(ino);
        let _ = fs.path_of(ino);
        let mut b = [0u8; 100];
        let _ = fs.read_at(ino, 0, &mut b);
        let _ = fs.read_at(ino, 5000, &mut b);
    }
    let _ = fs.create("/new", 1);
    let _ = fs.mkdir("/a/newdir", 1);
    let _ = fs.write_file("/c", &[1u8; 9000], 2);
    let _ = fs.write_file("/a/b/file", &[1u8; 100], 2);
    if let Ok(i) = fs.lookup("/frag") {
        let _ = fs.write_at(i, 3000, &[5u8; 20_000], 3);
        let _ = fs.truncate(i, 100, 4);
        let _ = fs.append(i, b"more", 4);
    }
    let _ = fs.rename("/a/other", "/zzz", 1);
    let _ = fs.rename("/a", "/a/b/inside", 1);
    let _ = fs.trash("/d", 1);
    let _ = fs.trash_restore(b"d", 2);
    let _ = fs.trash_restore(b"gone", 2);
    let _ = fs.remove("/c");
    let _ = fs.rmdir("/a/b");
    let _ = fs.remove_all("/a");
    let _ = fs.remove_all("/d");
    let _ = fs.empty_trash();
    let _ = fs.fsck();
    let _ = fs.sync();
}

#[test]
fn healthy_populated_image_is_clean_and_has_an_extent_chain() {
    let mut fs = populated();
    let frag = fs.lookup("/frag").unwrap();
    assert_ne!(fs.read_inode(frag).unwrap().ext_chain, 0);
    assert_clean(&mut fs);
    hammer(&mut fs); // also fine on a healthy one
}

#[test]
fn every_bit_of_the_primary_superblock_is_survivable() {
    let (img, _) = image();
    for bit in 0..512 * 8 {
        let mut c = img.clone();
        c[BASE + bit / 8] ^= 1 << (bit % 8);
        // The backup copy takes over: mount succeeds and the data is intact.
        let mut fs =
            Fs3::mount(RamDisk::from_bytes(c)).unwrap_or_else(|e| panic!("bit {bit}: {e:?}"));
        assert_eq!(fs.read_file("/c").unwrap(), b"tiny");
    }
}

#[test]
fn damage_in_both_superblock_copies_is_not_mountable_and_not_reformatted() {
    let (img, geo) = image();
    let mut c = img.clone();
    c[BASE + 40] ^= 1;
    c[blk_off(geo.backup_sb()) + 40] ^= 1;
    let mut disk = RamDisk::from_bytes(c);
    assert_eq!(detect(&mut disk).unwrap(), Detected::Unknown);
    assert!(matches!(Fs3::mount(disk), Err(FsError::BadSuperblock)));
}

#[test]
fn only_the_backup_copy_damaged_still_mounts() {
    let (img, geo) = image();
    let mut c = img.clone();
    c[blk_off(geo.backup_sb()) + 100] ^= 0x40;
    let mut disk = RamDisk::from_bytes(c);
    assert_eq!(detect(&mut disk).unwrap(), Detected::V3);
    assert!(Fs3::mount(disk).is_ok());
}

#[test]
fn bit_flips_in_the_idle_journal_header_are_ignored_safely() {
    let (img, geo) = image();
    let o = blk_off(geo.jhdr);
    for bit in (0..4096 * 8).step_by(13) {
        let mut c = img.clone();
        c[o + bit / 8] ^= 1 << (bit % 8);
        let mut fs =
            Fs3::mount(RamDisk::from_bytes(c)).unwrap_or_else(|e| panic!("bit {bit}: {e:?}"));
        assert_clean(&mut fs);
        assert_eq!(fs.read_file("/c").unwrap(), b"tiny");
    }
}

#[test]
fn every_flip_in_a_bitmap_block_is_detected_at_mount() {
    let (img, geo) = image();
    for (start, count) in [
        (geo.bbitmap_start, geo.bbitmap_blocks),
        (geo.ibitmap_start, geo.ibitmap_blocks),
    ] {
        for b in start..start + count {
            let o = blk_off(b);
            for bit in (0..4096 * 8).step_by(11) {
                let mut c = img.clone();
                c[o + bit / 8] ^= 1 << (bit % 8);
                assert!(
                    matches!(Fs3::mount(RamDisk::from_bytes(c)), Err(FsError::Corrupt(_))),
                    "bitmap block {b} bit {bit} accepted"
                );
            }
        }
    }
}

/// Block of inode `ino` and its byte offset inside the image.
fn inode_pos(geo: &Geometry, ino: u32) -> usize {
    let slot = ino - 1;
    blk_off(geo.itable_start + slot / 8) + (slot % 8) as usize * 512
}

#[test]
fn bit_flips_in_used_inodes_are_detected() {
    let (img, geo) = image();
    let mut fs = Fs3::mount(RamDisk::from_bytes(img.clone())).unwrap();
    let used: Vec<u32> = (1..=geo.inode_count)
        .filter(|&i| fs.stat_ino(i).is_ok())
        .collect();
    assert!(used.len() > 15);
    for ino in used {
        let o = inode_pos(&geo, ino);
        for bit in (0..512 * 8).step_by(29) {
            let mut c = img.clone();
            c[o + bit / 8] ^= 1 << (bit % 8);
            match Fs3::mount(RamDisk::from_bytes(c)) {
                Err(_) => {} // root/trash damage: refused at mount
                Ok(mut fs) => {
                    let rep = fs.fsck().unwrap();
                    assert!(!rep.is_clean(), "inode {ino} bit {bit} not detected");
                }
            }
        }
    }
}

#[test]
fn flips_in_directory_and_extent_blocks_are_detected() {
    let (img, geo) = image();
    let mut fs = Fs3::mount(RamDisk::from_bytes(img.clone())).unwrap();
    // Directory blocks and the extent chain of /frag.
    let mut blocks: Vec<u32> = Vec::new();
    for p in ["/", "/a", "/a/b", "/d", "/.trash"] {
        let ino = fs.lookup(p).unwrap();
        let node = fs.read_inode(ino).unwrap();
        blocks.extend(fs.dir_phys_blocks(ino, &node).unwrap());
    }
    let frag = fs.lookup("/frag").unwrap();
    let node = fs.read_inode(frag).unwrap();
    blocks.extend(fs.walk_chain(frag, node.ext_chain).unwrap());
    assert!(blocks.len() >= 6);
    for b in blocks {
        let o = blk_off(b);
        for bit in (0..4096 * 8).step_by(211) {
            let mut c = img.clone();
            c[o + bit / 8] ^= 1 << (bit % 8);
            match Fs3::mount(RamDisk::from_bytes(c)) {
                Err(_) => {}
                Ok(mut fs) => {
                    let rep = fs.fsck();
                    assert!(
                        !matches!(&rep, Ok(r) if r.is_clean()),
                        "block {b} bit {bit} not detected"
                    );
                    if bit % 4 == 0 {
                        hammer(&mut fs);
                    }
                }
            }
        }
    }
    let _ = geo;
}

#[test]
fn flips_in_file_data_are_not_metadata_errors() {
    // Data blocks carry no checksum (by design): fsck stays clean and the file
    // simply reads back different bytes.
    let (img, _) = image();
    let mut fs = Fs3::mount(RamDisk::from_bytes(img.clone())).unwrap();
    let ino = fs.lookup("/a/b/file").unwrap();
    let node = fs.read_inode(ino).unwrap();
    let pb = fs.load_extents(ino, &node).unwrap()[0].pblk;
    let mut c = img;
    c[blk_off(pb) + 10] ^= 1;
    let mut fs = Fs3::mount(RamDisk::from_bytes(c)).unwrap();
    assert_clean(&mut fs);
    assert_ne!(fs.read_file("/a/b/file").unwrap(), alloc::vec![7u8; 10_000]);
}

#[test]
fn random_garbage_in_metadata_never_panics() {
    let (img, geo) = image();
    let mut rng = Rng(0xBAD5EED);
    let metadata_blocks: Vec<u32> = (geo.jhdr..geo.data_start + 30).collect();
    for round in 0..150 {
        let mut c = img.clone();
        for _ in 0..1 + rng.below(6) {
            let b = metadata_blocks[rng.below(metadata_blocks.len() as u64) as usize];
            let o = blk_off(b) + rng.below(4096 - 64) as usize;
            let n = 1 + rng.below(64) as usize;
            match rng.below(3) {
                0 => c[o..o + n].fill(rng.next() as u8),
                1 => c[o..o + n].copy_from_slice(&rng.bytes(n)),
                _ => c[o] ^= 1 << rng.below(8),
            }
        }
        if let Ok(mut fs) = Fs3::mount(RamDisk::from_bytes(c)) {
            hammer(&mut fs);
        }
        let _ = round;
    }
}

#[test]
fn random_bytes_and_zero_images_are_not_mountable() {
    let mut rng = Rng(11);
    for len in [0usize, 512, 100 * 512, 2048 * 512] {
        let mut d = RamDisk::from_bytes(rng.bytes(len));
        assert!(Fs3::mount(RamDisk::from_bytes(d.as_bytes().to_vec())).is_err());
        assert_ne!(detect(&mut d).unwrap(), Detected::V3);
    }
    let z = RamDisk::new(4096);
    assert!(matches!(Fs3::mount(z), Err(FsError::BadSuperblock)));
}

#[test]
fn truncated_device_is_refused() {
    let (img, geo) = image();
    // Cut the image shorter than the filesystem claims.
    let keep = BASE + (geo.total_blocks as usize - 10) * 4096;
    let short = RamDisk::from_bytes(img[..keep].to_vec());
    assert!(matches!(Fs3::mount(short), Err(FsError::BadSuperblock)));
}

#[test]
fn a_larger_device_than_the_filesystem_is_refused_or_works_but_never_panics() {
    let (img, _) = image();
    let mut big = img.clone();
    big.resize(img.len() * 3, 0);
    // The backup superblock is no longer at the end of the device, but the
    // primary is fine, so this still mounts.
    let mut fs = Fs3::mount(RamDisk::from_bytes(big)).unwrap();
    assert_clean(&mut fs);
}

// ---- checksum-valid but semantically wrong states: fsck must notice ----

fn forged<F>(f: F) -> Fs3<RamDisk>
where
    F: FnOnce(&mut Fs3<RamDisk>) -> Result<(), FsError>,
{
    let mut fs = populated();
    fs.txn(f).unwrap();
    fs
}

#[test]
fn fsck_flags_two_files_sharing_blocks() {
    let mut fs = forged(|fs| {
        let a = fs.lookup("/a/b/file")?;
        let b = fs.lookup("/a/other")?;
        let na = fs.read_inode(a)?;
        let mut nb = fs.read_inode(b)?;
        nb.inline = na.inline;
        nb.nextents = na.nextents;
        nb.nblocks = na.nblocks;
        fs.write_inode(b, &nb)
    });
    let r = fs.fsck().unwrap();
    assert!(r.has("block owned by two structures"), "{r:?}");
    hammer(&mut fs);
}

#[test]
fn fsck_flags_a_leaked_block_and_a_used_block_marked_free() {
    let mut fs = populated();
    // Leak: mark a free data block as used.
    let free = bits::next_free(&fs.bbits, fs.geo.data_start, fs.geo.backup_sb()).unwrap();
    fs.txn(|fs| fs.mark_range(free, 1, true)).unwrap();
    let r = fs.fsck().unwrap();
    assert!(r.has("block marked used but unowned (leak)"), "{r:?}");
    // And the opposite: free a block that a file still owns.
    let mut fs = populated();
    let ino = fs.lookup("/a/b/file").unwrap();
    let node = fs.read_inode(ino).unwrap();
    let pb = fs.load_extents(ino, &node).unwrap()[0].pblk;
    fs.txn(|fs| fs.mark_range(pb, 1, false)).unwrap();
    let r = fs.fsck().unwrap();
    assert!(r.has("block in use but marked free"), "{r:?}");
}

#[test]
fn fsck_flags_wrong_counts_sizes_and_links() {
    for which in 0..6 {
        let mut fs = forged(|fs| {
            let ino = fs.lookup("/a/b/file")?;
            let mut n = fs.read_inode(ino)?;
            match which {
                0 => n.nblocks += 1,
                1 => n.nextents += 1,
                2 => n.nlink = 2,
                3 => n.parent = ROOT_INO,
                4 => n.size = 3, // extents now lie beyond EOF
                _ => n.flags |= inode::FLAG_TRASHED,
            }
            fs.write_inode(ino, &n)
        });
        let r = fs.fsck().unwrap();
        assert!(!r.is_clean(), "variant {which} not detected");
        hammer(&mut fs);
    }
}

#[test]
fn fsck_flags_directory_count_and_size_lies() {
    for which in 0..2 {
        let mut fs = forged(|fs| {
            let ino = fs.lookup("/d")?;
            let mut n = fs.read_inode(ino)?;
            if which == 0 {
                n.nentries += 3;
            } else {
                n.size += 4096;
            }
            fs.write_inode(ino, &n)
        });
        assert!(!fs.fsck().unwrap().is_clean(), "variant {which}");
    }
}

#[test]
fn fsck_flags_a_dangling_entry_and_an_orphan_inode() {
    let mut fs = forged(|fs| {
        let ino = fs.lookup("/c")?;
        // Dangling: an entry for an inode that is not allocated.
        fs.dir_insert(ROOT_INO, b"ghost", 60, Kind::File, 1)?;
        // Orphan: remove the entry of /c but keep its inode allocated.
        fs.dir_remove(ROOT_INO, b"c")?;
        let _ = ino;
        Ok(())
    });
    let r = fs.fsck().unwrap();
    assert!(r.has("entry points at an unallocated inode"), "{r:?}");
    assert!(r.has("allocated inode is unreachable"), "{r:?}");
    assert!(matches!(
        fs.read_file("/ghost"),
        Err(FsError::Corrupt(_)) | Err(FsError::NotFound)
    ));
    hammer(&mut fs);
}

#[test]
fn directory_cycles_are_detected_and_deletion_terminates() {
    let mut fs = forged(|fs| {
        // /a/b/loop -> /a  (a directory that contains its own ancestor)
        let a = fs.lookup("/a")?;
        let b = fs.lookup("/a/b")?;
        fs.dir_insert(b, b"loop", a, Kind::Dir, 1)
    });
    let r = fs.fsck().unwrap();
    assert!(r.has("inode referenced more than once"), "{r:?}");
    // Neither path resolution nor recursive deletion may loop forever.
    let _ = fs.lookup("/a/b/loop/b/loop/b/loop/b");
    let _ = fs.remove_all("/a");
    let ino = fs.lookup("/a/b").unwrap_or(ROOT_INO);
    let _ = fs.path_of(ino);
    hammer(&mut fs);
}

#[test]
fn parent_pointer_cycles_do_not_hang_rename_or_path_of() {
    let mut fs = forged(|fs| {
        let a = fs.lookup("/a")?;
        let b = fs.lookup("/a/b")?;
        let mut na = fs.read_inode(a)?;
        na.parent = b; // a's parent is its own child
        fs.write_inode(a, &na)
    });
    assert!(!fs.fsck().unwrap().is_clean());
    let _ = fs.rename("/d", "/a/b/x", 1);
    let b = fs.lookup("/a/b").unwrap();
    assert!(fs.path_of(b).is_err() || fs.path_of(b).is_ok());
    let _ = fs.trash_restore(b"gone", 1);
    hammer(&mut fs);
}

#[test]
fn fsck_flags_extents_that_point_into_metadata_or_overlap() {
    for which in 0..4 {
        let mut fs = forged(|fs| {
            let ino = fs.lookup("/a/b/file")?;
            let mut n = fs.read_inode(ino)?;
            match which {
                0 => n.inline[0].pblk = 1,                  // inside the journal
                1 => n.inline[0].pblk = fs.geo.backup_sb(), // the superblock copy
                2 => {
                    n.inline[1] = n.inline[0]; // duplicate extent: overlapping, unsorted
                    n.nextents = 2;
                }
                _ => n.inline[0].len = 0,
            }
            fs.write_inode(ino, &n)
        });
        let r = fs.fsck().unwrap();
        assert!(!r.is_clean(), "variant {which}: {r:?}");
        hammer(&mut fs);
    }
}

#[test]
fn fsck_flags_nonzero_bytes_past_end_of_file() {
    let mut fs = populated();
    let ino = fs.lookup("/a/other").unwrap(); // 5000 bytes: 3192 slack bytes in block 2
    let node = fs.read_inode(ino).unwrap();
    let pb = fs.load_extents(ino, &node).unwrap().last().unwrap().pblk;
    let last = pb + node.nblocks - 1;
    fs.device_mut().as_bytes_mut()[blk_off(last) + 4000] = 0x5A;
    fs.cache.invalidate_all();
    let r = fs.fsck().unwrap();
    assert!(r.has("non-zero bytes past end of file"), "{r:?}");
}

#[test]
fn trash_flag_and_location_must_agree() {
    let mut fs = forged(|fs| {
        // Clear the trashed flag on the item that sits in /.trash.
        let ino = fs.lookup("/.trash/gone")?;
        let mut n = fs.read_inode(ino)?;
        n.flags = 0;
        fs.write_inode(ino, &n)
    });
    assert!(fs.fsck().unwrap().has("trash flag does not match location"));
}

#[test]
fn a_committed_unretired_journal_is_reported_by_fsck() {
    use crate::fs3::layout::build_journal_header;
    let mut fs = populated();
    let geo = fs.geo;
    let payload = [0u8; 4096];
    let hdr = build_journal_header(99, &[geo.data_start], &[&payload]);
    let img = fs.device_mut().as_bytes_mut();
    img[blk_off(geo.jhdr)..blk_off(geo.jhdr) + 4096].copy_from_slice(&hdr);
    img[blk_off(geo.jpayload)..blk_off(geo.jpayload) + 4096].copy_from_slice(&payload);
    fs.cache.invalidate_all();
    assert!(
        fs.fsck()
            .unwrap()
            .has("journal holds a committed, unretired transaction")
    );
}

#[test]
fn hostile_extent_chain_loops_are_cut() {
    let mut fs = populated();
    let frag = fs.lookup("/frag").unwrap();
    let node = fs.read_inode(frag).unwrap();
    let chain = fs.walk_chain(frag, node.ext_chain).unwrap();
    // Make the (last) chain block point back at itself, with a valid checksum.
    let o = blk_off(chain[0]);
    let img = fs.device_mut().as_bytes_mut();
    wr32(&mut img[o..o + 4096], 8, chain[0]);
    let c = crate::fs3::crc32::crc32(&img[o + 4..o + 4096]);
    wr32(&mut img[o..o + 4096], 0, c);
    assert_eq!(rd32(&img[o..o + 4096], 8), chain[0]);
    fs.cache.invalidate_all();
    assert!(matches!(fs.read_file("/frag"), Err(FsError::Corrupt(_))));
    assert!(!fs.fsck().unwrap().is_clean());
    hammer(&mut fs);
}

#[test]
fn mount_verified_refuses_what_fsck_rejects_but_mount_accepts() {
    let mut fs = populated();
    let free = bits::next_free(&fs.bbits, fs.geo.data_start, fs.geo.backup_sb()).unwrap();
    fs.txn(|fs| fs.mark_range(free, 1, true)).unwrap(); // a leaked block
    let disk = fs.into_device();
    assert!(Fs3::mount(disk.clone()).is_ok());
    assert!(matches!(
        Fs3::mount_verified(disk),
        Err(FsError::Corrupt(_))
    ));
}
