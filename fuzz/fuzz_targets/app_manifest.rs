//! Fuzz target: the app package reader (`kitsune_core::{wasmsec, appmanifest}`).
//!
//! The first input byte picks how the rest is presented:
//!
//! * `0`: the bytes as a whole `.wasm` file (header, section walk, LEB128 sizes);
//! * `1`: the bytes wrapped as the payload of a well-formed `kitsune.manifest`
//!   custom section, so the fuzzer reaches the key/value parser directly;
//! * `2`: the bytes wrapped as an `kitsune.icon` section next to a valid manifest
//!   (PNG header checks, size/dimension limits, PNG decoding);
//! * `3`: the bytes as a bare manifest (`Manifest::parse`) and as a bare icon.
//!
//! Nothing may panic, loop or allocate without a bound; a manifest that parses
//! must satisfy its own invariants (id grammar, quotas under the ceilings).
#![no_main]

use libfuzzer_sys::fuzz_target;
use kitsune_core::appmanifest::{
    self, MAX_FDS, MAX_FUEL_FRAME, MAX_MEM_MIB, MAX_NET_HOSTS, MAX_NET_HOSTS_LEN, Manifest,
};
use kitsune_core::appnet;
use kitsune_core::wasmsec;

const MIN: &[u8] = b"id=fz\nname=Fz\nversion=1.0.0\n";

fn leb(v: &mut Vec<u8>, mut n: u32) {
    loop {
        let b = (n & 0x7F) as u8;
        n >>= 7;
        if n == 0 {
            v.push(b);
            return;
        }
        v.push(b | 0x80);
    }
}

fn section(name: &str, data: &[u8]) -> Vec<u8> {
    let mut p = Vec::new();
    leb(&mut p, name.len() as u32);
    p.extend_from_slice(name.as_bytes());
    p.extend_from_slice(data);
    let mut s = vec![0u8];
    leb(&mut s, p.len() as u32);
    s.extend_from_slice(&p);
    s
}

fn check(m: &Manifest) {
    assert!(appmanifest::valid_id(&m.id));
    let q = m.granted();
    assert!(q.mem_bytes <= (MAX_MEM_MIB as usize) << 20);
    assert!(q.fuel_frame <= MAX_FUEL_FRAME);
    assert!(q.max_fds >= 1 && q.max_fds <= MAX_FDS as usize);
    assert!(m.win_min_w <= m.win_w && m.win_min_h <= m.win_h);
    // `parse` already refused anything above the ceilings, so nothing is clamped.
    assert_eq!(q.mem_bytes, (m.mem_mib as usize) << 20);
    assert_eq!(q.fuel_frame, m.fuel_frame);
    // `net_hosts`: a bounded list of public names, only with the network permission,
    // and the allow-list never admits what the destination filter refuses.
    assert!(m.net_hosts.len() <= MAX_NET_HOSTS);
    assert!(m.net_hosts.is_empty() || m.net.allows_http());
    let total: usize = m.net_hosts.iter().map(|h| h.len() + 1).sum();
    assert!(total <= MAX_NET_HOSTS_LEN + 1);
    for e in &m.net_hosts {
        let base = e.strip_prefix("*.").unwrap_or(e);
        assert!(appnet::host_allowed(base), "{e}");
        assert!(appnet::host_permitted(std::slice::from_ref(e), base) == (base == e));
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    match mode % 4 {
        0 => {
            if let Ok(it) = wasmsec::Sections::new(rest) {
                for s in it {
                    let _ = s;
                }
            }
            if let Ok(Some(p)) = appmanifest::parse_package_opt(rest) {
                check(&p.manifest);
            }
        }
        1 => {
            let mut w = wasmsec::HEADER.to_vec();
            w.extend_from_slice(&section(appmanifest::MANIFEST_SECTION, rest));
            if let Ok(p) = appmanifest::parse_package(&w) {
                check(&p.manifest);
            }
        }
        2 => {
            let mut w = wasmsec::HEADER.to_vec();
            w.extend_from_slice(&section(appmanifest::MANIFEST_SECTION, MIN));
            w.extend_from_slice(&section(appmanifest::ICON_SECTION, rest));
            if let Ok(p) = appmanifest::parse_package(&w) {
                check(&p.manifest);
                if let Some(i) = p.icon {
                    assert!(i.width() <= 64 && i.height() <= 64);
                }
            }
        }
        _ => {
            if let Ok(m) = Manifest::parse(rest) {
                check(&m);
            }
            let _ = appmanifest::decode_icon(rest);
        }
    }
});
