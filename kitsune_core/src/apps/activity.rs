//! Pure helpers of the **Tarefas** app (the activity monitor): friendly process names,
//! number formatting in the language in effect, the history ring's interpolation for smooth graphs,
//! an exponential smoother, rates from wrapping counters, load averages, the memory
//! pressure level and the sortable process table.
//!
//! Everything is integer arithmetic (the kernel has no FPU in its hot paths) and
//! allocation-light; the kernel owns the sampling and the drawing.

use crate::i18n::{self, Arg, locale};
use crate::system::klog::FixedBuf;
use crate::system::sysmon::Series;
use crate::tk;
use alloc::string::String;
use core::fmt::Write;

// ------------------------------------------------------------------ names

/// The name a person sees for an internal thread or process name. Numbered instances
/// (`shell 2`) keep their number (`Terminal 2`). Unknown names are returned as they
/// are, so nothing disappears from the list.
pub fn friendly_name(raw: &[u8]) -> String {
    let (base, num) = split_number(raw);
    // Catalog keys; the text is looked up in the language in effect.
    let fixed: Option<&str> = match base {
        b"compositor" => Some(tk!("tasks.name.interface")),
        b"fetcher" => Some(tk!("tasks.name.net_fetch")),
        b"appd" => Some(tk!("tasks.name.apps")),
        b"shelld" | b"shelld2" => Some(tk!("tasks.name.shell_exec")),
        b"logd" => Some(tk!("app.log")),
        b"kernel" => Some(tk!("tasks.name.system")),
        b"(idle)" => Some(tk!("tasks.name.idle")),
        b"shell" => Some(tk!("app.terminal")),
        b"editor" => Some(tk!("app.editor")),
        b"taskmgr" | b"monitor" => Some(tk!("app.tasks")),
        b"calc" => Some(tk!("app.calculator")),
        b"browser" => Some(tk!("app.browser")),
        b"wasmapp" => Some(tk!("app.wasm_title")),
        b"files" => Some(tk!("app.files")),
        b"settings" => Some(tk!("app.settings")),
        b"syslog" => Some(tk!("app.log")),
        b"viewer" => Some(tk!("app.viewer")),
        b"gallery" => Some(tk!("app.gallery")),
        _ => None,
    };
    let mut out = String::new();
    match fixed {
        Some(key) => out.push_str(i18n::tr(key)),
        None => {
            // Raw names are ASCII in practice; anything else is shown 1:1 as Latin-1.
            out.extend(base.iter().map(|&b| b as char));
        }
    }
    if let Some(n) = num {
        let _ = write!(out, " {n}");
    }
    out
}

/// `"shell 2"` -> (`"shell"`, `Some(2)`); anything else -> (`name`, `None`).
fn split_number(raw: &[u8]) -> (&[u8], Option<u32>) {
    if let Some(sp) = raw.iter().rposition(|&b| b == b' ') {
        let tail = &raw[sp + 1..];
        if !tail.is_empty() && tail.len() <= 3 && tail.iter().all(u8::is_ascii_digit) {
            let n = tail.iter().fold(0u32, |a, &d| a * 10 + (d - b'0') as u32);
            return (&raw[..sp], Some(n));
        }
    }
    (raw, None)
}

/// Is `raw` the internal name of a system thread or process (one the user should
/// think twice before ending)?
pub fn is_system_name(raw: &[u8]) -> bool {
    let (base, _) = split_number(raw);
    matches!(
        base,
        b"compositor" | b"fetcher" | b"appd" | b"shelld" | b"shelld2" | b"logd" | b"kernel"
    )
}

// ------------------------------------------------------------------ formatting

/// `12,3%` (`12.3%` in English) from tenths of a percent.
pub fn fmt_pct(pm: u32) -> FixedBuf<12> {
    let mut b = FixedBuf::new();
    let _ = write!(
        b,
        "{}{}{}%",
        pm / 10,
        locale::decimal_sep(i18n::lang()),
        pm % 10
    );
    b
}

/// `12%` from tenths of a percent (rounded), for places too narrow for a decimal.
pub fn fmt_pct_int(pm: u32) -> FixedBuf<8> {
    let mut b = FixedBuf::new();
    let _ = write!(b, "{}%", (pm + 5) / 10);
    b
}

/// `512 B`, `1,5 KiB`, `12,0 MiB`, `3,2 GiB` (`1.5 KiB` in English).
pub fn fmt_size(v: u64) -> FixedBuf<20> {
    let mut b = FixedBuf::new();
    let _ = locale::write_size(&mut b, i18n::lang(), v);
    b
}

/// `2,0 KiB/s` for a byte rate.
pub fn fmt_speed(bytes_per_s: u64) -> FixedBuf<24> {
    let mut b = FixedBuf::new();
    let _ = write!(b, "{}/s", fmt_size(bytes_per_s));
    b
}

/// A whole number with the language's thousands separator: `12.345` / `12,345`.
pub fn fmt_count(v: u64) -> FixedBuf<28> {
    let mut b = FixedBuf::new();
    let _ = locale::write_int(
        &mut b,
        i18n::lang(),
        i64::try_from(v).unwrap_or(i64::MAX),
        true,
    );
    b
}

/// A catalog text filled with `args`, in a fixed buffer.
fn fill<const N: usize>(key: &str, args: &[(&str, Arg<'_>)]) -> FixedBuf<N> {
    let mut b = FixedBuf::new();
    let _ = b.write_str(&i18n::tr_fmt(key, args));
    b
}

/// Uptime for people: `42 s`, `3 min 05 s`, `1 h 02 min`, `2 d 03 h`.
pub fn fmt_elapsed(secs: u64) -> FixedBuf<20> {
    let int = |n: u64| Arg::Int(i64::try_from(n).unwrap_or(i64::MAX));
    let (h24, m60, s60) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if secs >= 86_400 {
        fill(
            tk!("tasks.fmt.dh"),
            &[("d", int(secs / 86_400)), ("h", Arg::Pad(h24, 2))],
        )
    } else if secs >= 3600 {
        fill(
            tk!("tasks.fmt.hm"),
            &[("h", int(secs / 3600)), ("m", Arg::Pad(m60, 2))],
        )
    } else if secs >= 60 {
        fill(
            tk!("tasks.fmt.ms"),
            &[("m", int(secs / 60)), ("s", Arg::Pad(s60, 2))],
        )
    } else {
        fill(tk!("tasks.fmt.s"), &[("s", int(secs))])
    }
}

/// `01:02:03` clock-style uptime, with `d` days in front past one day.
pub fn fmt_clock(secs: u64) -> FixedBuf<20> {
    let (d, h, m, s) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        let d = Arg::Int(i64::try_from(d).unwrap_or(i64::MAX));
        fill(
            tk!("tasks.fmt.clock_days"),
            &[
                ("d", d),
                ("h", Arg::Pad(h, 2)),
                ("m", Arg::Pad(m, 2)),
                ("s", Arg::Pad(s, 2)),
            ],
        )
    } else {
        let mut b = FixedBuf::new();
        let _ = write!(b, "{h:02}:{m:02}:{s:02}");
        b
    }
}

/// `há 12 s` / `12 s ago` for a graph's hover label (`0` is "agora" / "now").
pub fn fmt_ago(secs: u32) -> FixedBuf<16> {
    if secs == 0 {
        fill(tk!("tasks.fmt.now"), &[])
    } else {
        fill(tk!("tasks.fmt.ago"), &[("n", Arg::Int(i64::from(secs)))])
    }
}

/// A log timestamp (milliseconds since boot) as `12,345` seconds, or `3:25,100` once past
/// a minute and `1:02:03,400` past an hour.
pub fn fmt_log_time(ms: u32) -> FixedBuf<16> {
    let mut b = FixedBuf::new();
    let (h, m, s, ms) = (ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000);
    let dec = locale::decimal_sep(i18n::lang());
    if h > 0 {
        let _ = write!(b, "{h}:{m:02}:{s:02}{dec}{ms:03}");
    } else if m > 0 {
        let _ = write!(b, "{m}:{s:02}{dec}{ms:03}");
    } else {
        let _ = write!(b, "{s}{dec}{ms:03}");
    }
    b
}

/// The catalog key of the chip text of a log level.
pub const fn level_key(l: crate::system::klog::Level) -> &'static str {
    use crate::system::klog::Level;
    match l {
        Level::Trace => tk!("log.chip.trace"),
        Level::Debug => tk!("log.chip.debug"),
        Level::Info => tk!("log.chip.info"),
        Level::Warn => tk!("log.chip.warn"),
        Level::Error => tk!("log.chip.error"),
        Level::Fatal => tk!("log.chip.fatal"),
    }
}

/// The chip text of a log level, in the language in effect.
pub fn level_name(l: crate::system::klog::Level) -> &'static str {
    i18n::tr(level_key(l))
}

/// `0,42` from thousandths (the load average).
pub fn fmt_milli(v: u32) -> FixedBuf<12> {
    let mut b = FixedBuf::new();
    let _ = write!(
        b,
        "{}{}{:02}",
        v / 1000,
        locale::decimal_sep(i18n::lang()),
        v % 1000 / 10
    );
    b
}

// ------------------------------------------------------------------ smoothing

/// Move `cur` toward `target` (both Q8 fixed point: 256 = 1.0 of whatever the value
/// is) as an exponential approach with time constant `tau_ms` over `dt_ms`. Reaches
/// the target exactly once the remaining gap is below one step, so it always settles.
pub fn ease_toward(cur: i32, target: i32, dt_ms: u32, tau_ms: u32) -> i32 {
    if cur == target {
        return cur;
    }
    // k = dt / (tau + dt), a stable one-pole step for any dt.
    let k = (dt_ms as u64 * 256 / (tau_ms as u64 + dt_ms as u64).max(1)) as i64;
    let gap = (target - cur) as i64;
    let step = gap * k / 256;
    let next = cur as i64 + if step == 0 { gap.signum() } else { step };
    // Never overshoot.
    let next = if gap > 0 {
        next.min(target as i64)
    } else {
        next.max(target as i64)
    };
    next as i32
}

/// A value that glides toward a target (Q8), used for bars and gauges.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Glide {
    cur: i32,
    target: i32,
}

impl Glide {
    pub const fn at(v: i32) -> Self {
        Self {
            cur: v << 8,
            target: v << 8,
        }
    }

    /// Aim at `v` (plain units).
    pub fn set(&mut self, v: i32) {
        self.target = v << 8;
    }

    /// Jump to the target (reduced motion, first sample).
    pub fn snap(&mut self) {
        self.cur = self.target;
    }

    /// Advance by `dt_ms`; `true` while still moving.
    pub fn step(&mut self, dt_ms: u32, tau_ms: u32) -> bool {
        self.cur = ease_toward(self.cur, self.target, dt_ms, tau_ms);
        self.cur != self.target
    }

    /// Current value in plain units (rounded).
    pub fn value(&self) -> i32 {
        (self.cur + 128) >> 8
    }

    /// Where it is heading, in plain units.
    pub fn target(&self) -> i32 {
        self.target >> 8
    }

    /// Current value in Q8.
    pub fn q8(&self) -> i32 {
        self.cur
    }

    pub fn moving(&self) -> bool {
        self.cur != self.target
    }
}

/// Value of the history at fractional position `pos_q8` (Q8 index, `0` = oldest
/// kept), linearly interpolated and clamped to the ends. `None` when empty.
pub fn series_at(s: &Series, pos_q8: i64) -> Option<u32> {
    let mut buf = [0u32; crate::system::sysmon::HIST];
    let n = snapshot(s, &mut buf);
    slice_at(&buf[..n], pos_q8)
}

/// Copy a history into `out` (oldest first); returns how many samples were copied.
pub fn snapshot(s: &Series, out: &mut [u32; crate::system::sysmon::HIST]) -> usize {
    let n = s.len();
    for (i, o) in out.iter_mut().enumerate().take(n) {
        *o = s.get(i).unwrap_or(0);
    }
    n
}

/// [`series_at`] over a plain slice.
pub fn slice_at(v: &[u32], pos_q8: i64) -> Option<u32> {
    if v.is_empty() {
        return None;
    }
    let max = ((v.len() - 1) as i64) << 8;
    let p = pos_q8.clamp(0, max);
    let i = (p >> 8) as usize;
    let f = (p & 255) as u64;
    let a = v[i] as u64;
    let b = v[(i + 1).min(v.len() - 1)] as u64;
    Some(((a * (256 - f) + b * f + 128) >> 8) as u32)
}

/// Light smoothing of a history for drawing, in place: a 3-tap binomial filter
/// (1-2-1) with the ends kept as they are.
pub fn smooth121(v: &mut [u32]) {
    if v.len() < 3 {
        return;
    }
    let mut prev = v[0] as u64;
    for i in 1..v.len() - 1 {
        let cur = v[i] as u64;
        v[i] = ((prev + 2 * cur + v[i + 1] as u64 + 2) / 4) as u32;
        prev = cur;
    }
}

/// The text of a formatting buffer (empty if a cut left it invalid).
pub fn text<const N: usize>(b: &FixedBuf<N>) -> &str {
    core::str::from_utf8(b.as_bytes()).unwrap_or("")
}

/// Which sample of a plot `plot_w` pixels wide (starting at `plot_x`) is under the
/// pointer at `px`, for a history of `len` samples drawn right-aligned in `slots`
/// slots (the newest at the right edge). `None` outside the plot or where no sample
/// exists yet.
pub fn sample_under(px: i32, plot_x: i32, plot_w: i32, slots: usize, len: usize) -> Option<usize> {
    if plot_w <= 0 || len == 0 || slots < 2 || px < plot_x || px >= plot_x + plot_w {
        return None;
    }
    let slot = ((px - plot_x) as i64 * (slots as i64 - 1) * 2 / plot_w.max(1) as i64 + 1) / 2;
    let slot = (slot as usize).min(slots - 1);
    let first = slots.saturating_sub(len);
    slot.checked_sub(first)
}

// ------------------------------------------------------------------ rates

/// Bytes (or packets) moved between two reads of a counter that is `bits` wide and may
/// have wrapped around once. A counter that merely went *backwards* by less than half
/// its range is treated as a reset (0), not as a near-full wrap.
pub fn wrapping_delta(prev: u64, now: u64, bits: u32) -> u64 {
    let bits = bits.clamp(1, 64);
    let mask = if bits == 64 {
        u64::MAX
    } else {
        (1u64 << bits) - 1
    };
    let (prev, now) = (prev & mask, now & mask);
    let d = now.wrapping_sub(prev) & mask;
    // Behind the previous read by more than half the range: a reset, not a wrap.
    if now < prev && d > mask / 2 { 0 } else { d }
}

/// Per-second rate from a cumulative counter, tolerant of wrap-around and of an
/// irregular interval (milliseconds). The first read reports 0.
#[derive(Clone, Copy, Debug, Default)]
pub struct Rate {
    last: Option<(u64, u64)>,
    bits: u8,
}

impl Rate {
    /// A meter for a `bits`-wide counter.
    pub const fn new(bits: u8) -> Self {
        Self { last: None, bits }
    }

    /// Feed the counter `now` read at `t_ms`. Returns units per second.
    pub fn feed(&mut self, now: u64, t_ms: u64) -> u64 {
        let r = match self.last {
            Some((prev, t0)) if t_ms > t0 => {
                let d = wrapping_delta(prev, now, self.bits as u32);
                (d as u128 * 1000 / (t_ms - t0) as u128).min(u64::MAX as u128) as u64
            }
            _ => 0,
        };
        self.last = Some((now, t_ms));
        r
    }
}

// ------------------------------------------------------------------ load

/// Exponentially averaged busy share over 1, 5 and 15 minutes, in thousandths of a
/// full CPU (`1000` = one CPU busy all the time). Fed once per second.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadAvg {
    // Q8 thousandths.
    v: [i64; 3],
    primed: bool,
}

impl LoadAvg {
    pub const fn new() -> Self {
        Self {
            v: [0; 3],
            primed: false,
        }
    }

    /// One second elapsed with the CPU `busy_pm` thousandths busy.
    pub fn feed(&mut self, busy_pm: u32) {
        let x = (busy_pm.min(1000) as i64) << 8;
        if !self.primed {
            self.v = [x; 3];
            self.primed = true;
            return;
        }
        for (v, tau) in self.v.iter_mut().zip([60i64, 300, 900]) {
            *v += (x - *v) / tau;
        }
    }

    /// Averages in thousandths: (1 min, 5 min, 15 min).
    pub fn get(&self) -> (u32, u32, u32) {
        let f = |v: i64| ((v + 128) >> 8).clamp(0, 1000) as u32;
        (f(self.v[0]), f(self.v[1]), f(self.v[2]))
    }
}

// ------------------------------------------------------------------ memory pressure

/// How tight memory is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pressure {
    Normal,
    Attention,
    Critical,
}

impl Pressure {
    /// Pressure from used and total bytes: under 60 % is normal, up to 85 % deserves
    /// attention, above that it is critical.
    pub fn of(used: u64, total: u64) -> Pressure {
        if total == 0 {
            return Pressure::Normal;
        }
        let pm = used.min(total) as u128 * 1000 / total as u128;
        if pm < 600 {
            Pressure::Normal
        } else if pm < 850 {
            Pressure::Attention
        } else {
            Pressure::Critical
        }
    }

    /// The catalog key of the label.
    pub const fn key(self) -> &'static str {
        match self {
            Pressure::Normal => tk!("tasks.pressure.normal"),
            Pressure::Attention => tk!("tasks.pressure.attention"),
            Pressure::Critical => tk!("tasks.pressure.critical"),
        }
    }

    /// The label in the language in effect.
    pub fn label(self) -> &'static str {
        i18n::tr(self.key())
    }
}

/// Used share of `total` in thousandths (0 when `total` is 0).
pub fn permille(used: u64, total: u64) -> u32 {
    if total == 0 {
        return 0;
    }
    (used.min(total) as u128 * 1000 / total as u128) as u32
}

// ------------------------------------------------------------------ process table

/// What a row of the process table is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskKind {
    /// A kernel thread.
    Thread,
    /// An app window's process.
    App,
    /// A bookkeeping process (`kernel`, the idle share).
    System,
}

/// State shown in the table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskState {
    Running,
    Waiting,
    Suspended,
    Ended,
    Stopped,
}

impl TaskState {
    /// The catalog key of the label.
    pub const fn key(self) -> &'static str {
        match self {
            TaskState::Running => tk!("tasks.state.running"),
            TaskState::Waiting => tk!("tasks.state.waiting"),
            TaskState::Suspended => tk!("tasks.state.suspended"),
            TaskState::Ended => tk!("tasks.state.ended"),
            TaskState::Stopped => tk!("tasks.state.stopped"),
        }
    }

    /// The label in the language in effect.
    pub fn label(self) -> &'static str {
        i18n::tr(self.key())
    }
}

/// Columns of the table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Column {
    Pid,
    Name,
    State,
    Cpu,
    Mem,
    Up,
}

impl Column {
    pub const ALL: [Column; 6] = [
        Column::Pid,
        Column::Name,
        Column::State,
        Column::Cpu,
        Column::Mem,
        Column::Up,
    ];

    /// The catalog key of the header.
    pub const fn key(self) -> &'static str {
        match self {
            Column::Pid => tk!("tasks.col.pid"),
            Column::Name => tk!("tasks.col.name"),
            Column::State => tk!("tasks.col.state"),
            Column::Cpu => tk!("tasks.col.cpu"),
            Column::Mem => tk!("tasks.col.mem"),
            Column::Up => tk!("tasks.col.up"),
        }
    }

    /// The header text in the language in effect.
    pub fn title(self) -> &'static str {
        i18n::tr(self.key())
    }

    /// Sorting a column the first time: text ascending, numbers descending.
    pub const fn default_desc(self) -> bool {
        matches!(self, Column::Cpu | Column::Mem | Column::Up)
    }
}

/// One line of the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskRow {
    /// Stable identity across refreshes (so the selection survives re-sorting).
    pub id: u32,
    /// The internal name (shown in the tooltip).
    pub raw: String,
    /// The name a person sees.
    pub name: String,
    pub kind: TaskKind,
    /// Process-table id; `0` for threads.
    pub pid: u16,
    pub state: TaskState,
    /// Share of the CPU in thousandths; `None` when nobody measures it.
    pub cpu_pm: Option<u16>,
    /// Memory in KiB when known.
    pub mem_kib: Option<u32>,
    pub up_s: u32,
}

impl TaskRow {
    pub fn new(id: u32, raw: &[u8], kind: TaskKind, pid: u16, state: TaskState) -> TaskRow {
        TaskRow {
            id,
            raw: raw.iter().map(|&b| b as char).collect(),
            name: friendly_name(raw),
            kind,
            pid,
            state,
            cpu_pm: None,
            mem_kib: None,
            up_s: 0,
        }
    }

    /// Does `query` (already lowercase) occur in the friendly or internal name?
    pub fn matches(&self, query: &str) -> bool {
        query.is_empty()
            || fold(&self.name).contains(query)
            || self.raw.to_ascii_lowercase().contains(query)
    }
}

/// Lowercase with the Portuguese accents removed (`Execução` -> `execucao`), for
/// searching and sorting names.
pub fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' => 'a',
            'é' | 'ê' | 'è' | 'É' | 'Ê' => 'e',
            'í' | 'Í' => 'i',
            'ó' | 'ô' | 'õ' | 'Ó' | 'Ô' | 'Õ' => 'o',
            'ú' | 'ü' | 'Ú' => 'u',
            'ç' | 'Ç' => 'c',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

fn ord_of(a: &TaskRow, b: &TaskRow, col: Column) -> core::cmp::Ordering {
    match col {
        Column::Pid => a.pid.cmp(&b.pid),
        Column::Name => fold(&a.name).cmp(&fold(&b.name)),
        Column::State => (a.state as u8).cmp(&(b.state as u8)),
        Column::Cpu => a.cpu_pm.unwrap_or(0).cmp(&b.cpu_pm.unwrap_or(0)),
        Column::Mem => a.mem_kib.unwrap_or(0).cmp(&b.mem_kib.unwrap_or(0)),
        Column::Up => a.up_s.cmp(&b.up_s),
    }
}

/// Sort in place by `col` (stable: ties keep their previous order, so the list does
/// not shuffle between samples).
pub fn sort_tasks(rows: &mut [TaskRow], col: Column, descending: bool) {
    use core::cmp::Ordering;
    for i in 1..rows.len() {
        let mut j = i;
        while j > 0 {
            let o = ord_of(&rows[j - 1], &rows[j], col);
            let out_of_order = if descending {
                o == Ordering::Less
            } else {
                o == Ordering::Greater
            };
            if !out_of_order {
                break;
            }
            rows.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// Totals for the footer of the table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub processes: u32,
    pub threads: u32,
}

pub fn totals(rows: &[TaskRow]) -> Totals {
    let mut t = Totals::default();
    for r in rows {
        match r.kind {
            TaskKind::Thread => t.threads += 1,
            TaskKind::App | TaskKind::System => t.processes += 1,
        }
    }
    t
}

#[cfg(test)]
mod tests;
