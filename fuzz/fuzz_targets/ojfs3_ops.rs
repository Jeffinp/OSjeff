//! Fuzz target: sequences of OJFS v3 operations, with optional power loss.
//!
//! A fresh filesystem on a 1 or 2 MiB RAM disk runs a fuzzed operation
//! sequence. Invariants checked:
//!
//! * a successful `write_file` reads back exactly what was written;
//! * `fsck` is clean after the sequence;
//! * unmounting and mounting again changes nothing visible;
//! * with a power cut (`cut`, optionally with a volatile cache) at an
//!   arbitrary event, the surviving disk mounts, `fsck` is clean and the
//!   tree is exactly the state before or after the interrupted operation.
#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use osjeff_core::blockdev::{BlockDevice, CrashMode, FaultyDisk, RamDisk};
use osjeff_core::fs3::{FormatOptions, Fs3, Kind};
use std::collections::BTreeMap;

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
    Remount,
}

#[derive(Arbitrary, Debug)]
struct Input {
    ops: Vec<Op>,
    two_mib: bool,
    /// Cut the power at this event (scaled); `None` = healthy run.
    cut: Option<u16>,
    /// `Some(seed)`: volatile-cache crash model.
    lossy: Option<u8>,
}

fn path(raw: &[u8]) -> Vec<u8> {
    const A: &[u8] = b"//aabbcd.";
    raw.iter().take(24).map(|&b| A[b as usize % A.len()]).collect()
}

type Snap = BTreeMap<Vec<u8>, Option<Vec<u8>>>;

fn snapshot<D: BlockDevice>(fs: &mut Fs3<D>) -> Snap {
    let mut out = Snap::new();
    let mut stack = vec![b"/".to_vec(), b"/.trash".to_vec()];
    out.insert(b"/.trash".to_vec(), None);
    while let Some(dir) = stack.pop() {
        for e in fs.readdir(&dir).expect("readdir of a live directory") {
            let mut p = dir.clone();
            if p != b"/" {
                p.push(b'/');
            }
            p.extend_from_slice(&e.name);
            match e.kind {
                Kind::Dir => {
                    out.insert(p.clone(), None);
                    stack.push(p);
                }
                Kind::File => {
                    let d = fs.read_file(&p).expect("read of a live file");
                    out.insert(p, Some(d));
                }
            }
        }
    }
    out
}

/// Returns `Err(())` if the operation failed with an I/O error (power cut).
fn apply<D: BlockDevice>(fs: &mut Fs3<D>, op: &Op, now: u64) -> Result<(), ()> {
    use osjeff_core::fs3::FsError;
    let io = |r: Result<(), FsError>| match r {
        Err(FsError::Io(_)) | Err(FsError::Poisoned) => Err(()),
        _ => Ok(()),
    };
    match op {
        Op::Create(p) => io(fs.create(&path(p), now).map(|_| ())),
        Op::Mkdir(p) => io(fs.mkdir(&path(p), now).map(|_| ())),
        Op::Write(p, d) => {
            let d = &d[..d.len().min(20_000)];
            let p = path(p);
            let r = fs.write_file(&p, d, now);
            if r.is_ok() {
                assert_eq!(fs.read_file(&p).as_deref(), Ok(d), "write_file did not stick");
            }
            io(r)
        }
        Op::WriteAt(p, off, d) => match fs.lookup(&path(p)) {
            Ok(i) => io(fs.write_at(i, *off as u64 * 3, &d[..d.len().min(20_000)], now)),
            Err(e) => io(Err(e)),
        },
        Op::Append(p, d) => match fs.lookup(&path(p)) {
            Ok(i) => io(fs.append(i, &d[..d.len().min(20_000)], now)),
            Err(e) => io(Err(e)),
        },
        Op::Truncate(p, n) => match fs.lookup(&path(p)) {
            Ok(i) => io(fs.truncate(i, *n as u64, now)),
            Err(e) => io(Err(e)),
        },
        Op::Rename(a, b) => io(fs.rename(&path(a), &path(b), now)),
        Op::Remove(p) => io(fs.remove(&path(p))),
        Op::Rmdir(p) => io(fs.rmdir(&path(p))),
        Op::RemoveAll(p) => io(fs.remove_all(&path(p))),
        Op::Trash(p) => io(fs.trash(&path(p), now)),
        Op::Restore(n) => io(fs.trash_restore(&path(n), now).map(|_| ())),
        Op::Purge(n) => io(fs.trash_purge(&path(n))),
        Op::EmptyTrash => io(fs.empty_trash()),
        Op::Remount => Ok(()),
    }
}

fuzz_target!(|input: Input| {
    let ops: Vec<&Op> = input.ops.iter().take(24).collect();
    let sectors = if input.two_mib { 4096 } else { 2048 };
    let base = Fs3::format(RamDisk::new(sectors), &FormatOptions::new([3; 16], 1))
        .unwrap()
        .into_device()
        .into_bytes();

    match input.cut {
        None => {
            let mut fs = Fs3::mount(RamDisk::from_bytes(base)).unwrap();
            for (i, op) in ops.iter().enumerate() {
                if let Op::Remount = op {
                    let before = snapshot(&mut fs);
                    fs = Fs3::mount_verified(fs.into_device()).expect("remount");
                    assert_eq!(snapshot(&mut fs), before, "remount changed the tree");
                    continue;
                }
                let _ = apply(&mut fs, op, 10 + i as u64);
            }
            let rep = fs.fsck().unwrap();
            assert!(rep.is_clean(), "fsck: {rep:?}");
            let before = snapshot(&mut fs);
            let mut fs = Fs3::mount_verified(fs.into_device()).expect("final remount");
            assert_eq!(snapshot(&mut fs), before);
        }
        Some(cut) => {
            let mode = match input.lossy {
                Some(s) => CrashMode::Lossy(s as u64),
                None => CrashMode::InOrder,
            };
            // Oracle: state after every prefix, healthy.
            let mut oracle = Fs3::mount(RamDisk::from_bytes(base.clone())).unwrap();
            let mut snaps = vec![snapshot(&mut oracle)];
            for (i, op) in ops.iter().enumerate() {
                let _ = apply(&mut oracle, op, 10 + i as u64);
                snaps.push(snapshot(&mut oracle));
            }
            // Same sequence on a disk that loses power.
            let dev = FaultyDisk::new(RamDisk::from_bytes(base))
                .with_mode(mode)
                .crash_after(cut as u64 * 3);
            let mut fs = Fs3::mount(dev).unwrap();
            let mut failed = ops.len();
            for (i, op) in ops.iter().enumerate() {
                if apply(&mut fs, op, 10 + i as u64).is_err() {
                    failed = i;
                    break;
                }
            }
            let disk = fs.into_device().into_inner();
            let mut fs = Fs3::mount(disk).expect("mount after power loss");
            let rep = fs.fsck().unwrap();
            assert!(rep.is_clean(), "fsck after power loss: {rep:?}");
            let snap = snapshot(&mut fs);
            let before = &snaps[failed];
            let after = snaps.get(failed + 1).unwrap_or(before);
            assert!(snap == *before || snap == *after, "torn state after cut");
        }
    }
});
