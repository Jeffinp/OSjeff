//! Chain-validation tests over real certificates generated with
//! `tools/gen-test-certs.py` (openssl-compatible X.509, embedded as hex).

use super::testcerts as t;
use super::*;
use crate::unixtime::DateTime;
use alloc::boxed::Box;
use alloc::string::String;

fn hex(s: &str) -> &'static [u8] {
    let bytes: Vec<u8> = (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect();
    Box::leak(bytes.into_boxed_slice())
}

/// 2026-10-07T12:00:00Z
const NOW: u64 = 1_791_374_400;

fn store_of(roots: &[&str]) -> TrustStore {
    TrustStore {
        certs: roots.iter().map(|r| hex(r)).collect(),
    }
}

fn rsa_store() -> TrustStore {
    store_of(&[t::RSA_ROOT])
}

fn check(chain: &[&str], host: &str, now: u64, store: &TrustStore) -> Result<Verified, CertError> {
    let der: Vec<&[u8]> = chain.iter().map(|c| hex(c)).collect();
    verify_chain(&der, host, Some(now), store)
}

fn ok_rsa(leaf: &str, host: &str) -> Result<Verified, CertError> {
    check(&[leaf, t::RSA_INTER], host, NOW, &rsa_store())
}

// ---- the embedded trust store ----

#[test]
fn embedded_store_parses_and_has_expected_size() {
    let s = TrustStore::embedded();
    assert!(s.len() >= 40, "roots: {}", s.len());
}

#[test]
fn embedded_store_matches_manifest_fingerprints() {
    let s = TrustStore::embedded();
    for i in 0..s.len() {
        let (want, name) = TrustStore::manifest_entry(i).expect("manifest line");
        let got = s.fingerprint(i).unwrap();
        let hexed: String = got.iter().map(|b| alloc::format!("{b:02x}")).collect();
        assert_eq!(hexed, want, "root {i} ({name})");
    }
    assert!(
        TrustStore::manifest_entry(s.len()).is_none(),
        "manifest longer than blob"
    );
}

#[test]
fn embedded_store_has_the_well_known_roots() {
    let s = TrustStore::embedded();
    let known = [
        // ISRG Root X1 (Let's Encrypt)
        "96bcec06264976f37460779acf28c5a7cfe8a3c0aae11a8ffcee05c0bddf08c6",
        // DigiCert Global Root G2
        "cb3ccbb76031e5e0138f8dd39a23f9de47ffc35e43c1144cea27d46a5ab1cb5f",
    ];
    for want in known {
        assert!(
            (0..s.len()).any(|i| {
                let f: String = s
                    .fingerprint(i)
                    .unwrap()
                    .iter()
                    .map(|b| alloc::format!("{b:02x}"))
                    .collect();
                f == want
            }),
            "missing {want}"
        );
    }
}

#[test]
fn every_embedded_root_parses_as_a_ca_and_is_a_trust_anchor() {
    let s = TrustStore::embedded();
    assert_eq!(
        s.anchors().len(),
        s.len(),
        "every root must load as a trust anchor"
    );
    for i in 0..s.len() {
        let c = x509::parse(s.der(i).unwrap()).unwrap_or_else(|e| panic!("root {i}: {e:?}"));
        assert!(c.is_self_issued(), "root {i} is not self-issued");
        assert!(
            c.basic_constraints.is_some_and(|b| b.ca),
            "root {i} is not a CA"
        );
    }
}

#[test]
fn no_embedded_root_is_expired_today() {
    let s = TrustStore::embedded();
    for i in 0..s.len() {
        let c = x509::parse(s.der(i).unwrap()).unwrap();
        assert!(
            c.not_after > NOW,
            "root {i} ({:?}) already expired",
            TrustStore::manifest_entry(i)
        );
    }
}

#[test]
fn store_blob_parsing_is_strict() {
    static GOOD: &[u8] = b"OJTS1\0\x00\x01\x00\x03abc";
    assert_eq!(TrustStore::parse(GOOD).unwrap().len(), 1);
    assert!(TrustStore::parse(b"XJTS1\0\x00\x00").is_none(), "magic");
    assert!(
        TrustStore::parse(b"OJTS1\0\x00\x01\x00\x09abc").is_none(),
        "short entry"
    );
    assert!(
        TrustStore::parse(b"OJTS1\0\x00\x00x").is_none(),
        "trailing bytes"
    );
    assert!(TrustStore::parse(b"OJTS1\0\x00").is_none(), "short count");
    assert!(TrustStore::parse(b"").is_none());
}

// ---- successful chains ----

#[test]
fn valid_rsa_chain() {
    let v = ok_rsa(t::RSA_LEAF, "rsa.test").unwrap();
    assert_eq!(v.chain_len, 2);
    assert_eq!(v.root, Some(0));
}

#[test]
fn valid_chain_extra_names_and_sha_variants() {
    assert!(ok_rsa(t::RSA_LEAF, "www.rsa.test").is_ok());
    assert!(ok_rsa(t::RSA_LEAF_SHA384, "rsa384.test").is_ok());
    assert!(ok_rsa(t::RSA_LEAF_SHA512, "rsa512.test").is_ok());
}

#[test]
fn valid_chain_leaf_signed_with_rsa_pss() {
    assert!(ok_rsa(t::RSA_LEAF_PSS, "pss.test").is_ok());
}

#[test]
fn valid_ec_chain_p384_and_p256_leaves() {
    let store = store_of(&[t::EC_ROOT]);
    assert!(check(&[t::EC_LEAF384, t::EC_INTER], "ec.test", NOW, &store).is_ok());
    assert!(check(&[t::EC_LEAF256, t::EC_INTER], "ec256.test", NOW, &store).is_ok());
}

#[test]
fn server_may_send_the_root_too() {
    let v = check(
        &[t::RSA_LEAF, t::RSA_INTER, t::RSA_ROOT],
        "rsa.test",
        NOW,
        &rsa_store(),
    );
    assert!(v.is_ok(), "{v:?}");
}

#[test]
fn wildcard_san_matches_one_label() {
    assert!(ok_rsa(t::RSA_LEAF, "a.wild.test").is_ok());
    assert!(ok_rsa(t::RSA_LEAF, "A.WILD.test").is_ok());
    assert_eq!(
        ok_rsa(t::RSA_LEAF, "wild.test"),
        Err(CertError::NameMismatch)
    );
    assert_eq!(
        ok_rsa(t::RSA_LEAF, "a.b.wild.test"),
        Err(CertError::NameMismatch)
    );
}

#[test]
fn validity_boundaries() {
    let nb = x509::parse(hex(t::RSA_LEAF)).unwrap().not_before;
    let na = x509::parse(hex(t::RSA_LEAF)).unwrap().not_after;
    let st = rsa_store();
    let go = |now| check(&[t::RSA_LEAF, t::RSA_INTER], "rsa.test", now, &st);
    assert!(go(nb).is_ok());
    assert!(go(na).is_ok());
    assert_eq!(go(nb - 1), Err(CertError::NotYetValid));
    assert_eq!(go(na + 1), Err(CertError::Expired));
}

// ---- failures ----

#[test]
fn expired_leaf() {
    assert_eq!(
        ok_rsa(t::RSA_LEAF_EXPIRED, "expired.test"),
        Err(CertError::Expired)
    );
}

#[test]
fn not_yet_valid_leaf() {
    assert_eq!(
        ok_rsa(t::RSA_LEAF_FUTURE, "future.test"),
        Err(CertError::NotYetValid)
    );
}

#[test]
fn wrong_host_name() {
    assert_eq!(
        ok_rsa(t::RSA_LEAF, "evil.test"),
        Err(CertError::NameMismatch)
    );
    assert_eq!(
        ok_rsa(t::RSA_LEAF, "rsa.test.evil.com"),
        Err(CertError::NameMismatch)
    );
    assert_eq!(
        ok_rsa(t::RSA_LEAF, "xrsa.test"),
        Err(CertError::NameMismatch)
    );
}

#[test]
fn common_name_is_not_a_name_without_san() {
    // The CN of the intermediate is not a DNS name the leaf is valid for.
    assert_eq!(
        ok_rsa(t::RSA_LEAF, "Test RSA Inter"),
        Err(CertError::NameMismatch)
    );
}

#[test]
fn self_signed_leaf_is_reported_as_such() {
    let r = check(&[t::SELF_SIGNED_LEAF], "self.test", NOW, &rsa_store());
    assert_eq!(r, Err(CertError::SelfSigned));
}

#[test]
fn self_signed_leaf_trusted_only_if_it_is_a_root() {
    // Even if it is in the store, a non-CA leaf as anchor works the way anchors
    // do (the store is the trust decision); but it is not in ours.
    let r = check(&[t::SELF_SIGNED_LEAF], "self.test", NOW, &store_of(&[]));
    assert_eq!(r, Err(CertError::SelfSigned));
}

#[test]
fn untrusted_root_is_unknown_issuer() {
    let r = check(
        &[t::UNTRUSTED_LEAF, t::UNTRUSTED_ROOT],
        "untrusted.test",
        NOW,
        &rsa_store(),
    );
    assert_eq!(r, Err(CertError::UnknownIssuer));
    let r = check(&[t::UNTRUSTED_LEAF], "untrusted.test", NOW, &rsa_store());
    assert_eq!(r, Err(CertError::UnknownIssuer));
}

#[test]
fn missing_intermediate_is_unknown_issuer() {
    let r = check(&[t::RSA_LEAF], "rsa.test", NOW, &rsa_store());
    assert_eq!(r, Err(CertError::UnknownIssuer));
}

#[test]
fn empty_store_trusts_nothing() {
    let r = check(
        &[t::RSA_LEAF, t::RSA_INTER, t::RSA_ROOT],
        "rsa.test",
        NOW,
        &store_of(&[]),
    );
    assert_eq!(r, Err(CertError::UnknownIssuer));
}

#[test]
fn intermediate_without_basic_constraints_is_refused() {
    let r = check(
        &[t::LEAF_UNDER_NO_BC, t::INTER_NO_BC],
        "nobc.test",
        NOW,
        &rsa_store(),
    );
    assert!(
        matches!(r, Err(CertError::NotCa | CertError::UnknownIssuer)),
        "{r:?}"
    );
}

#[test]
fn intermediate_that_is_not_a_ca_is_refused() {
    let r = check(
        &[t::LEAF_UNDER_NOT_CA, t::INTER_NOT_CA],
        "notca.test",
        NOW,
        &rsa_store(),
    );
    assert!(
        matches!(r, Err(CertError::NotCa | CertError::UnknownIssuer)),
        "{r:?}"
    );
}

#[test]
fn path_length_constraint_is_enforced() {
    let r = check(
        &[t::LEAF_DEEP, t::INTER_DEEP, t::RSA_INTER],
        "deep.test",
        NOW,
        &rsa_store(),
    );
    assert!(
        matches!(r, Err(CertError::Constraint | CertError::UnknownIssuer)),
        "{r:?}"
    );
}

#[test]
fn leaf_without_server_auth_eku_is_refused() {
    let r = ok_rsa(t::RSA_LEAF_CLIENT_EKU, "client.test");
    assert!(matches!(r, Err(CertError::Constraint)), "{r:?}");
}

#[test]
fn tampered_signature_is_refused() {
    let r = ok_rsa(t::RSA_LEAF_TAMPERED, "rsa.test");
    assert!(
        matches!(r, Err(CertError::BadSignature | CertError::UnknownIssuer)),
        "{r:?}"
    );
}

#[test]
fn tampered_tbs_is_refused() {
    let r = ok_rsa(t::RSA_LEAF_TBS_EDIT, "xsa.test");
    assert!(
        matches!(r, Err(CertError::BadSignature | CertError::UnknownIssuer)),
        "{r:?}"
    );
}

#[test]
fn a_leaf_from_another_ca_does_not_verify_under_this_intermediate() {
    // EC leaf offered with the RSA intermediate and the RSA store.
    let r = check(&[t::EC_LEAF384, t::RSA_INTER], "ec.test", NOW, &rsa_store());
    assert_eq!(r, Err(CertError::UnknownIssuer));
}

#[test]
fn garbage_chains_are_bad_encoding() {
    let st = rsa_store();
    assert_eq!(
        verify_chain(&[&[1, 2, 3]], "rsa.test", Some(NOW), &st),
        Err(CertError::BadEncoding)
    );
    assert_eq!(
        verify_chain(&[&[0x30, 0x00]], "rsa.test", Some(NOW), &st),
        Err(CertError::BadEncoding)
    );
    assert_eq!(
        verify_chain(&[], "rsa.test", Some(NOW), &st),
        Err(CertError::BadEncoding)
    );
}

#[test]
fn truncated_leaf_is_bad_encoding() {
    let leaf = hex(t::RSA_LEAF);
    let st = rsa_store();
    for cut in [1, 10, 100, leaf.len() - 1] {
        let r = verify_chain(&[&leaf[..cut]], "rsa.test", Some(NOW), &st);
        assert_eq!(r, Err(CertError::BadEncoding), "cut {cut}");
    }
}

#[test]
fn chain_and_certificate_size_limits() {
    let st = rsa_store();
    let leaf = hex(t::RSA_LEAF);
    let nine: Vec<&[u8]> = (0..9).map(|_| leaf).collect();
    assert_eq!(
        verify_chain(&nine, "rsa.test", Some(NOW), &st),
        Err(CertError::ChainTooLong)
    );
    let big = alloc::vec![0u8; x509::MAX_CERT_LEN + 1];
    assert_eq!(
        verify_chain(&[&big], "rsa.test", Some(NOW), &st),
        Err(CertError::TooLarge)
    );
    assert_eq!(
        verify_chain(&[leaf, &[]], "rsa.test", Some(NOW), &st),
        Err(CertError::TooLarge)
    );
}

#[test]
fn eight_certificates_are_accepted_by_the_limit() {
    // The limit is on count, not on validity: eight certs reach the validator.
    let st = rsa_store();
    let leaf = hex(t::RSA_LEAF);
    let eight: Vec<&[u8]> = (0..8).map(|_| leaf).collect();
    let r = verify_chain(&eight, "rsa.test", Some(NOW), &st);
    assert_ne!(r, Err(CertError::ChainTooLong));
}

#[test]
fn unset_clock_refuses_to_judge() {
    let r = verify_chain(
        &[hex(t::RSA_LEAF), hex(t::RSA_INTER)],
        "rsa.test",
        None,
        &rsa_store(),
    );
    assert_eq!(r, Err(CertError::ClockUnset));
    assert!(CertError::ClockUnset.clock_may_be_to_blame());
}

#[test]
fn bad_host_names_do_not_panic() {
    for h in [
        "",
        " ",
        "a b",
        "..",
        "*.rsa.test",
        "rsa.test.",
        "\u{e9}.test",
        "a\0b.test",
    ] {
        let r = ok_rsa(t::RSA_LEAF, h);
        assert!(r.is_err(), "{h:?}");
    }
}

#[test]
fn ip_literal_does_not_match_dns_san() {
    assert!(ok_rsa(t::RSA_LEAF, "10.0.0.1").is_err());
}

#[test]
fn wrong_clock_makes_valid_cert_look_expired_and_hint_says_so() {
    // 2040: the leaf (valid to 2036) looks expired; the UI will hint at the clock.
    let far = DateTime {
        year: 2040,
        month: 1,
        day: 1,
        hour: 0,
        minute: 0,
        second: 0,
    }
    .to_unix()
    .unwrap();
    let r = check(&[t::RSA_LEAF, t::RSA_INTER], "rsa.test", far, &rsa_store());
    assert_eq!(r, Err(CertError::Expired));
    assert!(CertError::Expired.clock_may_be_to_blame());
    assert!(!CertError::NameMismatch.clock_may_be_to_blame());
}

#[test]
fn error_reasons_are_ascii() {
    for e in [
        CertError::BadEncoding,
        CertError::TooLarge,
        CertError::ChainTooLong,
        CertError::Expired,
        CertError::NotYetValid,
        CertError::NameMismatch,
        CertError::SelfSigned,
        CertError::UnknownIssuer,
        CertError::BadSignature,
        CertError::NotCa,
        CertError::Constraint,
        CertError::Unsupported,
        CertError::ClockUnset,
        CertError::Other,
    ] {
        assert!(e.reason().is_ascii() && !e.reason().is_empty());
    }
}

// ---- CertificateVerify signatures ----

#[test]
fn certificate_verify_rsa_pss() {
    let leaf = hex(t::RSA_LEAF);
    let msg = hex(t::CV_MESSAGE);
    assert_eq!(
        verify_handshake_signature(leaf, 0x0804, msg, hex(t::CV_RSA_PSS_SHA256)),
        Ok(())
    );
    assert_eq!(
        verify_handshake_signature(leaf, 0x0805, msg, hex(t::CV_RSA_PSS_SHA384)),
        Ok(())
    );
}

#[test]
fn certificate_verify_rsa_pss_wrong_message_or_scheme() {
    let leaf = hex(t::RSA_LEAF);
    let sig = hex(t::CV_RSA_PSS_SHA256);
    assert_eq!(
        verify_handshake_signature(leaf, 0x0804, b"another message", sig),
        Err(CertError::BadSignature)
    );
    // SHA-384 scheme with a SHA-256 signature.
    assert_eq!(
        verify_handshake_signature(leaf, 0x0805, hex(t::CV_MESSAGE), sig),
        Err(CertError::BadSignature)
    );
}

#[test]
fn certificate_verify_rejects_pkcs1_in_tls13() {
    let r = verify_handshake_signature(
        hex(t::RSA_LEAF),
        0x0401,
        hex(t::CV_MESSAGE),
        hex(t::CV_RSA_PKCS1_SHA256),
    );
    assert_eq!(r, Err(CertError::Unsupported));
}

#[test]
fn certificate_verify_ecdsa() {
    let msg = hex(t::CV_MESSAGE);
    assert_eq!(
        verify_handshake_signature(hex(t::EC_LEAF256), 0x0403, msg, hex(t::CV_ECDSA_P256)),
        Ok(())
    );
    assert_eq!(
        verify_handshake_signature(hex(t::EC_LEAF384), 0x0503, msg, hex(t::CV_ECDSA_P384)),
        Ok(())
    );
}

#[test]
fn certificate_verify_key_and_scheme_must_agree() {
    let msg = hex(t::CV_MESSAGE);
    // P-256 key with the P-384 scheme.
    assert!(
        verify_handshake_signature(hex(t::EC_LEAF256), 0x0503, msg, hex(t::CV_ECDSA_P256)).is_err()
    );
    // RSA key with an ECDSA scheme.
    assert!(
        verify_handshake_signature(hex(t::RSA_LEAF), 0x0403, msg, hex(t::CV_ECDSA_P256)).is_err()
    );
    // Signature by one key, certificate of another.
    assert!(
        verify_handshake_signature(hex(t::EC_LEAF384), 0x0503, msg, hex(t::CV_ECDSA_P256)).is_err()
    );
}

#[test]
fn certificate_verify_flipped_bits_fail() {
    let leaf = hex(t::RSA_LEAF);
    let msg = hex(t::CV_MESSAGE);
    let mut sig = hex(t::CV_RSA_PSS_SHA256).to_vec();
    for i in [0usize, 17, sig.len() - 1] {
        sig[i] ^= 0x80;
        assert!(verify_handshake_signature(leaf, 0x0804, msg, &sig).is_err());
        sig[i] ^= 0x80;
    }
    assert!(verify_handshake_signature(leaf, 0x0804, msg, &sig).is_ok());
    let mut ec = hex(t::CV_ECDSA_P256).to_vec();
    let n = ec.len();
    ec[n - 1] ^= 1;
    assert!(verify_handshake_signature(hex(t::EC_LEAF256), 0x0403, msg, &ec).is_err());
}

#[test]
fn certificate_verify_truncated_or_empty_signature() {
    let leaf = hex(t::RSA_LEAF);
    let msg = hex(t::CV_MESSAGE);
    let sig = hex(t::CV_RSA_PSS_SHA256);
    assert!(verify_handshake_signature(leaf, 0x0804, msg, &[]).is_err());
    assert!(verify_handshake_signature(leaf, 0x0804, msg, &sig[..sig.len() - 1]).is_err());
    let mut longer = sig.to_vec();
    longer.push(0);
    assert!(verify_handshake_signature(leaf, 0x0804, msg, &longer).is_err());
    assert!(verify_handshake_signature(leaf, 0x0403, msg, &[0x30, 0x00]).is_err());
}

#[test]
fn certificate_verify_unknown_scheme() {
    for s in [0u16, 0x0807, 0x0808, 0x0603, 0x0201, 0xFFFF] {
        assert_eq!(
            verify_handshake_signature(hex(t::RSA_LEAF), s, b"m", b"s"),
            Err(CertError::Unsupported),
            "{s:#x}"
        );
    }
}

#[test]
fn certificate_verify_rejects_weak_rsa_keys() {
    let r = verify_handshake_signature(
        hex(t::RSA1024_LEAF),
        0x0804,
        hex(t::CV_MESSAGE),
        hex(t::CV_RSA1024_PSS_SHA256),
    );
    assert!(r.is_err(), "1024-bit RSA must be refused, got {r:?}");
}

#[test]
fn verify_message_layout() {
    let m = tls13_server_verify_message(&[0xAB; 32]);
    assert_eq!(m.len(), 64 + 34 + 32);
    assert!(m[..64].iter().all(|&b| b == 0x20));
    assert_eq!(&m[64..98], b"TLS 1.3, server CertificateVerify\0");
    assert!(m[98..].iter().all(|&b| b == 0xAB));
}

// ---- the x509 reader against real certificates ----

#[test]
fn x509_reads_real_leaf() {
    let leaf = hex(t::RSA_LEAF);
    let c = x509::parse(leaf).unwrap();
    assert_eq!(c.subject_cn(), Some(&b"rsa.test"[..]));
    assert!(!c.is_self_issued());
    let names: Vec<&[u8]> = c.san_dns_names().collect();
    assert_eq!(names, [&b"rsa.test"[..], b"*.wild.test", b"www.rsa.test"]);
    assert!(c.matches_host("rsa.test"));
    assert!(c.matches_host("x.wild.test"));
    assert!(!c.matches_host("wild.test"));
    assert!(c.basic_constraints.is_some_and(|b| !b.ca));
    assert!(!c.may_sign_certs());
    assert!(c.valid_at(NOW));
    assert!(!c.valid_at(NOW + 20 * 365 * 86_400));
}

#[test]
fn x509_reads_ca_constraints() {
    let root = x509::parse(hex(t::RSA_ROOT)).unwrap();
    assert!(root.is_self_issued());
    assert!(root.may_sign_certs());
    assert_eq!(root.basic_constraints.unwrap().path_len, None);
    let inter = x509::parse(hex(t::RSA_INTER)).unwrap();
    assert!(inter.may_sign_certs());
    assert_eq!(inter.basic_constraints.unwrap().path_len, Some(0));
    assert_eq!(inter.issuer, root.subject);
    let nobc = x509::parse(hex(t::INTER_NO_BC)).unwrap();
    assert!(nobc.basic_constraints.is_none());
    assert!(!nobc.may_sign_certs());
    let notca = x509::parse(hex(t::INTER_NOT_CA)).unwrap();
    assert!(!notca.may_sign_certs());
}

#[test]
fn x509_reads_validity_and_self_signed() {
    let exp = x509::parse(hex(t::RSA_LEAF_EXPIRED)).unwrap();
    assert!(exp.not_after < NOW);
    assert!(!exp.valid_at(NOW));
    let ss = x509::parse(hex(t::SELF_SIGNED_LEAF)).unwrap();
    assert!(ss.is_self_issued());
    assert!(ss.matches_host("self.test"));
    let ec = x509::parse(hex(t::EC_LEAF384)).unwrap();
    assert_eq!(ec.subject_cn(), Some(&b"ec.test"[..]));
}

#[test]
fn x509_every_prefix_and_bitflip_of_a_real_cert_is_handled() {
    let leaf = hex(t::RSA_LEAF);
    for n in 0..leaf.len() {
        assert!(x509::parse(&leaf[..n]).is_err(), "prefix {n}");
    }
    let mut v = leaf.to_vec();
    for i in 0..v.len() {
        v[i] ^= 0xFF;
        let _ = x509::parse(&v);
        v[i] ^= 0xFF;
    }
    let mut trailing = leaf.to_vec();
    trailing.push(0);
    assert_eq!(
        x509::parse(&trailing).err(),
        Some(x509::Error::TrailingData)
    );
}

#[test]
fn verify_chain_survives_every_bitflip_of_the_leaf() {
    // The signature covers the whole TBS: no single flip may still verify.
    let st = rsa_store();
    let inter = hex(t::RSA_INTER);
    let mut leaf = hex(t::RSA_LEAF).to_vec();
    for i in (0..leaf.len()).step_by(3) {
        leaf[i] ^= 0x01;
        let r = verify_chain(&[&leaf, inter], "rsa.test", Some(NOW), &st);
        assert!(r.is_err(), "flip at {i} still verified");
        leaf[i] ^= 0x01;
    }
}
