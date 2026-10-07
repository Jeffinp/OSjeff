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
//! [`is_idle`] is false for good, so nothing posts to the dead thread. The NIC
//! stays with the dead worker's `Net` (nobody else may touch hardware it may have
//! left half-programmed), so the machine goes silent on the wire: no ARP or ping
//! answers either.
//! The main loop turns later navigations into the same error via [`worker_dead`].
//!
//! The worker thread is also the network owner: the NIC lives inside the `Net` it
//! holds (a `nic::Port`, moved in by [`init`]), so nothing else can reach the
//! hardware. Between fetches it wakes every few ticks to answer ARP and ping
//! (`Net::respond_idle`); the compositor never touches the NIC.

use crate::netd::Netd;
use crate::sync::RacyCell;
use crate::{netstack, serial_println};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};
use osjeff_core::browser::{Conn, FailReason};

const IDLE: u8 = 0;
const REQUESTED: u8 = 1;
const RUNNING: u8 = 2;
const DONE: u8 = 3;
/// The worker thread died; terminal (nothing leaves this state).
const WORKER_DEAD: u8 = 4;

const URL_CAP: usize = 512;

static STATE: AtomicU8 = AtomicU8::new(IDLE);
/// Scheduler slot of the worker thread (`usize::MAX` until it has started), so
/// [`try_post`] can wake it from its idle block.
static TID: AtomicUsize = AtomicUsize::new(usize::MAX);
/// No NIC was found: every navigation fails at once with a network error instead of
/// waiting for a worker that does not exist.
static OFFLINE: AtomicBool = AtomicBool::new(false);
static NET: RacyCell<Option<Netd>> = RacyCell::new(None);
static REQ_URL: RacyCell<[u8; URL_CAP]> = RacyCell::new([0; URL_CAP]);
static REQ_LEN: RacyCell<usize> = RacyCell::new(0);
/// Host the user explicitly allowed past a certificate error (empty = none). It
/// applies to that host only, on every hop of the navigation.
static REQ_INSECURE: RacyCell<[u8; INSECURE_CAP]> = RacyCell::new([0; INSECURE_CAP]);
static REQ_INSECURE_LEN: RacyCell<usize> = RacyCell::new(0);
const INSECURE_CAP: usize = 96;
static RESULT: RacyCell<Option<FetchResult>> = RacyCell::new(None);

/// A fetched page plus what the browser needs to describe it honestly.
pub struct Loaded {
    /// Raw HTTP response (headers + body), at most `MAX_RESPONSE_BYTES`.
    pub data: Vec<u8>,
    /// How the *final* connection (after redirects) was authenticated.
    pub conn: Conn,
    /// The response was cut at the size cap.
    pub truncated: bool,
}

/// Outcome of one navigation: the page, or why it failed.
pub type FetchResult = Result<Loaded, FailReason>;

/// Hand the network owner to the fetcher (call once, before spawning [`worker`]).
pub fn init(net: Netd) {
    // SAFETY: called once from `kernel_main` before the fetcher is spawned, so no other thread can
    // touch NET yet.
    unsafe {
        *NET.get() = Some(net);
    }
}

/// Record that there is no network interface: [`try_post`] then answers every request with
/// [`FailReason::Network`] right away (the browser shows the error instead of "Carregando").
pub fn init_offline() {
    OFFLINE.store(true, Ordering::Release);
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

/// Wake the worker from its idle block (a ping request is waiting for it).
pub fn wake_worker() {
    let tid = TID.load(Ordering::Acquire);
    if tid != usize::MAX {
        crate::sched::wake(tid);
    }
}

/// Queue a fetch for `url` if the worker is idle. Returns `true` if accepted.
/// `insecure_host` (usually empty) is the one host the user allowed to proceed
/// despite a certificate error, for this session.
pub fn try_post(url: &[u8], insecure_host: &[u8]) -> bool {
    if STATE.load(Ordering::Acquire) != IDLE || worker_dead() {
        return false;
    }
    if OFFLINE.load(Ordering::Acquire) {
        // SAFETY: STATE is IDLE (checked above) and no worker exists (OFFLINE), so nothing else touches
        // RESULT; the Release store of DONE below publishes it to `take_result`.
        unsafe {
            *RESULT.get() = Some(Err(FailReason::Network));
        }
        STATE.store(DONE, Ordering::Release);
        return true;
    }
    let n = url.len().min(URL_CAP);
    let m = insecure_host.len().min(INSECURE_CAP);
    // SAFETY: only the compositor posts, and only in IDLE (checked above), when the worker does not
    // touch REQ_URL/REQ_LEN/REQ_INSECURE*; the Release store of REQUESTED below publishes them.
    unsafe {
        let buf = &mut *REQ_URL.get();
        buf[..n].copy_from_slice(&url[..n]);
        *REQ_LEN.get() = n;
        let ib = &mut *REQ_INSECURE.get();
        ib[..m].copy_from_slice(&insecure_host[..m]);
        *REQ_INSECURE_LEN.get() = m;
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
    // Confirm the time over SNTP before anything else: certificate validity dates
    // are only as good as the clock. A request posted meanwhile waits (at most a
    // few seconds); if no server answers the RTC is used and the browser says
    // "hora nao confirmada" when a date check fails.
    // SAFETY: NET is set once before this thread exists and used only by this worker.
    if let Some(netd) = unsafe { (*NET.get()).as_mut() } {
        netd.net_mut().sync_time();
        LAST_RESYNC_MS.store(crate::netd::now_ms().max(1), Ordering::Relaxed);
    }
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
            // SAFETY: as for REQ_URL above (REQ_INSECURE* are written together with it).
            let insecure = unsafe {
                let n = *REQ_INSECURE_LEN.get();
                (&*REQ_INSECURE.get())[..n].to_vec()
            };
            // SAFETY: NET is set once, before this thread exists (`fetch::init`), and used only by this
            // worker, one request at a time, so the `&mut` is unique.
            let result = match unsafe { (*NET.get()).as_mut() } {
                Some(netd) => fetch_url(netd.net_mut(), &url, &insecure),
                None => Err(FailReason::Network),
            };
            // SAFETY: STATE is RUNNING here; the compositor only touches RESULT after seeing DONE, which is
            // stored (Release) right after this write.
            unsafe {
                *RESULT.get() = Some(result);
            }
            STATE.store(DONE, Ordering::Release);
        } else {
            // Idle: service the network (DHCP timers, ARP/ping responder, ping client),
            // then leave the run queue until a request is posted (`try_post` and
            // `ping_start` wake us) or the service interval ends. The scheduler
            // re-checks the condition after announcing the block, so a request posted
            // in between is not missed.
            // SAFETY: as above: NET is used only by this worker thread.
            let sleep = match unsafe { (*NET.get()).as_mut() } {
                Some(netd) => netd.service(),
                None => crate::netd::IDLE_POLL_TICKS,
            };
            crate::sched::block(crate::interrupts::ticks() + sleep, || {
                STATE.load(Ordering::Acquire) != REQUESTED && !crate::netd::ping_pending()
            });
        }
    }
}

/// Monotonic ms of the last SNTP attempt made on behalf of a page load.
static LAST_RESYNC_MS: AtomicU64 = AtomicU64::new(0);

/// Retry the SNTP sync before an HTTPS load when the clock is still
/// unconfirmed, rate-limited to one attempt per minute.
fn resync_clock_if_needed(net: &mut netstack::Net) {
    if crate::clock::confirmed() {
        return;
    }
    let now = crate::netd::now_ms();
    let last = LAST_RESYNC_MS.load(Ordering::Relaxed);
    if last != 0 && now.saturating_sub(last) < 60_000 {
        return;
    }
    LAST_RESYNC_MS.store(now.max(1), Ordering::Relaxed);
    net.sync_time();
}

/// Resolve a URL and fetch it (HTTP or HTTPS), following up to
/// [`osjeff_core::redirect::MAX_REDIRECTS`] redirects. Returns the final page,
/// or why the navigation failed. Redirect policy (scheme kept, https -> http
/// refused, loops, bad `Location` values) lives in `osjeff_core::redirect`.
fn fetch_url(net: &mut netstack::Net, url: &[u8], insecure_host: &[u8]) -> FetchResult {
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
        let (r, conn) = if u.https {
            // Certificate dates need a confirmed clock: if the boot-time SNTP did not
            // succeed, try again (at most once a minute) before the handshake.
            resync_clock_if_needed(net);
            // Validation is skipped only for the one host the user allowed.
            let allow = !insecure_host.is_empty() && u.host().eq_ignore_ascii_case(insecure_host);
            net.https_get(host, path, u.port, allow)?
        } else {
            (net.http_get(host, path, u.port)?, Conn::Plain)
        };

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
            conn,
            truncated: r.truncated,
        });
    }
}
