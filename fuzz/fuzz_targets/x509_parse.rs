//! Fuzz target: everything the browser does with a server-supplied certificate
//! chain: the strict DER / X.509 reader (`osjeff_core::x509`), the SAN/name
//! matcher, the time parser and the full chain validation
//! (`osjeff_core::tlsverify::verify_chain` on `rustls-webpki`, with a real
//! trust anchor), plus the TLS 1.3 `CertificateVerify` check.
//!
//! Input layout: `[mode, ...bytes]`.
//!
//! * `mode & 3`: how many certificates the input is split into (1..=4 equal-ish
//!   parts, each one a DER blob) for the chain validation.
//! * `mode & 4` / `mode & 16`: the input replaces the leaf (resp. the
//!   intermediate) of a valid test chain, so the signature path is reached.
//! * `mode & 8`: also run the `CertificateVerify` path with the input as the
//!   signature (scheme chosen from `mode >> 5`).
//!
//! The assertion is simply "never panics, never hangs".
#![no_main]

use libfuzzer_sys::fuzz_target;
use osjeff_core::tlsverify::{self, TrustStore};
use osjeff_core::x509;

/// 2026-10-07T12:00:00Z
const NOW: u64 = 1_791_374_400;

/// The chains generated for the unit tests (`tools/gen-test-certs.py`), as hex:
/// they give the fuzzer a real root, intermediate and leaf to mutate.
#[allow(dead_code)]
mod testcerts {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../osjeff_core/src/tlsverify/testcerts.rs"
    ));
}

/// Decode a hex constant once (cached by address) so the harness itself does
/// not leak on every execution.
fn unhex(s: &'static str) -> &'static [u8] {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<HashMap<usize, &'static [u8]>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    cache.entry(s.as_ptr() as usize).or_insert_with(|| {
        let v: Vec<u8> = (0..s.len() / 2)
            .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        Box::leak(v.into_boxed_slice())
    })
}

fn exercise_parser(der: &[u8]) {
    if let Ok(c) = x509::parse(der) {
        let _ = (c.subject_cn(), c.is_self_issued(), c.may_sign_certs());
        let _ = (
            c.valid_at(NOW),
            c.has_san(),
            c.key_usage,
            c.basic_constraints,
        );
        for n in c.san_dns_names() {
            let _ = x509::dns_name_matches(n, b"example.com");
            let _ = x509::dns_name_matches(n, b"a.b.example.com");
        }
        let _ = c.matches_host("example.com");
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };

    // Reader, names and time on the raw bytes.
    exercise_parser(rest);
    let _ = x509::parse_time(x509::TAG_UTCTIME, rest);
    let _ = x509::parse_time(x509::TAG_GENTIME, rest);
    let _ = x509::dns_name_matches(rest, b"www.example.com");
    if let Ok(host) = core::str::from_utf8(rest) {
        let _ = x509::dns_name_matches(b"*.example.com", host.as_bytes());
    }
    let mut r = x509::Reader::new(rest);
    while !r.is_empty() {
        if r.read().is_err() {
            break;
        }
    }

    // Chain validation against a real trust anchor (the first embedded root).
    let embedded = TrustStore::embedded();
    let Some(root) = embedded.der(0) else { return };
    let store = TrustStore::from_certs(vec![root]);

    let parts = usize::from(mode & 3) + 1;
    let chunk = rest.len().div_ceil(parts).max(1);
    let chain: Vec<&[u8]> = rest.chunks(chunk).collect();
    let _ = tlsverify::verify_chain(&chain, "example.com", Some(NOW), &store);
    let _ = tlsverify::verify_chain(&chain, "example.com", None, &store);

    // Mutated leaf or intermediate under a valid test root: reaches signature
    // checking, validity, name and constraint code with plausible structure.
    if mode & 4 != 0 || mode & 16 != 0 {
        let test_store = TrustStore::from_certs(vec![unhex(testcerts::RSA_ROOT)]);
        if mode & 4 != 0 {
            let inter = unhex(testcerts::RSA_INTER);
            let _ = tlsverify::verify_chain(&[rest, inter], "rsa.test", Some(NOW), &test_store);
        }
        if mode & 16 != 0 {
            let leaf = unhex(testcerts::RSA_LEAF);
            let _ = tlsverify::verify_chain(&[leaf, rest], "rsa.test", Some(NOW), &test_store);
        }
    }

    if mode & 8 != 0 {
        let scheme = [
            0x0403, 0x0503, 0x0804, 0x0805, 0x0806, 0x0401, 0x0807, 0,
        ][usize::from(mode >> 5)];
        let split = rest.len() / 2;
        let _ = tlsverify::verify_handshake_signature(
            &rest[..split],
            scheme,
            b"message",
            &rest[split..],
        );
        // Also with real leaf keys, so the signature parsers see the bytes.
        let _ = tlsverify::verify_handshake_signature(
            unhex(testcerts::RSA_LEAF),
            scheme,
            unhex(testcerts::CV_MESSAGE),
            rest,
        );
        let _ = tlsverify::verify_handshake_signature(
            unhex(testcerts::EC_LEAF256),
            scheme,
            unhex(testcerts::CV_MESSAGE),
            rest,
        );
    }
});
