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
//! If the worker thread dies (panic, CPU fault: see `sched::kill_current`) the
//! request in flight is answered with [`FailReason::WorkerDied`] by
//! [`take_result`], the machine moves to `WORKER_DEAD`, and from then on
//! [`is_idle`] is false for good, so nothing posts to the dead thread and the
//! compositor stops polling the NIC the worker may have left half-programmed.
//! The main loop turns later navigations into the same error via [`worker_dead`].
//!
//! The worker thread is also the network owner: the NIC lives inside the `Net` it
//! holds (a `nic::Port`, moved in by [`init`]), so nothing else can reach the
//! hardware. Between fetches it wakes every few ticks to answer ARP and ping
//! (`Net::respond_idle`); the compositor never touches the NIC.

use crate::sync::RacyCell;
use crate::{netstack, serial_println};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use osjeff_core::browser::FailReason;

const IDLE: u8 = 0;
const REQUESTED: u8 = 1;
const RUNNING: u8 = 2;
const DONE: u8 = 3;
/// The worker thread died; terminal (nothing leaves this state).
const WORKER_DEAD: u8 = 4;

const URL_CAP: usize = 512;

/// While idle the worker wakes this often (ticks of 4 ms) to service the NIC.
const IDLE_POLL_TICKS: u64 = 4;
/// Frames answered per idle wake: a flood must not starve the rest of the system.
const IDLE_RX_BUDGET: usize = 32;

static STATE: AtomicU8 = AtomicU8::new(IDLE);
/// Scheduler slot of the worker thread (`usize::MAX` until it has started), so
/// [`try_post`] can wake it from its idle block.
static TID: AtomicUsize = AtomicUsize::new(usize::MAX);
static NET: RacyCell<Option<netstack::Net>> = RacyCell::new(None);
static REQ_URL: RacyCell<[u8; URL_CAP]> = RacyCell::new([0; URL_CAP]);
static REQ_LEN: RacyCell<usize> = RacyCell::new(0);
static RESULT: RacyCell<Option<FetchResult>> = RacyCell::new(None);

/// A fetched page plus what the browser needs to describe it honestly.
pub struct Loaded {
    /// Raw HTTP response (headers + body), at most `MAX_RESPONSE_BYTES`.
    pub data: Vec<u8>,
    /// Scheme of the *final* URL, after redirects.
    pub https: bool,
    /// The response was cut at the size cap.
    pub truncated: bool,
}

/// Outcome of one navigation: the page, or why it failed.
pub type FetchResult = Result<Loaded, FailReason>;

/// Hand the network stack to the fetcher (call once, before spawning [`worker`]).
pub fn init(net: netstack::Net) {
    // SAFETY: called once from `kernel_main` before the fetcher is spawned, so no other thread can
    // touch NET yet.
    unsafe {
        *NET.get() = Some(net);
    }
}

/// True when no fetch is in flight and the worker is alive, so a navigation
/// request can be posted.
pub fn is_idle() -> bool {
    STATE.load(Ordering::Acquire) == IDLE && !worker_dead()
}

/// True once the worker thread has died. It never comes back: every later
/// navigation has to fail instead of waiting for a thread that no longer exists.
pub fn worker_dead() -> bool {
    let tid = TID.load(Ordering::Acquire);
    STATE.load(Ordering::Acquire) == WORKER_DEAD
        || (tid != usize::MAX && crate::sched::is_dead(tid))
}

/// Queue a fetch for `url` if the worker is idle. Returns `true` if accepted.
pub fn try_post(url: &[u8]) -> bool {
    if STATE.load(Ordering::Acquire) != IDLE || worker_dead() {
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
    let tid = TID.load(Ordering::Acquire);
    if tid != usize::MAX {
        crate::sched::wake(tid);
    }
    true
}

/// If a fetch has finished, return its result and reset to idle. Yields `None`
/// while nothing is ready.
pub fn take_result() -> Option<FetchResult> {
    let state = STATE.load(Ordering::Acquire);
    if matches!(state, REQUESTED | RUNNING) && worker_dead() {
        // The worker died with a request in flight: it will never answer.
        STATE.store(WORKER_DEAD, Ordering::Release);
        return Some(Err(FailReason::WorkerDied));
    }
    if state != DONE {
        return None;
    }
    // SAFETY: STATE == DONE (Acquire) means the worker finished writing RESULT and will not touch it
    // until the compositor sets IDLE, which happens after this take (compositor is the only caller).
    let r = unsafe { (*RESULT.get()).take() };
    STATE.store(IDLE, Ordering::Release);
    Some(r.unwrap_or(Err(FailReason::Network)))
}

/// Worker thread entry. Processes one queued request at a time and, while idle,
/// is *blocked* in the scheduler (it costs no time slice at all) until
/// [`try_post`] wakes it.
pub extern "C" fn worker() -> ! {
    TID.store(crate::sched::current(), Ordering::Release);
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
            let result = match unsafe { (*NET.get()).as_mut() } {
                Some(net) => fetch_url(net, &url),
                None => Err(FailReason::Network),
            };
            // SAFETY: STATE is RUNNING here; the compositor only touches RESULT after seeing DONE, which is
            // stored (Release) right after this write.
            unsafe {
                *RESULT.get() = Some(result);
            }
            STATE.store(DONE, Ordering::Release);
        } else {
            // Idle: answer ARP/ping for our address, then leave the run queue until
            // a request is posted (`try_post` wakes us) or the poll interval ends.
            // The scheduler re-checks the condition after announcing the block, so a
            // request posted in between is not missed.
            // SAFETY: as above: NET is used only by this worker thread.
            if let Some(net) = unsafe { (*NET.get()).as_mut() } {
                net.respond_idle(IDLE_RX_BUDGET);
            }
            crate::sched::block(crate::interrupts::ticks() + IDLE_POLL_TICKS, || {
                STATE.load(Ordering::Acquire) != REQUESTED
            });
        }
    }
}

/// Resolve a URL and fetch it (HTTP or HTTPS), following up to
/// [`osjeff_core::redirect::MAX_REDIRECTS`] redirects. Returns the final page,
/// or why the navigation failed. Redirect policy (scheme kept, https -> http
/// refused, loops, bad `Location` values) lives in `osjeff_core::redirect`.
fn fetch_url(net: &mut netstack::Net, url: &[u8]) -> FetchResult {
    use osjeff_core::browser::{header_value, parse_url, status_code};
    use osjeff_core::redirect::Redirects;
    let mut cur: Vec<u8> = url.to_vec();
    let mut chain: Option<Redirects> = None;

    loop {
        let u = parse_url(&cur).ok_or(FailReason::Network)?;
        let chain = chain.get_or_insert_with(|| Redirects::new(&u));
        let (Ok(host), Ok(path)) = (
            core::str::from_utf8(u.host()),
            core::str::from_utf8(u.path()),
        ) else {
            return Err(FailReason::Network);
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
        let r = resp.ok_or(FailReason::Network)?;

        let code = status_code(&r.data).unwrap_or(0);
        if matches!(code, 301 | 302 | 303 | 307 | 308)
            && let Some(loc) = header_value(&r.data, b"location")
        {
            match chain.follow(&u, loc) {
                Ok(next) => {
                    cur = next;
                    serial_println!("fetch: {} redirect", code);
                    continue;
                }
                Err(e) => {
                    serial_println!("fetch: {} redirect refused: {:?}", code, e);
                    return Err(FailReason::from_redirect(e));
                }
            }
        }

        serial_println!(
            "fetch: {} bytes (status {}){}",
            r.data.len(),
            code,
            if r.truncated { " truncated" } else { "" }
        );
        return Ok(Loaded {
            data: r.data,
            https: u.https,
            truncated: r.truncated,
        });
    }
}
