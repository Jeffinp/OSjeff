use super::*;

// ---- DER primitives ----

#[test]
fn short_and_long_lengths() {
    let mut r = Reader::new(&[0x04, 0x02, 0xAA, 0xBB]);
    assert_eq!(r.read().unwrap(), (0x04, &[0xAA, 0xBB][..]));
    assert!(r.is_empty());
    let mut long = vec![0x04, 0x81, 0x80];
    long.extend(std::iter::repeat_n(7u8, 0x80));
    let mut r = Reader::new(&long);
    assert_eq!(r.read().unwrap().1.len(), 0x80);
}

#[test]
fn rejects_bad_lengths() {
    // indefinite
    assert_eq!(
        Reader::new(&[0x30, 0x80, 0, 0]).read().err(),
        Some(Error::BadLength)
    );
    // 4-byte length
    assert_eq!(
        Reader::new(&[0x04, 0x84, 0, 0, 0, 1, 0]).read().err(),
        Some(Error::BadLength)
    );
    // long form for a short value
    assert_eq!(
        Reader::new(&[0x04, 0x81, 0x05, 0, 0, 0, 0, 0]).read().err(),
        Some(Error::BadLength)
    );
    // leading zero in length bytes
    assert_eq!(
        Reader::new(&[0x04, 0x82, 0x00, 0x90]).read().err(),
        Some(Error::BadLength)
    );
    // truncated content
    assert_eq!(
        Reader::new(&[0x04, 0x05, 1, 2]).read().err(),
        Some(Error::Truncated)
    );
    // truncated header
    assert_eq!(Reader::new(&[0x04]).read().err(), Some(Error::Truncated));
    assert_eq!(Reader::new(&[]).read().err(), Some(Error::Truncated));
    // multi-byte tag
    assert_eq!(
        Reader::new(&[0x1F, 0x01, 0x00]).read().err(),
        Some(Error::BadLength)
    );
}

#[test]
fn huge_length_does_not_overflow() {
    let r = Reader::new(&[0x04, 0x83, 0xFF, 0xFF, 0xFF]).read();
    assert_eq!(r.err(), Some(Error::Truncated));
}

#[test]
fn expect_and_optional() {
    let mut r = Reader::new(&[0x02, 0x01, 0x05, 0x04, 0x00]);
    assert_eq!(r.optional(TAG_BOOL).unwrap(), None);
    assert_eq!(r.expect(TAG_INT).unwrap(), &[5]);
    assert_eq!(r.expect(TAG_INT).err(), Some(Error::UnexpectedTag));
}

// ---- time ----

#[test]
fn utc_time_pivot() {
    assert_eq!(
        parse_time(TAG_UTCTIME, b"491231235959Z").unwrap(),
        2_524_607_999 // 2049-12-31T23:59:59Z
    );
    // 50..99 pivot to 19xx, which predates the Unix epoch: refused.
    assert!(parse_time(TAG_UTCTIME, b"500101000000Z").is_err());
    assert_eq!(parse_time(TAG_UTCTIME, b"700101000000Z").unwrap(), 0);
}

#[test]
fn generalized_time() {
    assert_eq!(
        parse_time(TAG_GENTIME, b"20240101000000Z").unwrap(),
        crate::format::unixtime::MIN_PLAUSIBLE_UNIX
    );
    assert_eq!(
        parse_time(TAG_GENTIME, b"99991231235959Z").unwrap(),
        253_402_300_799
    );
}

#[test]
fn bad_times() {
    for s in [
        &b"240101000000"[..], // no Z
        b"2401010000Z",       // no seconds
        b"240230000000Z",     // Feb 30
        b"241301000000Z",     // month 13
        b"240101240000Z",     // hour 24
        b"24010100000aZ",     // non digit
        b"240101000000+0100", // offset
        b"",
    ] {
        assert!(parse_time(TAG_UTCTIME, s).is_err(), "{s:?}");
    }
    assert!(parse_time(TAG_GENTIME, b"2024010100000Z").is_err());
    assert!(parse_time(0x04, b"240101000000Z").is_err());
}

// ---- names ----

#[test]
fn exact_match_case_insensitive() {
    assert!(dns_name_matches(b"example.com", b"EXAMPLE.com"));
    assert!(!dns_name_matches(b"example.com", b"www.example.com"));
    assert!(!dns_name_matches(b"www.example.com", b"example.com"));
}

#[test]
fn wildcard_one_label_only() {
    assert!(dns_name_matches(b"*.example.com", b"www.example.com"));
    assert!(dns_name_matches(b"*.example.com", b"WWW.Example.COM"));
    assert!(!dns_name_matches(b"*.example.com", b"example.com"));
    assert!(!dns_name_matches(b"*.example.com", b"a.b.example.com"));
    assert!(!dns_name_matches(b"*.example.com", b".example.com"));
}

#[test]
fn wildcard_must_be_whole_leftmost_label() {
    assert!(!dns_name_matches(b"w*.example.com", b"www.example.com"));
    assert!(!dns_name_matches(b"www.*.com", b"www.a.com"));
    assert!(!dns_name_matches(b"*w.example.com", b"www.example.com"));
    assert!(!dns_name_matches(b"**.example.com", b"a.example.com"));
}

#[test]
fn wildcard_needs_two_labels_after() {
    assert!(!dns_name_matches(b"*.com", b"example.com"));
    assert!(!dns_name_matches(b"*.", b"a."));
    assert!(!dns_name_matches(b"*", b"a"));
}

#[test]
fn rejects_malformed_names() {
    assert!(!dns_name_matches(b"", b"a.com"));
    assert!(!dns_name_matches(b"a.com", b""));
    assert!(!dns_name_matches(b"a..com", b"a..com"));
    assert!(!dns_name_matches(b"a.com.", b"a.com."));
    assert!(!dns_name_matches(b"a b.com", b"a b.com"));
    assert!(!dns_name_matches(b"a.com\0.evil.com", b"a.com\0.evil.com"));
    let long = [b'a'; 64];
    let mut name = long.to_vec();
    name.extend_from_slice(b".com");
    assert!(!dns_name_matches(&name, &name));
}

#[test]
fn ip_like_hosts_are_just_names() {
    assert!(dns_name_matches(b"10.0.0.1", b"10.0.0.1"));
    assert!(!dns_name_matches(b"*.0.0.1", b"10.0.0.1")); // no wildcard IPs
}

#[test]
fn chain_limits() {
    let c = [1u8; 10];
    assert!(chain_lens_ok(&[&c]));
    assert!(!chain_lens_ok(&[]));
    let nine: Vec<&[u8]> = (0..9).map(|_| &c[..]).collect();
    assert!(!chain_lens_ok(&nine));
    let big = vec![0u8; MAX_CERT_LEN + 1];
    assert!(!chain_lens_ok(&[&big]));
    assert!(!chain_lens_ok(&[&[]]));
}

#[test]
fn parse_rejects_garbage() {
    assert!(parse(&[]).is_err());
    assert!(parse(&[0x30, 0x00]).is_err());
    assert!(parse(&[0x30, 0x03, 0x30, 0x01, 0x00]).is_err());
    let big = vec![0x30u8; MAX_CERT_LEN + 1];
    assert_eq!(parse(&big).err(), Some(Error::TooLarge));
}

#[test]
fn parse_never_panics_on_prefixes_and_flips() {
    // Truncations and single-byte corruptions of a structurally plausible
    // blob must produce errors, not panics.
    let base: Vec<u8> = (0..600u32).map(|i| (i * 31 % 251) as u8).collect();
    for n in 0..base.len() {
        let _ = parse(&base[..n]);
    }
    let mut seed = 1u32;
    for _ in 0..2000 {
        let mut b = base.clone();
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let i = (seed >> 8) as usize % b.len();
        b[i] = (seed >> 3) as u8;
        let _ = parse(&b);
    }
}
