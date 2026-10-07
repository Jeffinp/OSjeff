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

    /// Zero the counters.
    pub fn reset_counters(&mut self) {
        self.counters = IoCounters::default();
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
mod tests {
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
}
