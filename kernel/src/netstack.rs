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

use crate::netd::now_ms;
use crate::nic::{Port, STATS};
use crate::sync::RacyCell;
use crate::{interrupts, io};
use alloc::vec;
use alloc::vec::Vec;
use embedded_tls::blocking::*;
use osjeff_core::browser::{MAX_RESPONSE_BYTES, append_capped};
use osjeff_core::rng::WeakMixer;
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
}

impl Net {
    /// Build the stack for `cfg`: interface address with its prefix, a default
    /// route through the gateway (if any) and the DNS server (if any).
    pub fn new(port: Port, cfg: &NetConfig) -> Net {
        let mut device = Phy { port };
        let config = Config::new(EthernetAddress(device.port.mac().0).into());
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

        // Probe the TLS random source now so the serial log records which
        // generator (RDRAND or the weak fallback) this machine will use.
        let _ = TlsRng::new();

        Net {
            iface,
            sockets,
            device,
            cfg: Some(*cfg),
            tcp,
            udp,
            resolver: Resolver::new(cfg.dns),
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
    fn resolve(&mut self, host: &str) -> Option<IpAddress> {
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
        let mix = io::rdtsc() ^ interrupts::ticks().rotate_left(29);
        let id = (mix ^ (mix >> 16) ^ (mix >> 32)) as u16;
        let local_port = 32768 + (mix % 28000) as u16;
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

    /// Blocking HTTP/1.0 GET over plain TCP. Returns the raw response bytes,
    /// at most [`MAX_RESPONSE_BYTES`] of them (the same cap as the TLS path).
    pub fn http_get(&mut self, host: &str, path: &str, port: u16) -> Option<Fetched> {
        let ip = self.resolve(host)?;

        // Connect.
        let local_port = 49152 + (interrupts::ticks() as u16 & 0x3FFF);
        {
            let s = self.sockets.get_mut::<tcp::Socket>(self.tcp);
            s.connect(self.iface.context(), (ip, port), local_port)
                .ok()?;
        }
        let end = Self::deadline(8000);
        while interrupts::ticks() < end {
            self.pump();
            if self.sockets.get_mut::<tcp::Socket>(self.tcp).may_send() {
                break;
            }
        }

        // Request.
        let mut req = Vec::new();
        req.extend_from_slice(b"GET ");
        req.extend_from_slice(path.as_bytes());
        req.extend_from_slice(b" HTTP/1.0\r\nHost: ");
        req.extend_from_slice(host.as_bytes());
        req.extend_from_slice(b"\r\nConnection: close\r\n\r\n");
        {
            let s = self.sockets.get_mut::<tcp::Socket>(self.tcp);
            s.send_slice(&req).ok()?;
        }

        // Drain the response until the peer closes or the cap is reached.
        let mut out = Vec::new();
        let mut truncated = false;
        let end = Self::deadline(10000);
        while interrupts::ticks() < end {
            self.pump();
            let s = self.sockets.get_mut::<tcp::Socket>(self.tcp);
            if s.can_recv() {
                let _ = s.recv(|data| {
                    // Consume everything the socket holds (so it keeps
                    // draining) but keep only what fits under the cap.
                    truncated |= append_capped(&mut out, data, MAX_RESPONSE_BYTES);
                    (data.len(), ())
                });
            }
            if truncated || !s.is_active() {
                break;
            }
        }
        {
            let s = self.sockets.get_mut::<tcp::Socket>(self.tcp);
            s.abort();
        }
        if truncated {
            crate::serial_println!("http: response truncated at {} bytes", MAX_RESPONSE_BYTES);
        }
        Some(Fetched {
            data: out,
            truncated,
        })
    }

    /// Open the TCP connection to `ip:port` (bounded). Returns `true` once the
    /// socket can send. Shared by the plain-HTTP and TLS paths.
    fn connect(&mut self, ip: IpAddress, port: u16) -> bool {
        let local_port = 49152 + (interrupts::ticks() as u16 & 0x3FFF);
        {
            let s = self.sockets.get_mut::<tcp::Socket>(self.tcp);
            if s.connect(self.iface.context(), (ip, port), local_port)
                .is_err()
            {
                return false;
            }
        }
        let end = Self::deadline(8000);
        while interrupts::ticks() < end {
            self.pump();
            if self.sockets.get_mut::<tcp::Socket>(self.tcp).may_send() {
                return true;
            }
        }
        false
    }

    /// Blocking HTTPS GET over TLS 1.3. Returns the raw HTTP response (headers +
    /// body) as received inside the TLS tunnel, capped at
    /// [`MAX_RESPONSE_BYTES`] like [`Net::http_get`].
    ///
    /// NOTE: certificate verification is skipped (`UnsecureProvider`). The
    /// traffic is encrypted, but the server is NOT authenticated: a man in the
    /// middle is not detected. The browser UI therefore labels every `https://`
    /// page "Conexao nao verificada" and never shows a padlock.
    pub fn https_get(&mut self, host: &str, path: &str, port: u16) -> Option<Fetched> {
        let ip = self.resolve(host)?;
        if !self.connect(ip, port) {
            return None;
        }

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

        let config = TlsConfig::new().with_server_name(host);
        let stream = Stream { net: self };
        let mut tls: TlsConnection<Stream, Aes128GcmSha256> =
            TlsConnection::new(stream, rx_rec, tx_rec);

        let rng = TlsRng::new();
        if let Err(e) = tls.open(TlsContext::new(
            &config,
            UnsecureProvider::new::<Aes128GcmSha256>(rng),
        )) {
            crate::serial_println!("https: TLS handshake failed for {}: {:?}", host, e);
            self.sockets.get_mut::<tcp::Socket>(self.tcp).abort();
            return None;
        }

        // Request (HTTP/1.0, Connection: close — avoids chunked responses).
        let mut req = Vec::new();
        req.extend_from_slice(b"GET ");
        req.extend_from_slice(path.as_bytes());
        req.extend_from_slice(b" HTTP/1.0\r\nHost: ");
        req.extend_from_slice(host.as_bytes());
        req.extend_from_slice(
            b"\r\nUser-Agent: OSjeff/1.0\r\nAccept: text/html\r\nConnection: close\r\n\r\n",
        );
        use embedded_io::Write as _;
        if tls.write_all(&req).is_err() || tls.flush().is_err() {
            self.sockets.get_mut::<tcp::Socket>(self.tcp).abort();
            return None;
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
                    if append_capped(&mut out, &buf[..n], MAX_RESPONSE_BYTES) {
                        truncated = true; // cap a runaway page
                        break;
                    }
                }
                Err(_) => break, // includes the peer's close_notify
            }
        }

        // `tls` is unused past here, so its `&mut self` borrow (via Stream) ends
        // and we can touch the socket again to tear the connection down.
        self.sockets.get_mut::<tcp::Socket>(self.tcp).abort();
        if truncated {
            crate::serial_println!("https: response truncated at {} bytes", MAX_RESPONSE_BYTES);
        }
        if out.is_empty() {
            None
        } else {
            Some(Fetched {
                data: out,
                truncated,
            })
        }
    }
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
        let end = Net::deadline(12000);
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
        let end = Net::deadline(12000);
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

/// Random source for the TLS handshake (ephemeral key share, client random).
///
/// * With `RDRAND` (`CPUID.01H:ECX[30]`) every word comes from the hardware
///   generator, retried up to [`osjeff_core::rng::RDRAND_RETRIES`] times.
/// * Without it (or if the hardware keeps failing) it falls back to
///   [`WeakMixer`]: a hash of TSC / timer ticks / RTC. That is best effort and
///   NOT cryptographically secure; the first use is announced on the serial
///   console as `RNG: weak fallback`.
///
/// LIMITATION: `embedded-tls` bounds its provider's RNG by `CryptoRng`, so the
/// fallback path has to implement that marker trait too even though it does
/// not deserve it. The type system cannot separate the two here; the serial
/// message and the browser's "Conexao nao verificada" label (the server is
/// never authenticated anyway) are the honest signals. Replace the fallback by
/// refusing TLS if this ever carries real secrets.
struct TlsRng {
    hw: bool,
    weak: WeakMixer,
    warned: bool,
}

impl TlsRng {
    fn new() -> Self {
        let mut hw = osjeff_core::rng::has_rdrand(cpuid_01h_ecx());
        // Advertised is not the same as working: draw one word to prove it.
        let hw_dead = hw && osjeff_core::rng::retry_hw(rdrand64).is_none();
        if hw_dead {
            hw = false;
        }
        // Noise: cycle counter + PIT tick count. (The CMOS RTC is deliberately
        // not read: its index/data port pair is shared with the compositor's
        // clock and a context switch between the two accesses would corrupt it.)
        let weak = WeakMixer::new(&[io::rdtsc(), interrupts::ticks()]);
        let mut rng = Self {
            hw,
            weak,
            warned: false,
        };
        if hw {
            crate::serial_println!("RNG: RDRAND");
        } else if hw_dead {
            rng.warn_weak("RDRAND advertised but failed");
        } else {
            rng.warn_weak("RDRAND not available");
        }
        rng
    }

    fn warn_weak(&mut self, why: &str) {
        if !self.warned {
            self.warned = true;
            crate::serial_println!("RNG: weak fallback ({})", why);
        }
    }

    fn next_word(&mut self) -> u64 {
        if self.hw {
            if let Some(v) = osjeff_core::rng::retry_hw(rdrand64) {
                return v;
            }
            self.warn_weak("RDRAND failed");
        }
        self.weak.next(io::rdtsc())
    }
}

/// `CPUID.01H:ECX`.
fn cpuid_01h_ecx() -> u32 {
    // CPUID leaf 1 exists on every x86_64 CPU and has no side effects.
    core::arch::x86_64::__cpuid(1).ecx
}

/// One `RDRAND` attempt: `Some` on success, `None` if the hardware was not
/// ready (carry flag clear). Only call after CPUID reported RDRAND.
fn rdrand64() -> Option<u64> {
    #[target_feature(enable = "rdrand")]
    fn step() -> Option<u64> {
        let mut v = 0u64;
        // (Safe to call here: the `rdrand` target feature is enabled on `step`.)
        let ok = core::arch::x86_64::_rdrand64_step(&mut v);
        (ok == 1).then_some(v)
    }
    // SAFETY: only reached when CPUID.01H:ECX[30] is set (`TlsRng::hw`), so the
    // instruction exists on this CPU.
    unsafe { step() }
}

impl rand_core::RngCore for TlsRng {
    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }

    fn next_u64(&mut self) -> u64 {
        self.next_word()
    }

    fn fill_bytes(&mut self, dst: &mut [u8]) {
        for chunk in dst.chunks_mut(8) {
            let v = self.next_word().to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
        }
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dst);
        Ok(())
    }
}

// Required by `embedded-tls` (see the LIMITATION note on `TlsRng`).
impl rand_core::CryptoRng for TlsRng {}
