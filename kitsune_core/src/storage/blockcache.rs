//! Write-back LRU cache of 4 KiB blocks over a [`BlockDevice`].
//!
//! Fixed capacity (`no_std` + `alloc`: slots are allocated lazily up to the
//! capacity and then recycled), least-recently-used eviction, and hit/miss
//! statistics. Block `b` lives at sectors `base_lba + 8*b ..` of the device, so
//! a filesystem that starts at an arbitrary LBA addresses its own blocks from 0.
//!
//! Failure behaviour (the property the filesystem relies on):
//!
//! * a failed device **read** never inserts anything and never changes state;
//! * a failed **write-back** (eviction or [`flush`](BlockCache::flush)) leaves
//!   the block cached *and still dirty*, so nothing is lost and a later flush
//!   retries it; eviction that cannot write its victim back fails the request
//!   instead of dropping data;
//! * [`flush`](BlockCache::flush) writes every dirty block (coalescing adjacent
//!   blocks into one transfer) and then issues the device barrier; if only the
//!   barrier failed, the next `flush` repeats it.
//!
//! Each slot also carries a one-byte caller *tag* that is reset to 0 whenever
//! the block is (re)loaded from the device or rewritten through
//! [`write`](BlockCache::write); the filesystem uses it to remember "checksum
//! already verified" and so avoids re-hashing hot metadata blocks.

use crate::storage::blockdev::{BlockDevice, IoError, SECTOR_SIZE};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Cache block size in bytes.
pub const BLOCK_SIZE: usize = 4096;
/// Sectors per cache block.
pub const SECTORS_PER_BLOCK: u64 = (BLOCK_SIZE / SECTOR_SIZE) as u64;

/// One cache block.
pub type Block = [u8; BLOCK_SIZE];

const NIL: usize = usize::MAX;
/// Largest transfer (in blocks) a flush coalesces into one device write.
const FLUSH_RUN: usize = 16;
/// Reads of more blocks than this bypass the cache (no insertion), so one big
/// sequential read cannot flush the hot metadata out.
const INSERT_RUN_MAX: usize = 8;
/// Writes of at least this many blocks go straight to the device.
const DIRECT_WRITE_MIN: usize = 16;

/// Cache counters.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct CacheStats {
    /// Block accesses served from the cache.
    pub hits: u64,
    /// Block accesses that were not cached.
    pub misses: u64,
    /// Victims evicted to make room.
    pub evictions: u64,
    /// Dirty blocks written to the device (eviction + flush).
    pub writebacks: u64,
    /// Sectors read from the device.
    pub sectors_read: u64,
    /// Sectors written to the device.
    pub sectors_written: u64,
    /// Device barriers issued.
    pub flushes: u64,
}

struct Slot {
    blk: u64,
    data: Box<Block>,
    dirty: bool,
    tag: u8,
    prev: usize,
    next: usize,
}

/// The cache. See the module documentation.
pub struct BlockCache<D: BlockDevice> {
    dev: D,
    base_lba: u64,
    nblocks: u64,
    capacity: usize,
    slots: Vec<Slot>,
    free: Vec<usize>,
    map: BTreeMap<u64, usize>,
    head: usize, // most recently used
    tail: usize, // least recently used
    dev_dirty: bool,
    stats: CacheStats,
}

impl<D: BlockDevice> BlockCache<D> {
    /// A cache over blocks `0..nblocks` located at `base_lba` of `dev`, holding
    /// at most `capacity` blocks (at least 1).
    ///
    /// `nblocks` is clamped to what the device can actually hold.
    pub fn new(dev: D, base_lba: u64, nblocks: u64, capacity: usize) -> Self {
        let avail = dev.sector_count().saturating_sub(base_lba) / SECTORS_PER_BLOCK;
        BlockCache {
            dev,
            base_lba,
            nblocks: nblocks.min(avail),
            capacity: capacity.max(1),
            slots: Vec::new(),
            free: Vec::new(),
            map: BTreeMap::new(),
            head: NIL,
            tail: NIL,
            dev_dirty: false,
            stats: CacheStats::default(),
        }
    }

    /// Number of addressable blocks.
    pub fn block_count(&self) -> u64 {
        self.nblocks
    }

    /// Maximum number of cached blocks.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of blocks currently cached.
    pub fn cached(&self) -> usize {
        self.map.len()
    }

    /// Number of cached blocks not yet written to the device.
    pub fn dirty_count(&self) -> usize {
        self.map.values().filter(|&&i| self.slots[i].dirty).count()
    }

    /// Counters.
    pub fn stats(&self) -> CacheStats {
        self.stats
    }

    /// Zero the counters.
    pub fn reset_stats(&mut self) {
        self.stats = CacheStats::default();
    }

    /// Borrow the device (cached dirty data is *not* on it yet).
    pub fn dev(&self) -> &D {
        &self.dev
    }

    /// Mutably borrow the device. Bypasses the cache: the caller must not
    /// change blocks that may be cached.
    pub fn dev_mut(&mut self) -> &mut D {
        &mut self.dev
    }

    /// Give back the device. Dirty blocks are **dropped**; call
    /// [`flush`](Self::flush) first to keep them.
    pub fn into_inner(self) -> D {
        self.dev
    }

    fn check(&self, blk: u64, n: u64) -> Result<(), IoError> {
        match blk.checked_add(n) {
            Some(end) if end <= self.nblocks => Ok(()),
            _ => Err(IoError::OutOfRange),
        }
    }

    fn lba(&self, blk: u64) -> u64 {
        self.base_lba + blk * SECTORS_PER_BLOCK
    }

    // ---- LRU list ----

    fn unlink(&mut self, i: usize) {
        let (p, n) = (self.slots[i].prev, self.slots[i].next);
        if p != NIL {
            self.slots[p].next = n;
        } else {
            self.head = n;
        }
        if n != NIL {
            self.slots[n].prev = p;
        } else {
            self.tail = p;
        }
        self.slots[i].prev = NIL;
        self.slots[i].next = NIL;
    }

    fn push_front(&mut self, i: usize) {
        self.slots[i].prev = NIL;
        self.slots[i].next = self.head;
        if self.head != NIL {
            self.slots[self.head].prev = i;
        } else {
            self.tail = i;
        }
        self.head = i;
    }

    fn touch(&mut self, i: usize) {
        if self.head != i {
            self.unlink(i);
            self.push_front(i);
        }
    }

    /// Cached slot of `blk` (promoted to most-recently-used), counting a hit.
    fn lookup(&mut self, blk: u64) -> Option<usize> {
        let i = *self.map.get(&blk)?;
        self.touch(i);
        self.stats.hits += 1;
        Some(i)
    }

    /// Write slot `i` back to the device; on error it stays dirty.
    fn write_back(&mut self, i: usize) -> Result<(), IoError> {
        let lba = self.lba(self.slots[i].blk);
        self.dev.write_sectors(lba, &self.slots[i].data[..])?;
        self.slots[i].dirty = false;
        self.dev_dirty = true;
        self.stats.writebacks += 1;
        self.stats.sectors_written += SECTORS_PER_BLOCK;
        Ok(())
    }

    /// A slot not in use, evicting the LRU block if the cache is full.
    fn acquire_slot(&mut self) -> Result<usize, IoError> {
        if let Some(i) = self.free.pop() {
            return Ok(i);
        }
        if self.slots.len() < self.capacity {
            self.slots.push(Slot {
                blk: 0,
                data: Box::new([0u8; BLOCK_SIZE]),
                dirty: false,
                tag: 0,
                prev: NIL,
                next: NIL,
            });
            return Ok(self.slots.len() - 1);
        }
        let i = self.tail;
        debug_assert!(i != NIL);
        if self.slots[i].dirty {
            self.write_back(i)?;
        }
        self.unlink(i);
        let blk = self.slots[i].blk;
        self.map.remove(&blk);
        self.stats.evictions += 1;
        Ok(i)
    }

    fn install(&mut self, i: usize, blk: u64, dirty: bool) {
        self.slots[i].blk = blk;
        self.slots[i].dirty = dirty;
        self.slots[i].tag = 0;
        self.map.insert(blk, i);
        self.push_front(i);
    }

    /// Slot holding `blk`, loading it from the device on a miss.
    fn load(&mut self, blk: u64) -> Result<usize, IoError> {
        self.check(blk, 1)?;
        if let Some(i) = self.lookup(blk) {
            return Ok(i);
        }
        self.stats.misses += 1;
        let i = self.acquire_slot()?;
        let lba = self.lba(blk);
        if let Err(e) = self.dev.read_sectors(lba, &mut self.slots[i].data[..]) {
            self.free.push(i);
            return Err(e);
        }
        self.stats.sectors_read += SECTORS_PER_BLOCK;
        self.install(i, blk, false);
        Ok(i)
    }

    // ---- single-block API ----

    /// Borrow block `blk` (loading it on a miss).
    pub fn get(&mut self, blk: u64) -> Result<&Block, IoError> {
        let i = self.load(blk)?;
        Ok(&self.slots[i].data)
    }

    /// Like [`get`](Self::get), also returning the caller tag (0 when the block
    /// was just loaded or rewritten through [`write`](Self::write)).
    pub fn get_tagged(&mut self, blk: u64) -> Result<(&Block, u8), IoError> {
        let i = self.load(blk)?;
        Ok((&self.slots[i].data, self.slots[i].tag))
    }

    /// Set the caller tag of a block that is currently cached (no-op otherwise).
    pub fn set_tag(&mut self, blk: u64, tag: u8) {
        if let Some(&i) = self.map.get(&blk) {
            self.slots[i].tag = tag;
        }
    }

    /// Copy block `blk` into `out`.
    pub fn read(&mut self, blk: u64, out: &mut Block) -> Result<(), IoError> {
        let i = self.load(blk)?;
        out.copy_from_slice(&self.slots[i].data[..]);
        Ok(())
    }

    /// Overwrite block `blk` in the cache (write-back: not on the device until
    /// evicted or flushed). Needs no read, since the whole block is replaced.
    pub fn write(&mut self, blk: u64, data: &Block) -> Result<(), IoError> {
        self.check(blk, 1)?;
        let i = match self.lookup(blk) {
            Some(i) => i,
            None => {
                self.stats.misses += 1;
                let i = self.acquire_slot()?;
                self.install(i, blk, true);
                i
            }
        };
        self.slots[i].data.copy_from_slice(&data[..]);
        self.slots[i].dirty = true;
        self.slots[i].tag = 0;
        Ok(())
    }

    /// Write block `blk` straight to the device (not durable until the next
    /// [`flush`](Self::flush)) and keep any cached copy coherent and clean,
    /// tagged `tag`. On error the cached copy is left untouched.
    pub fn write_direct(&mut self, blk: u64, data: &Block, tag: u8) -> Result<(), IoError> {
        self.check(blk, 1)?;
        let lba = self.lba(blk);
        self.dev.write_sectors(lba, &data[..])?;
        self.dev_dirty = true;
        self.stats.sectors_written += SECTORS_PER_BLOCK;
        if let Some(&i) = self.map.get(&blk) {
            self.slots[i].data.copy_from_slice(&data[..]);
            self.slots[i].dirty = false;
            self.slots[i].tag = tag;
        }
        Ok(())
    }

    /// Drop block `blk` from the cache **without** writing it back.
    pub fn discard(&mut self, blk: u64) {
        if let Some(i) = self.map.remove(&blk) {
            self.unlink(i);
            self.slots[i].dirty = false;
            self.free.push(i);
        }
    }

    /// Drop every cached block, dirty ones included.
    pub fn invalidate_all(&mut self) {
        self.map.clear();
        self.free.clear();
        self.slots.clear();
        self.head = NIL;
        self.tail = NIL;
    }

    // ---- multi-block API ----

    /// Read `out.len() / 4096` consecutive blocks starting at `blk`. Cached
    /// blocks are copied; each maximal uncached run is fetched with a single
    /// device read, and only short runs are inserted into the cache.
    pub fn read_many(&mut self, blk: u64, out: &mut [u8]) -> Result<(), IoError> {
        if !out.len().is_multiple_of(BLOCK_SIZE) {
            return Err(IoError::BadLength);
        }
        let n = out.len() / BLOCK_SIZE;
        self.check(blk, n as u64)?;
        let mut i = 0;
        while i < n {
            let b = blk + i as u64;
            if let Some(s) = self.lookup(b) {
                out[i * BLOCK_SIZE..(i + 1) * BLOCK_SIZE].copy_from_slice(&self.slots[s].data[..]);
                i += 1;
                continue;
            }
            let mut j = i + 1;
            while j < n && !self.map.contains_key(&(blk + j as u64)) {
                j += 1;
            }
            let lba = self.lba(b);
            self.dev
                .read_sectors(lba, &mut out[i * BLOCK_SIZE..j * BLOCK_SIZE])?;
            self.stats.misses += (j - i) as u64;
            self.stats.sectors_read += (j - i) as u64 * SECTORS_PER_BLOCK;
            if j - i <= INSERT_RUN_MAX {
                for k in i..j {
                    let Ok(s) = self.acquire_slot() else { break };
                    self.slots[s]
                        .data
                        .copy_from_slice(&out[k * BLOCK_SIZE..(k + 1) * BLOCK_SIZE]);
                    self.install(s, blk + k as u64, false);
                }
            }
            i = j;
        }
        Ok(())
    }

    /// Write `data.len() / 4096` consecutive blocks starting at `blk`. Long runs
    /// go straight to the device (not durable until [`flush`](Self::flush));
    /// short ones are cached write-back.
    pub fn write_many(&mut self, blk: u64, data: &[u8]) -> Result<(), IoError> {
        if !data.len().is_multiple_of(BLOCK_SIZE) {
            return Err(IoError::BadLength);
        }
        let n = data.len() / BLOCK_SIZE;
        self.check(blk, n as u64)?;
        if n >= DIRECT_WRITE_MIN {
            let lba = self.lba(blk);
            if let Err(e) = self.dev.write_sectors(lba, data) {
                // The device may hold a torn mix: forget the cached copies.
                for k in 0..n {
                    self.discard(blk + k as u64);
                }
                return Err(e);
            }
            self.dev_dirty = true;
            self.stats.sectors_written += n as u64 * SECTORS_PER_BLOCK;
            for k in 0..n {
                if let Some(&s) = self.map.get(&(blk + k as u64)) {
                    self.slots[s]
                        .data
                        .copy_from_slice(&data[k * BLOCK_SIZE..(k + 1) * BLOCK_SIZE]);
                    self.slots[s].dirty = false;
                    self.slots[s].tag = 0;
                }
            }
            return Ok(());
        }
        for k in 0..n {
            let mut b: Block = [0u8; BLOCK_SIZE];
            b.copy_from_slice(&data[k * BLOCK_SIZE..(k + 1) * BLOCK_SIZE]);
            self.write(blk + k as u64, &b)?;
        }
        Ok(())
    }

    /// Write every dirty block to the device (adjacent blocks in one transfer)
    /// and then issue the device barrier. On error, blocks not yet written stay
    /// dirty and a repeated `flush` retries them.
    pub fn flush(&mut self) -> Result<(), IoError> {
        let dirty: Vec<(u64, usize)> = self
            .map
            .iter()
            .filter(|&(_, &i)| self.slots[i].dirty)
            .map(|(&b, &i)| (b, i))
            .collect();
        let mut buf: Vec<u8> = Vec::new();
        let mut first_err = None;
        let mut k = 0;
        while k < dirty.len() {
            let mut m = 1;
            while k + m < dirty.len() && m < FLUSH_RUN && dirty[k + m].0 == dirty[k].0 + m as u64 {
                m += 1;
            }
            buf.clear();
            for &(_, i) in &dirty[k..k + m] {
                buf.extend_from_slice(&self.slots[i].data[..]);
            }
            let lba = self.lba(dirty[k].0);
            match self.dev.write_sectors(lba, &buf) {
                Ok(()) => {
                    self.dev_dirty = true;
                    self.stats.writebacks += m as u64;
                    self.stats.sectors_written += m as u64 * SECTORS_PER_BLOCK;
                    for &(_, i) in &dirty[k..k + m] {
                        self.slots[i].dirty = false;
                    }
                }
                // Keep going: the other runs should still reach the disk. The
                // failed run stays dirty and the first error is reported.
                Err(e) => first_err = first_err.or(Some(e)),
            }
            k += m;
        }
        if let Some(e) = first_err {
            return Err(e);
        }
        if self.dev_dirty {
            self.dev.flush()?;
            self.dev_dirty = false;
            self.stats.flushes += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
}
