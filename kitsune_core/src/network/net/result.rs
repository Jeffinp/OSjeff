//! result (split out of `net.rs`).

use super::*;

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
