//! Fuzz target: the app sandbox (`osjeff_core::{appfs, appnet}`).
//!
//! Two apps share one file system (seeded with a "system" file outside both roots):
//! the in-memory `MemFs`, or `VolumeFs` over an OJFS v3 volume in RAM (the real
//! back end; the first input bool picks).
//! A fuzzed sequence of operations with **raw byte paths** runs against them;
//! afterwards nothing may exist outside `/data/<id>` of the two apps (plus the
//! seed), quotas must hold, and the system file must be untouched. The URL
//! filter is exercised on the same bytes: an accepted URL must never name a
//! loopback/private IPv4 literal or a local name.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use osjeff_core::appfs::{AppFs, MemFs, Sandbox, VolumeFs};
use osjeff_core::appmanifest::FsPerm;
use osjeff_core::appnet;
use osjeff_core::blockdev::RamDisk;
use osjeff_core::fs3::{FormatOptions, Fs3};
use osjeff_core::vfs::Backend;

#[derive(Arbitrary, Debug)]
enum Op {
    Open { who: bool, path: Vec<u8>, flags: u8 },
    Close { who: bool, fd: u8 },
    Write { who: bool, fd: u8, data: Vec<u8> },
    Read { who: bool, fd: u8, len: u16 },
    Seek { who: bool, fd: u8, off: i64, whence: u8 },
    Mkdir { who: bool, path: Vec<u8> },
    Unlink { who: bool, path: Vec<u8> },
    Rename { who: bool, from: Vec<u8>, to: Vec<u8> },
    Stat { who: bool, path: Vec<u8> },
    ReadDir { who: bool, path: Vec<u8>, index: u8 },
    Url(Vec<u8>),
}

const QUOTA: u64 = 32 * 1024;

fn sb<'a>(i: i32, a: &'a mut Sandbox, b: &'a mut Sandbox) -> &'a mut Sandbox {
    if i == 0 { a } else { b }
}

/// Runs the operations on `fs` (two apps, "aa" and "bb"); quotas must hold throughout.
fn run(fs: &mut dyn AppFs, ops: &[Op]) {
    let mut a = Sandbox::new(FsPerm::Own, "aa", QUOTA, 6).unwrap();
    let mut b = Sandbox::new(FsPerm::Own, "bb", QUOTA, 6).unwrap();
    for op in ops.iter().take(256) {
        let pick = |who: &bool| if *who { 0 } else { 1 };
        match op {
            Op::Open { who, path, flags } => {
                let _ = sb(pick(who), &mut a, &mut b).open(fs, path, *flags as u32);
            }
            Op::Close { who, fd } => {
                let _ = sb(pick(who), &mut a, &mut b).close(*fd as i32);
            }
            Op::Write { who, fd, data } => {
                let _ = sb(pick(who), &mut a, &mut b).write(fs, *fd as i32, data);
            }
            Op::Read { who, fd, len } => {
                let mut buf = vec![0u8; (*len as usize).min(4096)];
                let _ = sb(pick(who), &mut a, &mut b).read(fs, *fd as i32, &mut buf);
            }
            Op::Seek { who, fd, off, whence } => {
                let _ = sb(pick(who), &mut a, &mut b).seek(fs, *fd as i32, *off, *whence as i32);
            }
            Op::Mkdir { who, path } => {
                let _ = sb(pick(who), &mut a, &mut b).mkdir(fs, path);
            }
            Op::Unlink { who, path } => {
                let _ = sb(pick(who), &mut a, &mut b).unlink(fs, path);
            }
            Op::Rename { who, from, to } => {
                let _ = sb(pick(who), &mut a, &mut b).rename(fs, from, to);
            }
            Op::Stat { who, path } => {
                let _ = sb(pick(who), &mut a, &mut b).stat(fs, path);
            }
            Op::ReadDir { who, path, index } => {
                let _ = sb(pick(who), &mut a, &mut b).read_dir(fs, path, *index as usize);
            }
            Op::Url(u) => {
                if let Ok(url) = appnet::parse_url(u) {
                    assert!(appnet::host_allowed(&url.host));
                    assert!(!url.host.starts_with("localhost") || url.host.contains('.'));
                    assert!(url.path.starts_with('/'));
                }
            }
        }
        assert!(a.used() <= QUOTA && b.used() <= QUOTA);
    }
}

fn memfs_case(ops: &[Op]) {
    let mut fs = MemFs::new(1 << 20);
    fs.mkdir_all("/etc").unwrap();
    fs.create("/etc/passwd").unwrap();
    fs.write_at("/etc/passwd", 0, b"root").unwrap();
    run(&mut fs, ops);
    let names = |fs: &mut MemFs, dir: &str| -> Vec<String> {
        let mut v = Vec::new();
        let mut i = 0;
        while let Ok(Some(e)) = fs.read_dir(dir, i) {
            v.push(e.name);
            i += 1;
        }
        v
    };
    let top = names(&mut fs, "/");
    assert!(top.iter().all(|n| n == "data" || n == "etc"), "{top:?}");
    assert_eq!(names(&mut fs, "/etc"), ["passwd"]);
    assert!(names(&mut fs, "/data").iter().all(|n| n == "aa" || n == "bb"));
    let mut buf = [0u8; 8];
    assert_eq!(fs.read_at("/etc/passwd", 0, &mut buf).unwrap(), 4);
    assert_eq!(&buf[..4], b"root");
}

fn volume_case(ops: &[Op]) {
    let mut v = Fs3::format(
        RamDisk::new(2048), // 1 MiB: small enough that the fuzzer also reaches "disk full"
        &FormatOptions::new(*b"fuzz-app-sandbox", 1_700_000_000),
    )
    .unwrap();
    v.mkdir("/etc", 1).unwrap();
    v.write_file("/etc/passwd", b"root", 1).unwrap();
    v.write_file("/notes.txt", b"user file", 1).unwrap();
    {
        let mut fs = VolumeFs::new(&mut v, 2);
        run(&mut fs, ops);
    }
    let names = |v: &mut Fs3<RamDisk>, dir: &str| -> Vec<String> {
        Backend::readdir(v, dir.as_bytes())
            .unwrap_or_default()
            .into_iter()
            .map(|e| String::from_utf8_lossy(&e.name).into_owned())
            .collect()
    };
    let top = names(&mut v, "/");
    assert!(
        top.iter().all(|n| matches!(n.as_str(), "data" | "etc" | "notes.txt" | "apps" | "home")),
        "{top:?}"
    );
    assert_eq!(names(&mut v, "/etc"), ["passwd"]);
    assert!(names(&mut v, "/data").iter().all(|n| n == "aa" || n == "bb"));
    assert_eq!(v.read_file("/etc/passwd").unwrap(), b"root");
    assert_eq!(v.read_file("/notes.txt").unwrap(), b"user file");
    let rep = v.fsck().unwrap();
    assert!(rep.is_clean(), "{:?}", rep.issues);
}

fuzz_target!(|inp: (bool, Vec<Op>)| {
    let (volume, ops) = inp;
    if volume {
        volume_case(&ops);
    } else {
        memfs_case(&ops);
    }
});
