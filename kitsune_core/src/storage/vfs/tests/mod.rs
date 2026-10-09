//! Tests of the desktop VFS layer, on an OJFS v3 volume in RAM.

use super::*;
use crate::storage::blockdev::RamDisk;
use crate::storage::fs3::FormatOptions;
use alloc::string::String;
use alloc::vec;

const NOW: u64 = 1_700_000_000;

fn fresh(mib: u64) -> Fs3<RamDisk> {
    Fs3::format(
        RamDisk::new(mib * 2048),
        &FormatOptions::new(*b"0123456789abcdef", NOW),
    )
    .unwrap()
}

fn pat(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u32).wrapping_mul(2_654_435_761).to_le_bytes()[2] ^ seed)
        .collect()
}

fn run(job: &mut CopyJob, fs: &mut Fs3<RamDisk>, budget: usize) -> usize {
    let mut steps = 0;
    loop {
        steps += 1;
        match job.step(fs, budget, NOW + 1).unwrap() {
            Progress::Done => return steps,
            Progress::Running => assert!(steps < 100_000, "copy never ends"),
        }
    }
}

fn names(es: &[Entry]) -> Vec<String> {
    let mut v: Vec<String> = es
        .iter()
        .map(|e| String::from_utf8_lossy(&e.name).into_owned())
        .collect();
    v.sort();
    v
}

fn tree(fs: &mut Fs3<RamDisk>) {
    fs.mkdir("/a", NOW).unwrap();
    fs.mkdir("/a/sub", NOW).unwrap();
    fs.write_file("/a/one.txt", b"one", NOW).unwrap();
    fs.write_file("/a/sub/two.txt", &pat(10_000, 7), NOW)
        .unwrap();
    fs.mkdir("/b", NOW).unwrap();
}

mod backend_basics;
mod copy;
mod errors;
mod move_ops;
mod names;
mod paths;
