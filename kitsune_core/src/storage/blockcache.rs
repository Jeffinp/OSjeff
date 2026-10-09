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
mod tests;
