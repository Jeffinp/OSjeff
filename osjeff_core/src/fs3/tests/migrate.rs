//! Detection and v2 -> v3 migration.

use super::crash::{Op, snapshot};
use super::*;
use crate::blockdev::{BlockDevice, CrashMode, FaultyDisk};
use crate::fs as v2;
use crate::fs3::{Detected, MigrateError, detect, migrate_v2, read_v2_image};
use alloc::collections::BTreeMap;

const LEIAME: &[u8] = b"Bem-vindo ao OSjeff.\nGerenciador de arquivos:\n setas   navegam\n Del     manda pra lixeira\n Tab     alterna arquivos/lixeira\n Enter   abre\n";

/// The image the kernel seeds on a blank disk (see `desktop/mod.rs`).
fn seeded() -> Vec<u8> {
    let mut img = alloc::vec![0u8; v2::IMAGE_SIZE];
    v2::format(&mut img);
    v2::write(&mut img, b"leiame.txt", LEIAME).unwrap();
    v2::write(&mut img, b"notas.txt", b"Arquivo de exemplo do OSjeff.").unwrap();
    let d = v2::mkdir(&mut img, v2::ROOT, b"Documentos").unwrap();
    v2::write_in(
        &mut img,
        d as u8,
        b"projeto.txt",
        b"Arquivo dentro de uma pasta.",
    )
    .unwrap();
    img
}

/// A disk of `sectors` sectors with the v2 image at LBA 0 (as the kernel stores it).
fn v2_disk(img: &[u8], sectors: u64) -> RamDisk {
    let mut d = RamDisk::new(sectors);
    let padded = img.len().div_ceil(512) * 512;
    let mut buf = alloc::vec![0u8; padded];
    buf[..img.len()].copy_from_slice(img);
    d.write_sectors(0, &buf).unwrap();
    d.reset_counters();
    d
}

fn opts() -> FormatOptions {
    FormatOptions::new(UUID, 5_000)
}

fn tree(fs: &mut Fs3<RamDisk>) -> BTreeMap<Vec<u8>, Option<Vec<u8>>> {
    snapshot(fs)
}

fn expect(pairs: &[(&str, Option<&[u8]>)]) -> BTreeMap<Vec<u8>, Option<Vec<u8>>> {
    let mut m = BTreeMap::new();
    m.insert(b"/.trash".to_vec(), None);
    for (p, c) in pairs {
        m.insert(p.as_bytes().to_vec(), c.map(|c| c.to_vec()));
    }
    m
}

#[test]
fn detect_blank_v2_v3_unknown() {
    let mut blank = RamDisk::new(4096);
    assert_eq!(detect(&mut blank).unwrap(), Detected::Blank);
    let mut d = v2_disk(&seeded(), 4096);
    assert_eq!(detect(&mut d).unwrap(), Detected::V2);
    let mut junk = RamDisk::new(4096);
    junk.write_sectors(0, &[0x33u8; 512]).unwrap();
    assert_eq!(detect(&mut junk).unwrap(), Detected::Unknown);
    let fs = Fs3::format(RamDisk::new(4096), &opts()).unwrap();
    let mut v3 = fs.into_device();
    assert_eq!(detect(&mut v3).unwrap(), Detected::V3);
}

#[test]
fn detect_on_tiny_and_empty_devices() {
    // The 99-sector v2 image alone on a tiny device (the kernel's old layout).
    let mut tiny = v2_disk(&seeded(), 99);
    assert_eq!(detect(&mut tiny).unwrap(), Detected::V2);
    let mut zero = RamDisk::new(99);
    assert_eq!(detect(&mut zero).unwrap(), Detected::Blank);
    let mut none = RamDisk::new(0);
    assert_eq!(detect(&mut none).unwrap(), Detected::Blank);
    let mut one = RamDisk::new(1);
    assert_eq!(detect(&mut one).unwrap(), Detected::Blank);
    // 64 KiB disk (128 sectors): v2 only.
    let mut small = v2_disk(&seeded(), 128);
    assert_eq!(detect(&mut small).unwrap(), Detected::V2);
}

#[test]
fn v3_wins_over_a_leftover_v2_image() {
    let mut d = v2_disk(&seeded(), 4096);
    migrate_v2(&mut d, &seeded(), &opts()).unwrap();
    assert_eq!(detect(&mut d).unwrap(), Detected::V3);
    // The v2 image is still physically there.
    assert_eq!(d.as_bytes()[..4], *b"OJF2");
}

#[test]
fn read_v2_image_roundtrips() {
    let img = seeded();
    let mut d = v2_disk(&img, 4096);
    assert_eq!(read_v2_image(&mut d).unwrap(), img);
    let mut tiny = RamDisk::new(50);
    assert!(read_v2_image(&mut tiny).is_err());
}

#[test]
fn migrates_the_kernel_seeded_image() {
    let img = seeded();
    let mut disk = v2_disk(&img, 4096);
    let before_reserved = disk.as_bytes()[..128 * 512].to_vec();
    let rep = migrate_v2(&mut disk, &img, &opts()).unwrap();
    assert_eq!(
        (rep.files, rep.dirs, rep.trashed, rep.renamed, rep.orphans),
        (3, 1, 0, 0, 0)
    );
    assert_eq!(rep.bytes, (LEIAME.len() + 29 + 28) as u64);
    // The reserved area is byte-for-byte untouched and never written.
    assert_eq!(&disk.as_bytes()[..128 * 512], &before_reserved[..]);
    assert!(disk.min_written_lba().unwrap() >= 128);
    assert_eq!(detect(&mut disk).unwrap(), Detected::V3);
    let mut fs = Fs3::mount_verified(disk).unwrap();
    assert_eq!(fs.read_file("/leiame.txt").unwrap(), LEIAME);
    assert_eq!(
        fs.read_file("/notas.txt").unwrap(),
        b"Arquivo de exemplo do OSjeff."
    );
    assert_eq!(
        fs.read_file("/Documentos/projeto.txt").unwrap(),
        b"Arquivo dentro de uma pasta."
    );
    assert_eq!(fs.stat("/Documentos").unwrap().kind, Kind::Dir);
    assert_eq!(fs.stat("/leiame.txt").unwrap().ctime, 5_000);
    assert!(fs.trash_list().unwrap().is_empty());
    assert_clean(&mut fs);
}

#[test]
fn migrated_filesystem_is_fully_usable() {
    let img = seeded();
    let mut disk = v2_disk(&img, 4096);
    migrate_v2(&mut disk, &img, &opts()).unwrap();
    let mut fs = Fs3::mount(disk).unwrap();
    fs.write_file("/Documentos/novo.txt", &[1u8; 50_000], 9)
        .unwrap();
    fs.rename("/notas.txt", "/Documentos/notas.txt", 9).unwrap();
    fs.trash("/leiame.txt", 9).unwrap();
    assert_eq!(fs.trash_restore(b"leiame.txt", 10).unwrap(), b"/leiame.txt");
    assert_clean(&mut fs);
}

#[test]
fn migration_with_a_disk_larger_than_the_minimum() {
    let img = seeded();
    let mut disk = v2_disk(&img, 64 * 2048); // 64 MiB
    migrate_v2(&mut disk, &img, &opts()).unwrap();
    let mut fs = Fs3::mount_verified(disk).unwrap();
    assert!(fs.statfs().total_blocks > 16_000);
    assert_eq!(
        fs.read_file("/notas.txt").unwrap(),
        b"Arquivo de exemplo do OSjeff."
    );
}

#[test]
fn too_small_disks_are_refused_without_touching_anything() {
    let img = seeded();
    for sectors in [99u64, 128, 136, 1000, 2047] {
        let mut disk = v2_disk(&img, sectors);
        let before = disk.as_bytes().to_vec();
        assert_eq!(
            migrate_v2(&mut disk, &img, &opts()),
            Err(MigrateError::TooSmall),
            "{sectors} sectors"
        );
        assert_eq!(
            disk.as_bytes(),
            &before[..],
            "{sectors} sectors were modified"
        );
        assert_eq!(disk.counters().sectors_written, 0);
        assert_eq!(detect(&mut disk).unwrap(), Detected::V2);
    }
}

#[test]
fn content_that_does_not_fit_is_refused_without_writing() {
    // A full 48 x 1 KiB image on the smallest disk fits; fill every record with
    // 1 KiB, then shrink the usable area with a huge journal so it cannot.
    let img = full_image();
    let mut disk = v2_disk(&img, 2048);
    let mut o = opts();
    o.journal_blocks = Some(200);
    let before = disk.as_bytes().to_vec();
    assert_eq!(migrate_v2(&mut disk, &img, &o), Err(MigrateError::TooSmall));
    assert_eq!(disk.as_bytes(), &before[..]);
}

#[test]
fn refuses_a_non_v2_image_and_an_existing_v3() {
    let mut disk = RamDisk::new(4096);
    assert_eq!(
        migrate_v2(&mut disk, &[0u8; v2::IMAGE_SIZE], &opts()),
        Err(MigrateError::NotV2)
    );
    assert_eq!(
        migrate_v2(&mut disk, &[], &opts()),
        Err(MigrateError::NotV2)
    );
    assert_eq!(disk.counters().sectors_written, 0);
    // An existing v3 must never be overwritten by a migration.
    let img = seeded();
    let mut d = v2_disk(&img, 4096);
    migrate_v2(&mut d, &img, &opts()).unwrap();
    let before = d.as_bytes().to_vec();
    assert_eq!(
        migrate_v2(&mut d, &img, &opts()),
        Err(MigrateError::AlreadyV3)
    );
    assert_eq!(d.as_bytes(), &before[..]);
    // Nor a damaged one.
    d.as_bytes_mut()[128 * 512 + 40] ^= 1;
    let last = d.as_bytes().len() - 4096;
    d.as_bytes_mut()[last + 40] ^= 1;
    assert!(matches!(
        migrate_v2(&mut d, &img, &opts()),
        Err(MigrateError::Fs(FsError::BadSuperblock))
    ));
}

fn full_image() -> Vec<u8> {
    let mut img = alloc::vec![0u8; v2::IMAGE_SIZE];
    v2::format(&mut img);
    for i in 0..v2::MAX_FILES {
        let name = alloc::format!("arquivo{i:02}.txt");
        let data = alloc::vec![(i * 5 + 1) as u8; v2::MAX_FILE_SIZE];
        v2::write(&mut img, name.as_bytes(), &data).unwrap();
    }
    assert_eq!(v2::count(&img), 48);
    img
}

#[test]
fn migrates_a_full_v2_image_of_48_files_of_1024_bytes() {
    let img = full_image();
    let mut disk = v2_disk(&img, 2048); // even the minimum disk holds it
    let rep = migrate_v2(&mut disk, &img, &opts()).unwrap();
    assert_eq!((rep.files, rep.dirs), (48, 0));
    assert_eq!(rep.bytes, 48 * 1024);
    let mut fs = Fs3::mount_verified(disk).unwrap();
    for i in 0..48 {
        let name = alloc::format!("/arquivo{i:02}.txt");
        assert_eq!(
            fs.read_file(&name).unwrap(),
            alloc::vec![(i * 5 + 1) as u8; 1024],
            "{name}"
        );
    }
    assert_eq!(fs.readdir("/").unwrap().len(), 48);
}

#[test]
fn migrates_a_full_image_made_of_folders_and_files() {
    let mut img = alloc::vec![0u8; v2::IMAGE_SIZE];
    v2::format(&mut img);
    let d1 = v2::mkdir(&mut img, v2::ROOT, b"um").unwrap() as u8;
    let d2 = v2::mkdir(&mut img, d1, b"dois").unwrap() as u8;
    let d3 = v2::mkdir(&mut img, d2, b"tres").unwrap() as u8;
    for i in 0..44 {
        let parent = [v2::ROOT, d1, d2, d3][i % 4];
        v2::write_in(
            &mut img,
            parent,
            alloc::format!("f{i}").as_bytes(),
            &[i as u8; 1024],
        )
        .unwrap();
    }
    assert_eq!(v2::count(&img), 47);
    let mut disk = v2_disk(&img, 2048);
    migrate_v2(&mut disk, &img, &opts()).unwrap();
    let mut fs = Fs3::mount_verified(disk).unwrap();
    for i in 0..44usize {
        let dir = ["", "/um", "/um/dois", "/um/dois/tres"][i % 4];
        let p = alloc::format!("{dir}/f{i}");
        assert_eq!(fs.read_file(&p).unwrap(), alloc::vec![i as u8; 1024], "{p}");
    }
}

#[test]
fn same_names_in_different_folders_stay_separate() {
    let mut img = alloc::vec![0u8; v2::IMAGE_SIZE];
    v2::format(&mut img);
    let d = v2::mkdir(&mut img, v2::ROOT, b"docs").unwrap() as u8;
    v2::write(&mut img, b"p.txt", b"root").unwrap();
    v2::write_in(&mut img, d, b"p.txt", b"inside").unwrap();
    let mut disk = v2_disk(&img, 2048);
    migrate_v2(&mut disk, &img, &opts()).unwrap();
    let mut fs = Fs3::mount(disk).unwrap();
    assert_eq!(fs.read_file("/p.txt").unwrap(), b"root");
    assert_eq!(fs.read_file("/docs/p.txt").unwrap(), b"inside");
}

#[test]
fn trash_state_is_preserved() {
    let mut img = seeded();
    // /Documentos/projeto.txt is trashed, notas.txt is trashed, and so is a folder
    // with a file inside.
    let docs = v2::find(&img, b"Documentos").unwrap();
    let projeto = v2::find_in(&img, docs as u8, b"projeto.txt").unwrap();
    v2::trash_slot(&mut img, projeto);
    v2::trash(&mut img, b"notas.txt").unwrap();
    let lixo = v2::mkdir(&mut img, v2::ROOT, b"lixo").unwrap();
    v2::write_in(&mut img, lixo as u8, b"dentro.txt", b"x").unwrap();
    v2::trash(&mut img, b"lixo").unwrap();
    let mut disk = v2_disk(&img, 2048);
    let rep = migrate_v2(&mut disk, &img, &opts()).unwrap();
    assert_eq!(rep.trashed, 3);
    let mut fs = Fs3::mount_verified(disk).unwrap();
    assert_eq!(
        tree(&mut fs),
        expect(&[
            ("/leiame.txt", Some(LEIAME)),
            ("/Documentos", None),
            ("/.trash/projeto.txt", Some(b"Arquivo dentro de uma pasta.")),
            ("/.trash/notas.txt", Some(b"Arquivo de exemplo do OSjeff.")),
            ("/.trash/lixo", None),
            ("/.trash/lixo/dentro.txt", Some(b"x")),
        ])
    );
    // Restoring puts things back where v2 had them.
    assert_eq!(
        fs.trash_restore(b"projeto.txt", 1).unwrap(),
        b"/Documentos/projeto.txt"
    );
    assert_eq!(fs.trash_restore(b"notas.txt", 1).unwrap(), b"/notas.txt");
    assert_eq!(fs.trash_restore(b"lixo", 1).unwrap(), b"/lixo");
    assert_eq!(fs.read_file("/lixo/dentro.txt").unwrap(), b"x");
    assert_clean(&mut fs);
}

#[test]
fn several_trashed_files_with_the_same_name_are_all_kept() {
    let mut img = alloc::vec![0u8; v2::IMAGE_SIZE];
    v2::format(&mut img);
    for i in 0..3u8 {
        v2::write(&mut img, b"dup.txt", &[i; 10]).unwrap();
        v2::trash(&mut img, b"dup.txt").unwrap();
        // Same name again: v2 allocates the next free slot while the old one is trashed.
    }
    assert_eq!(v2::count_trashed(&img), 3);
    let mut disk = v2_disk(&img, 2048);
    migrate_v2(&mut disk, &img, &opts()).unwrap();
    let mut fs = Fs3::mount_verified(disk).unwrap();
    let t = fs.trash_list().unwrap();
    assert_eq!(t.len(), 3);
    assert!(t.iter().all(|e| e.orig_name == b"dup.txt"));
    let mut contents: Vec<u8> = t
        .iter()
        .map(|e| {
            fs.read_file(&[b"/.trash/".as_slice(), &e.trash_name].concat())
                .unwrap()[0]
        })
        .collect();
    contents.sort();
    assert_eq!(contents, [0, 1, 2]);
}

#[test]
fn invalid_and_colliding_v2_names_are_repaired_and_counted() {
    let mut img = alloc::vec![0u8; v2::IMAGE_SIZE];
    v2::format(&mut img);
    // v2 accepted any bytes: slash, NUL, dots, empty-looking and reserved names.
    v2::write(&mut img, b"a/b", b"1").unwrap();
    v2::write(&mut img, b"x\0y", b"2").unwrap();
    v2::write(&mut img, b".", b"3").unwrap();
    v2::write(&mut img, b"..", b"4").unwrap();
    v2::write(&mut img, b".trash", b"5").unwrap();
    v2::write(&mut img, b"ok", b"6").unwrap();
    let mut disk = v2_disk(&img, 2048);
    let rep = migrate_v2(&mut disk, &img, &opts()).unwrap();
    assert_eq!(rep.files, 6);
    assert_eq!(rep.renamed, 5);
    let mut fs = Fs3::mount_verified(disk).unwrap();
    assert_eq!(fs.read_file("/a_b").unwrap(), b"1");
    assert_eq!(fs.read_file("/x_y").unwrap(), b"2");
    assert_eq!(fs.read_file("/_.").unwrap(), b"3");
    assert_eq!(fs.read_file("/_..").unwrap(), b"4");
    assert_eq!(fs.read_file("/.trash~2").unwrap(), b"5");
    assert_eq!(fs.read_file("/ok").unwrap(), b"6");
    assert_clean(&mut fs);
}

#[test]
fn name_collisions_after_sanitizing_get_numbered() {
    let mut img = alloc::vec![0u8; v2::IMAGE_SIZE];
    v2::format(&mut img);
    v2::write(&mut img, b"a_b", b"1").unwrap();
    v2::write(&mut img, b"a/b", b"2").unwrap();
    v2::write(&mut img, b"a\0b", b"3").unwrap();
    let mut disk = v2_disk(&img, 2048);
    migrate_v2(&mut disk, &img, &opts()).unwrap();
    let mut fs = Fs3::mount_verified(disk).unwrap();
    let mut got: Vec<Vec<u8>> = (["/a_b", "/a_b~2", "/a_b~3"])
        .iter()
        .map(|p| fs.read_file(p).unwrap())
        .collect();
    got.sort();
    assert_eq!(got, [b"1".to_vec(), b"2".to_vec(), b"3".to_vec()]);
}

#[test]
fn records_with_invalid_parents_go_to_the_root() {
    let mut img = alloc::vec![0u8; v2::IMAGE_SIZE];
    v2::format(&mut img);
    let d = v2::mkdir(&mut img, v2::ROOT, b"d").unwrap();
    v2::write_in(&mut img, d as u8, b"kid", b"k").unwrap();
    v2::write(&mut img, b"file", b"f").unwrap();
    let kid = v2::find_in(&img, d as u8, b"kid").unwrap();
    let file = v2::find(&img, b"file").unwrap();
    // Corrupt: kid's parent is a free slot; a record whose parent is a file.
    let rec = |i: usize| 4 + i * 1046;
    img[rec(kid) + 2] = 40;
    v2::write(&mut img, b"orphan2", b"o").unwrap();
    let o2 = v2::find(&img, b"orphan2").unwrap();
    img[rec(o2) + 2] = file as u8;
    // And one with an out-of-range parent byte.
    v2::write(&mut img, b"orphan3", b"p").unwrap();
    let o3 = v2::find(&img, b"orphan3").unwrap();
    img[rec(o3) + 2] = 200;
    let mut disk = v2_disk(&img, 2048);
    let rep = migrate_v2(&mut disk, &img, &opts()).unwrap();
    assert_eq!(rep.orphans, 3);
    let mut fs = Fs3::mount_verified(disk).unwrap();
    for (p, c) in [
        ("/kid", &b"k"[..]),
        ("/orphan2", b"o"),
        ("/orphan3", b"p"),
        ("/file", b"f"),
    ] {
        assert_eq!(fs.read_file(p).unwrap(), c, "{p}");
    }
}

#[test]
fn parent_cycles_in_v2_are_broken() {
    let mut img = alloc::vec![0u8; v2::IMAGE_SIZE];
    v2::format(&mut img);
    let a = v2::mkdir(&mut img, v2::ROOT, b"a").unwrap();
    let b = v2::mkdir(&mut img, a as u8, b"b").unwrap();
    v2::write_in(&mut img, b as u8, b"leaf", b"L").unwrap();
    let rec = |i: usize| 4 + i * 1046;
    img[rec(a) + 2] = b as u8; // a inside b inside a
    let self_cycle = v2::mkdir(&mut img, v2::ROOT, b"self").unwrap();
    img[rec(self_cycle) + 2] = self_cycle as u8;
    let mut disk = v2_disk(&img, 2048);
    let rep = migrate_v2(&mut disk, &img, &opts()).unwrap();
    assert!(rep.orphans >= 2);
    let mut fs = Fs3::mount_verified(disk).unwrap();
    assert_eq!(fs.readdir("/").unwrap().len(), 2);
    assert_eq!(fs.read_file("/a/b/leaf").unwrap(), b"L");
    assert_clean(&mut fs);
}

#[test]
fn active_record_inside_a_trashed_folder_is_rescued_to_the_root() {
    let mut img = alloc::vec![0u8; v2::IMAGE_SIZE];
    v2::format(&mut img);
    let d = v2::mkdir(&mut img, v2::ROOT, b"d").unwrap();
    v2::trash_slot(&mut img, d); // trashed folder
    v2::write_in(&mut img, d as u8, b"late", b"x").unwrap(); // written into it afterwards
    let mut disk = v2_disk(&img, 2048);
    let rep = migrate_v2(&mut disk, &img, &opts()).unwrap();
    assert_eq!(rep.orphans, 1);
    let mut fs = Fs3::mount_verified(disk).unwrap();
    assert_eq!(fs.read_file("/late").unwrap(), b"x");
    assert_eq!(fs.trash_list().unwrap().len(), 1);
}

#[test]
fn unknown_state_bytes_are_skipped() {
    let mut img = seeded();
    let rec = |i: usize| 4 + i * 1046;
    img[rec(40)] = 7; // garbage state byte in an otherwise free slot
    let mut disk = v2_disk(&img, 2048);
    let rep = migrate_v2(&mut disk, &img, &opts()).unwrap();
    assert_eq!(rep.skipped, 1);
    let mut fs = Fs3::mount_verified(disk).unwrap();
    assert_eq!(fs.readdir("/").unwrap().len(), 3);
}

#[test]
fn corrupt_v2_fields_do_not_break_the_migration() {
    let mut img = seeded();
    let rec = |i: usize| 4 + i * 1046;
    img[rec(0) + 3] = 200; // name_len > 16
    img[rec(1) + 20..rec(1) + 22].copy_from_slice(&0xFFFFu16.to_le_bytes()); // size > 1024
    let mut disk = v2_disk(&img, 2048);
    migrate_v2(&mut disk, &img, &opts()).unwrap();
    let mut fs = Fs3::mount_verified(disk).unwrap();
    assert_eq!(fs.readdir("/").unwrap().len(), 3);
    assert_clean(&mut fs);
}

#[test]
fn an_empty_v2_image_migrates_to_an_empty_v3() {
    let mut img = alloc::vec![0u8; v2::IMAGE_SIZE];
    v2::format(&mut img);
    let mut disk = v2_disk(&img, 2048);
    let rep = migrate_v2(&mut disk, &img, &opts()).unwrap();
    assert_eq!(rep, MigrationReport::default());
    let mut fs = Fs3::mount_verified(disk).unwrap();
    assert!(fs.readdir("/").unwrap().is_empty());
}

#[test]
fn junk_beyond_the_v2_image_in_the_reserved_area_survives() {
    let img = seeded();
    let mut disk = v2_disk(&img, 4096);
    // Bytes 51200..65536 belong to the reserved area too.
    disk.as_bytes_mut()[60_000..65_000].fill(0x99);
    migrate_v2(&mut disk, &img, &opts()).unwrap();
    assert!(disk.as_bytes()[60_000..65_000].iter().all(|&b| b == 0x99));
}

fn power_cut_migration(
    img: &[u8],
    mode: CrashMode,
    stride: u64,
    expect_tree: &BTreeMap<Vec<u8>, Option<Vec<u8>>>,
) {
    let base = v2_disk(img, 2048);
    let reserved = base.as_bytes()[..128 * 512].to_vec();
    let total = {
        let mut d = FaultyDisk::new(base.clone()).with_mode(mode);
        migrate_v2(&mut d, img, &opts()).unwrap();
        d.events()
    };
    assert!(total > 300, "{total}");
    let mut k = 0;
    while k <= total {
        let mut d = FaultyDisk::new(base.clone()).with_mode(mode).crash_after(k);
        let r = migrate_v2(&mut d, img, &opts());
        let mut disk = d.into_inner();
        // The reserved area is never written, whatever the cut.
        assert_eq!(
            &disk.as_bytes()[..128 * 512],
            &reserved[..],
            "cut {k} touched LBA 0..127"
        );
        match detect(&mut disk).unwrap() {
            Detected::V2 => {
                assert!(r.is_err(), "cut {k}: reported success but no v3 visible");
                // The v2 filesystem is intact and readable...
                let again = read_v2_image(&mut disk).unwrap();
                assert_eq!(again, img, "cut {k}: v2 image changed");
                // ...and the migration can simply be run again.
                migrate_v2(&mut disk, img, &opts())
                    .unwrap_or_else(|e| panic!("cut {k}: redo failed {e:?}"));
            }
            Detected::V3 => {}
            other => panic!("cut {k}: detect says {other:?}"),
        }
        let mut fs = Fs3::mount_verified(disk).unwrap_or_else(|e| panic!("cut {k}: {e:?}"));
        assert_eq!(&tree(&mut fs), expect_tree, "cut {k}: wrong content");
        k += stride;
    }
}

fn seeded_tree() -> BTreeMap<Vec<u8>, Option<Vec<u8>>> {
    expect(&[
        ("/leiame.txt", Some(LEIAME)),
        ("/notas.txt", Some(b"Arquivo de exemplo do OSjeff.")),
        ("/Documentos", None),
        (
            "/Documentos/projeto.txt",
            Some(b"Arquivo dentro de uma pasta."),
        ),
    ])
}

#[test]
fn power_cut_at_every_event_of_the_seeded_migration() {
    power_cut_migration(&seeded(), CrashMode::InOrder, 1, &seeded_tree());
}

#[test]
fn power_cut_with_a_volatile_cache_during_the_seeded_migration() {
    for seed in [1u64, 2, 3] {
        power_cut_migration(&seeded(), CrashMode::Lossy(seed), 2, &seeded_tree());
    }
}

#[test]
fn power_cut_during_the_migration_of_a_full_image() {
    let img = full_image();
    let mut m = BTreeMap::new();
    m.insert(b"/.trash".to_vec(), None);
    for i in 0..48 {
        m.insert(
            alloc::format!("/arquivo{i:02}.txt").into_bytes(),
            Some(alloc::vec![(i * 5 + 1) as u8; 1024]),
        );
    }
    power_cut_migration(&img, CrashMode::InOrder, 211, &m);
    power_cut_migration(&img, CrashMode::Lossy(9), 331, &m);
}

#[test]
fn the_v3_becomes_visible_only_at_the_very_last_write() {
    // Count the writes of a successful migration; dropping just the last
    // sector-write pair (primary + backup superblock) must leave v2.
    let img = seeded();
    let base = v2_disk(&img, 2048);
    let mut d = FaultyDisk::new(base.clone());
    migrate_v2(&mut d, &img, &opts()).unwrap();
    let total = d.events();
    // The last events are: sb write, flush, backup write, flush. A cut before
    // the first of them still reads as v2; a cut after it reads as v3.
    let cut = |k: u64| {
        let mut d = FaultyDisk::new(base.clone()).crash_after(k);
        let _ = migrate_v2(&mut d, &img, &opts());
        let mut disk = d.into_inner();
        detect(&mut disk).unwrap()
    };
    assert_eq!(cut(total - 4), Detected::V2);
    assert_eq!(cut(total - 3), Detected::V3);
    assert_eq!(cut(total), Detected::V3);
}

#[test]
fn migration_leaves_an_unrelated_ops_sequence_working() {
    // Smoke: run the standard op sequence on a migrated filesystem.
    let img = seeded();
    let mut disk = v2_disk(&img, 2048);
    migrate_v2(&mut disk, &img, &opts()).unwrap();
    let mut fs = Fs3::mount(disk).unwrap();
    for (i, op) in super::crash::sequence().iter().enumerate() {
        super::crash::apply(&mut fs, op, 100 + i as u64).unwrap();
    }
    assert_clean(&mut fs);
    let _ = Op::Mkdir("/x");
}

use crate::fs3::MigrationReport;
