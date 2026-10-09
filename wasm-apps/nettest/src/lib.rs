//! `nettest`: drives `net_http_get` through every rule of the app network policy.
//! Keys 1..9 and 0 issue one request each; the result line shows the ABI code and the start
//! of the body. The manifest allows only `203.0.113.5` (a TEST-NET-3 address, which the
//! destination filter lets through), so the other targets exercise the refusals.
//!
//! 1 `/hello` 200          2 `/redir-ok` -> /hello      3 `/redir-local` -> 10.0.2.2 (refused)
//! 4 `/missing` 404        5 other host (not on net_hosts)   6 `10.0.2.2` (private, filter)
//! 7 `/gzip` (decoded)     8 `https://` (self-signed certificate)   9 `/chunked`
//! 0 `127.0.0.1.nip.io` (a public-looking NAME that resolves to loopback: refused after DNS)

#![no_std]

use core::fmt::Write;
use osjeff_sdk::*;

manifest!(
    "id=nettest\nname=Network test\nname.pt=Teste de rede\nname.en=Network test\nversion=1.0.0\nabi=2\nnet=http\nnet_hosts=203.0.113.5,*.nip.io\nwin_w=520\nwin_h=300\nmem_mib=2\n"
);

const URLS: [&str; 10] = [
    "http://203.0.113.5:8077/hello",
    "http://203.0.113.5:8077/redir-ok",
    "http://203.0.113.5:8077/redir-local",
    "http://203.0.113.5:8077/missing",
    "http://203.0.113.9:8077/hello",
    "http://10.0.2.2/",
    "http://203.0.113.5:8077/gzip",
    "https://203.0.113.5:8078/hello",
    "http://203.0.113.5:8077/chunked",
    "http://127.0.0.1.nip.io:8077/hello",
];

struct NetTest {
    last: StrBuf<96>,
    body: StrBuf<96>,
    count: u32,
}

impl App for NetTest {
    fn new() -> Self {
        NetTest {
            last: StrBuf::new(),
            body: StrBuf::new(),
            count: 0,
        }
    }

    fn on_key(&mut self, code: i32, _mods: i32) {
        // keys 1..9, then 0 for the tenth
        let idx = match code as u8 {
            k @ b'1'..=b'9' => (k - b'1') as usize,
            b'0' => 9,
            _ => return,
        };
        let Some(url) = URLS.get(idx) else {
            return;
        };
        let mut buf = [0u8; 600];
        self.count += 1;
        self.last.clear();
        self.body.clear();
        match http_get(url, &mut buf) {
            Ok(n) => {
                let _ = write!(self.last, "{}: ok, {} bytes", url, n);
                for &b in buf[..n.min(90)].iter() {
                    let c = if (0x20..0x7F).contains(&b) { b as char } else { '.' };
                    let _ = self.body.write_char(c);
                }
                log!("nettest {}: ok {} bytes", url, n);
            }
            Err(e) => {
                let _ = write!(self.last, "{}: {} {}", url, tr("erro", "error"), e.0);
                log!("nettest {}: error {}", url, e.0);
            }
        }
    }

    fn render(&mut self, c: &mut Canvas) {
        let (w, h) = c.size();
        c.clear(0x10141F);
        c.fill_rect(0, 0, w, 30, 0x2F6FDE);
        c.text(
            12,
            8,
            tr(
                "Teste de rede: teclas 1 a 9 e 0",
                "Network test: keys 1 to 9 and 0",
            ),
            0xFFFFFF,
            2,
        );
        let mut y = 44;
        for (i, u) in URLS.iter().enumerate() {
            let mut b = StrBuf::<80>::new();
            let _ = write!(b, "{} {}", i + 1, u);
            c.text(12, y, b.as_str(), 0x9AA6BD, 1);
            y += 12;
        }
        c.fill_rect(12, y + 6, w - 24, 2, 0x2A3140);
        c.text(12, y + 16, self.last.as_str(), 0xFFD84D, 1);
        c.text(12, y + 32, self.body.as_str(), 0xFFFFFF, 1);
        let mut b = StrBuf::<32>::new();
        let _ = write!(b, "{}: {}", tr("pedidos", "requests"), self.count);
        c.text(12, h - 16, b.as_str(), 0x6A7488, 1);
    }
}

export_app!(NetTest);
