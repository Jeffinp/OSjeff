//! ChaCha20 DRBG with fast key erasure.
//!
//! Construction (Bernstein's "fast-key-erasure RNG"): the state is one 256-bit key. Every call to
//! [`Drbg::fill`] expands that key into keystream with ChaCha20 (nonce 0, block
//! counter 0..), uses the first 32 bytes as the **next key** and hands out the
//! rest. The old key is overwritten before the output leaves the function, so
//! someone who steals the state afterwards cannot recompute what was already
//! produced (backtracking resistance), and two calls never share keystream.
//!
//! Reseeding ([`Drbg::reseed`]) hashes the old key together with the new seed
//! (SHA-256, domain separated): a bad or attacker-known seed can never make the
//! state *less* secret than it was.
//!
//! This is not a NIST SP 800-90A DRBG and makes no claim to be one; it is a
//! small, well-understood design whose only assumptions are that ChaCha20 is a
//! PRF and that SHA-256 is good enough as a key-derivation function.

use super::chacha::{self, BLOCK_LEN};
use sha2::{Digest, Sha256};

/// Output bytes served from one key-erasure step. Larger requests re-key in
/// between, so no single key ever expands past 64 KiB (1024 blocks, far from
/// the 2^32-block counter limit).
const MAX_PER_KEY: usize = 64 * 1024;

/// Requests served (each [`Drbg::fill`] counts one) before [`Drbg::needs_reseed`]
/// turns true.
pub const RESEED_AFTER_FILLS: u64 = 1 << 16;
/// Bytes produced since the last reseed before [`Drbg::needs_reseed`] turns true.
pub const RESEED_AFTER_BYTES: u64 = 1 << 20;

const NONCE0: [u8; 12] = [0; 12];

/// Overwrite `buf` with zeros and tell the optimizer the result is observed
/// (best effort: safe Rust has no guaranteed volatile wipe).
fn wipe(buf: &mut [u8]) {
    buf.fill(0);
    core::hint::black_box(&*buf);
}

/// The generator. `Clone` is deliberately not derived: duplicating the state
/// duplicates the output stream.
pub struct Drbg {
    key: [u8; 32],
    fills: u64,
    bytes: u64,
    reseeds: u64,
}

impl Drbg {
    /// Start from `seed` (callers pass the output of [`super::Pool::drain`]).
    pub fn new(seed: &[u8; 32]) -> Drbg {
        let mut h = Sha256::new();
        // Domain-separation label kept from the Kitsune days: changing it would change every output.
        h.update(b"osjeff-drbg-v1-init");
        h.update(seed);
        Drbg {
            key: h.finalize().into(),
            fills: 0,
            bytes: 0,
            reseeds: 0,
        }
    }

    /// Mix `seed` into the state: `key = SHA256(label || key || seed)`.
    pub fn reseed(&mut self, seed: &[u8; 32]) {
        let mut h = Sha256::new();
        h.update(b"osjeff-drbg-v1-reseed");
        h.update(self.key);
        h.update(seed);
        self.key = h.finalize().into();
        self.fills = 0;
        self.bytes = 0;
        self.reseeds += 1;
    }

    /// Fill `out` with pseudo-random bytes (an empty slice does nothing).
    pub fn fill(&mut self, out: &mut [u8]) {
        if out.is_empty() {
            return;
        }
        self.fills = self.fills.saturating_add(1);
        self.bytes = self.bytes.saturating_add(out.len() as u64);
        for part in out.chunks_mut(MAX_PER_KEY) {
            self.step(part);
        }
    }

    /// One key-erasure step producing `out.len() <= MAX_PER_KEY` bytes.
    fn step(&mut self, out: &mut [u8]) {
        let mut cur = self.key;
        let mut b0 = chacha::block(&cur, 0, &NONCE0);
        // Erase: the next key replaces the current one before any output leaves.
        self.key.copy_from_slice(&b0[..32]);
        let head = out.len().min(BLOCK_LEN - 32);
        out[..head].copy_from_slice(&b0[32..32 + head]);
        wipe(&mut b0);
        if out.len() > head {
            // Blocks 1.. under the old key; at most 1024 blocks, so the counter cannot wrap.
            let _ = chacha::keystream(&cur, &NONCE0, 1, &mut out[head..]);
        }
        wipe(&mut cur);
    }

    /// True once enough requests or bytes passed that the owner should mix in
    /// fresh seed material.
    pub fn needs_reseed(&self) -> bool {
        self.fills >= RESEED_AFTER_FILLS || self.bytes >= RESEED_AFTER_BYTES
    }

    /// How many times [`Drbg::reseed`] ran.
    pub fn reseeds(&self) -> u64 {
        self.reseeds
    }
}

impl Drop for Drbg {
    fn drop(&mut self) {
        wipe(&mut self.key);
    }
}
