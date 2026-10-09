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
