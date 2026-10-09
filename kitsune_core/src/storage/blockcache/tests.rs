use super::*;
use crate::storage::blockdev::{FaultyDisk, RamDisk};

fn blk(b: u8) -> Block {
    [b; BLOCK_SIZE]
}

fn cache(cap: usize) -> BlockCache<RamDisk> {
    BlockCache::new(RamDisk::new(8 * 64), 0, 64, cap)
}

#[test]
fn write_then_read_hits_cache_without_device_io() {
    let mut c = cache(4);
    c.write(3, &blk(7)).unwrap();
    let mut out = [0u8; BLOCK_SIZE];
    c.read(3, &mut out).unwrap();
    assert_eq!(out, blk(7));
    assert_eq!(c.stats().hits, 1);
    assert_eq!(c.dev().counters().sectors_written, 0); // still only in RAM
    assert_eq!(c.dirty_count(), 1);
}

#[test]
fn flush_persists_and_cleans() {
    let mut c = cache(4);
    c.write(1, &blk(1)).unwrap();
    c.write(2, &blk(2)).unwrap();
    c.flush().unwrap();
    assert_eq!(c.dirty_count(), 0);
    let d = c.into_inner();
    assert_eq!(d.as_bytes()[BLOCK_SIZE], 1);
    assert_eq!(d.as_bytes()[2 * BLOCK_SIZE], 2);
    assert_eq!(d.counters().flushes, 1);
}

#[test]
fn flush_coalesces_adjacent_blocks_into_one_transfer() {
    let mut c = cache(16);
    for b in 4..10 {
        c.write(b, &blk(b as u8)).unwrap();
    }
    c.write(20, &blk(9)).unwrap();
    c.flush().unwrap();
    // blocks 4..10 in one call, block 20 in another
    assert_eq!(c.dev().counters().write_calls, 2);
    assert_eq!(c.dev().counters().sectors_written, 7 * 8);
}

#[test]
fn flush_without_dirty_data_issues_no_barrier() {
    let mut c = cache(4);
    c.flush().unwrap();
    assert_eq!(c.dev().counters().flushes, 0);
}

#[test]
fn lru_evicts_least_recently_used() {
    let mut c = cache(2);
    c.write(1, &blk(1)).unwrap();
    c.write(2, &blk(2)).unwrap();
    let mut out = [0u8; BLOCK_SIZE];
    c.read(1, &mut out).unwrap(); // 1 is now MRU; 2 is LRU
    c.write(3, &blk(3)).unwrap(); // evicts 2 (dirty -> written back)
    assert_eq!(c.stats().evictions, 1);
    assert_eq!(c.cached(), 2);
    assert_eq!(c.dev().as_bytes()[2 * BLOCK_SIZE], 2); // 2 reached the disk
    assert_eq!(c.dev().as_bytes()[BLOCK_SIZE], 0); // 1 still only cached
    c.read(2, &mut out).unwrap(); // miss, comes back from disk
    assert_eq!(out, blk(2));
}

#[test]
fn capacity_one_still_works() {
    let mut c = cache(1);
    c.write(0, &blk(1)).unwrap();
    c.write(1, &blk(2)).unwrap();
    c.write(2, &blk(3)).unwrap();
    let mut out = [0u8; BLOCK_SIZE];
    c.read(0, &mut out).unwrap();
    assert_eq!(out, blk(1));
    c.flush().unwrap();
    assert!(c.cached() <= 1);
}

#[test]
fn zero_capacity_is_bumped_to_one() {
    let c = BlockCache::new(RamDisk::new(64), 0, 8, 0);
    assert_eq!(c.capacity(), 1);
}

#[test]
fn hit_miss_statistics() {
    let mut c = cache(4);
    let mut out = [0u8; BLOCK_SIZE];
    c.read(5, &mut out).unwrap(); // miss
    c.read(5, &mut out).unwrap(); // hit
    c.read(5, &mut out).unwrap(); // hit
    c.read(6, &mut out).unwrap(); // miss
    let s = c.stats();
    assert_eq!((s.hits, s.misses), (2, 2));
    assert_eq!(s.sectors_read, 16);
    c.reset_stats();
    assert_eq!(c.stats(), CacheStats::default());
}

#[test]
fn out_of_range_blocks_are_rejected() {
    let mut c = cache(4);
    let mut out = [0u8; BLOCK_SIZE];
    assert_eq!(c.read(64, &mut out), Err(IoError::OutOfRange));
    assert_eq!(c.write(64, &blk(1)), Err(IoError::OutOfRange));
    assert_eq!(c.write(u64::MAX, &blk(1)), Err(IoError::OutOfRange));
    assert_eq!(c.write_direct(99, &blk(1), 0), Err(IoError::OutOfRange));
    assert_eq!(
        c.read_many(60, &mut [0u8; BLOCK_SIZE * 8]),
        Err(IoError::OutOfRange)
    );
    assert_eq!(c.read_many(0, &mut [0u8; 100]), Err(IoError::BadLength));
}

#[test]
fn base_lba_offsets_blocks_on_the_device() {
    let mut c = BlockCache::new(RamDisk::new(128 + 8 * 4), 128, 4, 4);
    c.write(1, &blk(9)).unwrap();
    c.flush().unwrap();
    let d = c.into_inner();
    assert_eq!(d.as_bytes()[(128 + 8) * 512], 9);
    assert_eq!(d.min_written_lba(), Some(136));
}

#[test]
fn nblocks_is_clamped_to_the_device() {
    let c = BlockCache::new(RamDisk::new(16), 0, 1000, 4);
    assert_eq!(c.block_count(), 2);
    let c = BlockCache::new(RamDisk::new(16), 100, 1000, 4);
    assert_eq!(c.block_count(), 0);
}

#[test]
fn failed_read_inserts_nothing() {
    let mut c = BlockCache::new(FaultyDisk::new(RamDisk::new(64)).fail_read_nth(0), 0, 8, 4);
    let mut out = [0u8; BLOCK_SIZE];
    assert_eq!(c.read(2, &mut out), Err(IoError::Read));
    assert_eq!(c.cached(), 0);
    c.read(2, &mut out).unwrap(); // the transient fault is gone
    assert_eq!(c.cached(), 1);
}

#[test]
fn failed_writeback_keeps_block_dirty_and_retries() {
    let mut c = BlockCache::new(
        FaultyDisk::new(RamDisk::new(64)).bad_write_range(8, 16),
        0,
        8,
        4,
    );
    c.write(1, &blk(5)).unwrap();
    c.write(5, &blk(6)).unwrap();
    assert_eq!(c.flush(), Err(IoError::Write));
    assert_eq!(c.dirty_count(), 1, "only the bad block stays dirty");
    assert_eq!(c.dev().inner().as_bytes()[5 * BLOCK_SIZE], 6);
    let mut out = [0u8; BLOCK_SIZE];
    c.read(1, &mut out).unwrap();
    assert_eq!(out, blk(5)); // data still readable from the cache
    c.dev_mut().heal();
    c.flush().unwrap();
    assert_eq!(c.dirty_count(), 0);
    assert_eq!(c.dev().inner().as_bytes()[BLOCK_SIZE], 5);
}

#[test]
fn eviction_that_cannot_write_back_fails_without_losing_data() {
    let mut c = BlockCache::new(
        FaultyDisk::new(RamDisk::new(64)).bad_write_range(0, 8),
        0,
        8,
        1,
    );
    c.write(0, &blk(3)).unwrap(); // dirty, and its home is a bad sector
    assert_eq!(c.write(1, &blk(4)), Err(IoError::Write)); // cannot evict block 0
    let mut out = [0u8; BLOCK_SIZE];
    c.read(0, &mut out).unwrap();
    assert_eq!(out, blk(3));
    assert_eq!(c.dirty_count(), 1);
}

#[test]
fn failed_barrier_is_retried() {
    struct BadFlush {
        inner: RamDisk,
        fail: bool,
    }
    impl BlockDevice for BadFlush {
        fn sector_count(&self) -> u64 {
            self.inner.sector_count()
        }
        fn read_sectors(&mut self, l: u64, b: &mut [u8]) -> Result<(), IoError> {
            self.inner.read_sectors(l, b)
        }
        fn write_sectors(&mut self, l: u64, b: &[u8]) -> Result<(), IoError> {
            self.inner.write_sectors(l, b)
        }
        fn flush(&mut self) -> Result<(), IoError> {
            if self.fail {
                self.fail = false;
                return Err(IoError::Flush);
            }
            self.inner.flush()
        }
    }
    let mut c = BlockCache::new(
        BadFlush {
            inner: RamDisk::new(64),
            fail: true,
        },
        0,
        8,
        4,
    );
    c.write(0, &blk(1)).unwrap();
    assert_eq!(c.flush(), Err(IoError::Flush));
    assert_eq!(c.dirty_count(), 0); // data was written; only the barrier failed
    c.flush().unwrap(); // retried even though nothing is dirty
    assert_eq!(c.dev().inner.counters().flushes, 1);
}

#[test]
fn discard_drops_dirty_data() {
    let mut c = cache(4);
    c.write(2, &blk(1)).unwrap();
    c.discard(2);
    assert_eq!(c.cached(), 0);
    c.flush().unwrap();
    let mut out = [1u8; BLOCK_SIZE];
    c.read(2, &mut out).unwrap();
    assert_eq!(out, blk(0));
    c.discard(40); // not cached: no-op
}

#[test]
fn invalidate_all_forgets_everything() {
    let mut c = cache(4);
    c.write(0, &blk(1)).unwrap();
    c.invalidate_all();
    assert_eq!(c.cached(), 0);
    c.write(1, &blk(2)).unwrap(); // still usable
    c.flush().unwrap();
    assert_eq!(c.dev().as_bytes()[BLOCK_SIZE], 2);
}

#[test]
fn write_direct_updates_cached_copy_and_marks_it_clean() {
    let mut c = cache(4);
    c.write(1, &blk(1)).unwrap();
    c.write_direct(1, &blk(2), 7).unwrap();
    assert_eq!(c.dirty_count(), 0);
    let (b, tag) = c.get_tagged(1).unwrap();
    assert_eq!((b[0], tag), (2, 7));
    assert_eq!(c.dev().as_bytes()[BLOCK_SIZE], 2);
    // an uncached block is written but not inserted
    c.write_direct(9, &blk(4), 0).unwrap();
    assert_eq!(c.cached(), 1);
}

#[test]
fn tags_reset_on_write_and_reload() {
    let mut c = cache(1);
    c.write(0, &blk(1)).unwrap();
    c.set_tag(0, 5);
    assert_eq!(c.get_tagged(0).unwrap().1, 5);
    c.write(0, &blk(2)).unwrap();
    assert_eq!(c.get_tagged(0).unwrap().1, 0);
    c.set_tag(0, 5);
    c.flush().unwrap();
    let _ = c.get(1).unwrap(); // evicts block 0
    assert_eq!(c.get_tagged(0).unwrap().1, 0); // reloaded: tag lost
    c.set_tag(33, 1); // not cached: no-op, no panic
}

#[test]
fn read_many_mixes_cached_and_uncached_blocks() {
    let mut raw = RamDisk::new(8 * 64);
    for b in 0..6u8 {
        raw.write_sectors(b as u64 * 8, &[b + 1; BLOCK_SIZE])
            .unwrap();
    }
    let mut c2 = BlockCache::new(raw, 0, 64, 8);
    c2.write(2, &blk(99)).unwrap(); // dirty cached copy newer than disk
    let mut out = alloc::vec![0u8; BLOCK_SIZE * 5];
    c2.read_many(0, &mut out).unwrap();
    assert_eq!(out[0], 1);
    assert_eq!(out[BLOCK_SIZE], 2);
    assert_eq!(out[2 * BLOCK_SIZE], 99);
    assert_eq!(out[3 * BLOCK_SIZE], 4);
    assert_eq!(out[4 * BLOCK_SIZE], 5);
    // blocks 0,1 were one run and 3,4 another: two device reads
    assert_eq!(c2.dev().counters().read_calls, 2);
}

#[test]
fn long_reads_bypass_the_cache() {
    let mut c = cache(4);
    let mut out = alloc::vec![0u8; BLOCK_SIZE * 32];
    c.read_many(0, &mut out).unwrap();
    assert_eq!(c.cached(), 0);
    assert_eq!(c.dev().counters().read_calls, 1);
    let mut small = alloc::vec![0u8; BLOCK_SIZE * 3];
    c.read_many(0, &mut small).unwrap();
    assert_eq!(c.cached(), 3);
}

#[test]
fn write_many_short_is_write_back_long_is_direct() {
    let mut c = cache(64);
    c.write_many(0, &alloc::vec![1u8; BLOCK_SIZE * 3]).unwrap();
    assert_eq!(c.dev().counters().sectors_written, 0);
    assert_eq!(c.dirty_count(), 3);
    c.write_many(10, &alloc::vec![2u8; BLOCK_SIZE * 20])
        .unwrap();
    assert_eq!(c.dev().counters().write_calls, 1);
    assert_eq!(c.dev().as_bytes()[10 * BLOCK_SIZE], 2);
    c.flush().unwrap();
    assert_eq!(c.dev().as_bytes()[0], 1);
}

#[test]
fn long_write_refreshes_cached_copies() {
    let mut c = cache(64);
    c.write(12, &blk(1)).unwrap();
    c.write_many(10, &alloc::vec![5u8; BLOCK_SIZE * 16])
        .unwrap();
    let mut out = [0u8; BLOCK_SIZE];
    c.read(12, &mut out).unwrap();
    assert_eq!(out, blk(5));
    assert_eq!(c.dirty_count(), 0);
}

#[test]
fn failed_long_write_discards_stale_cache() {
    let mut c = BlockCache::new(
        FaultyDisk::new(RamDisk::new(8 * 64)).bad_write_range(80, 88),
        0,
        64,
        64,
    );
    c.write(12, &blk(1)).unwrap();
    assert!(
        c.write_many(10, &alloc::vec![5u8; BLOCK_SIZE * 16])
            .is_err()
    );
    assert_eq!(c.cached(), 0);
}

#[test]
fn many_random_accesses_match_a_model() {
    let mut c = cache(5);
    let mut model = alloc::vec![[0u8; BLOCK_SIZE]; 64];
    let mut x = 12345u64;
    for step in 0..3000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let b = (x % 64) as usize;
        if x & 0x100 != 0 {
            let v = (step % 251) as u8;
            c.write(b as u64, &blk(v)).unwrap();
            model[b] = blk(v);
        } else {
            let mut out = [0u8; BLOCK_SIZE];
            c.read(b as u64, &mut out).unwrap();
            assert_eq!(out, model[b], "step {step} block {b}");
        }
        if step % 400 == 0 {
            c.flush().unwrap();
        }
    }
    c.flush().unwrap();
    let d = c.into_inner();
    for (b, m) in model.iter().enumerate() {
        assert_eq!(&d.as_bytes()[b * BLOCK_SIZE..(b + 1) * BLOCK_SIZE], &m[..]);
    }
}
