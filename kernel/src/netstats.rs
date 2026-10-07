//! Cumulative network counters (bytes and frames each way) kept by the NIC
//! driver and read by the resource monitor through `osjeff_core::sysif::NetStats`.
//!
//! Plain relaxed atomics: the compositor and the fetcher thread both use the
//! NIC (never at once, see `fetch`), and a counter that is read a moment late is
//! harmless. The network front that adds richer statistics (drops, errors, per
//! protocol) replaces this behind the same trait.

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering::Relaxed};
use osjeff_core::sysif::NetCounters;

static RX_BYTES: AtomicU64 = AtomicU64::new(0);
static TX_BYTES: AtomicU64 = AtomicU64::new(0);
static RX_FRAMES: AtomicU64 = AtomicU64::new(0);
static TX_FRAMES: AtomicU64 = AtomicU64::new(0);
static NIC_PRESENT: AtomicBool = AtomicBool::new(false);

/// Record whether a NIC was brought up (called once at boot).
pub fn set_nic_present(up: bool) {
    NIC_PRESENT.store(up, Relaxed);
}

/// One frame of `len` bytes was received.
#[inline]
pub fn record_rx(len: usize) {
    RX_BYTES.fetch_add(len as u64, Relaxed);
    RX_FRAMES.fetch_add(1, Relaxed);
}

/// One frame of `len` bytes was sent.
#[inline]
pub fn record_tx(len: usize) {
    TX_BYTES.fetch_add(len as u64, Relaxed);
    TX_FRAMES.fetch_add(1, Relaxed);
}

/// The counters, or `None` when there is no NIC.
pub fn counters() -> Option<NetCounters> {
    NIC_PRESENT.load(Relaxed).then(|| NetCounters {
        rx_bytes: RX_BYTES.load(Relaxed),
        tx_bytes: TX_BYTES.load(Relaxed),
        rx_frames: RX_FRAMES.load(Relaxed),
        tx_frames: TX_FRAMES.load(Relaxed),
    })
}
