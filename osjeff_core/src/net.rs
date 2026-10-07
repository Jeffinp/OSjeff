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
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ipv4(pub [u8; 4]);

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
    pub dns: Option<Ipv4>,       // option 6 (first usable entry of the list)
    pub lease_secs: Option<u32>, // option 51 (0xFFFF_FFFF = infinite, kept raw)
}

/// True for an address a host can sit behind or talk to: not `0.0.0.0/8`, not
/// loopback, not multicast/reserved/broadcast (`224.0.0.0` and up).
fn is_usable_unicast(ip: Ipv4) -> bool {
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

/// Lay down the Ethernet/IPv4/UDP/BOOTP envelope for a broadcast DHCP message
/// from `mac` with transaction id `xid`. Returns the offset where DHCP options
/// begin (right after the magic cookie).
fn dhcp_envelope(out: &mut [u8], mac: Mac, xid: u32) -> usize {
    write_eth(out, Mac([0xff; 6]), mac, ETHERTYPE_IPV4); // broadcast
    let b = ETH_HDR + IPV4_HDR + UDP_HDR; // BOOTP start
    let opt_start = b + BOOTP_FIXED + 4; // after the magic cookie
    out[b..opt_start].fill(0);
    out[b] = BOOTREQUEST;
    out[b + 1] = 1; // htype = ethernet
    out[b + 2] = 6; // hlen
    out[b + 4..b + 8].copy_from_slice(&xid.to_be_bytes());
    out[b + 10..b + 12].copy_from_slice(&0x8000u16.to_be_bytes()); // broadcast flag
    out[b + 28..b + 34].copy_from_slice(&mac.0); // chaddr (client MAC)
    out[b + BOOTP_FIXED..opt_start].copy_from_slice(&DHCP_MAGIC.to_be_bytes());
    opt_start
}

/// Fill the UDP + IPv4 headers for a DHCP packet whose content ends at `end`,
/// checksum the IP header, and return `end`. The UDP checksum is left zero,
/// which RFC 768 permits over IPv4.
fn dhcp_finalize(out: &mut [u8], end: usize) -> usize {
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
        h[16..20].copy_from_slice(&[255, 255, 255, 255]); // dst broadcast; src 0.0.0.0
    }
    let csum = checksum(&out[ip_off..ip_off + IPV4_HDR]);
    out[ip_off + 10..ip_off + 12].copy_from_slice(&csum.to_be_bytes());
    end
}

/// Build a DHCPDISCOVER broadcast from `mac`, transaction id `xid`.
pub fn dhcp_discover(out: &mut [u8], mac: Mac, xid: u32) -> usize {
    let mut o = dhcp_envelope(out, mac, xid);
    out[o] = 53; // message type
    out[o + 1] = 1;
    out[o + 2] = DHCP_DISCOVER;
    o += 3;
    out[o] = 55; // parameter request list: subnet, router, DNS
    out[o + 1] = 3;
    out[o + 2] = 1;
    out[o + 3] = 3;
    out[o + 4] = 6;
    o += 5;
    out[o] = 255; // end
    o += 1;
    dhcp_finalize(out, o)
}

/// Build a DHCPREQUEST for `requested_ip` from `server_id`, transaction `xid`.
pub fn dhcp_request(
    out: &mut [u8],
    mac: Mac,
    xid: u32,
    requested_ip: Ipv4,
    server_id: Ipv4,
) -> usize {
    let mut o = dhcp_envelope(out, mac, xid);
    out[o] = 53;
    out[o + 1] = 1;
    out[o + 2] = DHCP_REQUEST;
    o += 3;
    out[o] = 50; // requested IP
    out[o + 1] = 4;
    out[o + 2..o + 6].copy_from_slice(&requested_ip.0);
    o += 6;
    out[o] = 54; // server identifier
    out[o + 1] = 4;
    out[o + 2..o + 6].copy_from_slice(&server_id.0);
    o += 6;
    out[o] = 55;
    out[o + 1] = 3;
    out[o + 2] = 1;
    out[o + 3] = 3;
    out[o + 4] = 6;
    o += 5;
    out[o] = 255;
    o += 1;
    dhcp_finalize(out, o)
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
    let (mut server_id, mut subnet, mut router, mut dns) = (None, None, None, None);
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
            6 => dns = first_usable_addr(v),
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
    /// DNS resolver; `None` means "no resolver" (only IPv4 literals resolve).
    pub dns: Option<Ipv4>,
    /// Lease duration in seconds; `None` means the address never expires.
    pub lease_secs: Option<u32>,
}

/// `192.168.77.15/24 gw 192.168.77.2 dns 192.168.77.3` (`none` when absent):
/// the form the boot log uses.
impl core::fmt::Display for NetConfig {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}/{} gw ", self.ip, self.prefix)?;
        match self.gateway {
            Some(g) => write!(f, "{g}")?,
            None => f.write_str("none")?,
        }
        f.write_str(" dns ")?;
        match self.dns {
            Some(d) => write!(f, "{d}"),
            None => f.write_str("none"),
        }
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
        dns: Some(Ipv4([10, 0, 2, 3])),
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
    ///   gateway either, no resolver;
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
        let dns = r.dns.filter(not_us).or(gateway);
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
mod tests {
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
        assert_eq!(r.dns, None);
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
            0x45u8, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8,
            0x00, 0x01, 0xc0, 0xa8, 0x00, 0xc7,
        ];
        assert_eq!(checksum(&hdr), 0xb861);
    }

    #[test]
    fn checksum_over_header_with_csum_is_zero() {
        let mut hdr = [
            0x45u8, 0x00, 0x00, 0x73, 0x00, 0x00, 0x40, 0x00, 0x40, 0x11, 0x00, 0x00, 0xc0, 0xa8,
            0x00, 0x01, 0xc0, 0xa8, 0x00, 0xc7,
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
                dns: Some(LEASE_DNS),
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
        assert_eq!(c.dns, Some(LEASE_GW));
        // Neither router nor server id: no default route, no resolver.
        let r = ack_with(&[T_ACK, MASK24]);
        let c = NetConfig::from_ack(&r).unwrap();
        assert_eq!((c.gateway, c.dns), (None, None));
        // Router but no DNS: DNS = router.
        let r = ack_with(&[T_ACK, (3, &[192, 168, 77, 1])]);
        let c = NetConfig::from_ack(&r).unwrap();
        assert_eq!(c.dns, Some(Ipv4([192, 168, 77, 1])));
        // An explicit DNS wins over the router.
        let r = ack_with(&[T_ACK, ROUTER, (6, &[8, 8, 8, 8])]);
        assert_eq!(
            NetConfig::from_ack(&r).unwrap().dns,
            Some(Ipv4([8, 8, 8, 8]))
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
                (None, None, None),
                "{bad:?}"
            );
        }
        // Unusable addresses: 0.0.0.0, loopback, multicast, broadcast.
        for bad in [[0u8, 0, 0, 0], [127, 0, 0, 1], [224, 0, 0, 1], [255; 4]] {
            let r = ack_with(&[T_ACK, (54, &bad), (3, &bad), (6, &bad)]);
            assert_eq!(
                (r.server_id, r.router, r.dns),
                (None, None, None),
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
    fn gateway_or_dns_equal_to_our_ip_is_dropped() {
        let r = ack_with(&[T_ACK, (3, &LEASE_IP.0), (6, &LEASE_IP.0)]);
        let c = NetConfig::from_ack(&r).unwrap();
        assert_eq!((c.gateway, c.dns), (None, None));
    }

    #[test]
    fn static_fallback_is_the_slirp_default() {
        let f = NetConfig::STATIC_FALLBACK;
        assert_eq!(f.ip, Ipv4([10, 0, 2, 15]));
        assert_eq!(f.prefix, 24);
        assert_eq!(f.gateway, Some(Ipv4([10, 0, 2, 2])));
        assert_eq!(f.dns, Some(Ipv4([10, 0, 2, 3])));
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
            dns: None,
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
}
