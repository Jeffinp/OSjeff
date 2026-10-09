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

use crate::network::net::{
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
mod tests;
