//! DHCP lease state machine (RFC 2131 section 4.4) with an injected clock.
//!
//! The kernel owns the NIC and the clock; this module owns every decision of the
//! client life cycle and does no I/O at all. The driver feeds it two things, the
//! current time ([`Lease::poll`]) and parsed server replies
//! ([`Lease::on_reply`]), and gets back at most one [`Action`] per call: a frame
//! to send, or a change of interface configuration to apply.
//!
//! ```text
//!            +------------------------------ NAK / expiry --------------------+
//!            v                                                                 |
//!   Init --DISCOVER--> Selecting --OFFER--> Requesting --ACK--> Bound --T1--> Renewing
//!    ^                     ^  (retransmit,       |  (retransmit,                |  (unicast RENEW,
//!    |                     +--  backoff) --------+   backoff)                   |   retransmit)
//!    |                                          NAK/too many tries              T2
//!    +------------------------------------------------------------------ Rebinding
//!                                                                  (broadcast REBIND, retransmit)
//! ```
//!
//! * T1 is 50% and T2 87.5% of the lease, counted from the moment the REQUEST
//!   that was finally ACKed was *first* sent (conservative: the server's clock
//!   started no later than our ACK).
//! * At T1 a unicast REQUEST (RENEW) goes to the server that granted the lease,
//!   retransmitted at half the time left until T2 (never sooner than
//!   [`RETRY_FLOOR_MS`], never past T2). At T2 the client broadcasts (REBIND),
//!   with the same rhythm until the lease expires. At expiry the address must no
//!   longer be used ([`Action::Lost`]) and the client starts over with DISCOVER.
//! * An ACK that answers a RENEW/REBIND may carry a *different* configuration
//!   (new address, mask, gateway or DNS servers): that is
//!   [`Action::Reconfigured`], and the caller must reconfigure its interface.
//! * A NAK in any state drops the lease ([`Action::Lost`] if one was held) and
//!   restarts from DISCOVER after [`NAK_DELAY_MS`].
//! * Lost servers: DISCOVER/REQUEST retransmit with exponential backoff
//!   (1, 2, 4 ... 64 s) forever, so a link that comes up late still gets a lease.
//!   REQUEST gives up after [`MAX_REQUEST_TRIES`] and goes back to DISCOVER.
//! * A lease of "infinite" duration (`lease_secs == None`) has no timers.

use crate::net::{
    DHCP_ACK, DHCP_NAK, DHCP_OFFER, DhcpReply, Ipv4, Mac, NetConfig, is_usable_unicast,
};

/// Fraction of the lease at which RENEWING starts: 1/2.
pub const T1_NUM: u64 = 1;
pub const T1_DEN: u64 = 2;
/// Fraction of the lease at which REBINDING starts: 7/8 (87.5%).
pub const T2_NUM: u64 = 7;
pub const T2_DEN: u64 = 8;

/// RFC 2131 4.4.5: the retransmission interval during RENEWING/REBINDING is half
/// the time remaining, "down to a minimum of 60 seconds".
pub const RETRY_FLOOR_MS: u64 = 60_000;
/// First retransmission delay of DISCOVER/REQUEST; doubles up to [`BACKOFF_MAX_MS`].
pub const BACKOFF_BASE_MS: u64 = 1_000;
pub const BACKOFF_MAX_MS: u64 = 64_000;
/// How long to wait before restarting after a NAK or a rejected ACK.
pub const NAK_DELAY_MS: u64 = 1_000;
/// Unanswered REQUESTs (SELECTING) before falling back to DISCOVER.
pub const MAX_REQUEST_TRIES: u32 = 4;

/// Where the client is in the life cycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    /// No lease; the next [`Lease::poll`] at or after the deadline sends DISCOVER.
    Init,
    /// DISCOVER sent, waiting for an OFFER.
    Selecting,
    /// REQUEST sent for an offer, waiting for the ACK.
    Requesting,
    /// Holding a lease, before T1.
    Bound,
    /// Past T1: asking the granting server to extend (unicast).
    Renewing,
    /// Past T2: asking any server to extend (broadcast).
    Rebinding,
}

impl State {
    /// Short label for logs and statistics.
    pub fn name(self) -> &'static str {
        match self {
            State::Init => "init",
            State::Selecting => "selecting",
            State::Requesting => "requesting",
            State::Bound => "bound",
            State::Renewing => "renewing",
            State::Rebinding => "rebinding",
        }
    }
}

/// Which flavor of DHCPREQUEST to put on the wire.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Request {
    /// Answer to an OFFER (broadcast, option 50 + 54).
    Select { ip: Ipv4, server: Ipv4 },
    /// T1: unicast to the granting server, `ciaddr` = our address.
    Renew {
        ip: Ipv4,
        server: Ipv4,
        server_mac: Mac,
    },
    /// T2: broadcast, `ciaddr` = our address.
    Rebind { ip: Ipv4 },
}

/// What the driver must do next.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    SendDiscover {
        xid: u32,
    },
    SendRequest {
        xid: u32,
        kind: Request,
    },
    SendRelease {
        xid: u32,
        ip: Ipv4,
        server: Ipv4,
        server_mac: Mac,
    },
    /// A lease was obtained (first one, or after losing the previous one):
    /// configure the interface with it.
    Bound(NetConfig),
    /// The lease was extended with the same addressing: nothing to reconfigure,
    /// only the lease time moved.
    Renewed(NetConfig),
    /// The server extended the lease but changed the configuration (address,
    /// prefix, gateway or DNS): reconfigure the interface.
    Reconfigured(NetConfig),
    /// The lease is gone (expired or NAK): stop using the address.
    Lost,
}

/// The DHCP client. See the module docs.
#[derive(Clone, Debug)]
pub struct Lease {
    mac: Mac,
    state: State,
    xid: u32,
    xid_counter: u32,
    seed: u32,
    /// Configuration in force (Bound/Renewing/Rebinding).
    cfg: Option<NetConfig>,
    server: Option<Ipv4>,
    server_mac: Mac,
    /// The offer being requested (Requesting).
    offer: Option<(Ipv4, Ipv4)>,
    /// When the current exchange started (first DISCOVER/REQUEST of it); the
    /// lease clock starts here once the ACK arrives.
    txn_start: u64,
    /// Lease start / T1 / T2 / expiry in the injected clock (ms); only
    /// meaningful with a finite lease.
    t0: u64,
    t1: u64,
    t2: u64,
    expiry: u64,
    finite: bool,
    /// Next time something is due in the current state.
    next_at: u64,
    tries: u32,
}

/// `lease_ms` split into the T1/T2/expiry offsets (overflow-free: the lease is
/// at most `u32::MAX` seconds).
fn timers(lease_secs: u32) -> (u64, u64, u64) {
    let ms = u64::from(lease_secs) * 1000;
    (ms * T1_NUM / T1_DEN, ms * T2_NUM / T2_DEN, ms)
}

/// True when two configurations configure the interface identically (the lease
/// time is not part of the addressing).
pub fn same_addressing(a: &NetConfig, b: &NetConfig) -> bool {
    a.ip == b.ip && a.prefix == b.prefix && a.gateway == b.gateway && a.dns == b.dns
}

/// Retransmission delay for the `tries`-th retry of DISCOVER/REQUEST.
pub fn backoff_ms(tries: u32) -> u64 {
    BACKOFF_BASE_MS
        .checked_shl(tries.min(16))
        .map_or(BACKOFF_MAX_MS, |d| d.min(BACKOFF_MAX_MS))
}

impl Lease {
    /// A client for `mac`. `seed` randomizes transaction ids (any value; the
    /// kernel passes the TSC). The client is idle until [`Lease::start`].
    pub fn new(mac: Mac, seed: u32) -> Lease {
        Lease {
            mac,
            state: State::Init,
            xid: 0,
            xid_counter: 0,
            seed,
            cfg: None,
            server: None,
            server_mac: Mac([0xff; 6]),
            offer: None,
            txn_start: 0,
            t0: 0,
            t1: 0,
            t2: 0,
            expiry: 0,
            finite: false,
            next_at: u64::MAX,
            tries: 0,
        }
    }

    /// Begin (or restart) acquisition: the next [`Lease::poll`] sends DISCOVER.
    pub fn start(&mut self, now: u64) {
        self.cfg = None;
        self.offer = None;
        self.state = State::Init;
        self.next_at = now;
        self.tries = 0;
    }

    pub fn state(&self) -> State {
        self.state
    }

    /// The configuration in force, if a lease is held (Bound/Renewing/Rebinding).
    pub fn config(&self) -> Option<NetConfig> {
        self.cfg
    }

    pub fn mac(&self) -> Mac {
        self.mac
    }

    /// Transaction id of the exchange in progress.
    pub fn xid(&self) -> u32 {
        self.xid
    }

    /// Milliseconds of lease left at `now` (`None` for no lease or an infinite
    /// one; `Some(0)` once expired but not yet processed).
    pub fn remaining_ms(&self, now: u64) -> Option<u64> {
        if self.cfg.is_some() && self.finite {
            Some(self.expiry.saturating_sub(now))
        } else {
            None
        }
    }

    /// When the next [`Lease::poll`] has something to do (`None`: only a reply
    /// or `start` can move the machine). The driver sleeps until then.
    pub fn next_deadline(&self) -> Option<u64> {
        match self.state {
            State::Init | State::Selecting | State::Requesting => {
                (self.next_at != u64::MAX).then_some(self.next_at)
            }
            State::Bound => self.finite.then_some(self.t1),
            State::Renewing | State::Rebinding => Some(self.next_at.min(self.expiry)),
        }
    }

    fn fresh_xid(&mut self) -> u32 {
        self.xid_counter = self.xid_counter.wrapping_add(1);
        // A multiplicative mix so consecutive transactions do not look alike.
        let x = (self.seed ^ self.xid_counter.wrapping_mul(0x9E37_79B1)).rotate_left(7);
        self.xid = x | 1; // never zero
        self.xid
    }

    /// Time-driven step: returns the action due at `now`, if any. Call it again
    /// while it returns `Some` (a lost lease is followed by a DISCOVER).
    pub fn poll(&mut self, now: u64) -> Option<Action> {
        match self.state {
            State::Init => {
                if now < self.next_at {
                    return None;
                }
                self.txn_start = now;
                self.tries = 0;
                self.state = State::Selecting;
                self.next_at = now + backoff_ms(0);
                let xid = self.fresh_xid();
                Some(Action::SendDiscover { xid })
            }
            State::Selecting => {
                if now < self.next_at {
                    return None;
                }
                self.tries += 1;
                self.next_at = now + backoff_ms(self.tries);
                Some(Action::SendDiscover { xid: self.xid })
            }
            State::Requesting => {
                if now < self.next_at {
                    return None;
                }
                if self.tries + 1 >= MAX_REQUEST_TRIES {
                    // The server that offered stopped answering: start over.
                    self.offer = None;
                    self.state = State::Init;
                    self.next_at = now;
                    return self.poll(now);
                }
                self.tries += 1;
                self.next_at = now + backoff_ms(self.tries);
                let (ip, server) = self.offer?;
                Some(Action::SendRequest {
                    xid: self.xid,
                    kind: Request::Select { ip, server },
                })
            }
            State::Bound => {
                if !self.finite || now < self.t1 {
                    return None;
                }
                // Wake-up may be late (a long fetch held the thread): go straight
                // to the stage the clock says we are in.
                self.advance_held(now)
            }
            State::Renewing | State::Rebinding => {
                if now >= self.expiry {
                    return Some(self.lose(now, 0));
                }
                if self.state == State::Renewing && now >= self.t2 {
                    return self.advance_held(now);
                }
                if now < self.next_at {
                    return None;
                }
                self.next_at = self.retry_at(now);
                let xid = self.xid;
                Some(Action::SendRequest {
                    xid,
                    kind: self.held_request(),
                })
            }
        }
    }

    /// Enter RENEWING/REBINDING/Lost according to where `now` is relative to
    /// T1/T2/expiry, and emit the first REQUEST of the new stage.
    fn advance_held(&mut self, now: u64) -> Option<Action> {
        if now >= self.expiry {
            return Some(self.lose(now, 0));
        }
        self.txn_start = now;
        self.tries = 0;
        let xid = self.fresh_xid();
        self.state = if now >= self.t2 {
            State::Rebinding
        } else {
            State::Renewing
        };
        self.next_at = self.retry_at(now);
        Some(Action::SendRequest {
            xid,
            kind: self.held_request(),
        })
    }

    /// The REQUEST flavor for the current Renewing/Rebinding stage.
    fn held_request(&self) -> Request {
        let ip = self.cfg.map_or(Ipv4::UNSPECIFIED, |c| c.ip);
        if self.state == State::Renewing
            && let Some(server) = self.server
        {
            Request::Renew {
                ip,
                server,
                server_mac: self.server_mac,
            }
        } else {
            Request::Rebind { ip }
        }
    }

    /// Next retransmission time while Renewing/Rebinding: half of what is left
    /// until the end of the stage (T2 or expiry), at least [`RETRY_FLOOR_MS`] but
    /// never past the end of the stage.
    fn retry_at(&self, now: u64) -> u64 {
        let end = if self.state == State::Renewing {
            self.t2
        } else {
            self.expiry
        };
        let left = end.saturating_sub(now);
        now + (left / 2).max(RETRY_FLOOR_MS).min(left)
    }

    /// Drop the lease: back to INIT, DISCOVER after `delay_ms`.
    fn lose(&mut self, now: u64, delay_ms: u64) -> Action {
        self.cfg = None;
        self.server = None;
        self.offer = None;
        self.state = State::Init;
        self.next_at = now + delay_ms;
        self.tries = 0;
        Action::Lost
    }

    /// Feed a parsed server reply. Replies whose transaction id is not the
    /// current one (late answers to an older exchange, other clients' traffic)
    /// are ignored.
    pub fn on_reply(&mut self, now: u64, r: &DhcpReply) -> Option<Action> {
        if r.xid != self.xid {
            return None;
        }
        match (self.state, r.msg_type) {
            (State::Selecting, DHCP_OFFER) => {
                // A REQUEST cannot be built without the server id; an unusable
                // address will not become a config anyway.
                let server = r.server_id?;
                if !is_usable_unicast(r.your_ip) {
                    return None;
                }
                self.offer = Some((r.your_ip, server));
                self.server_mac = r.eth_src;
                self.state = State::Requesting;
                self.tries = 0;
                self.txn_start = now;
                self.next_at = now + backoff_ms(0);
                Some(Action::SendRequest {
                    xid: self.xid,
                    kind: Request::Select {
                        ip: r.your_ip,
                        server,
                    },
                })
            }
            (State::Requesting, DHCP_ACK) => match NetConfig::from_ack(r) {
                Ok(cfg) => {
                    let server = r.server_id.or(self.offer.map(|o| o.1));
                    self.offer = None;
                    self.bind(cfg, server, r.eth_src);
                    Some(Action::Bound(cfg))
                }
                Err(_) => {
                    // The server offered something unusable: start over.
                    self.offer = None;
                    self.state = State::Init;
                    self.next_at = now + NAK_DELAY_MS;
                    None
                }
            },
            (State::Requesting, DHCP_NAK) => {
                self.offer = None;
                self.state = State::Init;
                self.next_at = now + NAK_DELAY_MS;
                None
            }
            (State::Renewing | State::Rebinding, DHCP_ACK) => {
                let cfg = NetConfig::from_ack(r).ok()?; // junk ACK: keep trying
                let old = self.cfg;
                let server = r.server_id.or(self.server);
                self.bind(cfg, server, r.eth_src);
                Some(match old {
                    Some(o) if same_addressing(&o, &cfg) => Action::Renewed(cfg),
                    _ => Action::Reconfigured(cfg),
                })
            }
            (State::Renewing | State::Rebinding, DHCP_NAK) => Some(self.lose(now, NAK_DELAY_MS)),
            _ => None,
        }
    }

    /// Enter BOUND with `cfg`; the lease clock runs from the start of the
    /// exchange that this ACK ended.
    fn bind(&mut self, cfg: NetConfig, server: Option<Ipv4>, server_mac: Mac) {
        self.cfg = Some(cfg);
        self.server = server;
        self.server_mac = server_mac;
        self.state = State::Bound;
        self.tries = 0;
        match cfg.lease_secs {
            Some(secs) => {
                let (t1, t2, end) = timers(secs);
                self.finite = true;
                self.t0 = self.txn_start;
                self.t1 = self.t0 + t1;
                self.t2 = self.t0 + t2;
                self.expiry = self.t0 + end;
            }
            None => self.finite = false,
        }
        self.next_at = u64::MAX;
    }

    /// Hand the address back (optional courtesy at shutdown): returns the RELEASE
    /// to send and leaves the client idle until [`Lease::start`]. `None` if no
    /// lease with a known server is held.
    pub fn release(&mut self) -> Option<Action> {
        let cfg = self.cfg?;
        let server = self.server?;
        let xid = self.fresh_xid();
        let act = Action::SendRelease {
            xid,
            ip: cfg.ip,
            server,
            server_mac: self.server_mac,
        };
        self.cfg = None;
        self.server = None;
        self.state = State::Init;
        self.next_at = u64::MAX; // idle until `start`
        Some(act)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::DnsServers;

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
}
