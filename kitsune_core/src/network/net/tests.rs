use super::*;

const OUR_MAC: Mac = Mac([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
const OUR_IP: Ipv4 = Ipv4([10, 0, 2, 15]);
const PEER_MAC: Mac = Mac([0x52, 0x55, 0x0a, 0x00, 0x02, 0x02]);
const PEER_IP: Ipv4 = Ipv4([10, 0, 2, 2]);

/// Append one TLV option to `v`.
fn opt(v: &mut Vec<u8>, code: u8, val: &[u8]) {
    v.push(code);
    v.push(val.len() as u8);
    v.extend_from_slice(val);
}

/// Craft a BOOTREPLY (server -> client) DHCP packet for round-trip tests.
fn craft_reply(
    out: &mut [u8],
    our_mac: Mac,
    xid: u32,
    your_ip: Ipv4,
    server_id: Ipv4,
    msg_type: u8,
) -> usize {
    let mut opts = Vec::new();
    opt(&mut opts, 53, &[msg_type]);
    opt(&mut opts, 54, &server_id.0);
    opt(&mut opts, 1, &[255, 255, 255, 0]); // subnet
    opt(&mut opts, 3, &PEER_IP.0); // router
    opts.push(255);
    craft_raw(out, our_mac, xid, your_ip, server_id, &opts)
}

/// Craft a BOOTREPLY carrying exactly the (already encoded) `opts`.
fn craft_raw(
    out: &mut [u8],
    our_mac: Mac,
    xid: u32,
    your_ip: Ipv4,
    server_id: Ipv4,
    opts: &[u8],
) -> usize {
    write_eth(out, our_mac, PEER_MAC, ETHERTYPE_IPV4);
    let b = ETH_HDR + IPV4_HDR + UDP_HDR;
    let opt = b + BOOTP_FIXED + 4;
    out[b..opt].fill(0);
    out[b] = BOOTREPLY;
    out[b + 1] = 1;
    out[b + 2] = 6;
    out[b + 4..b + 8].copy_from_slice(&xid.to_be_bytes());
    out[b + 16..b + 20].copy_from_slice(&your_ip.0); // yiaddr
    out[b + 28..b + 34].copy_from_slice(&our_mac.0); // chaddr
    out[b + BOOTP_FIXED..opt].copy_from_slice(&DHCP_MAGIC.to_be_bytes());
    out[opt..opt + opts.len()].copy_from_slice(opts);
    let o = opt + opts.len();
    let udp_off = ETH_HDR + IPV4_HDR;
    let udp_len = o - udp_off;
    out[udp_off..udp_off + 2].copy_from_slice(&DHCP_SERVER_PORT.to_be_bytes());
    out[udp_off + 2..udp_off + 4].copy_from_slice(&DHCP_CLIENT_PORT.to_be_bytes());
    out[udp_off + 4..udp_off + 6].copy_from_slice(&(udp_len as u16).to_be_bytes());
    let ip_off = ETH_HDR;
    let h = &mut out[ip_off..ip_off + IPV4_HDR];
    h.fill(0);
    h[0] = 0x45;
    h[2..4].copy_from_slice(&((IPV4_HDR + udp_len) as u16).to_be_bytes());
    h[8] = 64;
    h[9] = IPPROTO_UDP;
    h[12..16].copy_from_slice(&server_id.0);
    h[16..20].copy_from_slice(&[255, 255, 255, 255]);
    o
}

#[test]
fn dhcp_discover_structure() {
    let mut buf = [0u8; 600];
    let n = dhcp_discover(&mut buf, OUR_MAC, 0xAABB_CCDD);
    assert!(n > ETH_HDR + IPV4_HDR + UDP_HDR + BOOTP_FIXED + 4);
    assert_eq!(&buf[0..6], &[0xff; 6]); // Ethernet broadcast
    assert_eq!(u16::from_be_bytes([buf[12], buf[13]]), ETHERTYPE_IPV4);
    assert_eq!(buf[ETH_HDR + 9], IPPROTO_UDP);
    let u = ETH_HDR + IPV4_HDR;
    assert_eq!(u16::from_be_bytes([buf[u], buf[u + 1]]), 68); // sport
    assert_eq!(u16::from_be_bytes([buf[u + 2], buf[u + 3]]), 67); // dport
    let b = u + UDP_HDR;
    assert_eq!(buf[b], BOOTREQUEST);
    let magic = u32::from_be_bytes([
        buf[b + BOOTP_FIXED],
        buf[b + BOOTP_FIXED + 1],
        buf[b + BOOTP_FIXED + 2],
        buf[b + BOOTP_FIXED + 3],
    ]);
    assert_eq!(magic, DHCP_MAGIC);
    let opt = b + BOOTP_FIXED + 4;
    assert_eq!(buf[opt], 53);
    assert_eq!(buf[opt + 2], DHCP_DISCOVER);
    // IPv4 header checksum must be valid (folds back to zero).
    assert_eq!(checksum(&buf[ETH_HDR..ETH_HDR + IPV4_HDR]), 0);
}

#[test]
fn parse_offer_roundtrip() {
    let mut buf = [0u8; 600];
    let your = Ipv4([10, 0, 2, 15]);
    let server = Ipv4([10, 0, 2, 2]);
    let n = craft_reply(&mut buf, OUR_MAC, 0x1234_5678, your, server, DHCP_OFFER);
    let r = parse_dhcp(&buf[..n], OUR_MAC).expect("offer should parse");
    assert_eq!(r.msg_type, DHCP_OFFER);
    assert_eq!(r.xid, 0x1234_5678);
    assert_eq!(r.your_ip, your);
    assert_eq!(r.server_id, Some(server));
    assert_eq!(r.subnet, Some(Ipv4([255, 255, 255, 0])));
    assert_eq!(r.router, Some(PEER_IP));
    assert_eq!(r.dns, DnsServers::NONE);
    assert_eq!(r.lease_secs, None);
}

#[test]
fn parse_dhcp_rejects_other_mac() {
    let mut buf = [0u8; 600];
    let n = craft_reply(
        &mut buf,
        OUR_MAC,
        1,
        Ipv4([1, 2, 3, 4]),
        Ipv4([1, 2, 3, 1]),
        DHCP_ACK,
    );
    assert!(parse_dhcp(&buf[..n], Mac([0, 0, 0, 0, 0, 1])).is_none());
}

#[test]
fn dhcp_request_carries_requested_ip_and_server() {
    let mut buf = [0u8; 600];
    let req_ip = Ipv4([10, 0, 2, 15]);
    let srv = Ipv4([10, 0, 2, 2]);
    let n = dhcp_request(&mut buf, OUR_MAC, 7, req_ip, srv);
    let opts = &buf[ETH_HDR + IPV4_HDR + UDP_HDR + BOOTP_FIXED + 4..n];
    let (mut f50, mut f54) = (None, None);
    let mut i = 0;
    while i < opts.len() {
        let c = opts[i];
        if c == 255 {
            break;
        }
        let l = opts[i + 1] as usize;
        let v = &opts[i + 2..i + 2 + l];
        if c == 50 {
            f50 = Some(Ipv4([v[0], v[1], v[2], v[3]]));
        }
        if c == 54 {
            f54 = Some(Ipv4([v[0], v[1], v[2], v[3]]));
        }
        i += 2 + l;
    }
    assert_eq!(f50, Some(req_ip));
    assert_eq!(f54, Some(srv));
}

#[test]
fn checksum_known_ipv4_header() {
    // Wikipedia IPv4 example; checksum field zeroed -> expect 0xb861.
    let hdr = [
        0x45u8, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8, 0x00,
        0x01, 0xc0, 0xa8, 0x00, 0xc7,
    ];
    assert_eq!(checksum(&hdr), 0xb861);
}

#[test]
fn checksum_over_header_with_csum_is_zero() {
    let mut hdr = [
        0x45u8, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8, 0x00,
        0x01, 0xc0, 0xa8, 0x00, 0xc7,
    ];
    let c = checksum(&hdr).to_be_bytes();
    hdr[10] = c[0];
    hdr[11] = c[1];
    assert_eq!(checksum(&hdr), 0);
}

#[test]
fn checksum_handles_odd_length() {
    // Must not panic and must fold the trailing byte.
    let _ = checksum(&[0x01, 0x02, 0x03]);
}

fn arp_request(target: Ipv4) -> [u8; ETH_HDR + ARP_LEN] {
    let mut f = [0u8; ETH_HDR + ARP_LEN];
    write_eth(&mut f, Mac([0xff; 6]), PEER_MAC, ETHERTYPE_ARP);
    let a = &mut f[ETH_HDR..];
    a[0..2].copy_from_slice(&1u16.to_be_bytes());
    a[2..4].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
    a[4] = 6;
    a[5] = 4;
    a[6..8].copy_from_slice(&ARP_REQUEST.to_be_bytes());
    a[8..14].copy_from_slice(&PEER_MAC.0);
    a[14..18].copy_from_slice(&PEER_IP.0);
    a[24..28].copy_from_slice(&target.0);
    f
}

#[test]
fn arp_request_for_us_gets_reply() {
    let frame = arp_request(OUR_IP);
    let mut out = [0u8; 64];
    let n = respond(&frame, OUR_MAC, OUR_IP, &mut out).unwrap();
    assert_eq!(n, ETH_HDR + ARP_LEN);
    // Reply addressed to the requester, from us.
    assert_eq!(&out[0..6], &PEER_MAC.0);
    assert_eq!(&out[6..12], &OUR_MAC.0);
    let a = &out[ETH_HDR..];
    assert_eq!(u16::from_be_bytes([a[6], a[7]]), ARP_REPLY);
    assert_eq!(&a[8..14], &OUR_MAC.0); // sender hw = us
    assert_eq!(&a[14..18], &OUR_IP.0); // sender proto = us
}

/// Regression: `respond` is documented to return `None` when it cannot
/// answer, but an ARP request with an output buffer smaller than the 42-byte
/// reply sliced `out[..]` out of range and panicked.
#[test]
fn arp_reply_with_small_out_buffer_returns_none() {
    let frame = arp_request(OUR_IP);
    for sz in 0..(ETH_HDR + ARP_LEN) {
        let mut out = vec![0u8; sz];
        assert_eq!(respond(&frame, OUR_MAC, OUR_IP, &mut out), None, "out={sz}");
    }
    let mut out = vec![0u8; ETH_HDR + ARP_LEN];
    assert_eq!(
        respond(&frame, OUR_MAC, OUR_IP, &mut out),
        Some(ETH_HDR + ARP_LEN)
    );
}

#[test]
fn arp_with_unexpected_hw_or_proto_sizes_ignored() {
    let mut out = [0u8; 64];
    for (off, val) in [(0usize, 6u8), (1, 2), (2, 0x86), (4, 8), (5, 16)] {
        let mut frame = arp_request(OUR_IP);
        frame[ETH_HDR + off] = val;
        assert_eq!(
            respond(&frame, OUR_MAC, OUR_IP, &mut out),
            None,
            "off={off}"
        );
    }
}

#[test]
fn arp_request_for_other_ip_ignored() {
    let frame = arp_request(Ipv4([10, 0, 2, 99]));
    let mut out = [0u8; 64];
    assert_eq!(respond(&frame, OUR_MAC, OUR_IP, &mut out), None);
}

fn icmp_echo(dst: Ipv4, payload: &[u8]) -> [u8; 128] {
    let mut f = [0u8; 128];
    write_eth(&mut f, OUR_MAC, PEER_MAC, ETHERTYPE_IPV4);
    let ip = &mut f[ETH_HDR..ETH_HDR + IPV4_HDR];
    ip[0] = 0x45;
    let total = (IPV4_HDR + ICMP_HDR + payload.len()) as u16;
    ip[2..4].copy_from_slice(&total.to_be_bytes());
    ip[8] = 64;
    ip[9] = IPPROTO_ICMP;
    ip[12..16].copy_from_slice(&PEER_IP.0);
    ip[16..20].copy_from_slice(&dst.0);
    let icmp = &mut f[ETH_HDR + IPV4_HDR..];
    icmp[0] = ICMP_ECHO_REQUEST;
    icmp[4..6].copy_from_slice(&0x1234u16.to_be_bytes()); // id
    icmp[6..8].copy_from_slice(&0x0001u16.to_be_bytes()); // seq
    icmp[8..8 + payload.len()].copy_from_slice(payload);
    f
}

#[test]
fn icmp_echo_request_gets_valid_reply() {
    let frame = icmp_echo(OUR_IP, b"ping-data");
    let mut out = [0u8; 128];
    let n = respond(&frame, OUR_MAC, OUR_IP, &mut out).unwrap();
    assert_eq!(n, ETH_HDR + IPV4_HDR + ICMP_HDR + 9);

    // Ethernet back to the peer, from us.
    assert_eq!(&out[0..6], &PEER_MAC.0);
    assert_eq!(&out[6..12], &OUR_MAC.0);

    // IPv4 header checksum must validate, addresses swapped.
    let ip = &out[ETH_HDR..ETH_HDR + IPV4_HDR];
    assert_eq!(checksum(ip), 0);
    assert_eq!(&ip[12..16], &OUR_IP.0);
    assert_eq!(&ip[16..20], &PEER_IP.0);

    // ICMP is an echo reply, checksum validates, payload echoed.
    let icmp = &out[ETH_HDR + IPV4_HDR..n];
    assert_eq!(icmp[0], ICMP_ECHO_REPLY);
    assert_eq!(checksum(icmp), 0);
    assert_eq!(&icmp[8..], b"ping-data");
}

#[test]
fn icmp_to_other_ip_ignored() {
    let frame = icmp_echo(Ipv4([10, 0, 2, 99]), b"x");
    let mut out = [0u8; 128];
    assert_eq!(respond(&frame, OUR_MAC, OUR_IP, &mut out), None);
}

#[test]
fn arp_announce_is_broadcast_from_us() {
    let mut out = [0u8; 64];
    let n = arp_announce(&mut out, OUR_MAC, OUR_IP);
    assert_eq!(n, ETH_HDR + ARP_LEN);
    assert_eq!(&out[0..6], &[0xff; 6]); // broadcast
    assert_eq!(&out[6..12], &OUR_MAC.0);
    let a = &out[ETH_HDR..];
    assert_eq!(u16::from_be_bytes([a[6], a[7]]), ARP_REQUEST);
    assert_eq!(&a[8..14], &OUR_MAC.0); // sender hw
    assert_eq!(&a[14..18], &OUR_IP.0); // sender proto
    assert_eq!(&a[24..28], &OUR_IP.0); // target proto = us (gratuitous)
}

#[test]
fn non_ip_non_arp_ignored() {
    let mut f = [0u8; 64];
    write_eth(&mut f, OUR_MAC, PEER_MAC, 0x88cc); // LLDP
    let mut out = [0u8; 64];
    assert_eq!(respond(&f, OUR_MAC, OUR_IP, &mut out), None);
}

#[test]
fn short_frame_ignored() {
    let mut out = [0u8; 64];
    assert_eq!(respond(&[0u8; 8], OUR_MAC, OUR_IP, &mut out), None);
}

/// Regression: the NE2000 driver computed `curr - 1` for the boundary
/// register; a hardware pointer of 0 underflowed (panic in debug, 255 in
/// release). The helper wraps for every input.
#[test]
fn ring_prev_page_wraps_and_never_underflows() {
    const START: u8 = 0x46;
    const STOP: u8 = 0x80;
    assert_eq!(ring_prev_page(0x47, START, STOP), 0x46);
    assert_eq!(ring_prev_page(0x7F, START, STOP), 0x7E);
    assert_eq!(ring_prev_page(START, START, STOP), STOP - 1);
    for p in 0..=u8::MAX {
        let prev = ring_prev_page(p, START, STOP);
        assert!((START..STOP).contains(&prev), "page {p} -> {prev}");
    }
    assert_eq!(ring_prev_page(0, START, STOP), STOP - 1);
    assert_eq!(ring_prev_page(0xFF, START, STOP), STOP - 1);
    assert_eq!(ring_prev_page(STOP, START, STOP), STOP - 1);
}

/// Regression: `ne2000::send` had no ceiling on the frame length, so an
/// oversized frame overran the 6-page transmit buffer into the RX ring.
#[test]
fn tx_len_pads_and_clamps() {
    assert_eq!(tx_len(0), MIN_TX_FRAME);
    assert_eq!(tx_len(42), 60);
    assert_eq!(tx_len(60), 60);
    assert_eq!(tx_len(1000), 1000);
    assert_eq!(tx_len(1514), 1514);
    assert_eq!(tx_len(1515), 1514);
    assert_eq!(tx_len(usize::MAX), 1514);
    const { assert!(MAX_TX_FRAME <= 6 * 256) };
}

// ---- NetConfig ----

const LEASE_IP: Ipv4 = Ipv4([192, 168, 77, 15]);
const LEASE_GW: Ipv4 = Ipv4([192, 168, 77, 2]);
const LEASE_DNS: Ipv4 = Ipv4([192, 168, 77, 3]);

/// Parse an ACK for `LEASE_IP` made of `opts` (+ end marker) from `LEASE_GW`.
fn ack_with(opts: &[(u8, &[u8])]) -> DhcpReply {
    let mut v = Vec::new();
    for (c, val) in opts {
        opt(&mut v, *c, val);
    }
    v.push(255);
    let mut buf = [0u8; 600];
    let n = craft_raw(&mut buf, OUR_MAC, 9, LEASE_IP, LEASE_GW, &v);
    parse_dhcp(&buf[..n], OUR_MAC).expect("ack should parse")
}

const T_ACK: (u8, &[u8]) = (53, &[DHCP_ACK]);
const SERVER: (u8, &[u8]) = (54, &LEASE_GW.0);
const MASK24: (u8, &[u8]) = (1, &[255, 255, 255, 0]);
const ROUTER: (u8, &[u8]) = (3, &LEASE_GW.0);
const DNS: (u8, &[u8]) = (6, &LEASE_DNS.0);

#[test]
fn full_ack_becomes_config() {
    let r = ack_with(&[
        T_ACK,
        SERVER,
        MASK24,
        ROUTER,
        DNS,
        (51, &86_400u32.to_be_bytes()),
    ]);
    assert_eq!(
        NetConfig::from_ack(&r),
        Ok(NetConfig {
            ip: LEASE_IP,
            prefix: 24,
            gateway: Some(LEASE_GW),
            dns: DnsServers::one(LEASE_DNS),
            lease_secs: Some(86_400),
        })
    );
}

#[test]
fn non_24_prefixes_are_derived_from_the_mask() {
    for (mask, want) in [
        ([255u8, 0, 0, 0], 8u8),
        ([255, 255, 0, 0], 16),
        ([255, 255, 255, 128], 25),
        ([255, 255, 255, 224], 27),
    ] {
        let r = ack_with(&[T_ACK, SERVER, (1, &mask)]);
        assert_eq!(NetConfig::from_ack(&r).unwrap().prefix, want, "{mask:?}");
    }
}

#[test]
fn missing_router_falls_back_to_server_then_nothing() {
    // No router option: the DHCP server is the gateway; DNS follows it.
    let r = ack_with(&[T_ACK, SERVER, MASK24]);
    let c = NetConfig::from_ack(&r).unwrap();
    assert_eq!(c.gateway, Some(LEASE_GW));
    assert_eq!(c.dns, DnsServers::one(LEASE_GW));
    // Neither router nor server id: no default route, no resolver.
    let r = ack_with(&[T_ACK, MASK24]);
    let c = NetConfig::from_ack(&r).unwrap();
    assert_eq!((c.gateway, c.dns), (None, DnsServers::NONE));
    // Router but no DNS: DNS = router.
    let r = ack_with(&[T_ACK, (3, &[192, 168, 77, 1])]);
    let c = NetConfig::from_ack(&r).unwrap();
    assert_eq!(c.dns, DnsServers::one(Ipv4([192, 168, 77, 1])));
    // An explicit DNS wins over the router.
    let r = ack_with(&[T_ACK, ROUTER, (6, &[8, 8, 8, 8])]);
    assert_eq!(
        NetConfig::from_ack(&r).unwrap().dns,
        DnsServers::one(Ipv4([8, 8, 8, 8]))
    );
}

#[test]
fn missing_mask_defaults_to_24() {
    let r = ack_with(&[T_ACK, SERVER]);
    assert_eq!(NetConfig::from_ack(&r).unwrap().prefix, DEFAULT_PREFIX);
}

#[test]
fn invalid_masks_are_rejected() {
    for mask in [
        [255u8, 0, 255, 0], // not contiguous
        [255, 255, 0, 255],
        [0, 255, 255, 255],
        [0, 0, 0, 0],         // prefix 0
        [255, 255, 255, 254], // /31 and /32 leave no room for a gateway
        [255, 255, 255, 255],
        [1, 0, 0, 0],
    ] {
        let r = ack_with(&[T_ACK, SERVER, (1, &mask)]);
        assert_eq!(
            NetConfig::from_ack(&r),
            Err(ConfigError::BadMask),
            "{mask:?}"
        );
    }
}

#[test]
fn mask_to_prefix_covers_every_contiguous_mask() {
    for p in 0..=32u32 {
        let m = if p == 0 { 0 } else { u32::MAX << (32 - p) };
        assert_eq!(mask_to_prefix(Ipv4(m.to_be_bytes())), Some(p as u8));
        if (1..=31).contains(&p) {
            // Flip the lowest set bit one position down -> a hole.
            let holey = (m | (1 << (32 - p - 1))) & !(1 << (32 - p));
            if holey != m && p > 1 {
                assert_eq!(mask_to_prefix(Ipv4(holey.to_be_bytes())), None, "p={p}");
            }
        }
    }
}

#[test]
fn lease_zero_infinite_and_absent() {
    let zero = ack_with(&[T_ACK, SERVER, (51, &0u32.to_be_bytes())]);
    assert_eq!(NetConfig::from_ack(&zero), Err(ConfigError::ZeroLease));
    let inf = ack_with(&[T_ACK, SERVER, (51, &u32::MAX.to_be_bytes())]);
    assert_eq!(NetConfig::from_ack(&inf).unwrap().lease_secs, None);
    let absent = ack_with(&[T_ACK, SERVER]);
    assert_eq!(NetConfig::from_ack(&absent).unwrap().lease_secs, None);
    let one = ack_with(&[T_ACK, SERVER, (51, &1u32.to_be_bytes())]);
    assert_eq!(NetConfig::from_ack(&one).unwrap().lease_secs, Some(1));
    // A malformed (3-byte) lease option is ignored, not misread.
    let short = ack_with(&[T_ACK, SERVER, (51, &[0, 0, 5])]);
    assert_eq!(NetConfig::from_ack(&short).unwrap().lease_secs, None);
}

#[test]
fn lease_expiry_boundary() {
    let c = NetConfig {
        lease_secs: Some(60),
        ..NetConfig::STATIC_FALLBACK
    };
    assert!(!c.lease_expired(0));
    assert!(!c.lease_expired(59));
    assert!(c.lease_expired(60));
    assert!(c.lease_expired(u64::MAX));
    assert!(!NetConfig::STATIC_FALLBACK.lease_expired(u64::MAX));
}

#[test]
fn offer_without_server_id_is_visible_to_the_caller() {
    let r = ack_with(&[(53, &[DHCP_OFFER]), MASK24, ROUTER]);
    assert_eq!(r.msg_type, DHCP_OFFER);
    assert_eq!(r.server_id, None); // dhcp_acquire cannot REQUEST this
    // And an OFFER is never a config.
    assert_eq!(NetConfig::from_ack(&r), Err(ConfigError::NotAck));
    let nak = ack_with(&[(53, &[DHCP_NAK]), SERVER]);
    assert_eq!(NetConfig::from_ack(&nak), Err(ConfigError::NotAck));
}

#[test]
fn malformed_address_options_are_ignored() {
    // Wrong lengths: 3, 5, 6, 0 bytes -> treated as absent.
    for bad in [&[1u8, 2, 3][..], &[1, 2, 3, 4, 5], &[1, 2, 3, 4, 5, 6], &[]] {
        let r = ack_with(&[T_ACK, (54, bad), (3, bad), (6, bad)]);
        assert_eq!(
            (r.server_id, r.router, r.dns),
            (None, None, DnsServers::NONE),
            "{bad:?}"
        );
    }
    // Unusable addresses: 0.0.0.0, loopback, multicast, broadcast.
    for bad in [[0u8, 0, 0, 0], [127, 0, 0, 1], [224, 0, 0, 1], [255; 4]] {
        let r = ack_with(&[T_ACK, (54, &bad), (3, &bad), (6, &bad)]);
        assert_eq!(
            (r.server_id, r.router, r.dns),
            (None, None, DnsServers::NONE),
            "{bad:?}"
        );
    }
    // A list: the first *usable* entry wins (0.0.0.0 is skipped).
    let r = ack_with(&[T_ACK, (3, &[0, 0, 0, 0, 192, 168, 77, 9, 192, 168, 77, 8])]);
    assert_eq!(r.router, Some(Ipv4([192, 168, 77, 9])));
}

#[test]
fn unusable_addresses_are_rejected() {
    for bad in [[0u8, 0, 0, 0], [127, 0, 0, 1], [224, 0, 0, 1], [255; 4]] {
        let mut r = ack_with(&[T_ACK, SERVER, MASK24]);
        r.your_ip = Ipv4(bad);
        assert_eq!(
            NetConfig::from_ack(&r),
            Err(ConfigError::BadAddress),
            "{bad:?}"
        );
    }
    // Network and broadcast address of the subnet.
    for bad in [[192u8, 168, 77, 0], [192, 168, 77, 255]] {
        let mut r = ack_with(&[T_ACK, SERVER, MASK24]);
        r.your_ip = Ipv4(bad);
        assert_eq!(
            NetConfig::from_ack(&r),
            Err(ConfigError::BadAddress),
            "{bad:?}"
        );
    }
    // .255 is fine in a /16.
    let mut r = ack_with(&[T_ACK, SERVER, (1, &[255, 255, 0, 0])]);
    r.your_ip = Ipv4([192, 168, 77, 255]);
    assert!(NetConfig::from_ack(&r).is_ok());
}

#[test]
fn arp_request_and_reply_roundtrip() {
    let mut buf = [0u8; 64];
    let n = arp_who_has(&mut buf, OUR_MAC, OUR_IP, PEER_IP);
    assert_eq!(n, 42);
    assert_eq!(&buf[0..6], &[0xff; 6]);
    assert_eq!(&buf[12..14], &ETHERTYPE_ARP.to_be_bytes());
    // The responder of the *peer* would answer with a reply: build one with
    // the existing builder and parse it back.
    let mut reply = [0u8; 64];
    let n = build_arp_reply(&mut reply, PEER_MAC, PEER_IP, OUR_MAC, OUR_IP).unwrap();
    assert_eq!(
        parse_arp_reply(&reply[..n], OUR_IP),
        Some((PEER_IP, PEER_MAC))
    );
    // Not for us, or not a reply, or truncated.
    assert_eq!(parse_arp_reply(&reply[..n], Ipv4([10, 0, 2, 99])), None);
    assert_eq!(parse_arp_reply(&buf[..42], OUR_IP), None);
    assert_eq!(parse_arp_reply(&reply[..30], OUR_IP), None);
    // A reply claiming 0.0.0.0 is rejected.
    let mut bad = reply;
    bad[14 + 14..14 + 18].copy_from_slice(&[0; 4]);
    assert_eq!(parse_arp_reply(&bad[..n], OUR_IP), None);
}

#[test]
fn option_6_keeps_every_usable_server_in_order() {
    let r = ack_with(&[
        T_ACK,
        (
            6,
            &[
                10, 0, 2, 3, 0, 0, 0, 0, 8, 8, 8, 8, 10, 0, 2, 3, 1, 1, 1, 1, 9, 9, 9, 9,
            ],
        ),
    ]);
    // 0.0.0.0 skipped, the duplicate dropped, capped at MAX_DNS.
    assert_eq!(
        r.dns.as_slice(),
        &[Ipv4([10, 0, 2, 3]), Ipv4([8, 8, 8, 8]), Ipv4([1, 1, 1, 1])]
    );
    // A list whose length is not a multiple of 4 is malformed as a whole.
    let r = ack_with(&[T_ACK, (6, &[10, 0, 2, 3, 8, 8])]);
    assert!(r.dns.is_empty());
}

#[test]
fn config_keeps_all_dns_servers_and_drops_our_own_address() {
    let r = ack_with(&[
        T_ACK,
        SERVER,
        (6, &[192, 168, 77, 15, 192, 168, 77, 3, 8, 8, 8, 8]),
    ]);
    let c = NetConfig::from_ack(&r).unwrap();
    assert_eq!(
        c.dns.as_slice(),
        &[Ipv4([192, 168, 77, 3]), Ipv4([8, 8, 8, 8])]
    );
    assert_eq!(
        format!("{c}"),
        "192.168.77.15/24 gw 192.168.77.2 dns 192.168.77.3,8.8.8.8"
    );
    // Only our own address as DNS: falls back to the gateway.
    let r = ack_with(&[T_ACK, SERVER, ROUTER, (6, &LEASE_IP.0)]);
    assert_eq!(
        NetConfig::from_ack(&r).unwrap().dns,
        DnsServers::one(LEASE_GW)
    );
}

#[test]
fn dns_servers_set_semantics() {
    let mut d = DnsServers::NONE;
    assert!(d.is_empty() && d.first().is_none());
    assert!(d.push(Ipv4([1, 1, 1, 1])));
    assert!(!d.push(Ipv4([1, 1, 1, 1])), "duplicate");
    assert!(!d.push(Ipv4([0, 0, 0, 0])), "unusable");
    assert!(!d.push(Ipv4([127, 0, 0, 1])), "loopback");
    assert!(d.push(Ipv4([2, 2, 2, 2])));
    assert!(d.push(Ipv4([3, 3, 3, 3])));
    assert!(!d.push(Ipv4([4, 4, 4, 4])), "full");
    assert_eq!(d.len(), MAX_DNS);
    assert_eq!(d.first(), Some(Ipv4([1, 1, 1, 1])));
    assert_eq!(format!("{d}"), "1.1.1.1,2.2.2.2,3.3.3.3");
    assert_eq!(format!("{}", DnsServers::NONE), "none");
}

/// Decode the pieces of a built client message that the tests care about.
struct Built {
    dst_mac: [u8; 6],
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    ciaddr: [u8; 4],
    flags: u16,
    opts: Vec<(u8, Vec<u8>)>,
}

fn decode(buf: &[u8]) -> Built {
    assert_eq!(checksum(&buf[ETH_HDR..ETH_HDR + IPV4_HDR]), 0);
    let b = ETH_HDR + IPV4_HDR + UDP_HDR;
    let mut opts = Vec::new();
    let mut i = b + BOOTP_FIXED + 4;
    while buf[i] != 255 {
        let len = buf[i + 1] as usize;
        opts.push((buf[i], buf[i + 2..i + 2 + len].to_vec()));
        i += 2 + len;
    }
    Built {
        dst_mac: buf[0..6].try_into().unwrap(),
        src_ip: buf[ETH_HDR + 12..ETH_HDR + 16].try_into().unwrap(),
        dst_ip: buf[ETH_HDR + 16..ETH_HDR + 20].try_into().unwrap(),
        ciaddr: buf[b + 12..b + 16].try_into().unwrap(),
        flags: u16::from_be_bytes([buf[b + 10], buf[b + 11]]),
        opts,
    }
}

fn has(b: &Built, code: u8) -> bool {
    b.opts.iter().any(|(c, _)| *c == code)
}

#[test]
fn renew_is_unicast_from_our_address_with_ciaddr_and_no_50_54() {
    let mut buf = [0u8; 600];
    let n = dhcp_request_renew(&mut buf, OUR_MAC, 0x77, OUR_IP, Some((PEER_MAC, PEER_IP)));
    assert!(n <= buf.len());
    let b = decode(&buf[..n]);
    assert_eq!(b.dst_mac, PEER_MAC.0);
    assert_eq!((b.src_ip, b.dst_ip), (OUR_IP.0, PEER_IP.0));
    assert_eq!(b.ciaddr, OUR_IP.0);
    assert_eq!(b.flags, 0, "no broadcast flag when we hold an address");
    assert_eq!(b.opts[0], (53, vec![DHCP_REQUEST]));
    assert!(!has(&b, 50) && !has(&b, 54));
}

#[test]
fn rebind_is_broadcast_from_our_address_with_ciaddr() {
    let mut buf = [0u8; 600];
    let n = dhcp_request_renew(&mut buf, OUR_MAC, 0x78, OUR_IP, None);
    let b = decode(&buf[..n]);
    assert_eq!(b.dst_mac, [0xff; 6]);
    assert_eq!((b.src_ip, b.dst_ip), (OUR_IP.0, [255; 4]));
    assert_eq!(b.ciaddr, OUR_IP.0);
    assert!(!has(&b, 50) && !has(&b, 54));
}

#[test]
fn selecting_messages_stay_broadcast_from_zero() {
    let mut buf = [0u8; 600];
    let n = dhcp_discover(&mut buf, OUR_MAC, 5);
    let b = decode(&buf[..n]);
    assert_eq!((b.src_ip, b.dst_ip, b.ciaddr), ([0; 4], [255; 4], [0; 4]));
    assert_eq!(b.flags, 0x8000);
    let n = dhcp_request(&mut buf, OUR_MAC, 5, OUR_IP, PEER_IP);
    let b = decode(&buf[..n]);
    assert_eq!((b.src_ip, b.ciaddr), ([0; 4], [0; 4]));
    assert!(has(&b, 50) && has(&b, 54));
}

#[test]
fn release_is_unicast_with_server_id_and_ciaddr() {
    let mut buf = [0u8; 600];
    let n = dhcp_release(&mut buf, OUR_MAC, 9, OUR_IP, PEER_MAC, PEER_IP);
    let b = decode(&buf[..n]);
    assert_eq!(b.dst_mac, PEER_MAC.0);
    assert_eq!((b.src_ip, b.dst_ip), (OUR_IP.0, PEER_IP.0));
    assert_eq!(b.ciaddr, OUR_IP.0);
    assert_eq!(b.opts[0], (53, vec![DHCP_RELEASE]));
    assert!(b.opts.contains(&(54, PEER_IP.0.to_vec())));
}

#[test]
fn builders_fit_in_the_documented_minimum_buffer() {
    let mut buf = [0u8; DHCP_BUILD_MIN];
    let n = dhcp_discover(&mut buf, OUR_MAC, 1);
    assert!(n <= DHCP_BUILD_MIN);
    let n = dhcp_request(&mut buf, OUR_MAC, 1, OUR_IP, PEER_IP);
    assert!(n <= DHCP_BUILD_MIN);
    let n = dhcp_request_renew(&mut buf, OUR_MAC, 1, OUR_IP, None);
    assert!(n <= DHCP_BUILD_MIN);
    let n = dhcp_release(&mut buf, OUR_MAC, 1, OUR_IP, PEER_MAC, PEER_IP);
    assert!(n <= DHCP_BUILD_MIN);
}

#[test]
fn reply_records_the_ethernet_source() {
    let mut buf = [0u8; 600];
    let n = craft_reply(&mut buf, OUR_MAC, 3, OUR_IP, PEER_IP, DHCP_ACK);
    let src = Mac([1, 2, 3, 4, 5, 6]);
    buf[6..12].copy_from_slice(&src.0);
    assert_eq!(parse_dhcp(&buf[..n], OUR_MAC).unwrap().eth_src, src);
}

#[test]
fn gateway_or_dns_equal_to_our_ip_is_dropped() {
    let r = ack_with(&[T_ACK, (3, &LEASE_IP.0), (6, &LEASE_IP.0)]);
    let c = NetConfig::from_ack(&r).unwrap();
    assert_eq!((c.gateway, c.dns), (None, DnsServers::NONE));
}

#[test]
fn static_fallback_is_the_slirp_default() {
    let f = NetConfig::STATIC_FALLBACK;
    assert_eq!(f.ip, Ipv4([10, 0, 2, 15]));
    assert_eq!(f.prefix, 24);
    assert_eq!(f.gateway, Some(Ipv4([10, 0, 2, 2])));
    assert_eq!(f.dns, DnsServers::one(Ipv4([10, 0, 2, 3])));
    assert_eq!(f.lease_secs, None);
}

#[test]
fn config_display_is_the_boot_log_form() {
    assert_eq!(
        format!("{}", NetConfig::STATIC_FALLBACK),
        "10.0.2.15/24 gw 10.0.2.2 dns 10.0.2.3"
    );
    let c = NetConfig {
        gateway: None,
        dns: DnsServers::NONE,
        ..NetConfig::STATIC_FALLBACK
    };
    assert_eq!(format!("{c}"), "10.0.2.15/24 gw none dns none");
}

/// The responder answers for the address the lease handed out, and only
/// that one: neither the SLIRP default nor anything else.
#[test]
fn responder_answers_for_the_leased_ip_only() {
    let cfg = NetConfig::from_ack(&ack_with(&[T_ACK, SERVER, MASK24])).unwrap();
    assert_eq!(cfg.ip, LEASE_IP);
    let mut out = [0u8; 128];

    // ARP who-has <lease ip> -> reply carrying the lease ip.
    let n = respond(&arp_request(cfg.ip), OUR_MAC, cfg.ip, &mut out).unwrap();
    assert_eq!(n, ETH_HDR + ARP_LEN);
    assert_eq!(&out[ETH_HDR + 14..ETH_HDR + 18], &LEASE_IP.0); // sender proto
    // ARP who-has the old fixed address -> silence.
    assert_eq!(
        respond(&arp_request(OUR_IP), OUR_MAC, cfg.ip, &mut out),
        None
    );

    // Ping to the lease ip -> echo reply from the lease ip; ping to the
    // fixed address -> silence.
    let n = respond(&icmp_echo(cfg.ip, b"hi"), OUR_MAC, cfg.ip, &mut out).unwrap();
    let ip = &out[ETH_HDR..ETH_HDR + IPV4_HDR];
    assert_eq!(checksum(ip), 0);
    assert_eq!(&ip[12..16], &LEASE_IP.0);
    assert_eq!(out[ETH_HDR + IPV4_HDR], ICMP_ECHO_REPLY);
    assert_eq!(n, ETH_HDR + IPV4_HDR + ICMP_HDR + 2);
    assert_eq!(
        respond(&icmp_echo(OUR_IP, b"hi"), OUR_MAC, cfg.ip, &mut out),
        None
    );
}

#[test]
fn parse_ipv4_accepts_dotted_quads() {
    assert_eq!(parse_ipv4(b"192.168.77.2"), Some(Ipv4([192, 168, 77, 2])));
    assert_eq!(parse_ipv4(b"0.0.0.0"), Some(Ipv4([0; 4])));
    assert_eq!(parse_ipv4(b"255.255.255.255"), Some(Ipv4([255; 4])));
    assert_eq!(parse_ipv4(b"10.0.2.2"), Some(Ipv4([10, 0, 2, 2])));
}

#[test]
fn parse_ipv4_rejects_everything_else() {
    for bad in [
        &b""[..],
        b"1.2.3",
        b"1.2.3.4.5",
        b"1.2.3.",
        b".1.2.3",
        b"1..2.3",
        b"256.1.1.1",
        b"1.1.1.999",
        b"1.1.1.1000",
        b"01.2.3.4",
        b"1.2.3.04",
        b"+1.2.3.4",
        b"-1.2.3.4",
        b"1.2.3.4 ",
        b" 1.2.3.4",
        b"0x7f.0.0.1",
        b"example.com",
        b"1.2.3.a",
        b"1.2.3.4:80",
    ] {
        assert_eq!(parse_ipv4(bad), None, "{:?}", core::str::from_utf8(bad));
    }
}

#[test]
fn ipv4_display_is_dotted_quad() {
    assert_eq!(format!("{}", Ipv4([192, 168, 77, 15])), "192.168.77.15");
    assert_eq!(format!("{}", Ipv4([0, 0, 0, 0])), "0.0.0.0");
}

/// Totality: every truncation of a full ACK frame, and every single-byte
/// corruption of its options area, goes through `parse_dhcp` and
/// `from_ack` without panicking.
#[test]
fn parse_and_config_are_total_over_truncation_and_corruption() {
    let mut buf = [0u8; 600];
    let mut v = Vec::new();
    for (c, val) in [
        T_ACK,
        SERVER,
        MASK24,
        ROUTER,
        DNS,
        (51, &3600u32.to_be_bytes()[..]),
    ] {
        opt(&mut v, c, val);
    }
    v.push(255);
    let n = craft_raw(&mut buf, OUR_MAC, 1, LEASE_IP, LEASE_GW, &v);
    let run = |f: &[u8]| {
        if let Some(r) = parse_dhcp(f, OUR_MAC) {
            let _ = NetConfig::from_ack(&r);
        }
    };
    for len in 0..=n {
        run(&buf[..len]);
    }
    let opts_at = ETH_HDR + IPV4_HDR + UDP_HDR + BOOTP_FIXED + 4;
    for i in opts_at..n {
        for val in [0u8, 1, 3, 4, 5, 255] {
            let mut f = buf;
            f[i] = val;
            run(&f[..n]);
        }
    }
}
