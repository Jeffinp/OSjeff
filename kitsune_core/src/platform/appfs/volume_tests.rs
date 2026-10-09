//! Tests of [`VolumeFs`]: the same operations as [`MemFs`] (the oracle), the
//! backend's own defense in depth, quota and disk-full behaviour, and persistence
//! through a remount of the OJFS v3 volume.

use super::volume::MAX_FILE;
use super::*;
use crate::platform::appabi::*;
use crate::platform::appinstall;
use crate::platform::appmanifest::{FsPerm, MANIFEST_SECTION};
use crate::platform::wasmsec;
use crate::storage::blockdev::RamDisk;
use crate::storage::fs3::{FormatOptions, Fs3};
use crate::storage::vfs::Backend;
use alloc::format;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;

const NOW: u64 = 1_700_000_000;
const MIB: u64 = 1 << 20;

fn volume(mib: u64) -> Fs3<RamDisk> {
    Fs3::format(
        RamDisk::new(mib * 2048),
        &FormatOptions::new(*b"volume-fs-tests!", NOW),
    )
    .unwrap()
}

/// Unmount and mount again from the same device: what a reboot does.
fn remount(fs: Fs3<RamDisk>) -> Fs3<RamDisk> {
    let mut fs = fs;
    fs.sync().unwrap();
    Fs3::mount(fs.into_device()).unwrap()
}

fn sb(perm: FsPerm, id: &str, quota: u64) -> Sandbox {
    Sandbox::new(perm, id, quota, 8).unwrap()
}

fn put(s: &mut Sandbox, fs: &mut dyn AppFs, path: &str, data: &[u8]) {
    let fd = s
        .open(fs, path.as_bytes(), O_WRITE | O_CREATE | O_TRUNC)
        .unwrap();
    let mut off = 0;
    while off < data.len() {
        let n = s.write(fs, fd, &data[off..]).unwrap();
        assert!(n > 0);
        off += n;
    }
    s.close(fd).unwrap();
}

fn get(s: &mut Sandbox, fs: &mut dyn AppFs, path: &str) -> Vec<u8> {
    let fd = s.open(fs, path.as_bytes(), O_READ).unwrap();
    let mut out = Vec::new();
    let mut b = [0u8; 97];
    loop {
        let n = s.read(fs, fd, &mut b).unwrap();
        if n == 0 {
            break;
        }
        out.extend_from_slice(&b[..n]);
    }
    s.close(fd).unwrap();
    out
}

// ------------------------------------------------------------ parity with MemFs

/// One scripted run of operations; every result goes into the returned trace.
fn script(fs: &mut dyn AppFs) -> Vec<String> {
    let mut t: Vec<String> = Vec::new();
    macro_rules! rec {
        ($e:expr) => {
            t.push(format!("{} => {:?}", stringify!($e), $e))
        };
    }
    rec!(fs.mkdir_all("/data/a/b"));
    rec!(fs.mkdir("/data/a"));
    rec!(fs.mkdir("/data/zz/q"));
    rec!(fs.create("/data/a/f.txt"));
    rec!(fs.create("/data/a/f.txt"));
    rec!(fs.create("/data/a/f.txt/x"));
    rec!(fs.create("/data/nope/f"));
    rec!(fs.write_at("/data/a/f.txt", 0, b"hello"));
    rec!(fs.write_at("/data/a/f.txt", 8, b"xyz"));
    rec!(fs.write_at("/data/a/f.txt", 0, b""));
    rec!(fs.write_at("/data/a/missing", 0, b"x"));
    rec!(fs.write_at("/data/a", 0, b"x"));
    rec!(fs.write_at("/data/a", 0, b""));
    rec!(fs.stat("/data/a/f.txt"));
    rec!(fs.stat("/data/a"));
    rec!(fs.stat("/data/missing"));
    let mut buf = [0xAAu8; 16];
    rec!(fs.read_at("/data/a/f.txt", 0, &mut buf));
    t.push(format!("{:?}", &buf[..11]));
    rec!(fs.read_at("/data/a/f.txt", 9, &mut buf[..5]));
    rec!(fs.read_at("/data/a/f.txt", 11, &mut buf));
    rec!(fs.read_at("/data/a/f.txt", 1 << 40, &mut buf));
    rec!(fs.read_at("/data/a", 0, &mut buf));
    rec!(fs.read_at("/data/a/missing", 0, &mut buf));
    rec!(fs.set_len("/data/a/f.txt", 4));
    rec!(fs.stat("/data/a/f.txt"));
    rec!(fs.set_len("/data/a/f.txt", 9));
    rec!(fs.read_at("/data/a/f.txt", 0, &mut buf));
    t.push(format!("{:?}", &buf[..9]));
    rec!(fs.set_len("/data/a", 1));
    rec!(fs.set_len("/data/a/missing", 1));
    rec!(fs.create("/data/a/g"));
    rec!(fs.create("/data/a/Zed"));
    for i in 0..6 {
        t.push(format!("readdir {i} => {:?}", fs.read_dir("/data/a", i)));
    }
    rec!(fs.read_dir("/data/a/f.txt", 0));
    rec!(fs.read_dir("/data/missing", 0));
    rec!(fs.tree_size("/data/a"));
    rec!(fs.tree_size("/data"));
    rec!(fs.tree_size("/data/missing"));
    rec!(fs.remove("/data/a"));
    rec!(fs.remove("/data/a/b"));
    rec!(fs.remove("/data/a/b"));
    rec!(fs.remove("/data/a/g"));
    rec!(fs.rename("/data/a/Zed", "/data/a/Zed"));
    rec!(fs.rename("/data/a/Zed", "/data/a/f.txt"));
    rec!(fs.rename("/data/a/none", "/data/a/y"));
    rec!(fs.rename("/data/a", "/data/a/b/inside"));
    rec!(fs.rename("/data/a/Zed", "/data/none/y"));
    rec!(fs.rename("/data/a/Zed", "/data/a/f.txt/y"));
    rec!(fs.mkdir_all("/data/c"));
    rec!(fs.rename("/data/a", "/data/c/moved"));
    rec!(fs.stat("/data/c/moved/f.txt"));
    rec!(fs.stat("/data/a"));
    rec!(fs.tree_size("/data/c"));
    rec!(fs.mkdir_all("/home"));
    rec!(fs.mkdir_all("/apps"));
    t
}

#[test]
fn same_results_as_memfs() {
    let mut mem = MemFs::new(64 << 20);
    let want = script(&mut mem);
    let mut v = volume(8);
    let got = {
        let mut fs = VolumeFs::new(&mut v, NOW);
        script(&mut fs)
    };
    assert_eq!(got.len(), want.len());
    for (g, w) in got.iter().zip(&want) {
        assert_eq!(g, w);
    }
}

// ------------------------------------------------------ defense in depth

#[test]
fn only_the_apps_trees_are_reachable() {
    let mut v = volume(4);
    Backend::mkdir(&mut v, b"/etc", NOW).unwrap();
    Backend::write_file(&mut v, b"/etc/kitsune.conf", b"secret", NOW).unwrap();
    Backend::write_file(&mut v, b"/leiame.txt", b"hi", NOW).unwrap();
    Backend::write_file(&mut v, b"/home-ish", b"x", NOW).unwrap();
    Backend::write_file(&mut v, b"/apps2", b"x", NOW).unwrap();
    Backend::trash(&mut v, b"/apps2", NOW).unwrap();
    let mut fs = VolumeFs::new(&mut v, NOW);
    let mut buf = [0u8; 8];
    for p in [
        "/etc",
        "/etc/kitsune.conf",
        "/leiame.txt",
        "/.trash",
        "/.trash/x",
        "/home-ish",
        "/apps2",
        "/var/log/syslog.txt",
    ] {
        assert_eq!(fs.stat(p), Err(FsError::Perm), "stat {p}");
        assert_eq!(fs.read_at(p, 0, &mut buf), Err(FsError::Perm), "read {p}");
        assert_eq!(fs.write_at(p, 0, b"x"), Err(FsError::Perm), "write {p}");
        assert_eq!(fs.set_len(p, 0), Err(FsError::Perm), "set_len {p}");
        assert_eq!(fs.create(p), Err(FsError::Perm), "create {p}");
        assert_eq!(fs.mkdir(p), Err(FsError::Perm), "mkdir {p}");
        assert_eq!(fs.remove(p), Err(FsError::Perm), "remove {p}");
        assert_eq!(fs.read_dir(p, 0), Err(FsError::Perm), "read_dir {p}");
        assert_eq!(fs.tree_size(p), Err(FsError::Perm), "tree_size {p}");
        assert_eq!(fs.mkdir_all(p), Err(FsError::Perm), "mkdir_all {p}");
    }
    // The root answers `stat` only.
    assert_eq!(fs.stat("/").unwrap().kind, Kind::Dir);
    assert_eq!(fs.read_dir("/", 0), Err(FsError::Perm));
    assert_eq!(fs.remove("/"), Err(FsError::Perm));
    // Moving between an allowed tree and anything else is refused too.
    fs.mkdir_all("/data/x").unwrap();
    assert_eq!(fs.rename("/data/x", "/etc/x"), Err(FsError::Perm));
    assert_eq!(fs.rename("/data/x", "/.trash/x"), Err(FsError::Perm));
    assert_eq!(
        fs.rename("/etc/kitsune.conf", "/data/c"),
        Err(FsError::Perm)
    );
    // The platform's own top-level folders cannot be removed or renamed.
    for r in ["/apps", "/data", "/home"] {
        fs.mkdir_all(r).unwrap();
        assert_eq!(fs.remove(r), Err(FsError::Perm), "{r}");
        assert_eq!(fs.rename(r, "/data/zz"), Err(FsError::Perm), "{r}");
    }
    // None of it changed the files outside.
    assert_eq!(
        Backend::read_file(&mut v, b"/etc/kitsune.conf").unwrap(),
        b"secret"
    );
    assert_eq!(Backend::read_file(&mut v, b"/leiame.txt").unwrap(), b"hi");
}

#[test]
fn backend_refuses_non_canonical_paths_itself() {
    let mut v = volume(4);
    let mut fs = VolumeFs::new(&mut v, NOW);
    for p in [
        "data/x",
        "/data//x",
        "/data/./x",
        "/data/../etc",
        "/data/x/",
        "",
        "/data/a\\b",
        "/data/\u{e9}",
    ] {
        assert_eq!(fs.create(p), Err(FsError::Invalid), "{p:?}");
        assert_eq!(fs.stat(p), Err(FsError::Invalid), "{p:?}");
    }
}

#[test]
fn a_file_cannot_grow_past_the_cap() {
    let mut v = volume(4);
    let mut fs = VolumeFs::new(&mut v, NOW);
    fs.mkdir_all("/data/a").unwrap();
    fs.create("/data/a/f").unwrap();
    assert_eq!(fs.set_len("/data/a/f", MAX_FILE + 1), Err(FsError::NoSpace));
    assert_eq!(
        fs.write_at("/data/a/f", MAX_FILE, b"x"),
        Err(FsError::NoSpace)
    );
    assert_eq!(
        fs.write_at("/data/a/f", u64::MAX, b"xx"),
        Err(FsError::Invalid)
    );
    assert_eq!(fs.stat("/data/a/f").unwrap().size, 0);
}

#[test]
fn listing_skips_names_an_app_cannot_address() {
    let mut v = volume(4);
    Backend::mkdir(&mut v, b"/home", NOW).unwrap();
    for name in [
        "ok.txt",
        "my file.txt",
        "a\u{e7}\u{e3}o.txt", // non-ASCII, made by the file manager
        "trailing.",          // not a valid component
    ] {
        Backend::write_file(&mut v, format!("/home/{name}").as_bytes(), b"1", NOW).unwrap();
    }
    let mut fs = VolumeFs::new(&mut v, NOW);
    let mut names = Vec::new();
    for i in 0..10 {
        match fs.read_dir("/home", i).unwrap() {
            Some(e) => names.push(e.name),
            None => break,
        }
    }
    assert_eq!(names, ["my file.txt", "ok.txt"]);
    // They still count against space accounting of the tree.
    assert_eq!(fs.tree_size("/home").unwrap(), 4 * (ENTRY_OVERHEAD + 1));
}

#[test]
fn desktop_files_and_app_files_share_one_namespace() {
    let mut v = volume(4);
    {
        let mut fs = VolumeFs::new(&mut v, NOW);
        let mut s = sb(FsPerm::Home, "viewer", MIB);
        put(&mut s, &mut fs, "/from-app.txt", b"app wrote this");
    }
    // The file manager sees it at /home/from-app.txt ...
    assert_eq!(
        Backend::read_file(&mut v, b"/home/from-app.txt").unwrap(),
        b"app wrote this"
    );
    // ... and a file the user drops there is visible to the app.
    Backend::write_file(&mut v, b"/home/from-user.txt", b"user file", NOW).unwrap();
    let mut fs = VolumeFs::new(&mut v, NOW);
    let mut s = sb(FsPerm::Home, "viewer", MIB);
    assert_eq!(get(&mut s, &mut fs, "/from-user.txt"), b"user file");
}

// ------------------------------------------------------------ persistence

#[test]
fn app_data_survives_a_remount() {
    let mut v = volume(8);
    let big: Vec<u8> = (0..300_000u32)
        .map(|i| i.wrapping_mul(2_654_435_761).to_le_bytes()[2])
        .collect();
    {
        let mut fs = VolumeFs::new(&mut v, NOW);
        let mut s = sb(FsPerm::Own, "notes", MIB);
        put(&mut s, &mut fs, "/hello.txt", b"hello from run 1");
        s.mkdir(&mut fs, b"/sub").unwrap();
        put(&mut s, &mut fs, "/sub/big.bin", &big);
        assert!(s.used() >= big.len() as u64);
    }
    let mut v = remount(v);
    // A new "boot": a new sandbox object (the old one died with its app).
    let mut fs = VolumeFs::new(&mut v, NOW + 100);
    let mut s = sb(FsPerm::Own, "notes", MIB);
    assert_eq!(get(&mut s, &mut fs, "/hello.txt"), b"hello from run 1");
    assert_eq!(get(&mut s, &mut fs, "/sub/big.bin"), big);
    // and a second app with its own tree sees none of it
    let mut other = sb(FsPerm::Own, "other", MIB);
    assert_eq!(
        other.open(&mut fs, b"/hello.txt", O_READ),
        Err(FsError::NotFound)
    );
    assert_eq!(
        other.open(&mut fs, b"../notes/hello.txt", O_READ),
        Err(FsError::Perm)
    );
    let rep = v.fsck().unwrap();
    assert!(rep.is_clean(), "{:?}", rep.issues);
}

#[test]
fn home_files_survive_a_remount_and_stay_visible_to_the_desktop() {
    let mut v = volume(4);
    {
        let mut fs = VolumeFs::new(&mut v, NOW);
        let mut s = sb(FsPerm::Home, "paint", MIB);
        put(&mut s, &mut fs, "/picture.txt", b"pixels");
    }
    let mut v = remount(v);
    assert_eq!(
        Backend::read_file(&mut v, b"/home/picture.txt").unwrap(),
        b"pixels"
    );
}

/// A minimal valid package (header + manifest section).
fn package(id: &str, extra: &str) -> Vec<u8> {
    let manifest = format!("id={id}\nname={id}\nversion=1.0.0\n{extra}");
    let mut payload = Vec::new();
    payload.push(MANIFEST_SECTION.len() as u8);
    payload.extend_from_slice(MANIFEST_SECTION.as_bytes());
    payload.extend_from_slice(manifest.as_bytes());
    let mut w = wasmsec::HEADER.to_vec();
    w.push(0);
    // LEB128 of the payload length (< 16384 here)
    let n = payload.len();
    if n < 128 {
        w.push(n as u8);
    } else {
        w.push((n & 0x7F) as u8 | 0x80);
        w.push((n >> 7) as u8);
    }
    w.extend_from_slice(&payload);
    w
}

#[test]
fn installed_apps_survive_a_remount() {
    let mut v = volume(8);
    let a = package("clock", "");
    let b = package("notes", "fs=own\n");
    {
        let mut fs = VolumeFs::new(&mut v, NOW);
        appinstall::install(&mut fs, &a).unwrap();
        appinstall::install(&mut fs, &b).unwrap();
        assert_eq!(
            appinstall::install(&mut fs, &a).unwrap_err(),
            appinstall::InstallError::Duplicate
        );
    }
    let mut v = remount(v);
    let mut fs = VolumeFs::new(&mut v, NOW + 5);
    assert_eq!(
        appinstall::installed_ids(&mut fs).unwrap(),
        ["clock", "notes"]
    );
    assert_eq!(appinstall::read_package(&mut fs, "notes").unwrap(), b);
    let cat = appinstall::load_catalog(&mut fs);
    assert_eq!(cat.len(), 2);
    // Removing is durable as well, and the app's data stays (as documented).
    let mut s = sb(FsPerm::Own, "notes", MIB);
    put(&mut s, &mut fs, "/n.txt", b"keep me");
    appinstall::remove(&mut fs, "notes").unwrap();
    let mut v = remount(v);
    let mut fs = VolumeFs::new(&mut v, NOW + 9);
    assert_eq!(appinstall::installed_ids(&mut fs).unwrap(), ["clock"]);
    let mut s = sb(FsPerm::Own, "notes", MIB);
    assert_eq!(get(&mut s, &mut fs, "/n.txt"), b"keep me");
}

#[test]
fn an_interrupted_install_leaves_no_half_package() {
    let mut v = volume(4);
    let a = package("clock", "");
    {
        let mut fs = VolumeFs::new(&mut v, NOW);
        appinstall::install(&mut fs, &a).unwrap();
        // The leftover of an install that lost power: a temp file, not a package.
        fs.create("/apps/.install.tmp").unwrap();
        fs.write_at("/apps/.install.tmp", 0, &a[..5]).unwrap();
    }
    let mut v = remount(v);
    let mut fs = VolumeFs::new(&mut v, NOW);
    assert_eq!(appinstall::installed_ids(&mut fs).unwrap(), ["clock"]);
    // The next install clears the leftover and works.
    appinstall::install(&mut fs, &package("calc", "")).unwrap();
    assert!(fs.stat("/apps/.install.tmp").is_err());
}

// ------------------------------------------------------------ quota and space

#[test]
fn quota_counts_what_is_already_on_disk_after_a_remount() {
    let mut v = volume(8);
    {
        let mut fs = VolumeFs::new(&mut v, NOW);
        let mut s = sb(FsPerm::Own, "q", 64 * 1024);
        put(&mut s, &mut fs, "/a", &[7u8; 40_000]);
        assert_eq!(s.used(), ENTRY_OVERHEAD + 40_000);
    }
    let mut v = remount(v);
    let mut fs = VolumeFs::new(&mut v, NOW);
    let mut s = sb(FsPerm::Own, "q", 64 * 1024);
    // First use after the "reboot": the old content is charged.
    let fd = s.open(&mut fs, b"/b", O_WRITE | O_CREATE).unwrap();
    assert_eq!(s.used(), ENTRY_OVERHEAD + 40_000 + ENTRY_OVERHEAD);
    // 64 KiB quota: only part of a 40 000-byte write still fits, then NoSpace.
    let room = 64 * 1024 - s.used();
    assert_eq!(s.write(&mut fs, fd, &[1u8; 40_000]).unwrap() as u64, room);
    assert_eq!(s.write(&mut fs, fd, &[1u8; 10]), Err(FsError::NoSpace));
    s.close(fd).unwrap();
    assert_eq!(s.used(), 64 * 1024);
    // The accounting is exact: a fresh sandbox computes the same number.
    let mut fs = VolumeFs::new(&mut v, NOW);
    assert_eq!(fs.tree_size("/data/q").unwrap(), 64 * 1024);
    let mut s2 = sb(FsPerm::Own, "q", 64 * 1024);
    s2.stat(&mut fs, b"/").unwrap();
    s2.open(&mut fs, b"/a", O_READ).unwrap();
    assert_eq!(s2.used(), 64 * 1024);
    // Deleting frees quota, which a later run can use again.
    s2.unlink(&mut fs, b"/a").unwrap();
    assert!(s2.used() < 64 * 1024 - 40_000);
    s2.close_all();
}

#[test]
fn quota_is_per_app_and_the_volume_is_not_shared_by_accident() {
    let mut v = volume(8);
    let mut fs = VolumeFs::new(&mut v, NOW);
    let mut a = sb(FsPerm::Own, "a", 4096);
    let mut b = sb(FsPerm::Own, "b", 4096);
    let fd = a.open(&mut fs, b"/f", O_WRITE | O_CREATE).unwrap();
    assert!(a.write(&mut fs, fd, &[0u8; 8000]).unwrap() < 8000);
    assert_eq!(a.write(&mut fs, fd, b"x"), Err(FsError::NoSpace));
    // `a` being full does not affect `b`.
    put(&mut b, &mut fs, "/f", &[1u8; 3000]);
    assert_eq!(get(&mut b, &mut fs, "/f").len(), 3000);
}

#[test]
fn a_full_volume_reports_nospace_and_stays_consistent() {
    // 1 MiB is the smallest v3 volume: it fills up quickly.
    let mut v = volume(1);
    let mut s = sb(FsPerm::Own, "fill", 64 * MIB); // the quota is not the limit here
    let mut fs = VolumeFs::new(&mut v, NOW);
    let mut wrote = 0u64;
    let mut hit = None;
    for i in 0..64 {
        let path = format!("/f{i}");
        let fd = match s.open(&mut fs, path.as_bytes(), O_WRITE | O_CREATE) {
            Ok(fd) => fd,
            Err(e) => {
                hit = Some(e);
                break;
            }
        };
        match s.write(&mut fs, fd, &[0x5Au8; 32 * 1024]) {
            Ok(n) => wrote += n as u64,
            Err(e) => {
                hit = Some(e);
                s.close(fd).unwrap();
                break;
            }
        }
        s.close(fd).unwrap();
    }
    assert_eq!(hit, Some(FsError::NoSpace), "wrote {wrote} bytes");
    assert!(wrote > 100 * 1024 && wrote < MIB, "{wrote}");
    // What was written reads back and the structures are intact.
    let rep = v.fsck().unwrap();
    assert!(rep.is_clean(), "{:?}", rep.issues);
    let mut fs = VolumeFs::new(&mut v, NOW);
    let mut s = sb(FsPerm::Own, "fill", 64 * MIB);
    assert_eq!(get(&mut s, &mut fs, "/f0"), vec![0x5Au8; 32 * 1024]);
    // Freeing space makes the volume usable again.
    s.unlink(&mut fs, b"/f0").unwrap();
    put(&mut s, &mut fs, "/again", &[1u8; 1000]);
}

#[test]
fn sparse_growth_does_not_use_space_but_counts_for_the_quota() {
    let mut v = volume(2);
    let free0 = v.statfs().free_bytes();
    let mut fs = VolumeFs::new(&mut v, NOW);
    let mut s = sb(FsPerm::Own, "sparse", 64 * MIB);
    let fd = s.open(&mut fs, b"/h", O_READ | O_WRITE | O_CREATE).unwrap();
    s.seek(&mut fs, fd, 20 * MIB as i64, SEEK_SET).unwrap();
    s.write(&mut fs, fd, b"end").unwrap();
    s.seek(&mut fs, fd, 5 * MIB as i64, SEEK_SET).unwrap();
    let mut b = [1u8; 4];
    assert_eq!(s.read(&mut fs, fd, &mut b).unwrap(), 4);
    assert_eq!(b, [0, 0, 0, 0]);
    s.close(fd).unwrap();
    assert!(free0 - v.statfs().free_bytes() < 64 * 1024);
}

// ------------------------------------------------------------ whole-stack walk

/// A long mixed sequence through the sandbox on both back ends, compared at the end:
/// the file tree each one holds must be identical.
#[test]
fn sandbox_sequence_leaves_the_same_tree_on_memfs_and_volume() {
    fn run(fs: &mut dyn AppFs) -> Vec<(String, u64)> {
        let mut s = sb(FsPerm::Own, "seq", 256 * 1024);
        let mut seed = 0x1234_5678u32;
        let mut next = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed >> 8
        };
        for i in 0..400u32 {
            let name = format!("/d{}/f{}", next() % 3, next() % 5);
            match next() % 6 {
                0 => {
                    let _ = s.mkdir(fs, format!("/d{}", next() % 3).as_bytes());
                }
                1 | 2 => {
                    if let Ok(fd) = s.open(fs, name.as_bytes(), O_WRITE | O_CREATE | O_APPEND) {
                        let n = (next() % 3000) as usize;
                        let _ = s.write(fs, fd, &vec![(i % 251) as u8; n]);
                        let _ = s.close(fd);
                    }
                }
                3 => {
                    let _ = s.unlink(fs, name.as_bytes());
                }
                4 => {
                    let to = format!("/d{}/g{}", next() % 3, next() % 5);
                    let _ = s.rename(fs, name.as_bytes(), to.as_bytes());
                }
                _ => {
                    let _ = s.stat(fs, name.as_bytes());
                }
            }
        }
        let mut out = Vec::new();
        let mut dirs = vec![String::from("/")];
        while let Some(d) = dirs.pop() {
            let mut i = 0;
            while let Some(e) = s.read_dir(fs, d.as_bytes(), i).unwrap() {
                let p = if d == "/" {
                    format!("/{}", e.name)
                } else {
                    format!("{d}/{}", e.name)
                };
                let st = s.stat(fs, p.as_bytes()).unwrap();
                if e.kind == Kind::Dir {
                    dirs.push(p.clone());
                }
                out.push((p, st.size));
                i += 1;
            }
        }
        out.sort();
        out.push((format!("used={}", s.used()), 0));
        out
    }
    let mut mem = MemFs::new(64 * MIB);
    let want = run(&mut mem);
    let mut v = volume(8);
    let got = {
        let mut fs = VolumeFs::new(&mut v, NOW);
        run(&mut fs)
    };
    assert_eq!(got, want);
    assert!(got.len() > 5, "the sequence should leave something behind");
    assert!(v.fsck().unwrap().is_clean());
}

#[test]
fn root_dirs_error_codes_reach_the_abi() {
    // The codes an app sees for the new refusals.
    assert_eq!(FsError::Perm.code(), ERR_PERM);
    assert_eq!(FsError::NoSpace.code(), ERR_NOSPC);
    assert_eq!(FsError::Perm.to_string(), "permission denied");
}

#[test]
fn a_removed_bundled_app_stays_removed_across_boots() {
    let mut v = volume(4);
    let (a, b) = (package("alpha", ""), package("beta", ""));
    let bundled: [&[u8]; 2] = [&a, &b];
    // Boot 1: first-boot seeding.
    {
        let mut fs = VolumeFs::new(&mut v, NOW);
        assert_eq!(appinstall::seed_once(&mut fs, &bundled), 2);
        appinstall::remove(&mut fs, "alpha").unwrap();
    }
    // Boot 2 and 3: the removal sticks, nothing is reinstalled.
    for _ in 0..2 {
        v = remount(v);
        let mut fs = VolumeFs::new(&mut v, NOW);
        assert_eq!(appinstall::seed_once(&mut fs, &bundled), 0);
        assert_eq!(appinstall::installed_ids(&mut fs).unwrap(), ["beta"]);
    }
}

#[test]
fn mutated_flags_only_calls_that_may_change_the_volume() {
    let mut v = volume(4);
    let mut fs = VolumeFs::new(&mut v, NOW);
    assert!(!fs.mutated());
    fs.stat("/").unwrap();
    assert_eq!(fs.stat("/data/x"), Err(FsError::NotFound));
    assert_eq!(fs.read_dir("/data", 0), Err(FsError::NotFound));
    assert_eq!(fs.tree_size("/data"), Err(FsError::NotFound));
    assert_eq!(
        fs.read_at("/data/x", 0, &mut [0u8; 4]),
        Err(FsError::NotFound)
    );
    assert!(!fs.mutated(), "reads and failed lookups change nothing");
    fs.mkdir_all("/data/x").unwrap();
    assert!(fs.mutated());
    for op in 0..5 {
        let mut v2 = volume(4);
        let mut f2 = VolumeFs::new(&mut v2, NOW);
        f2.mkdir_all("/data/d").unwrap();
        f2.create("/data/d/f").unwrap();
        let mut g = VolumeFs::new(&mut v2, NOW);
        match op {
            0 => drop(g.write_at("/data/d/f", 0, b"x")),
            1 => drop(g.set_len("/data/d/f", 1)),
            2 => drop(g.remove("/data/d/f")),
            3 => drop(g.rename("/data/d/f", "/data/d/g")),
            _ => drop(g.create("/data/d/h")),
        }
        assert!(g.mutated(), "op {op}");
    }
}
