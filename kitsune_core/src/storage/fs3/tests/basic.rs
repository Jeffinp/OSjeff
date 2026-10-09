//! Namespace behaviour: create/mkdir/lookup/readdir/rename/remove/trash, path and
//! name validation, stat, statfs.

use super::*;

fn names(fs: &mut Fs3<RamDisk>, path: &str) -> Vec<Vec<u8>> {
    let mut v: Vec<Vec<u8>> = fs
        .readdir(path)
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    v.sort();
    v
}

#[test]
fn format_then_mount_roundtrip() {
    let fs = fresh(2);
    assert_eq!(fs.superblock().uuid, UUID);
    assert_eq!(fs.superblock().created, 1_000);
    let mut fs = Fs3::mount(fs.into_device()).unwrap();
    assert_clean(&mut fs);
    assert!(fs.readdir("/").unwrap().is_empty());
}

#[test]
fn format_rejects_disks_under_one_mib() {
    for sectors in [0u64, 1, 99, 128, 136, 1000, 2047] {
        let r = Fs3::format(RamDisk::new(sectors), &FormatOptions::new(UUID, 0));
        assert!(matches!(r, Err(FsError::TooSmall)), "{sectors} sectors");
    }
    assert!(Fs3::format(RamDisk::new(2048), &FormatOptions::new(UUID, 0)).is_ok());
}

#[test]
fn format_never_touches_the_reserved_first_64k() {
    let mut disk = RamDisk::new(4096);
    disk.as_bytes_mut()[..65536].fill(0xA5);
    let mut fs = Fs3::format(disk, &FormatOptions::new(UUID, 0)).unwrap();
    fs.create("/f", 1).unwrap();
    fs.write_file("/g", &[7u8; 20_000], 2).unwrap();
    let disk = fs.into_device();
    assert!(disk.as_bytes()[..65536].iter().all(|&b| b == 0xA5));
    assert!(disk.min_written_lba().unwrap() >= 128);
}

#[test]
fn create_write_read() {
    let mut fs = fresh(2);
    let ino = fs.create("/a.txt", 5).unwrap();
    fs.write_at(ino, 0, b"hello", 6).unwrap();
    let mut buf = [0u8; 16];
    let n = fs.read_at(ino, 0, &mut buf).unwrap();
    assert_eq!(&buf[..n], b"hello");
    assert_clean(&mut fs);
}

#[test]
fn accepts_str_and_bytes_paths() {
    let mut fs = fresh(2);
    fs.create("/a", 0).unwrap();
    fs.create(b"/b", 0).unwrap();
    fs.create(&b"/c"[..], 0).unwrap();
    assert_eq!(
        names(&mut fs, "/"),
        [b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]
    );
}

#[test]
fn create_twice_is_exists() {
    let mut fs = fresh(2);
    fs.create("/a", 0).unwrap();
    assert_eq!(fs.create("/a", 0), Err(FsError::Exists));
    assert_eq!(fs.mkdir("/a", 0), Err(FsError::Exists));
    assert_clean(&mut fs);
}

#[test]
fn failed_create_leaves_no_trace() {
    let mut fs = fresh(2);
    fs.create("/a", 0).unwrap();
    let before = fs.statfs();
    assert!(fs.create("/a", 0).is_err());
    assert_eq!(fs.statfs(), before);
    assert_clean(&mut fs);
}

#[test]
fn mkdir_hierarchy_and_lookup() {
    let mut fs = fresh(2);
    let a = fs.mkdir("/a", 1).unwrap();
    let b = fs.mkdir("/a/b", 2).unwrap();
    let c = fs.mkdir("/a/b/c", 3).unwrap();
    let f = fs.create("/a/b/c/file.txt", 4).unwrap();
    assert_eq!(fs.lookup("/a").unwrap(), a);
    assert_eq!(fs.lookup("/a/b").unwrap(), b);
    assert_eq!(fs.lookup("/a/b/c").unwrap(), c);
    assert_eq!(fs.lookup("/a/b/c/file.txt").unwrap(), f);
    assert_eq!(fs.lookup("/").unwrap(), ROOT_INO);
    assert_eq!(fs.stat_ino(f).unwrap().parent, c);
    assert_eq!(fs.path_of(f).unwrap(), b"/a/b/c/file.txt");
    assert_eq!(fs.path_of(ROOT_INO).unwrap(), b"/");
    assert_clean(&mut fs);
}

#[test]
fn mkdir_needs_an_existing_parent() {
    let mut fs = fresh(2);
    assert_eq!(fs.mkdir("/x/y", 0), Err(FsError::NotFound));
    fs.create("/file", 0).unwrap();
    assert_eq!(fs.mkdir("/file/y", 0), Err(FsError::NotDir));
    assert_eq!(fs.create("/file/y", 0), Err(FsError::NotDir));
    assert_clean(&mut fs);
}

#[test]
fn lookup_errors() {
    let mut fs = fresh(2);
    fs.mkdir("/d", 0).unwrap();
    fs.create("/d/f", 0).unwrap();
    assert_eq!(fs.lookup("/nope"), Err(FsError::NotFound));
    assert_eq!(fs.lookup("/d/nope"), Err(FsError::NotFound));
    assert_eq!(fs.lookup("/d/f/x"), Err(FsError::NotDir));
    assert_eq!(fs.open("/d"), Err(FsError::IsDir));
    assert!(fs.open("/d/f").is_ok());
}

#[test]
fn invalid_paths_are_rejected() {
    let mut fs = fresh(2);
    fs.mkdir("/a", 0).unwrap();
    for bad in [
        "", "a", "a/b", "//", "//a", "/a//b", "/a/", "/a/./b", "/a/../b", "/..", "/.", "/a/..",
    ] {
        assert_eq!(fs.lookup(bad), Err(FsError::InvalidPath), "lookup {bad:?}");
        assert!(fs.create(bad, 0).is_err(), "create {bad:?}");
        assert!(fs.mkdir(bad, 0).is_err(), "mkdir {bad:?}");
        assert!(fs.remove(bad).is_err(), "remove {bad:?}");
    }
    // The root itself cannot be created, removed or renamed.
    assert_eq!(fs.create("/", 0), Err(FsError::InvalidPath));
    assert_eq!(fs.remove("/"), Err(FsError::InvalidPath));
    assert_eq!(fs.rmdir("/"), Err(FsError::InvalidPath));
    assert_eq!(fs.rename("/", "/x", 0), Err(FsError::InvalidPath));
    assert_eq!(fs.rename("/a", "/", 0), Err(FsError::InvalidPath));
    assert_clean(&mut fs);
}

#[test]
fn nul_in_a_path_is_an_invalid_name() {
    let mut fs = fresh(2);
    assert_eq!(fs.create(b"/a\0b", 0), Err(FsError::InvalidName));
    assert_eq!(fs.lookup(b"/a\0b"), Err(FsError::InvalidName));
}

#[test]
fn name_length_limit_is_255() {
    let mut fs = fresh(2);
    let ok = alloc::format!("/{}", "n".repeat(255));
    let long = alloc::format!("/{}", "n".repeat(256));
    fs.create(&ok, 0).unwrap();
    assert_eq!(fs.create(&long, 0), Err(FsError::NameTooLong));
    assert_eq!(fs.mkdir(&long, 0), Err(FsError::NameTooLong));
    assert_eq!(fs.lookup(&long), Err(FsError::NameTooLong));
    assert_eq!(fs.lookup(&ok).unwrap(), fs.open(&ok).unwrap());
    let entries = fs.readdir("/").unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name.len(), 255);
    assert_clean(&mut fs);
}

#[test]
fn non_utf8_and_spaces_in_names_work() {
    let mut fs = fresh(2);
    fs.create(b"/\xff\xfe name", 0).unwrap();
    fs.create("/ção ação.txt", 0).unwrap();
    assert!(fs.lookup(b"/\xff\xfe name").is_ok());
    assert!(fs.lookup("/ção ação.txt").is_ok());
    assert_clean(&mut fs);
}

#[test]
fn check_name_rules() {
    use super::super::ops::check_name;
    assert_eq!(check_name(b""), Err(FsError::InvalidName));
    assert_eq!(check_name(b"."), Err(FsError::InvalidName));
    assert_eq!(check_name(b".."), Err(FsError::InvalidName));
    assert_eq!(check_name(b"a/b"), Err(FsError::InvalidName));
    assert_eq!(check_name(b"a\0"), Err(FsError::InvalidName));
    assert_eq!(check_name(&[b'x'; 256]), Err(FsError::NameTooLong));
    assert_eq!(check_name(&[b'x'; 255]), Ok(()));
    assert_eq!(check_name(b"..."), Ok(()));
    assert_eq!(check_name(b".hidden"), Ok(()));
}

#[test]
fn readdir_lists_everything_and_reports_size_and_kind() {
    let mut fs = fresh(2);
    fs.mkdir("/d", 1).unwrap();
    fs.write_file("/f", b"12345", 2).unwrap();
    let mut e = fs.readdir("/").unwrap();
    e.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(e.len(), 2);
    assert_eq!((e[0].name.as_slice(), e[0].kind), (&b"d"[..], Kind::Dir));
    assert_eq!(
        (e[1].name.as_slice(), e[1].kind, e[1].size),
        (&b"f"[..], Kind::File, 5)
    );
    assert_eq!(e[1].mtime, 2);
    assert_eq!(fs.readdir("/f").unwrap_err(), FsError::NotDir);
    assert_eq!(fs.readdir("/zz").unwrap_err(), FsError::NotFound);
}

#[test]
fn readdir_hides_the_trash_directory_at_the_root() {
    let mut fs = fresh(2);
    assert!(fs.readdir("/").unwrap().is_empty());
    assert!(fs.lookup("/.trash").is_ok());
    assert_eq!(fs.lookup("/.trash").unwrap(), TRASH_INO);
}

#[test]
fn stat_reports_times_mode_and_kind() {
    let mut fs = fresh(2);
    fs.mkdir("/d", 100).unwrap();
    let f = fs.create("/d/f", 200).unwrap();
    fs.write_at(f, 0, b"abc", 300).unwrap();
    let s = fs.stat("/d/f").unwrap();
    assert_eq!(s.kind, Kind::File);
    assert_eq!((s.ctime, s.mtime, s.size), (200, 300, 3));
    assert_eq!(s.mode, 0o644);
    assert_eq!(s.uid, 0);
    assert_eq!(s.nlink, 1);
    assert_eq!(s.blocks, 1);
    let d = fs.stat("/d").unwrap();
    assert_eq!(d.kind, Kind::Dir);
    assert_eq!(d.mode, 0o755);
    assert_eq!(d.ctime, 100);
    assert_eq!(d.mtime, 200, "directory mtime follows entry changes");
    assert_eq!(fs.stat("/nope"), Err(FsError::NotFound));
    assert_eq!(fs.stat_ino(0), Err(FsError::NotFound));
    assert_eq!(fs.stat_ino(999_999), Err(FsError::NotFound));
}

#[test]
fn empty_files_use_no_blocks() {
    let mut fs = fresh(2);
    let before = fs.statfs().free_blocks;
    fs.create("/e", 0).unwrap();
    assert_eq!(fs.stat("/e").unwrap().blocks, 0);
    // The root directory block (holding .trash) has room for the entry.
    assert_eq!(fs.statfs().free_blocks, before);
}

#[test]
fn remove_file_frees_space_and_inode() {
    let mut fs = fresh(2);
    let s0 = fs.statfs();
    fs.write_file("/f", &[1u8; 10_000], 0).unwrap();
    assert!(fs.statfs().free_blocks < s0.free_blocks);
    assert_eq!(fs.statfs().free_inodes, s0.free_inodes - 1);
    fs.remove("/f").unwrap();
    let s1 = fs.statfs();
    assert_eq!(s1.free_inodes, s0.free_inodes);
    // The directory block holding the entry was returned too.
    assert_eq!(s1.free_blocks, s0.free_blocks);
    assert_eq!(fs.lookup("/f"), Err(FsError::NotFound));
    assert_eq!(fs.remove("/f"), Err(FsError::NotFound));
    assert_clean(&mut fs);
}

#[test]
fn remove_refuses_directories_and_rmdir_refuses_files() {
    let mut fs = fresh(2);
    fs.mkdir("/d", 0).unwrap();
    fs.create("/f", 0).unwrap();
    assert_eq!(fs.remove("/d"), Err(FsError::IsDir));
    assert_eq!(fs.rmdir("/f"), Err(FsError::NotDir));
    assert_eq!(fs.rmdir("/nope"), Err(FsError::NotFound));
    assert_clean(&mut fs);
}

#[test]
fn rmdir_requires_empty() {
    let mut fs = fresh(2);
    fs.mkdir("/d", 0).unwrap();
    fs.create("/d/f", 0).unwrap();
    assert_eq!(fs.rmdir("/d"), Err(FsError::NotEmpty));
    fs.remove("/d/f").unwrap();
    fs.rmdir("/d").unwrap();
    assert_eq!(fs.lookup("/d"), Err(FsError::NotFound));
    assert_clean(&mut fs);
}

#[test]
fn remove_all_deletes_a_whole_tree() {
    let mut fs = fresh(4);
    let s0 = fs.statfs();
    fs.mkdir("/t", 0).unwrap();
    for d in 0..5 {
        fs.mkdir(&alloc::format!("/t/d{d}"), 0).unwrap();
        for f in 0..6 {
            fs.write_file(&alloc::format!("/t/d{d}/f{f}"), &[d as u8; 5000], 0)
                .unwrap();
        }
        fs.mkdir(&alloc::format!("/t/d{d}/sub"), 0).unwrap();
        fs.write_file(&alloc::format!("/t/d{d}/sub/deep"), b"x", 0)
            .unwrap();
    }
    fs.remove_all("/t").unwrap();
    assert!(fs.readdir("/").unwrap().is_empty());
    assert_eq!(fs.statfs().free_blocks, s0.free_blocks);
    assert_eq!(fs.statfs().free_inodes, s0.free_inodes);
    assert_clean(&mut fs);
    assert_eq!(fs.remove_all("/t"), Err(FsError::NotFound));
}

#[test]
fn remove_all_on_a_plain_file() {
    let mut fs = fresh(2);
    fs.write_file("/f", b"hi", 0).unwrap();
    fs.remove_all("/f").unwrap();
    assert_eq!(fs.lookup("/f"), Err(FsError::NotFound));
}

#[test]
fn rename_file_within_and_across_directories() {
    let mut fs = fresh(2);
    fs.mkdir("/a", 0).unwrap();
    fs.mkdir("/b", 0).unwrap();
    fs.write_file("/a/f", b"data", 0).unwrap();
    let ino = fs.lookup("/a/f").unwrap();
    fs.rename("/a/f", "/a/g", 9).unwrap();
    assert_eq!(fs.lookup("/a/f"), Err(FsError::NotFound));
    assert_eq!(fs.lookup("/a/g").unwrap(), ino);
    fs.rename("/a/g", "/b/h", 10).unwrap();
    assert_eq!(fs.lookup("/b/h").unwrap(), ino);
    assert_eq!(fs.stat_ino(ino).unwrap().parent, fs.lookup("/b").unwrap());
    assert_eq!(fs.read_file("/b/h").unwrap(), b"data");
    assert_clean(&mut fs);
}

#[test]
fn rename_directory_moves_its_subtree() {
    let mut fs = fresh(2);
    fs.mkdir("/a", 0).unwrap();
    fs.mkdir("/a/b", 0).unwrap();
    fs.write_file("/a/b/f", b"x", 0).unwrap();
    fs.mkdir("/dst", 0).unwrap();
    fs.rename("/a", "/dst/moved", 5).unwrap();
    assert_eq!(fs.read_file("/dst/moved/b/f").unwrap(), b"x");
    assert_eq!(fs.lookup("/a"), Err(FsError::NotFound));
    assert_clean(&mut fs);
}

#[test]
fn rename_onto_existing_is_refused_and_changes_nothing() {
    let mut fs = fresh(2);
    fs.write_file("/a", b"A", 0).unwrap();
    fs.write_file("/b", b"B", 0).unwrap();
    fs.mkdir("/d", 0).unwrap();
    assert_eq!(fs.rename("/a", "/b", 0), Err(FsError::Exists));
    assert_eq!(fs.rename("/a", "/d", 0), Err(FsError::Exists));
    assert_eq!(fs.rename("/d", "/a", 0), Err(FsError::Exists));
    assert_eq!(fs.read_file("/a").unwrap(), b"A");
    assert_eq!(fs.read_file("/b").unwrap(), b"B");
    assert_clean(&mut fs);
}

#[test]
fn rename_to_itself_is_a_noop_and_missing_source_fails() {
    let mut fs = fresh(2);
    fs.write_file("/a", b"A", 0).unwrap();
    fs.rename("/a", "/a", 0).unwrap();
    assert_eq!(fs.read_file("/a").unwrap(), b"A");
    assert_eq!(fs.rename("/zz", "/y", 0), Err(FsError::NotFound));
    assert_eq!(fs.rename("/a", "/nodir/y", 0), Err(FsError::NotFound));
    fs.create("/f", 0).unwrap();
    assert_eq!(fs.rename("/a", "/f/y", 0), Err(FsError::NotDir));
}

#[test]
fn directory_cannot_move_into_itself() {
    let mut fs = fresh(2);
    fs.mkdir("/a", 0).unwrap();
    fs.mkdir("/a/b", 0).unwrap();
    fs.mkdir("/a/b/c", 0).unwrap();
    assert_eq!(fs.rename("/a", "/a/x", 0), Err(FsError::InvalidMove));
    assert_eq!(fs.rename("/a", "/a/b/c/x", 0), Err(FsError::InvalidMove));
    assert_eq!(fs.rename("/a/b", "/a/b/c/x", 0), Err(FsError::InvalidMove));
    fs.rename("/a/b/c", "/c", 0).unwrap(); // moving up is fine
    assert_clean(&mut fs);
}

#[test]
fn rename_keeps_a_file_readable_and_its_times() {
    let mut fs = fresh(2);
    fs.write_file("/a", b"keep", 10).unwrap();
    fs.rename("/a", "/b", 77).unwrap();
    let s = fs.stat("/b").unwrap();
    assert_eq!((s.ctime, s.mtime), (10, 10), "ctime is the creation time");
    assert_eq!(fs.stat("/").unwrap().mtime, 77, "the directory changed");
    assert_eq!(fs.read_file("/b").unwrap(), b"keep");
}

#[test]
fn rename_freeing_the_last_dir_block_shrinks_it() {
    let mut fs = fresh(2);
    fs.mkdir("/a", 0).unwrap();
    fs.mkdir("/b", 0).unwrap();
    fs.create("/a/x", 0).unwrap();
    let free = fs.statfs().free_blocks;
    fs.rename("/a/x", "/b/x", 0).unwrap();
    // /a lost its only block, /b gained one.
    assert_eq!(fs.statfs().free_blocks, free);
    assert_eq!(fs.stat("/a").unwrap().blocks, 0);
    assert_eq!(fs.stat("/b").unwrap().blocks, 1);
    assert_clean(&mut fs);
}

#[test]
fn trash_moves_to_dot_trash_and_hides_the_file() {
    let mut fs = fresh(2);
    fs.write_file("/a.txt", b"hi", 5).unwrap();
    fs.trash("/a.txt", 50).unwrap();
    assert_eq!(fs.lookup("/a.txt"), Err(FsError::NotFound));
    assert_eq!(fs.read_file("/.trash/a.txt").unwrap(), b"hi");
    let t = fs.trash_list().unwrap();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].trash_name, b"a.txt");
    assert_eq!(t[0].orig_name, b"a.txt");
    assert_eq!(t[0].orig_parent, ROOT_INO);
    assert_eq!((t[0].kind, t[0].size, t[0].deleted_at), (Kind::File, 2, 50));
    assert_clean(&mut fs);
}

#[test]
fn trash_restore_puts_it_back_with_the_same_inode() {
    let mut fs = fresh(2);
    fs.mkdir("/docs", 0).unwrap();
    fs.write_file("/docs/a.txt", b"hi", 5).unwrap();
    let ino = fs.lookup("/docs/a.txt").unwrap();
    fs.trash("/docs/a.txt", 50).unwrap();
    let p = fs.trash_restore(b"a.txt", 60).unwrap();
    assert_eq!(p, b"/docs/a.txt");
    assert_eq!(fs.lookup("/docs/a.txt").unwrap(), ino);
    assert!(fs.trash_list().unwrap().is_empty());
    assert_eq!(fs.read_file("/docs/a.txt").unwrap(), b"hi");
    assert_clean(&mut fs);
}

#[test]
fn trash_directory_takes_the_subtree_and_restore_returns_it() {
    let mut fs = fresh(2);
    fs.mkdir("/proj", 0).unwrap();
    fs.mkdir("/proj/src", 0).unwrap();
    fs.write_file("/proj/src/main.rs", b"fn main(){}", 0)
        .unwrap();
    fs.trash("/proj", 9).unwrap();
    assert_eq!(fs.lookup("/proj"), Err(FsError::NotFound));
    assert_eq!(
        fs.read_file("/.trash/proj/src/main.rs").unwrap(),
        b"fn main(){}"
    );
    assert_clean(&mut fs);
    assert_eq!(fs.trash_restore(b"proj", 10).unwrap(), b"/proj");
    assert_eq!(fs.read_file("/proj/src/main.rs").unwrap(), b"fn main(){}");
    assert_clean(&mut fs);
}

#[test]
fn trash_name_collisions_get_a_suffix_and_restore_the_right_name() {
    let mut fs = fresh(2);
    fs.write_file("/a.txt", b"one", 1).unwrap();
    fs.trash("/a.txt", 1).unwrap();
    fs.write_file("/a.txt", b"two", 2).unwrap();
    fs.trash("/a.txt", 2).unwrap();
    fs.write_file("/a.txt", b"three", 3).unwrap();
    fs.trash("/a.txt", 3).unwrap();
    let mut t = fs.trash_list().unwrap();
    t.sort_by(|x, y| x.trash_name.cmp(&y.trash_name));
    let tn: Vec<&[u8]> = t.iter().map(|e| e.trash_name.as_slice()).collect();
    assert_eq!(tn, [&b"a.txt"[..], b"a.txt~2", b"a.txt~3"]);
    assert!(t.iter().all(|e| e.orig_name == b"a.txt"));
    assert_eq!(fs.trash_restore(b"a.txt~2", 9).unwrap(), b"/a.txt");
    assert_eq!(fs.read_file("/a.txt").unwrap(), b"two");
    // The name is taken now: restoring another one is refused, not overwritten.
    assert_eq!(fs.trash_restore(b"a.txt", 9), Err(FsError::Exists));
    assert_eq!(fs.read_file("/a.txt").unwrap(), b"two");
    assert_clean(&mut fs);
}

#[test]
fn long_trash_names_stay_within_255_bytes_when_suffixed() {
    let mut fs = fresh(2);
    let name = "x".repeat(255);
    for i in 0..3u64 {
        fs.create(&alloc::format!("/{name}"), i).unwrap();
        fs.trash(&alloc::format!("/{name}"), i).unwrap();
    }
    let t = fs.trash_list().unwrap();
    assert_eq!(t.len(), 3);
    assert!(
        t.iter()
            .all(|e| e.trash_name.len() <= 255 && e.orig_name.len() == 255)
    );
    assert_clean(&mut fs);
}

#[test]
fn restore_falls_back_to_root_when_origin_is_gone_or_trashed() {
    let mut fs = fresh(2);
    fs.mkdir("/d", 0).unwrap();
    fs.write_file("/d/f", b"1", 0).unwrap();
    fs.trash("/d/f", 1).unwrap();
    // Origin directory removed after the file was trashed.
    fs.rmdir("/d").unwrap();
    assert_eq!(fs.trash_restore(b"f", 2).unwrap(), b"/f");
    // Origin directory itself in the trash.
    fs.mkdir("/e", 0).unwrap();
    fs.write_file("/e/g", b"2", 0).unwrap();
    fs.trash("/e/g", 3).unwrap();
    fs.trash("/e", 4).unwrap();
    assert_eq!(fs.trash_restore(b"g", 5).unwrap(), b"/g");
    assert_eq!(fs.read_file("/g").unwrap(), b"2");
    assert_clean(&mut fs);
}

#[test]
fn trash_purge_and_empty_trash_free_everything() {
    let mut fs = fresh(4);
    let s0 = fs.statfs();
    fs.write_file("/a", &[1u8; 9000], 0).unwrap();
    fs.mkdir("/d", 0).unwrap();
    fs.write_file("/d/b", &[2u8; 9000], 0).unwrap();
    fs.trash("/a", 1).unwrap();
    fs.trash("/d", 1).unwrap();
    fs.trash_purge(b"a").unwrap();
    assert_eq!(fs.trash_list().unwrap().len(), 1);
    assert_eq!(fs.trash_purge(b"a"), Err(FsError::NotFound));
    fs.write_file("/z", b"z", 0).unwrap();
    fs.trash("/z", 2).unwrap();
    fs.empty_trash().unwrap();
    assert!(fs.trash_list().unwrap().is_empty());
    assert_eq!(fs.statfs().free_blocks, s0.free_blocks);
    assert_eq!(fs.statfs().free_inodes, s0.free_inodes);
    assert_clean(&mut fs);
    fs.empty_trash().unwrap(); // idempotent
}

#[test]
fn trash_of_missing_path_and_double_trash() {
    let mut fs = fresh(2);
    assert_eq!(fs.trash("/nope", 0), Err(FsError::NotFound));
    fs.create("/f", 0).unwrap();
    fs.trash("/f", 0).unwrap();
    assert_eq!(fs.trash("/f", 0), Err(FsError::NotFound));
    assert_eq!(fs.trash_restore(b"missing", 0), Err(FsError::NotFound));
}

#[test]
fn dot_trash_is_protected() {
    let mut fs = fresh(2);
    fs.write_file("/f", b"x", 0).unwrap();
    fs.trash("/f", 0).unwrap();
    assert_eq!(fs.create("/.trash", 0), Err(FsError::Reserved));
    assert_eq!(fs.mkdir("/.trash", 0), Err(FsError::Reserved));
    assert_eq!(fs.remove("/.trash"), Err(FsError::Reserved));
    assert_eq!(fs.rmdir("/.trash"), Err(FsError::Reserved));
    assert_eq!(fs.remove_all("/.trash"), Err(FsError::Reserved));
    assert_eq!(fs.trash("/.trash", 0), Err(FsError::Reserved));
    assert_eq!(fs.rename("/.trash", "/x", 0), Err(FsError::Reserved));
    fs.create("/g", 0).unwrap();
    assert_eq!(fs.rename("/g", "/.trash", 0), Err(FsError::Reserved));
    assert_eq!(fs.rename("/g", "/.trash/g", 0), Err(FsError::Reserved));
    assert_eq!(fs.rename("/.trash/f", "/f", 0), Err(FsError::Reserved));
    assert_eq!(fs.create("/.trash/new", 0), Err(FsError::Reserved));
    assert_eq!(fs.remove("/.trash/f"), Err(FsError::Reserved));
    assert_eq!(fs.write_file("/.trash/f", b"y", 0), Err(FsError::Reserved));
    assert_eq!(fs.trash("/.trash/f", 0), Err(FsError::Reserved));
    assert_eq!(fs.read_file("/.trash/f").unwrap(), b"x"); // reading is fine
    assert_clean(&mut fs);
}

#[test]
fn a_directory_named_dot_trash_below_the_root_is_allowed() {
    let mut fs = fresh(2);
    fs.mkdir("/d", 0).unwrap();
    fs.mkdir("/d/.trash", 0).unwrap();
    fs.create("/d/.trash/x", 0).unwrap();
    assert_clean(&mut fs);
}

#[test]
fn statfs_accounts_blocks_and_inodes() {
    let mut fs = fresh(2);
    let s = fs.statfs();
    assert_eq!(s.block_size, 4096);
    assert_eq!(s.total_blocks, ((2 * 2048 - 128) / 8) as u32);
    assert!(s.free_blocks < s.total_blocks && s.free_blocks <= s.data_blocks);
    assert!(s.free_bytes() <= s.data_bytes());
    assert_eq!(s.total_inodes - s.free_inodes, 2); // root + trash
    fs.write_file("/f", &[0xAB; 4096 * 3], 0).unwrap();
    let s2 = fs.statfs();
    // 3 data blocks (the root directory block already has room), 1 more inode.
    assert_eq!(s.free_blocks - s2.free_blocks, 3);
    assert_eq!(s.free_inodes - s2.free_inodes, 1);
}

#[test]
fn many_small_files_in_nested_directories() {
    let mut fs = fresh(4);
    for d in 0..8 {
        fs.mkdir(&alloc::format!("/d{d}"), 0).unwrap();
        for f in 0..20 {
            fs.write_file(
                &alloc::format!("/d{d}/f{f}"),
                alloc::format!("{d}-{f}").as_bytes(),
                0,
            )
            .unwrap();
        }
    }
    for d in 0..8 {
        for f in 0..20 {
            assert_eq!(
                fs.read_file(&alloc::format!("/d{d}/f{f}")).unwrap(),
                alloc::format!("{d}-{f}").as_bytes()
            );
        }
    }
    assert_clean(&mut fs);
}

#[test]
fn no_inodes_left_is_reported_and_harmless() {
    let mut fs = fresh_with_inodes(2, 16); // 2 used by root/trash, 14 free
    for i in 0..14 {
        fs.create(&alloc::format!("/f{i}"), 0).unwrap();
    }
    assert_eq!(fs.create("/one-too-many", 0), Err(FsError::NoInodes));
    assert_eq!(fs.mkdir("/dir", 0), Err(FsError::NoInodes));
    assert_clean(&mut fs);
    fs.remove("/f3").unwrap();
    fs.create("/again", 0).unwrap();
    assert_clean(&mut fs);
}
