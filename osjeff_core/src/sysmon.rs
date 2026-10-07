//! Pure data structures of the resource monitor: the sample ring behind every
//! line graph, automatic scale, moving average, the CPU-share computation from
//! scheduler tick counters, process rows with sorting, and the number
//! formatters ("1.5 MiB", "12.3%", "01:02:03").
//!
//! The kernel samples once per second ([`Series::push`], [`CpuSampler::sample`])
//! and the window only reads. Everything is integer arithmetic (the kernel is
//! soft-float) and allocation-free except [`sort_rows`] callers' own `Vec`.

use crate::klog::FixedBuf;
use core::fmt::Write;

/// Samples kept per graph: the last 60 seconds at one sample per second.
pub const HIST: usize = 60;
/// Scheduler slots tracked (the kernel's `MAX_THREADS`).
pub const MAX_THREADS: usize = 8;

/// Ring of the last [`HIST`] samples.
#[derive(Clone, Copy, Debug)]
pub struct Series {
    buf: [u32; HIST],
    /// Where the next sample goes.
    head: usize,
    len: usize,
}

impl Default for Series {
    fn default() -> Self {
        Self::new()
    }
}

impl Series {
    pub const fn new() -> Self {
        Self {
            buf: [0; HIST],
            head: 0,
            len: 0,
        }
    }

    pub fn push(&mut self, v: u32) {
        self.buf[self.head] = v;
        self.head = (self.head + 1) % HIST;
        self.len = (self.len + 1).min(HIST);
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Sample `i`, `0` = oldest kept.
    pub fn get(&self, i: usize) -> Option<u32> {
        if i >= self.len {
            return None;
        }
        Some(self.buf[(self.head + HIST - self.len + i) % HIST])
    }

    /// Newest sample.
    pub fn last(&self) -> Option<u32> {
        self.len.checked_sub(1).and_then(|i| self.get(i))
    }

    pub fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        (0..self.len).filter_map(move |i| self.get(i))
    }

    /// Largest kept sample (0 when empty).
    pub fn max(&self) -> u32 {
        self.iter().max().unwrap_or(0)
    }

    /// Mean of the kept samples (0 when empty).
    pub fn avg(&self) -> u32 {
        if self.len == 0 {
            return 0;
        }
        (self.iter().map(u64::from).sum::<u64>() / self.len as u64) as u32
    }

    /// Mean of the `window` samples ending at `i` (fewer at the start).
    pub fn moving_avg(&self, i: usize, window: usize) -> u32 {
        if i >= self.len || window == 0 {
            return 0;
        }
        let from = (i + 1).saturating_sub(window);
        let n = i + 1 - from;
        let sum: u64 = (from..=i).filter_map(|k| self.get(k)).map(u64::from).sum();
        (sum / n as u64) as u32
    }
}

/// Smallest "nice" ceiling (1, 2, 5 times a power of ten) that is at least
/// `max` and at least `floor`, for the automatic Y axis of a graph.
pub fn nice_ceiling(max: u64, floor: u64) -> u64 {
    let want = max.max(floor).max(1);
    let mut mag = 1u64;
    loop {
        for m in [1u64, 2, 5] {
            let c = m.saturating_mul(mag);
            if c >= want || c == u64::MAX {
                return c;
            }
        }
        mag = mag.saturating_mul(10);
    }
}

/// Pixel height of `value` on an axis `0..ceiling` drawn `h` pixels tall.
pub fn scale_to(value: u64, ceiling: u64, h: u32) -> u32 {
    if ceiling == 0 {
        return 0;
    }
    (value.min(ceiling).saturating_mul(h as u64) / ceiling) as u32
}

/// Turns monotonic counters into per-second rates.
#[derive(Clone, Copy, Debug, Default)]
pub struct RateMeter {
    last: Option<u64>,
}

impl RateMeter {
    pub const fn new() -> Self {
        Self { last: None }
    }

    /// Feed the counter read `dt_s` seconds after the previous read. The first
    /// call (and a counter that went backwards) returns 0.
    pub fn rate(&mut self, now: u64, dt_s: u64) -> u64 {
        let r = match self.last {
            Some(prev) if now >= prev => (now - prev) / dt_s.max(1),
            _ => 0,
        };
        self.last = Some(now);
        r
    }
}

/// CPU shares over one sampling window, in tenths of a percent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CpuSample {
    /// Share of each scheduler slot (ticks that found it running).
    pub thread_pm: [u16; MAX_THREADS],
    /// Sum of the slots (the busy share of the machine).
    pub busy_pm: u16,
    /// What is left: the CPU sat in `hlt`. `busy_pm + idle_pm == 1000`.
    pub idle_pm: u16,
}

/// Computes [`CpuSample`]s from the scheduler's cumulative per-slot tick
/// counters and the timer's tick count.
#[derive(Clone, Copy, Debug)]
pub struct CpuSampler {
    last_busy: [u64; MAX_THREADS],
    last_total: u64,
    primed: bool,
}

impl Default for CpuSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl CpuSampler {
    pub const fn new() -> Self {
        Self {
            last_busy: [0; MAX_THREADS],
            last_total: 0,
            primed: false,
        }
    }

    /// `total_ticks` is the timer tick count now, `busy` the per-slot counters.
    /// The first call only primes the baseline and reports an idle machine.
    pub fn sample(&mut self, total_ticks: u64, busy: &[u64; MAX_THREADS]) -> CpuSample {
        let mut out = CpuSample {
            idle_pm: 1000,
            ..CpuSample::default()
        };
        let total = total_ticks.saturating_sub(self.last_total);
        let was_primed = self.primed;
        let mut delta = [0u64; MAX_THREADS];
        for i in 0..MAX_THREADS {
            delta[i] = busy[i].saturating_sub(self.last_busy[i]);
        }
        self.last_busy = *busy;
        self.last_total = total_ticks;
        self.primed = true;
        if !was_primed || total == 0 {
            return out;
        }
        // Counters are read without stopping the timer: if the slots add up to
        // slightly more than the elapsed ticks, stretch the window to fit.
        let sum: u64 = delta.iter().sum();
        let total = total.max(sum);
        // Total busy share rounded to nearest; each slot gets its floor and the
        // leftover tenths go to the slots with the largest remainders, so the
        // slots add up to exactly the busy share (and busy + idle = 1000).
        let busy_pm = ((sum * 1000 + total / 2) / total).min(1000) as u32;
        let mut given = 0u32;
        let mut rem = [0u64; MAX_THREADS];
        for i in 0..MAX_THREADS {
            let scaled = delta[i] * 1000;
            out.thread_pm[i] = (scaled / total) as u16;
            rem[i] = scaled % total;
            given += out.thread_pm[i] as u32;
        }
        while given < busy_pm {
            let best = (0..MAX_THREADS).max_by_key(|&i| rem[i]).unwrap_or(0);
            out.thread_pm[best] += 1;
            rem[best] = 0;
            given += 1;
        }
        let busy_pm = given;
        out.busy_pm = busy_pm as u16;
        out.idle_pm = (1000 - busy_pm) as u16;
        out
    }
}

// ------------------------------------------------------------------ process rows

/// What a row of the process list is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    /// A kernel thread (real CPU share, stack size).
    Thread,
    /// An app window's process entry.
    App,
    /// The `kernel` / `compositor` bookkeeping entries.
    System,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowState {
    Running,
    Idle,
    Suspended,
    Ended,
    Dead,
}

impl RowState {
    pub const fn label(self) -> &'static str {
        match self {
            RowState::Running => "RUN",
            RowState::Idle => "IDLE",
            RowState::Suspended => "SUS",
            RowState::Ended => "END",
            RowState::Dead => "DEAD",
        }
    }
}

/// One line of the monitor's process list.
#[derive(Clone, Copy, Debug)]
pub struct ProcRow {
    name: [u8; 16],
    name_len: u8,
    pub kind: RowKind,
    /// Process-table pid (0 for threads).
    pub pid: u16,
    pub state: RowState,
    /// CPU share in tenths of a percent; `None` for rows with no real figure.
    pub cpu_pm: Option<u16>,
    /// Seconds since the row's process / thread started.
    pub up_s: u32,
    /// Memory in KiB when the kernel knows it (thread stacks), else `None`.
    pub mem_kib: Option<u32>,
}

impl ProcRow {
    pub fn new(name: &[u8], kind: RowKind, pid: u16, state: RowState) -> Self {
        let n = name.len().min(16);
        let mut b = [0u8; 16];
        b[..n].copy_from_slice(&name[..n]);
        Self {
            name: b,
            name_len: n as u8,
            kind,
            pid,
            state,
            cpu_pm: None,
            up_s: 0,
            mem_kib: None,
        }
    }

    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }
}

/// Column to sort the list by.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortKey {
    Name,
    Cpu,
    Mem,
    Up,
}

impl SortKey {
    pub const ALL: [SortKey; 4] = [SortKey::Name, SortKey::Cpu, SortKey::Mem, SortKey::Up];
}

/// Sort rows in place by `key` (stable; rows without a figure sort as 0).
/// Ties keep the previous order, so the list does not jitter between samples.
pub fn sort_rows(rows: &mut [ProcRow], key: SortKey, descending: bool) {
    // Insertion sort: the list has at most a few dozen rows, and it needs no heap.
    for i in 1..rows.len() {
        let mut j = i;
        while j > 0 && out_of_order(&rows[j - 1], &rows[j], key, descending) {
            rows.swap(j - 1, j);
            j -= 1;
        }
    }
}

fn out_of_order(a: &ProcRow, b: &ProcRow, key: SortKey, desc: bool) -> bool {
    use core::cmp::Ordering;
    let ord = match key {
        SortKey::Name => a
            .name()
            .iter()
            .map(u8::to_ascii_lowercase)
            .cmp(b.name().iter().map(u8::to_ascii_lowercase)),
        SortKey::Cpu => a.cpu_pm.unwrap_or(0).cmp(&b.cpu_pm.unwrap_or(0)),
        SortKey::Mem => a.mem_kib.unwrap_or(0).cmp(&b.mem_kib.unwrap_or(0)),
        SortKey::Up => a.up_s.cmp(&b.up_s),
    };
    if desc {
        ord == Ordering::Less
    } else {
        ord == Ordering::Greater
    }
}

// -------------------------------------------------------------------- formatting

/// `"12.3%"` from tenths of a percent.
pub fn fmt_pct10(pm: u32) -> FixedBuf<8> {
    let mut b = FixedBuf::new();
    let _ = write!(b, "{}.{}%", pm / 10, pm % 10);
    b
}

/// `"512 B"`, `"1.5 KiB"`, `"12.0 MiB"`, `"3.2 GiB"`.
pub fn fmt_bytes(v: u64) -> FixedBuf<16> {
    let mut b = FixedBuf::new();
    const K: u64 = 1024;
    if v < K {
        let _ = write!(b, "{v} B");
        return b;
    }
    let (div, unit) = if v < K * K {
        (K, "KiB")
    } else if v < K * K * K {
        (K * K, "MiB")
    } else {
        (K * K * K, "GiB")
    };
    let tenths = v.saturating_mul(10) / div;
    let _ = write!(b, "{}.{} {unit}", tenths / 10, tenths % 10);
    b
}

/// `"12.3 KiB/s"` for a byte rate.
pub fn fmt_rate(bytes_per_s: u64) -> FixedBuf<20> {
    let mut b = FixedBuf::new();
    let _ = write!(b, "{}/s", fmt_bytes(bytes_per_s));
    b
}

/// `"02:03:04"`, or `"1d 02:03:04"` past a day.
pub fn fmt_uptime(secs: u64) -> FixedBuf<16> {
    let mut b = FixedBuf::new();
    let (d, h, m, s) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        let _ = write!(b, "{d}d {h:02}:{m:02}:{s:02}");
    } else {
        let _ = write!(b, "{h:02}:{m:02}:{s:02}");
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn series_ring_keeps_the_last_60() {
        let mut s = Series::new();
        assert!(s.is_empty());
        assert_eq!(s.last(), None);
        for i in 0..100u32 {
            s.push(i);
        }
        assert_eq!(s.len(), HIST);
        assert_eq!(s.get(0), Some(40));
        assert_eq!(s.last(), Some(99));
        assert_eq!(s.get(HIST), None);
        assert_eq!(s.max(), 99);
        let v: Vec<u32> = s.iter().collect();
        assert_eq!(v.len(), 60);
        assert!(v.windows(2).all(|w| w[1] == w[0] + 1));
    }

    #[test]
    fn series_stats() {
        let mut s = Series::new();
        for v in [10, 20, 30] {
            s.push(v);
        }
        assert_eq!(s.avg(), 20);
        assert_eq!(s.max(), 30);
        assert_eq!(s.moving_avg(2, 2), 25);
        assert_eq!(s.moving_avg(0, 5), 10);
        assert_eq!(s.moving_avg(2, 10), 20);
        assert_eq!(s.moving_avg(3, 2), 0);
        assert_eq!(s.moving_avg(1, 0), 0);
        assert_eq!(Series::new().avg(), 0);
        assert_eq!(Series::new().max(), 0);
    }

    #[test]
    fn nice_ceilings() {
        assert_eq!(nice_ceiling(0, 0), 1);
        assert_eq!(nice_ceiling(1, 0), 1);
        assert_eq!(nice_ceiling(3, 0), 5);
        assert_eq!(nice_ceiling(5, 0), 5);
        assert_eq!(nice_ceiling(6, 0), 10);
        assert_eq!(nice_ceiling(101, 0), 200);
        assert_eq!(nice_ceiling(4_999, 0), 5_000);
        assert_eq!(nice_ceiling(20, 100), 100);
        assert_eq!(nice_ceiling(u64::MAX, 0), u64::MAX);
        assert!(nice_ceiling(u64::MAX / 2, 0) >= u64::MAX / 2);
    }

    #[test]
    fn scaling() {
        assert_eq!(scale_to(50, 100, 80), 40);
        assert_eq!(scale_to(500, 100, 80), 80);
        assert_eq!(scale_to(5, 0, 80), 0);
    }

    #[test]
    fn rate_meter() {
        let mut m = RateMeter::new();
        assert_eq!(m.rate(1000, 1), 0);
        assert_eq!(m.rate(1600, 1), 600);
        assert_eq!(m.rate(2200, 2), 300);
        assert_eq!(m.rate(10, 1), 0); // counter reset
        assert_eq!(m.rate(40, 1), 30);
    }

    #[test]
    fn cpu_first_sample_is_idle() {
        let mut s = CpuSampler::new();
        let r = s.sample(1000, &[500, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(r.idle_pm, 1000);
        assert_eq!(r.busy_pm, 0);
    }

    #[test]
    fn cpu_shares_sum_to_100_percent() {
        let mut s = CpuSampler::new();
        s.sample(0, &[0; 8]);
        // 250 ticks: thread 0 ran 100, thread 1 ran 50, thread 2 ran 25.
        let r = s.sample(250, &[100, 50, 25, 0, 0, 0, 0, 0]);
        assert_eq!(r.thread_pm[0], 400);
        assert_eq!(r.thread_pm[1], 200);
        assert_eq!(r.thread_pm[2], 100);
        assert_eq!(r.busy_pm, 700);
        assert_eq!(r.idle_pm, 300);
        let total: u32 = r.thread_pm.iter().map(|&p| p as u32).sum::<u32>() + r.idle_pm as u32;
        assert_eq!(total, 1000);
    }

    #[test]
    fn cpu_sum_is_exact_for_awkward_splits() {
        let mut s = CpuSampler::new();
        s.sample(0, &[0; 8]);
        let r = s.sample(3, &[1, 1, 1, 0, 0, 0, 0, 0]);
        let total: u32 = r.thread_pm.iter().map(|&p| p as u32).sum::<u32>() + r.idle_pm as u32;
        assert_eq!(total, 1000);
        assert_eq!(r.idle_pm, 0);
    }

    #[test]
    fn cpu_over_count_is_clamped() {
        let mut s = CpuSampler::new();
        s.sample(0, &[0; 8]);
        // Slots add up to more ticks than elapsed (racy read): still <= 100%.
        let r = s.sample(100, &[60, 60, 0, 0, 0, 0, 0, 0]);
        assert!(r.busy_pm <= 1000);
        assert_eq!(r.busy_pm + r.idle_pm, 1000);
    }

    #[test]
    fn cpu_zero_elapsed_and_counter_regression() {
        let mut s = CpuSampler::new();
        s.sample(10, &[5; 8]);
        let r = s.sample(10, &[9; 8]);
        assert_eq!(r.idle_pm, 1000);
        let r = s.sample(20, &[0; 8]); // counters went backwards
        assert_eq!(r.busy_pm, 0);
    }

    fn row(name: &str, cpu: Option<u16>, mem: Option<u32>, up: u32) -> ProcRow {
        let mut r = ProcRow::new(name.as_bytes(), RowKind::App, 1, RowState::Running);
        r.cpu_pm = cpu;
        r.mem_kib = mem;
        r.up_s = up;
        r
    }

    fn names(rows: &[ProcRow]) -> Vec<&[u8]> {
        rows.iter().map(|r| r.name()).collect()
    }

    #[test]
    fn sorting() {
        let mut rows = [
            row("shell", Some(100), None, 5),
            row("Editor", Some(300), Some(128), 50),
            row("calc", None, Some(256), 20),
        ];
        sort_rows(&mut rows, SortKey::Name, false);
        assert_eq!(names(&rows), [&b"calc"[..], b"Editor", b"shell"]);
        sort_rows(&mut rows, SortKey::Cpu, true);
        assert_eq!(names(&rows), [&b"Editor"[..], b"shell", b"calc"]);
        sort_rows(&mut rows, SortKey::Mem, true);
        assert_eq!(names(&rows), [&b"calc"[..], b"Editor", b"shell"]);
        sort_rows(&mut rows, SortKey::Up, false);
        assert_eq!(names(&rows), [&b"shell"[..], b"calc", b"Editor"]);
    }

    #[test]
    fn sorting_is_stable() {
        let mut rows = [
            row("a", Some(10), None, 0),
            row("b", Some(10), None, 0),
            row("c", Some(10), None, 0),
        ];
        sort_rows(&mut rows, SortKey::Cpu, true);
        assert_eq!(names(&rows), [&b"a"[..], b"b", b"c"]);
        sort_rows(&mut rows, SortKey::Cpu, false);
        assert_eq!(names(&rows), [&b"a"[..], b"b", b"c"]);
    }

    #[test]
    fn row_name_is_cut() {
        let r = ProcRow::new(
            b"a-very-long-process-name",
            RowKind::Thread,
            0,
            RowState::Idle,
        );
        assert_eq!(r.name().len(), 16);
        assert_eq!(RowState::Dead.label(), "DEAD");
    }

    #[test]
    fn formatting() {
        assert_eq!(fmt_pct10(0).as_bytes(), b"0.0%");
        assert_eq!(fmt_pct10(123).as_bytes(), b"12.3%");
        assert_eq!(fmt_pct10(1000).as_bytes(), b"100.0%");
        assert_eq!(fmt_bytes(0).as_bytes(), b"0 B");
        assert_eq!(fmt_bytes(1023).as_bytes(), b"1023 B");
        assert_eq!(fmt_bytes(1024).as_bytes(), b"1.0 KiB");
        assert_eq!(fmt_bytes(1536).as_bytes(), b"1.5 KiB");
        assert_eq!(fmt_bytes(64 * 1024 * 1024).as_bytes(), b"64.0 MiB");
        assert_eq!(fmt_bytes(3 << 30).as_bytes(), b"3.0 GiB");
        assert_eq!(fmt_rate(2048).as_bytes(), b"2.0 KiB/s");
        assert_eq!(fmt_uptime(0).as_bytes(), b"00:00:00");
        assert_eq!(fmt_uptime(3725).as_bytes(), b"01:02:05");
        assert_eq!(fmt_uptime(90_061).as_bytes(), b"1d 01:01:01");
        // Never panics on the extremes.
        let _ = fmt_bytes(u64::MAX);
        let _ = fmt_uptime(u64::MAX);
    }
}
