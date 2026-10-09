//! The system sampler behind the graphs: what the main loop hands over once a second and the history it keeps.

use crate::desktop::kit::{self};
use crate::desktop::*;
use kitsune_core::activity::{LoadAvg, Rate};
use kitsune_core::sysif::NetStats;
use kitsune_core::sysmon::{CpuSample, CpuSampler, MAX_THREADS, Series};

/// Milliseconds the glide between two samples takes.
pub(crate) const ANIM_MS: u32 = 450;

/// What the compositor loop hands over once a second.
pub struct SysInputs {
    /// Timer ticks since boot.
    pub ticks: u64,
    /// Per-slot cumulative "ticks found running" from the scheduler.
    pub busy: [u64; MAX_THREADS],
    pub heap_used: usize,
    pub heap_total: usize,
    /// Frames drawn in the last second / duration of the last / worst this second.
    pub fps: u32,
    pub frame_us: u64,
    pub max_us: u64,
    pub tsc_khz: u64,
}

/// The sampled history behind the graphs.
pub(crate) struct SysMon {
    sampler: CpuSampler,
    pub last: CpuSample,
    /// The sample before `last` (the animation starts from it).
    pub prev: CpuSample,
    pub cpu_total: Series,
    pub heap: Series,
    pub heap_used: usize,
    pub heap_prev: usize,
    pub heap_total: usize,
    rx: Rate,
    tx: Rate,
    pub rx_rate: u64,
    pub tx_rate: u64,
    pub rx_prev: u64,
    pub tx_prev: u64,
    pub net_rx: Series,
    pub net_tx: Series,
    disk_r: Rate,
    disk_w: Rate,
    pub disk_rd_rate: u64,
    pub disk_wr_rate: u64,
    pub disk_rd_prev: u64,
    pub disk_wr_prev: u64,
    pub disk_rd: Series,
    pub disk_wr: Series,
    pub load: LoadAvg,
    pub frame_us: Series,
    pub frame_now_us: u64,
    pub frame_max_us: u64,
    pub fps: u32,
    pub uptime_s: u64,
    pub tsc_khz: u64,
    /// Milliseconds since the last sample (stepped by the animation clock).
    pub age_ms: u32,
    /// Samples taken so far.
    pub samples: u32,
}

impl SysMon {
    pub(crate) const fn new() -> Self {
        let idle = CpuSample {
            thread_pm: [0; MAX_THREADS],
            busy_pm: 0,
            idle_pm: 1000,
        };
        Self {
            sampler: CpuSampler::new(),
            last: idle,
            prev: idle,
            cpu_total: Series::new(),
            heap: Series::new(),
            heap_used: 0,
            heap_prev: 0,
            heap_total: 0,
            rx: Rate::new(64),
            tx: Rate::new(64),
            rx_rate: 0,
            tx_rate: 0,
            rx_prev: 0,
            tx_prev: 0,
            net_rx: Series::new(),
            net_tx: Series::new(),
            disk_r: Rate::new(64),
            disk_w: Rate::new(64),
            disk_rd_rate: 0,
            disk_wr_rate: 0,
            disk_rd_prev: 0,
            disk_wr_prev: 0,
            disk_rd: Series::new(),
            disk_wr: Series::new(),
            load: LoadAvg::new(),
            frame_us: Series::new(),
            frame_now_us: 0,
            frame_max_us: 0,
            fps: 0,
            uptime_s: 0,
            tsc_khz: 1,
            age_ms: u32::MAX / 2,
            samples: 0,
        }
    }

    /// Take one sample (call once per second).
    pub(crate) fn sample(&mut self, i: &SysInputs) {
        let t_ms = i.ticks.saturating_mul(1000) / crate::interrupts::TIMER_HZ as u64;
        self.uptime_s = t_ms / 1000;
        self.tsc_khz = i.tsc_khz.max(1);
        self.prev = self.last;
        self.last = self.sampler.sample(i.ticks, &i.busy);
        self.cpu_total.push(self.last.busy_pm as u32);
        self.load.feed(self.last.busy_pm as u32);
        self.heap_prev = self.heap_used;
        self.heap_used = i.heap_used;
        self.heap_total = i.heap_total;
        self.heap.push((i.heap_used / 1024) as u32);
        self.rx_prev = self.rx_rate;
        self.tx_prev = self.tx_rate;
        match KernelNetStats.counters() {
            Some(n) => {
                self.rx_rate = self.rx.feed(n.rx_bytes, t_ms);
                self.tx_rate = self.tx.feed(n.tx_bytes, t_ms);
            }
            None => {
                self.rx_rate = 0;
                self.tx_rate = 0;
            }
        }
        self.net_rx.push(self.rx_rate.min(u32::MAX as u64) as u32);
        self.net_tx.push(self.tx_rate.min(u32::MAX as u64) as u32);
        let (rd, wr) = crate::ata::io_bytes();
        self.disk_rd_prev = self.disk_rd_rate;
        self.disk_wr_prev = self.disk_wr_rate;
        self.disk_rd_rate = self.disk_r.feed(rd, t_ms);
        self.disk_wr_rate = self.disk_w.feed(wr, t_ms);
        self.disk_rd
            .push(self.disk_rd_rate.min(u32::MAX as u64) as u32);
        self.disk_wr
            .push(self.disk_wr_rate.min(u32::MAX as u64) as u32);
        self.fps = i.fps;
        self.frame_us.push(i.frame_us.min(u32::MAX as u64) as u32);
        self.frame_now_us = i.frame_us;
        self.frame_max_us = i.max_us;
        self.age_ms = 0;
        self.samples = self.samples.saturating_add(1);
    }

    /// Progress of the glide since the last sample, 0..=256 (256 = settled), eased out.
    pub(crate) fn t_q8(&self) -> i64 {
        if kitsune_core::anim::reduce_motion() || self.samples < 2 {
            return 256;
        }
        kit::ease_out((self.age_ms.min(ANIM_MS) as i64 * 256) / ANIM_MS as i64)
    }

    /// Is the glide still running?
    pub(crate) fn gliding(&self) -> bool {
        !kitsune_core::anim::reduce_motion() && self.samples >= 2 && self.age_ms < ANIM_MS
    }

    /// Peak heap use among the samples kept (KiB resolution).
    pub(crate) fn heap_peak(&self) -> u64 {
        self.heap.max() as u64 * 1024
    }

    /// Heap use now, glided from the previous sample.
    pub(super) fn heap_now(&self) -> u64 {
        kit::lerp(self.heap_prev as i64, self.heap_used as i64, self.t_q8()) as u64
    }
}
