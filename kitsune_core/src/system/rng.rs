//! Pure pieces of the kernel's hardware random-number sources: CPUID detection
//! of `RDRAND` / `RDSEED`, the bounded retry around them, and the sanity check
//! applied to what they return.
//!
//! The kernel owns the glue (`CPUID`, the instructions themselves, the TSC);
//! the decisions live here so they are host-tested. Everything produced by
//! these instructions is mixed into [`crate::system::entropy`], never used raw.

/// How many times a failed `RDRAND` is retried before giving up (Intel's
/// software guidance is 10).
pub const RDRAND_RETRIES: usize = 10;

/// How many times a failed `RDSEED` is retried. `RDSEED` draws straight from the
/// conditioner and legitimately underflows under load, so Intel's guidance
/// is a longer loop (with `pause`) than for `RDRAND`.
pub const RDSEED_RETRIES: usize = 64;

/// `CPUID.01H:ECX` bit 30: the `RDRAND` instruction is available.
pub const CPUID_ECX_RDRAND: u32 = 1 << 30;

/// `CPUID.(EAX=07H,ECX=0):EBX` bit 18: the `RDSEED` instruction is available.
pub const CPUID_EBX7_RDSEED: u32 = 1 << 18;

/// Does `CPUID.01H:ECX` advertise `RDRAND`?
pub fn has_rdrand(cpuid_01h_ecx: u32) -> bool {
    cpuid_01h_ecx & CPUID_ECX_RDRAND != 0
}

/// Does `CPUID.(07H,0):EBX` advertise `RDSEED`? `max_leaf` is `CPUID.0:EAX`: leaf 7 does
/// not exist on a CPU whose highest leaf is lower, and its (garbage) result must be ignored.
pub fn has_rdseed(max_leaf: u32, cpuid_07h_ebx: u32) -> bool {
    max_leaf >= 7 && cpuid_07h_ebx & CPUID_EBX7_RDSEED != 0
}

/// Call `step` (one `RDRAND`/`RDSEED` attempt: `Some` on success, `None` when
/// the hardware reported "not ready") up to `tries` times and return the first
/// success. `None` means the hardware generator is unusable right now and the
/// caller must not pretend it produced entropy.
pub fn retry_n(tries: usize, mut step: impl FnMut() -> Option<u64>) -> Option<u64> {
    for _ in 0..tries {
        if let Some(v) = step() {
            return Some(v);
        }
    }
    None
}

/// [`retry_n`] with [`RDRAND_RETRIES`].
pub fn retry_hw(step: impl FnMut() -> Option<u64>) -> Option<u64> {
    retry_n(RDRAND_RETRIES, step)
}

/// A single 64-bit value that real hardware has been seen to return when broken
/// (the AMD Ryzen/Jaguar `RDRAND` bug returns all ones with CF=1, a stuck
/// virtual device returns zeros), or the 32-bit form of it.
pub fn known_bad_word(w: u64) -> bool {
    w == 0 || w == u64::MAX || w == u64::from(u32::MAX) || w == !u64::from(u32::MAX)
}

/// Sanity check on a block of words drawn in a row: reject it if any word is a
/// [`known_bad_word`] or if all of them are equal. A healthy generator fails
/// this with probability about `2^-61` per word, so a rejection means a broken
/// or emulated-badly instruction, not bad luck.
pub fn plausible_block(words: &[u64]) -> bool {
    if words.iter().any(|&w| known_bad_word(w)) {
        return false;
    }
    match words.split_first() {
        Some((first, rest)) if !rest.is_empty() => rest.iter().any(|w| w != first),
        _ => true,
    }
}

#[cfg(test)]
mod tests;
