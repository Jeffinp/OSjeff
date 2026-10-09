//! Tests for the entropy subsystem: RFC 8439 vectors, independently computed
//! known answers, determinism, reseeding, health accounting and statistical
//! smoke tests on 1 MiB of output.

use super::chacha::{self, BLOCK_LEN};
use super::*;
use alloc::vec::Vec;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn key_00_1f() -> [u8; 32] {
    let mut k = [0u8; 32];
    for (i, b) in k.iter_mut().enumerate() {
        *b = i as u8;
    }
    k
}

/// 1 MiB from a fixed seed. The seed is fixed, so these checks are deterministic:
/// they cannot flake, and a bug that biases the output moves them out of bounds.
fn mib() -> Vec<u8> {
    let mut e = Entropy::new(b"stat-test");
    e.add(source::RDSEED, &[0xA5; 32], 256);
    let mut out = alloc::vec![0u8; 1 << 20];
    e.fill(&mut out, 1_000);
    assert_eq!(e.quality(), Quality::Strong);
    out
}

/// xorshift64* for test timestamps (fixed seed, deterministic).
struct Xs(u64);
impl Xs {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

fn jitter_stream(x: &mut Xs, ts: &mut u64) -> u64 {
    *ts += 4_000_000 + (x.next() % 10_000);
    *ts
}

mod chacha20_rfc_8439;
mod drbg_streams;
mod entropy_policy;
mod pool;
mod statistical_smoke_tests;
mod timing_estimator;
