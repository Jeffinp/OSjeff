//! Fuzz target: the OJFS v3 on-disk format (`kitsune_core::fs3`).
//!
//! The "disk" is attacker/corruption controlled. It is built in one of three
//! ways (see [`Image`]):
//!
//! * `Patched`: a valid, populated 1 MiB filesystem with fuzzed byte patches
//!   over its metadata. With `fix_crc` every checksum is recomputed after the
//!   patches, so the fuzzer gets past CRC32 (which coverage guidance cannot
//!   solve) and reaches the *semantic* checks: extent lists, directory
//!   entries, bitmaps, parent pointers, link counts, trash fields, cycles;
//! * `Raw`: arbitrary bytes laid over a zero disk from LBA 128;
//! * `Short`: arbitrary bytes used as the whole device (tiny/odd sizes).
//!
//! Then `detect`, `mount`, a bounded walk (readdir/stat/read/path_of/trash
//! list), `fsck`, a fuzzed sequence of mutating operations, and `fsck` again.
//! Nothing may panic, loop, or allocate absurdly; a *valid* filesystem must
//! stay fsck-clean through the operations.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use kitsune_core::blockdev::{BlockDevice, RamDisk};
use kitsune_core::fs3::crc32::crc32;
use kitsune_core::fs3::{Detected, FormatOptions, Fs3, Kind, detect};
use std::sync::OnceLock;

const BASE: usize = 128 * 512;
const BLOCKS: usize = 240; // a 1 MiB device holds 240 v3 blocks

#[derive(Arbitrary, Debug)]
struct Patch {
    block: u8,
    off: u16,
    bytes: Vec<u8>,
}

#[derive(Arbitrary, Debug)]
enum Image {
    Patched { patches: Vec<Patch>, fix_crc: bool },
    Raw(Vec<u8>),
    Short(Vec<u8>),
}

#[derive(Arbitrary, Debug)]
enum Op {
    Create(Vec<u8>),
    Mkdir(Vec<u8>),
    Write(Vec<u8>, Vec<u8>),
    WriteAt(Vec<u8>, u16, Vec<u8>),
    Append(Vec<u8>, Vec<u8>),
    Truncate(Vec<u8>, u32),
    Rename(Vec<u8>, Vec<u8>),
    Remove(Vec<u8>),
    Rmdir(Vec<u8>),
    RemoveAll(Vec<u8>),
    Trash(Vec<u8>),
    Restore(Vec<u8>),
    Purge(Vec<u8>),
    EmptyTrash,
}

#[derive(Arbitrary, Debug)]
struct Input {
    image: Image,
    ops: Vec<Op>,
    cache: u8,
}

/// Paths from a small alphabet so operations actually collide.
fn path(raw: &[u8]) -> Vec<u8> {
    const A: &[u8] = b"//aabbcd.\0";
    raw.iter().take(24).map(|&b| A[b as usize % A.len()]).collect()
}

fn base_image() -> &'static Vec<u8> {
    static BASE_IMG: OnceLock<Vec<u8>> = OnceLock::new();
    BASE_IMG.get_or_init(|| {
        let mut fs = Fs3::format(RamDisk::new(2048), &FormatOptions::new([9; 16], 100)).unwrap();
        fs.mkdir("/a", 1).unwrap();
        fs.mkdir("/a/b", 2).unwrap();
        fs.write_file("/a/b/f", &[7u8; 9000], 3).unwrap();
        fs.write_file("/c", b"tiny", 4).unwrap();
        let x = fs.create("/x", 5).unwrap();
        let y = fs.create("/y", 5).unwrap();
        for _ in 0..16 {
            fs.append(x, &[1u8; 4096], 6).unwrap();
            fs.append(y, &[2u8; 4096], 6).unwrap();
        }
        fs.write_file("/gone", b"bye", 7).unwrap();
        fs.trash("/gone", 8).unwrap();
        fs.into_device().into_bytes()
    })
}

/// Recompute every checksum the format uses (superblock copies, bitmap
/// blocks, inodes, directory and extent blocks) so that patches survive CRC
/// validation.
fn reseal(img: &mut [u8]) {
    let seal = |b: &mut [u8]| {
        let c = crc32(&b[4..]);
        b[..4].copy_from_slice(&c.to_le_bytes());
    };
    for o in [BASE, BASE + (BLOCKS - 1) * 4096] {
        if img.len() >= o + 512 && &img[o + 4..o + 8] == b"OJF3" {
            seal(&mut img[o..o + 512]);
        }
    }
    // Geometry of the 240-block default: bitmaps at 26/27, inode table 28..36.
    for blk in [26usize, 27] {
        let o = BASE + blk * 4096;
        if img.len() >= o + 4096 {
            seal(&mut img[o..o + 4096]);
        }
    }
    for blk in 28..36usize {
        for slot in 0..8 {
            let o = BASE + blk * 4096 + slot * 512;
            if img.len() >= o + 512 {
                seal(&mut img[o..o + 512]);
            }
        }
    }
    for blk in 36..BLOCKS - 1 {
        let o = BASE + blk * 4096;
        if img.len() >= o + 4096 && (&img[o + 4..o + 8] == b"OJD3" || &img[o + 4..o + 8] == b"OJX3")
        {
            seal(&mut img[o..o + 4096]);
        }
    }
}

fn build(image: &Image) -> Vec<u8> {
    match image {
        Image::Patched { patches, fix_crc } => {
            let mut img = base_image().clone();
            for p in patches.iter().take(16) {
                let o = BASE + (p.block as usize % 64) * 4096 + (p.off as usize % 4096);
                let n = p.bytes.len().min(256).min(img.len() - o);
                img[o..o + n].copy_from_slice(&p.bytes[..n]);
            }
            if *fix_crc {
                reseal(&mut img);
            }
            img
        }
        Image::Raw(b) => {
            let mut img = vec![0u8; 2048 * 512];
            let n = b.len().min(img.len() - BASE);
            img[BASE..BASE + n].copy_from_slice(&b[..n]);
            img
        }
        Image::Short(b) => b.clone(),
    }
}

fn walk(fs: &mut Fs3<RamDisk>) {
    let mut stack = vec![b"/".to_vec(), b"/.trash".to_vec()];
    let mut seen = 0u32;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs.readdir(&dir) else {
            continue;
        };
        for e in entries {
            seen += 1;
            if seen > 600 {
                return;
            }
            let mut p = dir.clone();
            if p != b"/" {
                p.push(b'/');
            }
            p.extend_from_slice(&e.name);
            let _ = fs.stat(&p);
            let _ = fs.path_of(e.ino);
            match e.kind {
                Kind::Dir => {
                    if p.len() < 400 {
                        stack.push(p);
                    }
                }
                Kind::File => {
                    let mut buf = [0u8; 4096];
                    for off in [0u64, 4095, 1 << 20] {
                        let _ = fs.read_at(e.ino, off, &mut buf);
                    }
                }
            }
        }
    }
    let _ = fs.trash_list();
    let _ = fs.statfs();
}

fn apply(fs: &mut Fs3<RamDisk>, op: &Op, now: u64) {
    match op {
        Op::Create(p) => {
            let _ = fs.create(&path(p), now);
        }
        Op::Mkdir(p) => {
            let _ = fs.mkdir(&path(p), now);
        }
        Op::Write(p, d) => {
            let _ = fs.write_file(&path(p), &d[..d.len().min(20_000)], now);
        }
        Op::WriteAt(p, off, d) => {
            if let Ok(i) = fs.lookup(&path(p)) {
                let _ = fs.write_at(i, *off as u64 * 3, &d[..d.len().min(20_000)], now);
            }
        }
        Op::Append(p, d) => {
            if let Ok(i) = fs.lookup(&path(p)) {
                let _ = fs.append(i, &d[..d.len().min(20_000)], now);
            }
        }
        Op::Truncate(p, n) => {
            if let Ok(i) = fs.lookup(&path(p)) {
                let _ = fs.truncate(i, *n as u64, now);
            }
        }
        Op::Rename(a, b) => {
            let _ = fs.rename(&path(a), &path(b), now);
        }
        Op::Remove(p) => {
            let _ = fs.remove(&path(p));
        }
        Op::Rmdir(p) => {
            let _ = fs.rmdir(&path(p));
        }
        Op::RemoveAll(p) => {
            let _ = fs.remove_all(&path(p));
        }
        Op::Trash(p) => {
            let _ = fs.trash(&path(p), now);
        }
        Op::Restore(n) => {
            let _ = fs.trash_restore(&path(n), now);
        }
        Op::Purge(n) => {
            let _ = fs.trash_purge(&path(n));
        }
        Op::EmptyTrash => {
            let _ = fs.empty_trash();
        }
    }
}

fuzz_target!(|input: Input| {
    let bytes = build(&input.image);
    let mut disk = RamDisk::from_bytes(bytes);
    let detected = detect(&mut disk);
    let Ok(mut fs) = Fs3::mount_with(disk, 2 + input.cache as usize % 8) else {
        // Not mountable: that is only legitimate if detect did not say "valid
        // and healthy"; either way it must not have panicked.
        let _ = detected;
        return;
    };
    debug_assert!(matches!(detected, Ok(Detected::V3)));
    let healthy_before = fs.fsck().map(|r| r.is_clean()).unwrap_or(false);
    walk(&mut fs);
    for (i, op) in input.ops.iter().take(16).enumerate() {
        apply(&mut fs, op, 1_000 + i as u64);
    }
    walk(&mut fs);
    if let Ok(r) = fs.fsck() {
        // A consistent filesystem must stay consistent. (A damaged one may
        // legitimately stay damaged, so only check the healthy case.)
        if healthy_before && !r.is_clean() {
            // Operations may legitimately refuse to run on a poisoned fs; a
            // poisoned fsck returns Err and is skipped above.
            panic!("fsck dirty after operations on a clean filesystem: {r:?}");
        }
    }
    let _ = fs.into_device().sector_count();
});
