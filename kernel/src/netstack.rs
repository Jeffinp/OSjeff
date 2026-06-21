//! TCP/IP networking via smoltcp, layered over the NE2000 driver. This is the
//! browser's transport: DNS resolution, TCP connections, and a blocking HTTP
//! GET that drives the smoltcp poll loop until the request completes.
//!
//! The guest sits behind QEMU's user-mode (SLIRP) NAT: static IP 10.0.2.15/24,
//! gateway 10.0.2.2, DNS 10.0.2.3 — so it reaches the real internet.

use crate::sync::RacyCell;
use crate::{interrupts, io, ne2000};
use alloc::vec;
use alloc::vec::Vec;
use embedded_tls::blocking::*;
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::{dns, tcp};
use smoltcp::time::Instant;
use smoltcp::wire::{DnsQueryType, EthernetAddress, IpAddress, IpCidr, Ipv4Address};

const IP: Ipv4Address = Ipv4Address::new(10, 0, 2, 15);
const GATEWAY: Ipv4Address = Ipv4Address::new(10, 0, 2, 2);
const DNS_SERVER: IpAddress = IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 3));

/// smoltcp `Instant` from the monotonic timer tick (TIMER_HZ).
fn now() -> Instant {
    let ms = interrupts::ticks() * 1000 / interrupts::TIMER_HZ as u64;
    Instant::from_millis(ms as i64)
}

// ---- NE2000 as a smoltcp phy::Device ----

pub struct Nic;
pub struct Rx(Vec<u8>);
pub struct Tx;

impl Device for Nic {
    type RxToken<'a> = Rx;
    type TxToken<'a> = Tx;

    fn receive(&mut self, _t: Instant) -> Option<(Rx, Tx)> {
        let mut buf = [0u8; 1600];
        ne2000::poll(&mut buf).map(|len| (Rx(buf[..len].to_vec()), Tx))
    }

    fn transmit(&mut self, _t: Instant) -> Option<Tx> {
        Some(Tx)
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

impl TxToken for Tx {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut buf = vec![0u8; len];
        let r = f(&mut buf);
        ne2000::send(&buf);
        r
    }
}

// ---- the stack ----

pub struct Net {
    iface: Interface,
    sockets: SocketSet<'static>,
    device: Nic,
    tcp: SocketHandle,
    dns: SocketHandle,
}

impl Net {
    pub fn new() -> Net {
        let mut device = Nic;
        let config = Config::new(EthernetAddress(ne2000::MAC.0).into());
        let mut iface = Interface::new(config, &mut device, now());
        iface.update_ip_addrs(|addrs| {
            let _ = addrs.push(IpCidr::new(IpAddress::Ipv4(IP), 24));
        });
        let _ = iface.routes_mut().add_default_ipv4_route(GATEWAY);

        let tcp_sock = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0u8; 8192]),
            tcp::SocketBuffer::new(vec![0u8; 8192]),
        );
        let dns_sock = dns::Socket::new(&[DNS_SERVER], vec![]);

        let mut sockets = SocketSet::new(vec![]);
        let tcp = sockets.add(tcp_sock);
        let dns = sockets.add(dns_sock);

        Net {
            iface,
            sockets,
            device,
            tcp,
            dns,
        }
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

    /// Resolve `host` to an IPv4 address (bounded).
    fn resolve(&mut self, host: &str) -> Option<IpAddress> {
        let query = {
            let s = self.sockets.get_mut::<dns::Socket>(self.dns);
            s.start_query(self.iface.context(), host, DnsQueryType::A)
                .ok()?
        };
        let end = Self::deadline(5000);
        while interrupts::ticks() < end {
            self.pump();
            let s = self.sockets.get_mut::<dns::Socket>(self.dns);
            match s.get_query_result(query) {
                Ok(addrs) => return addrs.first().copied(),
                Err(dns::GetQueryResultError::Pending) => {}
                Err(_) => return None,
            }
        }
        None
    }

    /// Blocking HTTP/1.0 GET over plain TCP. Returns the raw response bytes.
    pub fn http_get(&mut self, host: &str, path: &str, port: u16) -> Option<Vec<u8>> {
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

        // Drain the response until the peer closes.
        let mut out = Vec::new();
        let end = Self::deadline(10000);
        while interrupts::ticks() < end {
            self.pump();
            let s = self.sockets.get_mut::<tcp::Socket>(self.tcp);
            if s.can_recv() {
                let _ = s.recv(|data| {
                    out.extend_from_slice(data);
                    (data.len(), ())
                });
            }
            if !s.is_active() {
                break;
            }
        }
        {
            let s = self.sockets.get_mut::<tcp::Socket>(self.tcp);
            s.abort();
        }
        Some(out)
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
    /// body) as received inside the TLS tunnel.
    ///
    /// NOTE: certificate verification is skipped (`UnsecureProvider`). This
    /// reaches real search engines but does NOT authenticate the server — it is
    /// demo-grade, not production-secure.
    pub fn https_get(&mut self, host: &str, path: &str, port: u16) -> Option<Vec<u8>> {
        let ip = self.resolve(host)?;
        if !self.connect(ip, port) {
            return None;
        }

        // 16 KiB record buffers (one TLS frame). Kept in static memory so they
        // never land on the kernel stack.
        let rx_rec: &mut [u8] =
            unsafe { core::slice::from_raw_parts_mut(TLS_RX.get() as *mut u8, TLS_REC) };
        let tx_rec: &mut [u8] =
            unsafe { core::slice::from_raw_parts_mut(TLS_TX.get() as *mut u8, TLS_REC) };

        let config = TlsConfig::new().with_server_name(host);
        let stream = Stream { net: self };
        let mut tls: TlsConnection<Stream, Aes128GcmSha256> =
            TlsConnection::new(stream, rx_rec, tx_rec);

        let rng = Rdtsc::new();
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

        // Drain the decrypted response until the peer closes (read returns 0).
        let mut out = Vec::new();
        let mut buf = [0u8; 2048];
        loop {
            match tls.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    out.extend_from_slice(&buf[..n]);
                    if out.len() > 256 * 1024 {
                        break; // cap a runaway page
                    }
                }
                Err(_) => break, // includes the peer's close_notify
            }
        }

        // `tls` is unused past here, so its `&mut self` borrow (via Stream) ends
        // and we can touch the socket again to tear the connection down.
        self.sockets.get_mut::<tcp::Socket>(self.tcp).abort();
        if out.is_empty() { None } else { Some(out) }
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

/// TSC-seeded xorshift RNG. Implements the `rand_core` traits embedded-tls
/// needs for key generation.
///
/// WARNING: this is NOT a cryptographically secure RNG — it is seeded from the
/// cycle counter with no entropy pool. It is acceptable only for this demo's
/// "reach the search engine" goal, never for protecting real secrets.
struct Rdtsc {
    state: u64,
}

impl Rdtsc {
    fn new() -> Self {
        Self {
            state: io::rdtsc() | 1,
        }
    }
}

impl rand_core::RngCore for Rdtsc {
    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state ^ io::rdtsc();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    fn fill_bytes(&mut self, dst: &mut [u8]) {
        for chunk in dst.chunks_mut(8) {
            let v = self.next_u64().to_le_bytes();
            chunk.copy_from_slice(&v[..chunk.len()]);
        }
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dst);
        Ok(())
    }
}

impl rand_core::CryptoRng for Rdtsc {}
