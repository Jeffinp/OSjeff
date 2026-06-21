//! TCP/IP networking via smoltcp, layered over the NE2000 driver. This is the
//! browser's transport: DNS resolution, TCP connections, and a blocking HTTP
//! GET that drives the smoltcp poll loop until the request completes.
//!
//! The guest sits behind QEMU's user-mode (SLIRP) NAT: static IP 10.0.2.15/24,
//! gateway 10.0.2.2, DNS 10.0.2.3 — so it reaches the real internet.

use crate::{interrupts, ne2000};
use alloc::vec;
use alloc::vec::Vec;
use smoltcp::iface::{Config, Interface, SocketSet, SocketHandle};
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
            s.start_query(self.iface.context(), host, DnsQueryType::A).ok()?
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
            s.connect(self.iface.context(), (ip, port), local_port).ok()?;
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
}
