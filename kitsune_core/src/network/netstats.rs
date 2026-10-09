//! Per-interface network statistics, readable from any thread.
//!
//! The network owner thread (and the NIC wrapper it uses) updates the counters
//! with relaxed atomics; anyone (the future resource monitor, the `perf-trace`
//! report) takes a [`Snapshot`], a plain copy with the lease and resolver state
//! next to the packet counters. There is one interface today, so the kernel
//! keeps one `static NetStats`.
//!
//! Counter meaning (Linux `ip -s link` terms):
//! * `tx_errors`: the driver failed to send (no free descriptor, device hung).
//! * `tx_dropped`: a frame was refused before the device (longer than the MTU).
//! * `rx_errors`: a frame the device delivered but the driver found malformed
//!   (runt, bad ring header).
//! * `rx_dropped`: a good frame lost for lack of room (longer than the caller's
//!   buffer, or the device ring was full).

use crate::network::net::{DnsServers, Ipv4, NetConfig};
use core::fmt;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering::Relaxed};

/// Which driver is behind the interface.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NicKind {
    None,
    Ne2000,
    VirtioNet,
}

impl NicKind {
    pub fn name(self) -> &'static str {
        match self {
            NicKind::None => "none",
            NicKind::Ne2000 => "ne2000",
            NicKind::VirtioNet => "virtio-net",
        }
    }

    fn code(self) -> u8 {
        match self {
            NicKind::None => 0,
            NicKind::Ne2000 => 1,
            NicKind::VirtioNet => 2,
        }
    }

    fn from_code(c: u8) -> NicKind {
        match c {
            1 => NicKind::Ne2000,
            2 => NicKind::VirtioNet,
            _ => NicKind::None,
        }
    }
}

/// `lease_secs` value meaning "never expires".
const INFINITE: u32 = u32::MAX;
/// `lease_end_ms` value meaning "no finite lease".
const NO_END: u64 = u64::MAX;

/// The live counters.
pub struct NetStats {
    tx_packets: AtomicU64,
    tx_bytes: AtomicU64,
    tx_errors: AtomicU64,
    tx_dropped: AtomicU64,
    rx_packets: AtomicU64,
    rx_bytes: AtomicU64,
    rx_errors: AtomicU64,
    rx_dropped: AtomicU64,
    link_up: AtomicBool,
    kind: AtomicU8,

    cfg_valid: AtomicBool,
    ip: AtomicU32,
    prefix: AtomicU8,
    gateway: AtomicU32,
    dns: [AtomicU32; crate::network::net::MAX_DNS],
    lease_secs: AtomicU32,
    lease_end_ms: AtomicU64,
    dhcp_state: AtomicU8,

    dhcp_renewals: AtomicU64,
    dhcp_rebinds: AtomicU64,
    dhcp_lost: AtomicU64,
    dns_queries: AtomicU64,
    dns_cache_hits: AtomicU64,
    dns_failovers: AtomicU64,
    dns_failures: AtomicU64,
    pings_sent: AtomicU64,
    pings_ok: AtomicU64,
}

impl NetStats {
    pub const fn new() -> NetStats {
        NetStats {
            tx_packets: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            tx_errors: AtomicU64::new(0),
            tx_dropped: AtomicU64::new(0),
            rx_packets: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            rx_errors: AtomicU64::new(0),
            rx_dropped: AtomicU64::new(0),
            link_up: AtomicBool::new(false),
            kind: AtomicU8::new(0),
            cfg_valid: AtomicBool::new(false),
            ip: AtomicU32::new(0),
            prefix: AtomicU8::new(0),
            gateway: AtomicU32::new(0),
            dns: [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)],
            lease_secs: AtomicU32::new(INFINITE),
            lease_end_ms: AtomicU64::new(NO_END),
            dhcp_state: AtomicU8::new(0),
            dhcp_renewals: AtomicU64::new(0),
            dhcp_rebinds: AtomicU64::new(0),
            dhcp_lost: AtomicU64::new(0),
            dns_queries: AtomicU64::new(0),
            dns_cache_hits: AtomicU64::new(0),
            dns_failovers: AtomicU64::new(0),
            dns_failures: AtomicU64::new(0),
            pings_sent: AtomicU64::new(0),
            pings_ok: AtomicU64::new(0),
        }
    }

    // ---- packet counters ----

    pub fn on_tx(&self, len: usize) {
        self.tx_packets.fetch_add(1, Relaxed);
        self.tx_bytes.fetch_add(len as u64, Relaxed);
    }
    pub fn on_tx_error(&self) {
        self.tx_errors.fetch_add(1, Relaxed);
    }
    pub fn on_tx_dropped(&self) {
        self.tx_dropped.fetch_add(1, Relaxed);
    }
    pub fn on_rx(&self, len: usize) {
        self.rx_packets.fetch_add(1, Relaxed);
        self.rx_bytes.fetch_add(len as u64, Relaxed);
    }
    pub fn on_rx_error(&self) {
        self.rx_errors.fetch_add(1, Relaxed);
    }
    pub fn on_rx_dropped(&self) {
        self.rx_dropped.fetch_add(1, Relaxed);
    }

    pub fn set_link(&self, up: bool) {
        self.link_up.store(up, Relaxed);
    }

    pub fn set_nic(&self, kind: NicKind) {
        self.kind.store(kind.code(), Relaxed);
    }

    // ---- protocol counters ----

    pub fn on_dhcp_renewal(&self) {
        self.dhcp_renewals.fetch_add(1, Relaxed);
    }
    pub fn on_dhcp_rebind(&self) {
        self.dhcp_rebinds.fetch_add(1, Relaxed);
    }
    pub fn on_dhcp_lost(&self) {
        self.dhcp_lost.fetch_add(1, Relaxed);
    }
    pub fn on_dns_query(&self) {
        self.dns_queries.fetch_add(1, Relaxed);
    }
    pub fn on_dns_cache_hit(&self) {
        self.dns_cache_hits.fetch_add(1, Relaxed);
    }
    pub fn on_dns_failover(&self) {
        self.dns_failovers.fetch_add(1, Relaxed);
    }
    pub fn on_dns_failure(&self) {
        self.dns_failures.fetch_add(1, Relaxed);
    }
    pub fn on_ping_sent(&self) {
        self.pings_sent.fetch_add(1, Relaxed);
    }
    pub fn on_ping_ok(&self) {
        self.pings_ok.fetch_add(1, Relaxed);
    }

    // ---- configuration (lease / resolver) ----

    /// Publish the interface configuration in force (`None`: no address).
    /// `now_ms` is the clock `lease_remaining_ms` is later computed against.
    pub fn set_config(&self, cfg: Option<&NetConfig>, now_ms: u64) {
        match cfg {
            None => self.cfg_valid.store(false, Relaxed),
            Some(c) => {
                let word = |a: Ipv4| u32::from_be_bytes(a.0);
                self.ip.store(word(c.ip), Relaxed);
                self.prefix.store(c.prefix, Relaxed);
                self.gateway.store(c.gateway.map_or(0, word), Relaxed);
                for (i, slot) in self.dns.iter().enumerate() {
                    slot.store(c.dns.as_slice().get(i).map_or(0, |&a| word(a)), Relaxed);
                }
                self.lease_secs
                    .store(c.lease_secs.unwrap_or(INFINITE), Relaxed);
                self.lease_end_ms.store(
                    c.lease_secs
                        .map_or(NO_END, |s| now_ms + u64::from(s) * 1000),
                    Relaxed,
                );
                self.cfg_valid.store(true, Relaxed);
            }
        }
    }

    /// Move the published lease end (a renewal that kept the addressing).
    pub fn set_lease_end(&self, end_ms: Option<u64>) {
        self.lease_end_ms.store(end_ms.unwrap_or(NO_END), Relaxed);
    }

    /// Publish the DHCP client state (see [`crate::network::lease::State::name`]).
    pub fn set_dhcp_state(&self, s: crate::network::lease::State) {
        self.dhcp_state.store(state_code(s), Relaxed);
    }

    /// A plain copy of everything, with the remaining lease time computed
    /// against `now_ms`.
    pub fn snapshot(&self, now_ms: u64) -> Snapshot {
        let config = self.cfg_valid.load(Relaxed).then(|| {
            let addr = |a: &AtomicU32| Ipv4(a.load(Relaxed).to_be_bytes());
            let gw = self.gateway.load(Relaxed);
            let dns: [Ipv4; crate::network::net::MAX_DNS] =
                [addr(&self.dns[0]), addr(&self.dns[1]), addr(&self.dns[2])];
            let secs = self.lease_secs.load(Relaxed);
            NetConfig {
                ip: addr(&self.ip),
                prefix: self.prefix.load(Relaxed),
                gateway: (gw != 0).then(|| Ipv4(gw.to_be_bytes())),
                // Unset slots are 0.0.0.0, which `from_slice` drops.
                dns: DnsServers::from_slice(&dns),
                lease_secs: (secs != INFINITE).then_some(secs),
            }
        });
        let end = self.lease_end_ms.load(Relaxed);
        Snapshot {
            nic: NicKind::from_code(self.kind.load(Relaxed)),
            link_up: self.link_up.load(Relaxed),
            tx_packets: self.tx_packets.load(Relaxed),
            tx_bytes: self.tx_bytes.load(Relaxed),
            tx_errors: self.tx_errors.load(Relaxed),
            tx_dropped: self.tx_dropped.load(Relaxed),
            rx_packets: self.rx_packets.load(Relaxed),
            rx_bytes: self.rx_bytes.load(Relaxed),
            rx_errors: self.rx_errors.load(Relaxed),
            rx_dropped: self.rx_dropped.load(Relaxed),
            config,
            lease_remaining_ms: (config.is_some() && end != NO_END)
                .then(|| end.saturating_sub(now_ms)),
            dhcp_state: state_name(self.dhcp_state.load(Relaxed)),
            dhcp_renewals: self.dhcp_renewals.load(Relaxed),
            dhcp_rebinds: self.dhcp_rebinds.load(Relaxed),
            dhcp_lost: self.dhcp_lost.load(Relaxed),
            dns_queries: self.dns_queries.load(Relaxed),
            dns_cache_hits: self.dns_cache_hits.load(Relaxed),
            dns_failovers: self.dns_failovers.load(Relaxed),
            dns_failures: self.dns_failures.load(Relaxed),
            pings_sent: self.pings_sent.load(Relaxed),
            pings_ok: self.pings_ok.load(Relaxed),
        }
    }
}

impl Default for NetStats {
    fn default() -> Self {
        NetStats::new()
    }
}

fn state_code(s: crate::network::lease::State) -> u8 {
    use crate::network::lease::State::*;
    match s {
        Init => 0,
        Selecting => 1,
        Requesting => 2,
        Bound => 3,
        Renewing => 4,
        Rebinding => 5,
    }
}

fn state_name(code: u8) -> &'static str {
    use crate::network::lease::State::*;
    match code {
        1 => Selecting.name(),
        2 => Requesting.name(),
        3 => Bound.name(),
        4 => Renewing.name(),
        5 => Rebinding.name(),
        _ => Init.name(),
    }
}

/// A consistent-enough copy of [`NetStats`] (each field is read once; the set is
/// not a transaction).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snapshot {
    pub nic: NicKind,
    pub link_up: bool,
    pub tx_packets: u64,
    pub tx_bytes: u64,
    pub tx_errors: u64,
    pub tx_dropped: u64,
    pub rx_packets: u64,
    pub rx_bytes: u64,
    pub rx_errors: u64,
    pub rx_dropped: u64,
    /// Interface configuration in force; `None` when no address is held.
    pub config: Option<NetConfig>,
    /// Lease time left; `None` for no lease or an infinite one.
    pub lease_remaining_ms: Option<u64>,
    pub dhcp_state: &'static str,
    pub dhcp_renewals: u64,
    pub dhcp_rebinds: u64,
    pub dhcp_lost: u64,
    pub dns_queries: u64,
    pub dns_cache_hits: u64,
    pub dns_failovers: u64,
    pub dns_failures: u64,
    pub pings_sent: u64,
    pub pings_ok: u64,
}

/// One log line, e.g.
/// `net: virtio-net link=up tx=12/1204B err=0 drop=0 rx=15/3010B err=0 drop=0 \
///  | 10.0.2.15/24 gw 10.0.2.2 dns 10.0.2.3 lease=86380s dhcp=bound renew=0 \
///  rebind=0 lost=0 | dns q=3 hit=1 failover=0 fail=0 | ping 1/1`
impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "net: {} link={} tx={}/{}B err={} drop={} rx={}/{}B err={} drop={} | ",
            self.nic.name(),
            if self.link_up { "up" } else { "down" },
            self.tx_packets,
            self.tx_bytes,
            self.tx_errors,
            self.tx_dropped,
            self.rx_packets,
            self.rx_bytes,
            self.rx_errors,
            self.rx_dropped,
        )?;
        match &self.config {
            Some(c) => write!(f, "{c}")?,
            None => f.write_str("no address")?,
        }
        match (&self.config, self.lease_remaining_ms) {
            (Some(_), Some(ms)) => write!(f, " lease={}s", ms / 1000)?,
            (Some(_), None) => f.write_str(" lease=infinite")?,
            (None, _) => {}
        }
        write!(
            f,
            " dhcp={} renew={} rebind={} lost={} | dns q={} hit={} failover={} fail={} | ping {}/{}",
            self.dhcp_state,
            self.dhcp_renewals,
            self.dhcp_rebinds,
            self.dhcp_lost,
            self.dns_queries,
            self.dns_cache_hits,
            self.dns_failovers,
            self.dns_failures,
            self.pings_ok,
            self.pings_sent,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::lease::State;

    fn cfg() -> NetConfig {
        NetConfig {
            ip: Ipv4([192, 168, 77, 15]),
            prefix: 24,
            gateway: Some(Ipv4([192, 168, 77, 2])),
            dns: DnsServers::from_slice(&[Ipv4([192, 168, 77, 3]), Ipv4([8, 8, 8, 8])]),
            lease_secs: Some(600),
        }
    }

    #[test]
    fn fresh_stats_are_zero_and_unconfigured() {
        let s = NetStats::new().snapshot(0);
        assert_eq!(
            (s.tx_packets, s.rx_packets, s.tx_bytes, s.rx_bytes),
            (0, 0, 0, 0)
        );
        assert_eq!(s.nic, NicKind::None);
        assert!(!s.link_up);
        assert_eq!(s.config, None);
        assert_eq!(s.lease_remaining_ms, None);
        assert_eq!(s.dhcp_state, "init");
    }

    #[test]
    fn packet_counters_accumulate() {
        let n = NetStats::new();
        n.on_tx(60);
        n.on_tx(1514);
        n.on_rx(100);
        n.on_tx_error();
        n.on_tx_dropped();
        n.on_tx_dropped();
        n.on_rx_error();
        n.on_rx_dropped();
        n.on_rx_dropped();
        n.on_rx_dropped();
        let s = n.snapshot(0);
        assert_eq!((s.tx_packets, s.tx_bytes), (2, 1574));
        assert_eq!((s.rx_packets, s.rx_bytes), (1, 100));
        assert_eq!((s.tx_errors, s.tx_dropped), (1, 2));
        assert_eq!((s.rx_errors, s.rx_dropped), (1, 3));
    }

    #[test]
    fn config_round_trips_through_the_atomics() {
        let n = NetStats::new();
        n.set_config(Some(&cfg()), 1_000);
        let s = n.snapshot(1_000);
        assert_eq!(s.config, Some(cfg()));
        assert_eq!(s.lease_remaining_ms, Some(600_000));
        assert_eq!(n.snapshot(301_000).lease_remaining_ms, Some(300_000));
        // Past the end it saturates at zero.
        assert_eq!(n.snapshot(10_000_000).lease_remaining_ms, Some(0));
    }

    #[test]
    fn optional_parts_survive() {
        let n = NetStats::new();
        let c = NetConfig {
            gateway: None,
            dns: DnsServers::NONE,
            lease_secs: None,
            ..cfg()
        };
        n.set_config(Some(&c), 0);
        let s = n.snapshot(5);
        assert_eq!(s.config, Some(c));
        assert_eq!(s.lease_remaining_ms, None, "infinite lease");
        n.set_config(None, 6);
        assert_eq!(n.snapshot(6).config, None);
        assert_eq!(n.snapshot(6).lease_remaining_ms, None);
    }

    #[test]
    fn lease_end_can_be_moved_by_a_renewal() {
        let n = NetStats::new();
        n.set_config(Some(&cfg()), 0);
        n.set_lease_end(Some(900_000));
        assert_eq!(n.snapshot(100_000).lease_remaining_ms, Some(800_000));
        n.set_lease_end(None);
        assert_eq!(n.snapshot(100_000).lease_remaining_ms, None);
    }

    #[test]
    fn dhcp_state_names() {
        let n = NetStats::new();
        for (st, name) in [
            (State::Init, "init"),
            (State::Selecting, "selecting"),
            (State::Requesting, "requesting"),
            (State::Bound, "bound"),
            (State::Renewing, "renewing"),
            (State::Rebinding, "rebinding"),
        ] {
            n.set_dhcp_state(st);
            assert_eq!(n.snapshot(0).dhcp_state, name);
        }
    }

    #[test]
    fn nic_kind_and_link() {
        let n = NetStats::new();
        n.set_nic(NicKind::VirtioNet);
        n.set_link(true);
        let s = n.snapshot(0);
        assert_eq!(s.nic.name(), "virtio-net");
        assert!(s.link_up);
        n.set_nic(NicKind::Ne2000);
        assert_eq!(n.snapshot(0).nic.name(), "ne2000");
    }

    #[test]
    fn protocol_counters() {
        let n = NetStats::new();
        n.on_dhcp_renewal();
        n.on_dhcp_rebind();
        n.on_dhcp_lost();
        n.on_dns_query();
        n.on_dns_query();
        n.on_dns_cache_hit();
        n.on_dns_failover();
        n.on_dns_failure();
        n.on_ping_sent();
        n.on_ping_sent();
        n.on_ping_ok();
        let s = n.snapshot(0);
        assert_eq!((s.dhcp_renewals, s.dhcp_rebinds, s.dhcp_lost), (1, 1, 1));
        assert_eq!(
            (
                s.dns_queries,
                s.dns_cache_hits,
                s.dns_failovers,
                s.dns_failures
            ),
            (2, 1, 1, 1)
        );
        assert_eq!((s.pings_sent, s.pings_ok), (2, 1));
    }

    #[test]
    fn log_line_is_complete() {
        let n = NetStats::new();
        n.set_nic(NicKind::VirtioNet);
        n.set_link(true);
        n.on_tx(100);
        n.on_rx(200);
        n.set_config(Some(&cfg()), 0);
        n.set_dhcp_state(State::Bound);
        n.on_ping_sent();
        n.on_ping_ok();
        let line = format!("{}", n.snapshot(20_000));
        assert_eq!(
            line,
            "net: virtio-net link=up tx=1/100B err=0 drop=0 rx=1/200B err=0 drop=0 | \
             192.168.77.15/24 gw 192.168.77.2 dns 192.168.77.3,8.8.8.8 lease=580s \
             dhcp=bound renew=0 rebind=0 lost=0 | dns q=0 hit=0 failover=0 fail=0 | ping 1/1"
        );
        let none = NetStats::new();
        let line = format!("{}", none.snapshot(0));
        assert!(line.contains("| no address dhcp=init"), "{line}");
        assert!(!line.contains("lease="), "{line}");
    }
}
