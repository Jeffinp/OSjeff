//! The entropy pool: a SHA-256 accumulator plus honest bookkeeping.
//!
//! Everything a source delivers goes through [`Pool::add`] / [`Pool::add_milli`]:
//! the bytes are hashed in (with the source id and length, so two sources cannot
//! be confused) and the caller's *estimate* of how many bits of real
//! unpredictability they carried is recorded. The estimate is a claim, not a
//! measurement; the pool only enforces that it can never exceed the size of the
//! data (`8 * len` bits) and keeps per-source totals so the health of each source
//! can be inspected.
//!
//! Credit is tracked in **millibits** because a timing sample is worth well
//! under one bit. It is split in two classes:
//!
//! * **hardware** sources (`RDSEED`, `RDRAND`, virtio-rng): a generator outside
//!   our control that is trusted to produce full-entropy bytes;
//! * **timing** sources (interrupt jitter, ...): measured physical noise at a
//!   very conservative rate.
//!
//! [`Pool::drain`] turns the accumulated hash into a 256-bit seed and reports how
//! many bits of each class were credited to it; leftover credit beyond 256 bits
//! stays in the pool (the entropy is carried by the hash that is kept).

use sha2::{Digest, Sha256};

/// Size of the per-source health table; ids at or above it share the last slot.
pub const MAX_SOURCES: usize = 16;
/// A seed carries at most this many credited bits (the DRBG key size).
pub const SEED_BITS: u32 = 256;
/// Credit saturates here (millibits) so a flood cannot overflow the counters.
const PENDING_CAP_MBITS: u64 = 4096 * 1000;

/// Lifetime counters of one source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SourceHealth {
    /// Calls that delivered data.
    pub events: u64,
    /// Bytes hashed into the pool.
    pub bytes_in: u64,
    /// Millibits credited (after capping at `8 * len` per event).
    pub credited_mbits: u64,
    /// Samples the source's own health test refused to credit (stuck timers...).
    pub rejected: u64,
}

impl SourceHealth {
    /// Credited whole bits.
    pub fn credited_bits(&self) -> u64 {
        self.credited_mbits / 1000
    }
}

/// What [`Pool::drain`] returns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seed {
    /// 256-bit seed material for [`super::Drbg`].
    pub key: [u8; 32],
    /// Credited bits that came from hardware sources (<= 256).
    pub hw_bits: u32,
    /// Credited bits that came from timing sources (<= 256 - `hw_bits`).
    pub timing_bits: u32,
}

impl Seed {
    /// Total credited bits.
    pub fn bits(&self) -> u32 {
        self.hw_bits + self.timing_bits
    }
}

/// The accumulator.
pub struct Pool {
    hash: Sha256,
    health: [SourceHealth; MAX_SOURCES],
    pending_hw_mbits: u64,
    pending_timing_mbits: u64,
    drains: u64,
}

impl Default for Pool {
    fn default() -> Self {
        Self::new()
    }
}

impl Pool {
    /// An empty pool with no credit.
    pub fn new() -> Pool {
        let mut hash = Sha256::new();
        hash.update(b"osjeff-pool-v1");
        Pool {
            hash,
            health: [SourceHealth {
                events: 0,
                bytes_in: 0,
                credited_mbits: 0,
                rejected: 0,
            }; MAX_SOURCES],
            pending_hw_mbits: 0,
            pending_timing_mbits: 0,
            drains: 0,
        }
    }

    fn slot(source_id: u8) -> usize {
        usize::from(source_id).min(MAX_SOURCES - 1)
    }

    /// Hash `bytes` in and credit `estimated_bits` whole bits (capped at
    /// `8 * bytes.len()`).
    pub fn add(&mut self, source_id: u8, bytes: &[u8], estimated_bits: u32) {
        self.add_milli(source_id, bytes, estimated_bits.saturating_mul(1000));
    }

    /// Like [`Pool::add`] with the estimate in millibits.
    pub fn add_milli(&mut self, source_id: u8, bytes: &[u8], estimated_mbits: u32) {
        let cap = (bytes.len() as u64).saturating_mul(8000);
        let credit = u64::from(estimated_mbits).min(cap);
        self.hash.update([source_id]);
        self.hash.update((bytes.len() as u32).to_le_bytes());
        self.hash.update(bytes);
        let h = &mut self.health[Self::slot(source_id)];
        h.events += 1;
        h.bytes_in = h.bytes_in.saturating_add(bytes.len() as u64);
        h.credited_mbits = h.credited_mbits.saturating_add(credit);
        let pending = if super::source::is_hardware(source_id) {
            &mut self.pending_hw_mbits
        } else {
            &mut self.pending_timing_mbits
        };
        *pending = pending.saturating_add(credit).min(PENDING_CAP_MBITS);
    }

    /// Record that a source's own health test refused a sample.
    pub fn note_rejected(&mut self, source_id: u8) {
        let h = &mut self.health[Self::slot(source_id)];
        h.rejected = h.rejected.saturating_add(1);
    }

    /// Credit waiting for the next [`Pool::drain`], in whole bits (hardware, timing).
    pub fn pending_bits(&self) -> (u32, u32) {
        (
            (self.pending_hw_mbits / 1000) as u32,
            (self.pending_timing_mbits / 1000) as u32,
        )
    }

    /// Per-source lifetime counters.
    pub fn health(&self) -> &[SourceHealth; MAX_SOURCES] {
        &self.health
    }

    /// Finish the current accumulation: derive a seed from everything added so
    /// far and start a new accumulation that carries the old digest. Credit up to
    /// [`SEED_BITS`] moves into the returned [`Seed`] (hardware first); the rest
    /// stays pending.
    pub fn drain(&mut self) -> Seed {
        self.drains += 1;
        let digest: [u8; 32] = core::mem::replace(&mut self.hash, Sha256::new())
            .finalize()
            .into();
        // The new accumulation starts from the old digest, so credit left over stays backed by it.
        self.hash.update(b"osjeff-pool-v1-carry");
        self.hash.update(digest);
        let mut h = Sha256::new();
        h.update(b"osjeff-pool-v1-seed");
        h.update(digest);
        h.update(self.drains.to_le_bytes());
        let key: [u8; 32] = h.finalize().into();

        let cap = u64::from(SEED_BITS) * 1000;
        let hw = self.pending_hw_mbits.min(cap);
        let timing = self.pending_timing_mbits.min(cap - hw);
        self.pending_hw_mbits -= hw;
        self.pending_timing_mbits -= timing;
        Seed {
            key,
            hw_bits: (hw / 1000) as u32,
            timing_bits: (timing / 1000) as u32,
        }
    }
}
