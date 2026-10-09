//! TCP/IP networking via smoltcp, layered over a [`Port`] (whichever NIC driver
//! the boot picked). This is the browser's transport: DNS resolution, TCP
//! connections, and a blocking HTTP GET that drives the smoltcp poll loop until
//! the request completes. The `Net` *owns* the port, so nothing else can touch
//! the NIC while it exists unless it goes through [`Net::port`].
//!
//! The interface address, default route and DNS servers come from the
//! [`NetConfig`] that `netd` obtained over DHCP (or its static fallback, QEMU's
//! user-mode/SLIRP defaults 10.0.2.15/24, gateway 10.0.2.2, DNS 10.0.2.3) and
//! changes through [`Net::reconfigure`] when the lease is renewed with different
//! parameters or lost. The DHCP client is `osjeff_core::lease` run by `netd`;
//! smoltcp's own DHCP socket is deliberately not used, so there is a single owner
//! of the IP address.
//!
//! Names are resolved by `osjeff_core::dns` (not smoltcp's one-server DNS socket):
//! a TTL cache, every DHCP-supplied server, and failover with a timeout, over a
//! plain smoltcp UDP socket.

use crate::interrupts;
use crate::netd::now_ms;
use crate::nic::{Port, STATS};
use crate::sync::RacyCell;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use embedded_tls::blocking::*;
use osjeff_core::browser::{Conn, FailReason, append_capped};
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::{tcp, udp};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, IpAddress, IpCidr, IpEndpoint};

use osjeff_core::dns::{self, Cached, Outcome, Resolver, Step};
use osjeff_core::net::{DnsServers, Ipv4, NetConfig, parse_ipv4};

/// smoltcp `Instant` from the monotonic timer tick (TIMER_HZ).
fn now() -> Instant {
    let ms = interrupts::ticks() * 1000 / interrupts::TIMER_HZ as u64;
    Instant::from_millis(ms as i64)
}

/// A raw HTTP response (headers + body) as read from the network.
pub struct Fetched {
    pub data: Vec<u8>,
    /// The response was cut at [`MAX_RESPONSE_BYTES`]; the rest was discarded.
    pub truncated: bool,
    /// The server certificate of an https response (what the browser's popover shows).
    pub cert: Option<osjeff_core::browser::CertInfo>,
}

// ---- a Port as a smoltcp phy::Device ----

/// The device smoltcp drives: it owns the [`Port`].
pub struct Phy {
    port: Port,
}
pub struct Rx(Vec<u8>);
pub struct Tx<'a>(&'a mut Port);

impl Device for Phy {
    type RxToken<'a> = Rx;
    type TxToken<'a> = Tx<'a>;

    fn receive(&mut self, _t: Instant) -> Option<(Rx, Tx<'_>)> {
        let mut buf = [0u8; 1600];
        let len = self.port.poll(&mut buf)?;
        Some((Rx(buf[..len].to_vec()), Tx(&mut self.port)))
    }

    fn transmit(&mut self, _t: Instant) -> Option<Tx<'_>> {
        Some(Tx(&mut self.port))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut c = DeviceCapabilities::default();
        c.medium = Medium::Ethernet;
        c.max_transmission_unit = 1514;
        c
    }
}

impl RxToken for Rx {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}

impl TxToken for Tx<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut buf = vec![0u8; len];
        let r = f(&mut buf);
        self.0.send(&buf);
        r
    }
}

// ---- the stack ----

pub struct Net {
    iface: Interface,
    sockets: SocketSet<'static>,
    device: Phy,
    /// Configuration in force; `None` once the lease is lost.
    cfg: Option<NetConfig>,
    tcp: SocketHandle,
    udp: SocketHandle,
    resolver: Resolver,
    /// An app is fetching: a name (or literal) that resolves to a local, private or
    /// reserved address is refused after resolution, whatever the app's URL said
    /// (`osjeff_core::appnet::ipv4_allowed`).
    public_only: bool,
}

/// Absolute tick by which the TLS handshake in progress must be done (0 = none).
/// The stream refuses to wait past it, so a server that dribbles bytes cannot
/// hold the fetcher for longer than `MAX_HANDSHAKE_MS`.
static HS_DEADLINE: AtomicU64 = AtomicU64::new(0);

impl Net {
    /// Build the stack for `cfg`: interface address with its prefix, a default
    /// route through the gateway (if any) and the DNS server (if any).
    pub fn new(port: Port, cfg: &NetConfig) -> Net {
        let mut device = Phy { port };
        let mut config = Config::new(EthernetAddress(device.port.mac().0).into());
        // smoltcp derives TCP initial sequence numbers and ephemeral choices from this seed; a
        // zero seed would make them guessable. Weak entropy here only costs predictability, and
        // the seed is replaced by the kernel generator's output, not a timestamp.
        config.random_seed = crate::rng::u64();
        let mut iface = Interface::new(config, &mut device, now());
        iface.update_ip_addrs(|addrs| {
            let _ = addrs.push(IpCidr::new(IpAddress::Ipv4(cfg.ip.0.into()), cfg.prefix));
        });
        if let Some(gw) = cfg.gateway {
            let _ = iface.routes_mut().add_default_ipv4_route(gw.0.into());
        }

        let tcp_sock = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0u8; 8192]),
            tcp::SocketBuffer::new(vec![0u8; 8192]),
        );
        // UDP socket for DNS: a few datagrams in flight at most.
        let udp_sock = udp::Socket::new(
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0u8; 2048]),
            udp::PacketBuffer::new(vec![udp::PacketMetadata::EMPTY; 4], vec![0u8; 1024]),
        );

        let mut sockets = SocketSet::new(vec![]);
        let tcp = sockets.add(tcp_sock);
        let udp = sockets.add(udp_sock);

        Net {
            iface,
            sockets,
            device,
            cfg: Some(*cfg),
            tcp,
            udp,
            resolver: Resolver::new(cfg.dns),
            public_only: false,
        }
    }

    /// The NIC, for the frames smoltcp does not handle (DHCP, ARP/ping for the
    /// owner's own client). Only the network owner has a `Net`, and it uses this
    /// between fetches.
    pub fn port(&mut self) -> &mut Port {
        &mut self.device.port
    }

    /// The configuration in force (`None` after the lease was lost).
    pub fn config(&self) -> Option<&NetConfig> {
        self.cfg.as_ref()
    }

    /// Switch the interface to `cfg`: address, default route and resolver servers
    /// (`None`: no address, the lease is gone). Open connections are dropped and
    /// the DNS cache is flushed when the servers change.
    pub fn reconfigure(&mut self, cfg: Option<&NetConfig>) {
        self.sockets.get_mut::<tcp::Socket>(self.tcp).abort();
        self.iface.update_ip_addrs(|addrs| {
            addrs.clear();
            if let Some(c) = cfg {
                let _ = addrs.push(IpCidr::new(IpAddress::Ipv4(c.ip.0.into()), c.prefix));
            }
        });
        let routes = self.iface.routes_mut();
        routes.remove_default_ipv4_route();
        if let Some(gw) = cfg.and_then(|c| c.gateway) {
            let _ = routes.add_default_ipv4_route(gw.0.into());
        }
        self.resolver
            .set_servers(cfg.map_or(DnsServers::NONE, |c| c.dns));
        self.cfg = cfg.copied();
    }

    fn poll(&mut self) {
        self.iface.poll(now(), &mut self.device, &mut self.sockets);
    }

    /// Spin (driving the stack) until `deadline_ms` of timer time elapses.
    fn pump(&mut self) {
        self.poll();
        core::hint::spin_loop();
    }

    fn deadline(ms: u64) -> u64 {
        interrupts::ticks() + ms * interrupts::TIMER_HZ as u64 / 1000
    }

    /// Resolve `host` to an IPv4 address (bounded). A dotted-quad literal is its
    /// own answer and never goes to the network; otherwise the TTL cache is
    /// consulted, then the DHCP-supplied servers are asked in turn (rotating on
    /// timeout or SERVFAIL, see `osjeff_core::dns::Resolve`).
    pub(crate) fn resolve(&mut self, host: &str) -> Option<IpAddress> {
        if let Some(ip) = parse_ipv4(host.as_bytes()) {
            return Some(IpAddress::Ipv4(ip.0.into()));
        }
        STATS.on_dns_query();
        let start = now_ms();
        match self.resolver.lookup(host, start) {
            Some(Cached::Addr(a)) => {
                STATS.on_dns_cache_hit();
                crate::serial_println!("dns: {} -> {} (cache)", host, a);
                return Some(IpAddress::Ipv4(a.0.into()));
            }
            Some(Cached::NxDomain) => {
                STATS.on_dns_cache_hit();
                crate::serial_println!("dns: {} does not exist (cache)", host);
                return None;
            }
            None => {}
        }

        // An unpredictable id and source port are the only defense against a
        // forged answer from a host that cannot see our packets.
        let mut r = [0u8; 4];
        crate::rng::fill(&mut r);
        let id = u16::from_le_bytes([r[0], r[1]]);
        let local_port = 32768 + (u16::from_le_bytes([r[2], r[3]]) % 28000);
        let mut query = [0u8; 300];
        let qlen = dns::build_query(&mut query, id, host)?;
        {
            let s = self.sockets.get_mut::<udp::Socket>(self.udp);
            s.close();
            s.bind(local_port).ok()?;
        }

        let mut q = self.resolver.begin(host, id, start);
        let outcome = 'lookup: loop {
            match q.poll(now_ms()) {
                Step::Done(o) => break o,
                Step::Send { server, .. } => {
                    if q.attempts() > 1 {
                        STATS.on_dns_failover();
                        crate::serial_println!("dns: no answer for {}, trying {}", host, server);
                    }
                    let to = IpEndpoint::new(IpAddress::Ipv4(server.0.into()), dns::DNS_PORT);
                    let s = self.sockets.get_mut::<udp::Socket>(self.udp);
                    // smoltcp sends a socket's datagrams in order and a datagram whose next hop
                    // has not answered ARP stays at the head of the queue, blocking everything
                    // behind it: a dead first server would then starve the failover query. Closing
                    // the socket drops the stale datagram (a late answer to it is still accepted,
                    // it is matched by id and source, not by socket state).
                    s.close();
                    if s.bind(local_port).is_err() {
                        break Outcome::Failed;
                    }
                    let _ = s.send_slice(&query[..qlen], to);
                }
                Step::Wait(_) => {}
            }
            self.pump();
            let s = self.sockets.get_mut::<udp::Socket>(self.udp);
            while let Ok((data, meta)) = s.recv() {
                let IpAddress::Ipv4(from) = meta.endpoint.addr;
                if let Some(o) = q.on_response(Ipv4(from.octets()), data) {
                    break 'lookup o;
                }
            }
        };
        self.sockets.get_mut::<udp::Socket>(self.udp).close();
        self.resolver.finish(&q, outcome, now_ms());

        match outcome {
            Outcome::Resolved { addr, ttl } => {
                crate::serial_println!(
                    "dns: {} -> {} (ttl {} s, {} attempt{})",
                    host,
                    addr,
                    ttl,
                    q.attempts(),
                    if q.attempts() == 1 { "" } else { "s" }
                );
                Some(IpAddress::Ipv4(addr.0.into()))
            }
            Outcome::NxDomain | Outcome::NoData => {
                crate::serial_println!("dns: {} does not exist", host);
                None
            }
            Outcome::Failed | Outcome::NoServers => {
                STATS.on_dns_failure();
                crate::serial_println!("dns: {} failed (no answer from any server)", host);
                None
            }
        }
    }

    /// Tear the TCP connection down and let smoltcp put the RST on the wire (it
    /// is only queued by `abort`; without a poll the peer keeps a half-open
    /// connection and a single-threaded server never accepts the next one).
    fn abort_conn(&mut self) {
        self.sockets.get_mut::<tcp::Socket>(self.tcp).abort();
        for _ in 0..4 {
            self.poll();
        }
    }

    /// Restrict the next fetches to public destinations (apps), or lift the
    /// restriction (the browser). The name policy is `appnet::parse_url`; this repeats
    /// the check on the *resolved* address, which a name can point anywhere.
    pub fn set_public_only(&mut self, on: bool) {
        self.public_only = on;
    }

    /// Resolve for a fetch: [`FailReason::Dns`] when the name does not resolve.
    fn resolve_or_fail(&mut self, host: &str) -> Result<IpAddress, FailReason> {
        let ip = self.resolve(host).ok_or(FailReason::Dns)?;
        let IpAddress::Ipv4(a) = ip;
        if self.public_only && !osjeff_core::appnet::ipv4_allowed(a.octets()) {
            crate::serial_println!(
                "net: {} resolves to {}, a non-public address: refused",
                host,
                a
            );
            return Err(FailReason::Network);
        }
        Ok(ip)
    }

    /// Blocking HTTP/1.1 GET over plain TCP. Returns the raw response bytes,
    /// at most [`MAX_RESPONSE_BYTES`] of them (the same cap as the TLS path).
    pub fn http_get(
        &mut self,
        host: &str,
        path: &str,
        port: u16,
        cap: usize,
    ) -> Result<Fetched, FailReason> {
        let ip = self.resolve_or_fail(host)?;
        self.connect(ip, port)?;

        // Request.
        let mut req = Vec::new();
        build_request(&mut req, host, path, port, false);
        {
            let s = self.sockets.get_mut::<tcp::Socket>(self.tcp);
            if s.send_slice(&req).is_err() {
                s.abort();
                return Err(FailReason::Network);
            }
        }

        // Drain the response until the peer closes or the cap is reached.
        let mut out = Vec::new();
        let mut truncated = false;
        let end = Self::deadline(10000);
        let mut timed_out = true;
        while interrupts::ticks() < end {
            self.pump();
            let s = self.sockets.get_mut::<tcp::Socket>(self.tcp);
            if s.can_recv() {
                let _ = s.recv(|data| {
                    // Consume everything the socket holds (so it keeps
                    // draining) but keep only what fits under the cap.
                    truncated |= append_capped(&mut out, data, cap);
                    (data.len(), ())
                });
            }
            // Done when the cap is hit or the peer closed and everything it sent was
            // read (CloseWait still counts as `is_active`, so that alone would wait for
            // the whole timeout after the server's FIN).
            if truncated || (!s.may_recv() && !s.can_recv()) {
                timed_out = false;
                break;
            }
        }
        self.abort_conn();
        if truncated {
            crate::serial_println!("http: response truncated at {} bytes", cap);
        }
        if out.is_empty() {
            return Err(if timed_out {
                FailReason::Timeout
            } else {
                FailReason::Network
            });
        }
        Ok(Fetched {
            data: out,
            truncated,
            cert: None,
        })
    }

    /// Open the TCP connection to `ip:port` (bounded). Shared by the plain-HTTP
    /// and TLS paths. An RST during the handshake is [`FailReason::Refused`], no
    /// answer within 8 s is [`FailReason::Timeout`].
    fn connect(&mut self, ip: IpAddress, port: u16) -> Result<(), FailReason> {
        let local_port = 49152 + (crate::rng::u32() as u16 & 0x3FFF);
        {
            let s = self.sockets.get_mut::<tcp::Socket>(self.tcp);
            s.abort();
            if s.connect(self.iface.context(), (ip, port), local_port)
                .is_err()
            {
                return Err(FailReason::Network);
            }
        }
        let end = Self::deadline(8000);
        while interrupts::ticks() < end {
            self.pump();
            let s = self.sockets.get_mut::<tcp::Socket>(self.tcp);
            if s.may_send() {
                return Ok(());
            }
            if s.state() == tcp::State::Closed {
                return Err(FailReason::Refused);
            }
        }
        self.abort_conn();
        Err(FailReason::Timeout)
    }

    /// Blocking HTTPS GET over TLS 1.3 **with certificate verification**.
    /// Returns the raw HTTP response (headers + body) as received inside the TLS
    /// tunnel, capped at [`MAX_RESPONSE_BYTES`] like [`Net::http_get`], and how
    /// the connection was authenticated.
    ///
    /// The server's chain is validated against the embedded trust store, for
    /// `host`, at the trusted time (`crate::tlsv`). A failure aborts the
    /// handshake with [`FailReason::Cert`] unless `allow_insecure` (the user's
    /// per-origin, per-session override) is set, in which case the page is
    /// returned as [`Conn::Insecure`]. A verified chain is [`Conn::Verified`].
    pub fn https_get(
        &mut self,
        host: &str,
        path: &str,
        port: u16,
        allow_insecure: bool,
        cap: usize,
    ) -> Result<(Fetched, Conn), FailReason> {
        // No handshake without a seeded generator: wait (bounded) for 128 credited bits first, and
        // refuse rather than draw the client random and the ephemeral key from a weak generator.
        // Before the DNS lookup and the TCP connect, so no half-open connection waits on us.
        if !crate::rng::wait_ready(crate::rng::TLS_WAIT_MS) {
            return Err(FailReason::Tls);
        }
        let ip = self.resolve_or_fail(host)?;
        self.connect(ip, port)?;

        // 16 KiB record buffers (one TLS frame). Kept in static memory so they
        // never land on the kernel stack.
        // SAFETY: TLS_RX is used only here, and `https_get` runs only on the fetcher thread, one call at
        // a time (`&mut self`), so this is the only live reference; `TLS_REC` is the array length. It
        // dies with `tls` before this fn returns.
        let rx_rec: &mut [u8] =
            unsafe { core::slice::from_raw_parts_mut(TLS_RX.get() as *mut u8, TLS_REC) };
        // SAFETY: as for TLS_RX above (TLS_TX is a distinct static, used only here).
        let tx_rec: &mut [u8] =
            unsafe { core::slice::from_raw_parts_mut(TLS_TX.get() as *mut u8, TLS_REC) };

        let config = TlsConfig::new()
            .enable_rsa_signatures()
            .with_server_name(host);
        let mut verifier = crate::tlsv::Verifier::new(allow_insecure);
        HS_DEADLINE.store(
            interrupts::ticks()
                + osjeff_core::tlsverify::MAX_HANDSHAKE_MS * u64::from(interrupts::TIMER_HZ) / 1000,
            Ordering::Relaxed,
        );
        let hs_start = now_ms();
        let stream = Stream { net: self };
        let mut tls: TlsConnection<Stream, Aes128GcmSha256> =
            TlsConnection::new(stream, rx_rec, tx_rec);

        let provider = crate::tlsv::Provider {
            rng: crate::rng::KernelRng,
            verifier: &mut verifier,
        };
        let opened = tls.open(TlsContext::new(&config, provider));
        if let Err(e) = opened {
            HS_DEADLINE.store(0, Ordering::Relaxed);
            self.abort_conn();
            if let Some(ce) = verifier.failure() {
                return Err(FailReason::Cert(ce));
            }
            crate::serial_println!("https: TLS handshake failed for {}: {:?}", host, e);
            return Err(if now_ms().saturating_sub(hs_start) >= 11_000 {
                FailReason::Timeout
            } else {
                FailReason::Tls
            });
        }
        HS_DEADLINE.store(0, Ordering::Relaxed);
        let hs_ms = now_ms().saturating_sub(hs_start);
        let (mut chain_len, mut root) = (1usize, None);
        let conn = match verifier.outcome() {
            Some(crate::tlsv::Outcome::Verified(v)) => {
                chain_len = v.chain_len;
                root = Some(verifier.root_name(&v));
                crate::serial_println!(
                    "tls: chain verified for {} ({} certs, root {}); handshake {} ms",
                    host,
                    v.chain_len,
                    verifier.root_name(&v),
                    hs_ms
                );
                Conn::Verified
            }
            Some(crate::tlsv::Outcome::Overridden(e)) => {
                crate::serial_println!(
                    "tls: UNVERIFIED connection to {} ({}), user override; handshake {} ms",
                    host,
                    e.reason(),
                    hs_ms
                );
                Conn::Insecure
            }
            None => {
                // The handshake completed without the verifier concluding: never
                // report that as secure.
                self.abort_conn();
                return Err(FailReason::Tls);
            }
        };

        // Request (HTTP/1.1 with Connection: close; chunked and gzip responses are
        // decoded by `browser::page_body`).
        let mut req = Vec::new();
        build_request(&mut req, host, path, port, true);
        use embedded_io::Write as _;
        if tls.write_all(&req).is_err() || tls.flush().is_err() {
            self.abort_conn();
            return Err(FailReason::Network);
        }

        // Drain the decrypted response until the peer closes (read returns 0)
        // or the cap is reached.
        let mut out = Vec::new();
        let mut truncated = false;
        let mut buf = [0u8; 2048];
        loop {
            match tls.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if append_capped(&mut out, &buf[..n], cap) {
                        truncated = true; // cap a runaway page
                        break;
                    }
                }
                Err(_) => break, // includes the peer's close_notify
            }
        }

        // `tls` is unused past here, so its `&mut self` borrow (via Stream) ends
        // and we can touch the socket again to tear the connection down.
        self.abort_conn();
        if truncated {
            crate::serial_println!("https: response truncated at {} bytes", cap);
        }
        if out.is_empty() {
            Err(FailReason::Network)
        } else {
            let cert = osjeff_core::browser::CertInfo::from_leaf(
                verifier.leaf_der(),
                host,
                chain_len,
                root,
            );
            Ok((
                Fetched {
                    data: out,
                    truncated,
                    cert,
                },
                conn,
            ))
        }
    }

    /// One SNTP exchange with `server` (bounded to `timeout_ms`). Returns the
    /// validated measurement, or `None` (the reason is on the serial log).
    pub fn sntp_query(
        &mut self,
        server: IpAddress,
        timeout_ms: u64,
    ) -> Option<osjeff_core::sntp::Measurement> {
        use osjeff_core::sntp;
        let mut r = [0u8; 4];
        crate::rng::fill(&mut r);
        let local_port = 32768 + (u16::from_le_bytes([r[0], r[1]]) % 28000);
        let nonce = u16::from_le_bytes([r[2], r[3]]);
        {
            let s = self.sockets.get_mut::<udp::Socket>(self.udp);
            s.close();
            s.bind(local_port).ok()?;
        }
        let t1 = crate::clock::local_ntp(nonce);
        let req = sntp::build_request(t1);
        {
            let to = IpEndpoint::new(server, sntp::PORT);
            let s = self.sockets.get_mut::<udp::Socket>(self.udp);
            s.send_slice(&req, to).ok()?;
        }
        let end = Self::deadline(timeout_ms);
        let mut result = None;
        'wait: while interrupts::ticks() < end {
            self.pump();
            let s = self.sockets.get_mut::<udp::Socket>(self.udp);
            while let Ok((data, meta)) = s.recv() {
                if meta.endpoint.addr != server {
                    continue; // not from the server we asked
                }
                let t4 = crate::clock::local_ntp(0);
                match sntp::process_reply(t1, t4, data) {
                    Ok(m) => {
                        result = Some(m);
                        break 'wait;
                    }
                    Err(e) => {
                        crate::serial_println!(
                            "sntp: reply from {} rejected: {}",
                            server,
                            e.reason()
                        );
                    }
                }
            }
        }
        self.sockets.get_mut::<udp::Socket>(self.udp).close();
        result
    }

    /// Sync the trusted clock: try the time servers by name (DNS), then the
    /// gateway as a last resort. Logs every step; returns whether the clock is
    /// now confirmed.
    pub fn sync_time(&mut self) -> bool {
        const SERVERS: [&str; 3] = ["time.cloudflare.com", "pool.ntp.org", "time.google.com"];
        for name in SERVERS {
            let Some(ip) = self.resolve(name) else {
                continue;
            };
            for _ in 0..2 {
                if let Some(m) = self.sntp_query(ip, 1000) {
                    crate::serial_println!("sntp: {} answered", name);
                    crate::clock::apply_sntp(&m);
                    return true;
                }
            }
            crate::serial_println!("sntp: no valid answer from {}", name);
        }
        if let Some(gw) = self.cfg.and_then(|c| c.gateway) {
            let ip = IpAddress::Ipv4(gw.0.into());
            if let Some(m) = self.sntp_query(ip, 1500) {
                crate::serial_println!("sntp: gateway {} answered", gw);
                crate::clock::apply_sntp(&m);
                return true;
            }
        }
        crate::serial_println!("sntp: time NOT confirmed (using the RTC)");
        false
    }
}

/// Build an HTTP/1.1 GET request (`Connection: close`: one request per connection) (shared by the plain and TLS paths).
fn build_request(req: &mut Vec<u8>, host: &str, path: &str, port: u16, tls: bool) {
    req.extend_from_slice(b"GET ");
    req.extend_from_slice(path.as_bytes());
    req.extend_from_slice(b" HTTP/1.1\r\nHost: ");
    req.extend_from_slice(host.as_bytes());
    let default_port = if tls { 443 } else { 80 };
    if port != default_port {
        req.push(b':');
        let mut digits = [0u8; 5];
        let mut n = port;
        let mut i = digits.len();
        loop {
            i -= 1;
            digits[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        req.extend_from_slice(&digits[i..]);
    }
    req.extend_from_slice(
        b"\r\nUser-Agent: OSjeff/1.0\r\nAccept: text/html, image/png, image/bmp, */*;q=0.1\r\nAccept-Encoding: gzip, deflate\r\nConnection: close\r\n\r\n",
    );
}

// ---- TLS plumbing: an embedded-io stream over the smoltcp socket + an RNG ----

const TLS_REC: usize = 16 * 1024;
static TLS_RX: RacyCell<[u8; TLS_REC]> = RacyCell::new([0; TLS_REC]);
static TLS_TX: RacyCell<[u8; TLS_REC]> = RacyCell::new([0; TLS_REC]);

/// `embedded_io::Read + Write` over the active smoltcp TCP socket. Every call
/// drives the smoltcp poll loop until bytes move, so embedded-tls can run its
/// blocking handshake on our single-threaded stack.
struct Stream<'a> {
    net: &'a mut Net,
}

#[derive(Debug)]
struct StreamError;

impl core::fmt::Display for StreamError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("tcp stream error")
    }
}

impl core::error::Error for StreamError {}

impl embedded_io::Error for StreamError {
    fn kind(&self) -> embedded_io::ErrorKind {
        embedded_io::ErrorKind::Other
    }
}

impl embedded_io::ErrorType for Stream<'_> {
    type Error = StreamError;
}

impl embedded_io::Read for Stream<'_> {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, StreamError> {
        let mut end = Net::deadline(12000);
        let hs = HS_DEADLINE.load(Ordering::Relaxed);
        if hs != 0 {
            end = end.min(hs);
        }
        loop {
            self.net.poll();
            let s = self.net.sockets.get_mut::<tcp::Socket>(self.net.tcp);
            if s.can_recv() {
                let n = s.recv_slice(buf).map_err(|_| StreamError)?;
                if n > 0 {
                    return Ok(n);
                }
            }
            // Peer closed and the buffer is drained → EOF.
            if !s.may_recv() && !s.can_recv() {
                return Ok(0);
            }
            if interrupts::ticks() >= end {
                return Err(StreamError);
            }
            core::hint::spin_loop();
        }
    }
}

impl embedded_io::Write for Stream<'_> {
    fn write(&mut self, buf: &[u8]) -> Result<usize, StreamError> {
        let mut end = Net::deadline(12000);
        let hs = HS_DEADLINE.load(Ordering::Relaxed);
        if hs != 0 {
            end = end.min(hs);
        }
        loop {
            self.net.poll();
            let s = self.net.sockets.get_mut::<tcp::Socket>(self.net.tcp);
            if s.can_send() {
                let n = s.send_slice(buf).map_err(|_| StreamError)?;
                if n > 0 {
                    self.net.poll(); // flush the segment out promptly
                    return Ok(n);
                }
            }
            if !s.may_send() {
                return Err(StreamError);
            }
            if interrupts::ticks() >= end {
                return Err(StreamError);
            }
            core::hint::spin_loop();
        }
    }

    fn flush(&mut self) -> Result<(), StreamError> {
        self.net.poll();
        Ok(())
    }
}
