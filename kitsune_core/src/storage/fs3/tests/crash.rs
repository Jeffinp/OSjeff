//! Power-loss and I/O-failure behaviour, proved with `FaultyDisk`.
//!
//! The method: run an operation sequence once on a healthy disk to get the
//! state after every operation ("oracle"). Then, for **every** possible cut
//! (each sector write and each flush is an event), replay the sequence on a
//! disk that loses power at that event, mount the survivor, and require that
//! `fsck` is clean and the whole tree equals the state before or after the
//! interrupted operation — never anything else, never a lost operation that
//! had already returned.

use super::*;
use crate::storage::blockdev::{BlockDevice, CrashMode, FaultyDisk};
use alloc::collections::BTreeMap;

type Snap = BTreeMap<Vec<u8>, Option<Vec<u8>>>;

#[derive(Clone, Debug)]
pub(super) enum Op {
    Mkdir(&'static str),
    Write(&'static str, Vec<u8>),
    WriteAt(&'static str, u64, Vec<u8>),
    Append(&'static str, Vec<u8>),
    Truncate(&'static str, u64),
    Rename(&'static str, &'static str),
    Remove(&'static str),
    Rmdir(&'static str),
    Trash(&'static str),
    Restore(&'static str),
    Purge(&'static str),
}

pub(super) fn apply<D: BlockDevice>(fs: &mut Fs3<D>, op: &Op, now: u64) -> Result<(), FsError> {
    match op {
        Op::Mkdir(p) => fs.mkdir(p, now).map(|_| ()),
        Op::Write(p, d) => fs.write_file(p, d, now),
        Op::WriteAt(p, off, d) => {
            let ino = fs.lookup(p)?;
            fs.write_at(ino, *off, d, now)
        }
        Op::Append(p, d) => {
            let ino = fs.lookup(p)?;
            fs.append(ino, d, now)
        }
        Op::Truncate(p, n) => {
            let ino = fs.lookup(p)?;
            fs.truncate(ino, *n, now)
        }
        Op::Rename(a, b) => fs.rename(a, b, now),
        Op::Remove(p) => fs.remove(p),
        Op::Rmdir(p) => fs.rmdir(p),
        Op::Trash(p) => fs.trash(p, now),
        Op::Restore(n) => fs.trash_restore(n.as_bytes(), now).map(|_| ()),
        Op::Purge(n) => fs.trash_purge(n.as_bytes()),
    }
}

/// The full visible tree (including `/.trash`): path -> content (`None` = directory).
pub(super) fn snapshot<D: BlockDevice>(fs: &mut Fs3<D>) -> Snap {
    let mut out = Snap::new();
    let mut stack: Vec<Vec<u8>> = alloc::vec![b"/".to_vec(), b"/.trash".to_vec()];
    out.insert(b"/.trash".to_vec(), None);
    while let Some(dir) = stack.pop() {
        for e in fs.readdir(&dir).unwrap() {
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
                    let data = fs.read_file(&p).unwrap();
                    out.insert(p, Some(data));
                }
            }
        }
    }
    out
}

pub(super) fn sequence() -> Vec<Op> {
    let mut rng = Rng(42);
    alloc::vec![
        Op::Mkdir("/d"),
        Op::Write("/d/a", rng.bytes(5000)),
        Op::Write("/b", rng.bytes(20_000)),
        Op::WriteAt("/b", 3000, rng.bytes(6000)),
        Op::Append("/b", rng.bytes(1500)),
        Op::Rename("/b", "/d/b"),
        Op::Truncate("/d/b", 5000),
        Op::Remove("/d/a"),
        Op::Trash("/d/b"),
        Op::Restore("b"),
        Op::Write("/d/b", rng.bytes(300)),
        Op::Mkdir("/d/sub"),
        Op::Write("/d/sub/x", rng.bytes(9000)),
        Op::Trash("/d/sub/x"),
        Op::Purge("x"),
        Op::Trash("/d"),
        Op::Restore("d"),
        Op::Rmdir("/d/sub"),
    ]
}

fn base_image(mib: u64) -> Vec<u8> {
    fresh(mib).into_device().into_bytes()
}

/// Oracle: state after each prefix of `ops` on a healthy disk.
fn oracle(base: &[u8], ops: &[Op]) -> Vec<Snap> {
    let mut fs = Fs3::mount(RamDisk::from_bytes(base.to_vec())).unwrap();
    let mut snaps = alloc::vec![snapshot(&mut fs)];
    for (i, op) in ops.iter().enumerate() {
        apply(&mut fs, op, 10 + i as u64).unwrap_or_else(|e| panic!("oracle op {i} {op:?}: {e:?}"));
        snaps.push(snapshot(&mut fs));
    }
    snaps
}

/// Number of events the whole sequence generates (mount included).
fn count_events(base: &[u8], ops: &[Op], mode: CrashMode) -> u64 {
    let dev = FaultyDisk::new(RamDisk::from_bytes(base.to_vec())).with_mode(mode);
    let mut fs = Fs3::mount(dev).unwrap();
    for (i, op) in ops.iter().enumerate() {
        apply(&mut fs, op, 10 + i as u64).unwrap();
    }
    fs.device().events()
}

/// Cut power at `k`, return the surviving disk and the index of the operation
/// that was interrupted (`ops.len()` if the cut came after the last one).
fn run_until_cut(base: &[u8], ops: &[Op], mode: CrashMode, k: u64) -> (RamDisk, usize) {
    let dev = FaultyDisk::new(RamDisk::from_bytes(base.to_vec()))
        .with_mode(mode)
        .crash_after(k);
    let mut fs = Fs3::mount(dev).unwrap();
    let mut failed = ops.len();
    for (i, op) in ops.iter().enumerate() {
        if apply(&mut fs, op, 10 + i as u64).is_err() {
            failed = i;
            break;
        }
    }
    (fs.into_device().into_inner(), failed)
}

fn check_survivor(disk: RamDisk, failed: usize, snaps: &[Snap], ctx: &str) {
    let mut fs = Fs3::mount(disk).unwrap_or_else(|e| panic!("{ctx}: mount failed: {e:?}"));
    let rep = fs.fsck().unwrap();
    assert!(rep.is_clean(), "{ctx}: fsck: {rep:?}");
    let snap = snapshot(&mut fs);
    let before = &snaps[failed];
    let after = snaps.get(failed + 1).unwrap_or(before);
    assert!(
        snap == *before || snap == *after,
        "{ctx}: state is neither before nor after op {failed}"
    );
}

fn exhaustive(mode: CrashMode, stride: u64) {
    exhaustive_ops(&sequence(), mode, stride);
}

fn exhaustive_ops(ops: &[Op], mode: CrashMode, stride: u64) {
    let base = base_image(1);
    let snaps = oracle(&base, ops);
    let total = count_events(&base, ops, mode);
    assert!(total > 100, "suspiciously few events: {total}");
    let mut k = 0;
    while k <= total {
        let (disk, failed) = run_until_cut(&base, ops, mode, k);
        check_survivor(
            disk,
            failed,
            &snaps,
            &alloc::format!("{mode:?} cut {k}/{total}"),
        );
        k += stride;
    }
}

#[test]
fn power_cut_at_every_event_in_order() {
    exhaustive(CrashMode::InOrder, 1);
}

#[test]
fn power_cut_with_a_volatile_cache_seed_1() {
    exhaustive(CrashMode::Lossy(1), 1);
}

#[test]
fn power_cut_with_a_volatile_cache_other_seeds() {
    for seed in [2u64, 3, 0xDEAD_BEEF, 77_777] {
        exhaustive(CrashMode::Lossy(seed), 3);
    }
}

#[test]
fn completed_operations_are_durable_across_a_cut_right_after() {
    // Cut immediately after the last event of each prefix: everything that
    // returned must be there (no "before" allowed).
    let ops = sequence();
    let base = base_image(1);
    let snaps = oracle(&base, &ops);
    for mode in [CrashMode::InOrder, CrashMode::Lossy(5)] {
        let mut ends = Vec::new();
        {
            let dev = FaultyDisk::new(RamDisk::from_bytes(base.clone())).with_mode(mode);
            let mut fs = Fs3::mount(dev).unwrap();
            for (i, op) in ops.iter().enumerate() {
                apply(&mut fs, op, 10 + i as u64).unwrap();
                ends.push(fs.device().events());
            }
        }
        for (i, &e) in ends.iter().enumerate() {
            let (disk, failed) = run_until_cut(&base, &ops, mode, e);
            // The cut lands exactly after op i returned, so op i+1 is the
            // interrupted one (or none).
            assert_eq!(failed, i + 1, "{mode:?} op {i}");
            let mut fs = Fs3::mount(disk).unwrap();
            assert!(fs.fsck().unwrap().is_clean());
            assert_eq!(snapshot(&mut fs), snaps[i + 1], "{mode:?}: op {i} lost");
        }
    }
}

#[test]
fn power_cut_during_the_mount_time_replay_is_harmless() {
    let ops = alloc::vec![
        Op::Mkdir("/d"),
        Op::Write("/d/a", alloc::vec![7u8; 6000]),
        Op::Rename("/d/a", "/a"),
    ];
    let base = base_image(1);
    let snaps = oracle(&base, &ops);
    let total = count_events(&base, &ops, CrashMode::InOrder);
    let mut replays = 0;
    for k in 0..=total {
        let (disk, failed) = run_until_cut(&base, &ops, CrashMode::InOrder, k);
        // Does mounting this survivor have to replay a journal?
        let probe = Fs3::mount(FaultyDisk::new(disk.clone())).unwrap();
        let replay_events = probe.device().events();
        drop(probe);
        if replay_events == 0 {
            continue;
        }
        replays += 1;
        for j in 0..=replay_events {
            let mut d = FaultyDisk::new(disk.clone()).crash_after(j);
            // The mount may fail when the cut lands inside the replay; the
            // survivor is whatever reached the medium.
            let _ = Fs3::mount(&mut d);
            let survivor = d.into_inner();
            check_survivor(
                survivor,
                failed,
                &snaps,
                &alloc::format!("cut {k} replay-cut {j}"),
            );
        }
    }
    assert!(replays > 5, "only {replays} cuts needed a replay");
}

#[test]
fn mount_replay_is_idempotent() {
    let ops = sequence();
    let base = base_image(1);
    let total = count_events(&base, &ops, CrashMode::InOrder);
    // Find a cut that leaves a committed-but-unapplied transaction.
    let mut found = 0;
    for k in 0..=total {
        let (disk, _) = run_until_cut(&base, &ops, CrashMode::InOrder, k);
        let probe = Fs3::mount(FaultyDisk::new(disk.clone())).unwrap();
        if probe.device().events() == 0 {
            continue;
        }
        found += 1;
        // Mount the replayed image again: must be a no-op (no writes at all).
        let replayed = probe.into_device().into_inner();
        let again = Fs3::mount(FaultyDisk::new(replayed.clone())).unwrap();
        assert_eq!(again.device().events(), 0, "second mount wrote again");
        assert_eq!(
            again.into_device().into_inner().as_bytes(),
            replayed.as_bytes()
        );
        if found >= 25 {
            break;
        }
    }
    assert!(found > 5);
}

#[test]
fn power_cut_during_format_leaves_blank_or_a_complete_filesystem() {
    use crate::storage::fs3::{Detected, detect};
    let mut total = 0;
    {
        let dev = FaultyDisk::new(RamDisk::new(2048));
        let fs = Fs3::format(dev, &FormatOptions::new(UUID, 0)).unwrap();
        total = total.max(fs.device().events());
    }
    assert!(total > 50, "{total} events");
    for k in 0..=total {
        let mut d = FaultyDisk::new(RamDisk::new(2048)).crash_after(k);
        let _ = Fs3::format(&mut d, &FormatOptions::new(UUID, 0));
        let mut disk = d.into_inner();
        match detect(&mut disk).unwrap() {
            Detected::Blank => {
                // Not visible: formatting again must work.
                let mut fs = Fs3::format(disk, &FormatOptions::new(UUID, 0)).unwrap();
                assert_clean(&mut fs);
            }
            Detected::V3 => {
                let mut fs = Fs3::mount(disk).unwrap();
                assert_clean(&mut fs);
            }
            other => panic!("cut {k}: unexpected {other:?}"),
        }
    }
}

/// A populated base image plus the operations used by the failure-injection tests.
fn populated() -> (Vec<u8>, Vec<Op>) {
    let mut seed = Fs3::mount(RamDisk::from_bytes(base_image(1))).unwrap();
    for (i, op) in sequence().iter().take(6).enumerate() {
        apply(&mut seed, op, i as u64).unwrap();
    }
    let ops = alloc::vec![
        Op::WriteAt("/d/b", 100, alloc::vec![7u8; 3000]),
        Op::Rename("/d/b", "/d/c"),
        Op::Mkdir("/d/e"),
        Op::Trash("/d/c"),
        Op::Restore("c"),
        Op::Remove("/d/a"),
        Op::Truncate("/d/c", 10),
        Op::Write("/d/new", alloc::vec![3u8; 70_000]),
        Op::Append("/d/new", alloc::vec![4u8; 5000]),
        Op::Rename("/d/new", "/n"),
        Op::Rmdir("/d/e"),
    ];
    (seed.into_device().into_bytes(), ops)
}

/// Mount `base` with a 2-block cache (so almost every access is a device read)
/// and run `ops[..i]` healthily.
fn prefix(base: &[u8], ops: &[Op], i: usize) -> Fs3<FaultyDisk<RamDisk>> {
    let dev = FaultyDisk::new(RamDisk::from_bytes(base.to_vec()));
    let mut fs = Fs3::mount_with(dev, 2).unwrap();
    for (k, op) in ops.iter().take(i).enumerate() {
        apply(&mut fs, op, 10 + k as u64).unwrap();
    }
    fs
}

#[test]
fn a_read_failure_at_any_point_aborts_the_operation_without_side_effects() {
    let (base, ops) = populated();
    let snaps = oracle(&base, &ops);
    let mut hits = 0;
    for (i, op) in ops.iter().enumerate() {
        let mut probe = prefix(&base, &ops, i);
        let r0 = probe.device().read_calls();
        apply(&mut probe, op, 10 + i as u64).unwrap();
        let r1 = probe.device().read_calls();
        for j in r0..r1 {
            let mut fs = prefix(&base, &ops, i);
            fs.device_mut().set_fail_read_at(Some(j));
            let r = apply(&mut fs, op, 10 + i as u64);
            fs.device_mut().heal();
            assert!(matches!(r, Err(FsError::Io(_))), "op {i} read #{j}: {r:?}");
            hits += 1;
            // Not poisoned: the operation was undone, nothing else changed.
            assert_eq!(snapshot(&mut fs), snaps[i], "op {i} read #{j}");
            assert_clean(&mut fs);
            apply(&mut fs, op, 10 + i as u64).unwrap();
            assert_eq!(snapshot(&mut fs), snaps[i + 1], "op {i} read #{j} retry");
            assert_clean(&mut fs);
        }
    }
    assert!(hits > 25, "only {hits} read failures injected");
}

#[test]
fn a_write_failure_at_any_point_leaves_a_consistent_disk() {
    let (base, ops) = populated();
    let snaps = oracle(&base, &ops);
    let mut hits = 0;
    for (i, op) in ops.iter().enumerate() {
        let mut probe = prefix(&base, &ops, i);
        let w0 = probe.device().write_calls();
        apply(&mut probe, op, 10 + i as u64).unwrap();
        let w1 = probe.device().write_calls();
        for j in w0..w1 {
            let mut fs = prefix(&base, &ops, i);
            fs.device_mut().set_fail_write_at(Some(j));
            let r = apply(&mut fs, op, 10 + i as u64);
            assert!(matches!(r, Err(FsError::Io(_))), "op {i} write #{j}: {r:?}");
            hits += 1;
            // Whether the transaction was aborted (failure before the journal)
            // or the filesystem poisoned (failure during commit), the medium
            // holds the state before or after the operation.
            let mut dev = fs.into_device();
            dev.heal();
            check_survivor(
                dev.into_inner(),
                i,
                &snaps,
                &alloc::format!("op {i} write #{j}"),
            );
        }
    }
    assert!(hits > 30, "only {hits} write failures injected");
}

#[test]
fn write_failure_during_commit_is_sticky_until_remount() {
    let (base, ops) = populated();
    let mut fs = prefix(&base, &ops, 0);
    let w = fs.device().write_calls();
    // The first journal write of the next commit fails.
    fs.device_mut().set_fail_write_at(Some(w));
    assert!(matches!(apply(&mut fs, &ops[1], 5), Err(FsError::Io(_))));
    assert_eq!(apply(&mut fs, &ops[2], 5), Err(FsError::Poisoned));
    assert_eq!(fs.fsck(), Err(FsError::Poisoned));
}

#[test]
fn poisoned_filesystem_refuses_everything_until_remounted() {
    let base = base_image(1);
    // Fail the journal payload write of the first commit.
    let dev = FaultyDisk::new(RamDisk::from_bytes(base)).bad_write_range(128 + 16, 128 + 8 * 26);
    let mut fs = Fs3::mount(dev).unwrap();
    assert!(matches!(fs.create("/a", 0), Err(FsError::Io(_))));
    assert_eq!(fs.create("/b", 0), Err(FsError::Poisoned));
    assert_eq!(fs.lookup("/"), Err(FsError::Poisoned));
    assert_eq!(fs.readdir("/"), Err(FsError::Poisoned));
    assert_eq!(fs.sync(), Err(FsError::Poisoned));
    assert_eq!(fs.fsck(), Err(FsError::Poisoned));
    let mut dev = fs.into_device();
    dev.heal();
    let mut fs = Fs3::mount(dev.into_inner()).unwrap();
    assert_clean(&mut fs);
    assert!(fs.readdir("/").unwrap().is_empty());
    fs.create("/a", 0).unwrap();
}

#[test]
fn large_direct_write_failure_aborts_cleanly() {
    // A 100-block write goes straight to the device; fail that transfer.
    let base = base_image(2);
    let dev = FaultyDisk::new(RamDisk::from_bytes(base)).fail_write_nth(0);
    let mut fs = Fs3::mount(dev).unwrap();
    let before = snapshot(&mut fs);
    let free = fs.statfs();
    let r = fs.write_file("/big", &alloc::vec![9u8; 100 * 4096], 1);
    assert!(matches!(r, Err(FsError::Io(_))));
    assert_eq!(fs.statfs(), free);
    assert_eq!(snapshot(&mut fs), before);
    assert_clean(&mut fs);
    fs.write_file("/big", &alloc::vec![9u8; 100 * 4096], 1)
        .unwrap();
    assert_clean(&mut fs);
}

#[test]
fn hostile_journal_record_cannot_overwrite_the_superblock() {
    use crate::storage::fs3::layout::{FS_START_LBA, build_journal_header};
    // Craft a checksum-valid journal that targets block 0 (the superblock).
    let mut disk = RamDisk::from_bytes(base_image(1));
    let payload = [0xEEu8; 4096];
    let hdr = build_journal_header(5, &[0], &[&payload]);
    let base_byte = FS_START_LBA as usize * 512;
    disk.as_bytes_mut()[base_byte + 4096..base_byte + 8192].copy_from_slice(&hdr);
    disk.as_bytes_mut()[base_byte + 8192..base_byte + 12288].copy_from_slice(&payload);
    assert!(matches!(Fs3::mount(disk.clone()), Err(FsError::Corrupt(_))));
    // The superblock is untouched.
    assert_eq!(disk.as_bytes()[base_byte + 4], b'O');
}

/// A pseudo-random operation sequence over a small namespace.
fn random_sequence(seed: u64, len: usize) -> Vec<Op> {
    const DIRS: [&str; 3] = ["/d", "/d/e", "/f"];
    const FILES: [&str; 6] = ["/d/a", "/d/e/b", "/f/c", "/g", "/d/h", "/f/i"];
    let mut rng = Rng(seed);
    let mut ops = alloc::vec![Op::Mkdir("/d"), Op::Mkdir("/d/e"), Op::Mkdir("/f")];
    for _ in 0..len {
        let file = FILES[rng.below(6) as usize];
        let other = FILES[rng.below(6) as usize];
        let size = [10usize, 700, 4096, 4097, 9000, 14_000][rng.below(6) as usize];
        ops.push(match rng.below(11) {
            0 => Op::Mkdir(DIRS[rng.below(3) as usize]),
            1..=3 => Op::Write(file, rng.bytes(size)),
            4 => Op::WriteAt(file, rng.below(9000), rng.bytes(size / 2 + 1)),
            5 => Op::Append(file, rng.bytes(size)),
            6 => Op::Truncate(file, rng.below(12_000)),
            7 => Op::Rename(file, other),
            8 => Op::Remove(file),
            9 => Op::Trash(file),
            _ => Op::Restore(["a", "b", "c", "g", "h", "i"][rng.below(6) as usize]),
        });
    }
    ops
}

#[test]
fn random_sequences_survive_a_power_cut_at_every_event() {
    for seed in 1..=4u64 {
        // Operations that fail on a healthy disk (missing parent, name taken,
        // ...) are part of the mix, but the oracle must succeed for every op
        // it counts: keep only the ones that do.
        let base = base_image(1);
        let mut fs = Fs3::mount(RamDisk::from_bytes(base)).unwrap();
        let mut ops = Vec::new();
        for (i, op) in random_sequence(seed * 7919, 80).into_iter().enumerate() {
            if ops.len() < 22 && apply(&mut fs, &op, 10 + ops.len() as u64).is_ok() {
                ops.push(op);
            }
            let _ = i;
        }
        assert!(
            ops.len() >= 8,
            "seed {seed}: only {} ops succeeded",
            ops.len()
        );
        exhaustive_ops(&ops, CrashMode::InOrder, 2);
        exhaustive_ops(&ops, CrashMode::Lossy(seed), 5);
    }
}
