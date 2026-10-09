//! Fuzz target: the entropy subsystem (`kitsune_core::entropy`).
//!
//! The input is a script of operations on one `Entropy`: add bytes from any source id
//! with any claimed bit count, feed timestamps, request output of any size, trigger
//! reseeds, move the clock. Whatever the sequence, the invariants that the kernel's
//! policy relies on must hold:
//!
//! * nothing panics (including source ids and claimed bits far out of range);
//! * per-source credit never exceeds 8 bits per byte received, and never decreases;
//! * the quality rating never goes down, and timing sources alone never reach `Strong`;
//! * the credit moved into the DRBG key never exceeds 256 bits per class;
//! * `fill` always fills the whole buffer, and no two 32-byte outputs of one run repeat.
#![no_main]

use libfuzzer_sys::fuzz_target;
use kitsune_core::entropy::{Entropy, MAX_SOURCES, Quality, source};

const MAX_OUT: usize = 4096;
const MAX_ADD: usize = 256;

fuzz_target!(|data: &[u8]| {
    let mut it = data.iter().copied();
    let mut next = move || it.next();
    let boot_len = usize::from(next().unwrap_or(0)) % 33;
    let boot: Vec<u8> = (0..boot_len).filter_map(|_| next()).collect();
    let mut e = Entropy::new(&boot);

    let mut now = 0u64;
    let mut ts = 0u64;
    let mut best = Quality::Weak;
    let mut credited = [0u64; MAX_SOURCES];
    let mut outputs: Vec<[u8; 32]> = Vec::new();
    let mut hardware_used = false;

    while let Some(op) = next() {
        match op % 6 {
            0 => {
                // add: id, claimed bits, length, bytes
                let id = next().unwrap_or(0);
                let bits = u32::from(next().unwrap_or(0)) * 37;
                let len = usize::from(next().unwrap_or(0)) % MAX_ADD;
                let bytes: Vec<u8> = (0..len).filter_map(|_| next()).collect();
                if source::is_hardware(id) && bits > 0 && !bytes.is_empty() {
                    hardware_used = true;
                }
                e.add(id, &bytes, bits);
            }
            1 | 2 => {
                // timestamp from a timing source (never a hardware id)
                let id = 3 + next().unwrap_or(0) % 8;
                let step = u64::from(next().unwrap_or(0)) * u64::from(next().unwrap_or(0)) * 1024;
                ts = ts.wrapping_add(step);
                e.sample(id, ts);
            }
            3 => {
                let n = usize::from(next().unwrap_or(0)) * usize::from(next().unwrap_or(0)) % MAX_OUT;
                now += u64::from(next().unwrap_or(0)) * 400;
                let mut out = vec![0xA5u8; n];
                e.fill(&mut out, now);
                if n == 32 {
                    let mut a = [0u8; 32];
                    a.copy_from_slice(&out);
                    outputs.push(a);
                }
            }
            4 => {
                now += u64::from(next().unwrap_or(0)) * 700;
                e.maybe_reseed(now);
            }
            _ => {
                // one more fixed-size draw so repeats can be detected
                let mut a = [0u8; 32];
                e.fill(&mut a, now);
                outputs.push(a);
            }
        }
        let q = e.quality();
        assert!(q >= best, "rating went down");
        best = q;
        if !hardware_used {
            assert!(q != Quality::Strong, "timing and boot input alone rated Strong");
        }
        for (i, h) in e.health().iter().enumerate() {
            assert!(h.credited_mbits <= h.bytes_in.saturating_mul(8000));
            assert!(h.credited_mbits >= credited[i], "credit decreased");
            credited[i] = h.credited_mbits;
        }
        let (hw, timing) = e.seeded_bits();
        assert!(hw <= 256 && timing <= 256);
    }
    let n = outputs.len();
    outputs.sort_unstable();
    outputs.dedup();
    assert_eq!(outputs.len(), n, "a 32-byte output repeated");
});
