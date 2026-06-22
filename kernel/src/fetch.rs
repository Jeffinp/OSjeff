//! Background page fetcher.
//!
//! HTTP(S) requests run on a dedicated kernel thread ([`worker`]) instead of
//! inline in the compositor loop, so the UI keeps rendering during the slow
//! software TLS handshake. The compositor and worker communicate through a tiny
//! atomic state machine plus a few static mailboxes:
//!
//! ```text
//! IDLE --try_post--> REQUESTED --worker--> RUNNING --worker--> DONE --take_result--> IDLE
//! ```
//!
//! Only one fetch is ever in flight, and the NIC is touched solely by the worker
//! while a fetch runs (the main loop gates its ARP responder on [`is_idle`]), so
//! there is no concurrent access to the single NE2000 from the two threads.

use crate::sync::RacyCell;
use crate::{netstack, serial_println};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, Ordering};

const IDLE: u8 = 0;
const REQUESTED: u8 = 1;
const RUNNING: u8 = 2;
const DONE: u8 = 3;

const URL_CAP: usize = 512;

static STATE: AtomicU8 = AtomicU8::new(IDLE);
static NET: RacyCell<Option<netstack::Net>> = RacyCell::new(None);
static REQ_URL: RacyCell<[u8; URL_CAP]> = RacyCell::new([0; URL_CAP]);
static REQ_LEN: RacyCell<usize> = RacyCell::new(0);
static RESULT: RacyCell<Option<Vec<u8>>> = RacyCell::new(None);

/// Hand the network stack to the fetcher (call once, before spawning [`worker`]).
pub fn init(net: netstack::Net) {
    unsafe {
        *NET.get() = Some(net);
    }
}

/// True when no fetch is in flight — the main loop may safely poll the NIC for
/// its ARP/ping responder only in this state.
pub fn is_idle() -> bool {
    STATE.load(Ordering::Acquire) == IDLE
}

/// Queue a fetch for `url` if the worker is idle. Returns `true` if accepted.
pub fn try_post(url: &[u8]) -> bool {
    if STATE.load(Ordering::Acquire) != IDLE {
        return false;
    }
    let n = url.len().min(URL_CAP);
    unsafe {
        let buf = &mut *REQ_URL.get();
        buf[..n].copy_from_slice(&url[..n]);
        *REQ_LEN.get() = n;
    }
    STATE.store(REQUESTED, Ordering::Release);
    true
}

/// If a fetch has finished, return its result (`Some(bytes)` on success, `None`
/// on failure) and reset to idle. Yields `None` while nothing is ready.
pub fn take_result() -> Option<Option<Vec<u8>>> {
    if STATE.load(Ordering::Acquire) != DONE {
        return None;
    }
    let r = unsafe { (*RESULT.get()).take() };
    STATE.store(IDLE, Ordering::Release);
    Some(r)
}

/// Worker thread entry. Processes one queued request at a time and halts the CPU
/// while idle, so it yields the core to the compositor instead of busy-spinning.
pub extern "C" fn worker() -> ! {
    loop {
        if STATE.load(Ordering::Acquire) == REQUESTED {
            STATE.store(RUNNING, Ordering::Relaxed);
            let url = unsafe {
                let n = *REQ_LEN.get();
                let buf = &*REQ_URL.get();
                buf[..n].to_vec()
            };
            let result = match unsafe { (*NET.get()).as_mut() } {
                Some(net) => fetch_url(net, &url),
                None => None,
            };
            unsafe {
                *RESULT.get() = result;
            }
            STATE.store(DONE, Ordering::Release);
        } else {
            x86_64::instructions::hlt();
        }
    }
}

/// Resolve a URL and fetch it (HTTP or HTTPS), following up to 5 redirects.
/// Returns the final raw HTTP response, or `None` on failure.
fn fetch_url(net: &mut netstack::Net, url: &[u8]) -> Option<Vec<u8>> {
    use osjeff_core::browser::{header_value, parse_url, status_code};
    let mut cur: Vec<u8> = url.to_vec();

    for _hop in 0..5 {
        let u = parse_url(&cur)?;
        let (Ok(host), Ok(path)) = (
            core::str::from_utf8(u.host()),
            core::str::from_utf8(u.path()),
        ) else {
            return None;
        };
        serial_println!(
            "fetch: GET {}://{}{} :{}",
            if u.https { "https" } else { "http" },
            host,
            path,
            u.port
        );
        let resp = if u.https {
            net.https_get(host, path, u.port)
        } else {
            net.http_get(host, path, u.port)
        };
        let r = resp?;

        let code = status_code(&r).unwrap_or(0);
        if matches!(code, 301 | 302 | 303 | 307 | 308)
            && let Some(loc) = header_value(&r, b"location")
        {
            cur = resolve_redirect(host, loc);
            serial_println!("fetch: {} redirect", code);
            continue;
        }

        serial_println!("fetch: {} bytes (status {})", r.len(), code);
        return Some(r);
    }
    serial_println!("fetch: too many redirects");
    None
}

/// Build an absolute URL from a redirect `Location` value relative to `host`.
fn resolve_redirect(host: &str, loc: &[u8]) -> Vec<u8> {
    let ci = |p: &[u8]| loc.len() >= p.len() && loc[..p.len()].eq_ignore_ascii_case(p);
    let mut out = Vec::new();
    if ci(b"http://") || ci(b"https://") {
        out.extend_from_slice(loc);
    } else if loc.starts_with(b"//") {
        out.extend_from_slice(b"https:");
        out.extend_from_slice(loc);
    } else if loc.starts_with(b"/") {
        out.extend_from_slice(b"https://");
        out.extend_from_slice(host.as_bytes());
        out.extend_from_slice(loc);
    } else {
        out.extend_from_slice(b"https://");
        out.extend_from_slice(host.as_bytes());
        out.push(b'/');
        out.extend_from_slice(loc);
    }
    out
}
