use super::*;

const ME_MAC: Mac = Mac([0x52, 0x54, 0, 0x12, 0x34, 0x56]);
const GW_MAC: Mac = Mac([0x52, 0x55, 10, 0, 2, 2]);
const ME: Ipv4 = Ipv4([10, 0, 2, 15]);
const GW: Ipv4 = Ipv4([10, 0, 2, 2]);

fn echo(id: u16, seq: u16, payload: usize) -> Echo {
    Echo {
        src_mac: ME_MAC,
        dst_mac: GW_MAC,
        src_ip: ME,
        dst_ip: GW,
        id,
        seq,
        payload,
    }
}

fn req(id: u16, seq: u16, payload: usize) -> Vec<u8> {
    let mut b = vec![0u8; frame_len(payload)];
    let n = build_echo_request(&mut b, &echo(id, seq, payload)).unwrap();
    b.truncate(n);
    b
}

/// Turn our own request into the reply a peer would send (swap addresses, type 0).
fn reply_to(req: &[u8]) -> Vec<u8> {
    let mut r = req.to_vec();
    r[0..6].copy_from_slice(&ME_MAC.0);
    r[6..12].copy_from_slice(&GW_MAC.0);
    let (src, dst) = (r[26..30].to_vec(), r[30..34].to_vec());
    r[26..30].copy_from_slice(&dst);
    r[30..34].copy_from_slice(&src);
    r[24..26].fill(0);
    let c = checksum(&r[14..34]);
    r[24..26].copy_from_slice(&c.to_be_bytes());
    r[34] = ICMP_ECHO_REPLY;
    r[36..38].fill(0);
    let c = checksum(&r[34..]);
    r[36..38].copy_from_slice(&c.to_be_bytes());
    r
}

/// An ICMP error from `router` quoting the first `quote` bytes of the IP
/// datagram of `orig` (header + 8 bytes by default).
fn error_for(orig: &[u8], router: Ipv4, ty: u8, code: u8, quote: usize) -> Vec<u8> {
    let q = &orig[14..14 + quote];
    let mut icmp = vec![ty, code, 0, 0, 0, 0, 0, 0];
    icmp.extend_from_slice(q);
    let c = checksum(&icmp);
    icmp[2..4].copy_from_slice(&c.to_be_bytes());
    let mut f = vec![0u8; 14 + 20];
    f[0..6].copy_from_slice(&ME_MAC.0);
    f[6..12].copy_from_slice(&GW_MAC.0);
    f[12..14].copy_from_slice(&0x0800u16.to_be_bytes());
    f[14] = 0x45;
    f[16..18].copy_from_slice(&((20 + icmp.len()) as u16).to_be_bytes());
    f[22] = 64;
    f[23] = 1;
    f[26..30].copy_from_slice(&router.0);
    f[30..34].copy_from_slice(&ME.0);
    let c = checksum(&f[14..34]);
    f[24..26].copy_from_slice(&c.to_be_bytes());
    f.extend_from_slice(&icmp);
    f
}

#[test]
fn request_has_valid_checksums_and_layout() {
    for payload in [0, 1, 32, 56, MAX_PAYLOAD] {
        let f = req(0x1234, 7, payload);
        assert_eq!(f.len(), 14 + 20 + 8 + payload);
        assert_eq!(&f[0..6], &GW_MAC.0);
        assert_eq!(&f[6..12], &ME_MAC.0);
        assert_eq!(checksum(&f[14..34]), 0, "IP header");
        assert_eq!(checksum(&f[34..]), 0, "ICMP");
        assert_eq!(f[34], ICMP_ECHO_REQUEST);
        assert_eq!(&f[38..40], &[0x12, 0x34]);
        assert_eq!(&f[40..42], &[0, 7]);
        assert_eq!(&f[26..30], &ME.0);
        assert_eq!(&f[30..34], &GW.0);
    }
}

#[test]
fn request_size_limits() {
    let mut b = [0u8; 100];
    assert_eq!(build_echo_request(&mut b, &echo(1, 1, 100)), None);
    assert_eq!(build_echo_request(&mut b, &echo(1, 1, 58)), Some(100));
    assert_eq!(build_echo_request(&mut b, &echo(1, 1, 59)), None);
    let mut big = vec![0u8; 3000];
    assert_eq!(
        build_echo_request(&mut big, &echo(1, 1, MAX_PAYLOAD + 1)),
        None
    );
}

#[test]
fn reply_is_recognized() {
    let f = reply_to(&req(0xBEEF, 3, 32));
    let ev = parse_event(&f, ME).unwrap();
    assert_eq!(ev.from, GW);
    assert_eq!(ev.kind, Kind::EchoReply { id: 0xBEEF, seq: 3 });
    // Not for us / damaged / own request (type 8) / fragment.
    assert_eq!(parse_event(&f, Ipv4([10, 0, 2, 99])), None);
    let mut bad = f.clone();
    bad[40] ^= 1; // payload bit flip breaks the ICMP checksum
    assert_eq!(parse_event(&bad, ME), None);
    let mut bad = f.clone();
    bad[20] = 0x20; // more-fragments flag
    assert_eq!(parse_event(&bad, ME), None);
    assert_eq!(
        parse_event(&req(1, 1, 8), GW),
        None,
        "an echo request is not an event"
    );
}

#[test]
fn matching_reply_gives_the_round_trip() {
    let r = req(9, 1, 32);
    let mut p = Ping::new(GW, 9, 1, 1_000, 2_000);
    let ev = parse_event(&reply_to(&r), ME).unwrap();
    assert_eq!(p.on_event(1_750, &ev), Some(Ok(750)));
    // Finished: nothing more, not even a timeout.
    assert_eq!(p.on_event(1_800, &ev), None);
    assert_eq!(p.poll(10_000_000), None);
}

#[test]
fn replies_for_other_flows_are_ignored() {
    let r = req(9, 1, 32);
    let ev = parse_event(&reply_to(&r), ME).unwrap();
    for (id, seq) in [(8, 1), (9, 2)] {
        let mut p = Ping::new(GW, id, seq, 0, 1000);
        assert_eq!(p.on_event(10, &ev), None);
    }
    // Right id/seq but from another host.
    let mut p = Ping::new(Ipv4([10, 0, 2, 3]), 9, 1, 0, 1000);
    assert_eq!(p.on_event(10, &ev), None);
}

#[test]
fn timeout_fires_exactly_at_the_deadline() {
    let mut p = Ping::new(GW, 1, 1, 5_000, 100);
    assert_eq!(p.deadline_us(), 105_000);
    assert_eq!(p.poll(104_999), None);
    assert_eq!(p.poll(105_000), Some(Err(PingError::Timeout)));
    assert_eq!(p.poll(200_000), None, "reported once");
    // A late reply after the timeout does not resurrect it.
    let ev = parse_event(&reply_to(&req(1, 1, 8)), ME).unwrap();
    assert_eq!(p.on_event(200_001, &ev), None);
}

#[test]
fn unreachable_errors_end_the_ping() {
    let r = req(0x42, 5, 32);
    let router = Ipv4([10, 0, 2, 1]);
    for code in [0u8, 1, 3, 4] {
        let e = error_for(&r, router, ICMP_DEST_UNREACHABLE, code, 28);
        let ev = parse_event(&e, ME).unwrap();
        assert_eq!(ev.from, router);
        let mut p = Ping::new(GW, 0x42, 5, 0, 1000);
        assert_eq!(p.on_event(10, &ev), Some(Err(PingError::Unreachable(code))));
    }
    let e = error_for(&r, router, ICMP_TIME_EXCEEDED, 0, 28);
    let ev = parse_event(&e, ME).unwrap();
    let mut p = Ping::new(GW, 0x42, 5, 0, 1000);
    assert_eq!(p.on_event(10, &ev), Some(Err(PingError::TimeExceeded)));
}

#[test]
fn errors_about_other_traffic_are_ignored() {
    let r = req(0x42, 5, 32);
    let e = error_for(&r, GW, ICMP_DEST_UNREACHABLE, 1, 28);
    let ev = parse_event(&e, ME).unwrap();
    // Another seq, another id, another destination.
    for (id, seq, dst) in [(0x42, 6, GW), (0x43, 5, GW), (0x42, 5, Ipv4([1, 1, 1, 1]))] {
        let mut p = Ping::new(dst, id, seq, 0, 1000);
        assert_eq!(p.on_event(10, &ev), None);
    }
    // A quote shorter than IP header + 8 bytes, or of a non-echo datagram, is dropped.
    assert_eq!(parse_event(&error_for(&r, GW, 3, 1, 24), ME), None);
    let mut udp = r.clone();
    udp[23] = 17; // quoted protocol UDP
    assert_eq!(parse_event(&error_for(&udp, GW, 3, 3, 28), ME), None);
}

#[test]
fn parse_event_never_panics_on_damage() {
    let r = req(1, 1, 16);
    let frames = [reply_to(&r), error_for(&r, GW, 3, 1, 28)];
    let mut s = 0x1234_5678_9ABC_DEF1u64;
    for f in frames {
        for cut in 0..f.len() {
            let _ = parse_event(&f[..cut], ME);
        }
        for _ in 0..3000 {
            let mut m = f.clone();
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let i = (s as usize) % m.len();
            m[i] = (s >> 24) as u8;
            if let Some(ev) = parse_event(&m, ME) {
                let mut p = Ping::new(GW, 1, 1, 0, 10);
                let _ = p.on_event(1, &ev);
            }
        }
    }
}

#[test]
fn ihl_options_are_skipped() {
    // Reply with IP options (IHL 6): the ICMP message starts 4 bytes later.
    let r = reply_to(&req(2, 2, 8));
    let mut f = Vec::new();
    f.extend_from_slice(&r[..14]);
    let mut ip = r[14..34].to_vec();
    ip[0] = 0x46;
    let total = u16::from_be_bytes([ip[2], ip[3]]) + 4;
    ip[2..4].copy_from_slice(&total.to_be_bytes());
    ip[10..12].fill(0);
    ip.extend_from_slice(&[1, 1, 1, 0]); // NOPs + end
    let c = checksum(&ip);
    ip[10..12].copy_from_slice(&c.to_be_bytes());
    f.extend_from_slice(&ip);
    f.extend_from_slice(&r[34..]);
    let ev = parse_event(&f, ME).unwrap();
    assert_eq!(ev.kind, Kind::EchoReply { id: 2, seq: 2 });
}

#[test]
fn next_hop_rules() {
    // On-link goes direct, off-link through the gateway.
    assert_eq!(next_hop(GW, ME, 24, Some(GW)), Some(GW));
    assert_eq!(
        next_hop(Ipv4([8, 8, 8, 8]), ME, 24, Some(GW)),
        Some(GW),
        "off-link via gateway"
    );
    assert_eq!(
        next_hop(Ipv4([8, 8, 8, 8]), ME, 24, None),
        None,
        "no gateway"
    );
    assert_eq!(
        next_hop(Ipv4([10, 0, 2, 7]), ME, 24, None),
        Some(Ipv4([10, 0, 2, 7]))
    );
    // Bad targets.
    for bad in [
        Ipv4([0, 0, 0, 0]),
        Ipv4([127, 0, 0, 1]),
        Ipv4([224, 0, 0, 1]),
        Ipv4([255, 255, 255, 255]),
        ME,
        Ipv4([10, 0, 2, 255]), // subnet broadcast
        Ipv4([10, 0, 2, 0]),   // network address
    ] {
        let nh = next_hop(bad, ME, 24, Some(GW));
        assert_eq!(nh, None, "{bad:?}");
    }
    // /16: .255 is a host; a /32-ish prefix still works.
    assert_eq!(
        next_hop(Ipv4([10, 0, 2, 255]), ME, 16, Some(GW)),
        Some(Ipv4([10, 0, 2, 255]))
    );
    assert_eq!(next_hop(GW, ME, 33, Some(GW)), None);
    assert_eq!(
        next_hop(GW, ME, 0, Some(GW)),
        Some(GW),
        "prefix 0 is all on-link"
    );
}

#[test]
fn rtt_rounding() {
    assert_eq!(rtt_ms(0), 1);
    assert_eq!(rtt_ms(1), 1);
    assert_eq!(rtt_ms(1000), 1);
    assert_eq!(rtt_ms(1001), 2);
    assert_eq!(rtt_ms(u64::MAX), u32::MAX);
}

#[test]
fn errors_display() {
    assert_eq!(format!("{}", PingError::Timeout), "timeout");
    assert_eq!(
        format!("{}", PingError::Unreachable(3)),
        "destination unreachable (code 3)"
    );
}
