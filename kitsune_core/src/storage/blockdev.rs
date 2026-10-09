//! Block-device abstraction: 512-byte sectors, flush barrier, and two host-side
//! implementations used to test storage code ([`RamDisk`] and [`FaultyDisk`]).
//!
//! The kernel implements [`BlockDevice`] over its ATA driver; everything above
//! (block cache, OJFS v3) only sees this trait, so the same code runs against a
//! `Vec<u8>` on the host. [`FaultyDisk`] is how the filesystem's power-loss
//! consistency is *proved*: it cuts the power after exactly N events (sector
//! writes and flushes), optionally modelling a volatile write cache in which
//! only a pseudo-random subset of the not-yet-flushed sectors survives.

use alloc::vec::Vec;

/// Sector size in bytes.
pub const SECTOR_SIZE: usize = 512;

/// Why a device operation failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IoError {
    /// The sector range lies (partly) beyond the end of the device.
    OutOfRange,
    /// The buffer length is not a multiple of [`SECTOR_SIZE`].
    BadLength,
    /// The device reported a read error.
    Read,
    /// The device reported a write error.
    Write,
    /// The cache flush failed.
    Flush,
    /// The power was cut (only produced by [`FaultyDisk`]).
    PowerLoss,
}

/// A device addressed in 512-byte sectors.
///
/// Contract: `read_sectors`/`write_sectors` take a buffer whose length is a
/// multiple of [`SECTOR_SIZE`]; a successful write is not durable until a
/// later successful [`flush`](BlockDevice::flush), and writes issued between
/// two flushes may reach the medium in any order (a sector write itself is
/// atomic).
pub trait BlockDevice {
    /// Number of sectors.
    fn sector_count(&self) -> u64;
    /// Read `buf.len() / 512` sectors starting at `lba`.
    fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), IoError>;
    /// Write `buf.len() / 512` sectors starting at `lba`.
    fn write_sectors(&mut self, lba: u64, buf: &[u8]) -> Result<(), IoError>;
    /// Barrier: everything written before is durable once this returns `Ok`.
    fn flush(&mut self) -> Result<(), IoError>;
}

impl<T: BlockDevice + ?Sized> BlockDevice for &mut T {
    fn sector_count(&self) -> u64 {
        (**self).sector_count()
    }
    fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), IoError> {
        (**self).read_sectors(lba, buf)
    }
    fn write_sectors(&mut self, lba: u64, buf: &[u8]) -> Result<(), IoError> {
        (**self).write_sectors(lba, buf)
    }
    fn flush(&mut self) -> Result<(), IoError> {
        (**self).flush()
    }
}

/// Validate a transfer and return its byte offset and sector count.
fn check_range(sectors: u64, lba: u64, len: usize) -> Result<(usize, u64), IoError> {
    if !len.is_multiple_of(SECTOR_SIZE) {
        return Err(IoError::BadLength);
    }
    let n = (len / SECTOR_SIZE) as u64;
    let end = lba.checked_add(n).ok_or(IoError::OutOfRange)?;
    if end > sectors {
        return Err(IoError::OutOfRange);
    }
    let off = lba
        .checked_mul(SECTOR_SIZE as u64)
        .and_then(|o| usize::try_from(o).ok())
        .ok_or(IoError::OutOfRange)?;
    Ok((off, n))
}

/// I/O counters kept by [`RamDisk`] (used to measure write amplification).
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct IoCounters {
    pub read_calls: u64,
    pub write_calls: u64,
    pub sectors_read: u64,
    pub sectors_written: u64,
    pub flushes: u64,
}

/// A device backed by a `Vec<u8>`.
#[derive(Clone, Debug)]
pub struct RamDisk {
    data: Vec<u8>,
    counters: IoCounters,
    /// Lowest LBA ever written (for "never touches the reserved area" tests).
    min_written_lba: Option<u64>,
}

impl RamDisk {
    /// A zero-filled disk of `sectors` sectors.
    pub fn new(sectors: u64) -> Self {
        RamDisk {
            data: alloc::vec![0u8; (sectors as usize) * SECTOR_SIZE],
            counters: IoCounters::default(),
            min_written_lba: None,
        }
    }

    /// A disk holding `bytes`, zero-padded up to a whole number of sectors.
    pub fn from_bytes(mut bytes: Vec<u8>) -> Self {
        let rem = bytes.len() % SECTOR_SIZE;
        if rem != 0 {
            bytes.resize(bytes.len() + SECTOR_SIZE - rem, 0);
        }
        RamDisk {
            data: bytes,
            counters: IoCounters::default(),
            min_written_lba: None,
        }
    }

    /// The raw contents.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Mutable raw contents (to corrupt an image in tests; bypasses counters).
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Consume the disk and return its bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }

    /// I/O counters since creation (or the last [`reset_counters`](Self::reset_counters)).
    pub fn counters(&self) -> IoCounters {
        self.counters
    }

    /// Zero the counters (and the lowest-written-LBA mark).
    pub fn reset_counters(&mut self) {
        self.counters = IoCounters::default();
        self.min_written_lba = None;
    }

    /// Lowest LBA written since creation, if any write happened.
    pub fn min_written_lba(&self) -> Option<u64> {
        self.min_written_lba
    }
}

impl BlockDevice for RamDisk {
    fn sector_count(&self) -> u64 {
        (self.data.len() / SECTOR_SIZE) as u64
    }

    fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), IoError> {
        let (off, n) = check_range(self.sector_count(), lba, buf.len())?;
        buf.copy_from_slice(&self.data[off..off + buf.len()]);
        self.counters.read_calls += 1;
        self.counters.sectors_read += n;
        Ok(())
    }

    fn write_sectors(&mut self, lba: u64, buf: &[u8]) -> Result<(), IoError> {
        let (off, n) = check_range(self.sector_count(), lba, buf.len())?;
        self.data[off..off + buf.len()].copy_from_slice(buf);
        self.counters.write_calls += 1;
        self.counters.sectors_written += n;
        if n > 0 {
            self.min_written_lba = Some(self.min_written_lba.map_or(lba, |m| m.min(lba)));
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), IoError> {
        self.counters.flushes += 1;
        Ok(())
    }
}

/// How [`FaultyDisk`] treats writes that were not yet flushed when the power
/// is cut.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrashMode {
    /// Writes reach the medium immediately and in order; the cut simply stops
    /// them. Models a disk with no reordering.
    InOrder,
    /// Writes since the last flush sit in a volatile cache; at the cut each
    /// pending sector independently survives or is lost, chosen by a PRNG
    /// seeded with this value. This is the adversarial model: it is what
    /// proves the flush barriers are in the right places.
    Lossy(u64),
}

/// A wrapper that injects faults into any [`BlockDevice`].
///
/// *Events* are sector writes and flushes, counted in issue order. With
/// [`crash_after`](FaultyDisk::crash_after)`(k)` the first `k` events succeed
/// and the power is cut at event `k`: that event and every later one fails
/// with [`IoError::PowerLoss`] (reads too). [`into_inner`](FaultyDisk::into_inner)
/// then returns the surviving medium, i.e. "power back on".
///
/// Independently, individual reads/writes can be made to fail transiently
/// (`fail_read_nth`/`fail_write_nth`: the n-th call, counted from 0) or
/// persistently over an LBA range (`bad_read_range`/`bad_write_range`).
pub struct FaultyDisk<D: BlockDevice> {
    inner: D,
    mode: CrashMode,
    crash_at: Option<u64>,
    events: u64,
    crashed: bool,
    /// Unflushed sector writes (Lossy mode only), oldest first.
    pending: Vec<(u64, [u8; SECTOR_SIZE])>,
    rng: u64,
    read_calls: u64,
    write_calls: u64,
    fail_read_nth: Option<u64>,
    fail_write_nth: Option<u64>,
    bad_read: Option<(u64, u64)>,
    bad_write: Option<(u64, u64)>,
}

impl<D: BlockDevice> FaultyDisk<D> {
    /// Wrap `inner` with no fault configured.
    pub fn new(inner: D) -> Self {
        FaultyDisk {
            inner,
            mode: CrashMode::InOrder,
            crash_at: None,
            events: 0,
            crashed: false,
            pending: Vec::new(),
            rng: 0,
            read_calls: 0,
            write_calls: 0,
            fail_read_nth: None,
            fail_write_nth: None,
            bad_read: None,
            bad_write: None,
        }
    }

    /// Choose the crash model (default [`CrashMode::InOrder`]).
    pub fn with_mode(mut self, mode: CrashMode) -> Self {
        self.mode = mode;
        if let CrashMode::Lossy(seed) = mode {
            self.rng = seed ^ 0x9E37_79B9_7F4A_7C15;
        }
        self
    }

    /// Cut the power at event number `events` (0 = before the first event).
    pub fn crash_after(mut self, events: u64) -> Self {
        self.crash_at = Some(events);
        self
    }

    /// Make the `n`-th `read_sectors` call (0-based) fail once.
    pub fn fail_read_nth(mut self, n: u64) -> Self {
        self.fail_read_nth = Some(n);
        self
    }

    /// Make the `n`-th `write_sectors` call (0-based) fail once.
    pub fn fail_write_nth(mut self, n: u64) -> Self {
        self.fail_write_nth = Some(n);
        self
    }

    /// Reads touching `[lo, hi)` always fail.
    pub fn bad_read_range(mut self, lo: u64, hi: u64) -> Self {
        self.bad_read = Some((lo, hi));
        self
    }

    /// Writes touching `[lo, hi)` always fail.
    pub fn bad_write_range(mut self, lo: u64, hi: u64) -> Self {
        self.bad_write = Some((lo, hi));
        self
    }

    /// Fail the `n`-th `read_sectors` call (0-based, counted since creation);
    /// `None` clears it. Unlike [`fail_read_nth`](Self::fail_read_nth) this
    /// works on a disk that is already in use.
    pub fn set_fail_read_at(&mut self, n: Option<u64>) {
        self.fail_read_nth = n;
    }

    /// Like [`set_fail_read_at`](Self::set_fail_read_at) for `write_sectors`.
    pub fn set_fail_write_at(&mut self, n: Option<u64>) {
        self.fail_write_nth = n;
    }

    /// `read_sectors` calls so far.
    pub fn read_calls(&self) -> u64 {
        self.read_calls
    }

    /// `write_sectors` calls so far.
    pub fn write_calls(&self) -> u64 {
        self.write_calls
    }

    /// Clear the persistent bad ranges and transient failures.
    pub fn heal(&mut self) {
        self.bad_read = None;
        self.bad_write = None;
        self.fail_read_nth = None;
        self.fail_write_nth = None;
    }

    /// Events (sector writes + flushes) seen so far.
    pub fn events(&self) -> u64 {
        self.events
    }

    /// True once the power cut happened.
    pub fn crashed(&self) -> bool {
        self.crashed
    }

    /// Borrow the wrapped device (does not resolve pending writes).
    pub fn inner(&self) -> &D {
        &self.inner
    }

    fn next_rand(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.rng | 1;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Apply every pending (unflushed) write to the medium.
    fn apply_all_pending(&mut self) {
        let pending = core::mem::take(&mut self.pending);
        for (lba, sec) in pending {
            let _ = self.inner.write_sectors(lba, &sec);
        }
    }

    /// Resolve the pending writes at the instant of the cut: each survives with
    /// probability 1/2.
    fn resolve_crash(&mut self) {
        let pending = core::mem::take(&mut self.pending);
        for (lba, sec) in pending {
            if self.next_rand() & 1 == 1 {
                let _ = self.inner.write_sectors(lba, &sec);
            }
        }
    }

    /// Count one event; returns `Err` if the power is (now) cut.
    fn tick(&mut self) -> Result<(), IoError> {
        if self.crashed {
            return Err(IoError::PowerLoss);
        }
        if self.crash_at == Some(self.events) {
            self.crashed = true;
            self.resolve_crash();
            return Err(IoError::PowerLoss);
        }
        self.events += 1;
        Ok(())
    }

    /// "Power back on": resolve any pending writes if the cut did not happen
    /// yet (a clean shutdown flushes), and return the medium.
    pub fn into_inner(mut self) -> D {
        if !self.crashed {
            self.apply_all_pending();
        }
        self.inner
    }
}

fn overlaps(range: Option<(u64, u64)>, lba: u64, n: u64) -> bool {
    match range {
        Some((lo, hi)) => lba < hi && lba.saturating_add(n) > lo,
        None => false,
    }
}

impl<D: BlockDevice> BlockDevice for FaultyDisk<D> {
    fn sector_count(&self) -> u64 {
        self.inner.sector_count()
    }

    fn read_sectors(&mut self, lba: u64, buf: &mut [u8]) -> Result<(), IoError> {
        if self.crashed {
            return Err(IoError::PowerLoss);
        }
        let call = self.read_calls;
        self.read_calls += 1;
        let (_, n) = check_range(self.sector_count(), lba, buf.len())?;
        if self.fail_read_nth == Some(call) || overlaps(self.bad_read, lba, n) {
            return Err(IoError::Read);
        }
        self.inner.read_sectors(lba, buf)?;
        // Read-your-writes: overlay sectors still sitting in the volatile cache.
        for (plba, sec) in &self.pending {
            if *plba >= lba && *plba < lba + n {
                let o = ((*plba - lba) as usize) * SECTOR_SIZE;
                buf[o..o + SECTOR_SIZE].copy_from_slice(sec);
            }
        }
        Ok(())
    }

    fn write_sectors(&mut self, lba: u64, buf: &[u8]) -> Result<(), IoError> {
        if self.crashed {
            return Err(IoError::PowerLoss);
        }
        let call = self.write_calls;
        self.write_calls += 1;
        let (_, n) = check_range(self.sector_count(), lba, buf.len())?;
        if self.fail_write_nth == Some(call) || overlaps(self.bad_write, lba, n) {
            return Err(IoError::Write);
        }
        // Each sector is its own event: the cut can land in the middle of a
        // multi-sector write, leaving it torn at sector granularity.
        for i in 0..n {
            self.tick()?;
            let sec = &buf[(i as usize) * SECTOR_SIZE..(i as usize + 1) * SECTOR_SIZE];
            match self.mode {
                CrashMode::InOrder => self.inner.write_sectors(lba + i, sec)?,
                CrashMode::Lossy(_) => {
                    let mut a = [0u8; SECTOR_SIZE];
                    a.copy_from_slice(sec);
                    self.pending.push((lba + i, a));
                }
            }
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), IoError> {
        self.tick()?;
        self.apply_all_pending();
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests;
