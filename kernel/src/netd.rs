//! `netd`: the network owner. One value ([`Netd`]) holds the TCP/IP stack (and
//! through it the only handle on the NIC), the DHCP client and the ping client, and
//! it runs on the `fetcher` thread. There is no second party to race with: the
//! compositor never touches the hardware, it only posts requests through atomics
//! (`fetch::try_post` for pages, [`ping_start`] for echo requests) and reads
//! statistics ([`stats`]).
//!
//! # Who does what, and when
//!
//! The fetcher thread alternates between two jobs:
//!
//! * **A fetch** (it was woken by `fetch::try_post`): it drives smoltcp until the
//!   page is done. The NIC belongs to smoltcp for that time; incoming ARP and echo
//!   requests are answered by smoltcp itself, and the DHCP timers simply wait (a
//!   fetch is bounded: the longest is tens of seconds, the shortest lease timer is
//!   T1 = lease / 2).
//! * **Service** ([`Netd::service`], every [`IDLE_POLL_TICKS`] ticks of 4 ms while
//!   idle, every tick while a ping is running): drain the receive ring, then
//!   dispatch each frame (DHCP replies go to the lease machine, ARP replies and ICMP
//!   to the ping client, ARP/echo requests to the responder), fire the lease timers
//!   (T1 RENEW, T2 REBIND, expiry) and advance the ping.
//!
//! Boot is the same code run to completion before the thread exists
//! ([`Netd::boot`]): the first DISCOVER/REQUEST exchange (bounded by
//! [`BOOT_BUDGET_MS`]), then the static fallback if nothing answered. The lease
//! machine keeps trying in the background after a fallback, so a DHCP server that
//! appears later still configures the interface.
//!
//! # Lease changes
//!
//! `Bound` and `Reconfigured` replace the interface address, route and DNS servers
//! (`Net::reconfigure`) and announce the address with a gratuitous ARP. `Renewed`
//! only moves the lease clock. `Lost` (expiry or NAK) removes the address: the stack
//! then refuses connections instead of using an address that is no longer ours.
//!
//! # Timing
//!
//! There is no NIC interrupt: a received frame waits for the next service wake
//! (at most one 4 ms tick while a ping runs, [`IDLE_POLL_TICKS`] otherwise), so a
//! measured round trip includes up to one tick of that latency.

use crate::netstack::Net;
use crate::nic::{Port, STATS};
use crate::{interrupts, io, serial_println};
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use kitsune_core::icmp::{self, Echo, Ping, PingError};
use kitsune_core::lease::{Action, Lease, Request};
use kitsune_core::net::{self, DHCP_BUILD_MIN, DhcpReply, Ipv4, Mac, NetConfig};

/// While idle the service runs this often (ticks of 4 ms).
pub const IDLE_POLL_TICKS: u64 = 4;
/// Frames dispatched per service wake: a flood must not starve the rest of the system.
const RX_BUDGET: usize = 32;
/// Longest the boot waits for a DHCP lease before using the static fallback.
const BOOT_BUDGET_MS: u64 = 2000;
/// ARP resolution for a ping: retries and the wait for each.
const ARP_TRIES: u32 = 3;
const ARP_WAIT_MS: u64 = 300;
/// Next hops remembered for ping.
const ARP_SLOTS: usize = 4;
/// How long a learned next-hop MAC is trusted.
const ARP_TTL_MS: u64 = 60_000;
/// Echo payload size (the classic 56 bytes).
const PING_PAYLOAD: usize = 56;

// ---- clocks ----

static TSC_KHZ: AtomicU64 = AtomicU64::new(0);

/// Tell netd the calibrated TSC rate (for microsecond timestamps).
pub fn set_tsc_khz(khz: u64) {
    TSC_KHZ.store(khz, Ordering::Relaxed);
}

/// Monotonic milliseconds (timer ticks).
pub fn now_ms() -> u64 {
    interrupts::ticks() * 1000 / u64::from(interrupts::TIMER_HZ)
}

/// Monotonic microseconds: the TSC when calibrated, else the timer tick.
pub fn now_us() -> u64 {
    let khz = TSC_KHZ.load(Ordering::Relaxed);
    if khz == 0 {
        interrupts::ticks() * 1_000_000 / u64::from(interrupts::TIMER_HZ)
    } else {
        (u128::from(io::rdtsc()) * 1000 / u128::from(khz)) as u64
    }
}

// ---- ping mailbox (any thread -> netd) ----

const P_IDLE: u8 = 0;
const P_REQUESTED: u8 = 1;
const P_RUNNING: u8 = 2;
const P_DONE: u8 = 3;
/// A requester is filling in the parameters.
const P_CLAIMED: u8 = 4;

static P_STATE: AtomicU8 = AtomicU8::new(P_IDLE);
static P_TARGET: AtomicU32 = AtomicU32::new(0);
static P_TIMEOUT_MS: AtomicU32 = AtomicU32::new(0);
static P_RESULT: AtomicU64 = AtomicU64::new(0);
/// netd exists (a NIC was found and the fetcher owns the stack).
static UP: AtomicBool = AtomicBool::new(false);

fn encode_result(r: Result<u64, PingError>) -> u64 {
    // bits 0..=31 rtt_us (saturated) | bits 32..=39 error kind | bits 40..=47 error arg
    match r {
        Ok(us) => us.min(u64::from(u32::MAX)),
        Err(e) => {
            let (kind, arg) = match e {
                PingError::Timeout => (1u64, 0u64),
                PingError::Unreachable(c) => (2, u64::from(c)),
                PingError::TimeExceeded => (3, 0),
                PingError::NoRoute => (4, 0),
                PingError::ArpFailed => (5, 0),
                PingError::BadTarget => (6, 0),
                PingError::NoNetwork => (7, 0),
                PingError::Busy => (8, 0),
            };
            kind << 32 | arg << 40
        }
    }
}

fn decode_result(v: u64) -> Result<u64, PingError> {
    let arg = (v >> 40) as u8;
    match (v >> 32) as u8 {
        0 => Ok(v & u64::from(u32::MAX)),
        1 => Err(PingError::Timeout),
        2 => Err(PingError::Unreachable(arg)),
        3 => Err(PingError::TimeExceeded),
        4 => Err(PingError::NoRoute),
        5 => Err(PingError::ArpFailed),
        6 => Err(PingError::BadTarget),
        8 => Err(PingError::Busy),
        _ => Err(PingError::NoNetwork),
    }
}

/// Ask netd to ping `target` (non-blocking): the answer is collected with
/// [`ping_poll`]. `Err(Busy)` while a page is loading or another ping is
/// running, `Err(NoNetwork)` without a NIC. `timeout_ms` bounds the wait for the
/// echo reply (the ARP resolution of the next hop comes on top, at most ~1 s).
#[allow(dead_code)] // public API for the terminal and other callers (not wired to a command yet)
pub fn ping_start(target: Ipv4, timeout_ms: u32) -> Result<(), PingError> {
    if !UP.load(Ordering::Acquire) || crate::fetch::worker_dead() {
        return Err(PingError::NoNetwork);
    }
    if !crate::fetch::is_idle() {
        return Err(PingError::Busy);
    }
    // Claim the mailbox: from idle, or over a finished ping nobody collected.
    let claimed = P_STATE
        .compare_exchange(P_IDLE, P_CLAIMED, Ordering::AcqRel, Ordering::Acquire)
        .or_else(|_| {
            P_STATE.compare_exchange(P_DONE, P_CLAIMED, Ordering::AcqRel, Ordering::Acquire)
        });
    if claimed.is_err() {
        return Err(PingError::Busy);
    }
    P_TARGET.store(u32::from_be_bytes(target.0), Ordering::Relaxed);
    P_TIMEOUT_MS.store(timeout_ms.clamp(10, 60_000), Ordering::Relaxed);
    P_STATE.store(P_REQUESTED, Ordering::Release);
    crate::fetch::wake_worker();
    Ok(())
}

/// The finished ping's round trip in microseconds, or why it failed; `None` while it
/// has not finished (or none was started).
#[allow(dead_code)] // public API for the terminal and other callers (not wired to a command yet)
pub fn ping_poll_us() -> Option<Result<u64, PingError>> {
    if P_STATE
        .compare_exchange(P_DONE, P_IDLE, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return None;
    }
    Some(decode_result(P_RESULT.load(Ordering::Acquire)))
}

/// Like [`ping_poll_us`] with the round trip in whole milliseconds (at least 1 for a
/// reply that arrived).
#[allow(dead_code)] // public API for the terminal and other callers (not wired to a command yet)
pub fn ping_poll() -> Option<Result<u32, PingError>> {
    ping_poll_us().map(|r| r.map(icmp::rtt_ms))
}

/// Ping `target` and wait for the answer: the round trip in milliseconds. The
/// caller sleeps (halted or yielding, never spinning) until the answer or the
/// timeout. Fine for a worker thread; from the compositor it would freeze the UI
/// for up to `timeout_ms` (but not deadlock), so the terminal uses [`ping_start`] +
/// [`ping_poll`] instead.
#[allow(dead_code)] // public API for the terminal and other callers (not wired to a command yet)
pub fn ping(target: Ipv4, timeout_ms: u32) -> Result<u32, PingError> {
    ping_us(target, timeout_ms).map(icmp::rtt_ms)
}

/// [`ping`] returning the round trip in microseconds.
#[allow(dead_code)] // public API for the terminal and other callers (not wired to a command yet)
pub fn ping_us(target: Ipv4, timeout_ms: u32) -> Result<u64, PingError> {
    ping_start(target, timeout_ms)?;
    // ARP (<= ~1 s) + echo timeout + one service period of slack.
    let limit = interrupts::ticks()
        + (u64::from(timeout_ms) + 1500) * u64::from(interrupts::TIMER_HZ) / 1000
        + IDLE_POLL_TICKS;
    loop {
        if let Some(r) = ping_poll_us() {
            return r;
        }
        if interrupts::ticks() >= limit {
            return Err(PingError::Timeout);
        }
        // `idle` halts until the next interrupt (or yields to a runnable thread) and, unlike
        // `block`, never parks the caller: it is safe from any thread, the compositor included
        // (the scheduler must always have a runnable thread, and the compositor is it).
        crate::sched::idle(|| P_STATE.load(Ordering::Acquire) == P_DONE);
    }
}

/// A ping request is waiting for netd (used by the fetcher's block condition).
pub fn ping_pending() -> bool {
    P_STATE.load(Ordering::Acquire) == P_REQUESTED
}

/// A copy of the interface statistics (packets, lease, resolver, ping counters).
pub fn stats() -> kitsune_core::netstats::Snapshot {
    STATS.snapshot(now_ms())
}

/// Print the statistics on the serial port (called from the `perf-trace` report).
pub fn log_stats() {
    if UP.load(Ordering::Relaxed) {
        serial_println!("[trace] {}", stats());
    }
}

// ---- the owner ----

/// What the ping client is doing.
enum PingJob {
    Idle,
    /// Resolving the next hop's MAC.
    Arping {
        target: Ipv4,
        hop: Ipv4,
        timeout_ms: u32,
        tries: u32,
        next_try_ms: u64,
    },
    /// Echo sent, waiting for the reply.
    Waiting(Ping),
}

/// A change of interface configuration decided by the lease machine.
enum Change {
    None,
    Bound(NetConfig),
    Renewed(NetConfig),
    Reconfigured(NetConfig),
    Lost,
}

pub struct Netd {
    net: Net,
    lease: Lease,
    ping: PingJob,
    /// Next hops learned for ping, `(ip, mac, learned_ms)`; the oldest is replaced.
    arp: [Option<(Ipv4, Mac, u64)>; ARP_SLOTS],
    ping_seq: u16,
    ping_id: u16,
}

const _: () = assert!(DHCP_BUILD_MIN <= 600);

/// Send the frame a lease [`Action`] asks for; report the configuration change it
/// carries, if any.
fn execute(port: &mut Port, mac: Mac, action: Action) -> Change {
    let mut buf = [0u8; 600];
    match action {
        Action::SendDiscover { xid } => {
            let n = net::dhcp_discover(&mut buf, mac, xid);
            port.send(&buf[..n]);
            Change::None
        }
        Action::SendRequest { xid, kind } => {
            let n = match kind {
                Request::Select { ip, server } => net::dhcp_request(&mut buf, mac, xid, ip, server),
                Request::Renew {
                    ip,
                    server,
                    server_mac,
                } => {
                    STATS.on_dhcp_renewal();
                    serial_println!("net: DHCP RENEW {} unicast to {} (T1)", ip, server);
                    net::dhcp_request_renew(&mut buf, mac, xid, ip, Some((server_mac, server)))
                }
                Request::Rebind { ip } => {
                    STATS.on_dhcp_rebind();
                    serial_println!("net: DHCP REBIND {} broadcast (T2)", ip);
                    net::dhcp_request_renew(&mut buf, mac, xid, ip, None)
                }
            };
            port.send(&buf[..n]);
            Change::None
        }
        Action::SendRelease {
            xid,
            ip,
            server,
            server_mac,
        } => {
            let n = net::dhcp_release(&mut buf, mac, xid, ip, server_mac, server);
            port.send(&buf[..n]);
            Change::None
        }
        Action::Bound(c) => Change::Bound(c),
        Action::Renewed(c) => Change::Renewed(c),
        Action::Reconfigured(c) => Change::Reconfigured(c),
        Action::Lost => Change::Lost,
    }
}

fn log_lease(prefix: &str, cfg: &NetConfig) {
    serial_println!("net: {} {}", prefix, cfg);
    match cfg.lease_secs {
        Some(s) => serial_println!("net: lease time {} s", s),
        None => serial_println!("net: lease time infinite"),
    }
}

impl Netd {
    /// Bring the interface up: run the DHCP exchange (bounded by
    /// [`BOOT_BUDGET_MS`]), fall back to the static configuration if nobody answers,
    /// announce the address with a gratuitous ARP and build the stack around `port`.
    /// The DHCP client keeps running afterwards (see the module docs).
    pub fn boot(mut port: Port) -> Netd {
        let mac = port.mac();
        let seed = crate::rng::u32();
        let mut lease = Lease::new(mac, seed);
        let t0 = now_ms();
        lease.start(t0);
        STATS.set_dhcp_state(lease.state());

        let mut rx = [0u8; 1600];
        let mut leased = None;
        'wait: while leased.is_none() && now_ms() < t0 + BOOT_BUDGET_MS {
            let now = now_ms();
            for _ in 0..4 {
                let Some(a) = lease.poll(now) else { break };
                let _ = execute(&mut port, mac, a);
            }
            while let Some(n) = port.poll(&mut rx) {
                if let Some(r) = net::parse_dhcp(&rx[..n], mac)
                    && let Some(a) = lease.on_reply(now, &r)
                    && let Change::Bound(c) = execute(&mut port, mac, a)
                {
                    leased = Some(c);
                    break 'wait;
                }
            }
            core::hint::spin_loop();
        }
        STATS.set_dhcp_state(lease.state());

        let cfg = match leased {
            Some(c) => {
                log_lease("DHCP lease", &c);
                c
            }
            None => {
                let why = match lease.state() {
                    kitsune_core::lease::State::Requesting => "no DHCP ack",
                    _ => "no DHCP offer",
                };
                serial_println!("net: static fallback ({})", why);
                NetConfig::STATIC_FALLBACK
            }
        };
        STATS.set_config(Some(&cfg), now_ms());

        let mut frame = [0u8; 64];
        let len = net::arp_announce(&mut frame, mac, cfg.ip);
        port.send(&frame[..len]);

        let n = Netd {
            net: Net::new(port, &cfg),
            lease,
            ping: PingJob::Idle,
            arp: [None; ARP_SLOTS],
            ping_seq: 0,
            ping_id: (seed >> 8) as u16,
        };
        UP.store(true, Ordering::Release);
        n
    }

    /// The TCP/IP stack, for a fetch.
    pub fn net_mut(&mut self) -> &mut Net {
        &mut self.net
    }

    fn apply(&mut self, change: Change) {
        let now = now_ms();
        match change {
            Change::None => {}
            Change::Bound(cfg) | Change::Reconfigured(cfg) => {
                self.net.reconfigure(Some(&cfg));
                STATS.set_config(Some(&cfg), now);
                self.arp = [None; ARP_SLOTS];
                log_lease("DHCP lease", &cfg);
                let mac = self.net.port().mac();
                let mut frame = [0u8; 64];
                let len = net::arp_announce(&mut frame, mac, cfg.ip);
                self.net.port().send(&frame[..len]);
            }
            Change::Renewed(cfg) => {
                STATS.set_config(Some(&cfg), now);
                match cfg.lease_secs {
                    Some(s) => serial_println!("net: DHCP lease renewed for {} s", s),
                    None => serial_println!("net: DHCP lease renewed (infinite)"),
                }
            }
            Change::Lost => {
                self.net.reconfigure(None);
                STATS.set_config(None, now);
                STATS.on_dhcp_lost();
                self.arp = [None; ARP_SLOTS];
                serial_println!("net: DHCP lease expired; address dropped, searching again");
            }
        }
        STATS.set_dhcp_state(self.lease.state());
    }

    /// Feed one DHCP reply to the lease machine.
    fn lease_reply(&mut self, now: u64, r: &DhcpReply) {
        if let Some(a) = self.lease.on_reply(now, r) {
            let mac = self.net.port().mac();
            let change = execute(self.net.port(), mac, a);
            self.apply(change);
        }
        STATS.set_dhcp_state(self.lease.state());
    }

    /// One service pass; returns how many ticks the owner may sleep before the next.
    pub fn service(&mut self) -> u64 {
        let now = now_ms();
        let mac = self.net.port().mac();
        STATS.set_link(self.net.port().link_up());

        // 1. Receive: dispatch what is waiting.
        let mut rx = [0u8; 1600];
        let mut tx = [0u8; 1600];
        for _ in 0..RX_BUDGET {
            let Some(len) = self.net.port().poll(&mut rx) else {
                break;
            };
            let frame = &rx[..len];
            if let Some(r) = net::parse_dhcp(frame, mac) {
                self.lease_reply(now, &r);
                continue;
            }
            let our_ip = self.net.config().map(|c| c.ip);
            if let Some(ip) = our_ip {
                self.on_ping_frame(frame, ip);
                if let Some(n) = net::respond(frame, mac, ip, &mut tx) {
                    self.net.port().send(&tx[..n]);
                }
            }
        }

        // 2. DHCP timers.
        for _ in 0..4 {
            let Some(a) = self.lease.poll(now) else { break };
            let change = execute(self.net.port(), mac, a);
            self.apply(change);
        }
        STATS.set_dhcp_state(self.lease.state());

        // 3. Ping.
        self.ping_step(now);

        if matches!(self.ping, PingJob::Idle) {
            IDLE_POLL_TICKS
        } else {
            1
        }
    }

    // ---- ping client ----

    fn finish_ping(&mut self, r: Result<u64, PingError>) {
        self.ping = PingJob::Idle;
        if r.is_ok() {
            STATS.on_ping_ok();
        }
        P_RESULT.store(encode_result(r), Ordering::Release);
        P_STATE.store(P_DONE, Ordering::Release);
    }

    fn on_ping_frame(&mut self, frame: &[u8], our_ip: Ipv4) {
        if matches!(self.ping, PingJob::Idle) {
            return;
        }
        let now = now_ms();
        if let Some((ip, mac)) = net::parse_arp_reply(frame, our_ip) {
            self.arp_learn(ip, mac, now);
            return;
        }
        if let PingJob::Waiting(p) = &mut self.ping
            && let Some(ev) = icmp::parse_event(frame, our_ip)
            && let Some(r) = p.on_event(now_us(), &ev)
        {
            self.finish_ping(r);
        }
    }

    /// Remember `ip -> mac`: refresh an existing entry, else take a free slot, else
    /// replace the oldest.
    fn arp_learn(&mut self, ip: Ipv4, mac: Mac, now: u64) {
        let slot = self
            .arp
            .iter()
            .position(|e| e.is_some_and(|(i, _, _)| i == ip))
            .or_else(|| self.arp.iter().position(Option::is_none))
            .unwrap_or_else(|| {
                (0..ARP_SLOTS)
                    .min_by_key(|&i| self.arp[i].map_or(0, |(_, _, t)| t))
                    .unwrap_or(0)
            });
        self.arp[slot] = Some((ip, mac, now));
    }

    /// Cached next-hop MAC, if still fresh.
    fn arp_lookup(&self, hop: Ipv4, now: u64) -> Option<Mac> {
        self.arp
            .iter()
            .flatten()
            .find(|&&(ip, _, t)| ip == hop && now.saturating_sub(t) < ARP_TTL_MS)
            .map(|&(_, m, _)| m)
    }

    fn send_echo(&mut self, cfg: &NetConfig, target: Ipv4, hop_mac: Mac, timeout_ms: u32) {
        self.ping_seq = self.ping_seq.wrapping_add(1);
        let mac = self.net.port().mac();
        let mut buf = [0u8; icmp::frame_len(PING_PAYLOAD)];
        let e = Echo {
            src_mac: mac,
            dst_mac: hop_mac,
            src_ip: cfg.ip,
            dst_ip: target,
            id: self.ping_id,
            seq: self.ping_seq,
            payload: PING_PAYLOAD,
        };
        let Some(n) = icmp::build_echo_request(&mut buf, &e) else {
            self.finish_ping(Err(PingError::BadTarget));
            return;
        };
        let sent = now_us();
        if !self.net.port().send(&buf[..n]) {
            self.finish_ping(Err(PingError::NoNetwork));
            return;
        }
        STATS.on_ping_sent();
        self.ping = PingJob::Waiting(Ping::new(
            target,
            self.ping_id,
            self.ping_seq,
            sent,
            timeout_ms,
        ));
    }

    fn start_ping(&mut self, target: Ipv4, timeout_ms: u32, now: u64) {
        let Some(cfg) = self.net.config().copied() else {
            return self.finish_ping(Err(PingError::NoNetwork));
        };
        if !self.net.port().link_up() {
            return self.finish_ping(Err(PingError::NoNetwork));
        }
        let Some(hop) = icmp::next_hop(target, cfg.ip, cfg.prefix, cfg.gateway) else {
            let usable = net::is_usable_unicast(target) && target != cfg.ip;
            return self.finish_ping(Err(if usable {
                PingError::NoRoute
            } else {
                PingError::BadTarget
            }));
        };
        if let Some(m) = self.arp_lookup(hop, now) {
            return self.send_echo(&cfg, target, m, timeout_ms);
        }
        self.ping = PingJob::Arping {
            target,
            hop,
            timeout_ms,
            tries: 0,
            next_try_ms: now,
        };
    }

    fn ping_step(&mut self, now: u64) {
        // A new request from another thread.
        if matches!(self.ping, PingJob::Idle)
            && P_STATE
                .compare_exchange(P_REQUESTED, P_RUNNING, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            let target = Ipv4(P_TARGET.load(Ordering::Relaxed).to_be_bytes());
            let timeout = P_TIMEOUT_MS.load(Ordering::Relaxed);
            self.start_ping(target, timeout, now);
        }

        match core::mem::replace(&mut self.ping, PingJob::Idle) {
            PingJob::Idle => {}
            PingJob::Arping {
                target,
                hop,
                timeout_ms,
                mut tries,
                mut next_try_ms,
            } => {
                if let Some(m) = self.arp_lookup(hop, now) {
                    match self.net.config().copied() {
                        Some(cfg) => self.send_echo(&cfg, target, m, timeout_ms),
                        None => self.finish_ping(Err(PingError::NoNetwork)),
                    }
                    return;
                }
                if now >= next_try_ms {
                    if tries >= ARP_TRIES {
                        self.finish_ping(Err(PingError::ArpFailed));
                        return;
                    }
                    tries += 1;
                    next_try_ms = now + ARP_WAIT_MS;
                    if let Some(cfg) = self.net.config().copied() {
                        let mac = self.net.port().mac();
                        let mut frame = [0u8; 64];
                        let n = net::arp_who_has(&mut frame, mac, cfg.ip, hop);
                        self.net.port().send(&frame[..n]);
                    }
                }
                self.ping = PingJob::Arping {
                    target,
                    hop,
                    timeout_ms,
                    tries,
                    next_try_ms,
                };
            }
            PingJob::Waiting(mut p) => match p.poll(now_us()) {
                Some(r) => self.finish_ping(r),
                None => self.ping = PingJob::Waiting(p),
            },
        }
    }
}
