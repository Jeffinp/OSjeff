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

// ---------------------------------------------------------------- ChaCha20 (RFC 8439)

#[test]
fn rfc8439_quarter_round() {
    // Section 2.1.1.
    let (a, b, c, d) = chacha::quarter_round(0x1111_1111, 0x0102_0304, 0x9b8d_6f43, 0x0123_4567);
    assert_eq!(
        (a, b, c, d),
        (0xea2a_92f4, 0xcb1c_f8ce, 0x4581_472e, 0x5881_c4bb)
    );
}

#[test]
fn rfc8439_block_function() {
    // Section 2.3.2: key 00..1f, counter 1, nonce 00:00:00:09:00:00:00:4a:00:00:00:00.
    let nonce = [0, 0, 0, 9, 0, 0, 0, 0x4a, 0, 0, 0, 0];
    let out = chacha::block(&key_00_1f(), 1, &nonce);
    let want = hex(
        "10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4e\
         d2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e",
    );
    assert_eq!(&out[..], &want[..]);
}

#[test]
fn rfc8439_encryption_example() {
    // Section 2.4.2: the "sunscreen" plaintext, counter 1.
    let pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
    let nonce = [0, 0, 0, 0, 0, 0, 0, 0x4a, 0, 0, 0, 0];
    let mut ks = alloc::vec![0u8; pt.len()];
    assert_eq!(
        chacha::keystream(&key_00_1f(), &nonce, 1, &mut ks),
        Some(1 + 2)
    );
    let ct: Vec<u8> = pt.iter().zip(&ks).map(|(p, k)| p ^ k).collect();
    let want = hex(
        "6e2e359a2568f98041ba0728dd0d6981e97e7aec1d4360c20a27afccfd9fae0b\
         f91b65c5524733ab8f593dabcd62b3571639d624e65152ab8f530c359f0861d8\
         07ca0dbf500d6a6156a38e088a22b65e52bc514d16ccf806818ce91ab7793736\
         5af90bbf74a35be6b40b8eedf2785e42874d",
    );
    assert_eq!(ct, want);
}

#[test]
fn rfc8439_appendix_a1_zero_key_vectors() {
    // Appendix A.1, tests 1 and 2: all-zero key and nonce, counters 0 and 1.
    let z = [0u8; 32];
    let n = [0u8; 12];
    assert_eq!(
        &chacha::block(&z, 0, &n)[..],
        &hex(
            "76b8e0ada0f13d90405d6ae55386bd28bdd219b8a08ded1aa836efcc8b770dc7\
             da41597c5157488d7724e03fb8d84a376a43b8f41518a11cc387b669b2ee6586"
        )[..]
    );
    assert_eq!(
        &chacha::block(&z, 1, &n)[..],
        &hex(
            "9f07e7be5551387a98ba977c732d080dcb0f29a048e3656912c6533e32ee7aed\
             29b721769ce64e43d57133b074d839d531ed1f28510afb45ace10a1f4b794d6f"
        )[..]
    );
}

#[test]
fn chacha_keystream_near_counter_wrap_matches_reference() {
    // Computed with an independent implementation (python `cryptography`).
    let nonce = hex("0102030405060708090a0b0c");
    let nonce: [u8; 12] = nonce.try_into().unwrap();
    let mut out = [0u8; 128];
    // Blocks 0xfffffffe and 0xffffffff are both usable; the counter space is then exhausted.
    assert_eq!(
        chacha::keystream(&key_00_1f(), &nonce, 0xffff_fffe, &mut out),
        None
    );
    let want = hex(
        "0a6ef904698cfed0e2a9d264cf24029803976e6dfe192f36866801bae8dcd28d\
         1b9f7d81ccad0bc769f1f3af36276e3e47e0cf943c2b3b0022363525db15520b\
         c4e9bc95855bfc7f5456e79b23564422a662e195208fc8a7d24bd6c35a366573\
         688febc54eade68bc3d3e3529dcec58ac73440107ff9021defca771ad3825c3d",
    );
    assert_eq!(&out[..], &want[..]);
}

#[test]
fn chacha_keystream_prefix_property() {
    // Any prefix of a longer request is the same keystream.
    let n = [7u8; 12];
    let mut long = [0u8; 3 * BLOCK_LEN + 5];
    let mut short = [0u8; BLOCK_LEN + 9];
    chacha::keystream(&key_00_1f(), &n, 3, &mut long).unwrap();
    chacha::keystream(&key_00_1f(), &n, 3, &mut short).unwrap();
    assert_eq!(&long[..short.len()], &short[..]);
}

// ---------------------------------------------------------------- DRBG

#[test]
fn drbg_known_answer() {
    // Expected values computed independently (python: sha256 + ChaCha20) from the
    // construction documented in drbg.rs.
    let mut d = Drbg::new(&[0u8; 32]);
    let mut a = [0u8; 100];
    d.fill(&mut a);
    assert_eq!(
        &a[..],
        &hex(
            "4a98b913720da2945c4c0822d2ea19e7c0dd848e99187ca18f8408b26de0bdbd\
             3be81938f61bafa4ff0eacaa7d7edc8ef2de3e7bb2448de785ac66ab3c462a5b\
             87fbbb867304810a5c1642996d155aa52b0c30eb14fc576229b88f51e50cf6ca\
             7b7575b6"
        )[..]
    );
    let mut b = [0u8; 16];
    d.fill(&mut b);
    assert_eq!(&b[..], &hex("c026d3d9335818d5102ab5d9deb79751")[..]);
    d.reseed(&[1u8; 32]);
    let mut c = [0u8; 8];
    d.fill(&mut c);
    assert_eq!(&c[..], &hex("289db04f955ee6e4")[..]);
}

#[test]
fn drbg_is_deterministic_for_equal_seed_and_calls() {
    let mut a = Drbg::new(&[9u8; 32]);
    let mut b = Drbg::new(&[9u8; 32]);
    let (mut x, mut y) = ([0u8; 77], [0u8; 77]);
    a.fill(&mut x);
    b.fill(&mut y);
    assert_eq!(x, y);
    a.fill(&mut x);
    b.fill(&mut y);
    assert_eq!(x, y);
    let mut c = Drbg::new(&[10u8; 32]);
    c.fill(&mut y);
    assert_ne!(x, y);
}

#[test]
fn drbg_key_erasure_makes_each_call_independent_of_chunking() {
    // With fast key erasure two calls are NOT the concatenation of one longer call.
    let mut a = Drbg::new(&[3u8; 32]);
    let mut b = Drbg::new(&[3u8; 32]);
    let mut one = [0u8; 64];
    a.fill(&mut one);
    let (mut h1, mut h2) = ([0u8; 32], [0u8; 32]);
    b.fill(&mut h1);
    b.fill(&mut h2);
    assert_eq!(&one[..32], &h1[..]);
    assert_ne!(&one[32..], &h2[..]);
}

#[test]
fn drbg_reseed_changes_the_stream_and_depends_on_old_state() {
    let mut a = Drbg::new(&[1u8; 32]);
    let mut b = Drbg::new(&[1u8; 32]);
    a.reseed(&[5u8; 32]);
    let (mut x, mut y) = ([0u8; 32], [0u8; 32]);
    a.fill(&mut x);
    b.fill(&mut y);
    assert_ne!(x, y);
    // The same reseed seed on a different prior state must not collapse them.
    let mut c = Drbg::new(&[2u8; 32]);
    c.reseed(&[5u8; 32]);
    let mut z = [0u8; 32];
    c.fill(&mut z);
    assert_ne!(x, z);
    assert_eq!(a.reseeds(), 1);
}

#[test]
fn drbg_reseed_counter_trips_on_fills_and_on_bytes() {
    let mut d = Drbg::new(&[0u8; 32]);
    assert!(!d.needs_reseed());
    let mut one = [0u8; 1];
    for _ in 0..drbg::RESEED_AFTER_FILLS - 1 {
        d.fill(&mut one);
    }
    assert!(!d.needs_reseed());
    d.fill(&mut one);
    assert!(d.needs_reseed());
    d.reseed(&[1u8; 32]);
    assert!(!d.needs_reseed());
    let mut big = alloc::vec![0u8; drbg::RESEED_AFTER_BYTES as usize];
    d.fill(&mut big);
    assert!(d.needs_reseed());
    // Empty requests are free.
    let mut e = Drbg::new(&[0u8; 32]);
    e.fill(&mut []);
    assert!(!e.needs_reseed());
}

#[test]
fn drbg_large_request_spans_several_keys_and_never_repeats_a_block() {
    let mut d = Drbg::new(&[4u8; 32]);
    let mut out = alloc::vec![0u8; 3 * 64 * 1024 + 123];
    d.fill(&mut out);
    let mut blocks: Vec<&[u8; 16]> = out.as_chunks::<16>().0.iter().collect();
    let n = blocks.len();
    blocks.sort_unstable();
    blocks.dedup();
    assert_eq!(blocks.len(), n, "a 16-byte block repeated");
}

#[test]
fn drbg_outputs_are_unique_across_many_small_fills() {
    let mut d = Drbg::new(&[8u8; 32]);
    let mut seen: Vec<[u8; 16]> = Vec::new();
    for _ in 0..20_000 {
        let mut o = [0u8; 16];
        d.fill(&mut o);
        seen.push(o);
    }
    seen.sort_unstable();
    let n = seen.len();
    seen.dedup();
    assert_eq!(seen.len(), n);
}

// ---------------------------------------------------------------- statistical smoke tests

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

#[test]
fn stats_bit_balance_on_1_mib() {
    let out = mib();
    let n = (out.len() * 8) as f64;
    let ones: u64 = out.iter().map(|b| u64::from(b.count_ones())).sum();
    // Binomial(n, 1/2): sigma = sqrt(n)/2. Allow 5 sigma.
    let dev = (ones as f64 - n / 2.0).abs();
    assert!(dev < 5.0 * n.sqrt() / 2.0, "ones {ones} of {n}");
}

#[test]
fn stats_byte_chi_square_on_1_mib() {
    let out = mib();
    let mut hist = [0u64; 256];
    for &b in &out {
        hist[usize::from(b)] += 1;
    }
    let exp = out.len() as f64 / 256.0;
    let chi: f64 = hist
        .iter()
        .map(|&o| {
            let d = o as f64 - exp;
            d * d / exp
        })
        .sum();
    // df = 255: mean 255, sigma sqrt(510) = 22.6; bounds are about +-5 sigma.
    assert!((140.0..370.0).contains(&chi), "chi-square {chi}");
}

#[test]
fn stats_adjacent_bit_transitions_on_1_mib() {
    let out = mib();
    // Count bit flips between consecutive bits (LSB first): about half.
    let mut flips = 0u64;
    let mut prev = out[0] & 1;
    let mut total = 0u64;
    for &b in &out {
        for i in 0..8 {
            let bit = (b >> i) & 1;
            flips += u64::from(bit != prev);
            prev = bit;
            total += 1;
        }
    }
    let dev = (flips as f64 - total as f64 / 2.0).abs();
    assert!(
        dev < 5.0 * (total as f64).sqrt() / 2.0,
        "flips {flips}/{total}"
    );
}

#[test]
fn stats_every_bit_position_is_balanced() {
    let out = mib();
    for pos in 0..8 {
        let ones = out.iter().filter(|b| (**b >> pos) & 1 == 1).count() as f64;
        let n = out.len() as f64;
        assert!(
            (ones - n / 2.0).abs() < 5.0 * n.sqrt() / 2.0,
            "bit {pos}: {ones}"
        );
    }
}

// ---------------------------------------------------------------- pool

#[test]
fn pool_known_answer_and_carry() {
    let mut p = Pool::new();
    p.add(source::TIMER, &[1, 2, 3], 8);
    p.add(source::RDSEED, b"abcd", 32);
    let s = p.drain();
    assert_eq!(
        &s.key[..],
        &hex("1a96b1e6c006417ebe1da0fc3f4a159569af4484fb3c32cc1dd2eb27e6facf57")[..]
    );
    assert_eq!((s.hw_bits, s.timing_bits), (32, 8));
    // A second drain without new input still differs (it carries the digest and counts drains)
    // and has no new credit.
    let s2 = p.drain();
    assert_eq!(
        &s2.key[..],
        &hex("017fc480b1190234d2420c79b2b2d6ec8e1d256361c81247e73e91a13ea3a1c9")[..]
    );
    assert_eq!(s2.bits(), 0);
}

#[test]
fn pool_never_credits_more_than_the_data_holds() {
    let mut p = Pool::new();
    p.add(source::NIC, &[1], 1000); // one byte cannot carry 1000 bits
    assert_eq!(p.health()[usize::from(source::NIC)].credited_bits(), 8);
    p.add_milli(source::NIC, &[], 5_000); // no data, no credit
    assert_eq!(p.health()[usize::from(source::NIC)].credited_bits(), 8);
    assert_eq!(p.pending_bits(), (0, 8));
}

#[test]
fn pool_separates_classes_and_caps_a_seed_at_256_bits() {
    let mut p = Pool::new();
    p.add(source::RDRAND, &[0u8; 32], 200);
    p.add(source::VIRTIO_RNG, &[1u8; 32], 100);
    p.add(source::TIMER, &[2u8; 64], 300);
    assert_eq!(p.pending_bits(), (300, 300));
    let s = p.drain();
    assert_eq!((s.hw_bits, s.timing_bits), (256, 0)); // hardware first, capped at the key size
    // Nothing is lost, the rest waits for the next seed.
    assert_eq!(p.pending_bits(), (44, 300));
    let s = p.drain();
    assert_eq!((s.hw_bits, s.timing_bits), (44, 212));
    assert_eq!(p.pending_bits(), (0, 88));
}

#[test]
fn pool_keeps_per_source_health() {
    let mut p = Pool::new();
    p.add(source::KEYBOARD, &[1, 2], 1);
    p.add(source::KEYBOARD, &[3], 1);
    p.note_rejected(source::KEYBOARD);
    let h = p.health()[usize::from(source::KEYBOARD)];
    assert_eq!(
        (h.events, h.bytes_in, h.credited_bits(), h.rejected),
        (2, 3, 2, 1)
    );
    // An id beyond the table shares the last slot and does not panic.
    p.add(200, &[9], 1);
    assert_eq!(p.health()[MAX_SOURCES - 1].events, 1);
}

#[test]
fn pool_input_changes_the_seed() {
    let mut a = Pool::new();
    let mut b = Pool::new();
    a.add(source::TIMER, &[1], 0);
    b.add(source::TIMER, &[2], 0);
    assert_ne!(a.drain().key, b.drain().key);
    // The source id is part of what is hashed.
    let mut c = Pool::new();
    let mut d = Pool::new();
    c.add(source::TIMER, &[1], 0);
    d.add(source::NIC, &[1], 0);
    assert_ne!(c.drain().key, d.drain().key);
    // And so is the framing: [1,2]+[3] is not [1]+[2,3].
    let mut e = Pool::new();
    let mut f = Pool::new();
    e.add(source::TIMER, &[1, 2], 0);
    e.add(source::TIMER, &[3], 0);
    f.add(source::TIMER, &[1], 0);
    f.add(source::TIMER, &[2, 3], 0);
    assert_ne!(e.drain().key, f.drain().key);
}

// ---------------------------------------------------------------- timing estimator

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

#[test]
fn estimator_credits_nothing_for_a_perfectly_regular_source() {
    let mut e = TimingEstimator::new(jitter::IRQ_MILLIBITS);
    let mut ts = 1_000_000u64;
    for _ in 0..10_000 {
        ts += 4_000_000; // a 250 Hz timer on a 1 GHz TSC in a deterministic emulator
        assert_eq!(e.observe(ts), Verdict::Rejected);
    }
}

#[test]
fn estimator_rejects_stuck_and_alternating_sources() {
    let mut e = TimingEstimator::new(jitter::IRQ_MILLIBITS);
    for _ in 0..100 {
        assert_eq!(e.observe(42), Verdict::Rejected); // same timestamp every time
    }
    // Two alternating intervals: d2 is constant, d3 is zero.
    let mut e = TimingEstimator::new(jitter::IRQ_MILLIBITS);
    let mut ts = 0u64;
    let mut accepted = 0;
    for i in 0..1000 {
        ts += if i % 2 == 0 { 1000 } else { 1500 };
        if matches!(e.observe(ts), Verdict::Accepted(_)) {
            accepted += 1;
        }
    }
    assert_eq!(accepted, 0);
}

#[test]
fn estimator_rejects_a_small_quantization_wobble() {
    // Intervals jitter by +-1 cycle only: below MIN_VARIATION, so no credit.
    let mut e = TimingEstimator::new(jitter::IRQ_MILLIBITS);
    let mut x = Xs(7);
    let mut ts = 0u64;
    let mut accepted = 0;
    for _ in 0..2000 {
        ts += 10_000 + (x.next() & 1);
        if matches!(e.observe(ts), Verdict::Accepted(_)) {
            accepted += 1;
        }
    }
    assert_eq!(accepted, 0);
}

#[test]
fn estimator_credits_genuine_jitter_at_the_configured_rate() {
    let mut e = TimingEstimator::new(jitter::IRQ_MILLIBITS);
    let mut x = Xs(0x1234_5678_9abc_def1);
    let mut ts = 0u64;
    let mut mbits = 0u64;
    let n = 4000u32;
    for _ in 0..n {
        ts += 4_000_000 + (x.next() % 5_000);
        if let Verdict::Accepted(m) = e.observe(ts) {
            mbits += u64::from(m);
        }
    }
    // Nearly every event passes and each is worth at most 0.5 bit.
    assert!(mbits > u64::from(n) * 400, "{mbits}");
    assert!(mbits <= u64::from(n) * 500, "{mbits}");
}

#[test]
fn estimator_needs_history_before_it_credits() {
    let mut e = TimingEstimator::new(500);
    let mut x = Xs(99);
    let mut ts = 0;
    for i in 0..10 {
        ts += 1000 + (x.next() % 997);
        let v = e.observe(ts);
        if i < 3 {
            assert_eq!(v, Verdict::Rejected, "event {i}");
        }
    }
}

#[test]
fn estimator_flags_a_repeating_low_byte() {
    // d1 low byte constant at 0x10 but d2/d3 non-zero (high bytes move): the RCT trips.
    let mut e = TimingEstimator::new(500);
    let mut ts = 0u64;
    let mut late_accepted = 0;
    for i in 0..200u64 {
        ts += 0x1_0010 + ((i * i) << 8) % 0x3000 + 0x10_0000;
        // The first few events are judged before the run is long enough.
        if matches!(e.observe(ts), Verdict::Accepted(_)) && i >= 10 {
            late_accepted += 1;
        }
    }
    assert_eq!(late_accepted, 0);
}

// ---------------------------------------------------------------- Entropy: policy

fn jitter_stream(x: &mut Xs, ts: &mut u64) -> u64 {
    *ts += 4_000_000 + (x.next() % 10_000);
    *ts
}

#[test]
fn quality_ratings() {
    assert_eq!(Quality::of(0, 0), Quality::Weak);
    assert_eq!(Quality::of(0, 127), Quality::Weak);
    assert_eq!(Quality::of(0, 128), Quality::Mixed);
    assert_eq!(Quality::of(0, 256), Quality::Mixed); // timing alone never rates Strong
    assert_eq!(Quality::of(127, 0), Quality::Weak);
    assert_eq!(Quality::of(100, 28), Quality::Mixed);
    assert_eq!(Quality::of(128, 0), Quality::Strong);
    assert!(Quality::Weak < Quality::Mixed && Quality::Mixed < Quality::Strong);
    assert_eq!(Quality::Strong.as_str(), "strong");
}

#[test]
fn a_fresh_generator_is_weak_but_differs_per_boot() {
    let mut a = Entropy::new(&1u64.to_le_bytes());
    let mut b = Entropy::new(&2u64.to_le_bytes());
    let mut a2 = Entropy::new(&1u64.to_le_bytes());
    assert_eq!(a.quality(), Quality::Weak);
    let (mut x, mut y, mut z) = ([0u8; 32], [0u8; 32], [0u8; 32]);
    a.fill(&mut x, 0);
    b.fill(&mut y, 0);
    a2.fill(&mut z, 0);
    assert_ne!(x, y);
    assert_eq!(x, z); // deterministic for fixed inputs
}

#[test]
fn a_hardware_seed_makes_it_strong_at_once() {
    let mut e = Entropy::new(b"boot");
    e.add(source::RDSEED, &[7u8; 32], 256);
    let mut o = [0u8; 8];
    e.fill(&mut o, 5);
    assert_eq!(e.quality(), Quality::Strong);
    assert_eq!(e.seeded_bits(), (256, 0));
    assert_eq!(e.reseeds(), 1);
}

#[test]
fn timing_credit_below_128_bits_stays_weak_and_is_not_spent() {
    let mut e = Entropy::new(b"boot");
    let (mut x, mut ts) = (Xs(5), 0u64);
    // 200 events * 0.5 bit = ~100 bits.
    for _ in 0..200 {
        let t = jitter_stream(&mut x, &mut ts);
        e.sample(source::TIMER, t);
    }
    let mut o = [0u8; 8];
    e.fill(&mut o, 1000);
    assert_eq!(e.quality(), Quality::Weak);
    assert_eq!(e.reseeds(), 0);
    let (_, pending) = e.pending_bits();
    assert!((90..=100).contains(&pending), "{pending}");
    // The rest arrives: Mixed, and the earlier credit counted.
    for _ in 0..100 {
        let t = jitter_stream(&mut x, &mut ts);
        e.sample(source::TIMER, t);
    }
    e.fill(&mut o, 2000);
    assert_eq!(e.quality(), Quality::Mixed);
    assert!(e.seeded_bits().1 >= 128);
}

#[test]
fn timing_alone_never_reaches_strong_however_long_it_runs() {
    let mut e = Entropy::new(b"boot");
    let (mut x, mut ts) = (Xs(11), 0u64);
    let mut o = [0u8; 4];
    for round in 0..50u64 {
        for _ in 0..600 {
            let t = jitter_stream(&mut x, &mut ts);
            e.sample(source::TIMER, t);
        }
        e.fill(&mut o, round * 61_000);
    }
    assert_eq!(e.quality(), Quality::Mixed);
    assert!(
        e.reseeds() >= 10,
        "periodic reseeds happened: {}",
        e.reseeds()
    );
}

#[test]
fn a_deterministic_timer_earns_no_credit_and_the_generator_stays_weak() {
    let mut e = Entropy::new(b"boot");
    let mut ts = 0u64;
    for _ in 0..100_000 {
        ts += 4_000_000;
        e.sample(source::TIMER, ts);
    }
    let mut o = [0u8; 4];
    e.fill(&mut o, 10_000);
    assert_eq!(e.quality(), Quality::Weak);
    let h = e.health()[usize::from(source::TIMER)];
    assert_eq!(h.credited_bits(), 0);
    assert_eq!(h.rejected, 100_000);
    assert_eq!(h.events, 100_000); // still mixed in
}

#[test]
fn periodic_reseed_waits_for_the_interval() {
    let mut e = Entropy::new(b"boot");
    e.add(source::RDSEED, &[1u8; 32], 256);
    assert!(e.maybe_reseed(1_000)); // first seed
    e.add(source::RDSEED, &[2u8; 32], 256);
    assert!(!e.maybe_reseed(1_000 + RESEED_INTERVAL_MS - 1));
    assert!(e.maybe_reseed(1_000 + RESEED_INTERVAL_MS));
    // No new credit: no periodic reseed, even much later.
    assert!(!e.maybe_reseed(10 * RESEED_INTERVAL_MS));
}

#[test]
fn an_exhausted_drbg_reseeds_even_without_new_credit() {
    let mut e = Entropy::new(b"boot");
    e.add(source::RDSEED, &[1u8; 32], 256);
    let mut o = [0u8; 1];
    e.fill(&mut o, 0);
    let before = e.reseeds();
    for _ in 0..drbg::RESEED_AFTER_FILLS {
        e.fill(&mut o, 1);
    }
    assert!(e.reseeds() > before);
    assert_eq!(e.quality(), Quality::Strong); // the rating survives a credit-free reseed
}

#[test]
fn a_late_hardware_source_upgrades_a_mixed_generator() {
    let mut e = Entropy::new(b"boot");
    let (mut x, mut ts) = (Xs(21), 0u64);
    for _ in 0..400 {
        let t = jitter_stream(&mut x, &mut ts);
        e.sample(source::TIMER, t);
    }
    let mut o = [0u8; 4];
    e.fill(&mut o, 100);
    assert_eq!(e.quality(), Quality::Mixed);
    e.add(source::VIRTIO_RNG, &[3u8; 32], 256);
    e.fill(&mut o, 200); // far less than the 60 s interval
    assert_eq!(e.quality(), Quality::Strong);
}

#[test]
fn rtc_and_boot_values_are_mixed_but_never_credited() {
    let mut e = Entropy::new(b"boot");
    e.add(source::RTC, &1_700_000_000u32.to_le_bytes(), 0);
    e.add(source::BOOT, &[0xFFu8; 32], 0);
    assert_eq!(e.pending_bits(), (0, 0));
    let mut o = [0u8; 4];
    e.fill(&mut o, 0);
    assert_eq!(e.quality(), Quality::Weak);
}

#[test]
fn source_names_and_classes() {
    assert!(source::is_hardware(source::RDSEED));
    assert!(source::is_hardware(source::RDRAND));
    assert!(source::is_hardware(source::VIRTIO_RNG));
    assert!(!source::is_hardware(source::TIMER));
    assert!(!source::is_hardware(source::NIC));
    assert_eq!(source::name(source::VIRTIO_RNG), "virtio-rng");
    assert_eq!(source::name(250), "other");
}

#[test]
fn random_operation_sequences_keep_the_invariants() {
    // A deterministic stand-in for a property test: many random sequences of the public API.
    for seed in 1..=40u64 {
        let mut x = Xs(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut e = Entropy::new(&seed.to_le_bytes());
        let mut ts = 0u64;
        let mut now = 0u64;
        let mut best = Quality::Weak;
        let mut seen: Vec<[u8; 16]> = Vec::new();
        let mut credited_before = 0u64;
        for _ in 0..400 {
            match x.next() % 5 {
                0 => {
                    let len = (x.next() % 40) as usize;
                    let data: Vec<u8> = (0..len).map(|_| x.next() as u8).collect();
                    let id = (x.next() % 24) as u8;
                    let bits = (x.next() % 600) as u32;
                    e.add(id, &data, bits);
                }
                1 | 2 => {
                    let id = (x.next() % 10) as u8;
                    ts += x.next() % 8_000_000;
                    e.sample(id, ts);
                }
                3 => {
                    now += x.next() % 90_000;
                    let mut o = [0u8; 16];
                    e.fill(&mut o, now);
                    seen.push(o);
                }
                _ => {
                    now += x.next() % 30_000;
                    e.maybe_reseed(now);
                }
            }
            // The rating never goes down.
            assert!(e.quality() >= best);
            best = e.quality();
            // Per-source credit is monotone and never exceeds 8 bits per byte received.
            let mut total = 0u64;
            for h in e.health() {
                assert!(h.credited_mbits <= h.bytes_in * 8000);
                total += h.credited_mbits;
            }
            assert!(total >= credited_before);
            credited_before = total;
            // What was handed to the DRBG key never exceeds what was credited.
            let (sh, st) = e.seeded_bits();
            assert!(sh <= 256 && st <= 256);
        }
        let n = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), n, "repeated output (seed {seed})");
    }
}
