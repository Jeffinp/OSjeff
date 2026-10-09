//! Kernel entropy: the sources, the IRQ-safe sample ring and the one generator
//! everything random in the kernel draws from ([`fill`], [`u64`], ...).
//!
//! The pure half (pool, ChaCha20 DRBG, credit accounting, quality rating, reseed
//! policy) is `kitsune_core::entropy`, host-tested. This file is the glue:
//!
//! * **Hardware**: `RDSEED` and `RDRAND` when CPUID says they exist (retried, and the
//!   output checked against the known-bad all-zero / all-ones values), and the virtio-rng
//!   device (`virtio_rng.rs`). Pulled at boot and again at each periodic reseed.
//! * **Timing**: the TSC at each timer / keyboard / mouse interrupt and at each received
//!   frame goes into [`sample`], a lock-free ring of raw timestamps. The ring is folded
//!   into the pool from *thread* context ([`service`], [`fill`]), where the estimator and
//!   SHA-256 may take as long as they like.
//! * **Uniqueness only**: boot TSC, tick count and the RTC are hashed in with no credit.
//!
//! # Interrupt rules
//!
//! [`sample`] is the only function an ISR may call. It does one `rdtsc`, one atomic
//! `fetch_add` and one atomic store into a static array: no allocation, no lock (the
//! `lock xadd` is a CPU instruction, not a spin lock), no logging. Everything else
//! runs in thread context; the generator's state is protected by switching interrupts
//! off for short, bounded sections (the same mutual exclusion `klog` uses on this
//! single-core kernel), so a thread preempted inside never leaves it locked.
//!
//! # Policy
//!
//! * `Strong` / `Mixed`: carry on; [`service`] logs the transition once (no repeating toast).
//! * `Weak`: [`fill`] still answers (a DHCP transaction id does not need secrecy), but
//!   TLS must call [`wait_ready`] first, which waits a bounded time for 128 credited
//!   bits (collecting CPU-jitter samples while it waits) and returns `false`, with a
//!   warning, if they do not come. `docs/design/entropy.md` has the reasoning.

use crate::sync::RacyCell;
use crate::virtio_rng::VirtioRng;
use crate::{interrupts, io};
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use kitsune_core::entropy::{Entropy, MIN_SEED_BITS, Quality, source};

// Source ids for `sample`, so callers do not import `kitsune_core::entropy::source` themselves.
pub use kitsune_core::entropy::source::{KEYBOARD, MOUSE, NIC, TIMER};
use kitsune_core::rng as hw;

// ---------------------------------------------------------------- IRQ-safe sample ring

/// Slots in the ring (a power of two, so the index wraps cleanly with the counter).
const RING_N: usize = 256;
/// Set in every stored sample; a slot holding 0 is empty / already consumed.
const VALID: u64 = 1 << 55;
const TS_MASK: u64 = VALID - 1;

static RING: [AtomicU64; RING_N] = [const { AtomicU64::new(0) }; RING_N];
/// Next slot to write (producers, any context). Counts forever; the slot is `% RING_N`.
static HEAD: AtomicUsize = AtomicUsize::new(0);
/// Samples overwritten before they were folded (a burst bigger than the ring).
static LOST: AtomicU64 = AtomicU64::new(0);

/// Record "event `src` happened now" for the entropy pool.
///
/// Safe from an interrupt handler: one `rdtsc`, one atomic increment, one atomic store into a
/// static array. No allocation, no lock, no I/O. Several producers (the timer ISR, the keyboard
/// ISR, a thread reading frames) may race: each gets its own slot from the atomic counter.
/// A producer that is preempted between taking its slot and storing leaves it empty for the
/// consumer, which skips empty slots.
#[inline]
pub fn sample(src: u8) {
    let packed = (u64::from(src) << 56) | VALID | (io::rdtsc() & TS_MASK);
    let i = HEAD.fetch_add(1, Ordering::Relaxed);
    RING[i % RING_N].store(packed, Ordering::Release);
}

// ---------------------------------------------------------------- generator state

struct State {
    ent: Entropy,
    /// Next ring index the consumer reads.
    tail: usize,
    rdseed: bool,
    rdrand: bool,
    vrng: Option<VirtioRng>,
    /// Consecutive blocks each instruction failed to deliver a usable block (retries exhausted, or
    /// the block failed the sanity check); the source is dropped after `MAX_HW_FAILS` in a row.
    rdseed_fails: u8,
    rdrand_fails: u8,
    /// Last time hardware sources were pulled (ms), and whether a virtio request is out.
    last_pull_ms: u64,
    /// Highest rating already announced in the log.
    announced: Quality,
}

static STATE: RacyCell<Option<State>> = RacyCell::new(None);

/// Failed blocks in a row after which `RDSEED` / `RDRAND` is no longer asked.
const MAX_HW_FAILS: u8 = 3;

fn now_ms() -> u64 {
    interrupts::ticks() * 1000 / u64::from(interrupts::TIMER_HZ)
}

/// Run `f` on the generator with interrupts off (creating it on first use, unseeded).
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    x86_64::instructions::interrupts::without_interrupts(|| {
        // SAFETY: single core and interrupts are off for the whole closure, so no other thread
        // (and no ISR, none of which touches STATE: they only call `sample`) can hold a reference
        // to it meanwhile; this is the only live one.
        let slot = unsafe { &mut *STATE.get() };
        let st = slot.get_or_insert_with(State::boot);
        f(st)
    })
}

impl State {
    /// An unseeded generator: the boot noise (TSC, tick count) makes its output differ from
    /// boot to boot but earns no credit.
    fn boot() -> State {
        let mut noise = [0u8; 24];
        noise[..8].copy_from_slice(&io::rdtsc().to_le_bytes());
        noise[8..16].copy_from_slice(&interrupts::ticks().to_le_bytes());
        noise[16..24].copy_from_slice(&(core::ptr::addr_of!(STATE) as u64).to_le_bytes());
        State {
            ent: Entropy::new(&noise),
            // From the start: samples the timer ISR stored before the generator existed count too
            // (`fold` skips empty slots and clamps to the last `RING_N`).
            tail: 0,
            rdseed: false,
            rdrand: false,
            vrng: None,
            rdseed_fails: 0,
            rdrand_fails: 0,
            last_pull_ms: 0,
            announced: Quality::Weak,
        }
    }

    /// Fold up to `max` ring samples into the pool; returns how many were read.
    fn fold(&mut self, max: usize) -> usize {
        let head = HEAD.load(Ordering::Acquire);
        let behind = head.wrapping_sub(self.tail);
        if behind > RING_N {
            LOST.fetch_add((behind - RING_N) as u64, Ordering::Relaxed);
            self.tail = head.wrapping_sub(RING_N);
        }
        let mut n = 0;
        while self.tail != head && n < max {
            let v = RING[self.tail % RING_N].swap(0, Ordering::Acquire);
            self.tail = self.tail.wrapping_add(1);
            n += 1;
            if v & VALID != 0 {
                self.ent.sample((v >> 56) as u8, v & TS_MASK);
            }
        }
        n
    }

    /// Draw hardware entropy into the pool; refreshes `last_pull_ms`.
    fn pull_hardware(&mut self, now: u64) {
        self.last_pull_ms = now;
        self.pull_cpu();
        self.drain_virtio();
        if let Some(v) = self.vrng.as_mut() {
            v.request();
        }
    }

    /// One block each from `RDSEED` and `RDRAND` (when present and not dropped).
    fn pull_cpu(&mut self) {
        if self.rdseed && self.rdseed_fails < MAX_HW_FAILS {
            match rdseed_block() {
                Some(b) => {
                    self.rdseed_fails = 0;
                    self.ent.add(source::RDSEED, &b, 256);
                }
                None => self.rdseed_fails += 1,
            }
        }
        if self.rdrand && self.rdrand_fails < MAX_HW_FAILS {
            // RDRAND is a DRBG reseeded from the DRNG at a ratio: read 512 bits, credit 256.
            match rdrand_block() {
                Some(b) => {
                    self.rdrand_fails = 0;
                    self.ent.add(source::RDRAND, &b, 256);
                }
                None => self.rdrand_fails += 1,
            }
        }
    }

    /// Take the virtio-rng answer if the device has produced one.
    fn drain_virtio(&mut self) {
        let Some(v) = self.vrng.as_mut() else { return };
        if let Some((b, n)) = v.poll() {
            self.ent
                .add(source::VIRTIO_RNG, &b[..n], device_credit(&b[..n]));
        }
    }

    /// Fold, pull hardware when due, reseed when the policy says so. Returns the rating
    /// to announce if it just improved.
    fn step(&mut self, now: u64) -> Option<(Quality, [u32; 2])> {
        while self.fold(64) == 64 {}
        self.drain_virtio();
        let hw_due = now.saturating_sub(self.last_pull_ms)
            >= kitsune_core::entropy::RESEED_INTERVAL_MS
            && self.ent.quality() != Quality::Weak;
        if hw_due {
            self.pull_hardware(now);
        }
        self.ent.maybe_reseed(now);
        let q = self.ent.quality();
        if q > self.announced {
            self.announced = q;
            let (h, t) = self.ent.seeded_bits();
            return Some((q, [h, t]));
        }
        None
    }
}

/// Bits to credit for `b` (<= 32 bytes) delivered by the virtio-rng device: 8 per byte, unless the
/// block is constant or contains a known-bad word, in which case the device is not trusted for it.
fn device_credit(b: &[u8]) -> u32 {
    let mut w = [0u64; 4];
    let n = b.len().div_ceil(8).min(4);
    for (slot, c) in w.iter_mut().zip(b.chunks(8)) {
        let mut x = [0u8; 8];
        x[..c.len()].copy_from_slice(c);
        *slot = u64::from_le_bytes(x);
    }
    if hw::plausible_block(&w[..n]) {
        (b.len() as u32) * 8
    } else {
        0
    }
}

fn announce(q: Option<(Quality, [u32; 2])>) {
    match q {
        Some((Quality::Strong, [h, t])) => {
            crate::serial_println!(
                "RNG: strong ({} bits from hardware, {} from timing credited) at {} ms",
                h,
                t,
                now_ms()
            );
        }
        Some((Quality::Mixed, [h, t])) => {
            // No hardware generator vouched for the seed: say what it is made of, once, as a
            // plain INFO line (nothing here is a warning, so no toast).
            crate::serial_println!(
                "RNG: pool seeded from timing jitter ({} bits credited{}) at {} ms",
                h + t,
                if h > 0 { ", some hardware" } else { "" },
                now_ms()
            );
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- hardware instructions

fn cpuid_max_leaf() -> u32 {
    // CPUID leaf 0 exists on every x86_64 CPU and has no side effects.
    core::arch::x86_64::__cpuid(0).eax
}

fn cpuid_01h_ecx() -> u32 {
    // CPUID leaf 1 exists on every x86_64 CPU and has no side effects.
    core::arch::x86_64::__cpuid(1).ecx
}

fn cpuid_07h_ebx() -> u32 {
    // Only called after `cpuid_max_leaf() >= 7`, so leaf 7 exists; no side effects.
    core::arch::x86_64::__cpuid_count(7, 0).ebx
}

/// One `RDRAND` attempt: `Some` on success, `None` if the hardware was not ready (carry
/// flag clear). Only call after CPUID reported RDRAND.
fn rdrand64() -> Option<u64> {
    #[target_feature(enable = "rdrand")]
    fn step() -> Option<u64> {
        let mut v = 0u64;
        // (Safe to call here: the `rdrand` target feature is enabled on `step`.)
        let ok = core::arch::x86_64::_rdrand64_step(&mut v);
        (ok == 1).then_some(v)
    }
    // SAFETY: only reached when CPUID.01H:ECX[30] is set (`State::rdrand`), so the instruction
    // exists on this CPU.
    unsafe { step() }
}

/// One `RDSEED` attempt, as [`rdrand64`]. Only call after CPUID reported RDSEED.
fn rdseed64() -> Option<u64> {
    #[target_feature(enable = "rdseed")]
    fn step() -> Option<u64> {
        let mut v = 0u64;
        // (Safe to call here: the `rdseed` target feature is enabled on `step`.)
        let ok = core::arch::x86_64::_rdseed64_step(&mut v);
        (ok == 1).then_some(v)
    }
    // SAFETY: only reached when CPUID.(07H,0):EBX[18] is set (`State::rdseed`), so the instruction
    // exists on this CPU.
    unsafe { step() }
}

/// `W` words from `step`, each retried `tries` times, as bytes; `None` if any word never came
/// or the block fails the sanity check (all zero / all ones / stuck), after up to 3 blocks.
fn hw_block<const W: usize>(tries: usize, step: fn() -> Option<u64>) -> Option<[u8; 32]> {
    for _ in 0..3 {
        let mut w = [0u64; W];
        for slot in w.iter_mut() {
            *slot = hw::retry_n(tries, || {
                let v = step();
                if v.is_none() {
                    core::hint::spin_loop();
                }
                v
            })?;
        }
        if hw::plausible_block(&w) {
            // Fold W words down to 32 bytes by XOR so 512 bits of RDRAND output become 256.
            let mut out = [0u8; 32];
            for (i, x) in w.iter().enumerate() {
                let b = x.to_le_bytes();
                for (j, v) in b.iter().enumerate() {
                    out[(i * 8 + j) % 32] ^= v;
                }
            }
            return Some(out);
        }
    }
    None
}

fn rdseed_block() -> Option<[u8; 32]> {
    hw_block::<4>(hw::RDSEED_RETRIES, rdseed64)
}

fn rdrand_block() -> Option<[u8; 32]> {
    hw_block::<8>(hw::RDRAND_RETRIES, rdrand64)
}

// ---------------------------------------------------------------- public API

/// Bring the generator up: probe the hardware sources, take the first draw, hash in the
/// boot values and log what was found. Call once at boot, after interrupts are on (so the
/// timer samples start flowing) and before any thread other than the boot thread exists
/// (it reads the CMOS RTC). Safe to call before the NIC probe: DHCP already uses [`u32`].
pub fn init(phys_offset: Option<u64>) {
    let max_leaf = cpuid_max_leaf();
    let rdrand = hw::has_rdrand(cpuid_01h_ecx());
    let rdseed = hw::has_rdseed(max_leaf, if max_leaf >= 7 { cpuid_07h_ebx() } else { 0 });
    let vrng = phys_offset.and_then(VirtioRng::probe);
    let rtc = crate::rtc::now_unix();
    let now = now_ms();
    let (announce_now, found) = with_state(|st| {
        st.rdrand = rdrand;
        st.rdseed = rdseed;
        st.vrng = vrng;
        st.ent.add(source::RTC, &rtc.to_le_bytes(), 0);
        st.ent.add(source::BOOT, &io::rdtsc().to_le_bytes(), 0);
        if let Some(v) = st.vrng.as_mut() {
            v.request();
            // The device answers from the host's main loop: give it a bounded moment (100 ms).
            let end = interrupts::ticks() + u64::from(interrupts::TIMER_HZ) / 10;
            while interrupts::ticks() < end {
                if let Some((b, n)) = v.poll() {
                    st.ent
                        .add(source::VIRTIO_RNG, &b[..n], device_credit(&b[..n]));
                    v.request();
                    break;
                }
                core::hint::spin_loop();
            }
        }
        st.last_pull_ms = now;
        st.pull_cpu();
        let a = st.step(now);
        let yn = |present: bool, fails: u8| if present && fails == 0 { "yes" } else { "no" };
        (
            a,
            [
                yn(st.rdseed, st.rdseed_fails),
                yn(st.rdrand, st.rdrand_fails),
                yn(st.vrng.is_some(), 0),
            ],
        )
    });
    crate::serial_println!(
        "RNG: sources rdseed={} rdrand={} virtio-rng={}",
        found[0],
        found[1],
        found[2]
    );
    announce(announce_now);
    if quality() == Quality::Weak {
        crate::serial_println!(
            "RNG: no hardware generator; collecting timing jitter ({} bits credited needed)",
            MIN_SEED_BITS
        );
    }
}

/// Housekeeping for a thread that has a spare moment (the network owner's idle loop):
/// fold the sample ring, refresh from hardware and reseed when due, announce a rating
/// change. Cheap when there is nothing to do.
pub fn service() {
    let now = now_ms();
    let a = with_state(|st| st.step(now));
    announce(a);
}

/// Fill `out` with random bytes from the kernel generator. Never blocks. Whether the
/// bytes are fit for a secret is [`quality`]; TLS must go through [`wait_ready`] first.
pub fn fill(out: &mut [u8]) {
    for chunk in out.chunks_mut(256) {
        let now = now_ms();
        let a = with_state(|st| {
            let a = st.step(now);
            st.ent.fill(chunk, now);
            a
        });
        announce(a);
    }
}

/// A random `u64` (see [`fill`]).
pub fn u64() -> u64 {
    let mut b = [0u8; 8];
    fill(&mut b);
    u64::from_le_bytes(b)
}

/// A random `u32` (see [`fill`]).
pub fn u32() -> u32 {
    let mut b = [0u8; 4];
    fill(&mut b);
    u32::from_le_bytes(b)
}

/// Current rating of the generator.
pub fn quality() -> Quality {
    with_state(|st| st.ent.quality())
}

/// A few hundred CPU-timing samples: tiny memory-dependent loops, each followed by a TSC
/// read. The estimator in `kitsune_core::entropy` credits only samples whose timing
/// differences vary (and 0.1 bit each at most); a deterministic emulator earns nothing.
fn collect_cpu_jitter(rounds: usize) {
    let mut buf = [0u8; 1024];
    let mut x = io::rdtsc() | 1;
    for _ in 0..rounds {
        for _ in 0..48 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let i = (x as usize) & (buf.len() - 1);
            buf[i] = buf[i].wrapping_add(x as u8);
        }
        sample(source::ACTIVE_JITTER);
    }
    core::hint::black_box(&buf);
}

/// How long HTTPS waits for the generator to leave `Weak` before it gives up.
pub const TLS_WAIT_MS: u64 = 5_000;

static LAST_REFUSAL_MS: AtomicU64 = AtomicU64::new(0);

/// Block the calling thread (up to `max_ms`) until the generator holds at least 128
/// credited bits. `true` at once when it already does. While waiting it collects CPU-timing
/// samples and lets the other threads run. Returns `false` (after one warning, which is the
/// only RNG toast) if the credit did not arrive: the caller must not start a handshake.
pub fn wait_ready(max_ms: u64) -> bool {
    service();
    if quality() != Quality::Weak {
        return true;
    }
    let (h, t) = with_state(|st| st.ent.pending_bits());
    crate::serial_println!(
        "RNG: waiting up to {} ms for {} credited bits ({} so far)",
        max_ms,
        MIN_SEED_BITS,
        h + t
    );
    let end = interrupts::ticks() + max_ms * u64::from(interrupts::TIMER_HZ) / 1000;
    while interrupts::ticks() < end {
        collect_cpu_jitter(96);
        service();
        if quality() != Quality::Weak {
            return true;
        }
        crate::sched::block(interrupts::ticks() + 1, || true);
    }
    let (h, t) = with_state(|st| st.ent.pending_bits());
    let now = now_ms();
    let last = LAST_REFUSAL_MS.load(Ordering::Relaxed);
    if last == 0 || now.saturating_sub(last) >= 30_000 {
        LAST_REFUSAL_MS.store(now.max(1), Ordering::Relaxed);
        crate::klog!(
            Warn,
            "RNG: weak, only {} of {} bits credited; HTTPS refused until entropy arrives",
            h + t,
            MIN_SEED_BITS
        );
    }
    false
}

/// `rand_core` adapter over [`fill`] for `embedded-tls`.
///
/// `CryptoRng` is a marker the TLS crate requires. It is honest here because every TLS
/// handshake is gated by [`wait_ready`] (see `Net::https_get`): this type is only
/// constructed after the generator rated `Mixed` or better.
pub struct KernelRng;

impl rand_core::RngCore for KernelRng {
    fn next_u32(&mut self) -> u32 {
        u32()
    }

    fn next_u64(&mut self) -> u64 {
        u64()
    }

    fn fill_bytes(&mut self, dst: &mut [u8]) {
        fill(dst);
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), rand_core::Error> {
        fill(dst);
        Ok(())
    }
}

impl rand_core::CryptoRng for KernelRng {}
