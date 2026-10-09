use super::*;

fn sector(b: u8) -> [u8; SECTOR_SIZE] {
    [b; SECTOR_SIZE]
}

#[test]
fn ramdisk_roundtrip_and_counters() {
    let mut d = RamDisk::new(8);
    assert_eq!(d.sector_count(), 8);
    d.write_sectors(2, &[7u8; 1024]).unwrap();
    let mut out = [0u8; 1024];
    d.read_sectors(2, &mut out).unwrap();
    assert_eq!(out, [7u8; 1024]);
    d.flush().unwrap();
    let c = d.counters();
    assert_eq!(c.sectors_written, 2);
    assert_eq!(c.sectors_read, 2);
    assert_eq!(c.flushes, 1);
    assert_eq!(d.min_written_lba(), Some(2));
    d.reset_counters();
    assert_eq!(d.counters(), IoCounters::default());
}

#[test]
fn ramdisk_rejects_bad_ranges_and_lengths() {
    let mut d = RamDisk::new(4);
    let mut b = [0u8; 512];
    assert_eq!(d.read_sectors(4, &mut b), Err(IoError::OutOfRange));
    assert_eq!(d.write_sectors(4, &b), Err(IoError::OutOfRange));
    assert_eq!(d.read_sectors(u64::MAX, &mut b), Err(IoError::OutOfRange));
    assert_eq!(d.read_sectors(0, &mut [0u8; 100]), Err(IoError::BadLength));
    assert_eq!(d.write_sectors(0, &[0u8; 513]), Err(IoError::BadLength));
    assert_eq!(d.write_sectors(3, &[0u8; 1024]), Err(IoError::OutOfRange));
    // zero-length transfers are fine
    assert_eq!(d.write_sectors(4, &[]), Ok(()));
}

#[test]
fn ramdisk_from_bytes_pads() {
    let d = RamDisk::from_bytes(alloc::vec![1u8; 700]);
    assert_eq!(d.sector_count(), 2);
    assert_eq!(d.as_bytes()[699], 1);
    assert_eq!(d.as_bytes()[700], 0);
    assert_eq!(d.into_bytes().len(), 1024);
}

#[test]
fn mut_ref_implements_device() {
    let mut d = RamDisk::new(2);
    fn use_dev<T: BlockDevice>(mut t: T) {
        t.write_sectors(0, &[9u8; 512]).unwrap();
        t.flush().unwrap();
    }
    use_dev(&mut d);
    assert_eq!(d.as_bytes()[0], 9);
}

#[test]
fn crash_in_order_stops_after_n_events() {
    let mut f = FaultyDisk::new(RamDisk::new(8)).crash_after(2);
    f.write_sectors(0, &sector(1)).unwrap(); // event 0
    f.write_sectors(1, &sector(2)).unwrap(); // event 1
    assert_eq!(f.write_sectors(2, &sector(3)), Err(IoError::PowerLoss));
    assert!(f.crashed());
    let mut b = [0u8; 512];
    assert_eq!(f.read_sectors(0, &mut b), Err(IoError::PowerLoss));
    assert_eq!(f.flush(), Err(IoError::PowerLoss));
    let d = f.into_inner();
    assert_eq!(d.as_bytes()[0], 1);
    assert_eq!(d.as_bytes()[512], 2);
    assert_eq!(d.as_bytes()[1024], 0);
}

#[test]
fn crash_tears_multi_sector_write() {
    let mut f = FaultyDisk::new(RamDisk::new(8)).crash_after(3);
    assert_eq!(f.write_sectors(0, &[5u8; 4096]), Err(IoError::PowerLoss));
    let d = f.into_inner();
    assert_eq!(d.as_bytes()[..1536], [5u8; 1536]);
    assert_eq!(d.as_bytes()[1536], 0);
}

#[test]
fn crash_counts_flush_as_event() {
    let mut f = FaultyDisk::new(RamDisk::new(2)).crash_after(1);
    f.write_sectors(0, &sector(1)).unwrap();
    assert_eq!(f.flush(), Err(IoError::PowerLoss));
    assert_eq!(f.events(), 1);
}

#[test]
fn lossy_flushed_writes_always_survive() {
    for seed in 0..20 {
        let mut f = FaultyDisk::new(RamDisk::new(8))
            .with_mode(CrashMode::Lossy(seed))
            .crash_after(3);
        f.write_sectors(0, &sector(1)).unwrap(); // 0
        f.flush().unwrap(); // 1
        f.write_sectors(1, &sector(2)).unwrap(); // 2
        assert_eq!(f.write_sectors(2, &sector(3)), Err(IoError::PowerLoss)); // cut at 3
        let d = f.into_inner();
        assert_eq!(d.as_bytes()[0], 1, "flushed write lost (seed {seed})");
    }
}

#[test]
fn lossy_unflushed_writes_vary_with_seed() {
    let mut survived = [0u32; 2];
    for seed in 0..64 {
        let mut f = FaultyDisk::new(RamDisk::new(8))
            .with_mode(CrashMode::Lossy(seed))
            .crash_after(2);
        f.write_sectors(0, &sector(1)).unwrap();
        f.write_sectors(1, &sector(2)).unwrap();
        let _ = f.flush();
        let d = f.into_inner();
        survived[0] += (d.as_bytes()[0] == 1) as u32;
        survived[1] += (d.as_bytes()[512] == 2) as u32;
    }
    // Both outcomes must occur for both sectors across 64 seeds.
    for s in survived {
        assert!(s > 5 && s < 59, "survival count {s}");
    }
}

#[test]
fn lossy_reads_see_pending_writes() {
    let mut f = FaultyDisk::new(RamDisk::new(4)).with_mode(CrashMode::Lossy(1));
    f.write_sectors(1, &sector(4)).unwrap();
    let mut b = [0u8; 512];
    f.read_sectors(1, &mut b).unwrap();
    assert_eq!(b, sector(4));
    // clean shutdown flushes
    let d = f.into_inner();
    assert_eq!(d.as_bytes()[512], 4);
}

#[test]
fn transient_failures_hit_exactly_the_nth_call() {
    let mut f = FaultyDisk::new(RamDisk::new(4))
        .fail_read_nth(1)
        .fail_write_nth(0);
    let mut b = [0u8; 512];
    f.read_sectors(0, &mut b).unwrap();
    assert_eq!(f.read_sectors(0, &mut b), Err(IoError::Read));
    f.read_sectors(0, &mut b).unwrap();
    assert_eq!(f.write_sectors(0, &sector(1)), Err(IoError::Write));
    f.write_sectors(0, &sector(1)).unwrap();
    assert_eq!(f.inner().as_bytes()[0], 1);
}

#[test]
fn failures_can_be_armed_on_a_disk_in_use() {
    let mut f = FaultyDisk::new(RamDisk::new(4));
    let mut b = [0u8; 512];
    f.read_sectors(0, &mut b).unwrap();
    f.write_sectors(0, &sector(1)).unwrap();
    assert_eq!((f.read_calls(), f.write_calls()), (1, 1));
    f.set_fail_read_at(Some(2));
    f.set_fail_write_at(Some(2));
    f.read_sectors(0, &mut b).unwrap();
    assert_eq!(f.read_sectors(0, &mut b), Err(IoError::Read));
    f.write_sectors(0, &sector(1)).unwrap();
    assert_eq!(f.write_sectors(0, &sector(1)), Err(IoError::Write));
    f.set_fail_read_at(None);
    f.read_sectors(0, &mut b).unwrap();
}

#[test]
fn bad_ranges_are_persistent_until_healed() {
    let mut f = FaultyDisk::new(RamDisk::new(8))
        .bad_read_range(2, 4)
        .bad_write_range(6, 8);
    let mut b = [0u8; 1024];
    assert_eq!(f.read_sectors(1, &mut b), Err(IoError::Read)); // sectors 1,2
    assert_eq!(f.read_sectors(1, &mut b), Err(IoError::Read));
    f.read_sectors(4, &mut b).unwrap();
    assert_eq!(f.write_sectors(7, &sector(1)), Err(IoError::Write));
    f.write_sectors(5, &sector(1)).unwrap();
    f.heal();
    f.read_sectors(1, &mut b).unwrap();
    f.write_sectors(7, &sector(1)).unwrap();
}

#[test]
fn no_fault_configured_is_transparent() {
    let mut f = FaultyDisk::new(RamDisk::new(4));
    f.write_sectors(0, &[3u8; 2048]).unwrap();
    f.flush().unwrap();
    assert_eq!(f.events(), 5);
    assert!(!f.crashed());
    assert_eq!(f.into_inner().as_bytes()[2047], 3);
}
