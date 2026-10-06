//! Fuzz target: every network parser/responder in `osjeff_core::net`.
//!
//! The input is an arbitrary Ethernet frame (so also: truncated frames, bad
//! IHL, total-length > buffer, ARP with odd hlen/plen, huge ICMP, DHCP options
//! with len 0 / running past the end ...). Each frame is pushed through
//! `respond` with output buffers of several sizes (including ones too small for
//! the reply), through `parse_dhcp`, and through the checksum.
//!
//! A second "shaped" mode (top bit of the first input byte) rewrites the frame
//! into a structurally valid ARP / ICMP / DHCP packet and lets the remaining
//! bytes mutate the individual fields, so the fuzzer gets past the cheap
//! ethertype/protocol/magic-cookie checks.
#![no_main]

use libfuzzer_sys::fuzz_target;
use osjeff_core::net::{self, Ipv4, Mac};

const OUR_MAC: Mac = Mac([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
const OUR_IP: Ipv4 = Ipv4([10, 0, 2, 15]);

/// Output buffer sizes: empty, tiny, around the ARP/ICMP minimums, MTU-ish.
const OUT_SIZES: [usize; 12] = [0, 1, 13, 14, 41, 42, 43, 60, 64, 128, 600, 1600];

fn run_frame(frame: &[u8], mac: Mac) {
    // checksum over arbitrary data must be total.
    let _ = net::checksum(frame);

    for &sz in &OUT_SIZES {
        let mut out = vec![0u8; sz];
        if let Some(n) = net::respond(frame, mac, OUR_IP, &mut out) {
            assert!(n <= sz, "respond returned {n} > out buffer {sz}");
            // A produced ICMP reply must carry valid checksums.
            if n >= 14 + 20 + 8 && out[12] == 0x08 && out[13] == 0x00 && out[14 + 9] == 1 {
                assert_eq!(net::checksum(&out[14..34]), 0, "bad IPv4 hdr csum");
                assert_eq!(net::checksum(&out[34..n]), 0, "bad ICMP csum");
            }
        }
    }

    // DHCP parser: also try with the MAC the frame claims in chaddr, so the
    // `chaddr == our_mac` gate does not hide the option loop.
    let _ = net::parse_dhcp(frame, mac);
    let dhcp_off = 14 + 20 + 8;
    if frame.len() >= dhcp_off + 34 {
        let mut m = [0u8; 6];
        m.copy_from_slice(&frame[dhcp_off + 28..dhcp_off + 34]);
        let _ = net::parse_dhcp(frame, Mac(m));
    }
}

/// Force a frame into a plausible shape of the protocol selected by `kind`.
fn shape(kind: u8, data: &[u8]) -> Vec<u8> {
    let mut f = data.to_vec();
    match kind % 3 {
        // ARP request for us.
        0 => {
            f.resize(f.len().max(14 + 28), 0);
            f[12] = 0x08;
            f[13] = 0x06;
            f[14 + 6] = 0;
            f[14 + 7] = 1;
            f[14 + 24..14 + 28].copy_from_slice(&OUR_IP.0);
        }
        // IPv4 / ICMP echo request to us.
        1 => {
            f.resize(f.len().max(14 + 20 + 8), 0);
            f[12] = 0x08;
            f[13] = 0x00;
            f[14 + 9] = 1;
            f[14 + 16..14 + 20].copy_from_slice(&OUR_IP.0);
            if f[14] & 1 == 0 {
                // ICMP type = echo request (offset depends on IHL; use 20).
                f[14 + 20] = 8;
            }
        }
        // IPv4 / UDP / DHCP BOOTREPLY with our MAC and a valid magic cookie.
        _ => {
            f.resize(f.len().max(14 + 20 + 8 + 240 + 8), 0);
            f[12] = 0x08;
            f[13] = 0x00;
            f[14] = 0x45;
            f[14 + 9] = 17;
            f[14 + 20 + 2] = 0;
            f[14 + 20 + 3] = 68;
            let b = 14 + 20 + 8;
            f[b] = 2;
            f[b + 28..b + 34].copy_from_slice(&OUR_MAC.0);
            f[b + 236..b + 240].copy_from_slice(&[0x63, 0x82, 0x53, 0x63]);
        }
    }
    f
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    if mode & 0x80 == 0 {
        // Raw mode: the bytes are the frame.
        run_frame(rest, OUR_MAC);
    } else {
        run_frame(&shape(mode, rest), OUR_MAC);
    }

    // Builders with a sufficiently large buffer must never panic, whatever the
    // identity / xid / addresses.
    let byte = |i: usize| rest.get(i).copied().unwrap_or(0);
    let mac = Mac([byte(0), byte(1), byte(2), byte(3), byte(4), byte(5)]);
    let ip = Ipv4([byte(6), byte(7), byte(8), byte(9)]);
    let xid = u32::from_le_bytes([byte(6), byte(7), byte(8), byte(9)]);
    let mut out = [0u8; 600];
    let n = net::arp_announce(&mut out, mac, ip);
    assert!(n <= out.len());
    let n = net::dhcp_discover(&mut out, mac, xid);
    assert!(n <= out.len());
    let n = net::dhcp_request(&mut out, mac, xid, ip, ip);
    assert!(n <= out.len());
    // What we build must carry a valid IPv4 header checksum.
    assert_eq!(net::checksum(&out[14..34]), 0);
});
