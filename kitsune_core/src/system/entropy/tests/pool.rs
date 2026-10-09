use super::*;

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
