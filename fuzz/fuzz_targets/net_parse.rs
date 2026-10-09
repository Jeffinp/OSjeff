//! Fuzz target: every network parser/responder in `kitsune_core::net`.
//!
//! The input is an arbitrary Ethernet frame (so also: truncated frames, bad
//! IHL, total-length > buffer, ARP with odd hlen/plen, huge ICMP, DHCP options
//! with len 0 / running past the end ...). Each frame is pushed through
//! `respond` with output buffers of several sizes (including ones too small for
//! the reply), through `parse_dhcp`, and through the checksum.
//!
//! The DHCP lease state machine (`kitsune_core::lease`), the DNS message parser,
//! cache and resolver (`kitsune_core::dns`), and the ICMP echo parser/ping
//! (`kitsune_core::icmp`) are driven from the same input: see `fuzz_lease`,
//! `fuzz_dns` and `fuzz_icmp`.
//!
//! A second "shaped" mode (top bit of the first input byte) rewrites the frame
//! into a structurally valid ARP / ICMP / DHCP packet and lets the remaining
//! bytes mutate the individual fields, so the fuzzer gets past the cheap
//! ethertype/protocol/magic-cookie checks.
#![no_main]

use libfuzzer_sys::fuzz_target;
use kitsune_core::net::{self, DnsServers, Ipv4, Mac};
use kitsune_core::{dns, icmp, lease};

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
    check_dhcp(frame, mac);
    let dhcp_off = 14 + 20 + 8;
    if frame.len() >= dhcp_off + 34 {
        let mut m = [0u8; 6];
        m.copy_from_slice(&frame[dhcp_off + 28..dhcp_off + 34]);
        check_dhcp(frame, Mac(m));
    }
}

/// Parse a DHCP reply and, whatever it carries, try to turn it into a
/// `NetConfig`: never a panic, and an accepted config satisfies the contract
/// the stack relies on (prefix 1..=30, usable address, no self-gateway).
fn check_dhcp(frame: &[u8], mac: Mac) {
    let Some(reply) = net::parse_dhcp(frame, mac) else {
        return;
    };
    // Same reply as an ACK, so the option handling is reached whatever the
    // message type the fuzzer picked.
    for r in [
        reply,
        net::DhcpReply {
            msg_type: net::DHCP_ACK,
            ..reply
        },
    ] {
        if let Ok(cfg) = net::NetConfig::from_ack(&r) {
            assert!((1..=30).contains(&cfg.prefix), "prefix {}", cfg.prefix);
            assert!(cfg.ip.0[0] != 0 && cfg.ip.0[0] != 127 && cfg.ip.0[0] < 224);
            assert_ne!(cfg.gateway, Some(cfg.ip));
            assert!(!cfg.dns.as_slice().contains(&cfg.ip));
            assert!(cfg.dns.len() <= net::MAX_DNS);
            assert_ne!(cfg.lease_secs, Some(0));
            // The boot-log form never panics either.
            let _ = format!("{cfg}");
        }
    }
}

/// Force a frame into a plausible shape of the protocol selected by `kind`.
fn shape(kind: u8, data: &[u8]) -> Vec<u8> {
    let mut f = data.to_vec();
    match kind % 4 {
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
        // IPv4 / ICMP echo reply or error to us, with valid checksums so the parser
        // gets past them and the quoted-datagram logic is reached.
        3 => {
            f.resize(f.len().max(14 + 20 + 8 + 28), 0);
            f[12] = 0x08;
            f[13] = 0x00;
            f[14] = 0x45;
            f[14 + 6] = 0;
            f[14 + 7] = 0;
            f[14 + 9] = 1;
            f[14 + 16..14 + 20].copy_from_slice(&OUR_IP.0);
            let total = (f.len() - 14).min(0xFFFF) as u16;
            f[14 + 2..14 + 4].copy_from_slice(&total.to_be_bytes());
            // Quoted datagram (for the error types) is an IPv4/ICMP echo request.
            f[14 + 20 + 8] = 0x45;
            f[14 + 20 + 8 + 9] = 1;
            f[14 + 20 + 8 + 20] = 8;
            fix_icmp_checksums(&mut f);
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

/// Recompute the IPv4 header and ICMP checksums of an IPv4/ICMP frame (when the
/// lengths are consistent) so mutated frames still pass the parser's checks.
fn fix_icmp_checksums(f: &mut [u8]) {
    if f.len() < 14 + 20 {
        return;
    }
    let ihl = usize::from(f[14] & 0x0F) * 4;
    let total = usize::from(u16::from_be_bytes([f[16], f[17]])).min(f.len() - 14);
    if ihl < 20 || total < ihl + 8 || 14 + ihl > f.len() {
        return;
    }
    let ip_end = 14 + ihl;
    f[24] = 0;
    f[25] = 0;
    let c = net::checksum(&f[14..ip_end]);
    f[24..26].copy_from_slice(&c.to_be_bytes());
    let end = 14 + total;
    f[ip_end + 2] = 0;
    f[ip_end + 3] = 0;
    let c = net::checksum(&f[ip_end..end]);
    f[ip_end + 2..ip_end + 4].copy_from_slice(&c.to_be_bytes());
}

/// ICMP: the parser must be total, a built request must carry valid checksums,
/// and a ping fed whatever the parser accepted must end in a consistent state.
fn fuzz_icmp(frame: &[u8], rest: &[u8]) {
    let byte = |i: usize| rest.get(i).copied().unwrap_or(0);
    let mut shaped = frame.to_vec();
    fix_icmp_checksums(&mut shaped);
    for f in [frame, &shaped[..]] {
        if let Some(ev) = icmp::parse_event(f, OUR_IP) {
            let target = Ipv4([byte(0), byte(1), byte(2), byte(3)]);
            let mut p = icmp::Ping::new(target, u16::from(byte(4)), u16::from(byte(5)), 0, 100);
            let _ = p.on_event(1_000, &ev);
            // Once finished (reply, error or timeout) a ping never reports again.
            let first = p.poll(u64::MAX);
            assert!(p.poll(u64::MAX).is_none());
            let _ = first;
        }
    }
    let _ = net::parse_arp_reply(frame, OUR_IP);

    let payload = usize::from(byte(6)) * 6;
    let e = icmp::Echo {
        src_mac: OUR_MAC,
        dst_mac: Mac([byte(7), byte(8), byte(9), byte(10), byte(11), byte(12)]),
        src_ip: OUR_IP,
        dst_ip: Ipv4([byte(13), byte(14), byte(15), byte(16)]),
        id: u16::from_le_bytes([byte(17), byte(18)]),
        seq: u16::from_le_bytes([byte(19), byte(20)]),
        payload,
    };
    for size in [0, 41, 42, 60, 1514, 1600] {
        let mut out = vec![0u8; size];
        if let Some(n) = icmp::build_echo_request(&mut out, &e) {
            assert!(n <= size && n == icmp::frame_len(payload));
            assert_eq!(net::checksum(&out[14..34]), 0);
            assert_eq!(net::checksum(&out[34..n]), 0);
        }
    }
    let nh = icmp::next_hop(e.dst_ip, OUR_IP, byte(21) % 40, Some(Ipv4([10, 0, 2, 2])));
    if let Some(h) = nh {
        assert!(h.0[0] != 0 && h.0[0] < 224);
    }
}

/// DNS: parser, name normalizer, cache and resolver must never panic and the
/// resolver must always terminate.
fn fuzz_dns(rest: &[u8]) {
    let name_len = usize::from(rest.first().copied().unwrap_or(0) % 40);
    let body = rest.get(1..).unwrap_or(&[]);
    let name_bytes = &body[..name_len.min(body.len())];
    let name = String::from_utf8_lossy(name_bytes).into_owned();
    let msg = &body[name_len.min(body.len())..];
    let b = |i: usize| msg.get(i).copied().unwrap_or(0);
    let id = u16::from_le_bytes([b(0), b(1)]);

    let _ = dns::parse_response(msg, id, &name);
    if let Some(n) = dns::normalize_name(&name) {
        assert!(n.len() <= dns::MAX_NAME && !n.ends_with('.'));
        // Shaped response: valid question for `n`, answers are the fuzz bytes.
        let mut q = vec![0u8; 12 + n.len() + 8];
        if let Some(len) = dns::build_query(&mut q, id, &n) {
            let mut m = q[..len].to_vec();
            m[2] |= 0x80;
            m[3] = (m[3] & 0xF0) | (b(2) & 0x0F);
            m[6..8].copy_from_slice(&u16::from(b(3)).to_be_bytes());
            m.extend_from_slice(msg.get(4..).unwrap_or(&[]));
            let _ = dns::parse_response(&m, id, &n);
            // Drive a lookup with it from the first/second server.
            let servers = DnsServers::from_slice(&[Ipv4([10, 0, 2, 3]), Ipv4([8, 8, 8, 8])]);
            let mut r = dns::Resolve::new(&n, servers, usize::from(b(2)), id, 0);
            let mut t = 0u64;
            for step in 0..64 {
                match r.poll(t) {
                    dns::Step::Done(_) => break,
                    dns::Step::Wait(u) => t = u.max(t + 1),
                    dns::Step::Send { server, .. } => {
                        let from = if step % 3 == 0 {
                            Ipv4([6, 6, 6, 6])
                        } else {
                            server
                        };
                        let _ = r.on_response(from, &m);
                    }
                }
            }
            assert!(
                t <= dns::TOTAL_TIMEOUT_MS + 64,
                "resolver ran past its bound"
            );
        }
        let mut c = dns::DnsCache::new();
        c.insert_addr(&n, Ipv4([1, 2, 3, 4]), u32::from(id) * 97, 5);
        let _ = c.get(&n, 5 + u64::from(id));
        c.insert_nxdomain(&name, 7);
        c.purge(u64::from(id) * 1000);
    }
    let mut out = [0u8; 300];
    if let Some(len) = dns::build_query(&mut out, id, &name) {
        assert!(len <= out.len());
    }
}

/// DHCP lease machine: bytes are a program of ticks and replies. Whatever the
/// order, the config exists exactly while a lease is held and no call panics.
fn fuzz_lease(rest: &[u8]) {
    let mut c = lease::Lease::new(OUR_MAC, u32::from(rest.first().copied().unwrap_or(0)) | 1);
    let mut now = 0u64;
    c.start(0);
    let mut it = rest.iter().copied();
    while let (Some(op), Some(arg)) = (it.next(), it.next()) {
        now += u64::from(arg) * u64::from(arg) * 13; // up to ~850 s per step
        match op % 6 {
            0 | 1 => {
                for _ in 0..4 {
                    if c.poll(now).is_none() {
                        break;
                    }
                }
            }
            2..=4 => {
                let kind = [net::DHCP_OFFER, net::DHCP_ACK, net::DHCP_NAK][usize::from(op % 3)];
                let lease_secs = match arg % 5 {
                    0 => None,
                    1 => Some(0),
                    2 => Some(20),
                    3 => Some(3600),
                    _ => Some(u32::MAX - 1),
                };
                let r = net::DhcpReply {
                    msg_type: kind,
                    xid: if arg & 0x80 != 0 {
                        c.xid() ^ 1
                    } else {
                        c.xid()
                    },
                    your_ip: Ipv4([10, 0, 2, arg]),
                    server_id: (arg & 0x40 == 0).then_some(Ipv4([10, 0, 2, 2])),
                    subnet: Some(Ipv4([255, 255, 255, 0])),
                    router: Some(Ipv4([10, 0, 2, 2])),
                    dns: DnsServers::from_slice(&[Ipv4([10, 0, 2, 3]), Ipv4([1, 1, 1, arg])]),
                    lease_secs,
                    eth_src: Mac([2, 0, 0, 0, 0, arg]),
                };
                let _ = c.on_reply(now, &r);
            }
            _ => {
                if arg % 7 == 0 {
                    let _ = c.release();
                    c.start(now);
                } else {
                    let _ = c.next_deadline();
                    let _ = c.remaining_ms(now);
                }
            }
        }
        let held = matches!(
            c.state(),
            lease::State::Bound | lease::State::Renewing | lease::State::Rebinding
        );
        assert_eq!(c.config().is_some(), held);
        if let Some(cfg) = c.config() {
            assert!(!cfg.dns.as_slice().contains(&cfg.ip));
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    if mode & 0x80 == 0 {
        // Raw mode: the bytes are the frame.
        run_frame(rest, OUR_MAC);
        fuzz_icmp(rest, rest);
    } else {
        let f = shape(mode, rest);
        run_frame(&f, OUR_MAC);
        fuzz_icmp(&f, rest);
    }
    fuzz_dns(rest);
    fuzz_lease(rest);

    // Builders with a sufficiently large buffer must never panic, whatever the
    // identity / xid / addresses.
    // IPv4 literal parser (hostnames typed in the browser) must be total, and
    // anything it accepts must print back to the same bytes.
    if let Some(a) = net::parse_ipv4(rest) {
        assert_eq!(format!("{a}").as_bytes(), rest);
    }

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
