//! Disks written by the formatter of the OSjeff days must keep mounting after the
//! rename: the on-disk identifiers (`OJF3`, `OJF2`, ...) are deliberately unchanged.
//!
//! The golden images are sparse dumps (`<lba> <hex of one 512-byte sector>` per
//! non-zero sector) made by the formatter of the commit before the rename.
//! Regenerate ONLY when the on-disk format changes on purpose (a format revision):
//! `cargo test -p kitsune_core golden_dump -- --ignored --nocapture` prints both
//! files; paste them into `kitsune_core/tests/golden/` and say why in the commit.

use super::*;
use crate::storage::fs as v2;
use crate::storage::fs3::migrate_v2;

const V3_GOLDEN: &str = include_str!("../../../../tests/golden/ojfs3-min.sparse");
const V2_GOLDEN: &str = include_str!("../../../../tests/golden/ojfs2-seeded.sparse");

fn encode(img: &[u8]) -> String {
    let mut out = String::new();
    for (lba, s) in img.chunks(512).enumerate() {
        if s.iter().any(|&b| b != 0) {
            out.push_str(&alloc::format!("{lba} "));
            for b in s {
                out.push_str(&alloc::format!("{b:02x}"));
            }
            out.push('\n');
        }
    }
    out
}

fn decode(text: &str, total_sectors: usize) -> Vec<u8> {
    let mut img = alloc::vec![0u8; total_sectors * 512];
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let (lba, hex) = line.split_once(' ').unwrap();
        let lba: usize = lba.parse().unwrap();
        for i in 0..512 {
            img[lba * 512 + i] = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap();
        }
    }
    img
}

const V3_SECTORS: usize = 2048; // 1 MiB

fn build_v3() -> Vec<u8> {
    let mut fs = Fs3::format(
        RamDisk::new(V3_SECTORS as u64),
        &FormatOptions::new(UUID, 1_000),
    )
    .unwrap();
    fs.write_file("/leiame.txt", b"Bem-vindo ao OSjeff.\n", 1_001)
        .unwrap();
    fs.mkdir("/etc", 1_002).unwrap();
    fs.write_file("/etc/osjeff.conf", b"# OSjeff settings\nversion=1\n", 1_003)
        .unwrap();
    fs.into_device().into_bytes()
}

fn build_v2() -> Vec<u8> {
    let mut img = alloc::vec![0u8; v2::IMAGE_SIZE];
    v2::format(&mut img);
    v2::write(&mut img, b"leiame.txt", b"Bem-vindo ao OSjeff.\n").unwrap();
    img
}

#[test]
#[ignore = "prints the golden files; see the module docs"]
fn golden_dump() {
    println!("=== ojfs3-min.sparse\n{}", encode(&build_v3()));
    println!("=== ojfs2-seeded.sparse\n{}", encode(&build_v2()));
}

#[test]
fn v3_disk_from_the_osjeff_formatter_still_mounts() {
    let disk = RamDisk::from_bytes(decode(V3_GOLDEN, V3_SECTORS));
    let mut fs = Fs3::mount_verified(disk).unwrap();
    assert_eq!(
        fs.read_file("/leiame.txt").unwrap(),
        b"Bem-vindo ao OSjeff.\n"
    );
    assert_eq!(
        fs.read_file("/etc/osjeff.conf").unwrap(),
        b"# OSjeff settings\nversion=1\n"
    );
    assert_clean(&mut fs);
    // And it is still writable by today's code.
    fs.write_file("/etc/kitsune.conf", b"x", 2_000).unwrap();
    assert_clean(&mut fs);
}

#[test]
fn the_formatter_still_writes_the_historical_magic() {
    let img = build_v3();
    assert!(
        img.chunks(512).any(|s| &s[4..8] == b"OJF3"),
        "superblock magic is the on-disk name"
    );
    assert_eq!(&build_v2()[..4], b"OJF2");
}

#[test]
fn v2_image_from_the_osjeff_formatter_still_migrates() {
    let mut img = decode(V2_GOLDEN, v2::IMAGE_SIZE.div_ceil(512));
    img.truncate(v2::IMAGE_SIZE);
    assert_eq!(&img[..4], b"OJF2");
    let mut disk = RamDisk::new(4096);
    disk.as_bytes_mut()[..img.len()].copy_from_slice(&img);
    let o = FormatOptions::new(UUID, 5_000);
    migrate_v2(&mut disk, &img, &o).unwrap();
    let mut fs = Fs3::mount_verified(disk).unwrap();
    assert_eq!(
        fs.read_file("/leiame.txt").unwrap(),
        b"Bem-vindo ao OSjeff.\n"
    );
}
