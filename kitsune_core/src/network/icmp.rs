//! ICMP echo client: build echo requests, classify what comes back, and decide
//! whether a ping succeeded, failed or timed out. Pure and clock-injected (the
//! clock is microseconds, so a round trip on a virtual LAN does not read as 0).
//!
//! The kernel's `netd` owns the NIC: it resolves the next hop with ARP
//! ([`crate::network::net::arp_who_has`], [`crate::network::net::parse_arp_reply`]), sends the frame
//! built here and feeds every received frame to [`parse_event`] and then to
//! [`Ping::on_event`]. The responder half (answering other hosts' echo requests)
//! is [`crate::network::net::respond`].
//!
//! Replies are matched on the identifier, the sequence number and the sender; an
//! ICMP error (destination unreachable, time exceeded) counts only when the
//! datagram it quotes is our echo request, so a stray or forged error about some
//! other flow cannot end a ping.

use crate::network::net::{
    ETHERTYPE_IPV4, ICMP_ECHO_REPLY, ICMP_ECHO_REQUEST, IPPROTO_ICMP, Ipv4, Mac, checksum,
    is_usable_unicast,
};

pub const ICMP_DEST_UNREACHABLE: u8 = 3;
pub const ICMP_TIME_EXCEEDED: u8 = 11;

const ETH_HDR: usize = 14;
const IPV4_HDR: usize = 20;
const ICMP_HDR: usize = 8;

/// Largest echo payload [`build_echo_request`] accepts (frame must fit the MTU).
pub const MAX_PAYLOAD: usize = 1500 - IPV4_HDR - ICMP_HDR;
/// Smallest buffer for a request with `payload` bytes.
pub const fn frame_len(payload: usize) -> usize {
    ETH_HDR + IPV4_HDR + ICMP_HDR + payload
}

/// Everything an echo request carries.
#[derive(Clone, Copy, Debug)]
pub struct Echo {
    pub src_mac: Mac,
    pub dst_mac: Mac,
    pub src_ip: Ipv4,
    pub dst_ip: Ipv4,
    pub id: u16,
    pub seq: u16,
    /// Bytes of the classic `0x20 + i` payload pattern.
    pub payload: usize,
}

/// Build the Ethernet/IPv4/ICMP echo request described by `e` into `out`.
/// Returns the frame length, or `None` if the payload is over [`MAX_PAYLOAD`] or
/// `out` is too small.
pub fn build_echo_request(out: &mut [u8], e: &Echo) -> Option<usize> {
    let Echo {
        src_mac,
        dst_mac,
        src_ip,
        dst_ip,
        id,
        seq,
        payload,
    } = *e;
    let total = frame_len(payload);
    if payload > MAX_PAYLOAD || out.len() < total {
        return None;
    }
    out[0..6].copy_from_slice(&dst_mac.0);
    out[6..12].copy_from_slice(&src_mac.0);
    out[12..14].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
    {
        let h = &mut out[ETH_HDR..ETH_HDR + IPV4_HDR];
        h.fill(0);
        h[0] = 0x45;
        h[2..4].copy_from_slice(&((IPV4_HDR + ICMP_HDR + payload) as u16).to_be_bytes());
        h[4..6].copy_from_slice(&seq.to_be_bytes()); // identification
        h[6] = 0x40; // don't fragment
        h[8] = 64;
        h[9] = IPPROTO_ICMP;
        h[12..16].copy_from_slice(&src_ip.0);
        h[16..20].copy_from_slice(&dst_ip.0);
    }
    let c = checksum(&out[ETH_HDR..ETH_HDR + IPV4_HDR]);
    out[ETH_HDR + 10..ETH_HDR + 12].copy_from_slice(&c.to_be_bytes());

    let i = ETH_HDR + IPV4_HDR;
    out[i] = ICMP_ECHO_REQUEST;
    out[i + 1] = 0;
    out[i + 2..i + 4].fill(0);
    out[i + 4..i + 6].copy_from_slice(&id.to_be_bytes());
    out[i + 6..i + 8].copy_from_slice(&seq.to_be_bytes());
    for (k, b) in out[i + ICMP_HDR..total].iter_mut().enumerate() {
        *b = 0x20u8.wrapping_add(k as u8);
    }
    let c = checksum(&out[i..total]);
    out[i + 2..i + 4].copy_from_slice(&c.to_be_bytes());
    Some(total)
}

/// What a received frame means for a ping in flight.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Event {
    /// IP source of the ICMP message.
    pub from: Ipv4,
    pub kind: Kind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    EchoReply {
        id: u16,
        seq: u16,
    },
    /// Destination unreachable (`code`: 0 net, 1 host, 3 port, 4 frag needed ...)
    /// about our echo request to `orig_dst`.
    Unreachable {
        code: u8,
        id: u16,
        seq: u16,
        orig_dst: Ipv4,
    },
    TimeExceeded {
        id: u16,
        seq: u16,
        orig_dst: Ipv4,
    },
}

/// Classify `frame` if it is an ICMP echo reply, or an ICMP error quoting an echo
/// request, addressed to `our_ip`. Header and message checksums must be valid.
/// Everything else (ARP, TCP, other ICMP, damaged frames) is `None`.
pub fn parse_event(frame: &[u8], our_ip: Ipv4) -> Option<Event> {
    if frame.len() < ETH_HDR + IPV4_HDR || frame[12..14] != ETHERTYPE_IPV4.to_be_bytes() {
        return None;
    }
    let ip = &frame[ETH_HDR..];
    let ihl = usize::from(ip[0] & 0x0F) * 4;
    if ip[0] >> 4 != 4 || ihl < IPV4_HDR || ip.len() < ihl {
        return None;
    }
    let total = usize::from(u16::from_be_bytes([ip[2], ip[3]])).min(ip.len());
    if total < ihl + ICMP_HDR || ip[9] != IPPROTO_ICMP {
        return None;
    }
    // A fragment is not a whole message.
    if u16::from_be_bytes([ip[6], ip[7]]) & 0x3FFF != 0 {
        return None;
    }
    if checksum(&ip[..ihl]) != 0 {
        return None;
    }
    let from = Ipv4([ip[12], ip[13], ip[14], ip[15]]);
    if ip[16..20] != our_ip.0 {
        return None;
    }
    let icmp = &ip[ihl..total];
    if checksum(icmp) != 0 {
        return None;
    }
    let kind = match (icmp[0], icmp[1]) {
        (ICMP_ECHO_REPLY, 0) => Kind::EchoReply {
            id: u16::from_be_bytes([icmp[4], icmp[5]]),
            seq: u16::from_be_bytes([icmp[6], icmp[7]]),
        },
        (t @ (ICMP_DEST_UNREACHABLE | ICMP_TIME_EXCEEDED), code) => {
            // Quoted datagram: IPv4 header + at least 8 bytes of our echo request.
            let q = &icmp[ICMP_HDR..];
            if q.len() < IPV4_HDR + ICMP_HDR || q[0] >> 4 != 4 {
                return None;
            }
            let qihl = usize::from(q[0] & 0x0F) * 4;
            if qihl < IPV4_HDR || q.len() < qihl + ICMP_HDR || q[9] != IPPROTO_ICMP {
                return None;
            }
            let inner = &q[qihl..];
            if inner[0] != ICMP_ECHO_REQUEST {
                return None;
            }
            let id = u16::from_be_bytes([inner[4], inner[5]]);
            let seq = u16::from_be_bytes([inner[6], inner[7]]);
            let orig_dst = Ipv4([q[16], q[17], q[18], q[19]]);
            if t == ICMP_DEST_UNREACHABLE {
                Kind::Unreachable {
                    code,
                    id,
                    seq,
                    orig_dst,
                }
            } else {
                Kind::TimeExceeded { id, seq, orig_dst }
            }
        }
        _ => return None,
    };
    Some(Event { from, kind })
}

/// Next hop for `dst`: itself when on the same subnet, else the gateway. `None`
/// when `dst` is not a usable unicast address, or is off-link with no gateway.
pub fn next_hop(dst: Ipv4, our_ip: Ipv4, prefix: u8, gateway: Option<Ipv4>) -> Option<Ipv4> {
    if !is_usable_unicast(dst) || dst == Ipv4::BROADCAST {
        return None;
    }
    let mask = match prefix {
        0 => 0,
        1..=32 => u32::MAX << (32 - u32::from(prefix)),
        _ => return None,
    };
    let a = u32::from_be_bytes(dst.0);
    let b = u32::from_be_bytes(our_ip.0);
    if a & mask == b & mask {
        // On-link, but not our own address, nor the subnet's network or
        // broadcast address (a /31 or /32 has neither).
        let host = a & !mask;
        if dst == our_ip || (prefix < 31 && (host == 0 || host == !mask)) {
            return None;
        }
        Some(dst)
    } else {
        gateway
    }
}

/// Why a ping did not produce a round-trip time.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PingError {
    /// No reply within the timeout.
    Timeout,
    /// An ICMP "destination unreachable" came back (the code is the ICMP code).
    Unreachable(u8),
    /// An ICMP "time exceeded" came back (a routing loop or a TTL too small).
    TimeExceeded,
    /// Off-link and no default gateway.
    NoRoute,
    /// The next hop did not answer ARP.
    ArpFailed,
    /// Not a usable unicast target (0.0.0.0, loopback, multicast, broadcast, our
    /// own address, the subnet broadcast).
    BadTarget,
    /// No network interface, no address (lease lost) or link down.
    NoNetwork,
    /// The network task is busy (a page is loading or another ping runs).
    Busy,
}

impl core::fmt::Display for PingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PingError::Timeout => f.write_str("timeout"),
            PingError::Unreachable(c) => write!(f, "destination unreachable (code {c})"),
            PingError::TimeExceeded => f.write_str("time exceeded"),
            PingError::NoRoute => f.write_str("no route to host"),
            PingError::ArpFailed => f.write_str("host did not answer ARP"),
            PingError::BadTarget => f.write_str("invalid target address"),
            PingError::NoNetwork => f.write_str("network unavailable"),
            PingError::Busy => f.write_str("network busy"),
        }
    }
}

/// One echo request in flight.
#[derive(Clone, Copy, Debug)]
pub struct Ping {
    target: Ipv4,
    id: u16,
    seq: u16,
    sent_us: u64,
    timeout_us: u64,
    done: bool,
}

impl Ping {
    /// A ping to `target` (identifier `id`, sequence `seq`) sent at `now_us`.
    pub fn new(target: Ipv4, id: u16, seq: u16, now_us: u64, timeout_ms: u32) -> Ping {
        Ping {
            target,
            id,
            seq,
            sent_us: now_us,
            timeout_us: u64::from(timeout_ms) * 1000,
            done: false,
        }
    }

    pub fn id(&self) -> u16 {
        self.id
    }

    pub fn seq(&self) -> u16 {
        self.seq
    }

    /// Absolute time (us) at which the ping times out.
    pub fn deadline_us(&self) -> u64 {
        self.sent_us.saturating_add(self.timeout_us)
    }

    fn finish(&mut self, r: Result<u64, PingError>) -> Option<Result<u64, PingError>> {
        self.done = true;
        Some(r)
    }

    /// Feed a classified frame. `Some(Ok(rtt_us))` for the matching reply,
    /// `Some(Err(..))` for a matching ICMP error, `None` for anything else.
    pub fn on_event(&mut self, now_us: u64, ev: &Event) -> Option<Result<u64, PingError>> {
        if self.done {
            return None;
        }
        match ev.kind {
            Kind::EchoReply { id, seq } if id == self.id && seq == self.seq => {
                if ev.from != self.target {
                    return None; // someone else's reply to our id
                }
                self.finish(Ok(now_us.saturating_sub(self.sent_us)))
            }
            Kind::Unreachable {
                code,
                id,
                seq,
                orig_dst,
            } if id == self.id && seq == self.seq && orig_dst == self.target => {
                self.finish(Err(PingError::Unreachable(code)))
            }
            Kind::TimeExceeded { id, seq, orig_dst }
                if id == self.id && seq == self.seq && orig_dst == self.target =>
            {
                self.finish(Err(PingError::TimeExceeded))
            }
            _ => None,
        }
    }

    /// Timer step: `Some(Err(Timeout))` once the deadline passes.
    pub fn poll(&mut self, now_us: u64) -> Option<Result<u64, PingError>> {
        if !self.done && now_us >= self.deadline_us() {
            return self.finish(Err(PingError::Timeout));
        }
        None
    }
}

/// Round a microsecond round trip to whole milliseconds, never reporting 0 for a
/// reply that did arrive (sub-millisecond replies read as 1 ms).
pub fn rtt_ms(rtt_us: u64) -> u32 {
    u32::try_from(rtt_us.div_ceil(1000))
        .unwrap_or(u32::MAX)
        .max(1)
}

#[cfg(test)]
mod tests {
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
}
