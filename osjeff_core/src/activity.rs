//! Pure helpers of the **Tarefas** app (the activity monitor): friendly process names,
//! Portuguese number formatting, the history ring's interpolation for smooth graphs,
//! an exponential smoother, rates from wrapping counters, load averages, the memory
//! pressure level and the sortable process table.
//!
//! Everything is integer arithmetic (the kernel has no FPU in its hot paths) and
//! allocation-light; the kernel owns the sampling and the drawing.

use crate::klog::FixedBuf;
use crate::sysmon::Series;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

// ------------------------------------------------------------------ names

/// The name a person sees for an internal thread or process name. Numbered instances
/// (`shell 2`) keep their number (`Terminal 2`). Unknown names are returned as they
/// are, so nothing disappears from the list.
pub fn friendly_name(raw: &[u8]) -> String {
    let (base, num) = split_number(raw);
    let fixed: Option<&str> = match base {
        b"compositor" => Some("Interface"),
        b"fetcher" => Some("Rede (busca)"),
        b"appd" => Some("Aplicativos"),
        b"shelld" | b"shelld2" => Some("Terminal (execução)"),
        b"logd" => Some("Registro"),
        b"kernel" => Some("Sistema"),
        b"(ocioso)" => Some("Ocioso"),
        b"shell" => Some("Terminal"),
        b"editor" => Some("Editor"),
        b"taskmgr" | b"monitor" => Some("Tarefas"),
        b"calc" => Some("Calculadora"),
        b"browser" => Some("Navegador"),
        b"wasmapp" => Some("Aplicativo"),
        b"files" => Some("Arquivos"),
        b"settings" => Some("Ajustes"),
        b"syslog" => Some("Registro"),
        b"viewer" => Some("Imagens"),
        b"gallery" => Some("Componentes"),
        _ => None,
    };
    let mut out = String::new();
    match fixed {
        Some(s) => out.push_str(s),
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

/// `12,3%` from tenths of a percent.
pub fn fmt_pct(pm: u32) -> FixedBuf<12> {
    let mut b = FixedBuf::new();
    let _ = write!(b, "{},{}%", pm / 10, pm % 10);
    b
}

/// `12%` from tenths of a percent (rounded), for places too narrow for a decimal.
pub fn fmt_pct_int(pm: u32) -> FixedBuf<8> {
    let mut b = FixedBuf::new();
    let _ = write!(b, "{}%", (pm + 5) / 10);
    b
}

/// `512 B`, `1,5 KiB`, `12,0 MiB`, `3,2 GiB`.
pub fn fmt_size(v: u64) -> FixedBuf<20> {
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
    let _ = write!(b, "{},{} {unit}", tenths / 10, tenths % 10);
    b
}

/// `2,0 KiB/s` for a byte rate.
pub fn fmt_speed(bytes_per_s: u64) -> FixedBuf<24> {
    let mut b = FixedBuf::new();
    let _ = write!(b, "{}/s", fmt_size(bytes_per_s));
    b
}

/// A whole number with a thin grouping: `12.345` (Portuguese uses the dot).
pub fn fmt_count(v: u64) -> FixedBuf<28> {
    let mut digits = FixedBuf::<24>::new();
    let _ = write!(digits, "{v}");
    let d = digits.as_bytes();
    let mut b = FixedBuf::new();
    for (i, &c) in d.iter().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            let _ = b.write_char('.');
        }
        let _ = b.write_char(c as char);
    }
    b
}

/// Uptime for people: `42 s`, `3 min 05 s`, `1 h 02 min`, `2 d 03 h`.
pub fn fmt_elapsed(secs: u64) -> FixedBuf<20> {
    let mut b = FixedBuf::new();
    let (d, h, m, s) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        let _ = write!(b, "{d} d {h:02} h");
    } else if h > 0 {
        let _ = write!(b, "{h} h {m:02} min");
    } else if m > 0 {
        let _ = write!(b, "{m} min {s:02} s");
    } else {
        let _ = write!(b, "{s} s");
    }
    b
}

/// `01:02:03` clock-style uptime, with `d` days in front past one day.
pub fn fmt_clock(secs: u64) -> FixedBuf<20> {
    let mut b = FixedBuf::new();
    let (d, h, m, s) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    if d > 0 {
        let _ = write!(b, "{d} d {h:02}:{m:02}:{s:02}");
    } else {
        let _ = write!(b, "{h:02}:{m:02}:{s:02}");
    }
    b
}

/// `há 12 s` for a graph's hover label (`0` is "agora").
pub fn fmt_ago(secs: u32) -> FixedBuf<12> {
    let mut b = FixedBuf::new();
    if secs == 0 {
        let _ = write!(b, "agora");
    } else {
        let _ = write!(b, "há {secs} s");
    }
    b
}

/// `0,42` from thousandths (the load average).
pub fn fmt_milli(v: u32) -> FixedBuf<12> {
    let mut b = FixedBuf::new();
    let _ = write!(b, "{},{:02}", v / 1000, v % 1000 / 10);
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
    let n = s.len();
    if n == 0 {
        return None;
    }
    let max = ((n - 1) as i64) << 8;
    let p = pos_q8.clamp(0, max);
    let i = (p >> 8) as usize;
    let f = (p & 255) as u64;
    let a = s.get(i)? as u64;
    let b = s.get((i + 1).min(n - 1))? as u64;
    Some(((a * (256 - f) + b * f + 128) >> 8) as u32)
}

/// Light smoothing of a history for drawing: a 3-tap binomial filter (1-2-1), ends
/// kept. Output has the same length as the input (at most [`crate::sysmon::HIST`]).
pub fn smooth121(vals: &[u32], out: &mut Vec<u32>) {
    out.clear();
    for i in 0..vals.len() {
        let prev = vals[i.saturating_sub(1)] as u64;
        let next = vals[(i + 1).min(vals.len() - 1)] as u64;
        out.push(((prev + 2 * vals[i] as u64 + next + 2) / 4) as u32);
    }
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

    pub fn label(self) -> &'static str {
        match self {
            Pressure::Normal => "Normal",
            Pressure::Attention => "Atenção",
            Pressure::Critical => "Crítica",
        }
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
    pub const fn label(self) -> &'static str {
        match self {
            TaskState::Running => "Ativo",
            TaskState::Waiting => "Em espera",
            TaskState::Suspended => "Suspenso",
            TaskState::Ended => "Encerrado",
            TaskState::Stopped => "Parado",
        }
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

    pub const fn title(self) -> &'static str {
        match self {
            Column::Pid => "PID",
            Column::Name => "Nome",
            Column::State => "Estado",
            Column::Cpu => "CPU",
            Column::Mem => "Memória",
            Column::Up => "Tempo ativo",
        }
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
mod tests {
    use super::*;

    fn s(b: FixedBuf<20>) -> String {
        String::from_utf8_lossy(b.as_bytes()).into_owned()
    }

    #[test]
    fn names_are_translated() {
        assert_eq!(friendly_name(b"compositor"), "Interface");
        assert_eq!(friendly_name(b"fetcher"), "Rede (busca)");
        assert_eq!(friendly_name(b"appd"), "Aplicativos");
        assert_eq!(friendly_name(b"shelld"), "Terminal (execução)");
        assert_eq!(friendly_name(b"shelld2"), "Terminal (execução)");
        assert_eq!(friendly_name(b"logd"), "Registro");
        assert_eq!(friendly_name(b"kernel"), "Sistema");
        assert_eq!(friendly_name(b"shell"), "Terminal");
        assert_eq!(friendly_name(b"shell 2"), "Terminal 2");
        assert_eq!(friendly_name(b"calc 12"), "Calculadora 12");
        assert_eq!(friendly_name(b"mystery"), "mystery");
        assert_eq!(friendly_name(b"mystery 7"), "mystery 7");
        assert_eq!(friendly_name(b""), "");
        // A long numeric tail is not an instance number.
        assert_eq!(friendly_name(b"app 12345"), "app 12345");
    }

    #[test]
    fn system_names() {
        assert!(is_system_name(b"appd"));
        assert!(is_system_name(b"kernel"));
        assert!(!is_system_name(b"shell"));
        assert!(!is_system_name(b"calc 2"));
    }

    #[test]
    fn pt_formatting() {
        assert_eq!(fmt_pct(0).as_bytes(), b"0,0%");
        assert_eq!(fmt_pct(123).as_bytes(), b"12,3%");
        assert_eq!(fmt_pct(1000).as_bytes(), b"100,0%");
        assert_eq!(fmt_pct_int(125).as_bytes(), b"13%");
        assert_eq!(fmt_pct_int(4).as_bytes(), b"0%");
        assert_eq!(s(fmt_size(0)), "0 B");
        assert_eq!(s(fmt_size(1023)), "1023 B");
        assert_eq!(s(fmt_size(1024)), "1,0 KiB");
        assert_eq!(s(fmt_size(1536)), "1,5 KiB");
        assert_eq!(s(fmt_size(64 << 20)), "64,0 MiB");
        assert_eq!(s(fmt_size(3 << 30)), "3,0 GiB");
        assert_eq!(fmt_speed(2048).as_bytes(), b"2,0 KiB/s");
        assert_eq!(fmt_count(0).as_bytes(), b"0");
        assert_eq!(fmt_count(999).as_bytes(), b"999");
        assert_eq!(fmt_count(1000).as_bytes(), b"1.000");
        assert_eq!(fmt_count(1234567).as_bytes(), b"1.234.567");
        assert_eq!(s(fmt_elapsed(7)), "7 s");
        assert_eq!(s(fmt_elapsed(185)), "3 min 05 s");
        assert_eq!(s(fmt_elapsed(3725)), "1 h 02 min");
        assert_eq!(s(fmt_elapsed(90_061)), "1 d 01 h");
        assert_eq!(s(fmt_clock(3725)), "01:02:05");
        assert_eq!(s(fmt_clock(90_061)), "1 d 01:01:01");
        assert_eq!(fmt_ago(0).as_bytes(), b"agora");
        assert_eq!(fmt_ago(12).as_bytes(), "há 12 s".as_bytes());
        assert_eq!(fmt_milli(420).as_bytes(), b"0,42");
        assert_eq!(fmt_milli(1000).as_bytes(), b"1,00");
        // Extremes never panic.
        let _ = fmt_size(u64::MAX);
        let _ = fmt_elapsed(u64::MAX);
        let _ = fmt_count(u64::MAX);
        let _ = fmt_pct(u32::MAX);
    }

    #[test]
    fn easing_settles_and_never_overshoots() {
        let mut v = 0;
        let target = 100 << 8;
        let mut steps = 0;
        while v != target {
            let n = ease_toward(v, target, 16, 120);
            assert!(n > v && n <= target);
            v = n;
            steps += 1;
            assert!(steps < 2000, "must settle");
        }
        // Downwards too.
        let mut v = 100 << 8;
        while v != 0 {
            let n = ease_toward(v, 0, 16, 120);
            assert!((0..v).contains(&n));
            v = n;
        }
        // A huge dt lands (nearly) on the target in one step, never beyond it.
        assert!(ease_toward(0, 256, 10_000, 100) <= 256);
        assert_eq!(ease_toward(5, 5, 16, 100), 5);
    }

    #[test]
    fn glide_moves_then_rests() {
        let mut g = Glide::at(10);
        assert!(!g.moving());
        g.set(50);
        assert!(g.moving());
        let mut guard = 0;
        while g.step(16, 80) {
            guard += 1;
            assert!(guard < 1000);
        }
        assert_eq!(g.value(), 50);
        g.set(0);
        g.snap();
        assert_eq!(g.value(), 0);
        assert!(!g.moving());
    }

    #[test]
    fn series_interpolates() {
        let mut ser = Series::new();
        assert_eq!(series_at(&ser, 0), None);
        for v in [0u32, 100, 200] {
            ser.push(v);
        }
        assert_eq!(series_at(&ser, 0), Some(0));
        assert_eq!(series_at(&ser, 128), Some(50));
        assert_eq!(series_at(&ser, 256), Some(100));
        assert_eq!(series_at(&ser, 384), Some(150));
        assert_eq!(series_at(&ser, 512), Some(200));
        // Clamped at both ends.
        assert_eq!(series_at(&ser, -500), Some(0));
        assert_eq!(series_at(&ser, 9999), Some(200));
    }

    #[test]
    fn smoothing_keeps_a_flat_line_and_rounds_a_spike() {
        let mut out = Vec::new();
        smooth121(&[40, 40, 40, 40], &mut out);
        assert_eq!(out, [40, 40, 40, 40]);
        smooth121(&[0, 0, 100, 0, 0], &mut out);
        assert_eq!(out, [0, 25, 50, 25, 0]);
        smooth121(&[7], &mut out);
        assert_eq!(out, [7]);
        smooth121(&[], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn pointer_to_sample() {
        // 60 slots over 590 px; a 3-sample history sits at the right edge.
        assert_eq!(sample_under(100, 100, 590, 60, 60), Some(0));
        assert_eq!(sample_under(689, 100, 590, 60, 60), Some(59));
        assert_eq!(sample_under(99, 100, 590, 60, 60), None);
        assert_eq!(sample_under(690, 100, 590, 60, 60), None);
        assert_eq!(sample_under(100, 100, 590, 60, 3), None);
        assert_eq!(sample_under(689, 100, 590, 60, 3), Some(2));
        assert_eq!(sample_under(5, 0, 0, 60, 3), None);
        assert_eq!(sample_under(5, 0, 100, 60, 0), None);
    }

    #[test]
    fn wrapping_counters() {
        assert_eq!(wrapping_delta(10, 25, 32), 15);
        // A 32-bit counter wrapped once.
        assert_eq!(wrapping_delta(u32::MAX as u64 - 4, 5, 32), 10);
        // 64-bit, no wrap.
        assert_eq!(wrapping_delta(1, 1 << 40, 64), (1 << 40) - 1);
        // A small step backwards is a reset, not 4 billion bytes.
        assert_eq!(wrapping_delta(1000, 10, 64), 0);
        assert_eq!(wrapping_delta(0, 0, 8), 0);
        // 8-bit wrap.
        assert_eq!(wrapping_delta(250, 4, 8), 10);
    }

    #[test]
    fn rate_from_counters() {
        let mut r = Rate::new(32);
        assert_eq!(r.feed(1000, 0), 0);
        assert_eq!(r.feed(3000, 1000), 2000);
        // Irregular interval: 1500 bytes in 500 ms.
        assert_eq!(r.feed(4500, 1500), 3000);
        // Wrap-around of a 32-bit counter.
        let mut w = Rate::new(32);
        w.feed(u32::MAX as u64 - 99, 0);
        assert_eq!(w.feed(100, 1000), 200);
        // Same timestamp: no division by zero.
        assert_eq!(w.feed(200, 1000), 0);
        // Clock going backwards.
        assert_eq!(w.feed(300, 500), 0);
    }

    #[test]
    fn load_average_follows_and_decays() {
        let mut l = LoadAvg::new();
        assert_eq!(l.get(), (0, 0, 0));
        for _ in 0..600 {
            l.feed(500);
        }
        let (a, b, c) = l.get();
        assert!((495..=505).contains(&a), "{a}");
        assert!((490..=510).contains(&b), "{b}");
        assert!(c > 400, "{c}");
        for _ in 0..300 {
            l.feed(0);
        }
        let (a2, b2, c2) = l.get();
        assert!(a2 < 20, "{a2}");
        assert!(b2 < a && b2 > a2);
        assert!(c2 > b2);
        // Out-of-range input is clamped.
        l.feed(9999);
        assert!(l.get().0 <= 1000);
    }

    #[test]
    fn pressure_levels() {
        assert_eq!(Pressure::of(0, 0), Pressure::Normal);
        assert_eq!(Pressure::of(59, 100), Pressure::Normal);
        assert_eq!(Pressure::of(60, 100), Pressure::Attention);
        assert_eq!(Pressure::of(84, 100), Pressure::Attention);
        assert_eq!(Pressure::of(85, 100), Pressure::Critical);
        assert_eq!(Pressure::of(500, 100), Pressure::Critical);
        assert!(Pressure::Critical > Pressure::Normal);
        assert_eq!(Pressure::Attention.label(), "Atenção");
        assert_eq!(permille(1, 4), 250);
        assert_eq!(permille(9, 0), 0);
        assert_eq!(permille(9, 3), 1000);
    }

    fn task(id: u32, raw: &str, pid: u16, cpu: Option<u16>, mem: Option<u32>, up: u32) -> TaskRow {
        let mut r = TaskRow::new(id, raw.as_bytes(), TaskKind::App, pid, TaskState::Running);
        r.cpu_pm = cpu;
        r.mem_kib = mem;
        r.up_s = up;
        r
    }

    fn ids(rows: &[TaskRow]) -> Vec<u32> {
        rows.iter().map(|r| r.id).collect()
    }

    #[test]
    fn table_sorts_every_column_both_ways() {
        let mut rows = [
            task(1, "shell", 3, Some(100), None, 5),
            task(2, "editor", 2, Some(300), Some(128), 50),
            task(3, "calc", 9, None, Some(256), 20),
        ];
        sort_tasks(&mut rows, Column::Name, false);
        // Calculadora, Editor, Terminal
        assert_eq!(ids(&rows), [3, 2, 1]);
        sort_tasks(&mut rows, Column::Name, true);
        assert_eq!(ids(&rows), [1, 2, 3]);
        sort_tasks(&mut rows, Column::Pid, false);
        assert_eq!(ids(&rows), [2, 1, 3]);
        sort_tasks(&mut rows, Column::Cpu, true);
        assert_eq!(ids(&rows), [2, 1, 3]);
        sort_tasks(&mut rows, Column::Mem, true);
        assert_eq!(ids(&rows), [3, 2, 1]);
        sort_tasks(&mut rows, Column::Up, false);
        assert_eq!(ids(&rows), [1, 3, 2]);
    }

    #[test]
    fn table_sort_is_stable_and_accent_blind() {
        let mut rows = [
            task(1, "a", 1, Some(10), None, 0),
            task(2, "b", 2, Some(10), None, 0),
            task(3, "c", 3, Some(10), None, 0),
        ];
        sort_tasks(&mut rows, Column::Cpu, true);
        assert_eq!(ids(&rows), [1, 2, 3]);
        sort_tasks(&mut rows, Column::Cpu, false);
        assert_eq!(ids(&rows), [1, 2, 3]);
        // "Terminal (execução)" sorts by its folded name.
        let mut rows = [
            TaskRow::new(1, b"shelld", TaskKind::Thread, 0, TaskState::Waiting),
            TaskRow::new(2, b"appd", TaskKind::Thread, 0, TaskState::Waiting),
            TaskRow::new(3, b"compositor", TaskKind::Thread, 0, TaskState::Waiting),
        ];
        sort_tasks(&mut rows, Column::Name, false);
        assert_eq!(ids(&rows), [2, 3, 1]);
        assert_eq!(fold("Execução"), "execucao");
        assert_eq!(fold("Atenção Ônibus"), "atencao onibus");
    }

    #[test]
    fn table_search_and_totals() {
        let r = TaskRow::new(1, b"shelld", TaskKind::Thread, 0, TaskState::Waiting);
        assert!(r.matches(""));
        assert!(r.matches("execucao"));
        assert!(r.matches("shelld"));
        assert!(!r.matches("browser"));
        let rows = [
            r,
            task(2, "calc", 4, None, None, 0),
            TaskRow::new(3, b"kernel", TaskKind::System, 1, TaskState::Running),
        ];
        assert_eq!(
            totals(&rows),
            Totals {
                processes: 2,
                threads: 1
            }
        );
        assert_eq!(TaskState::Waiting.label(), "Em espera");
        assert!(Column::Cpu.default_desc() && !Column::Name.default_desc());
        assert_eq!(Column::ALL.len(), 6);
        assert_eq!(Column::Up.title(), "Tempo ativo");
    }
}
