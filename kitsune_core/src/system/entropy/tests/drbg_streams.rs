use super::*;

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
