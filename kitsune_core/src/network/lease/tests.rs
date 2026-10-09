use super::*;
use crate::network::net::DnsServers;

const MAC: Mac = Mac([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
const SRV_MAC: Mac = Mac([0x52, 0x55, 0x0a, 0x00, 0x02, 0x02]);
const SRV: Ipv4 = Ipv4([10, 0, 2, 2]);
const IP: Ipv4 = Ipv4([10, 0, 2, 15]);
const DNS1: Ipv4 = Ipv4([10, 0, 2, 3]);
const DNS2: Ipv4 = Ipv4([8, 8, 8, 8]);

fn reply(kind: u8, xid: u32, ip: Ipv4, lease: Option<u32>) -> DhcpReply {
    DhcpReply {
        msg_type: kind,
        xid,
        your_ip: ip,
        server_id: Some(SRV),
        subnet: Some(Ipv4([255, 255, 255, 0])),
        router: Some(SRV),
        dns: DnsServers::from_slice(&[DNS1, DNS2]),
        lease_secs: lease,
        eth_src: SRV_MAC,
    }
}

/// Run the happy path to BOUND at time `t`, returning the client and the
/// time DISCOVER was sent (lease clock origin).
fn bound_at(t: u64, lease: Option<u32>) -> Lease {
    let mut c = Lease::new(MAC, 0x1234);
    c.start(t);
    let Some(Action::SendDiscover { xid }) = c.poll(t) else {
        panic!("expected DISCOVER")
    };
    let offer = reply(DHCP_OFFER, xid, IP, lease);
    let Some(Action::SendRequest { xid: x2, kind }) = c.on_reply(t, &offer) else {
        panic!("expected REQUEST")
    };
    assert_eq!(x2, xid);
    assert_eq!(
        kind,
        Request::Select {
            ip: IP,
            server: SRV
        }
    );
    let ack = reply(DHCP_ACK, xid, IP, lease);
    let act = c.on_reply(t + 10, &ack).expect("bound");
    assert!(matches!(act, Action::Bound(_)));
    assert_eq!(c.state(), State::Bound);
    c
}

#[test]
fn idle_until_started_and_discover_is_immediate() {
    let mut c = Lease::new(MAC, 1);
    assert_eq!(c.poll(0), None);
    assert_eq!(c.next_deadline(), None);
    c.start(100);
    assert_eq!(c.poll(99), None);
    assert!(matches!(c.poll(100), Some(Action::SendDiscover { .. })));
    assert_eq!(c.state(), State::Selecting);
    assert_ne!(c.xid(), 0);
}

#[test]
fn happy_path_yields_config_with_all_dns_servers() {
    let c = bound_at(0, Some(3600));
    let cfg = c.config().unwrap();
    assert_eq!(cfg.ip, IP);
    assert_eq!(cfg.dns.as_slice(), &[DNS1, DNS2]);
    assert_eq!(cfg.gateway, Some(SRV));
}

#[test]
fn discover_retransmits_with_exponential_backoff_forever() {
    let mut c = Lease::new(MAC, 1);
    c.start(0);
    let Some(Action::SendDiscover { xid }) = c.poll(0) else {
        panic!()
    };
    let mut t = 0;
    // Gaps 1, 2, 4, 8, 16, 32, 64, 64, 64 s.
    for gap in [1, 2, 4, 8, 16, 32, 64, 64, 64] {
        assert_eq!(c.poll(t + gap * 1000 - 1), None, "too early at gap {gap}");
        t += gap * 1000;
        assert_eq!(c.poll(t), Some(Action::SendDiscover { xid }), "gap {gap}");
    }
    assert_eq!(c.state(), State::Selecting);
    assert_eq!(c.next_deadline(), Some(t + 64_000));
}

#[test]
fn replies_with_a_foreign_xid_or_in_the_wrong_state_are_ignored() {
    let mut c = Lease::new(MAC, 1);
    c.start(0);
    let Some(Action::SendDiscover { xid }) = c.poll(0) else {
        panic!()
    };
    assert_eq!(
        c.on_reply(1, &reply(DHCP_OFFER, xid ^ 2, IP, Some(60))),
        None
    );
    // An ACK while selecting is meaningless.
    assert_eq!(c.on_reply(1, &reply(DHCP_ACK, xid, IP, Some(60))), None);
    assert_eq!(c.state(), State::Selecting);
    // An offer without a server id cannot be requested.
    let mut o = reply(DHCP_OFFER, xid, IP, Some(60));
    o.server_id = None;
    assert_eq!(c.on_reply(1, &o), None);
    // Nor can one for an unusable address.
    let o = reply(DHCP_OFFER, xid, Ipv4([0, 0, 0, 0]), Some(60));
    assert_eq!(c.on_reply(1, &o), None);
    assert_eq!(c.state(), State::Selecting);
}

#[test]
fn unanswered_request_retries_then_goes_back_to_discover() {
    let mut c = Lease::new(MAC, 1);
    c.start(0);
    let Some(Action::SendDiscover { xid }) = c.poll(0) else {
        panic!()
    };
    c.on_reply(10, &reply(DHCP_OFFER, xid, IP, Some(60)));
    assert_eq!(c.state(), State::Requesting);
    let mut t = 10;
    // Tries 1..3 retransmit the REQUEST (1, 2, 4 s)...
    for gap in [1_000, 2_000, 4_000] {
        t += gap;
        assert!(
            matches!(c.poll(t), Some(Action::SendRequest { .. })),
            "retry at {t}"
        );
    }
    // ...the next deadline gives up and starts a new DISCOVER (new xid).
    t += 8_000;
    let Some(Action::SendDiscover { xid: x2 }) = c.poll(t) else {
        panic!("expected DISCOVER")
    };
    assert_ne!(x2, xid);
    assert_eq!(c.state(), State::Selecting);
}

#[test]
fn nak_to_the_request_restarts_after_a_delay() {
    let mut c = Lease::new(MAC, 1);
    c.start(0);
    let Some(Action::SendDiscover { xid }) = c.poll(0) else {
        panic!()
    };
    c.on_reply(10, &reply(DHCP_OFFER, xid, IP, Some(60)));
    assert_eq!(c.on_reply(20, &reply(DHCP_NAK, xid, IP, None)), None);
    assert_eq!(c.state(), State::Init);
    assert_eq!(c.poll(20 + NAK_DELAY_MS - 1), None);
    assert!(matches!(
        c.poll(20 + NAK_DELAY_MS),
        Some(Action::SendDiscover { .. })
    ));
}

#[test]
fn unusable_ack_restarts() {
    let mut c = Lease::new(MAC, 1);
    c.start(0);
    let Some(Action::SendDiscover { xid }) = c.poll(0) else {
        panic!()
    };
    c.on_reply(10, &reply(DHCP_OFFER, xid, IP, Some(60)));
    // Zero-length lease: from_ack refuses it.
    assert_eq!(c.on_reply(20, &reply(DHCP_ACK, xid, IP, Some(0))), None);
    assert_eq!(c.state(), State::Init);
    assert_eq!(c.config(), None);
}

#[test]
fn t1_and_t2_are_half_and_seven_eighths_of_the_lease() {
    // Lease 1000 s, clock origin 0 (DISCOVER at 0, bound at 10 ms).
    let mut c = bound_at(0, Some(1000));
    assert_eq!(c.next_deadline(), Some(500_000)); // T1
    assert_eq!(c.poll(499_999), None);
    assert_eq!(c.state(), State::Bound);
    let Some(Action::SendRequest { xid, kind }) = c.poll(500_000) else {
        panic!("expected RENEW at T1")
    };
    assert_eq!(c.state(), State::Renewing);
    assert_eq!(
        kind,
        Request::Renew {
            ip: IP,
            server: SRV,
            server_mac: SRV_MAC
        }
    );
    // Unanswered: the next attempt is at half of (T2 - now) = 187.5 s later
    // (>= the 60 s floor), i.e. 687.5 s.
    assert_eq!(c.poll(687_499), None);
    assert_eq!(
        c.poll(687_500),
        Some(Action::SendRequest {
            xid,
            kind: Request::Renew {
                ip: IP,
                server: SRV,
                server_mac: SRV_MAC
            }
        })
    );
    // Still before T2 = 875 s: next retry is half of the 187.5 s left, but
    // the 60 s floor wins over 93.75 s? No: 93.75 s > 60 s, so 781.25 s.
    assert_eq!(c.poll(781_249), None);
    assert!(matches!(c.poll(781_250), Some(Action::SendRequest { .. })));
    // Next retry would be 46.9 s later but the floor is 60 s, capped at T2
    // (875 s - 781.25 s = 93.75 s left -> 60 s is allowed): 841.25 s.
    assert!(matches!(c.poll(841_250), Some(Action::SendRequest { .. })));
    // Only 33.75 s remain until T2: the retry lands exactly on T2, where the
    // stage changes to REBIND (broadcast, new xid).
    assert_eq!(c.poll(874_999), None);
    let Some(Action::SendRequest { xid: x2, kind }) = c.poll(875_000) else {
        panic!("expected REBIND at T2")
    };
    assert_ne!(x2, xid);
    assert_eq!(kind, Request::Rebind { ip: IP });
    assert_eq!(c.state(), State::Rebinding);
}

#[test]
fn renew_ack_with_same_addressing_is_renewed_and_restarts_the_clock() {
    let mut c = bound_at(0, Some(1000));
    let Some(Action::SendRequest { xid, .. }) = c.poll(500_000) else {
        panic!()
    };
    let ack = reply(DHCP_ACK, xid, IP, Some(2000));
    let act = c.on_reply(500_020, &ack).unwrap();
    assert!(matches!(act, Action::Renewed(cfg) if cfg.lease_secs == Some(2000)));
    assert_eq!(c.state(), State::Bound);
    // New lease counted from when the RENEW was first sent (t = 500 000).
    assert_eq!(c.next_deadline(), Some(500_000 + 1_000_000));
    assert_eq!(c.remaining_ms(500_020), Some(2_000_000 - 20));
}

#[test]
fn renew_ack_with_a_different_configuration_reconfigures() {
    let mut c = bound_at(0, Some(1000));
    let Some(Action::SendRequest { xid, .. }) = c.poll(500_000) else {
        panic!()
    };
    let mut ack = reply(DHCP_ACK, xid, Ipv4([10, 0, 2, 99]), Some(1000));
    ack.dns = DnsServers::one(Ipv4([1, 1, 1, 1]));
    let act = c.on_reply(500_010, &ack).unwrap();
    let Action::Reconfigured(cfg) = act else {
        panic!("expected Reconfigured, got {act:?}")
    };
    assert_eq!(cfg.ip, Ipv4([10, 0, 2, 99]));
    assert_eq!(cfg.dns.as_slice(), &[Ipv4([1, 1, 1, 1])]);
    assert_eq!(c.config(), Some(cfg));
    assert_eq!(c.state(), State::Bound);
}

#[test]
fn a_different_gateway_or_prefix_alone_also_reconfigures() {
    for tweak in 0..2 {
        let mut c = bound_at(0, Some(1000));
        let Some(Action::SendRequest { xid, .. }) = c.poll(500_000) else {
            panic!()
        };
        let mut ack = reply(DHCP_ACK, xid, IP, Some(1000));
        if tweak == 0 {
            ack.router = Some(Ipv4([10, 0, 2, 1]));
        } else {
            ack.subnet = Some(Ipv4([255, 255, 0, 0]));
        }
        assert!(
            matches!(c.on_reply(500_010, &ack), Some(Action::Reconfigured(_))),
            "tweak {tweak}"
        );
    }
}

#[test]
fn nak_while_renewing_drops_the_lease_and_rediscovers() {
    let mut c = bound_at(0, Some(1000));
    let Some(Action::SendRequest { xid, .. }) = c.poll(500_000) else {
        panic!()
    };
    assert_eq!(
        c.on_reply(500_010, &reply(DHCP_NAK, xid, IP, None)),
        Some(Action::Lost)
    );
    assert_eq!(c.config(), None);
    assert_eq!(c.state(), State::Init);
    assert_eq!(c.poll(500_010 + NAK_DELAY_MS - 1), None);
    assert!(matches!(
        c.poll(500_010 + NAK_DELAY_MS),
        Some(Action::SendDiscover { .. })
    ));
}

#[test]
fn nak_while_rebinding_also_drops_the_lease() {
    let mut c = bound_at(0, Some(1000));
    let _ = c.poll(500_000);
    let Some(Action::SendRequest { xid, .. }) = c.poll(875_000) else {
        panic!()
    };
    assert_eq!(c.state(), State::Rebinding);
    assert_eq!(
        c.on_reply(875_010, &reply(DHCP_NAK, xid, IP, None)),
        Some(Action::Lost)
    );
}

#[test]
fn rebind_ack_from_another_server_is_accepted() {
    let mut c = bound_at(0, Some(1000));
    let _ = c.poll(500_000);
    let Some(Action::SendRequest { xid, .. }) = c.poll(875_000) else {
        panic!()
    };
    let mut ack = reply(DHCP_ACK, xid, IP, Some(1000));
    ack.server_id = Some(Ipv4([10, 0, 2, 4]));
    ack.eth_src = Mac([2, 0, 0, 0, 0, 4]);
    assert!(matches!(
        c.on_reply(875_010, &ack),
        Some(Action::Renewed(_) | Action::Reconfigured(_))
    ));
    assert_eq!(c.state(), State::Bound);
    // The next RENEW goes to the new server.
    let Some(Action::SendRequest { kind, .. }) = c.poll(875_000 + 500_000) else {
        panic!()
    };
    assert_eq!(
        kind,
        Request::Renew {
            ip: IP,
            server: Ipv4([10, 0, 2, 4]),
            server_mac: Mac([2, 0, 0, 0, 0, 4])
        }
    );
}

#[test]
fn lost_server_walks_renew_rebind_expire_discover() {
    let mut c = bound_at(0, Some(1000));
    let _ = c.poll(500_000); // RENEW
    let _ = c.poll(875_000); // REBIND
    assert_eq!(c.state(), State::Rebinding);
    // Retries while rebinding: half of the 125 s left = 62.5 s (>= floor).
    assert_eq!(c.poll(937_499), None);
    assert!(matches!(c.poll(937_500), Some(Action::SendRequest { .. })));
    // 62.5 s left, floor 60 s: next at 997.5 s, then the last 2.5 s to expiry.
    assert!(matches!(c.poll(997_500), Some(Action::SendRequest { .. })));
    assert_eq!(c.poll(999_999), None);
    assert_eq!(c.next_deadline(), Some(1_000_000));
    // Expiry: the address is dropped and DISCOVER follows at once.
    assert_eq!(c.poll(1_000_000), Some(Action::Lost));
    assert_eq!(c.config(), None);
    assert_eq!(c.state(), State::Init);
    assert!(matches!(
        c.poll(1_000_000),
        Some(Action::SendDiscover { .. })
    ));
    assert_eq!(c.state(), State::Selecting);
}

#[test]
fn a_late_wakeup_jumps_to_the_right_stage() {
    // Slept through T1 and T2: straight to REBIND.
    let mut c = bound_at(0, Some(1000));
    let Some(Action::SendRequest { kind, .. }) = c.poll(900_000) else {
        panic!()
    };
    assert_eq!(kind, Request::Rebind { ip: IP });
    // Slept through the whole lease: Lost.
    let mut c = bound_at(0, Some(1000));
    assert_eq!(c.poll(5_000_000), Some(Action::Lost));
    // Slept through T1 only: RENEW.
    let mut c = bound_at(0, Some(1000));
    let Some(Action::SendRequest { kind, .. }) = c.poll(600_000) else {
        panic!()
    };
    assert!(matches!(kind, Request::Renew { .. }));
    // Renewing but past T2 at the next poll: REBIND.
    let Some(Action::SendRequest { kind, .. }) = c.poll(880_000) else {
        panic!()
    };
    assert_eq!(kind, Request::Rebind { ip: IP });
}

#[test]
fn short_lease_20s_matches_the_documented_schedule() {
    // The lease used for the QEMU proof: T1 = 10 s, T2 = 17.5 s.
    let mut c = bound_at(0, Some(20));
    assert_eq!(c.next_deadline(), Some(10_000));
    let Some(Action::SendRequest { kind, .. }) = c.poll(10_000) else {
        panic!()
    };
    assert!(matches!(kind, Request::Renew { .. }));
    // 7.5 s to T2, floor 60 s capped at T2: the retry is the REBIND at 17.5 s.
    assert_eq!(c.next_deadline(), Some(17_500));
    let Some(Action::SendRequest { kind, .. }) = c.poll(17_500) else {
        panic!()
    };
    assert_eq!(kind, Request::Rebind { ip: IP });
    assert_eq!(c.next_deadline(), Some(20_000));
    assert_eq!(c.poll(20_000), Some(Action::Lost));
}

#[test]
fn infinite_lease_has_no_timers() {
    let mut c = bound_at(0, None);
    assert_eq!(c.next_deadline(), None);
    assert_eq!(c.remaining_ms(1), None);
    assert_eq!(c.poll(u64::MAX / 2), None);
    assert_eq!(c.state(), State::Bound);
    // 0xFFFF_FFFF seconds is infinite too (from_ack normalizes it).
    let c = bound_at(0, Some(0xFFFF_FFFF));
    assert_eq!(c.next_deadline(), None);
}

#[test]
fn longest_finite_lease_does_not_overflow() {
    let c = bound_at(1 << 40, Some(0xFFFF_FFFE));
    assert!(c.next_deadline().is_some());
}

#[test]
fn release_hands_the_address_back_and_idles() {
    let mut c = bound_at(0, Some(1000));
    let Some(Action::SendRelease {
        ip,
        server,
        server_mac,
        ..
    }) = c.release()
    else {
        panic!("expected RELEASE")
    };
    assert_eq!((ip, server, server_mac), (IP, SRV, SRV_MAC));
    assert_eq!(c.config(), None);
    assert_eq!(c.poll(u64::MAX - 1), None);
    // Nothing to release now.
    assert_eq!(c.release(), None);
    // `start` brings it back.
    c.start(5);
    assert!(matches!(c.poll(5), Some(Action::SendDiscover { .. })));
}

#[test]
fn release_without_a_lease_is_none() {
    let mut c = Lease::new(MAC, 1);
    assert_eq!(c.release(), None);
    c.start(0);
    let _ = c.poll(0);
    assert_eq!(c.release(), None);
}

#[test]
fn xids_are_nonzero_and_change_per_exchange() {
    let mut seen = std::collections::HashSet::new();
    for seed in [0u32, 1, 0xFFFF_FFFF, 0xDEAD_BEEF] {
        let mut c = Lease::new(MAC, seed);
        for _ in 0..50 {
            let x = c.fresh_xid();
            assert_ne!(x, 0);
            seen.insert((seed, x));
        }
    }
    assert_eq!(seen.len(), 4 * 50, "xid repeated within a client");
}

#[test]
fn backoff_is_monotonic_and_capped() {
    let mut prev = 0;
    for t in 0..40 {
        let b = backoff_ms(t);
        assert!(b >= prev && b <= BACKOFF_MAX_MS);
        prev = b;
    }
    assert_eq!(backoff_ms(0), 1000);
    assert_eq!(backoff_ms(6), 64_000);
    assert_eq!(backoff_ms(u32::MAX), BACKOFF_MAX_MS);
}

#[test]
fn lost_lease_then_new_lease_is_bound_not_reconfigured() {
    let mut c = bound_at(0, Some(20));
    let _ = c.poll(10_000);
    let _ = c.poll(17_500);
    assert_eq!(c.poll(20_000), Some(Action::Lost));
    let Some(Action::SendDiscover { xid }) = c.poll(20_000) else {
        panic!()
    };
    let _ = c.on_reply(
        20_010,
        &reply(DHCP_OFFER, xid, Ipv4([10, 0, 2, 77]), Some(30)),
    );
    let act = c.on_reply(
        20_020,
        &reply(DHCP_ACK, xid, Ipv4([10, 0, 2, 77]), Some(30)),
    );
    assert!(matches!(act, Some(Action::Bound(cfg)) if cfg.ip == Ipv4([10, 0, 2, 77])));
}

/// Whatever sequence of ticks and replies, the machine never panics and
/// keeps its invariants (config only while holding a lease).
#[test]
fn random_walk_keeps_invariants() {
    let mut s = 0x2545_F491_4F6C_DD1Du64;
    let mut next = move || {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        s
    };
    let mut c = Lease::new(MAC, 7);
    c.start(0);
    let mut now = 0u64;
    for _ in 0..20_000 {
        now += next() % 20_000;
        match next() % 4 {
            0 | 1 => while c.poll(now).is_some() {},
            _ => {
                let kind = [DHCP_OFFER, DHCP_ACK, DHCP_NAK, 9][(next() % 4) as usize];
                let xid = if next() % 3 == 0 {
                    c.xid() ^ 1
                } else {
                    c.xid()
                };
                let lease = [None, Some(0), Some(30), Some(3600)][(next() % 4) as usize];
                let ip = Ipv4([10, 0, 2, (next() % 256) as u8]);
                let _ = c.on_reply(now, &reply(kind, xid, ip, lease));
            }
        }
        let held = matches!(c.state(), State::Bound | State::Renewing | State::Rebinding);
        assert_eq!(c.config().is_some(), held, "state {:?}", c.state());
        if let (Some(d), true) = (c.next_deadline(), held) {
            let _ = d;
        }
    }
}
