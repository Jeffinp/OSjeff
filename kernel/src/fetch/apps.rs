//! The app side of the fetcher: `net_http_get` from a WASM app, policed before it is posted.

use super::*;

// ---- apps ----

/// Why an app's request produced no response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppFetchError {
    /// No NIC.
    NoNetwork,
    /// The request slot stayed busy (browser or another app) for the whole budget.
    Busy,
    /// The request was too long for the mailbox.
    TooLong,
    /// Nothing came back within the budget; the late result will be dropped.
    Timeout,
    /// The fetcher thread died.
    Died,
    /// The transfer failed (DNS, refused, TLS or certificate error, redirect refused...).
    Failed(FailReason),
}

/// Free a result nobody will collect (its app timed out).
pub(super) fn reap_abandoned() {
    if STATE.load(Ordering::Acquire) == DONE
        && WHO.load(Ordering::Acquire) == WHO_ABANDONED
        && STATE
            .compare_exchange(DONE, CLAIMED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        // SAFETY: the DONE -> CLAIMED exchange above was won by this thread, so no other thread
        // touches RESULT until the slot is IDLE again (stored below, after the take).
        unsafe {
            drop((*RESULT.get()).take());
        }
        WHO.store(WHO_BROWSER, Ordering::Release);
        STATE.store(IDLE, Ordering::Release);
    }
}

/// `GET url` for a WASM app and wait for it (called on the `appd` thread, which sleeps
/// meanwhile: the compositor and the other threads keep running, the other apps wait).
/// `perm` and `hosts` are the app's permission and allow-list: the caller already
/// authorized `url`, the worker authorizes it again and every redirect hop. The raw
/// HTTP response comes back (decode it with `appnet::app_response`). `budget_ms` bounds
/// the wait for the slot **and** for the answer together.
pub fn app_get(
    url: &[u8],
    perm: NetPerm,
    hosts: &[String],
    budget_ms: u64,
) -> Result<Loaded, AppFetchError> {
    if OFFLINE.load(Ordering::Acquire) {
        return Err(AppFetchError::NoNetwork);
    }
    if url.len() > URL_CAP {
        return Err(AppFetchError::TooLong);
    }
    let hz = u64::from(crate::interrupts::TIMER_HZ);
    let end = crate::interrupts::ticks() + budget_ms * hz / 1000;
    // 1. win the slot (the browser or another app may be using it).
    loop {
        if worker_dead() {
            return Err(AppFetchError::Died);
        }
        reap_abandoned();
        if STATE
            .compare_exchange(IDLE, CLAIMED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            break;
        }
        if crate::interrupts::ticks() >= end {
            return Err(AppFetchError::Busy);
        }
        let until = (crate::interrupts::ticks() + 2).min(end);
        crate::sched::block(until, || STATE.load(Ordering::Acquire) != IDLE);
    }
    WHO.store(WHO_APP, Ordering::Release);
    REQ_KIND.store(KIND_PAGE, Ordering::Relaxed);
    APP_TID.store(crate::sched::current(), Ordering::Release);
    // SAFETY: this thread holds the slot (CLAIMED, won above): the worker does not touch these until
    // the Release store of REQUESTED below, and no other poster can get past the exchange.
    unsafe {
        let buf = &mut *REQ_URL.get();
        buf[..url.len()].copy_from_slice(url);
        *REQ_LEN.get() = url.len();
        *REQ_INSECURE_LEN.get() = 0;
        *APP_POLICY.get() = Some(AppPolicy {
            perm,
            hosts: hosts.to_vec(),
        });
    }
    STATE.store(REQUESTED, Ordering::Release);
    wake_worker();
    // 2. wait for the answer.
    loop {
        if STATE.load(Ordering::Acquire) == DONE && WHO.load(Ordering::Acquire) == WHO_APP {
            // SAFETY: STATE == DONE (Acquire) means the worker finished writing RESULT and will not touch
            // it again; this app owns the request (WHO_APP), so it alone takes it.
            let r = unsafe { (*RESULT.get()).take() };
            WHO.store(WHO_BROWSER, Ordering::Release);
            STATE.store(IDLE, Ordering::Release);
            return match r {
                Some(Ok(l)) => Ok(l),
                Some(Err(e)) => Err(AppFetchError::Failed(e)),
                None => Err(AppFetchError::Failed(FailReason::Network)),
            };
        }
        if worker_dead() {
            return Err(AppFetchError::Died);
        }
        let now = crate::interrupts::ticks();
        if now >= end {
            // Give up. If the worker is still busy it will drop the result; if it just finished,
            // take it back ourselves.
            if WHO
                .compare_exchange(WHO_APP, WHO_ABANDONED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                reap_abandoned();
            }
            return Err(AppFetchError::Timeout);
        }
        let until = (now + 25).min(end);
        crate::sched::block(until, || STATE.load(Ordering::Acquire) != DONE);
    }
}
