//! dhcp (split out of `net.rs`).

use super::*;

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

pub(super) const UDP_HDR: usize = 8;

pub(super) const DHCP_CLIENT_PORT: u16 = 68;

pub(super) const DHCP_SERVER_PORT: u16 = 67;

pub(super) const BOOTREQUEST: u8 = 1;

pub(super) const BOOTREPLY: u8 = 2;

pub(super) const DHCP_MAGIC: u32 = 0x6382_5363;

/// BOOTP fixed area length (op..file), i.e. everything before the magic cookie.
pub(super) const BOOTP_FIXED: usize = 236;

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
    pub(super) addrs: [Ipv4; MAX_DNS],
    pub(super) len: u8,
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
    pub(super) fn without(&self, ip: Ipv4) -> DnsServers {
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
pub(super) fn usable_addrs(v: &[u8]) -> DnsServers {
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
pub(super) fn first_usable_addr(v: &[u8]) -> Option<Ipv4> {
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
pub(super) fn single_addr(v: &[u8]) -> Option<Ipv4> {
    let a = first_usable_addr(v)?;
    (v.len() == 4).then_some(a)
}

/// Who a DHCP client message goes to, and from which IP.
#[derive(Clone, Copy)]
pub(super) struct DhcpPath {
    pub(super) dst_mac: Mac,
    pub(super) src_ip: Ipv4,
    pub(super) dst_ip: Ipv4,
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
pub(super) fn dhcp_envelope(
    out: &mut [u8],
    mac: Mac,
    xid: u32,
    path: DhcpPath,
    ciaddr: Ipv4,
) -> usize {
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
pub(super) fn dhcp_finalize(out: &mut [u8], end: usize, path: DhcpPath) -> usize {
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
pub(super) fn put_opt(out: &mut [u8], o: usize, code: u8, val: &[u8]) -> usize {
    out[o] = code;
    out[o + 1] = val.len() as u8;
    out[o + 2..o + 2 + val.len()].copy_from_slice(val);
    o + 2 + val.len()
}

/// Parameter request list: subnet mask, router, DNS servers.
pub(super) fn put_param_request(out: &mut [u8], o: usize) -> usize {
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
