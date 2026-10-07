//! TCP/IP networking via smoltcp, layered over the NE2000 driver. This is the
//! browser's transport: DNS resolution, TCP connections, and a blocking HTTP
//! GET that drives the smoltcp poll loop until the request completes.
//!
//! The interface address, default route and DNS server come from the
//! [`NetConfig`] the boot obtained over DHCP (or its static fallback, QEMU's
//! user-mode/SLIRP defaults 10.0.2.15/24, gateway 10.0.2.2, DNS 10.0.2.3). The
//! DHCP client is `osjeff_core::net` run once at boot; smoltcp's own DHCP socket
//! is deliberately not used, so there is a single owner of the IP address.

use crate::sync::RacyCell;
use crate::{interrupts, io, ne2000};
use alloc::vec;
use alloc::vec::Vec;
use embedded_tls::blocking::*;
use osjeff_core::browser::{MAX_RESPONSE_BYTES, append_capped};
use osjeff_core::rng::WeakMixer;
use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::socket::{dns, tcp};
use smoltcp::time::Instant;
use smoltcp::wire::{DnsQueryType, EthernetAddress, IpAddress, IpCidr};

use osjeff_core::net::NetConfig;

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
    /// Build the stack for `cfg`: interface address with its prefix, a default
    /// route through the gateway (if any) and the DNS server (if any).
    pub fn new(cfg: &NetConfig) -> Net {
        let mut device = Nic;
        let config = Config::new(EthernetAddress(ne2000::MAC.0).into());
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
        let dns_servers: Vec<IpAddress> = cfg
            .dns
            .map(|d| IpAddress::Ipv4(d.0.into()))
            .into_iter()
            .collect();
        let dns_sock = dns::Socket::new(&dns_servers, vec![]);

        let mut sockets = SocketSet::new(vec![]);
        let tcp = sockets.add(tcp_sock);
        let dns = sockets.add(dns_sock);

        // Probe the TLS random source now so the serial log records which
        // generator (RDRAND or the weak fallback) this machine will use.
        let _ = TlsRng::new();

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
