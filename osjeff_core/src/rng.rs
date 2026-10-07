//! Pure pieces of the kernel's TLS random-number source: CPUID detection of
//! the hardware generator, the bounded retry around `RDRAND`, and the
//! best-effort mixer used when no hardware generator exists.
//!
//! The kernel owns the hardware glue (`CPUID`, `RDRAND`, TSC/PIT/RTC reads);
//! the decisions live here so they are host-tested.
//!
//! **Security note.** Only [`retry_hw`] output (from `RDRAND`) is
//! cryptographic-grade. [`WeakMixer`] hashes whatever timing noise the caller
//! feeds it; it removes the *trivial* predictability of a bare TSC xorshift
//! but is NOT a CSPRNG and must never be described as one.

/// How many times a failed `RDRAND` is retried before giving up (Intel's
/// software guidance is 10).
pub const RDRAND_RETRIES: usize = 10;

/// `CPUID.01H:ECX` bit 30: the `RDRAND` instruction is available.
pub const CPUID_ECX_RDRAND: u32 = 1 << 30;

/// Does `CPUID.01H:ECX` advertise `RDRAND`?
pub fn has_rdrand(cpuid_01h_ecx: u32) -> bool {
    cpuid_01h_ecx & CPUID_ECX_RDRAND != 0
}

/// Call `step` (one `RDRAND` attempt: `Some` on success, `None` when the
/// hardware reported "not ready") up to [`RDRAND_RETRIES`] times and return
/// the first success. `None` means the hardware generator is unusable right
/// now and the caller must not pretend it produced entropy.
pub fn retry_hw(mut step: impl FnMut() -> Option<u64>) -> Option<u64> {
    for _ in 0..RDRAND_RETRIES {
        if let Some(v) = step() {
            return Some(v);
        }
    }
    None
}

/// SplitMix64 finalizer: a fast bijective bit mixer (every input bit affects
/// every output bit).
pub fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Best-effort generator for machines without `RDRAND`. Absorbs caller
/// supplied noise (TSC, timer ticks, RTC...) into a 64-bit state and emits
/// mixed output. NOT cryptographically secure: the state is only as
/// unpredictable as the noise, and 64 bits is small.
pub struct WeakMixer {
    state: u64,
}

impl WeakMixer {
    /// Seed from a few independent noise words.
    pub fn new(noise: &[u64]) -> Self {
        let mut state = 0x9E37_79B9_7F4A_7C15;
        for &n in noise {
            state = mix64(state ^ n).wrapping_add(0x9E37_79B9_7F4A_7C15);
        }
        WeakMixer { state }
    }

    /// Mix in fresh `noise` (e.g. the TSC at this instant) and return the next
    /// 64-bit word.
    pub fn next(&mut self, noise: u64) -> u64 {
        self.state = mix64(self.state ^ noise).wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix64(self.state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpuid_bit_30_means_rdrand() {
        assert!(has_rdrand(1 << 30));
        assert!(has_rdrand(0xFFFF_FFFF));
        assert!(!has_rdrand(0));
        assert!(!has_rdrand(!(1u32 << 30)));
        assert_eq!(CPUID_ECX_RDRAND, 0x4000_0000);
    }

    #[test]
    fn retry_hw_returns_first_success() {
        let mut calls = 0;
        let v = retry_hw(|| {
            calls += 1;
            Some(42)
        });
        assert_eq!(v, Some(42));
        assert_eq!(calls, 1);
    }

    #[test]
    fn retry_hw_retries_up_to_the_limit_then_gives_up() {
        // Succeeds on the last allowed attempt.
        let mut calls = 0;
        let v = retry_hw(|| {
            calls += 1;
            (calls == RDRAND_RETRIES).then_some(7)
        });
        assert_eq!(v, Some(7));
        assert_eq!(calls, RDRAND_RETRIES);
        // One attempt too late: gives up after exactly RDRAND_RETRIES tries.
        let mut calls = 0;
        let v = retry_hw(|| {
            calls += 1;
            (calls == RDRAND_RETRIES + 1).then_some(7)
        });
        assert_eq!(v, None);
        assert_eq!(calls, RDRAND_RETRIES);
        // A hardware that never succeeds is reported as failure, not as 0.
        assert_eq!(retry_hw(|| None), None);
    }

    #[test]
    fn mix64_is_a_bijection_with_avalanche() {
        assert_ne!(mix64(0), mix64(1));
        // Flipping one input bit flips about half of the output bits.
        for bit in 0..64 {
            let d = (mix64(0x1234_5678_9ABC_DEF0) ^ mix64(0x1234_5678_9ABC_DEF0 ^ (1 << bit)))
                .count_ones();
            assert!((16..=48).contains(&d), "bit {bit}: {d} bits changed");
        }
    }

    #[test]
    fn weak_mixer_depends_on_seed_and_noise() {
        let mut a = WeakMixer::new(&[1, 2, 3]);
        let mut b = WeakMixer::new(&[1, 2, 4]);
        assert_ne!(a.next(0), b.next(0));
        // Same seed and same noise stream is deterministic (it is only a mixer).
        let mut c = WeakMixer::new(&[9]);
        let mut d = WeakMixer::new(&[9]);
        assert_eq!(c.next(5), d.next(5));
        // Different noise at the same step diverges.
        assert_ne!(c.next(1), d.next(2));
    }

    #[test]
    fn weak_mixer_output_is_not_stuck_or_biased() {
        // Even with constant noise the output keeps moving and bits balance.
        let mut m = WeakMixer::new(&[0]);
        let mut ones = 0u32;
        let mut prev = m.next(0);
        for _ in 0..4096 {
            let v = m.next(0);
            assert_ne!(v, prev);
            ones += v.count_ones();
            prev = v;
        }
        let avg = ones as f64 / 4096.0;
        assert!((31.0..=33.0).contains(&avg), "mean popcount {avg}");
    }
}
