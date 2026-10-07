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
    // SAFETY: called once from `kernel_main` before the fetcher is spawned, so no other thread can
    // touch NET yet.
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
    // SAFETY: only the compositor posts, and only in IDLE (checked above), when the worker does not
    // touch REQ_URL/REQ_LEN; the Release store of REQUESTED below publishes them.
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
    // SAFETY: STATE == DONE (Acquire) means the worker finished writing RESULT and will not touch it
    // until the compositor sets IDLE, which happens after this take (compositor is the only caller).
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
            // SAFETY: STATE == REQUESTED (Acquire) pairs with `try_post`'s Release, so REQ_URL/REQ_LEN are
            // complete; the compositor will not rewrite them until DONE -> IDLE.
            let url = unsafe {
                let n = *REQ_LEN.get();
                let buf = &*REQ_URL.get();
                buf[..n].to_vec()
            };
            // SAFETY: NET is set once, before this thread exists (`fetch::init`), and used only by this
            // worker, one request at a time, so the `&mut` is unique.
            // NOTE: the NIC underneath is shared with the compositor's ARP responder; exclusion comes from
            // `STATE`/`is_idle()`, not from the type.
            let result = match unsafe { (*NET.get()).as_mut() } {
                Some(net) => fetch_url(net, &url),
                None => None,
            };
            // SAFETY: STATE is RUNNING here; the compositor only touches RESULT after seeing DONE, which is
            // stored (Release) right after this write.
            unsafe {
                *RESULT.get() = result;
            }
            STATE.store(DONE, Ordering::Release);
        } else {
            x86_64::instructions::hlt();
        }
    }
}

/// Resolve a URL and fetch it (HTTP or HTTPS), following up to
/// [`osjeff_core::redirect::MAX_REDIRECTS`] redirects. Returns the final raw
/// HTTP response, or `None` on failure. Redirect policy (scheme kept,
/// https -> http refused, loops, bad `Location` values) lives in
/// `osjeff_core::redirect`.
fn fetch_url(net: &mut netstack::Net, url: &[u8]) -> Option<Vec<u8>> {
    use osjeff_core::browser::{header_value, parse_url, status_code};
    use osjeff_core::redirect::Redirects;
    let mut cur: Vec<u8> = url.to_vec();
    let mut chain: Option<Redirects> = None;

    loop {
        let u = parse_url(&cur)?;
        let chain = chain.get_or_insert_with(|| Redirects::new(&u));
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
        let resp = resp?;
        let truncated = resp.truncated;
        let r = resp.data;

        let code = status_code(&r).unwrap_or(0);
        if matches!(code, 301 | 302 | 303 | 307 | 308)
            && let Some(loc) = header_value(&r, b"location")
        {
            match chain.follow(&u, loc) {
                Ok(next) => {
                    cur = next;
                    serial_println!("fetch: {} redirect", code);
                    continue;
                }
                Err(e) => {
                    serial_println!("fetch: {} redirect refused: {:?}", code, e);
                    return None;
                }
            }
        }

        serial_println!(
            "fetch: {} bytes (status {}){}",
            r.len(),
            code,
            if truncated { " truncated" } else { "" }
        );
        return Some(r);
    }
}
