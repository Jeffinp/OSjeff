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
fn build_arp_reply(out: &mut [u8], mac: Mac, ip: Ipv4, target_mac: Mac, target_ip: Ipv4) -> usize {
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
    ETH_HDR + ARP_LEN
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
            Some(build_arp_reply(out, mac, ip, sha, spa))
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DhcpReply {
    pub msg_type: u8, // DHCP_OFFER / DHCP_ACK / DHCP_NAK
    pub xid: u32,
    pub your_ip: Ipv4,        // yiaddr — the address offered/assigned
    pub server_id: Ipv4,      // option 54
    pub subnet: Option<Ipv4>, // option 1
    pub router: Option<Ipv4>, // option 3
    pub dns: Option<Ipv4>,    // option 6
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

    let (mut msg_type, mut server_id) = (0u8, Ipv4([0; 4]));
    let (mut subnet, mut router, mut dns) = (None, None, None);
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
        let ip4 = || Ipv4([v[0], v[1], v[2], v[3]]);
        match code {
            53 if len >= 1 => msg_type = v[0],
            54 if len >= 4 => server_id = ip4(),
            1 if len >= 4 => subnet = Some(ip4()),
            3 if len >= 4 => router = Some(ip4()),
            6 if len >= 4 => dns = Some(ip4()),
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUR_MAC: Mac = Mac([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
    const OUR_IP: Ipv4 = Ipv4([10, 0, 2, 15]);
    const PEER_MAC: Mac = Mac([0x52, 0x55, 0x0a, 0x00, 0x02, 0x02]);
    const PEER_IP: Ipv4 = Ipv4([10, 0, 2, 2]);

    /// Craft a BOOTREPLY (server -> client) DHCP packet for round-trip tests.
    fn craft_reply(
        out: &mut [u8],
        our_mac: Mac,
        xid: u32,
        your_ip: Ipv4,
        server_id: Ipv4,
        msg_type: u8,
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
        let mut o = opt;
        out[o] = 53;
        out[o + 1] = 1;
        out[o + 2] = msg_type;
        o += 3;
        out[o] = 54;
        out[o + 1] = 4;
        out[o + 2..o + 6].copy_from_slice(&server_id.0);
        o += 6;
        out[o] = 1;
        out[o + 1] = 4;
        out[o + 2..o + 6].copy_from_slice(&[255, 255, 255, 0]); // subnet
        o += 6;
        out[o] = 3;
        out[o + 1] = 4;
        out[o + 2..o + 6].copy_from_slice(&PEER_IP.0); // router
        o += 6;
        out[o] = 255;
        o += 1;
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
        assert_eq!(r.server_id, server);
        assert_eq!(r.subnet, Some(Ipv4([255, 255, 255, 0])));
        assert_eq!(r.router, Some(PEER_IP));
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
}
