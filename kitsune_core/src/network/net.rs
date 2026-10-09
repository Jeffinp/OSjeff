//! Minimal network protocol logic: Ethernet, ARP, IPv4 and ICMP echo.
//!
//! Pure byte parsing/building + the internet checksum — the error-prone part of
//! a network stack — lives here and is unit-tested on the host. The kernel only
//! provides the NIC driver (DMA-free port I/O) and feeds received frames to
//! [`respond`], which returns the reply frame to transmit (so the whole
//! ARP/ping responder is testable without hardware).

pub const ETHERTYPE_ARP: u16 = 0x0806;
pub const ETHERTYPE_IPV4: u16 = 0x0800;
pub const ARP_REQUEST: u16 = 1;
pub const ARP_REPLY: u16 = 2;
pub const IPPROTO_ICMP: u8 = 1;
pub const ICMP_ECHO_REQUEST: u8 = 8;
pub const ICMP_ECHO_REPLY: u8 = 0;

const ETH_HDR: usize = 14;
const ARP_LEN: usize = 28;
const IPV4_HDR: usize = 20;
const ICMP_HDR: usize = 8;

/// 48-bit hardware (MAC) address.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mac(pub [u8; 6]);

/// IPv4 address.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Ipv4(pub [u8; 4]);

impl Ipv4 {
    /// `0.0.0.0` (also what `Default` gives): "no address yet".
    pub const UNSPECIFIED: Ipv4 = Ipv4([0; 4]);
    /// `255.255.255.255`.
    pub const BROADCAST: Ipv4 = Ipv4([255; 4]);
}

impl core::fmt::Display for Ipv4 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let [a, b, c, d] = self.0;
        write!(f, "{a}.{b}.{c}.{d}")
    }
}

/// Parse a dotted-quad IPv4 literal (`"192.168.77.2"`). Strict: exactly four
/// decimal parts of 1-3 digits, each at most 255, no leading zeros (`"010"` is
/// ambiguous between decimal and octal, so it is not an address), no signs, no
/// spaces. Used so that `http://192.168.77.2:8000/` needs no DNS lookup.
pub fn parse_ipv4(s: &[u8]) -> Option<Ipv4> {
    let mut out = [0u8; 4];
    let mut parts = s.split(|&b| b == b'.');
    for slot in &mut out {
        let p = parts.next()?;
        if p.is_empty() || p.len() > 3 || (p.len() > 1 && p[0] == b'0') {
            return None;
        }
        let mut v = 0u16;
        for &d in p {
            if !d.is_ascii_digit() {
                return None;
            }
            v = v * 10 + u16::from(d - b'0');
        }
        *slot = u8::try_from(v).ok()?;
    }
    parts.next().is_none().then_some(Ipv4(out))
}

/// Internet checksum (RFC 1071): one's-complement sum of 16-bit big-endian
/// words, folded and inverted.
pub fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8; // pad the final odd byte
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Parsed Ethernet header (source + type; the destination is not needed for the
/// responder) plus the payload slice.
struct Eth {
    src: Mac,
    ethertype: u16,
}

fn parse_eth(frame: &[u8]) -> Option<(Eth, &[u8])> {
    if frame.len() < ETH_HDR {
        return None;
    }
    let mut src = [0u8; 6];
    src.copy_from_slice(&frame[6..12]);
    let ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    Some((
        Eth {
            src: Mac(src),
            ethertype,
        },
        &frame[ETH_HDR..],
    ))
}

fn write_eth(out: &mut [u8], dst: Mac, src: Mac, ethertype: u16) {
    out[0..6].copy_from_slice(&dst.0);
    out[6..12].copy_from_slice(&src.0);
    out[12..14].copy_from_slice(&ethertype.to_be_bytes());
}

/// Build a full Ethernet+ARP reply announcing `mac`/`ip` to the requester.
/// Returns the frame length (42 bytes).
fn build_arp_reply(
    out: &mut [u8],
    mac: Mac,
    ip: Ipv4,
    target_mac: Mac,
    target_ip: Ipv4,
) -> Option<usize> {
    if out.len() < ETH_HDR + ARP_LEN {
        return None; // caller's buffer cannot hold the reply
    }
    write_eth(out, target_mac, mac, ETHERTYPE_ARP);
    let a = &mut out[ETH_HDR..ETH_HDR + ARP_LEN];
    a[0..2].copy_from_slice(&1u16.to_be_bytes()); // htype: Ethernet
    a[2..4].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes()); // ptype: IPv4
    a[4] = 6; // hlen
    a[5] = 4; // plen
    a[6..8].copy_from_slice(&ARP_REPLY.to_be_bytes());
    a[8..14].copy_from_slice(&mac.0); // sender hw
    a[14..18].copy_from_slice(&ip.0); // sender proto
    a[18..24].copy_from_slice(&target_mac.0); // target hw
    a[24..28].copy_from_slice(&target_ip.0); // target proto
    Some(ETH_HDR + ARP_LEN)
}

/// Build a broadcast "gratuitous ARP" announcing `mac`/`ip` (sender == target).
/// Sent on boot so the OS advertises itself on the wire. Returns 42.
pub fn arp_announce(out: &mut [u8], mac: Mac, ip: Ipv4) -> usize {
    write_eth(out, Mac([0xff; 6]), mac, ETHERTYPE_ARP);
    let a = &mut out[ETH_HDR..ETH_HDR + ARP_LEN];
    a[0..2].copy_from_slice(&1u16.to_be_bytes());
    a[2..4].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
    a[4] = 6;
    a[5] = 4;
    a[6..8].copy_from_slice(&ARP_REQUEST.to_be_bytes());
    a[8..14].copy_from_slice(&mac.0); // sender hw = us
    a[14..18].copy_from_slice(&ip.0); // sender proto = us
    a[18..24].copy_from_slice(&[0u8; 6]); // target hw unknown
    a[24..28].copy_from_slice(&ip.0); // target proto = us (gratuitous)
    ETH_HDR + ARP_LEN
}

/// Build a broadcast ARP request: "who has `target_ip`? tell `ip`". Returns 42.
pub fn arp_who_has(out: &mut [u8], mac: Mac, ip: Ipv4, target_ip: Ipv4) -> usize {
    write_eth(out, Mac([0xff; 6]), mac, ETHERTYPE_ARP);
    let a = &mut out[ETH_HDR..ETH_HDR + ARP_LEN];
    a[0..2].copy_from_slice(&1u16.to_be_bytes());
    a[2..4].copy_from_slice(&ETHERTYPE_IPV4.to_be_bytes());
    a[4] = 6;
    a[5] = 4;
    a[6..8].copy_from_slice(&ARP_REQUEST.to_be_bytes());
    a[8..14].copy_from_slice(&mac.0);
    a[14..18].copy_from_slice(&ip.0);
    a[18..24].copy_from_slice(&[0u8; 6]);
    a[24..28].copy_from_slice(&target_ip.0);
    ETH_HDR + ARP_LEN
}

/// If `frame` is an ARP reply addressed to `our_ip`, the `(ip, mac)` pair it
/// announces (the sender). Anything else, including a reply whose sender
/// address is not a usable unicast address, is `None`.
pub fn parse_arp_reply(frame: &[u8], our_ip: Ipv4) -> Option<(Ipv4, Mac)> {
    let (eth, p) = parse_eth(frame)?;
    if eth.ethertype != ETHERTYPE_ARP || p.len() < ARP_LEN {
        return None;
    }
    let htype = u16::from_be_bytes([p[0], p[1]]);
    let ptype = u16::from_be_bytes([p[2], p[3]]);
    if htype != 1 || ptype != ETHERTYPE_IPV4 || p[4] != 6 || p[5] != 4 {
        return None;
    }
    if u16::from_be_bytes([p[6], p[7]]) != ARP_REPLY || p[24..28] != our_ip.0 {
        return None;
    }
    let spa = Ipv4([p[14], p[15], p[16], p[17]]);
    if !is_usable_unicast(spa) {
        return None;
    }
    let sha = Mac([p[8], p[9], p[10], p[11], p[12], p[13]]);
    Some((spa, sha))
}

/// Build an ICMP echo reply for a received echo request whose ICMP payload is
/// `icmp_req` (type/code/checksum/id/seq/data), addressed back to `peer`.
fn build_icmp_reply(
    out: &mut [u8],
    mac: Mac,
    ip: Ipv4,
    peer_mac: Mac,
    peer_ip: Ipv4,
    icmp_req: &[u8],
) -> Option<usize> {
    let icmp_len = icmp_req.len();
    let total = ETH_HDR + IPV4_HDR + icmp_len;
    if icmp_len < ICMP_HDR || total > out.len() {
        return None;
    }
    write_eth(out, peer_mac, mac, ETHERTYPE_IPV4);

    // IPv4 header.
    let ip_off = ETH_HDR;
    {
        let h = &mut out[ip_off..ip_off + IPV4_HDR];
        h.fill(0);
        h[0] = 0x45; // version 4, IHL 5
        h[2..4].copy_from_slice(&((IPV4_HDR + icmp_len) as u16).to_be_bytes());
        h[8] = 64; // TTL
        h[9] = IPPROTO_ICMP;
        h[12..16].copy_from_slice(&ip.0);
        h[16..20].copy_from_slice(&peer_ip.0);
    }
    let csum = checksum(&out[ip_off..ip_off + IPV4_HDR]);
    out[ip_off + 10..ip_off + 12].copy_from_slice(&csum.to_be_bytes());

    // ICMP: echo the request, flip type to reply, recompute the checksum.
    let icmp_off = ip_off + IPV4_HDR;
    out[icmp_off..icmp_off + icmp_len].copy_from_slice(icmp_req);
    out[icmp_off] = ICMP_ECHO_REPLY;
    out[icmp_off + 1] = 0; // code
    out[icmp_off + 2] = 0; // zero checksum before recomputing
    out[icmp_off + 3] = 0;
    let csum = checksum(&out[icmp_off..icmp_off + icmp_len]);
    out[icmp_off + 2..icmp_off + 4].copy_from_slice(&csum.to_be_bytes());

    Some(total)
}

/// Given a received `frame` and our identity (`mac`, `ip`), build the reply to
/// transmit into `out` and return its length, or `None` if the frame needs no
/// answer. Handles ARP requests for our IP and ICMP echo requests to our IP
/// (i.e. makes the OS pingable).
pub fn respond(frame: &[u8], mac: Mac, ip: Ipv4, out: &mut [u8]) -> Option<usize> {
    let (eth, payload) = parse_eth(frame)?;

    match eth.ethertype {
        ETHERTYPE_ARP => {
            if payload.len() < ARP_LEN {
                return None;
            }
            // Only Ethernet/IPv4 ARP (htype 1, ptype 0x0800, hlen 6, plen 4):
            // the field offsets below are only valid for those sizes.
            let htype = u16::from_be_bytes([payload[0], payload[1]]);
            let ptype = u16::from_be_bytes([payload[2], payload[3]]);
            if htype != 1 || ptype != ETHERTYPE_IPV4 || payload[4] != 6 || payload[5] != 4 {
                return None;
            }
            let oper = u16::from_be_bytes([payload[6], payload[7]]);
            let tpa = Ipv4([payload[24], payload[25], payload[26], payload[27]]);
            if oper != ARP_REQUEST || tpa != ip {
                return None;
            }
            let sha = Mac([
                payload[8],
                payload[9],
                payload[10],
                payload[11],
                payload[12],
                payload[13],
            ]);
            let spa = Ipv4([payload[14], payload[15], payload[16], payload[17]]);
            build_arp_reply(out, mac, ip, sha, spa)
        }
        ETHERTYPE_IPV4 => {
            if payload.len() < IPV4_HDR {
                return None;
            }
            let ihl = (payload[0] & 0x0F) as usize * 4;
            // Trim any Ethernet padding using the IPv4 total-length field.
            let total_len =
                (u16::from_be_bytes([payload[2], payload[3]]) as usize).min(payload.len());
            if ihl < IPV4_HDR || total_len < ihl {
                return None;
            }
            let proto = payload[9];
            let src = Ipv4([payload[12], payload[13], payload[14], payload[15]]);
            let dst = Ipv4([payload[16], payload[17], payload[18], payload[19]]);
            if proto != IPPROTO_ICMP || dst != ip {
                return None;
            }
            let icmp = &payload[ihl..total_len];
            if icmp.len() < ICMP_HDR || icmp[0] != ICMP_ECHO_REQUEST {
                return None;
            }
            // Copy the ICMP request out so we can borrow `out` mutably.
            let mut tmp = [0u8; 1500];
            let n = icmp.len().min(tmp.len());
            tmp[..n].copy_from_slice(&icmp[..n]);
            build_icmp_reply(out, mac, ip, eth.src, src, &tmp[..n])
        }
        _ => None,
    }
}

// ---- UDP + DHCP client (acquire an IP automatically) ----

/// Largest Ethernet frame the NIC driver will put on the wire (MTU 1500 +
/// 14-byte header). The NE2000 transmit buffer is 6 pages (1536 bytes) wide;
/// anything longer would spill into the receive ring.
pub const MAX_TX_FRAME: usize = 1514;
/// Ethernet minimum frame length without FCS (shorter frames are padded).
pub const MIN_TX_FRAME: usize = 60;

/// Length actually transmitted for a frame of `len` bytes: padded up to the
/// Ethernet minimum, clamped to [`MAX_TX_FRAME`].
pub fn tx_len(len: usize) -> usize {
    len.clamp(MIN_TX_FRAME, MAX_TX_FRAME)
}

/// The ring page just before `page` in a receive ring `[start, stop)`, wrapping
/// from `start` back to the last page. Total over every `u8`: a garbled hardware
/// pointer outside the ring (below `start`, e.g. 0, or above `stop`) must not
/// underflow or point outside the ring, so it also maps to the last page.
pub fn ring_prev_page(page: u8, start: u8, stop: u8) -> u8 {
    if page <= start || page > stop {
        stop - 1
    } else {
        page - 1
    }
}

pub const IPPROTO_UDP: u8 = 17;
const UDP_HDR: usize = 8;
const DHCP_CLIENT_PORT: u16 = 68;
const DHCP_SERVER_PORT: u16 = 67;
const BOOTREQUEST: u8 = 1;
const BOOTREPLY: u8 = 2;
const DHCP_MAGIC: u32 = 0x6382_5363;
/// BOOTP fixed area length (op..file), i.e. everything before the magic cookie.
const BOOTP_FIXED: usize = 236;

// DHCP message types (option 53).
pub const DHCP_DISCOVER: u8 = 1;
pub const DHCP_OFFER: u8 = 2;
pub const DHCP_REQUEST: u8 = 3;
pub const DHCP_ACK: u8 = 5;
pub const DHCP_NAK: u8 = 6;
pub const DHCP_RELEASE: u8 = 7;

/// What a parsed DHCP reply tells us.
///
/// Every field is the *raw* content of the reply, already checked for being
/// well-formed (right option length, usable unicast address) but not yet
/// interpreted: turning an ACK into something the stack can use is
/// [`NetConfig::from_ack`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DhcpReply {
    pub msg_type: u8, // DHCP_OFFER / DHCP_ACK / DHCP_NAK
    pub xid: u32,
    pub your_ip: Ipv4,           // yiaddr — the address offered/assigned
    pub server_id: Option<Ipv4>, // option 54 (a REQUEST cannot be built without it)
    pub subnet: Option<Ipv4>,    // option 1 (exactly 4 bytes)
    pub router: Option<Ipv4>,    // option 3 (first usable entry of the list)
    pub dns: DnsServers,         // option 6 (every usable entry, up to `MAX_DNS`)
    pub lease_secs: Option<u32>, // option 51 (0xFFFF_FFFF = infinite, kept raw)
    /// Ethernet source of the frame that carried the reply: where a later
    /// unicast RENEW/RELEASE for this server is sent (no ARP needed).
    pub eth_src: Mac,
}

/// How many DNS servers the stack keeps (option 6 may list more; the rest are
/// dropped).
pub const MAX_DNS: usize = 3;

/// An ordered set of at most [`MAX_DNS`] distinct, usable DNS server addresses
/// (the resolver tries them in order and rotates on failure).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DnsServers {
    addrs: [Ipv4; MAX_DNS],
    len: u8,
}

impl DnsServers {
    /// No resolver.
    pub const NONE: DnsServers = DnsServers {
        addrs: [Ipv4([0; 4]); MAX_DNS],
        len: 0,
    };

    /// A single resolver (unusable addresses give [`DnsServers::NONE`]).
    pub fn one(ip: Ipv4) -> DnsServers {
        let mut d = DnsServers::NONE;
        d.push(ip);
        d
    }

    /// Build from a slice, keeping the usable, distinct entries in order.
    pub fn from_slice(ips: &[Ipv4]) -> DnsServers {
        let mut d = DnsServers::NONE;
        for &ip in ips {
            d.push(ip);
        }
        d
    }

    /// Append `ip` if it is a usable unicast address, not already present and
    /// there is room. Returns whether it was added.
    pub fn push(&mut self, ip: Ipv4) -> bool {
        if !is_usable_unicast(ip) || self.as_slice().contains(&ip) || self.len() >= MAX_DNS {
            return false;
        }
        self.addrs[self.len()] = ip;
        self.len += 1;
        true
    }

    pub fn as_slice(&self) -> &[Ipv4] {
        &self.addrs[..self.len()]
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The preferred (first) server.
    pub fn first(&self) -> Option<Ipv4> {
        self.as_slice().first().copied()
    }

    /// Copy of `self` without `ip` (a DNS server that is our own address cannot
    /// be reached).
    fn without(&self, ip: Ipv4) -> DnsServers {
        let mut d = DnsServers::NONE;
        for &a in self.as_slice() {
            if a != ip {
                d.push(a);
            }
        }
        d
    }
}

/// `10.0.2.3` or `10.0.2.3,8.8.8.8`; `none` when empty.
impl core::fmt::Display for DnsServers {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_empty() {
            return f.write_str("none");
        }
        for (i, a) in self.as_slice().iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            write!(f, "{a}")?;
        }
        Ok(())
    }
}

/// Every usable entry of an option holding a list of IPv4 addresses (DNS
/// servers), in order. Malformed lengths yield an empty set.
fn usable_addrs(v: &[u8]) -> DnsServers {
    if v.is_empty() || !v.len().is_multiple_of(4) {
        return DnsServers::NONE;
    }
    let (addrs, _) = v.as_chunks::<4>();
    let mut d = DnsServers::NONE;
    for &a in addrs {
        d.push(Ipv4(a));
    }
    d
}

/// True for an address a host can sit behind or talk to: not `0.0.0.0/8`, not
/// loopback, not multicast/reserved/broadcast (`224.0.0.0` and up).
pub fn is_usable_unicast(ip: Ipv4) -> bool {
    let a = ip.0[0];
    a != 0 && a != 127 && a < 224
}

/// First usable unicast entry of an option holding a list of IPv4 addresses
/// (routers, DNS servers). RFC 2132 makes the length a non-zero multiple of 4;
/// anything else is malformed and yields `None`.
fn first_usable_addr(v: &[u8]) -> Option<Ipv4> {
    if v.is_empty() || !v.len().is_multiple_of(4) {
        return None;
    }
    let (addrs, _) = v.as_chunks::<4>();
    addrs
        .iter()
        .map(|&a| Ipv4(a))
        .find(|&a| is_usable_unicast(a))
}

/// A single-address option: exactly 4 bytes and usable.
fn single_addr(v: &[u8]) -> Option<Ipv4> {
    let a = first_usable_addr(v)?;
    (v.len() == 4).then_some(a)
}

/// Who a DHCP client message goes to, and from which IP.
#[derive(Clone, Copy)]
struct DhcpPath {
    dst_mac: Mac,
    src_ip: Ipv4,
    dst_ip: Ipv4,
}

impl DhcpPath {
    /// Link and IP broadcast from `0.0.0.0` (DISCOVER, SELECTING REQUEST).
    const BROADCAST: DhcpPath = DhcpPath {
        dst_mac: Mac([0xff; 6]),
        src_ip: Ipv4::UNSPECIFIED,
        dst_ip: Ipv4::BROADCAST,
    };
}

/// Lay down the Ethernet/IPv4/UDP/BOOTP envelope for a DHCP message from `mac`
/// with transaction id `xid`, `ciaddr` set to `ciaddr` (all zeros unless the
/// client already holds the address). Returns the offset where DHCP options
/// begin (right after the magic cookie).
fn dhcp_envelope(out: &mut [u8], mac: Mac, xid: u32, path: DhcpPath, ciaddr: Ipv4) -> usize {
    write_eth(out, path.dst_mac, mac, ETHERTYPE_IPV4);
    let b = ETH_HDR + IPV4_HDR + UDP_HDR; // BOOTP start
    let opt_start = b + BOOTP_FIXED + 4; // after the magic cookie
    out[b..opt_start].fill(0);
    out[b] = BOOTREQUEST;
    out[b + 1] = 1; // htype = ethernet
    out[b + 2] = 6; // hlen
    out[b + 4..b + 8].copy_from_slice(&xid.to_be_bytes());
    // The broadcast flag asks the server to answer by broadcast: needed while we
    // have no address (ciaddr unset), pointless when we already do.
    if ciaddr == Ipv4::UNSPECIFIED {
        out[b + 10..b + 12].copy_from_slice(&0x8000u16.to_be_bytes());
    }
    out[b + 12..b + 16].copy_from_slice(&ciaddr.0);
    out[b + 28..b + 34].copy_from_slice(&mac.0); // chaddr (client MAC)
    out[b + BOOTP_FIXED..opt_start].copy_from_slice(&DHCP_MAGIC.to_be_bytes());
    opt_start
}

/// Fill the UDP + IPv4 headers for a DHCP packet whose content ends at `end`,
/// checksum the IP header, and return `end`. The UDP checksum is left zero,
/// which RFC 768 permits over IPv4.
fn dhcp_finalize(out: &mut [u8], end: usize, path: DhcpPath) -> usize {
    let ip_off = ETH_HDR;
    let udp_off = ETH_HDR + IPV4_HDR;
    let udp_len = end - udp_off;
    out[udp_off..udp_off + 2].copy_from_slice(&DHCP_CLIENT_PORT.to_be_bytes());
    out[udp_off + 2..udp_off + 4].copy_from_slice(&DHCP_SERVER_PORT.to_be_bytes());
    out[udp_off + 4..udp_off + 6].copy_from_slice(&(udp_len as u16).to_be_bytes());
    out[udp_off + 6..udp_off + 8].copy_from_slice(&[0, 0]); // no UDP checksum
    {
        let h = &mut out[ip_off..ip_off + IPV4_HDR];
        h.fill(0);
        h[0] = 0x45; // IPv4, IHL 5
        h[2..4].copy_from_slice(&((IPV4_HDR + udp_len) as u16).to_be_bytes());
        h[8] = 64; // TTL
        h[9] = IPPROTO_UDP;
        h[12..16].copy_from_slice(&path.src_ip.0);
        h[16..20].copy_from_slice(&path.dst_ip.0);
    }
    let csum = checksum(&out[ip_off..ip_off + IPV4_HDR]);
    out[ip_off + 10..ip_off + 12].copy_from_slice(&csum.to_be_bytes());
    end
}

/// Append a TLV option; returns the new write offset.
fn put_opt(out: &mut [u8], o: usize, code: u8, val: &[u8]) -> usize {
    out[o] = code;
    out[o + 1] = val.len() as u8;
    out[o + 2..o + 2 + val.len()].copy_from_slice(val);
    o + 2 + val.len()
}

/// Parameter request list: subnet mask, router, DNS servers.
fn put_param_request(out: &mut [u8], o: usize) -> usize {
    put_opt(out, o, 55, &[1, 3, 6])
}

/// Smallest buffer any of the DHCP builders needs.
pub const DHCP_BUILD_MIN: usize = ETH_HDR + IPV4_HDR + UDP_HDR + BOOTP_FIXED + 4 + 32;

/// Build a DHCPDISCOVER broadcast from `mac`, transaction id `xid`.
pub fn dhcp_discover(out: &mut [u8], mac: Mac, xid: u32) -> usize {
    let p = DhcpPath::BROADCAST;
    let mut o = dhcp_envelope(out, mac, xid, p, Ipv4::UNSPECIFIED);
    o = put_opt(out, o, 53, &[DHCP_DISCOVER]);
    o = put_param_request(out, o);
    out[o] = 255; // end
    dhcp_finalize(out, o + 1, p)
}

/// Build a DHCPREQUEST for `requested_ip` from `server_id`, transaction `xid`
/// (the SELECTING state: answers an OFFER, broadcast).
pub fn dhcp_request(
    out: &mut [u8],
    mac: Mac,
    xid: u32,
    requested_ip: Ipv4,
    server_id: Ipv4,
) -> usize {
    let p = DhcpPath::BROADCAST;
    let mut o = dhcp_envelope(out, mac, xid, p, Ipv4::UNSPECIFIED);
    o = put_opt(out, o, 53, &[DHCP_REQUEST]);
    o = put_opt(out, o, 50, &requested_ip.0);
    o = put_opt(out, o, 54, &server_id.0);
    o = put_param_request(out, o);
    out[o] = 255;
    dhcp_finalize(out, o + 1, p)
}

/// Build a DHCPREQUEST that extends a lease we hold (RFC 2131 4.3.2).
///
/// * RENEWING (`unicast_to = Some((server_mac, server_ip))`): sent straight to
///   the server that granted the lease, from our own address.
/// * REBINDING (`None`): broadcast, because the first server stopped answering.
///
/// In both, `ciaddr` carries our address and neither option 50 (requested IP)
/// nor option 54 (server id) is present.
pub fn dhcp_request_renew(
    out: &mut [u8],
    mac: Mac,
    xid: u32,
    our_ip: Ipv4,
    unicast_to: Option<(Mac, Ipv4)>,
) -> usize {
    let p = match unicast_to {
        Some((dst_mac, dst_ip)) => DhcpPath {
            dst_mac,
            src_ip: our_ip,
            dst_ip,
        },
        None => DhcpPath {
            dst_mac: Mac([0xff; 6]),
            src_ip: our_ip,
            dst_ip: Ipv4::BROADCAST,
        },
    };
    let mut o = dhcp_envelope(out, mac, xid, p, our_ip);
    o = put_opt(out, o, 53, &[DHCP_REQUEST]);
    o = put_param_request(out, o);
    out[o] = 255;
    dhcp_finalize(out, o + 1, p)
}

/// Build a DHCPRELEASE: tells `server` (reached at `server_mac`) that we give
/// `our_ip` back. Unicast, no reply expected.
pub fn dhcp_release(
    out: &mut [u8],
    mac: Mac,
    xid: u32,
    our_ip: Ipv4,
    server_mac: Mac,
    server_ip: Ipv4,
) -> usize {
    let p = DhcpPath {
        dst_mac: server_mac,
        src_ip: our_ip,
        dst_ip: server_ip,
    };
    let mut o = dhcp_envelope(out, mac, xid, p, our_ip);
    o = put_opt(out, o, 53, &[DHCP_RELEASE]);
    o = put_opt(out, o, 54, &server_ip.0);
    out[o] = 255;
    dhcp_finalize(out, o + 1, p)
}

/// Parse a received `frame` as a DHCP reply addressed to us (UDP -> port 68 with
/// our MAC in chaddr). `None` if it is not a DHCP reply we should act on.
pub fn parse_dhcp(frame: &[u8], our_mac: Mac) -> Option<DhcpReply> {
    let (eth, payload) = parse_eth(frame)?;
    if eth.ethertype != ETHERTYPE_IPV4 || payload.len() < IPV4_HDR {
        return None;
    }
    let ihl = (payload[0] & 0x0F) as usize * 4;
    if ihl < IPV4_HDR || payload[9] != IPPROTO_UDP {
        return None;
    }
    let total = (u16::from_be_bytes([payload[2], payload[3]]) as usize).min(payload.len());
    if total < ihl + UDP_HDR {
        return None;
    }
    let udp = &payload[ihl..total];
    if u16::from_be_bytes([udp[2], udp[3]]) != DHCP_CLIENT_PORT {
        return None;
    }
    let dhcp = &udp[UDP_HDR..];
    if dhcp.len() < BOOTP_FIXED + 4 || dhcp[0] != BOOTREPLY || dhcp[28..34] != our_mac.0 {
        return None;
    }
    let magic = u32::from_be_bytes([
        dhcp[BOOTP_FIXED],
        dhcp[BOOTP_FIXED + 1],
        dhcp[BOOTP_FIXED + 2],
        dhcp[BOOTP_FIXED + 3],
    ]);
    if magic != DHCP_MAGIC {
        return None;
    }
    let xid = u32::from_be_bytes([dhcp[4], dhcp[5], dhcp[6], dhcp[7]]);
    let your_ip = Ipv4([dhcp[16], dhcp[17], dhcp[18], dhcp[19]]);

    let mut msg_type = 0u8;
    let (mut server_id, mut subnet, mut router) = (None, None, None);
    let mut dns = DnsServers::NONE;
    let mut lease_secs = None;
    let mut i = BOOTP_FIXED + 4;
    while i < dhcp.len() {
        let code = dhcp[i];
        if code == 255 {
            break; // end
        }
        if code == 0 {
            i += 1; // pad
            continue;
        }
        if i + 1 >= dhcp.len() {
            break;
        }
        let len = dhcp[i + 1] as usize;
        let val = i + 2;
        if val + len > dhcp.len() {
            break;
        }
        let v = &dhcp[val..val + len];
        match code {
            53 if len >= 1 => msg_type = v[0],
            54 => server_id = single_addr(v),
            1 if len == 4 => subnet = Some(Ipv4([v[0], v[1], v[2], v[3]])),
            3 => router = first_usable_addr(v),
            6 => dns = usable_addrs(v),
            51 if len == 4 => lease_secs = Some(u32::from_be_bytes([v[0], v[1], v[2], v[3]])),
            _ => {}
        }
        i = val + len;
    }
    if msg_type == 0 {
        return None;
    }
    Some(DhcpReply {
        msg_type,
        xid,
        your_ip,
        server_id,
        subnet,
        router,
        dns,
        lease_secs,
        eth_src: eth.src,
    })
}

// ---- The result of DHCP, as a plain value ----

/// Prefix length assumed when an ACK carries no subnet-mask option (the common
/// home/SLIRP /24).
pub const DEFAULT_PREFIX: u8 = 24;

/// Everything the TCP/IP stack and the ARP/ping responder need to know about
/// this host's network identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetConfig {
    /// Our address.
    pub ip: Ipv4,
    /// Subnet prefix length, `1..=30`.
    pub prefix: u8,
    /// Default gateway; `None` means "no default route" (on-link only).
    pub gateway: Option<Ipv4>,
    /// DNS resolvers in preference order; empty means "no resolver" (only IPv4
    /// literals resolve).
    pub dns: DnsServers,
    /// Lease duration in seconds; `None` means the address never expires.
    pub lease_secs: Option<u32>,
}

/// `192.168.77.15/24 gw 192.168.77.2 dns 192.168.77.3` (`none` when absent,
/// `a,b` for several resolvers): the form the boot log uses.
impl core::fmt::Display for NetConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}/{} gw ", self.ip, self.prefix)?;
        match self.gateway {
            Some(g) => write!(f, "{g}")?,
            None => f.write_str("none")?,
        }
        write!(f, " dns {}", self.dns)
    }
}

/// Why an ACK could not become a [`NetConfig`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// The reply is not a DHCPACK.
    NotAck,
    /// `yiaddr` is not a usable host address (0.0.0.0, loopback, multicast,
    /// broadcast) or is the network/broadcast address of its own subnet.
    BadAddress,
    /// The subnet mask is not a contiguous run of ones, or its prefix is
    /// outside `1..=30`.
    BadMask,
    /// The lease time is zero seconds.
    ZeroLease,
}

/// Prefix length of a netmask, or `None` if the mask is not contiguous ones
/// followed by zeros (e.g. `255.0.255.0`). `0.0.0.0` is prefix 0.
pub fn mask_to_prefix(mask: Ipv4) -> Option<u8> {
    let m = u32::from_be_bytes(mask.0);
    let ones = m.leading_ones();
    // A valid mask is `ones` ones then only zeros.
    let expect = if ones == 0 {
        0
    } else {
        u32::MAX << (32 - ones)
    };
    (m == expect).then_some(ones as u8)
}

impl NetConfig {
    /// What the boot uses when no DHCP server answers (or there is no NIC):
    /// QEMU's user-mode (SLIRP) defaults, 10.0.2.15/24, gateway 10.0.2.2, DNS
    /// 10.0.2.3, no expiry.
    pub const STATIC_FALLBACK: NetConfig = NetConfig {
        ip: Ipv4([10, 0, 2, 15]),
        prefix: DEFAULT_PREFIX,
        gateway: Some(Ipv4([10, 0, 2, 2])),
        dns: DnsServers {
            addrs: [Ipv4([10, 0, 2, 3]), Ipv4([0; 4]), Ipv4([0; 4])],
            len: 1,
        },
        lease_secs: None,
    };

    /// Interpret a DHCPACK. Total: any combination of options (missing,
    /// truncated, wrong-sized, hostile values) yields a config or a
    /// [`ConfigError`], never a panic.
    ///
    /// Defaults for what the server leaves out:
    /// * no subnet mask -> `/24` ([`DEFAULT_PREFIX`]); an invalid one is an error
    ///   (a wrong mask silently misroutes everything);
    /// * no router -> the DHCP server itself (option 54), which is the on-link
    ///   router in SLIRP and in consumer gateways; with no server id either, no
    ///   default route;
    /// * no DNS server -> the gateway (routers usually forward DNS); with no
    ///   gateway either, no resolver; every listed server is kept (up to
    ///   [`MAX_DNS`]), in the server's order;
    /// * no lease time, or `0xFFFF_FFFF` -> never expires; `0` is an error;
    /// * a gateway/DNS equal to our own address is dropped (it cannot be reached).
    pub fn from_ack(r: &DhcpReply) -> Result<NetConfig, ConfigError> {
        if r.msg_type != DHCP_ACK {
            return Err(ConfigError::NotAck);
        }
        let prefix = match r.subnet {
            None => DEFAULT_PREFIX,
            Some(m) => match mask_to_prefix(m) {
                Some(p @ 1..=30) => p,
                _ => return Err(ConfigError::BadMask),
            },
        };
        let ip = r.your_ip;
        if !is_usable_unicast(ip) {
            return Err(ConfigError::BadAddress);
        }
        let host_mask = u32::MAX >> prefix; // the host bits
        let host = u32::from_be_bytes(ip.0) & host_mask;
        if host == 0 || host == host_mask {
            return Err(ConfigError::BadAddress); // network or broadcast address
        }
        let lease_secs = match r.lease_secs {
            Some(0) => return Err(ConfigError::ZeroLease),
            Some(0xFFFF_FFFF) | None => None,
            Some(n) => Some(n),
        };
        let not_us = |a: &Ipv4| *a != ip;
        let gateway = r.router.or(r.server_id).filter(not_us);
        let mut dns = r.dns.without(ip);
        if dns.is_empty()
            && let Some(g) = gateway
        {
            dns.push(g);
        }
        Ok(NetConfig {
            ip,
            prefix,
            gateway,
            dns,
            lease_secs,
        })
    }

    /// True once a lease of `lease_secs` has run for `elapsed_secs`.
    pub fn lease_expired(&self, elapsed_secs: u64) -> bool {
        self.lease_secs
            .is_some_and(|l| elapsed_secs >= u64::from(l))
    }
}

#[cfg(test)]
mod tests;
