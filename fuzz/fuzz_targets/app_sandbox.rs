//! Fuzz target: the app sandbox (`osjeff_core::{appfs, appnet}`).
//!
//! Two apps share one `MemFs` (seeded with a "system" file outside both roots).
//! A fuzzed sequence of operations with **raw byte paths** runs against them;
//! afterwards nothing may exist outside `/data/<id>` of the two apps (plus the
//! seed), quotas must hold, and the system file must be untouched. The URL
//! filter is exercised on the same bytes: an accepted URL must never name a
//! loopback/private IPv4 literal or a local name.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use osjeff_core::appfs::{AppFs, MemFs, Sandbox};
use osjeff_core::appmanifest::FsPerm;
use osjeff_core::appnet;

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

fuzz_target!(|ops: Vec<Op>| {
    let mut fs = MemFs::new(1 << 20);
    fs.mkdir_all("/etc").unwrap();
    fs.create("/etc/passwd").unwrap();
    fs.write_at("/etc/passwd", 0, b"root").unwrap();
    let mut a = Sandbox::new(FsPerm::Own, "aa", QUOTA, 6).unwrap();
    let mut b = Sandbox::new(FsPerm::Own, "bb", QUOTA, 6).unwrap();
    for op in ops.iter().take(256) {
        let pick = |who: &bool| if *who { 0 } else { 1 };
        match op {
            Op::Open { who, path, flags } => {
                let _ = sb(pick(who), &mut a, &mut b).open(&mut fs, path, *flags as u32);
            }
            Op::Close { who, fd } => {
                let _ = sb(pick(who), &mut a, &mut b).close(*fd as i32);
            }
            Op::Write { who, fd, data } => {
                let _ = sb(pick(who), &mut a, &mut b).write(&mut fs, *fd as i32, data);
            }
            Op::Read { who, fd, len } => {
                let mut buf = vec![0u8; (*len as usize).min(4096)];
                let _ = sb(pick(who), &mut a, &mut b).read(&mut fs, *fd as i32, &mut buf);
            }
            Op::Seek { who, fd, off, whence } => {
                let _ = sb(pick(who), &mut a, &mut b).seek(&mut fs, *fd as i32, *off, *whence as i32);
            }
            Op::Mkdir { who, path } => {
                let _ = sb(pick(who), &mut a, &mut b).mkdir(&mut fs, path);
            }
            Op::Unlink { who, path } => {
                let _ = sb(pick(who), &mut a, &mut b).unlink(&mut fs, path);
            }
            Op::Rename { who, from, to } => {
                let _ = sb(pick(who), &mut a, &mut b).rename(&mut fs, from, to);
            }
            Op::Stat { who, path } => {
                let _ = sb(pick(who), &mut a, &mut b).stat(&mut fs, path);
            }
            Op::ReadDir { who, path, index } => {
                let _ = sb(pick(who), &mut a, &mut b).read_dir(&mut fs, path, *index as usize);
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
});
