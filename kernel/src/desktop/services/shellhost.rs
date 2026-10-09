//! What the shell engine (`kitsune_core::shell`) runs on: the filesystem, the
//! system information and the thread that executes command lines.
//!
//! * [`VfsFs`] implements `ShellFs` over [`vfs`](crate::desktop::services::vfs), the desktop's only
//!   file API: absolute normalized paths, a working directory per terminal.
//! * [`KSys`] implements `SysInfo`. The facts that live in the compositor (the
//!   process table, disk identity, heap use) arrive as a [`Snap`] taken when the
//!   command is posted; everything else (clock, uptime, network) is read from
//!   thread-safe sources.
//! * `shelld` ([`worker`]) is one kernel thread that runs command lines posted by
//!   any terminal, one at a time. `sleep`, `ping`, `nslookup` and `curl` can wait
//!   for seconds; running them off the compositor keeps the desktop alive. A
//!   terminal hands its shell and filesystem state ([`Ctx`]) to the job and gets
//!   them back with the result ([`Done`]); Ctrl+C is [`cancel`].
//!
//! Commands that must act on the desktop (`edit`, `tasks`, `reboot`...) cannot
//! run on this thread: they leave a [`UiReq`] that the compositor applies.

use crate::desktop::services::vfs;
use crate::desktop::*;
use crate::sync::YieldMutex;
use alloc::boxed::Box;
use alloc::collections::VecDeque;
use alloc::string::ToString;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use kitsune_core::shell::exec::CmdCtx;
use kitsune_core::shell::fs::{DirEntry, FsErr, FsUsage, Kind as FsKind, ShellFs, Stat};
use kitsune_core::shell::sys::{
    DateTime, DiskInfo, HttpResponse, MemInfo, NetInfo, PingStats, ProcInfo, SysErr, SysInfo,
};
use kitsune_core::shell::{Host, RunResult, Shell};
use kitsune_core::{t, tk};

// ---- filesystem ------------------------------------------------------------

/// The shell's view of the VFS. Holds only the working directory.
pub(crate) struct VfsFs {
    cwd: String,
}

impl VfsFs {
    pub(crate) fn new() -> Self {
        Self {
            cwd: String::from("/"),
        }
    }
}

/// A VFS error as the shell's error kind.
fn map_err(e: vfs::VfsError) -> FsErr {
    use vfs::VfsError as V;
    match e {
        V::NotFound => FsErr::NotFound,
        V::Exists => FsErr::AlreadyExists,
        V::NotDir => FsErr::NotADirectory,
        V::IsDir => FsErr::IsADirectory,
        V::NotEmpty => FsErr::NotEmpty,
        V::InvalidName | V::InvalidPath | V::InvalidMove => FsErr::InvalidPath,
        V::NameTooLong => FsErr::NameTooLong,
        V::Reserved => FsErr::ReadOnly,
        V::NoSpace | V::NoInodes => FsErr::NoSpace,
        V::TooBig => FsErr::TooBig,
        V::Busy | V::Unavailable | V::Io | V::Corrupt | V::Cancelled => FsErr::Io,
    }
}

fn kind_of(k: vfs::EntryKind) -> FsKind {
    match k {
        vfs::EntryKind::File => FsKind::File,
        vfs::EntryKind::Dir => FsKind::Dir,
    }
}

impl ShellFs for VfsFs {
    fn cwd(&self) -> String {
        self.cwd.clone()
    }

    fn set_cwd(&mut self, path: &str) -> Result<(), FsErr> {
        let p = self.resolve(path);
        match self.stat(&p)?.kind {
            FsKind::Dir => {
                self.cwd = p;
                Ok(())
            }
            FsKind::File => Err(FsErr::NotADirectory),
        }
    }

    fn stat(&self, path: &str) -> Result<Stat, FsErr> {
        let p = self.resolve(path);
        if p == "/" {
            return Ok(Stat {
                kind: FsKind::Dir,
                size: 0,
            });
        }
        let i = vfs::stat(p.as_bytes()).map_err(map_err)?;
        Ok(Stat {
            kind: kind_of(i.kind),
            size: i.size,
        })
    }

    fn read(&mut self, path: &str) -> Result<Vec<u8>, FsErr> {
        vfs::read_file(self.resolve(path).as_bytes()).map_err(map_err)
    }

    fn read_at(&mut self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>, FsErr> {
        vfs::read_range(self.resolve(path).as_bytes(), offset, len).map_err(map_err)
    }

    fn write(&mut self, path: &str, data: &[u8]) -> Result<(), FsErr> {
        vfs::write_file(self.resolve(path).as_bytes(), data).map_err(map_err)
    }

    fn append(&mut self, path: &str, data: &[u8]) -> Result<(), FsErr> {
        vfs::append(self.resolve(path).as_bytes(), data).map_err(map_err)
    }

    fn list(&self, path: &str) -> Result<Vec<DirEntry>, FsErr> {
        let rows = vfs::list(self.resolve(path).as_bytes()).map_err(map_err)?;
        Ok(rows
            .into_iter()
            .map(|e| DirEntry {
                name: String::from_utf8_lossy(&e.name).into_owned(),
                kind: kind_of(e.kind),
                size: e.size,
            })
            .collect())
    }

    fn mkdir(&mut self, path: &str) -> Result<(), FsErr> {
        vfs::mkdir(self.resolve(path).as_bytes()).map_err(map_err)
    }

    /// Files and empty folders go to the trash (restorable from the file
    /// manager), like the desktop's own delete.
    fn remove(&mut self, path: &str) -> Result<(), FsErr> {
        let p = self.resolve(path);
        if self.stat(&p)?.kind == FsKind::Dir && !self.list(&p)?.is_empty() {
            return Err(FsErr::NotEmpty);
        }
        vfs::remove(p.as_bytes()).map_err(map_err)
    }

    /// An existing destination file is replaced (the old one goes to the trash).
    fn rename(&mut self, from: &str, to: &str) -> Result<(), FsErr> {
        let (a, b) = (self.resolve(from), self.resolve(to));
        if a == b {
            return Ok(());
        }
        if let Ok(st) = self.stat(&b) {
            if st.kind == FsKind::Dir {
                return Err(FsErr::AlreadyExists);
            }
            vfs::remove(b.as_bytes()).map_err(map_err)?;
        }
        vfs::rename_path(a.as_bytes(), b.as_bytes()).map_err(map_err)
    }

    fn usage(&self) -> FsUsage {
        let u = vfs::statfs();
        FsUsage {
            total_bytes: u.total,
            used_bytes: u.used(),
            files: 0,
            dirs: 0,
        }
    }
}

// ---- system information ----------------------------------------------------

/// What the compositor knows and the command thread needs, copied when a
/// command line is posted.
pub(crate) struct Snap {
    /// `ps`: the entries and whether `kill` may close them.
    pub procs: Vec<(ProcInfo, bool)>,
    pub mem: MemInfo,
    /// Identity of the two IDE disks (boot, filesystem).
    pub disks: [Option<crate::ata::DiskInfo>; 2],
}

/// `SysInfo` for one command line.
pub(crate) struct KSys {
    uid: u32,
    snap: Snap,
    /// `kill` requests for the compositor to carry out after the run.
    kills: Vec<(u32, i32)>,
}

/// Scheduler ticks in `ms` milliseconds.
fn ticks_of(ms: u64) -> u64 {
    ms * u64::from(crate::interrupts::TIMER_HZ) / 1000
}

impl KSys {
    fn new(uid: u32, snap: Snap) -> Self {
        Self {
            uid,
            snap,
            kills: Vec::new(),
        }
    }

    /// Wait `ms`, waking early on Ctrl+C. Returns `false` when interrupted.
    fn wait(&self, ms: u64) -> bool {
        let until = crate::interrupts::ticks() + ticks_of(ms);
        while crate::interrupts::ticks() < until {
            if self.interrupted() {
                return false;
            }
            crate::sched::block(until.min(crate::interrupts::ticks() + 5), || {
                !self.interrupted()
            });
        }
        true
    }

    /// An address from a dotted-quad literal or through DNS.
    fn address(&mut self, host: &str) -> Result<[u8; 4], SysErr> {
        if let Some(ip) = kitsune_core::shell::netcmds::parse_ipv4(host) {
            return Ok(ip);
        }
        let uid = self.uid;
        match crate::fetch::run_job(crate::fetch::NetJob::Resolve(host.to_string()), || {
            CANCEL.load(Ordering::Relaxed) == uid
        }) {
            Ok(crate::fetch::NetJobResult::Addr(Some(a))) => Ok(a),
            Ok(crate::fetch::NetJobResult::Addr(None)) => Err(SysErr::HostNotFound),
            Ok(_) => Err(SysErr::Failed),
            Err(e) => Err(job_err(e)),
        }
    }
}

fn job_err(e: crate::fetch::JobError) -> SysErr {
    use crate::fetch::JobError as J;
    match e {
        J::NoNetwork => SysErr::Network,
        J::Busy | J::Timeout => SysErr::Timeout,
        J::Cancelled => SysErr::Cancelled,
    }
}

fn fail_err(r: kitsune_core::browser::FailReason) -> SysErr {
    use kitsune_core::browser::FailReason as F;
    match r {
        F::Dns => SysErr::HostNotFound,
        F::Timeout => SysErr::Timeout,
        F::Network | F::Refused | F::WorkerDied => SysErr::Network,
        _ => SysErr::Failed,
    }
}

impl SysInfo for KSys {
    fn now(&self) -> DateTime {
        // The SNTP-corrected clock when the network confirmed it, else the RTC value read at
        // boot and advanced by the timer; shifted to the zone the settings app chose. (The RTC
        // itself is not read here: its ports belong to the compositor thread.)
        let secs = crate::clock::trusted_unix_secs()
            .or_else(|| crate::clock::local_unix_ms().map(|ms| ms / 1000))
            .unwrap_or(0);
        let local = secs as i64 + i64::from(crate::rtc::tz_minutes()) * 60;
        let d = kitsune_core::hw::rtc::DateTime::from_epoch(local);
        DateTime {
            year: d.date.y,
            month: d.date.m,
            day: d.date.d,
            hour: d.time.h,
            minute: d.time.m,
            second: d.time.s,
        }
    }

    fn uptime_ms(&self) -> u64 {
        crate::netd::now_ms()
    }

    fn mem(&self) -> MemInfo {
        self.snap.mem
    }

    fn disks(&self) -> Vec<DiskInfo> {
        let u = vfs::statfs();
        let on_disk = vfs::volume() == vfs::Volume::Disk;
        let mut v = alloc::vec![DiskInfo {
            name: String::from(if on_disk { "ojfs3" } else { "ramfs" }),
            mount: String::from("/"),
            total: u.total,
            used: u.used(),
        }];
        // The raw drives: a drive that is in use as the volume above is not listed twice.
        for (i, d) in self.snap.disks.iter().enumerate() {
            let Some(d) = d else { continue };
            if i == 1 && on_disk {
                continue;
            }
            let total = d.sectors.saturating_mul(512);
            v.push(DiskInfo {
                name: alloc::format!("hd{}", (b'a' + i as u8) as char),
                mount: alloc::format!(
                    "({})",
                    if i == 0 {
                        t!("sh.df.boot_disk")
                    } else {
                        t!("sh.df.unused")
                    }
                ),
                total,
                used: if i == 0 { total } else { 0 },
            });
        }
        v
    }

    fn procs(&self) -> Vec<ProcInfo> {
        self.snap.procs.iter().map(|(p, _)| p.clone()).collect()
    }

    fn kill(&mut self, pid: u32, signal: i32) -> Result<(), SysErr> {
        match self.snap.procs.iter().find(|(p, _)| p.pid == pid) {
            None => Err(SysErr::NoSuchProcess),
            Some((_, false)) => Err(SysErr::Denied),
            Some(_) => {
                // Signal 0 only asks whether the process exists.
                if signal != 0 {
                    self.kills.push((pid, signal));
                }
                Ok(())
            }
        }
    }

    fn ping(&mut self, host: &str, count: u32) -> Result<PingStats, SysErr> {
        use kitsune_core::icmp::PingError;
        let ip = self.address(host)?;
        let target = kitsune_core::net::Ipv4(ip);
        let mut stats = PingStats::default();
        let mut total_us = 0u64;
        let mut hard_fail = 0u32;
        for i in 0..count {
            if i > 0 && !self.wait(1000) {
                return Err(SysErr::Cancelled);
            }
            if self.interrupted() {
                return Err(SysErr::Cancelled);
            }
            stats.sent += 1;
            // A page load in progress keeps the NIC busy: wait for it (bounded).
            let mut tries = 0;
            let r = loop {
                match crate::netd::ping_us(target, 1000) {
                    Err(PingError::Busy) if tries < 100 && !self.interrupted() => {
                        tries += 1;
                        if !self.wait(100) {
                            return Err(SysErr::Cancelled);
                        }
                    }
                    other => break other,
                }
            };
            match r {
                Ok(us) => {
                    stats.received += 1;
                    total_us = total_us.saturating_add(us);
                }
                Err(PingError::NoNetwork | PingError::NoRoute | PingError::BadTarget) => {
                    hard_fail += 1;
                }
                // Timeout, unreachable, TTL exceeded, ARP: this echo got no reply.
                Err(_) => {}
            }
        }
        if stats.received == 0 && hard_fail == stats.sent {
            return Err(SysErr::Network);
        }
        stats.avg_rtt_us = total_us.checked_div(u64::from(stats.received)).unwrap_or(0);
        Ok(stats)
    }

    fn resolve(&mut self, host: &str) -> Result<Vec<[u8; 4]>, SysErr> {
        self.address(host).map(|a| alloc::vec![a])
    }

    fn http_get(&mut self, url: &str, max_body: usize) -> Result<HttpResponse, SysErr> {
        use crate::fetch::{NetJob, NetJobResult};
        let uid = self.uid;
        let r = crate::fetch::run_job(NetJob::Get(url.to_string()), || {
            CANCEL.load(Ordering::Relaxed) == uid
        })
        .map_err(job_err)?;
        let page = match r {
            NetJobResult::Page(p) => p,
            NetJobResult::Failed(f) => return Err(fail_err(f)),
            NetJobResult::Addr(_) => return Err(SysErr::Failed),
        };
        let data = page.data;
        let split = data
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map_or(data.len(), |i| i + 4);
        let decoded = kitsune_core::browser::page_body_partial(&data, page.truncated);
        let mut body = decoded.body;
        let mut truncated = page.truncated || decoded.note.is_some();
        if body.len() > max_body {
            body.truncate(max_body);
            truncated = true;
        }
        Ok(HttpResponse {
            status: kitsune_core::browser::status_code(&data).unwrap_or(0),
            head: data[..split].to_vec(),
            body,
            truncated,
        })
    }

    fn net_info(&self) -> Option<NetInfo> {
        let s = crate::netd::stats();
        let cfg = s.config;
        Some(NetInfo {
            nic: String::from(s.nic.name()),
            link_up: s.link_up,
            ip: cfg.map(|c| c.ip.to_string()),
            prefix: cfg.map_or(0, |c| c.prefix),
            gateway: cfg.and_then(|c| c.gateway).map(|g| g.to_string()),
            dns: cfg.map_or_else(Vec::new, |c| {
                c.dns.as_slice().iter().map(|d| d.to_string()).collect()
            }),
            dhcp: String::from(s.dhcp_state),
            rx_packets: s.rx_packets,
            rx_bytes: s.rx_bytes,
            tx_packets: s.tx_packets,
            tx_bytes: s.tx_bytes,
        })
    }

    fn interrupted(&self) -> bool {
        CANCEL.load(Ordering::Relaxed) == self.uid
    }

    fn sleep_ms(&mut self, ms: u64) {
        self.wait(ms);
    }

    fn hostname(&self) -> String {
        String::from("kitsune")
    }
}

// ---- commands that act on the desktop ----------------------------------------

/// A request from a command to the compositor.
pub(crate) enum UiReq {
    /// `edit [FILE]`: open an editor (on `FILE`, created on save when new).
    Edit(Option<String>),
    /// `files [DIR]`: open a file manager.
    Files,
    Tasks,
    Calc,
    Reboot,
    Shutdown,
}

static UI: YieldMutex<Vec<UiReq>> = YieldMutex::new(Vec::new());

fn post_ui(r: UiReq) {
    if let Ok(mut q) = UI.lock()
        && q.len() < 16
    {
        q.push(r);
    }
}

/// Requests waiting for the compositor.
pub(crate) fn take_ui() -> Vec<UiReq> {
    match UI.lock() {
        Ok(mut q) if !q.is_empty() => core::mem::take(&mut *q),
        _ => Vec::new(),
    }
}

fn cmd_edit(cx: &mut CmdCtx<'_>) -> i32 {
    match cx.args.len() {
        1 => post_ui(UiReq::Edit(None)),
        2 => {
            let p = cx.fs.resolve(&cx.args[1]);
            if let Ok(st) = cx.fs.stat(&p)
                && st.kind == FsKind::Dir
            {
                cx.error(&alloc::format!("{p}: {}", FsErr::IsADirectory.message()));
                return 1;
            }
            post_ui(UiReq::Edit(Some(p)));
        }
        _ => {
            cx.error(t!("sh.edit.usage"));
            return 2;
        }
    }
    0
}

fn cmd_files(_: &mut CmdCtx<'_>) -> i32 {
    post_ui(UiReq::Files);
    0
}

fn cmd_tasks(_: &mut CmdCtx<'_>) -> i32 {
    post_ui(UiReq::Tasks);
    0
}

fn cmd_calc(_: &mut CmdCtx<'_>) -> i32 {
    post_ui(UiReq::Calc);
    0
}

fn cmd_reboot(_: &mut CmdCtx<'_>) -> i32 {
    post_ui(UiReq::Reboot);
    0
}

fn cmd_shutdown(_: &mut CmdCtx<'_>) -> i32 {
    post_ui(UiReq::Shutdown);
    0
}

/// A shell with the desktop's own commands added to the standard ones.
pub(crate) fn new_shell() -> Shell {
    let mut sh = Shell::new();
    // The kernel thread has a 128 KiB stack; keep recursion well under the host-tested worst case.
    sh.limits.max_call_depth = 16;
    sh.limits.max_sub_depth = 4;
    sh.register("edit", tk!("sh.edit.help"), cmd_edit);
    sh.register("files", tk!("sh.files.help"), cmd_files);
    sh.register("tasks", tk!("sh.tasks.help"), cmd_tasks);
    sh.register("calc", tk!("sh.calc.help"), cmd_calc);
    sh.register("reboot", tk!("sh.reboot.help"), cmd_reboot);
    sh.register("shutdown", tk!("sh.shutdown.help"), cmd_shutdown);
    sh.env.set("PS1", "\\w\\$ ");
    sh.env.set("HOME", "/");
    sh
}

// ---- the command thread --------------------------------------------------------

/// A terminal's shell and filesystem view; moves to the worker with each job.
pub(crate) struct Ctx {
    pub shell: Shell,
    pub fs: VfsFs,
}

impl Ctx {
    pub(crate) fn new() -> Box<Ctx> {
        Box::new(Ctx {
            shell: new_shell(),
            fs: VfsFs::new(),
        })
    }
}

/// A command line to run.
pub(crate) struct Job {
    pub uid: u32,
    pub ctx: Box<Ctx>,
    pub line: String,
    pub snap: Snap,
}

/// A finished command line.
pub(crate) struct Done {
    pub uid: u32,
    pub ctx: Box<Ctx>,
    pub result: RunResult,
    /// The prompt for the next line (the working directory may have changed).
    pub prompt: String,
    /// `kill` requests to carry out.
    pub kills: Vec<(u32, i32)>,
}

static QUEUE: YieldMutex<VecDeque<Job>> = YieldMutex::new(VecDeque::new());
static FINISHED: YieldMutex<Vec<Done>> = YieldMutex::new(Vec::new());
/// Jobs in `QUEUE` (an atomic so the scheduler's idle test needs no lock).
static PENDING: AtomicUsize = AtomicUsize::new(0);
/// Terminal whose running command was asked to stop (0 = none).
static CANCEL: AtomicU32 = AtomicU32::new(0);
/// Scheduler slots of the workers (`usize::MAX` until each starts). Two threads, so a command
/// that waits (`sleep`, `ping`, `curl`) does not hold up another terminal's `ls`.
static TIDS: [AtomicUsize; WORKERS] = [const { AtomicUsize::new(usize::MAX) }; WORKERS];
/// How many `shelld` threads run command lines.
const WORKERS: usize = 2;

/// Wake every worker (a job arrived or a command was cancelled).
fn wake_workers() {
    for t in &TIDS {
        let tid = t.load(Ordering::Acquire);
        if tid != usize::MAX {
            crate::sched::wake(tid);
        }
    }
}

/// Most command lines waiting at once.
const QUEUE_MAX: usize = 16;

/// True once the worker thread has died: nothing will answer any more.
pub(crate) fn worker_dead() -> bool {
    TIDS.iter().all(|t| {
        let tid = t.load(Ordering::Acquire);
        tid != usize::MAX && crate::sched::is_dead(tid)
    })
}

/// Post a command line. The job comes back unchanged when the queue is full.
pub(crate) fn post(job: Job) -> Result<(), Box<Job>> {
    let Ok(mut q) = QUEUE.lock() else {
        return Err(Box::new(job));
    };
    if q.len() >= QUEUE_MAX {
        return Err(Box::new(job));
    }
    q.push_back(job);
    PENDING.fetch_add(1, Ordering::AcqRel);
    drop(q);
    wake_workers();
    Ok(())
}

/// Ctrl+C in terminal `uid`: drop its queued job, or stop the one running.
pub(crate) fn cancel(uid: u32) {
    if let Ok(mut q) = QUEUE.lock()
        && let Some(i) = q.iter().position(|j| j.uid == uid)
        && let Some(job) = q.remove(i)
    {
        PENDING.fetch_sub(1, Ordering::AcqRel);
        drop(q);
        let prompt = prompt_of(&job.ctx);
        finish(
            job.uid,
            job.ctx,
            RunResult {
                status: 130,
                output: alloc::format!("sh: {}\n", t!("sh.sys.interrupted")).into_bytes(),
                ..RunResult::default()
            },
            prompt,
            Vec::new(),
        );
        return;
    }
    CANCEL.store(uid, Ordering::Relaxed);
    wake_workers();
}

fn finish(uid: u32, ctx: Box<Ctx>, result: RunResult, prompt: String, kills: Vec<(u32, i32)>) {
    // Never drop a result: keep trying while the compositor holds the list for a moment.
    let mut done = Some(Done {
        uid,
        ctx,
        result,
        prompt,
        kills,
    });
    for _ in 0..1000 {
        if let Ok(mut f) = FINISHED.lock() {
            if let Some(d) = done.take() {
                f.push(d);
            }
            break;
        }
        crate::sched::yield_now();
    }
}

/// Finished command lines, oldest first.
pub(crate) fn take_done() -> Vec<Done> {
    match FINISHED.lock() {
        Ok(mut f) if !f.is_empty() => core::mem::take(&mut *f),
        _ => Vec::new(),
    }
}

fn run_job(job: Job) {
    let Job {
        uid,
        mut ctx,
        line,
        snap,
    } = job;
    // A Ctrl+C meant for an earlier command (it ended just before the key) must not stop this one.
    let _ = CANCEL.compare_exchange(uid, 0, Ordering::AcqRel, Ordering::Relaxed);
    let mut sys = KSys::new(uid, snap);
    let result = {
        let Ctx { shell, fs } = &mut *ctx;
        let mut host = Host { fs, sys: &mut sys };
        shell.run_line(&line, &mut host)
    };
    let prompt = ctx.shell.prompt(&ctx.fs, &sys);
    // A Ctrl+C that arrived after the command ended must not hit the next one.
    let _ = CANCEL.compare_exchange(uid, 0, Ordering::AcqRel, Ordering::Relaxed);
    finish(uid, ctx, result, prompt, core::mem::take(&mut sys.kills));
}

/// Thread entries: run posted command lines, one at a time each; parked while idle.
pub extern "C" fn worker() -> ! {
    worker_loop(0)
}

pub extern "C" fn worker2() -> ! {
    worker_loop(1)
}

fn worker_loop(slot: usize) -> ! {
    TIDS[slot].store(crate::sched::current(), Ordering::Release);
    loop {
        let job = match QUEUE.lock() {
            Ok(mut q) => q.pop_front(),
            Err(_) => None,
        };
        match job {
            Some(job) => {
                PENDING.fetch_sub(1, Ordering::AcqRel);
                run_job(job);
            }
            None => {
                crate::sched::block(crate::sched::FOREVER, || {
                    PENDING.load(Ordering::Acquire) == 0
                });
            }
        }
    }
}

/// The prompt `ctx` would show now (no system facts needed beyond the host name).
pub(crate) fn prompt_of(ctx: &Ctx) -> String {
    let sys = KSys::new(
        0,
        Snap {
            procs: Vec::new(),
            mem: MemInfo::default(),
            disks: [None, None],
        },
    );
    ctx.shell.prompt(&ctx.fs, &sys)
}
