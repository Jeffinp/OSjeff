use super::*;

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
