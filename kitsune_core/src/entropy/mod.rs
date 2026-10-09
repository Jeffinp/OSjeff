//! The entropy subsystem: sources feed a [`Pool`], the pool seeds a ChaCha20
//! [`Drbg`], and [`Entropy`] ties them together with an honest [`Quality`] rating.
//!
//! ```text
//!   RDSEED / RDRAND / virtio-rng ──┐ (hardware class, full credit)
//!   timer, PS/2, NIC, active jitter ┼─► Pool (SHA-256, credit per source)
//!   RTC, boot TSC (no credit) ─────┘            │ drain() when credit >= 128 bits
//!                                              ▼
//!                                   Drbg (ChaCha20, fast key erasure) ─► fill()
//! ```
//!
//! This module is pure: no I/O, no clock, no globals. The kernel (`kernel/src/rng.rs`)
//! owns the hardware glue and the IRQ-safe sample ring, and feeds this code from
//! thread context. Design and threat discussion: `docs/design/entropy.md`.
//!
//! # Quality
//!
//! * [`Quality::Strong`]: a hardware generator (RDSEED, RDRAND or virtio-rng) put at
//!   least [`MIN_SEED_BITS`] credited bits into the DRBG key.
//! * [`Quality::Mixed`]: no hardware generator, but at least [`MIN_SEED_BITS`]
//!   credited bits of timing noise. Good against an attacker who cannot see the
//!   machine's clocks; **not** a promise against one who can, and in a fully
//!   deterministic virtual machine the real entropy can be zero while the
//!   estimator, which only sees timestamps, cannot know. That is why timing alone
//!   never reaches `Strong`, however much it credits.
//! * [`Quality::Weak`]: less than that. The DRBG still produces bytes (they are
//!   unique per boot when the boot noise differs) but nothing secret may rely on them.

pub mod chacha;
pub mod drbg;
pub mod jitter;
pub mod pool;

#[cfg(test)]
mod tests;

pub use drbg::Drbg;
pub use jitter::{TimingEstimator, Verdict};
pub use pool::{MAX_SOURCES, Pool, Seed, SourceHealth};

/// Credited bits the DRBG key needs before the generator leaves `Weak`.
pub const MIN_SEED_BITS: u32 = 128;
/// Minimum time between periodic reseeds once the generator is seeded.
pub const RESEED_INTERVAL_MS: u64 = 60_000;

/// Source ids for [`Pool::add`] and [`Entropy::sample`].
pub mod source {
    /// `RDSEED` (hardware, full credit).
    pub const RDSEED: u8 = 0;
    /// `RDRAND` (hardware, full credit).
    pub const RDRAND: u8 = 1;
    /// virtio-rng (host entropy, full credit).
    pub const VIRTIO_RNG: u8 = 2;
    /// Timer interrupt (IRQ0) timestamps.
    pub const TIMER: u8 = 3;
    /// PS/2 keyboard interrupt timestamps.
    pub const KEYBOARD: u8 = 4;
    /// PS/2 mouse interrupt timestamps.
    pub const MOUSE: u8 = 5;
    /// Frame arrival timestamps from the NIC.
    pub const NIC: u8 = 6;
    /// Active CPU execution-time jitter, collected while a caller waits.
    pub const ACTIVE_JITTER: u8 = 7;
    /// RTC seconds (known, mixed for uniqueness, never credited).
    pub const RTC: u8 = 8;
    /// TSC at boot and other one-off values (mixed, never credited).
    pub const BOOT: u8 = 9;

    /// Is `id` a hardware generator (full-credit class)?
    pub const fn is_hardware(id: u8) -> bool {
        id <= VIRTIO_RNG
    }

    /// Short name for logs.
    pub const fn name(id: u8) -> &'static str {
        match id {
            RDSEED => "rdseed",
            RDRAND => "rdrand",
            VIRTIO_RNG => "virtio-rng",
            TIMER => "timer",
            KEYBOARD => "keyboard",
            MOUSE => "mouse",
            NIC => "nic",
            ACTIVE_JITTER => "cpu-jitter",
            RTC => "rtc",
            BOOT => "boot",
            _ => "other",
        }
    }
}

/// How much the output can be trusted (see the module docs). Ordered:
/// `Weak < Mixed < Strong`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Quality {
    Weak,
    Mixed,
    Strong,
}

impl Quality {
    /// Lower-case name for logs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Quality::Weak => "weak",
            Quality::Mixed => "mixed",
            Quality::Strong => "strong",
        }
    }

    /// Rating for a DRBG key that received `hw_bits` of hardware credit and
    /// `timing_bits` of timing credit.
    pub const fn of(hw_bits: u32, timing_bits: u32) -> Quality {
        if hw_bits >= MIN_SEED_BITS {
            Quality::Strong
        } else if hw_bits.saturating_add(timing_bits) >= MIN_SEED_BITS {
            Quality::Mixed
        } else {
            Quality::Weak
        }
    }
}

/// Per-source timing estimators for the timestamps that [`Entropy::sample`] takes.
struct Collector {
    est: [TimingEstimator; MAX_SOURCES],
}

impl Collector {
    const fn new() -> Collector {
        let mut est = [TimingEstimator::new(jitter::IRQ_MILLIBITS); MAX_SOURCES];
        est[source::ACTIVE_JITTER as usize] = TimingEstimator::new(jitter::EXEC_MILLIBITS);
        Collector { est }
    }
}

/// Pool + DRBG + reseed policy.
pub struct Entropy {
    pool: Pool,
    drbg: Drbg,
    col: Collector,
    /// Credit the DRBG key has received so far (each saturating at 256).
    hw_bits: u32,
    timing_bits: u32,
    last_reseed_ms: u64,
}

impl Entropy {
    /// Start unseeded: `boot_noise` (TSC, RTC, ...) is hashed in with no credit and keys
    /// the DRBG so output differs between boots, but the rating is [`Quality::Weak`].
    pub fn new(boot_noise: &[u8]) -> Entropy {
        let mut pool = Pool::new();
        pool.add(source::BOOT, boot_noise, 0);
        let seed = pool.drain();
        Entropy {
            drbg: Drbg::new(&seed.key),
            pool,
            col: Collector::new(),
            hw_bits: 0,
            timing_bits: 0,
            last_reseed_ms: 0,
        }
    }

    /// Hash `bytes` in, crediting `estimated_bits` (see [`Pool::add`]).
    pub fn add(&mut self, source_id: u8, bytes: &[u8], estimated_bits: u32) {
        self.pool.add(source_id, bytes, estimated_bits);
    }

    /// Feed one timestamp from a timing source: it is always mixed in, and credited
    /// only if the source's health tests pass. Returns the verdict.
    pub fn sample(&mut self, source_id: u8, ts: u64) -> Verdict {
        let slot = usize::from(source_id).min(MAX_SOURCES - 1);
        let v = self.col.est[slot].observe(ts);
        match v {
            Verdict::Accepted(mbits) => {
                self.pool.add_milli(source_id, &ts.to_le_bytes(), mbits);
            }
            Verdict::Rejected => {
                self.pool.add_milli(source_id, &ts.to_le_bytes(), 0);
                self.pool.note_rejected(source_id);
            }
        }
        v
    }

    /// Current rating of the DRBG key.
    pub fn quality(&self) -> Quality {
        Quality::of(self.hw_bits, self.timing_bits)
    }

    /// Credit the DRBG key has received `(hardware, timing)`.
    pub fn seeded_bits(&self) -> (u32, u32) {
        (self.hw_bits, self.timing_bits)
    }

    /// Credit waiting in the pool `(hardware, timing)`.
    pub fn pending_bits(&self) -> (u32, u32) {
        self.pool.pending_bits()
    }

    /// Per-source counters.
    pub fn health(&self) -> &[SourceHealth; MAX_SOURCES] {
        self.pool.health()
    }

    /// How many times the DRBG was reseeded from the pool.
    pub fn reseeds(&self) -> u64 {
        self.drbg.reseeds()
    }

    /// Reseed the DRBG from the pool if the policy says so; returns whether it did.
    ///
    /// * Until the key holds [`MIN_SEED_BITS`] credited bits, as soon as the pool has that
    ///   much pending (so the first reseed is as early as the credit allows, and a smaller
    ///   pile keeps accumulating instead of being spent).
    /// * Afterwards, every [`RESEED_INTERVAL_MS`] if the pool has [`MIN_SEED_BITS`] pending
    ///   (at once if that pending credit is hardware and the key has none), and whenever the DRBG asks for it ([`Drbg::needs_reseed`]), even with less credit:
    ///   mixing never makes the state worse.
    pub fn maybe_reseed(&mut self, now_ms: u64) -> bool {
        let (ph, pt) = self.pool.pending_bits();
        let pending = ph.saturating_add(pt);
        let due = if self.quality() == Quality::Weak {
            pending >= MIN_SEED_BITS
        } else {
            // A hardware generator that shows up late upgrades the rating at once.
            (ph >= MIN_SEED_BITS && self.hw_bits < MIN_SEED_BITS)
                || (pending >= MIN_SEED_BITS
                    && now_ms.saturating_sub(self.last_reseed_ms) >= RESEED_INTERVAL_MS)
                || self.drbg.needs_reseed()
        };
        if !due {
            return false;
        }
        let seed = self.pool.drain();
        self.drbg.reseed(&seed.key);
        self.hw_bits = (self.hw_bits + seed.hw_bits).min(pool::SEED_BITS);
        self.timing_bits = (self.timing_bits + seed.timing_bits).min(pool::SEED_BITS);
        self.last_reseed_ms = now_ms;
        true
    }

    /// Fill `out` from the DRBG, reseeding first if due.
    pub fn fill(&mut self, out: &mut [u8], now_ms: u64) {
        self.maybe_reseed(now_ms);
        self.drbg.fill(out);
    }
}
